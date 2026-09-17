//! Go WASM runtime using TinyGo compilation.
//!
//! This runtime compiles Go source code to WASM at load time using TinyGo,
//! then executes it via the Wasmer engine. This enables dynamic tool registration
//! without server restarts - just like Python/Lua/JS interpreted runtimes.

use std::collections::HashMap;
use std::error::Error;
use std::path::PathBuf;
use std::process::Command;
use wasmer::{FunctionEnv, Instance, Module, Store};

use super::wasi::{build_wasi_imports, WasiEnv};
use super::{compile_module_cached, WasmRuntime};

/// Cached compiled Go WASM modules per tool.
/// The engine is kept so per-call Stores can share the module's engine
/// (Wasmer rejects instantiation when they differ).
struct CompiledGoTool {
    module: Module,
    engine: wasmer::Engine,
}

pub struct GoWasmRuntime {
    _wasm_path: String,
    tools: HashMap<String, String>, // tool_name -> source_code
    compiled_tools: HashMap<String, CompiledGoTool>,
}

impl GoWasmRuntime {
    pub fn new(wasm_path: &str) -> Self {
        Self {
            _wasm_path: wasm_path.to_string(),
            tools: HashMap::new(),
            compiled_tools: HashMap::new(),
        }
    }

    /// Compile Go source to WASM using TinyGo
    fn compile_go_to_wasm(&self, source: &str, tool_name: &str) -> Result<Vec<u8>, String> {
        // Create temporary directory for compilation
        let temp_dir = std::env::temp_dir().join(format!("velocity_go_{}", tool_name));
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| format!("Failed to create temp dir: {}", e))?;

        let go_file = temp_dir.join("tool.go");
        let wasm_file = temp_dir.join("tool.wasm");

        // Write Go source to file with proper package and main function wrapper
        let full_source = format!(
            r#"
package main

import (
    "encoding/json"
    "unsafe"
)

{}

//export prepare_call
func prepare_call() {{}}

//export tool_execute
func tool_execute(ptr int32, length int32) int64 {{
    inputBytes := unsafe.Slice((*byte)(unsafe.Pointer(uintptr(ptr))), length)
    var payloadMap map[string]interface{{}}
    if err := json.Unmarshal(inputBytes, &payloadMap); err != nil {{
        return encodeResult(map[string]string{{"error": "invalid json: " + err.Error()}})
    }}
    
    toolName, ok := payloadMap["_tool_name"].(string)
    if !ok {{
        toolName = "{}"
    }}
    delete(payloadMap, "_tool_name")
    
    // Dispatch based on tool name - call the user's function directly
    switch toolName {{
    case "{}":
        result := {}(payloadMap)
        return encodeResult(result)
    default:
        return encodeResult(map[string]string{{"error": "unknown tool: " + toolName}})
    }}
}}

func encodeResult(v interface{{}}) int64 {{
    data, err := json.Marshal(v)
    if err != nil {{
        data = []byte(`{{"error":"marshal failed"}}`)
    }}
    pinned := make([]byte, len(data))
    copy(pinned, data)
    ptr := int64(uintptr(unsafe.Pointer(&pinned[0])))
    length := int64(len(pinned))
    return (ptr << 32) | length
}}

func main() {{}}
"#,
            source,
            tool_name,
            tool_name,
            get_main_function_name(source)
        );

        std::fs::write(&go_file, full_source)
            .map_err(|e| format!("Failed to write Go source: {}", e))?;

        // Find TinyGo binary
        let tinygo_bin = find_tinygo()?;

        // Compile with TinyGo
        let output = Command::new(&tinygo_bin)
            .arg("build")
            .arg("-target=wasi")
            .arg("-o")
            .arg(&wasm_file)
            .arg(&go_file)
            .output()
            .map_err(|e| {
                format!(
                    "Failed to run TinyGo: {}. Is it installed? Try: scoop install tinygo",
                    e
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("TinyGo compilation failed: {}", stderr));
        }

        // Read compiled WASM
        let wasm_bytes = std::fs::read(&wasm_file)
            .map_err(|e| format!("Failed to read compiled WASM: {}", e))?;

        // Cleanup temp files
        let _ = std::fs::remove_dir_all(&temp_dir);

        Ok(wasm_bytes)
    }
}

/// Extract the main function name from Go source
fn get_main_function_name(source: &str) -> &str {
    // Simple heuristic: look for "func <Name>(" pattern
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("func ") && trimmed.contains('(') {
            if let Some(start) = trimmed.find("func ") {
                let after_func = &trimmed[start + 5..];
                if let Some(end) = after_func.find('(') {
                    return after_func[..end].trim();
                }
            }
        }
    }
    "MainFunc" // Default fallback
}

impl WasmRuntime for GoWasmRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        // No initialization needed for Go runtime
        Ok(())
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // Check if source changed (skip recompilation if unchanged)
        if let Some(existing) = self.tools.get(name) {
            if existing == source {
                return Ok(()); // No change, skip recompilation
            }
        }

        // "source" can be either:
        // 1. A file path to a pre-compiled .wasm module (like the Rust runtime)
        // 2. Inline Go source, compiled to WASM via TinyGo at load time
        let wasm_bytes = if std::path::Path::new(source).exists() {
            std::fs::read(source)?
        } else {
            self.compile_go_to_wasm(source, name)
                .map_err(|e| format!("Compilation failed: {}", e))?
        };

        // Create WASM module (using shared cache for faster repeat loads)
        let engine = super::build_metered_engine(super::instruction_limit());
        let module = compile_module_cached(&engine, &wasm_bytes, super::instruction_limit())
            .map_err(|e| format!("Failed to compile WASM module: {}", e))?;

        // Cache the compiled module
        self.compiled_tools
            .insert(name.to_string(), CompiledGoTool { module, engine });
        self.tools.insert(name.to_string(), source.to_string());

        Ok(())
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        // Get cached module
        let tool = self
            .compiled_tools
            .get(name)
            .ok_or_else(|| format!("Tool '{}' not registered or compilation failed", name))?;

        // Clone the module for this execution
        let module = tool.module.clone();

        // Create a fresh store and instance for each call (isolated execution).
        // The store must share the module's engine or Wasmer rejects instantiation.
        let mut store = Store::new(tool.engine.clone());

        // TinyGo WASI modules import wasi_snapshot_preview1 — instantiation
        // fails without these imports.
        let env = FunctionEnv::new(&mut store, WasiEnv::new());
        let imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &imports)?;

        let memory = instance.exports.get_memory("memory")?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());

        // _start initializes the TinyGo runtime (heap, GC) — required before malloc
        if let Ok(start_fn) = instance.exports.get_function("_start") {
            let _ = start_fn.call(&mut store, &[]);
        }

        // No-op for Go, exported for ABI compatibility with the Rust tool
        if let Ok(prepare_call) = instance.exports.get_function("prepare_call") {
            let _ = prepare_call.call(&mut store, &[]);
        }

        // Allocate input JSON in guest memory: malloc when exported (TinyGo
        // always exports it), else fall back to a fixed offset past the data
        // segment for modules without malloc.
        let input_bytes = args_json.as_bytes();
        let input_len = input_bytes.len() as i32;

        let memory_offset: u64 = if let Ok(malloc_fn) = instance.exports.get_function("malloc") {
            let results = malloc_fn.call(&mut store, &[wasmer::Value::I32(input_len)])?;
            results[0].unwrap_i32() as u32 as u64
        } else {
            let offset = 65536u64;
            let current_pages = memory.view(&store).size();
            let needed_pages = ((offset + input_bytes.len() as u64) / 65536 + 1) as u32;
            if current_pages.0 < needed_pages {
                memory.grow(&mut store, wasmer::Pages(needed_pages - current_pages.0))?;
            }
            offset
        };

        // Write input to WASM memory
        memory.view(&store).write(memory_offset, input_bytes)?;

        // Call tool_execute(ptr, length)
        let tool_execute = instance.exports.get_function("tool_execute")?;
        let result_value = tool_execute.call(
            &mut store,
            &[
                wasmer::Value::I32(memory_offset as i32),
                wasmer::Value::I32(input_len),
            ],
        )?;

        // Extract result pointer and length from i64 return value
        let result_i64 = result_value[0].unwrap_i64();
        let result_ptr = (result_i64 >> 32) as u32;
        let result_len = (result_i64 & 0xFFFFFFFF) as u32;

        // Read result from WASM memory
        let mut result_buf = vec![0u8; result_len as usize];
        memory
            .view(&store)
            .read(result_ptr as u64, &mut result_buf)?;
        let result_str = String::from_utf8_lossy(&result_buf).to_string();

        Ok(result_str)
    }

    fn call_tool_binary(
        &mut self,
        name: &str,
        args_tlv: &[u8],
    ) -> Result<String, Box<dyn Error>> {
        use crate::protocol::nda_native::decode_json_value;
        let (value, _) = decode_json_value(args_tlv)?;
        let json_str = serde_json::to_string(&value)?;
        self.call_tool(name, &json_str)
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        self.tools.clear();
        self.compiled_tools.clear();
        Ok(())
    }

    fn language(&self) -> &str {
        "go"
    }
}

/// Find TinyGo binary in PATH or common locations
fn find_tinygo() -> Result<PathBuf, String> {
    // Try common installation locations on Windows
    let common_paths = vec![
        "C:\\Program Files\\TinyGo\\bin\\tinygo.exe",
        "C:\\Users\\ian\\scoop\\apps\\tinygo\\current\\bin\\tinygo.exe",
        "C:\\Users\\ian\\AppData\\Local\\Microsoft\\WinGet\\Packages\\tinygo-org.tinygo_*\\tinygo.exe",
    ];

    for path in common_paths {
        if std::path::Path::new(path).exists() {
            return Ok(PathBuf::from(path));
        }
    }

    // Try PATH (using where command on Windows)
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("where").arg("tinygo").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(path) = stdout.lines().next() {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        return Ok(PathBuf::from(trimmed));
                    }
                }
            }
        }
    }

    // Unix-like systems
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(output) = Command::new("which").arg("tinygo").output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if let Some(path) = stdout.lines().next() {
                    let trimmed = path.trim();
                    if !trimmed.is_empty() {
                        return Ok(PathBuf::from(trimmed));
                    }
                }
            }
        }
    }

    Err("TinyGo not found. Install from https://tinygo.org/getting-started/install/".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires TinyGo installation
    fn test_compile_go_to_wasm() {
        let mut runtime = GoWasmRuntime::new("dummy");
        let source = r#"
func AnalyzeLogsGo(args map[string]interface{}) map[string]interface{} {
    return map[string]interface{}{"status": "ok"}
}
"#;
        let result = runtime.register_tool("test_tool", source);
        assert!(result.is_ok(), "Registration should succeed: {:?}", result);
    }
}
