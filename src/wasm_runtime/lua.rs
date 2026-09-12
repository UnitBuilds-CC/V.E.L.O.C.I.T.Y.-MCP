//! Lua/WASM runtime — Lua tool execution via Lua 5.4 compiled to WASM.
//!
//! Uses a WASI reactor build of Lua 5.4.7 running inside Wasmer.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{WasiEnv, build_wasi_imports};
use super::{WasmRuntime, WasmExecHelper, create_wasm_instance, WasmRuntimeConfig, memory_layout};

pub struct LuaRuntime {
    store: Store,
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    tools: HashMap<String, String>,
    call_tool_fn: Option<Function>,
    call_tool_binary_fn: Option<Function>,
}

// Re-export shared constants for backwards compatibility with existing code
use memory_layout::{EXEC_SLOT, ARGS_SLOT, NAME_SLOT};

impl LuaRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let (store, instance, memory, env) = create_wasm_instance(WasmRuntimeConfig {
            wasm_bytes,
            import_builder: Box::new(|store, env| build_wasi_imports(store, env)),
            extra_memory_pages: 1, // 64KB beyond EXEC_SLOT
            instruction_limit: None, // No metering by default
        })?;

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
        let mut helper = WasmExecHelper::new(&mut self.store, &self.instance, &self.memory);
        helper.exec(src, "lua_wasi_exec")
    }

    fn get_output(&mut self) -> Result<String, Box<dyn Error>> {
        let mut helper = WasmExecHelper::new(&mut self.store, &self.instance, &self.memory);
        helper.get_output("lua_wasi_get_output", "lua_wasi_get_output_len")
    }

    pub fn exec_and_get_output(&mut self, src: &str) -> Result<String, Box<dyn Error>> {
        let rc = self.exec(src)?;
        let output = self.get_output()?;
        if rc != 0 {
            return Err(format!("Lua error: {}", output.trim()).into());
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
            let init_fn = instance.exports.get_function("lua_wasi_init")
                .unwrap().typed::<(), i32>(&store).unwrap();
            let _ = init_fn.call(&mut store).unwrap();
        }
        start.elapsed().as_nanos() as f64 / cold_iters as f64 / 1_000_000.0
    }

    /// Write source to a fixed slot and call exec+get_output_len `iters` times.
    /// Returns (ns_per_call, checksum).
    pub fn bench_exec_repeated(&mut self, src: &str, iters: usize) -> (f64, u32) {
        let data = src.as_bytes();
        self.memory.view(&self.store).write(EXEC_SLOT, data).unwrap();
        let ptr = EXEC_SLOT as i32;
        let len = data.len() as i32;

        let exec_fn = self.instance.exports.get_function("lua_wasi_exec").unwrap();
        let get_len_fn = self.instance.exports.get_function("lua_wasi_get_output_len").unwrap();

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

impl WasmRuntime for LuaRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let init_fn = self.instance.exports.get_function("lua_wasi_init")?
            .typed::<(), i32>(&self.store)?;
        let result = init_fn.call(&mut self.store)?;
        if result != 0 {
            return Err(format!("lua_wasi_init() failed with code {}", result).into());
        }
        self.call_tool_fn = Some(self.instance.exports.get_function("lua_wasi_call_tool")?.clone());
        // Load binary protocol function if available (optional, for optimized path)
        self.call_tool_binary_fn = self.instance.exports.get_function("lua_wasi_call_tool_binary").ok().cloned();
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
            "local _args = get_tool_args()\n\
             local _result = {}(_args)\n\
             set_tool_result(_result)",
            name
        );

        // Write wrapper source to EXEC_SLOT
        let wrapper_bytes = wrapper.as_bytes();
        self.memory.view(&self.store).write(EXEC_SLOT, wrapper_bytes)?;

        // Write tool name to NAME_SLOT
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        // Call lua_wasi_register_wrapper(name_ptr, name_len, src_ptr, src_len)
        let register_fn = self.instance.exports.get_function("lua_wasi_register_wrapper")?;
        let result = register_fn.call(&mut self.store, &[
            Value::I32(NAME_SLOT as i32),
            Value::I32(name_bytes.len() as i32),
            Value::I32(EXEC_SLOT as i32),
            Value::I32(wrapper_bytes.len() as i32),
        ])?;

        if result[0].unwrap_i32() != 0 {
            let output = self.get_output()?;
            return Err(format!("Failed to register wrapper: {}", output.trim()).into());
        }

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Lua tool: {}", name).into());
        }

        // Write args to ARGS_SLOT and tool name to NAME_SLOT
        let args_bytes = args_json.as_bytes();
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(ARGS_SLOT, args_bytes)?;
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        // Single WASI call: lua_wasi_call_tool(args_ptr, args_len, name_ptr, name_len)
        let call_fn = self.call_tool_fn.as_ref().unwrap();
        let result = call_fn.call(&mut self.store, &[
            Value::I32(ARGS_SLOT as i32),
            Value::I32(args_bytes.len() as i32),
            Value::I32(NAME_SLOT as i32),
            Value::I32(name_bytes.len() as i32),
        ])?;

        let rc = result[0].unwrap_i32();
        let output = self.get_output()?;

        if rc != 0 {
            return Err(format!("Lua tool error: {}", output.trim()).into());
        }

        Ok(output.trim().to_string())
    }

    fn call_tool_binary(&mut self, name: &str, args_tlv: &[u8]) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Lua tool: {}", name).into());
        }

        // Use optimized binary path if available
        if let Some(call_fn) = &self.call_tool_binary_fn {
            // Write TLV bytes to ARGS_SLOT and tool name to NAME_SLOT
            let name_bytes = name.as_bytes();
            self.memory.view(&self.store).write(ARGS_SLOT, args_tlv)?;
            self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

            // Call lua_wasi_call_tool_binary(tlv_ptr, tlv_len, name_ptr, name_len)
            let result = call_fn.call(&mut self.store, &[
                Value::I32(ARGS_SLOT as i32),
                Value::I32(args_tlv.len() as i32),
                Value::I32(NAME_SLOT as i32),
                Value::I32(name_bytes.len() as i32),
            ])?;

            let rc = result[0].unwrap_i32();
            let output = self.get_output()?;

            if rc != 0 {
                return Err(format!("Lua tool (binary) error: {}", output.trim()).into());
            }

            return Ok(output.trim().to_string());
        }

        // Fallback to JSON path if binary function not available
        use super::super::protocol::nda_native::decode_json_value;
        let (value, _) = decode_json_value(args_tlv)?;
        let json_str = serde_json::to_string(&value)?;
        self.call_tool(name, &json_str)
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        let destroy_fn = self.instance.exports.get_function("lua_wasi_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
    }

    fn language(&self) -> &str {
        "lua"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/lua_wasm/lua.wasm");
        p
    }

    #[test]
    #[ignore]
    fn test_lua_memory_layout() {
        let wasm = std::fs::read(wasm_path()).expect("Lua WASM not found");
        let mut rt = LuaRuntime::cold_start(&wasm).expect("cold start failed");

        let buf_start_fn = rt.instance.exports.get_function("lua_wasi_get_output_buf_start").expect("no output_buf_start");
        let buf_start = buf_start_fn.call(&mut rt.store, &[]).unwrap()[0].unwrap_i32();
        let buf_end = buf_start + 256 * 1024;

        assert!(EXEC_SLOT > buf_end as u64,
            "EXEC_SLOT {} overlaps output_buf ({} - {})",
            EXEC_SLOT, buf_start, buf_end);

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_lua_basic_exec() {
        let wasm = std::fs::read(wasm_path()).expect("Lua WASM not found — run build.sh first");
        let mut rt = LuaRuntime::cold_start(&wasm).expect("cold start failed");

        let output = rt.exec_and_get_output("print('hello from lua')").expect("exec failed");
        assert_eq!(output.trim(), "hello from lua");

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_lua_precompiled_tool() {
        let wasm = std::fs::read(wasm_path()).expect("Lua WASM not found");
        let mut rt = LuaRuntime::cold_start(&wasm).expect("cold start failed");

        // Register a tool that adds two numbers
        let source = r#"
            function add_numbers(args)
                return { sum = args.a + args.b }
            end
        "#;
        rt.register_tool("add_numbers", source).expect("register_tool failed");

        // Call the tool with JSON args
        let result = rt.call_tool("add_numbers", r#"{"a": 3, "b": 4}"#).expect("call_tool failed");
        assert!(result.contains("\"sum\""), "Expected sum in result: {}", result);
        assert!(result.contains("7"), "Expected 7 in result: {}", result);

        // Call again with different args to verify pre-compiled wrapper reuse
        let result2 = rt.call_tool("add_numbers", r#"{"a": 10, "b": 20}"#).expect("call_tool failed");
        assert!(result2.contains("30"), "Expected 30 in result: {}", result2);

        rt.destroy().unwrap();
    }
}
