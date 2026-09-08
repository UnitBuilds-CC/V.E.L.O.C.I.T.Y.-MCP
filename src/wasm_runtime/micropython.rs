//! MicroPython/WASM runtime — Python tool execution via MicroPython compiled to WASM.
//!
//! Uses a WASI reactor build of MicroPython v1.24.1 running inside Wasmer.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{WasiEnv, build_wasi_imports};
use super::WasmRuntime;

const PYSTACK_SIZE: i32 = 16384;
const HEAP_SIZE: i32 = 256 * 1024;

pub struct MicroPythonRuntime {
    store: Store,
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    next_offset: u64,
    tools: HashMap<String, String>,
}

impl MicroPythonRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &imports)?;

        let memory = instance.exports.get_memory("memory")?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());

        Ok(Self {
            store,
            instance,
            memory,
            env,
            next_offset: 64 * 1024,
            tools: HashMap::new(),
        })
    }

    pub fn cold_start(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut rt = Self::new(wasm_bytes)?;
        rt.init()?;
        Ok(rt)
    }

    fn write_to_memory(&mut self, data: &[u8]) -> Result<(i32, i32), Box<dyn Error>> {
        let ptr = self.next_offset as i32;
        let len = data.len() as i32;
        self.memory.view(&self.store).write(self.next_offset, data)?;
        self.next_offset += data.len() as u64 + 64;
        Ok((ptr, len))
    }

    fn exec(&mut self, src: &str) -> Result<i32, Box<dyn Error>> {
        let (ptr, len) = self.write_to_memory(src.as_bytes())?;
        let exec_fn = self.instance.exports.get_function("mp_wasi_exec")?;
        let result = exec_fn.call(&mut self.store, &[Value::I32(ptr), Value::I32(len)])?;
        Ok(result[0].unwrap_i32())
    }

    fn get_output(&mut self) -> Result<String, Box<dyn Error>> {
        let get_output_fn = self.instance.exports.get_function("mp_wasi_get_output")?;
        let get_len_fn = self.instance.exports.get_function("mp_wasi_get_output_len")?;

        let out_ptr = get_output_fn.call(&mut self.store, &[])?[0].unwrap_i32();
        let out_len = get_len_fn.call(&mut self.store, &[])?[0].unwrap_i32();

        if out_ptr == 0 || out_len == 0 {
            return Ok(String::new());
        }

        let mut buf = vec![0u8; out_len as usize];
        self.memory.view(&self.store).read(out_ptr as u64, &mut buf)?;
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
            let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
            let imports = build_wasi_imports(&mut store, &env);
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            let memory = instance.exports.get_memory("memory").unwrap().clone();
            env.as_mut(&mut store).memory = Some(memory);
            let init_fn = instance.exports.get_function("mp_wasi_init")
                .unwrap().typed::<(i32, i32), i32>(&store).unwrap();
            let _ = init_fn.call(&mut store, PYSTACK_SIZE, HEAP_SIZE).unwrap();
        }
        start.elapsed().as_nanos() as f64 / cold_iters as f64 / 1_000_000.0
    }

    /// Write source to a fixed slot and call exec+get_output `iters` times.
    /// Returns (ns_per_call, checksum).
    pub fn bench_exec_repeated(&mut self, src: &str, iters: usize) -> (f64, u32) {
        let data = src.as_bytes();
        let slot = 64 * 1024;
        self.memory.view(&self.store).write(slot as u64, data).unwrap();
        let ptr = slot as i32;
        let len = data.len() as i32;

        let exec_fn = self.instance.exports.get_function("mp_wasi_exec").unwrap();
        let get_len_fn = self.instance.exports.get_function("mp_wasi_get_output_len").unwrap();

        let start = std::time::Instant::now();
        let mut checksum: u32 = 0;
        for _ in 0..iters {
            let rc = exec_fn.call(&mut self.store, &[Value::I32(ptr), Value::I32(len)]).unwrap();
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
        let init_fn = self.instance.exports.get_function("mp_wasi_init")?
            .typed::<(i32, i32), i32>(&self.store)?;
        let result = init_fn.call(&mut self.store, PYSTACK_SIZE, HEAP_SIZE)?;
        if result != 0 {
            return Err(format!("mp_wasi_init() failed with code {}", result).into());
        }
        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        self.exec_and_get_output(source)?;
        self.tools.insert(name.to_string(), name.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Python tool: {}", name).into());
        }

        let escaped = args_json
            .replace('\\', "\\\\")
            .replace('\'', "\\'")
            .replace('\n', "\\n")
            .replace('\r', "\\r");

        let wrapper = format!(
            "import json\n\
             _args = json.loads('{}')\n\
             _result = {}(_args)\n\
             print(json.dumps(_result))",
            escaped, name
        );

        let output = self.exec_and_get_output(&wrapper)?;
        Ok(output.trim().to_string())
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        let destroy_fn = self.instance.exports.get_function("mp_wasi_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
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
        let wasm = std::fs::read(wasm_path()).expect("MicroPython WASM not found — run build.sh first");
        let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

        let output = rt.exec_and_get_output("print('hello from micropython')").expect("exec failed");
        assert_eq!(output.trim(), "hello from micropython");

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_micropython_tool_roundtrip() {
        let wasm = std::fs::read(wasm_path()).expect("MicroPython WASM not found");
        let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

        let tool_source = "def greet(args):\n    return {'message': 'Hello, ' + args.get('name', 'world') + '!'}\n";
        rt.register_tool("greet", tool_source).expect("register failed");

        let result = rt.call_tool("greet", r#"{"name": "Wasmer"}"#).expect("call failed");
        assert!(result.contains("Hello, Wasmer!"), "unexpected result: {}", result);

        rt.destroy().unwrap();
    }
}
