//! QuickJS/WASM runtime — JavaScript tool execution via QuickJS compiled to WASM.
//!
//! Uses the `quickjs-wasi` npm package build (`quickjs.wasm`) running inside Wasmer.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{WasiEnv, build_quickjs_imports};
use super::WasmRuntime;

/// QuickJS runtime compiled to WASM, running in Wasmer.
pub struct QuickJsRuntime {
    store: Store,
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    /// Registered tool names.
    tools: HashMap<String, String>,
    /// Cached function references to avoid export hash lookups on every call.
    qjs_eval_fn: Option<Function>,
    qjs_is_exception_fn: Option<Function>,
    qjs_get_exception_fn: Option<Function>,
    qjs_get_string_len_fn: Option<Function>,
    qjs_free_value_fn: Option<Function>,
    qjs_new_string_fn: Option<Function>,
    qjs_call_fn: Option<Function>,
    /// Cached JS handles for the direct-call path (bypasses qjs_eval parsing).
    global_handle: Option<i32>,
    wrapper_handle: Option<i32>,
    /// Fixed memory slot for length pointer (avoids malloc/free per call).
    len_ptr_slot: u64,
}

const EXEC_SLOT: u64 = 512 * 1024;
const LEN_PTR_SLOT: u64 = EXEC_SLOT + 64 * 1024;
const ARGV_SLOT: u64 = LEN_PTR_SLOT + 8;
const PROP_NAME_SLOT: u64 = ARGV_SLOT + 16;

impl QuickJsRuntime {
    /// Create a new QuickJS runtime from WASM module bytes.
    ///
    /// Compiles the module, instantiates with WASI imports, but does NOT call
    /// `qjs_init` — that happens in `init()`.
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let imports = build_quickjs_imports(&mut store, &env);
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
            qjs_eval_fn: None,
            qjs_is_exception_fn: None,
            qjs_get_exception_fn: None,
            qjs_get_string_len_fn: None,
            qjs_free_value_fn: None,
            qjs_new_string_fn: None,
            qjs_call_fn: None,
            global_handle: None,
            wrapper_handle: None,
            len_ptr_slot: LEN_PTR_SLOT,
        })
    }

    /// Cold start: compile + instantiate + init from raw bytes.
    pub fn cold_start(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut rt = Self::new(wasm_bytes)?;
        rt.init()?;
        Ok(rt)
    }

    /// Evaluate JS code at a pre-written memory location.
    /// Returns the result handle (caller must free via `free_value`).
    pub fn eval_at(&mut self, code_ptr: i32, code_len: i32) -> Result<i32, Box<dyn Error>> {
        let qjs_eval_fn = self.qjs_eval_fn.as_ref()
            .ok_or_else(|| "qjs_eval not cached — call init() first")?;
        let result = qjs_eval_fn.call(&mut self.store, &[
            Value::I32(code_ptr),
            Value::I32(code_len),
            Value::I32(0),
            Value::I32(0),
        ])?;
        Ok(result[0].unwrap_i32())
    }

    fn exec(&mut self, src: &str) -> Result<i32, Box<dyn Error>> {
        let data = src.as_bytes();
        let view = self.memory.view(&self.store);
        view.write(EXEC_SLOT, data)?;
        view.write(EXEC_SLOT + data.len() as u64, &[0u8])?;
        let handle = self.eval_at(EXEC_SLOT as i32, data.len() as i32)?;
        Ok(handle)
    }

    pub fn write_to_exec_slot(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        self.memory.view(&self.store).write(EXEC_SLOT, data)?;
        self.memory.view(&self.store).write(EXEC_SLOT + data.len() as u64, &[0u8])?;
        Ok(())
    }

    pub fn exec_slot_ptr(&self) -> i32 {
        EXEC_SLOT as i32
    }

    /// Check if a result handle is an exception. Returns the error message if so.
    pub fn check_exception(&mut self, handle: i32) -> Result<Option<String>, Box<dyn Error>> {
        let is_exc_fn = self.qjs_is_exception_fn.as_ref()
            .ok_or_else(|| "qjs_is_exception not cached")?;
        let result = is_exc_fn.call(&mut self.store, &[Value::I32(handle)])?;
        if result[0].unwrap_i32() == 0 {
            return Ok(None);
        }

        let exc_fn = self.qjs_get_exception_fn.as_ref()
            .ok_or_else(|| "qjs_get_exception not cached")?;
        let exc_result = exc_fn.call(&mut self.store, &[])?;
        let exc_handle = exc_result[0].unwrap_i32();
        let msg = self.read_string_from_handle(exc_handle)?;
        self.free_value(exc_handle)?;
        Ok(Some(msg.unwrap_or_else(|| "Unknown JS exception".to_string())))
    }

    /// Read a string from a JS value handle using qjs_get_string_len.
    /// Frees the value handle after reading.
    fn read_string_value(&mut self, handle: i32) -> Result<Option<String>, Box<dyn Error>> {
        let s = self.read_string_from_handle(handle)?;
        self.free_value(handle)?;
        Ok(s)
    }

    /// Read a string from a JS value handle without freeing it.
    fn read_string_from_handle(&mut self, handle: i32) -> Result<Option<String>, Box<dyn Error>> {
        let get_str_fn = self.qjs_get_string_len_fn.as_ref()
            .ok_or_else(|| "qjs_get_string_len not cached")?;

        let len_ptr = self.len_ptr_slot as i32;

        let cstr_result = get_str_fn.call(&mut self.store, &[
            Value::I32(handle),
            Value::I32(len_ptr),
        ])?;
        let cstr_ptr = cstr_result[0].unwrap_i32();

        let result = if cstr_ptr != 0 {
            let mut len_buf = [0u8; 4];
            self.memory.view(&self.store).read(len_ptr as u64, &mut len_buf)?;
            let str_len = u32::from_le_bytes(len_buf) as usize;
            let mut str_buf = vec![0u8; str_len];
            self.memory.view(&self.store).read(cstr_ptr as u64, &mut str_buf)?;
            Some(String::from_utf8_lossy(&str_buf).to_string())
        } else {
            None
        };

        Ok(result)
    }

    /// Free a JS value handle.
    pub fn free_value(&mut self, handle: i32) -> Result<(), Box<dyn Error>> {
        let free_fn = self.qjs_free_value_fn.as_ref()
            .ok_or_else(|| "qjs_free_value not cached")?;
        free_fn.call(&mut self.store, &[Value::I32(handle)])?;
        Ok(())
    }

    /// Get the string length of a JS value handle without reading the content.
    /// Returns None if the value is not a string.
    pub fn string_len(&mut self, handle: i32) -> Result<Option<u32>, Box<dyn Error>> {
        let get_str_fn = self.qjs_get_string_len_fn.as_ref()
            .ok_or_else(|| "qjs_get_string_len not cached")?;

        let len_ptr = self.len_ptr_slot as i32;

        let cstr_result = get_str_fn.call(&mut self.store, &[
            Value::I32(handle),
            Value::I32(len_ptr),
        ])?;
        let cstr_ptr = cstr_result[0].unwrap_i32();

        let result = if cstr_ptr != 0 {
            let mut len_buf = [0u8; 4];
            self.memory.view(&self.store).read(len_ptr as u64, &mut len_buf)?;
            Some(u32::from_le_bytes(len_buf))
        } else {
            None
        };

        Ok(result)
    }

    /// Evaluate JS code and return the string result, or error.
    pub fn eval_to_string(&mut self, code: &str) -> Result<String, Box<dyn Error>> {
        let handle = self.exec(code)?;
        if let Some(exc) = self.check_exception(handle)? {
            self.free_value(handle)?;
            return Err(exc.into());
        }
        self.read_string_value(handle)?
            .ok_or_else(|| "JS evaluation returned null".into())
    }

    /// Read the string result from a handle, check for exceptions, free the handle.
    /// Returns the string or an error.
    pub fn read_result(&mut self, handle: i32) -> Result<String, Box<dyn Error>> {
        if let Some(exc) = self.check_exception(handle)? {
            self.free_value(handle)?;
            return Err(exc.into());
        }
        self.read_string_value(handle)?
            .ok_or_else(|| "JS evaluation returned null".into())
    }

    /// Benchmark cold start: compile+instantiate+init, returns ms per iteration.
    pub fn bench_cold_start(wasm_bytes: &[u8]) -> f64 {
        let cold_iters = 20;
        let start = std::time::Instant::now();
        for _ in 0..cold_iters {
            let engine = wasmer::Engine::from(wasmer::Cranelift::default());
            let module = Module::new(&engine, wasm_bytes).unwrap();
            let mut store = Store::new(engine);
            let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
            let imports = build_quickjs_imports(&mut store, &env);
            let instance = Instance::new(&mut store, &module, &imports).unwrap();
            let memory = instance.exports.get_memory("memory").unwrap().clone();
            env.as_mut(&mut store).memory = Some(memory);
            let init_fn = instance.exports.get_function("_initialize")
                .unwrap().typed::<(), ()>(&store).unwrap();
            init_fn.call(&mut store).unwrap();
            let qjs_init_fn = instance.exports.get_function("qjs_init")
                .unwrap().typed::<(), i32>(&store).unwrap();
            let _ = qjs_init_fn.call(&mut store).unwrap();
        }
        start.elapsed().as_nanos() as f64 / cold_iters as f64 / 1_000_000.0
    }
}

impl WasmRuntime for QuickJsRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let init_fn = self.instance.exports.get_function("_initialize")?
            .typed::<(), ()>(&self.store)?;
        init_fn.call(&mut self.store)?;

        let qjs_init_fn = self.instance.exports.get_function("qjs_init")?
            .typed::<(), i32>(&self.store)?;
        let result = qjs_init_fn.call(&mut self.store)?;
        if result != 0 {
            return Err(format!("qjs_init() failed with code {}", result).into());
        }

        self.qjs_eval_fn = Some(self.instance.exports.get_function("qjs_eval")?.clone());
        self.qjs_is_exception_fn = Some(self.instance.exports.get_function("qjs_is_exception")?.clone());
        self.qjs_get_exception_fn = Some(self.instance.exports.get_function("qjs_get_exception")?.clone());
        self.qjs_get_string_len_fn = Some(self.instance.exports.get_function("qjs_get_string_len")?.clone());
        self.qjs_free_value_fn = Some(self.instance.exports.get_function("qjs_free_value")?.clone());
        self.qjs_new_string_fn = Some(self.instance.exports.get_function("qjs_new_string")?.clone());
        self.qjs_call_fn = Some(self.instance.exports.get_function("qjs_call")?.clone());

        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        self.eval_to_string(source)?;

        let wrapper = format!(
            "var __wrapper = function(a) {{ return JSON.stringify({name}(JSON.parse(a))); }};"
        );
        self.eval_to_string(&wrapper)?;

        let get_global_fn = self.instance.exports.get_function("qjs_get_global")?;
        let global_result = get_global_fn.call(&mut self.store, &[])?;
        let global_handle = global_result[0].unwrap_i32();
        self.global_handle = Some(global_handle);

        let get_prop_fn = self.instance.exports.get_function("qjs_get_prop_string")?;
        let prop_name = b"__wrapper\0";
        self.memory.view(&self.store).write(PROP_NAME_SLOT, prop_name)?;
        let wrapper_result = get_prop_fn.call(&mut self.store, &[
            Value::I32(global_handle),
            Value::I32(PROP_NAME_SLOT as i32),
        ])?;
        let wrapper_handle = wrapper_result[0].unwrap_i32();
        self.wrapper_handle = Some(wrapper_handle);

        self.tools.insert(name.to_string(), name.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown JS tool: {}", name).into());
        }

        let args_bytes = args_json.as_bytes();
        self.memory.view(&self.store).write(EXEC_SLOT, args_bytes)?;

        let new_str_fn = self.qjs_new_string_fn.as_ref()
            .ok_or_else(|| "qjs_new_string not cached")?;
        let args_result = new_str_fn.call(&mut self.store, &[
            Value::I32(EXEC_SLOT as i32),
            Value::I32(args_bytes.len() as i32),
        ])?;
        let args_handle = args_result[0].unwrap_i32();

        let argv_bytes = args_handle.to_le_bytes();
        self.memory.view(&self.store).write(ARGV_SLOT, &argv_bytes)?;

        let global = self.global_handle.unwrap_or(0);
        let wrapper = self.wrapper_handle
            .ok_or_else(|| "wrapper not cached — call register_tool first")?;

        let call_fn = self.qjs_call_fn.as_ref()
            .ok_or_else(|| "qjs_call not cached")?;
        let result = call_fn.call(&mut self.store, &[
            Value::I32(wrapper),
            Value::I32(global),
            Value::I32(1),
            Value::I32(ARGV_SLOT as i32),
        ])?;
        let result_handle = result[0].unwrap_i32();

        if let Some(exc) = self.check_exception(result_handle)? {
            self.free_value(args_handle)?;
            self.free_value(result_handle)?;
            return Err(exc.into());
        }

        let output = match self.read_string_value(result_handle)? {
            Some(s) => s,
            None => return Err("JS evaluation returned null".into()),
        };
        self.free_value(args_handle)?;
        Ok(output)
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        if let Some(wrapper) = self.wrapper_handle.take() {
            self.free_value(wrapper)?;
        }
        if let Some(global) = self.global_handle.take() {
            self.free_value(global)?;
        }
        let destroy_fn = self.instance.exports.get_function("qjs_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
    }

    fn language(&self) -> &str {
        "javascript"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/quickjs_wasm/quickjs.wasm");
        p
    }

    #[test]
    #[ignore]
    fn test_quickjs_basic_exec() {
        let wasm = std::fs::read(wasm_path()).expect("QuickJS WASM not found");
        let mut rt = QuickJsRuntime::cold_start(&wasm).expect("cold start failed");

        let output = rt.eval_to_string("'hello from quickjs'").expect("exec failed");
        assert_eq!(output, "hello from quickjs");

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_quickjs_call_tool_via_api() {
        let wasm = std::fs::read(wasm_path()).expect("QuickJS WASM not found");
        let mut rt = QuickJsRuntime::cold_start(&wasm).expect("cold start failed");

        let source = r#"
            function greet(args) {
                return { message: 'Hello, ' + (args.name || 'world') + '!' };
            }
        "#;
        rt.register_tool("greet", source).expect("register failed");

        let result = rt.call_tool("greet", r#"{"name": "QuickJS"}"#).expect("call failed");
        assert!(result.contains("Hello, QuickJS!"), "unexpected: {}", result);

        let result2 = rt.call_tool("greet", r#"{"name": "Wasmer"}"#).expect("call failed");
        assert!(result2.contains("Hello, Wasmer!"), "unexpected: {}", result2);

        rt.destroy().unwrap();
    }
}
