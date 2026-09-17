//! Java/Kotlin WASM runtime — JVM tool execution via Java VM compiled to WASM.
//!
//! Uses a WASI reactor build of a lightweight JVM (e.g. CheerpJ or TeaVM) running
//! inside Wasmer. Implements the `WasmRuntime` trait for uniform cross-language
//! tool execution.

use std::collections::HashMap;
use std::error::Error;
use wasmer::{Function, FunctionEnv, Instance, Memory, Store, Value};

use super::wasi::{build_wasi_imports, WasiEnv};
use super::{create_wasm_instance, WasmRuntime, WasmRuntimeConfig};

const EXEC_SLOT: u64 = 512 * 1024;
const ARGS_SLOT: u64 = 4 * 1024;
const NAME_SLOT: u64 = 8 * 1024;

pub struct JavaRuntime {
    store: Store,
    instance: Instance,
    memory: Memory,
    #[allow(dead_code)]
    env: FunctionEnv<WasiEnv>,
    tools: HashMap<String, String>,
    call_tool_fn: Option<Function>,
    call_tool_binary_fn: Option<Function>,
}

impl JavaRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let (store, instance, memory, env) = create_wasm_instance(WasmRuntimeConfig {
            wasm_bytes,
            import_builder: Box::new(build_wasi_imports),
            extra_memory_pages: 1,
            instruction_limit: super::instruction_limit(),
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
        let data = src.as_bytes();
        self.memory.view(&self.store).write(EXEC_SLOT, data)?;
        let exec_fn = self.instance.exports.get_function("java_wasi_exec")?;
        let result = exec_fn.call(
            &mut self.store,
            &[Value::I32(EXEC_SLOT as i32), Value::I32(data.len() as i32)],
        )?;
        Ok(result[0].unwrap_i32())
    }

    fn get_output(&mut self) -> Result<String, Box<dyn Error>> {
        let get_output_fn = self.instance.exports.get_function("java_wasi_get_output")?;
        let get_len_fn = self
            .instance
            .exports
            .get_function("java_wasi_get_output_len")?;

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
            return Err(format!("Java error: {}", output.trim()).into());
        }
        Ok(output)
    }
}

impl WasmRuntime for JavaRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        let init_fn = self
            .instance
            .exports
            .get_function("java_wasi_init")?
            .typed::<(), i32>(&self.store)?;
        let result = init_fn.call(&mut self.store)?;
        if result != 0 {
            return Err(format!("java_wasi_init() failed with code {}", result).into());
        }
        self.call_tool_fn = Some(
            self.instance
                .exports
                .get_function("java_wasi_call_tool")?
                .clone(),
        );
        // Load binary protocol function if available (optional, for optimized path)
        self.call_tool_binary_fn = self
            .instance
            .exports
            .get_function("java_wasi_call_tool_binary")
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

        let source_bytes = source.as_bytes();
        self.memory
            .view(&self.store)
            .write(EXEC_SLOT, source_bytes)?;

        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

        let register_fn = self
            .instance
            .exports
            .get_function("java_wasi_register_tool")?;
        let result = register_fn.call(
            &mut self.store,
            &[
                Value::I32(NAME_SLOT as i32),
                Value::I32(name_bytes.len() as i32),
                Value::I32(EXEC_SLOT as i32),
                Value::I32(source_bytes.len() as i32),
            ],
        )?;

        if result[0].unwrap_i32() != 0 {
            let output = self.get_output()?;
            return Err(format!("Failed to register Java tool: {}", output.trim()).into());
        }

        self.tools.insert(name.to_string(), source.to_string());
        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Java tool: {}", name).into());
        }

        let args_bytes = args_json.as_bytes();
        let name_bytes = name.as_bytes();
        self.memory.view(&self.store).write(ARGS_SLOT, args_bytes)?;
        self.memory.view(&self.store).write(NAME_SLOT, name_bytes)?;

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
            return Err(format!("Java tool error: {}", output.trim()).into());
        }

        Ok(output.trim().to_string())
    }

    fn call_tool_binary(&mut self, name: &str, args_tlv: &[u8]) -> Result<String, Box<dyn Error>> {
        if !self.tools.contains_key(name) {
            return Err(format!("Unknown Java tool: {}", name).into());
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

        // Single WASI call: java_wasi_call_tool_binary(tlv_ptr, tlv_len, name_ptr, name_len)
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
            return Err(format!("Java tool error: {}", output.trim()).into());
        }

        Ok(output.trim().to_string())
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        let destroy_fn = self.instance.exports.get_function("java_wasi_destroy")?;
        destroy_fn.call(&mut self.store, &[])?;
        Ok(())
    }

    fn reset_instruction_budget(&mut self) {
        super::reset_instruction_budget(&mut self.store, &self.instance);
    }

    fn language(&self) -> &str {
        "java"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/java_wasm/java.wasm");
        p
    }

    #[test]
    #[ignore]
    fn test_java_basic_exec() {
        let wasm = std::fs::read(wasm_path()).expect("Java WASM not found");
        let mut rt = JavaRuntime::cold_start(&wasm).expect("cold start failed");

        let output = rt
            .exec_and_get_output("System.out.println(\"hello from java\");")
            .expect("exec failed");
        assert_eq!(output.trim(), "hello from java");

        rt.destroy().unwrap();
    }

    #[test]
    #[ignore]
    fn test_java_tool_roundtrip() {
        let wasm = std::fs::read(wasm_path()).expect("Java WASM not found");
        let mut rt = JavaRuntime::cold_start(&wasm).expect("cold start failed");

        let source = "set_tool_result(\"Hello, \" + name + \"!\");";
        rt.register_tool("greet", source).expect("register failed");

        let result = rt
            .call_tool("greet", r#"{"name": "Java"}"#)
            .expect("call failed");
        assert!(
            result.contains("Hello, Java!"),
            "unexpected result: {}",
            result
        );

        rt.destroy().unwrap();
    }
}
