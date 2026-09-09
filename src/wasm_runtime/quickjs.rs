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
    /// Cached qjs_eval function to avoid export hash lookups.
    qjs_eval_fn: Option<Function>,
    /// Reusable buffer for building eval snippets (avoids per-call allocation).
    eval_buf: Vec<u8>,
}

const EXEC_SLOT: u64 = 64 * 1024;

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

        Ok(Self {
            store,
            instance,
            memory,
            env,
            tools: HashMap::new(),
            qjs_eval_fn: None,
            eval_buf: Vec::with_capacity(512),
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
        self.memory.view(&self.store).write(EXEC_SLOT, data)?;
        // Null-terminate for C-style string handling
        self.memory.view(&self.store).write(EXEC_SLOT + data.len() as u64, &[0u8])?;
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
        let is_exc_fn = self.instance.exports.get_function("qjs_is_exception")?;
        let result = is_exc_fn.call(&mut self.store, &[Value::I32(handle)])?;
        if result[0].unwrap_i32() == 0 {
            return Ok(None);
        }

        let exc_fn = self.instance.exports.get_function("qjs_get_exception")?;
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
        let malloc_fn = self.instance.exports.get_function("wasm_malloc")?;
        let free_fn = self.instance.exports.get_function("wasm_free")?;
        let get_str_fn = self.instance.exports.get_function("qjs_get_string_len")?;

        let len_ptr_result = malloc_fn.call(&mut self.store, &[Value::I32(4)])?;
        let len_ptr = len_ptr_result[0].unwrap_i32();

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

        free_fn.call(&mut self.store, &[Value::I32(len_ptr)])?;
        Ok(result)
    }

    /// Free a JS value handle.
    pub fn free_value(&mut self, handle: i32) -> Result<(), Box<dyn Error>> {
        let free_fn = self.instance.exports.get_function("qjs_free_value")?;
        free_fn.call(&mut self.store, &[Value::I32(handle)])?;
        Ok(())
    }

    /// Get the string length of a JS value handle without reading the content.
    /// Returns None if the value is not a string.
    pub fn string_len(&mut self, handle: i32) -> Result<Option<u32>, Box<dyn Error>> {
        let malloc_fn = self.instance.exports.get_function("wasm_malloc")?;
        let free_fn = self.instance.exports.get_function("wasm_free")?;
        let get_str_fn = self.instance.exports.get_function("qjs_get_string_len")?;

        let len_ptr_result = malloc_fn.call(&mut self.store, &[Value::I32(4)])?;
        let len_ptr = len_ptr_result[0].unwrap_i32();

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

        free_fn.call(&mut self.store, &[Value::I32(len_ptr)])?;
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
        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        self.eval_to_string(source)?;

        let wrapper = format!(
            "var __wrapper = function() {{ return JSON.stringify({name}(JSON.parse(__args_buf))); }};"
        );
        self.eval_to_string(&wrapper)?;

        self.tools.insert(name.to_string(), name.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown JS tool: {}", name).into());
        }

        // Build eval snippet directly into reusable buffer:
        //   __args_buf = '<escaped>'; __wrapper()
        // Single-pass escape — no intermediate allocations.
        self.eval_buf.clear();
        self.eval_buf.extend_from_slice(b"__args_buf='");
        for &b in args_json.as_bytes() {
            match b {
                b'\\' => self.eval_buf.extend_from_slice(b"\\\\"),
                b'\'' => self.eval_buf.extend_from_slice(b"\\'"),
                b'\n' => self.eval_buf.extend_from_slice(b"\\n"),
                b'\r' => self.eval_buf.extend_from_slice(b"\\r"),
                _ => self.eval_buf.push(b),
            }
        }
        self.eval_buf.extend_from_slice(b"';__wrapper()");

        // Write to EXEC_SLOT and eval
        let len = self.eval_buf.len();
        self.memory.view(&self.store).write(EXEC_SLOT, &self.eval_buf)?;
        self.memory.view(&self.store).write(EXEC_SLOT + len as u64, &[0u8])?;
        let handle = self.eval_at(EXEC_SLOT as i32, len as i32)?;

        if let Some(exc) = self.check_exception(handle)? {
            self.free_value(handle)?;
            return Err(exc.into());
        }
        self.read_string_value(handle)?
            .ok_or_else(|| "JS evaluation returned null".into())
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
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
    fn test_quickjs_precompiled_tool() {
        let wasm = std::fs::read(wasm_path()).expect("QuickJS WASM not found");
        let mut rt = QuickJsRuntime::cold_start(&wasm).expect("cold start failed");

        let source = r#"
            function greet(args) {
                return { message: 'Hello, ' + (args.name || 'world') + '!' };
            }
        "#;
        rt.register_tool("greet", source).expect("register failed");

        let result = rt.call_tool("greet", r#"{"name": "QuickJS"}"#).expect("call failed");
        assert!(result.contains("Hello, QuickJS!"), "unexpected result: {}", result);

        let result2 = rt.call_tool("greet", r#"{"name": "Wasmer"}"#).expect("call failed");
        assert!(result2.contains("Hello, Wasmer!"), "unexpected result: {}", result2);

        rt.destroy().unwrap();
    }
}
