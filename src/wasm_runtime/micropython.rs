//! MicroPython/WASM runtime — Python tool execution via MicroPython compiled to WASM.
//!
//! Uses a WASI reactor build of MicroPython v1.24.1 running inside Wasmer.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{build_wasi_imports, WasiEnv};
use super::WasmRuntime;

const PYSTACK_SIZE: i32 = 16384;
const HEAP_SIZE: i32 = 256 * 1024;

const EXEC_SLOT: u64 = 512 * 1024; // 512KB - for source code
const ARGS_SLOT: u64 = 4 * 1024; // 4KB - for tool args JSON
const NAME_SLOT: u64 = 8 * 1024; // 8KB - for tool name

pub struct MicroPythonRuntime {
    store: Store,
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    tools: HashMap<String, String>,
    call_tool_fn: Option<Function>,
    call_tool_binary_fn: Option<Function>,
}

impl MicroPythonRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = super::build_metered_engine(super::instruction_limit());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let env = FunctionEnv::new(&mut store, WasiEnv::new());
        let imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &imports)?;

        let memory = instance.exports.get_memory("memory")?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());

        let current_pages = memory.view(&store).size();
        let needed_pages = ((EXEC_SLOT + 64 * 1024) / 65536 + 1) as u32;
        if current_pages.0 < needed_pages {
            memory.grow(&mut store, wasmer::Pages(needed_pages - current_pages.0))?;
        }

        Ok(Self {
            store,
            instance,
            memory,
            env,
            tools: HashMap::new(),
            call_tool_fn: None,
            call_tool_binary_fn: None,
        })
    }

    pub fn cold_start(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut rt = Self::new(wasm_bytes)?;
        rt.init()?;
        Ok(rt)
    }

    fn exec(&mut self, src: &str) -> Result<i32, Box<dyn Error>> {
        let data = src.as_bytes();
        self.memory.view(&self.store).write(EXEC_SLOT, data)?;
        let exec_fn = self.instance.exports.get_function("mp_wasi_exec")?;
        let result = exec_fn.call(
            &mut self.store,
            &[Value::I32(EXEC_SLOT as i32), Value::I32(data.len() as i32)],
        )?;
        Ok(result[0].unwrap_i32())
    }

    fn get_output(&mut self) -> Result<String, Box<dyn Error>> {
        let get_output_fn = self.instance.exports.get_function("mp_wasi_get_output")?;
        let get_len_fn = self
            .instance
            .exports
            .get_function("mp_wasi_get_output_len")?;

        let out_ptr = get_output_fn.call(&mut self.store, &[])?[0].unwrap_i32();
        let out_len = get_len_fn.call(&mut self.store, &[])?[0].unwrap_i32();

        if out_ptr == 0 || out_len == 0 {
            return Ok(String::new());
        }

        let mut buf = vec![0u8; out_len as usize];
        self.memory
            .view(&self.store)
            .read(out_ptr as u64, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    pub fn exec_and_get_output(&mut self, src: &str) -> Result<String, Box<dyn Error>> {
        let rc = self.exec(src)?;
        let output = self.get_output()?;
        if rc != 0 {
            return Err(format!("Python error: {}", output.trim()).into());
        }
        Ok(output)
    }

    pub fn bench_cold_start(wasm_bytes: &[u8]) -> f64 {
        let cold_iters = 20;
        let start = std::time::Instant::now();
        for _ in 0..cold_iters {
            let engine = wasmer::Engine::from(wasmer::Cranelift::default());
            let module = Module::new(&engine, wasm_bytes).unwrap();
            let mut store = Store::new(engine);
            let env = FunctionEnv::new(&mut store, WasiEnv::new());
            let imports = build_wasi_imports(&mut store, &env);
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            let memory = instance.exports.get_memory("memory").unwrap().clone();
            env.as_mut(&mut store).memory = Some(memory);
            let init_fn = instance
                .exports
                .get_function("mp_wasi_init")
                .unwrap()
                .typed::<(i32, i32), i32>(&store)
                .unwrap();
            let _ = init_fn.call(&mut store, PYSTACK_SIZE, HEAP_SIZE).unwrap();
        }
        start.elapsed().as_nanos() as f64 / cold_iters as f64 / 1_000_000.0
    }

    /// Write source to a fixed slot and call exec+get_output `iters` times.
    /// Returns (ns_per_call, checksum).
    pub fn bench_exec_repeated(&mut self, src: &str, iters: usize) -> (f64, u32) {
        let data = src.as_bytes();
        self.memory
            .view(&self.store)
            .write(EXEC_SLOT, data)
            .unwrap();
        let ptr = EXEC_SLOT as i32;
        let len = data.len() as i32;

        let exec_fn = self.instance.exports.get_function("mp_wasi_exec").unwrap();
        let get_len_fn = self
            .instance
            .exports
            .get_function("mp_wasi_get_output_len")
            .unwrap();

        let start = std::time::Instant::now();
        let mut checksum: u32 = 0;
        for _ in 0..iters {
            let rc = exec_fn
                .call(&mut self.store, &[Value::I32(ptr), Value::I32(len)])
                .unwrap();
            if rc[0].unwrap_i32() == 0 {
                let out_len = get_len_fn.call(&mut self.store, &[]).unwrap()[0].unwrap_i32();
                checksum = checksum.wrapping_add(out_len as u32);
            }
        }
        let ns = start.elapsed().as_nanos() as f64 / iters as f64;
        (ns, checksum)
    }
}

impl WasmRuntime for MicroPythonRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let init_fn = self
            .instance
            .exports
            .get_function("mp_wasi_init")?
            .typed::<(i32, i32), i32>(&self.store)?;
        let result = init_fn.call(&mut self.store, PYSTACK_SIZE, HEAP_SIZE)?;
        if result != 0 {
            return Err(format!("mp_wasi_init() failed with code {}", result).into());
        }
        self.call_tool_fn = Some(
            self.instance
                .exports
                .get_function("mp_wasi_call_tool")?
                .clone(),
        );
        // Load binary protocol function if available (optional, for optimized path)
        self.call_tool_binary_fn = self
            .instance
            .exports
            .get_function("mp_wasi_call_tool_binary")
            .ok()
            .cloned();
        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // Check if source changed (skip re-execution if unchanged)
        if let Some(existing_source) = self.tools.get(name) {
            if existing_source == source {
                return Ok(()); // No change, skip re-execution
            }
        }

        // Execute the tool source to define the function
        self.exec_and_get_output(source)?;

        // Build wrapper that uses pre-compiled protocol
        let wrapper = format!(
            "_args = get_tool_args()\n\
             _result = {}(_args)\n\
             set_tool_result(_result)",
            name
        );

        // Write wrapper source to EXEC_SLOT
        let wrapper_bytes = wrapper.as_bytes();
        self.memory
            .view(&self.store)
            .write(EXEC_SLOT, wrapper_bytes)?;

        // Write tool name to NAME_SLOT
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        // Call mp_wasi_register_tool(name_ptr, name_len, src_ptr, src_len)
        let register_fn = self
            .instance
            .exports
            .get_function("mp_wasi_register_tool")?;
        let result = register_fn.call(
            &mut self.store,
            &[
                Value::I32(NAME_SLOT as i32),
                Value::I32(name_bytes.len() as i32),
                Value::I32(EXEC_SLOT as i32),
                Value::I32(wrapper_bytes.len() as i32),
            ],
        )?;

        if result[0].unwrap_i32() != 0 {
            let output = self.get_output()?;
            return Err(format!("Failed to register tool: {}", output.trim()).into());
        }

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Python tool: {}", name).into());
        }

        // Write args to ARGS_SLOT and tool name to NAME_SLOT
        let args_bytes = args_json.as_bytes();
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(ARGS_SLOT, args_bytes)?;
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        // Single WASI call: mp_wasi_call_tool(args_ptr, args_len, name_ptr, name_len)
        let call_fn = self.call_tool_fn.as_ref().unwrap();
        let result = call_fn.call(
            &mut self.store,
            &[
                Value::I32(ARGS_SLOT as i32),
                Value::I32(args_bytes.len() as i32),
                Value::I32(NAME_SLOT as i32),
                Value::I32(name_bytes.len() as i32),
            ],
        )?;

        let rc = result[0].unwrap_i32();
        let output = self.get_output()?;

        if rc != 0 {
            return Err(format!("Python tool error: {}", output.trim()).into());
        }

        Ok(output.trim().to_string())
    }

    fn call_tool_binary(&mut self, name: &str, args_tlv: &[u8]) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Python tool: {}", name).into());
        }

        // If binary function not available, fall back to JSON path
        let call_fn = match &self.call_tool_binary_fn {
            Some(f) => f,
            None => {
                use crate::protocol::nda_native::decode_json_value;
                let (value, _) = decode_json_value(args_tlv)?;
                let json_str = serde_json::to_string(&value)?;
                return self.call_tool(name, &json_str);
            }
        };

        // Write TLV bytes to ARGS_SLOT and tool name to NAME_SLOT
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(ARGS_SLOT, args_tlv)?;
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        // Single WASI call: mp_wasi_call_tool_binary(tlv_ptr, tlv_len, name_ptr, name_len)
        let result = call_fn.call(
            &mut self.store,
            &[
                Value::I32(ARGS_SLOT as i32),
                Value::I32(args_tlv.len() as i32),
                Value::I32(NAME_SLOT as i32),
                Value::I32(name_bytes.len() as i32),
            ],
        )?;

        let rc = result[0].unwrap_i32();
        let output = self.get_output()?;

        if rc != 0 {
            return Err(format!("Python tool error: {}", output.trim()).into());
        }

        Ok(output.trim().to_string())
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        let destroy_fn = self.instance.exports.get_function("mp_wasi_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
    }

    fn reset_instruction_budget(&mut self) {
        super::reset_instruction_budget(&mut self.store, &self.instance);
    }

    fn language(&self) -> &str {
        "python"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/micropython_wasm/micropython-1.24.1/ports/webassembly/build-wasi/micropython.wasm");
        p
    }

    #[test]
    #[ignore] // requires MicroPython WASM binary to be built first
    fn test_micropython_basic_exec() {
        let wasm =
            std::fs::read(wasm_path()).expect("MicroPython WASM not found — run build.sh first");
        let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

        let output = rt
            .exec_and_get_output("print('hello from micropython')")
            .expect("exec failed");
        assert_eq!(output.trim(), "hello from micropython");

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_micropython_tool_roundtrip() {
        let wasm = std::fs::read(wasm_path()).expect("MicroPython WASM not found");
        let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

        let tool_source = "def greet(args):\n    return {'message': 'Hello, ' + args.get('name', 'world') + '!'}\n";
        rt.register_tool("greet", tool_source)
            .expect("register failed");

        let result = rt
            .call_tool("greet", r#"{"name": "Wasmer"}"#)
            .expect("call failed");
        assert!(
            result.contains("Hello, Wasmer!"),
            "unexpected result: {}",
            result
        );

        rt.destroy().unwrap();
    }

    #[test]
    fn test_micropython_precompiled_tool() {
        let wasm = std::fs::read(wasm_path()).expect("MicroPython WASM not found");
        let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

        let source = "def add_numbers(args):\n    return {'sum': args['a'] + args['b']}\n";
        rt.register_tool("add_numbers", source)
            .expect("register failed");

        let result = rt
            .call_tool("add_numbers", r#"{"a": 3, "b": 4}"#)
            .expect("call failed");
        assert!(result.contains("7"), "Expected 7 in result: {}", result);

        let result2 = rt
            .call_tool("add_numbers", r#"{"a": 10, "b": 20}"#)
            .expect("call failed");
        assert!(result2.contains("30"), "Expected 30 in result: {}", result2);

        let result3 = rt
            .call_tool("add_numbers", r#"{"a": 100, "b": 200}"#)
            .expect("call 3 failed");
        assert!(
            result3.contains("300"),
            "Expected 300 in result: {}",
            result3
        );

        let result4 = rt
            .call_tool("add_numbers", r#"{"a": 1000, "b": 2000}"#)
            .expect("call 4 failed");
        assert!(
            result4.contains("3000"),
            "Expected 3000 in result: {}",
            result4
        );

        let result5 = rt
            .call_tool("add_numbers", r#"{"a": 5, "b": 5}"#)
            .expect("call 5 failed");
        assert!(result5.contains("10"), "Expected 10 in result: {}", result5);

        rt.destroy().unwrap();
    }

    #[test]
    fn test_micropython_gc_stress() {
        let wasm = std::fs::read(wasm_path()).expect("MicroPython WASM not found");
        let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

        let source = "def bench_tool(args):\n    return {'size': 64, 'payload': 'hello'}\n";
        rt.register_tool("bench_tool", source)
            .expect("register failed");

        for i in 0..100 {
            let result = rt
                .call_tool("bench_tool", r#"{"text": "hello"}"#)
                .unwrap_or_else(|_| panic!("call {} failed", i));
            assert!(
                result.contains("hello"),
                "call {} missing 'hello': {}",
                i,
                result
            );
        }

        rt.destroy().unwrap();
    }
}
