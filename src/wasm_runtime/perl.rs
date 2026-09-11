//! Perl/WASM runtime — Perl tool execution via Perl interpreter compiled to WASM.
//!
//! Uses a WASI reactor build of Perl running inside Wasmer.
//! Implements the `WasmRuntime` trait for uniform cross-language tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Module, Store, Value};

use super::wasi::{WasiEnv, build_wasi_imports};
use super::WasmRuntime;

const EXEC_SLOT: u64 = 512 * 1024;
const ARGS_SLOT: u64 = 4 * 1024;
const NAME_SLOT: u64 = 8 * 1024;

pub struct PerlRuntime {
    store: Store,
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    tools: HashMap<String, String>,
    call_tool_fn: Option<Function>,
}

impl PerlRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes)?;
        let mut store = Store::new(engine);

        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
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
        let exec_fn = self.instance.exports.get_function("perl_wasi_exec")?;
        let result = exec_fn.call(&mut self.store, &[Value::I32(EXEC_SLOT as i32), Value::I32(data.len() as i32)])?;
        Ok(result[0].unwrap_i32())
    }

    fn get_output(&mut self) -> Result<String, Box<dyn Error>> {
        let get_output_fn = self.instance.exports.get_function("perl_wasi_get_output")?;
        let get_len_fn = self.instance.exports.get_function("perl_wasi_get_output_len")?;

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
            return Err(format!("Perl error: {}", output.trim()).into());
        }
        Ok(output)
    }
}

impl WasmRuntime for PerlRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let init_fn = self.instance.exports.get_function("perl_wasi_init")?
            .typed::<(), i32>(&self.store)?;
        let result = init_fn.call(&mut self.store)?;
        if result != 0 {
            return Err(format!("perl_wasi_init() failed with code {}", result).into());
        }
        self.call_tool_fn = Some(self.instance.exports.get_function("perl_wasi_call_tool")?.clone());
        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        let source_bytes = source.as_bytes();
        self.memory.view(&self.store).write(EXEC_SLOT, source_bytes)?;

        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        let register_fn = self.instance.exports.get_function("perl_wasi_register_tool")?;
        let result = register_fn.call(&mut self.store, &[
            Value::I32(NAME_SLOT as i32),
            Value::I32(name_bytes.len() as i32),
            Value::I32(EXEC_SLOT as i32),
            Value::I32(source_bytes.len() as i32),
        ])?;

        if result[0].unwrap_i32() != 0 {
            let output = self.get_output()?;
            return Err(format!("Failed to register Perl tool: {}", output.trim()).into());
        }

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Perl tool: {}", name).into());
        }

        let args_bytes = args_json.as_bytes();
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(ARGS_SLOT, args_bytes)?;
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

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
            return Err(format!("Perl tool error: {}", output.trim()).into());
        }

        Ok(output.trim().to_string())
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        let destroy_fn = self.instance.exports.get_function("perl_wasi_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
    }

    fn language(&self) -> &str {
        "perl"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/perl_wasm/perl.wasm");
        p
    }

    #[test]
    #[ignore]
    fn test_perl_basic_exec() {
        let wasm = std::fs::read(wasm_path()).expect("Perl WASM not found");
        let mut rt = PerlRuntime::cold_start(&wasm).expect("cold start failed");

        let output = rt.exec_and_get_output("print 'hello from perl';").expect("exec failed");
        assert_eq!(output.trim(), "hello from perl");

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_perl_tool_roundtrip() {
        let wasm = std::fs::read(wasm_path()).expect("Perl WASM not found");
        let mut rt = PerlRuntime::cold_start(&wasm).expect("cold start failed");

        let source = "sub greet { my ($args) = @_; return { message => 'Hello, ' . $args->{name} . '!' }; }";
        rt.register_tool("greet", source).expect("register failed");

        let result = rt.call_tool("greet", r#"{"name": "Perl"}"#).expect("call failed");
        assert!(result.contains("Hello, Perl!"), "unexpected result: {}", result);

        rt.destroy().unwrap();
    }
}
