//! Ruby/WASM runtime — CRuby tool execution via ruby.wasm Component Model.
//!
//! Uses the `@ruby/3.4-wasm-wasi` package build (`ruby.wasm`) running inside Wasmer.
//! The Ruby WASM binary uses the Component Model ABI with full type-signature export names.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{WasiEnv, RubySlab, new_ruby_slab, slab_insert, slab_get, build_ruby_imports};
use super::WasmRuntime;

/// Ruby runtime compiled to WASM via Component Model, running in Wasmer.
pub struct RubyRuntime {
    store: Store,
    #[allow(dead_code)]
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    ruby_slab: RubySlab,
    #[allow(dead_code)]
    js_slab: RubySlab,
    /// Registered tool names → Ruby source code
    tools: HashMap<String, String>,
    /// Cached cabi_realloc for string allocation
    cabi_realloc: Function,
    /// Cached rb-eval-string-protect
    rb_eval: Function,
    /// Cached rb-intern
    rb_intern: Function,
    /// Cached rb-funcallv-protect
    rb_funcallv: Function,
    /// Cached rstring-ptr
    rstring_ptr_fn: Function,
    /// Cached cabi_post_rstring-ptr
    cabi_post_rstring: Function,
    /// Cached rb-errinfo
    rb_errinfo: Function,
    /// Cached rb-clear-errinfo
    rb_clear_errinfo: Function,
    /// Cached method ID for "to_s"
    to_s_mid: u32,
}

/// Status codes from ruby_tag_type (masked with 0xF):
const RUBY_TAG_NONE: u32 = 0;
const RUBY_TAG_RAISE: u32 = 6;
const RUBY_TAG_FATAL: u32 = 8;

impl RubyRuntime {
    /// Create a new Ruby runtime from WASM module bytes.
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let ruby_slab = new_ruby_slab();
        let js_slab = new_ruby_slab();
        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let imports = build_ruby_imports(&mut store, &env, ruby_slab.clone(), js_slab.clone());
        let instance = Instance::new(&mut store, &module, &imports)?;

        let memory = instance.exports.get_memory("memory")?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());

        // Cache all needed exports (Component Model names include full type signatures)
        let cabi_realloc = instance.exports.get_function(
            "cabi_realloc"
        )?.clone();
        let rb_eval = instance.exports.get_function(
            "rb-eval-string-protect: func(str: string) -> tuple<handle<rb-abi-value>, s32>"
        )?.clone();
        let rb_intern = instance.exports.get_function(
            "rb-intern: func(name: string) -> u32"
        )?.clone();
        let rb_funcallv = instance.exports.get_function(
            "rb-funcallv-protect: func(recv: handle<rb-abi-value>, mid: u32, args: list<handle<rb-abi-value>>) -> tuple<handle<rb-abi-value>, s32>"
        )?.clone();
        let rstring_ptr_fn = instance.exports.get_function(
            "rstring-ptr: func(value: handle<rb-abi-value>) -> string"
        )?.clone();
        let cabi_post_rstring = instance.exports.get_function(
            "cabi_post_rstring-ptr"
        )?.clone();
        let rb_errinfo = instance.exports.get_function(
            "rb-errinfo: func() -> handle<rb-abi-value>"
        )?.clone();
        let rb_clear_errinfo = instance.exports.get_function(
            "rb-clear-errinfo: func() -> ()"
        )?.clone();

        let rt = Self {
            store,
            instance,
            memory,
            env,
            ruby_slab,
            js_slab,
            tools: HashMap::new(),
            cabi_realloc,
            rb_eval,
            rb_intern,
            rb_funcallv,
            rstring_ptr_fn,
            cabi_post_rstring,
            rb_errinfo,
            rb_clear_errinfo,
            to_s_mid: 0,
        };

        Ok(rt)
    }

    /// Cold start: compile + instantiate + init from raw bytes.
    pub fn cold_start(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let mut rt = Self::new(wasm_bytes)?;
        rt.init()?;
        Ok(rt)
    }

    /// Allocate memory in the WASM guest via cabi_realloc and write a string there.
    /// Returns (ptr, len). The string is NOT null-terminated (Component Model uses explicit length).
    fn alloc_string_in_wasm(&mut self, s: &str) -> Result<(i32, i32), Box<dyn Error>> {
        let bytes = s.as_bytes();
        let len = bytes.len() as i32;

        let result = self.cabi_realloc.call(&mut self.store, &[
            Value::I32(0),     // old_ptr = 0 (new allocation)
            Value::I32(0),     // old_size = 0
            Value::I32(1),     // alignment = 1 (bytes)
            Value::I32(len),   // new_size
        ])?;
        let ptr = result[0].unwrap_i32();

        if ptr == 0 {
            return Err("cabi_realloc returned null pointer".into());
        }

        self.memory.view(&self.store).write(ptr as u64, bytes)?;
        Ok((ptr, len))
    }

    /// Evaluate a Ruby code string. Returns (handle, status).
    /// handle is an index into the Ruby VALUE slab.
    /// status is a ruby_tag_type: 0 = success, 6 = exception, 8 = fatal.
    fn eval_ruby(&mut self, code: &str) -> Result<(i32, i32), Box<dyn Error>> {
        let (ptr, len) = self.alloc_string_in_wasm(code)?;

        let result = self.rb_eval.call(&mut self.store, &[
            Value::I32(ptr),
            Value::I32(len),
        ])?;

        // The function returns a POINTER to a memory area containing [handle: i32, status: i32]
        let ret_area_ptr = result[0].unwrap_i32() as u64;

        let mut buf = [0u8; 8];
        self.memory.view(&self.store).read(ret_area_ptr, &mut buf)?;
        let handle = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let status = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

        Ok((handle, status))
    }

    /// Get the raw Ruby VALUE from a handle.
    fn raw_value_of(&self, handle: i32) -> i32 {
        slab_get(&self.ruby_slab, handle)
    }

    /// Convert a Ruby VALUE handle to a Rust string by calling .to_s then rstring-ptr.
    fn value_to_string(&mut self, handle: i32) -> Result<String, Box<dyn Error>> {
        let raw_recv = self.raw_value_of(handle);

        // Create a new handle for the receiver (funcallv needs a handle, not a raw VALUE)
        // We call resource_new_rb-abi-value indirectly: the WASM funcallv export will call
        // our host resource_new for the result. But we also need a handle for the receiver.
        // We can create one by inserting into the slab directly.
        let recv_handle = slab_insert(&self.ruby_slab, raw_recv);

        // Call rb-funcallv-protect(recv_handle, to_s_mid, empty_args_ptr=0, empty_args_len=0)
        let result = self.rb_funcallv.call(&mut self.store, &[
            Value::I32(recv_handle),
            Value::I32(self.to_s_mid as i32),
            Value::I32(0), // args ptr (empty list)
            Value::I32(0), // args len
        ])?;

        // Returns pointer to [result_handle, status]
        let ret_area_ptr = result[0].unwrap_i32() as u64;
        let mut buf = [0u8; 8];
        self.memory.view(&self.store).read(ret_area_ptr, &mut buf)?;
        let str_handle = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let status = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);

        if (status as u32 & 0xF) != RUBY_TAG_NONE {
            return Err(format!("rb_funcallv(to_s) failed with status {}", status).into());
        }

        // Call rstring-ptr(str_handle) — returns pointer to [str_ptr, str_len] area
        let result = self.rstring_ptr_fn.call(&mut self.store, &[
            Value::I32(str_handle),
        ])?;
        let str_area_ptr = result[0].unwrap_i32() as u64;

        let mut str_buf = [0u8; 8];
        self.memory.view(&self.store).read(str_area_ptr, &mut str_buf)?;
        let str_ptr = i32::from_le_bytes([str_buf[0], str_buf[1], str_buf[2], str_buf[3]]) as u64;
        let str_len = i32::from_le_bytes([str_buf[4], str_buf[5], str_buf[6], str_buf[7]]) as usize;

        // Read the actual string from WASM memory
        let mut result_bytes = vec![0u8; str_len];
        self.memory.view(&self.store).read(str_ptr, &mut result_bytes)?;
        let result_string = String::from_utf8_lossy(&result_bytes).to_string();

        // Call cabi_post_rstring-ptr to release the return area
        self.cabi_post_rstring.call(&mut self.store, &[
            Value::I32(str_area_ptr as i32),
        ])?;

        Ok(result_string)
    }

    /// Get the error info string from Ruby's exception system.
    fn get_error_string(&mut self) -> Result<String, Box<dyn Error>> {
        // rb-errinfo returns a handle directly (single-value return, not return-area)
        let result = self.rb_errinfo.call(&mut self.store, &[])?;
        let err_handle = result[0].unwrap_i32();

        if err_handle == 0 {
            return Ok("Unknown Ruby error".to_string());
        }

        // Convert the error handle to a string via to_s
        let err_str = self.value_to_string(err_handle)?;

        // Clear the error
        self.rb_clear_errinfo.call(&mut self.store, &[])?;

        Ok(err_str)
    }
}

impl WasmRuntime for RubyRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        // Call ruby-init with args: ["ruby.wasm\0", "-EUTF-8\0", "-e_=0\0"]
        let args = ["ruby.wasm", "-EUTF-8", "-e_=0"];

        // Encode each arg as null-terminated string via cabi_realloc
        let mut arg_ptrs: Vec<(i32, i32)> = Vec::new();
        for arg in &args {
            let null_term = format!("{}\0", arg);
            let bytes = null_term.as_bytes();
            let result = self.cabi_realloc.call(&mut self.store, &[
                Value::I32(0),
                Value::I32(0),
                Value::I32(1),
                Value::I32(bytes.len() as i32),
            ])?;
            let ptr = result[0].unwrap_i32();
            self.memory.view(&self.store).write(ptr as u64, bytes)?;
            arg_ptrs.push((ptr, bytes.len() as i32 - 1)); // len excludes null terminator
        }

        // Allocate the list of (ptr, len) pairs
        let list_result = self.cabi_realloc.call(&mut self.store, &[
            Value::I32(0),
            Value::I32(0),
            Value::I32(4),     // alignment 4 for i32 pairs
            Value::I32(arg_ptrs.len() as i32 * 8), // each pair is 2×i32 = 8 bytes
        ])?;
        let list_ptr = list_result[0].unwrap_i32();

        // Write each (ptr, len) pair — Component Model encodes string as (ptr, len) in memory
        for (i, (str_ptr, str_len)) in arg_ptrs.iter().enumerate() {
            let offset = list_ptr as u64 + (i as u64 * 8);
            self.memory.view(&self.store).write(offset, &str_ptr.to_le_bytes())?;
            self.memory.view(&self.store).write(offset + 4, &str_len.to_le_bytes())?;
        }

        // Call ruby-init
        let init_fn = self.instance.exports.get_function(
            "ruby-init: func(args: list<string>) -> ()"
        )?;
        init_fn.call(&mut self.store, &[
            Value::I32(list_ptr),
            Value::I32(arg_ptrs.len() as i32),
        ])?;

        // Cache the method ID for "to_s"
        let (to_s_ptr, to_s_len) = self.alloc_string_in_wasm("to_s")?;
        let result = self.rb_intern.call(&mut self.store, &[
            Value::I32(to_s_ptr),
            Value::I32(to_s_len),
        ])?;
        self.to_s_mid = result[0].unwrap_i32() as u32;

        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // Evaluate the Ruby source to define the method/class
        let (_handle, status) = self.eval_ruby(source)?;
        let tag = status as u32 & 0xF;
        if tag != RUBY_TAG_NONE {
            let err = if tag == RUBY_TAG_RAISE || tag == RUBY_TAG_FATAL {
                self.get_error_string().unwrap_or_else(|_| format!("status {}", status))
            } else {
                format!("Ruby init returned status {}", status)
            };
            return Err(format!("Failed to register Ruby tool '{}': {}", name, err).into());
        }

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Ruby tool: {}", name).into());
        }

        // Build a Ruby expression that calls the tool function with parsed JSON args.
        // The tool source defines a function with the tool's name.
        let escaped_json = args_json
            .replace('\\', "\\\\")
            .replace('\'', "\\\'");

        let code = format!(
            "require 'json'; JSON.generate({name}(JSON.parse('{escaped_json}')))",
            name = name,
            escaped_json = escaped_json,
        );

        let (handle, status) = self.eval_ruby(&code)?;
        let tag = status as u32 & 0xF;

        if tag == RUBY_TAG_NONE {
            let result = self.value_to_string(handle)?;
            Ok(result)
        } else if tag == RUBY_TAG_RAISE || tag == RUBY_TAG_FATAL {
            let err = self.get_error_string()?;
            Err(format!("Ruby error in tool '{}': {}", name, err).into())
        } else {
            Err(format!("Ruby tool '{}' returned status {}", name, status).into())
        }
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        // No explicit shutdown export in the Ruby WASM Component Model.
        // Resources are freed when the WASM instance is dropped.
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
    #[ignore]
    fn test_ruby_init() {
        let wasm = std::fs::read(wasm_path()).expect("Ruby WASM not found");
        let mut rt = RubyRuntime::cold_start(&wasm).expect("cold start failed");
        assert_eq!(rt.to_s_mid, 0); // to_s_mid should be non-zero after init
        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_ruby_eval_simple() {
        let wasm = std::fs::read(wasm_path()).expect("Ruby WASM not found");
        let mut rt = RubyRuntime::cold_start(&wasm).expect("cold start failed");

        let (handle, status) = rt.eval_ruby("1 + 2").expect("eval failed");
        assert_eq!(status, 0, "Expected success status");
        assert_ne!(handle, 0, "Expected non-zero handle");

        let result = rt.value_to_string(handle).expect("to_s failed");
        assert_eq!(result, "3");

        rt.destroy().unwrap();
    }
}
