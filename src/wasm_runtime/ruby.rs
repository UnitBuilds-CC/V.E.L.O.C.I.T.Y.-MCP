//! Ruby/WASM runtime — mruby tool execution via WASI reactor.
//!
//! Uses a manually built mruby WASM binary (`ruby.wasm`) running inside Wasmer.
//! mruby is compiled without WASM exception handling; setjmp/longjmp are stubbed
//! (setjmp returns 0, longjmp aborts). Ruby exceptions will terminate the
//! interpreter, but normal tool execution without exceptions works correctly.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{build_wasi_imports, WasiEnv};
use super::WasmRuntime;

pub struct RubyRuntime {
    store: Store,
    #[allow(dead_code)]
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    /// Registered tool names.
    tools: HashMap<String, String>,
    /// Cached function references to avoid export hash lookups on every call.
    mruby_eval_fn: Option<Function>,
    mruby_call_tool_fn: Option<Function>,
    mruby_get_output_len_fn: Option<Function>,
    mruby_get_output_fn: Option<Function>,
    /// Reusable buffer for tool call output (avoids per-call allocation).
    output_buf: Vec<u8>,
}

const SLOT_CODE: u64 = 512 * 1024;
const SLOT_ARGS: u64 = SLOT_CODE + 65536;
const SLOT_NAME: u64 = SLOT_ARGS + 65536;
const TOTAL_SLOTS: u64 = SLOT_NAME + 1024;

impl RubyRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let env = FunctionEnv::new(&mut store, WasiEnv::new());
        let imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &imports)?;

        let memory = instance.exports.get_memory("memory")?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());

        let needed_pages = TOTAL_SLOTS.div_ceil(65536) as u32;
        let current_pages = memory.view(&store).size();
        if current_pages.0 < needed_pages {
            memory.grow(&mut store, wasmer::Pages(needed_pages - current_pages.0))?;
        }

        Ok(Self {
            store,
            instance,
            memory,
            env,
            tools: HashMap::new(),
            mruby_eval_fn: None,
            mruby_call_tool_fn: None,
            mruby_get_output_len_fn: None,
            mruby_get_output_fn: None,
            output_buf: Vec::with_capacity(512),
        })
    }

    pub fn cold_start(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut rt = Self::new(wasm_bytes)?;
        rt.init()?;
        Ok(rt)
    }

    fn write_to_slot(&mut self, slot: u64, data: &[u8]) -> Result<(), Box<dyn Error>> {
        self.memory.view(&self.store).write(slot, data)?;
        Ok(())
    }

    fn read_output(&mut self) -> Result<String, Box<dyn Error>> {
        let get_len_fn = self
            .mruby_get_output_len_fn
            .as_ref()
            .ok_or("mruby_get_output_len not cached")?;
        let get_ptr_fn = self
            .mruby_get_output_fn
            .as_ref()
            .ok_or("mruby_get_output not cached")?;

        let len_result = get_len_fn.call(&mut self.store, &[])?;
        let len = len_result[0].unwrap_i32() as usize;

        if len == 0 {
            return Ok(String::new());
        }

        let ptr_result = get_ptr_fn.call(&mut self.store, &[])?;
        let ptr = ptr_result[0].unwrap_i32() as u64;

        // Reuse output_buf to avoid per-call allocation
        self.output_buf.clear();
        self.output_buf.resize(len, 0u8);
        self.memory
            .view(&self.store)
            .read(ptr, &mut self.output_buf)?;
        Ok(String::from_utf8_lossy(&self.output_buf).to_string())
    }

    #[allow(dead_code)]
    fn eval_code(&mut self, code: &str) -> Result<String, Box<dyn Error>> {
        let bytes = code.as_bytes();
        self.write_to_slot(SLOT_CODE, bytes)?;

        let eval_fn = self.mruby_eval_fn.as_ref().ok_or("mruby_eval not cached")?;
        let result = eval_fn.call(
            &mut self.store,
            &[Value::I32(SLOT_CODE as i32), Value::I32(bytes.len() as i32)],
        )?;
        let status = result[0].unwrap_i32();

        let output = self.read_output()?;

        if status != 0 {
            return Err(output.into());
        }
        Ok(output)
    }
}

impl WasmRuntime for RubyRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let init_fn = self.instance.exports.get_function("mruby_init")?;
        init_fn.call(&mut self.store, &[])?;

        // Cache all frequently-used function references
        self.mruby_eval_fn = Some(self.instance.exports.get_function("mruby_eval")?.clone());
        self.mruby_call_tool_fn = Some(
            self.instance
                .exports
                .get_function("mruby_call_tool")?
                .clone(),
        );
        self.mruby_get_output_len_fn = Some(
            self.instance
                .exports
                .get_function("mruby_get_output_len")?
                .clone(),
        );
        self.mruby_get_output_fn = Some(
            self.instance
                .exports
                .get_function("mruby_get_output")?
                .clone(),
        );

        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // Check if source changed (skip re-execution if unchanged)
        if let Some(existing_source) = self.tools.get(name) {
            if existing_source == source {
                return Ok(()); // No change, skip re-execution
            }
        }

        let name_bytes = name.as_bytes();
        let source_bytes = source.as_bytes();

        self.write_to_slot(SLOT_CODE, source_bytes)?;
        self.write_to_slot(SLOT_NAME, name_bytes)?;

        let register_fn = self.instance.exports.get_function("mruby_register_tool")?;
        let result = register_fn.call(
            &mut self.store,
            &[
                Value::I32(SLOT_NAME as i32),
                Value::I32(name_bytes.len() as i32),
                Value::I32(SLOT_CODE as i32),
                Value::I32(source_bytes.len() as i32),
            ],
        )?;
        let status = result[0].unwrap_i32();

        if status != 0 {
            let err = self
                .read_output()
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(format!("Failed to register Ruby tool '{}': {}", name, err).into());
        }

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Ruby tool: {}", name).into());
        }

        let name_bytes = name.as_bytes();
        let args_bytes = args_json.as_bytes();

        self.write_to_slot(SLOT_ARGS, args_bytes)?;
        self.write_to_slot(SLOT_NAME, name_bytes)?;

        let call_fn = self
            .mruby_call_tool_fn
            .as_ref()
            .ok_or("mruby_call_tool not cached")?;
        let result = call_fn.call(
            &mut self.store,
            &[
                Value::I32(SLOT_NAME as i32),
                Value::I32(name_bytes.len() as i32),
                Value::I32(SLOT_ARGS as i32),
                Value::I32(args_bytes.len() as i32),
            ],
        )?;
        let status = result[0].unwrap_i32();

        let output = self.read_output()?;

        if status != 0 {
            return Err(output.into());
        }
        Ok(output)
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        let destroy_fn = self.instance.exports.get_function("mruby_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
    }

    fn language(&self) -> &str {
        "ruby"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/mruby_wasm/ruby.wasm");
        p
    }

    #[test]
    fn test_ruby_init() {
        let wasm = std::fs::read(wasm_path()).expect("Ruby WASM not found");
        let mut rt = RubyRuntime::cold_start(&wasm).expect("cold start failed");
        let output = rt.eval_code("1 + 2").expect("eval failed");
        assert_eq!(output, "3");
        rt.destroy().unwrap();
    }

    #[test]
    fn test_ruby_tool() {
        let wasm = std::fs::read(wasm_path()).expect("Ruby WASM not found");
        let mut rt = RubyRuntime::cold_start(&wasm).expect("cold start failed");

        let source =
            "def greet(args)\n  {\"message\" => \"Hello, \" + args[\"name\"].to_s + \"!\"}\nend\n";
        rt.register_tool("greet", source).expect("register failed");

        let result = rt
            .call_tool("greet", r#"{"name": "mruby"}"#)
            .expect("call failed");
        assert!(
            result.contains("Hello, mruby!"),
            "unexpected result: {}",
            result
        );

        rt.destroy().unwrap();
    }
}
