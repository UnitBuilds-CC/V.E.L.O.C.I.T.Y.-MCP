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
    qjs_tool_call_fn: Option<Function>,
    qjs_tool_call_binary_fn: Option<Function>,
    qjs_free_cstring_fn: Option<Function>,
    /// Batched single-call tool invocation (requires qjs_tool_call + qjs_free_cstring exports).
    batched: bool,
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
/// 16-byte output area for qjs_tool_call:
/// [result_ptr: u32][result_len: u32][error_ptr: u32][error_len: u32]
const OUT_SLOT: u64 = PROP_NAME_SLOT + 16;

impl QuickJsRuntime {
    /// Create a new QuickJS runtime from WASM module bytes.
    ///
    /// Compiles the module, instantiates with WASI imports, but does NOT call
    /// `qjs_init` — that happens in `init()`.
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let env = FunctionEnv::new(&mut store, WasiEnv::new());
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
            qjs_tool_call_fn: None,
            qjs_tool_call_binary_fn: None,
            qjs_free_cstring_fn: None,
            batched: false,
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

        // qjs_get_string_len returns a caller-owned cstring ref (JS_ToCStringLen2);
        // it must be released or every read leaks the string. Optional because the
        // stock npm build does not export qjs_free_cstring.
        if cstr_ptr != 0 {
            if let Some(free_fn) = self.qjs_free_cstring_fn.as_ref() {
                free_fn.call(&mut self.store, &[Value::I32(cstr_ptr)])?;
            }
        }

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
            if let Some(free_fn) = self.qjs_free_cstring_fn.as_ref() {
                free_fn.call(&mut self.store, &[Value::I32(cstr_ptr)])?;
            }
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
            let env = FunctionEnv::new(&mut store, WasiEnv::new());
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

    /// Batched tool invocation: 1 WASM call instead of 6.
    ///
    /// qjs_tool_call performs new_string + call + exception check + string
    /// extraction + value frees inside the module, returning caller-owned
    /// cstring refs that we read once and release via qjs_free_cstring.
    fn call_tool_batched(&mut self, args_json: &str) -> Result<String, Box<dyn Error>> {
        let args_bytes = args_json.as_bytes();
        if args_bytes.len() >= 64 * 1024 {
            return Err("JS tool arguments exceed 64KB EXEC_SLOT".into());
        }
        let wrapper = self.wrapper_handle
            .ok_or_else(|| "wrapper not cached — call register_tool first")?;
        let global = self.global_handle.unwrap_or(0);

        {
            let view = self.memory.view(&self.store);
            view.write(OUT_SLOT, &[0u8; 16])?;
            view.write(EXEC_SLOT, args_bytes)?;
        }

        let tool_fn = self.qjs_tool_call_fn.as_ref()
            .ok_or_else(|| "qjs_tool_call not cached")?;
        let ret = tool_fn.call(&mut self.store, &[
            Value::I32(EXEC_SLOT as i32),
            Value::I32(args_bytes.len() as i32),
            Value::I32(wrapper),
            Value::I32(global),
            Value::I32(OUT_SLOT as i32),
            Value::I32((OUT_SLOT + 4) as i32),
            Value::I32((OUT_SLOT + 8) as i32),
            Value::I32((OUT_SLOT + 12) as i32),
        ])?;
        let status = ret[0].unwrap_i32();

        let mut out = [0u8; 16];
        self.memory.view(&self.store).read(OUT_SLOT, &mut out)?;
        let result_ptr = u32::from_le_bytes(out[0..4].try_into().unwrap());
        let result_len = u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize;
        let error_ptr = u32::from_le_bytes(out[8..12].try_into().unwrap());
        let error_len = u32::from_le_bytes(out[12..16].try_into().unwrap()) as usize;

        if status != 0 || error_ptr != 0 {
            let msg = if error_ptr != 0 {
                let mut buf = vec![0u8; error_len];
                self.memory.view(&self.store).read(error_ptr as u64, &mut buf)?;
                self.free_cstring(error_ptr)?;
                String::from_utf8_lossy(&buf).to_string()
            } else {
                "JS tool call failed".to_string()
            };
            return Err(msg.into());
        }

        if result_ptr == 0 {
            return Err("JS tool returned null".into());
        }
        let mut buf = vec![0u8; result_len];
        self.memory.view(&self.store).read(result_ptr as u64, &mut buf)?;
        self.free_cstring(result_ptr)?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    fn free_cstring(&mut self, ptr: u32) -> Result<(), Box<dyn Error>> {
        let f = self.qjs_free_cstring_fn.as_ref()
            .ok_or_else(|| "qjs_free_cstring not cached")?;
        f.call(&mut self.store, &[Value::I32(ptr as i32)])?;
        Ok(())
    }
}

impl WasmRuntime for QuickJsRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        // _initialize is only for WASI reactor modules; QuickJS is a command module
        // Skip _initialize and just call qjs_init
        
        let qjs_init_fn = match self.instance.exports.get_function("qjs_init") {
            Ok(f) => f.typed::<(), i32>(&self.store)?,
            Err(_) => return Ok(()), // No qjs_init means already initialized or different build
        };
        
        let result = qjs_init_fn.call(&mut self.store)?;
        if result != 0 {
            // qjs_init can fail on some builds; continue anyway and try to get functions
            eprintln!("Warning: qjs_init() returned {}, continuing anyway", result);
        }

        // Try to get functions even if init failed
        self.qjs_eval_fn = self.instance.exports.get_function("qjs_eval").ok().cloned();
        self.qjs_is_exception_fn = self.instance.exports.get_function("qjs_is_exception").ok().cloned();
        self.qjs_get_exception_fn = self.instance.exports.get_function("qjs_get_exception").ok().cloned();
        self.qjs_get_string_len_fn = self.instance.exports.get_function("qjs_get_string_len").ok().cloned();
        self.qjs_free_value_fn = self.instance.exports.get_function("qjs_free_value").ok().cloned();
        self.qjs_new_string_fn = self.instance.exports.get_function("qjs_new_string").ok().cloned();
        self.qjs_call_fn = self.instance.exports.get_function("qjs_call").ok().cloned();

        // Batched API is optional (custom build only); fall back to 6-call path.
        self.qjs_tool_call_fn = self.instance.exports.get_function("qjs_tool_call").ok().cloned();
        self.qjs_tool_call_binary_fn = self.instance.exports.get_function("qjs_tool_call_binary").ok().cloned();
        self.qjs_free_cstring_fn = self.instance.exports.get_function("qjs_free_cstring").ok().cloned();
        self.batched = self.qjs_tool_call_fn.is_some() && self.qjs_free_cstring_fn.is_some();

        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // Check if source changed (skip re-execution if unchanged)
        if let Some(existing_source) = self.tools.get(name) {
            if existing_source == source {
                return Ok(()); // No change, skip re-execution
            }
        }

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

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown JS tool: {}", name).into());
        }

        if self.batched {
            return self.call_tool_batched(args_json);
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

    fn call_tool_binary(&mut self, name: &str, args_tlv: &[u8]) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown JS tool: {}", name).into());
        }

        // If binary function not available, fall back to JSON path
        let call_fn = match &self.qjs_tool_call_binary_fn {
            Some(f) => f,
            None => {
                use crate::protocol::nda_native::decode_json_value;
                let (value, _) = decode_json_value(args_tlv)?;
                let json_str = serde_json::to_string(&value)?;
                return self.call_tool(name, &json_str);
            }
        };

        // Write TLV bytes to EXEC_SLOT and tool name to NAME_SLOT
        let wrapper = self.wrapper_handle
            .ok_or_else(|| "wrapper not cached — call register_tool first")?;
        let global = self.global_handle.unwrap_or(0);

        {
            let view = self.memory.view(&self.store);
            view.write(OUT_SLOT, &[0u8; 16])?;
            view.write(EXEC_SLOT, args_tlv)?;
        }

        let ret = call_fn.call(&mut self.store, &[
            Value::I32(EXEC_SLOT as i32),
            Value::I32(args_tlv.len() as i32),
            Value::I32(wrapper),
            Value::I32(global),
            Value::I32(OUT_SLOT as i32),
            Value::I32((OUT_SLOT + 4) as i32),
            Value::I32((OUT_SLOT + 8) as i32),
            Value::I32((OUT_SLOT + 12) as i32),
        ])?;
        let status = ret[0].unwrap_i32();

        let mut out = [0u8; 16];
        self.memory.view(&self.store).read(OUT_SLOT, &mut out)?;
        let result_ptr = u32::from_le_bytes(out[0..4].try_into().unwrap());
        let result_len = u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize;
        let error_ptr = u32::from_le_bytes(out[8..12].try_into().unwrap());
        let error_len = u32::from_le_bytes(out[12..16].try_into().unwrap()) as usize;

        if status != 0 || error_ptr != 0 {
            let msg = if error_ptr != 0 {
                let mut buf = vec![0u8; error_len];
                self.memory.view(&self.store).read(error_ptr as u64, &mut buf)?;
                self.free_cstring(error_ptr)?;
                String::from_utf8_lossy(&buf).to_string()
            } else {
                "JS tool call failed".to_string()
            };
            return Err(msg.into());
        }

        if result_ptr == 0 {
            return Err("JS tool returned null".into());
        }
        let mut buf = vec![0u8; result_len];
        self.memory.view(&self.store).read(result_ptr as u64, &mut buf)?;
        self.free_cstring(result_ptr)?;
        Ok(String::from_utf8_lossy(&buf).to_string())
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

    #[test]
    #[ignore]
    fn test_quickjs_custom_batched_path() {
        let wasm = std::fs::read(wasm_path()).expect("QuickJS WASM not found");
        let mut rt = QuickJsRuntime::cold_start(&wasm).expect("cold start failed");
        assert!(rt.batched, "shipped build should expose qjs_tool_call");

        let source = r#"
            function greet(args) {
                if (args.boom) throw new Error('kaboom: ' + args.boom);
                return { message: 'Hello, ' + (args.name || 'world') + '!', n: args.n + 1 };
            }
        "#;
        rt.register_tool("greet", source).expect("register failed");

        let result = rt.call_tool("greet", r#"{"name": "Batched", "n": 41}"#).expect("call failed");
        assert!(result.contains("Hello, Batched!") && result.contains(r#""n":42"#),
            "unexpected: {}", result);

        // Unicode round-trip through the batched path
        let result2 = rt.call_tool("greet", r#"{"name": "wörld ✓", "n": 0}"#).expect("call failed");
        assert!(result2.contains("wörld ✓"), "unicode mangled: {}", result2);

        // Error propagation: JS exception must surface as Err with the message
        let err = rt.call_tool("greet", r#"{"boom": "test"}"#).expect_err("should fail");
        assert!(err.to_string().contains("kaboom: test"), "unexpected error: {}", err);

        // Repeated calls after an error must still work (state not corrupted)
        let result3 = rt.call_tool("greet", r#"{"name": "again", "n": 1}"#).expect("call after error failed");
        assert!(result3.contains("Hello, again!"), "unexpected: {}", result3);

        rt.destroy().unwrap();
    }

    /// Scratch test for destroy-trap isolation: QJS_ISO_MODE=legacy|batched,
    /// QJS_ISO_ITERS=N
    #[test]
    #[ignore]
    fn test_destroy_isolation() {
        let mode = std::env::var("QJS_ISO_MODE").unwrap_or_else(|_| "batched".into());
        let iters: usize = std::env::var("QJS_ISO_ITERS").ok()
            .and_then(|v| v.parse().ok()).unwrap_or(20_000);

        let wasm = std::fs::read(wasm_path()).expect("wasm not found");
        let mut rt = QuickJsRuntime::cold_start(&wasm).expect("cold start failed");

        let source = r#"
            function bench_tool(args) {
                return {size: 64, payload: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"};
            }
        "#;
        rt.register_tool("bench_tool", source).expect("register failed");
        let save = rt.batched;
        // Modes: "legacy" | "batched" run one path; "both" = legacy then
        // batched; "both_rev" = batched then legacy (matches A/B bench order).
        let phase1_batched = match mode.as_str() {
            "legacy" | "both" => false,
            _ => true,
        };
        rt.batched = phase1_batched;

        for i in 0..iters {
            rt.call_tool("bench_tool", r#"{"text": "hello"}"#)
                .unwrap_or_else(|e| panic!("call {} failed: {}", i, e));
        }
        rt.batched = save;
        println!(
            "phase 1 done ({} / batched={}), heap pages={}, continuing...",
            iters, phase1_batched,
            rt.memory.view(&rt.store).size().0
        );

        if mode == "both" || mode == "both_rev" {
            rt.batched = !phase1_batched;
            for i in 0..iters {
                rt.call_tool("bench_tool", r#"{"text": "hello"}"#)
                    .unwrap_or_else(|e| panic!("call {} failed: {}", i, e));
            }
            rt.batched = save;
            println!(
                "phase 2 done (batched={}, heap pages={}), destroying...",
                !phase1_batched,
                rt.memory.view(&rt.store).size().0
            );
        }

        println!("destroying...");
        match rt.destroy() {
            Ok(()) => println!("destroy: OK"),
            Err(e) => println!("destroy: TRAP — {}", e),
        }
    }

    #[test]
    #[ignore]
    fn bench_quickjs_batched_vs_legacy() {
        use std::hint::black_box;
        let wasm = std::fs::read(wasm_path()).expect("QuickJS WASM not found");
        let mut rt = QuickJsRuntime::cold_start(&wasm).expect("cold start failed");
        assert!(rt.batched);

        let source = r#"
            function bench_tool(args) {
                return {size: 64, payload: "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"};
            }
        "#;
        rt.register_tool("bench_tool", source).expect("register failed");
        let args = r#"{"text": "hello"}"#;
        let iterations = 20_000;

        let measure = |rt: &mut QuickJsRuntime| {
            let mut checksum: u32 = 0;
            let start = std::time::Instant::now();
            for _ in 0..iterations {
                let result = rt.call_tool("bench_tool", args).expect("call failed");
                checksum = checksum.wrapping_add(result.len() as u32);
            }
            (start.elapsed().as_nanos() as f64 / iterations as f64, checksum)
        };

        // Warmup both paths
        rt.call_tool("bench_tool", args).unwrap();
        let save = rt.batched;
        rt.batched = false;
        rt.call_tool("bench_tool", args).unwrap();
        rt.batched = save;

        let (legacy_ns, c1) = {
            rt.batched = false;
            let r = measure(&mut rt);
            rt.batched = save;
            r
        };
        let (batched_ns, c2) = measure(&mut rt);
        assert_eq!(c1, c2, "paths must produce identical output");
        black_box((legacy_ns, batched_ns));

        println!("legacy (6 calls):  {:.0} ns/call", legacy_ns);
        println!("batched (1 call):  {:.0} ns/call", batched_ns);
        println!("improvement:       {:.2}x", legacy_ns / batched_ns);

        rt.destroy().unwrap();
    }
}
