//! Rust/WASM runtime — native Rust tools compiled to wasm32-wasi.
//!
//! Unlike interpreter-based runtimes (QuickJS, MicroPython, Lua), Rust tools
//! are compiled directly to WASM modules. Each tool is a standalone WASM binary
//! that exports a standard interface for tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{FunctionEnv, Instance, Module, Store, Value as WasmValue};

use super::wasi::{WasiEnv, build_wasi_imports};
use super::WasmRuntime;

pub struct RustRuntime {
    store: Store,
    env: FunctionEnv<WasiEnv>,
    /// Compiled WASM modules keyed by tool name
    modules: HashMap<String, Module>,
    /// Active instances (one per tool, reused across calls)
    instances: HashMap<String, Instance>,
}

impl RustRuntime {
    pub fn new(_wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let mut store = Store::new(engine);
        let env = FunctionEnv::new(&mut store, WasiEnv::new());

        Ok(Self {
            store,
            env,
            modules: HashMap::new(),
            instances: HashMap::new(),
        })
    }

    /// Load a pre-compiled Rust WASM module as a tool.
    /// The WASM module must export: prepare_call(), tool_execute(ptr, len), malloc(size)
    pub fn load_tool(&mut self, name: &str, wasm_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
        let module = Module::new(&self.store, wasm_bytes)?;
        self.modules.insert(name.to_string(), module);
        Ok(())
    }

    /// Instantiate a tool's WASM module (called once per tool, reused for all calls)
    fn ensure_instantiated(&mut self, name: &str) -> Result<(), Box<dyn Error>> {
        if self.instances.contains_key(name) {
            return Ok(());
        }

        let module = self.modules.get(name)
            .ok_or_else(|| format!("Tool not loaded: {}", name))?
            .clone();

        let wasi_imports = build_wasi_imports(&mut self.store, &self.env);
        let instance = Instance::new(&mut self.store, &module, &wasi_imports)?;

        let memory = instance.exports.get_memory("memory")?.clone();
        self.env.as_mut(&mut self.store).memory = Some(memory);

        // Call _start to initialize the tool
        if let Ok(start_fn) = instance.exports.get_function("_start") {
            let _ = start_fn.call(&mut self.store, &[]);
        }

        self.instances.insert(name.to_string(), instance);
        Ok(())
    }
}

impl WasmRuntime for RustRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        // Rust runtime doesn't need initialization — tools are loaded on demand
        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // For Rust, "source" is the path to a pre-compiled .wasm file
        let wasm_bytes = std::fs::read(source)?;
        self.load_tool(name, &wasm_bytes)?;
        self.ensure_instantiated(name)?;
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        self.ensure_instantiated(name)?;

        let instance = self.instances.get(name).unwrap();

        let get_input_ptr = instance.exports.get_function("get_input_ptr")?;
        let ptr_results = get_input_ptr.call(&mut self.store, &[])?;
        let input_ptr = ptr_results[0].unwrap_i32();

        let args_bytes = args_json.as_bytes();
        let memory = instance.exports.get_memory("memory")?;
        memory.view(&self.store).write(input_ptr as u64, args_bytes)?;

        let execute = instance.exports.get_function("tool_execute")?;
        let result = execute.call(&mut self.store, &[WasmValue::I32(args_bytes.len() as i32)])?;

        let encoded = result[0].unwrap_i64();
        let result_ptr = (encoded >> 32) as u32;
        let result_len = (encoded & 0xFFFF_FFFF) as u32;

        let mut result_buf = vec![0u8; result_len as usize];
        memory.view(&self.store).read(result_ptr as u64, &mut result_buf)?;

        Ok(String::from_utf8_lossy(&result_buf).to_string())
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        self.instances.clear();
        self.modules.clear();
        Ok(())
    }

    fn language(&self) -> &str {
        "rust"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // requires a pre-compiled Rust WASM tool
    fn test_rust_tool_execution() {
        // This test requires a Rust tool compiled to wasm32-wasi
        // Example: cargo build --target wasm32-wasi --release
        let wasm_path = "bench_tools/rust_wasm/example_tool.wasm";
        if !std::path::Path::new(wasm_path).exists() {
            eprintln!("Skipping test: {} not found", wasm_path);
            return;
        }

        let wasm_bytes = std::fs::read(wasm_path).expect("failed to read WASM");
        let mut rt = RustRuntime::new(&wasm_bytes).expect("failed to create runtime");
        rt.register_tool("example_tool", wasm_path).expect("failed to register tool");

        let result = rt.call_tool("example_tool", r#"{"input": "test"}"#).expect("call failed");
        assert!(!result.is_empty(), "tool returned empty result");

        rt.destroy().unwrap();
    }
}
