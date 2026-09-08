//! Plugin system for dynamic tool loading.
//!
//! This module provides a plugin system that allows loading tools dynamically
//! from external sources without recompiling the server.
//!
//! # Plugin Manifest Format
//!
//! Plugins are defined by JSON manifest files with the following structure:
//!
//! ```json
//! {
//!   "name": "my-plugin",
//!   "version": "1.0.0",
//!   "tools": [
//!     {
//!       "name": "my_tool",
//!       "description": "A custom tool",
//!       "inputSchema": {
//!         "type": "object",
//!         "properties": {
//!           "param1": { "type": "string" }
//!         },
//!         "required": ["param1"]
//!       },
//!       "executor": {
//!         "type": "process",
//!         "command": "python",
//!         "args": ["my_tool.py", "--param1", "{{param1}}"]
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! # Executor Types
//!
//! Currently supported executor types:
//! - `process`: Execute an external process
//!
//! # Template Variables
//!
//! Executor arguments support template variables using `{{variable_name}}` syntax.
//! Variables are replaced with the corresponding tool argument values.

pub mod marketplace;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use tracing::{error, info};

/// Plugin manifest defining tools and their executors.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Plugin name
    pub name: String,
    /// Plugin version
    pub version: String,
    /// Tools provided by this plugin
    pub tools: Vec<PluginTool>,
}

/// A tool definition from a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginTool {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// JSON Schema for tool input
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    /// Executor configuration
    pub executor: PluginExecutor,
}

/// Executor configuration for a plugin tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginExecutor {
    /// Executor type: "process" for external commands, "wasm" for in-process WASM execution
    pub executor_type: String,
    /// Command to execute (for "process" type)
    #[serde(default)]
    pub command: String,
    /// Command arguments (supports template variables, for "process" type)
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory for the command
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,

    /// Language runtime for WASM executor (e.g., "javascript", "python", "lua", "go")
    #[serde(default)]
    pub language: Option<String>,
    /// Inline source code for WASM executor (evaluated in the language runtime)
    #[serde(default)]
    pub source: Option<String>,
    /// Path to a source file (.js, .py, .lua) or compiled WASM module (.wasm)
    #[serde(default)]
    pub source_file: Option<String>,
    /// Handler function name to invoke for WASM executor
    #[serde(default)]
    pub handler_function: Option<String>,
}

fn default_timeout() -> u64 {
    30
}

/// Loaded plugin with manifest and path.
#[derive(Debug, Clone)]
pub struct LoadedPlugin {
    /// Plugin manifest
    pub manifest: PluginManifest,
    /// Path to the plugin manifest file
    pub path: PathBuf,
}

/// Load plugins from a directory.
///
/// Scans the directory for `.json` files and attempts to load each as a plugin manifest.
///
/// # Arguments
///
/// * `plugin_dir` - Directory to scan for plugin manifests
///
/// # Returns
///
/// Vector of successfully loaded plugins
pub fn load_plugins_from_directory(plugin_dir: &Path) -> Vec<LoadedPlugin> {
    let mut plugins = Vec::new();

    if !plugin_dir.exists() {
        info!(?plugin_dir, "Plugin directory does not exist, skipping plugin loading");
        return plugins;
    }

    if !plugin_dir.is_dir() {
        error!(?plugin_dir, "Plugin path is not a directory");
        return plugins;
    }

    let entries = match std::fs::read_dir(plugin_dir) {
        Ok(entries) => entries,
        Err(e) => {
            error!(?plugin_dir, error = %e, "Failed to read plugin directory");
            return plugins;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                error!(error = %e, "Failed to read directory entry");
                continue;
            }
        };

        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        match load_plugin_manifest(&path) {
            Ok(manifest) => {
                info!(name = %manifest.name, version = %manifest.version, ?path, "Loaded plugin");
                plugins.push(LoadedPlugin { manifest, path });
            }
            Err(e) => {
                error!(?path, error = %e, "Failed to load plugin manifest");
            }
        }
    }

    info!(count = plugins.len(), "Loaded plugins from directory");
    plugins
}

/// Load a plugin manifest from a file.
///
/// # Arguments
///
/// * `path` - Path to the plugin manifest file
///
/// # Returns
///
/// The loaded plugin manifest or an error
pub fn load_plugin_manifest(path: &Path) -> Result<PluginManifest, String> {
    let meta = std::fs::metadata(path)
        .map_err(|e| format!("Failed to stat plugin manifest: {}", e))?;
    if meta.len() > 1_048_576 {
        return Err("Plugin manifest exceeds 1MB limit".to_string());
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read plugin manifest: {}", e))?;

    let manifest: PluginManifest = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse plugin manifest: {}", e))?;

    // Validate manifest
    if manifest.name.is_empty() {
        return Err("Plugin name cannot be empty".to_string());
    }

    if manifest.tools.is_empty() {
        return Err("Plugin must define at least one tool".to_string());
    }

    for tool in &manifest.tools {
        if tool.name.is_empty() {
            return Err("Tool name cannot be empty".to_string());
        }
        match tool.executor.executor_type.as_str() {
            "process" => {
                if tool.executor.command.is_empty() {
                    return Err(format!("Tool '{}' with process executor must specify a command", tool.name));
                }
            }
            "wasm" => {
                if tool.executor.language.is_none() {
                    return Err(format!("Tool '{}' with wasm executor must specify a language", tool.name));
                }
                if tool.executor.source.is_none() && tool.executor.source_file.is_none() {
                    return Err(format!("Tool '{}' with wasm executor must specify source or source_file", tool.name));
                }
            }
            other => {
                return Err(format!("Unsupported executor type: {}", other));
            }
        }
    }

    Ok(manifest)
}

/// Execute a plugin tool.
///
/// # Arguments
///
/// * `tool` - The plugin tool to execute
/// * `arguments` - Tool arguments
///
/// # Returns
///
/// Tool execution result as a string or an error
pub fn execute_plugin_tool(tool: &PluginTool, arguments: &Value) -> Result<String, String> {
    let executor = &tool.executor;

    match executor.executor_type.as_str() {
        "wasm" => execute_wasm_plugin_tool(tool, arguments),
        "process" => execute_process_plugin_tool(tool, arguments),
        other => Err(format!("Unsupported executor type: {}", other)),
    }
}

/// Execute a process-based plugin tool.
fn execute_process_plugin_tool(tool: &PluginTool, arguments: &Value) -> Result<String, String> {
    let executor = &tool.executor;

    // Detect if the plugin invokes a shell interpreter — arguments become shell commands
    let shell_interpreters = ["sh", "bash", "cmd", "cmd.exe", "/bin/sh", "/bin/bash", "powershell", "powershell.exe"];
    let cmd_lower = executor.command.to_lowercase();
    let is_shell_command = shell_interpreters.iter().any(|s| cmd_lower == *s || cmd_lower.ends_with(s))
        || executor.args.iter().any(|a| a == "-c" || a == "/C");

    // Build command with template variable substitution and validation
    let args: Vec<String> = executor.args.iter().map(|arg| {
        substitute_template_variables(arg, arguments)
    }).collect();

    // Validate substituted arguments
    for (i, arg) in args.iter().enumerate() {
        validate_plugin_argument(arg, is_shell_command, i)?;
    }

    let mut cmd = Command::new(&executor.command);
    cmd.args(&args);

    if let Some(working_dir) = &executor.working_dir {
        cmd.current_dir(working_dir);
    }

    for (key, value) in &executor.env {
        cmd.env(key, value);
    }

    // Execute the command with timeout
    let timeout_secs = executor.timeout.min(300);
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to spawn plugin tool: {}", e))?;

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(out) = child.stdout {
                    use std::io::Read;
                    if let Err(e) = out.take(1_048_576).read_to_end(&mut stdout) {
                        tracing::warn!(error = %e, "Failed to read plugin stdout");
                    }
                }
                if let Some(err) = child.stderr {
                    use std::io::Read;
                    if let Err(e) = err.take(262_144).read_to_end(&mut stderr) {
                        tracing::warn!(error = %e, "Failed to read plugin stderr");
                    }
                }
                if !status.success() {
                    let stderr_str = String::from_utf8_lossy(&stderr);
                    return Err(format!("Plugin tool execution failed: {}", stderr_str));
                }
                let stdout_str = String::from_utf8_lossy(&stdout);
                return Ok(stdout_str.to_string());
            }
            Ok(None) => {
                if start.elapsed().as_secs() > timeout_secs {
                    if let Err(e) = child.kill() {
                        tracing::debug!(error = %e, "Failed to kill timed-out plugin process");
                    }
                    return Err(format!("Plugin tool timed out after {} seconds", timeout_secs));
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(e) => {
                if let Err(ke) = child.kill() {
                    tracing::debug!(error = %ke, "Failed to kill plugin process after wait error");
                }
                return Err(format!("Failed to wait for plugin tool: {}", e));
            }
        }
    }
}

static WASM_RUNTIME_CACHE: std::sync::LazyLock<Mutex<HashMap<String, Box<dyn crate::wasm_runtime::WasmRuntime>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn execute_wasm_plugin_tool(tool: &PluginTool, arguments: &Value) -> Result<String, String> {
    let executor = &tool.executor;
    let language = executor.language.as_deref()
        .ok_or_else(|| "WASM executor requires 'language' field".to_string())?;

    let source = resolve_wasm_source(executor)?;

    match language {
        "go" => execute_standalone_wasm_tool(executor, arguments),
        _ => {
            let mut cache = WASM_RUNTIME_CACHE.lock()
                .map_err(|e| format!("WASM runtime cache poisoned: {}", e))?;

            if !cache.contains_key(language) {
                let runtime = create_wasm_runtime(language)?;
                cache.insert(language.to_string(), runtime);
            }

            let runtime = cache.get_mut(language)
                .ok_or_else(|| format!("No runtime for language: {}", language))?;

            runtime.register_tool(&tool.name, &source)
                .map_err(|e| format!("Failed to register WASM tool '{}': {}", tool.name, e))?;

            let args_json = serde_json::to_string(arguments)
                .map_err(|e| format!("Failed to serialize arguments: {}", e))?;

            runtime.call_tool(&tool.name, &args_json)
                .map_err(|e| format!("WASM tool '{}' execution failed: {}", tool.name, e))
        }
    }
}

fn resolve_wasm_source(executor: &PluginExecutor) -> Result<String, String> {
    if let Some(source) = &executor.source {
        return Ok(source.clone());
    }
    if let Some(source_file) = &executor.source_file {
        let path = Path::new(source_file);
        if !path.exists() {
            return Err(format!("WASM source file not found: {}", source_file));
        }
        std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read source file '{}': {}", source_file, e))
    } else {
        Err("WASM executor requires 'source' or 'source_file'".to_string())
    }
}

fn create_wasm_runtime(language: &str) -> Result<Box<dyn crate::wasm_runtime::WasmRuntime>, String> {
    let (wasm_path, lang_name): (&str, &str) = match language {
        "javascript" => ("bench_tools/quickjs_wasm/quickjs.wasm", "javascript"),
        "python" => ("bench_tools/micropython_wasm/wasi-reactor/build/micropython.wasm", "python"),
        "lua" => ("bench_tools/lua_wasm/lua.wasm", "lua"),
        _ => return Err(format!("Unsupported WASM language: {}", language)),
    };

    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| format!("Failed to read WASM module '{}': {}", wasm_path, e))?;

    let mut runtime: Box<dyn crate::wasm_runtime::WasmRuntime> = match lang_name {
        "javascript" => Box::new(crate::wasm_runtime::quickjs::QuickJsRuntime::new(&wasm_bytes)
            .map_err(|e| format!("QuickJS init failed: {}", e))?),
        "python" => Box::new(crate::wasm_runtime::micropython::MicroPythonRuntime::new(&wasm_bytes)
            .map_err(|e| format!("MicroPython init failed: {}", e))?),
        "lua" => Box::new(crate::wasm_runtime::lua::LuaRuntime::new(&wasm_bytes)
            .map_err(|e| format!("Lua init failed: {}", e))?),
        _ => unreachable!(),
    };

    runtime.init()
        .map_err(|e| format!("WASM runtime init failed for '{}': {}", language, e))?;

    info!(language = %language, wasm_path = %wasm_path, "Initialized WASM runtime for plugin");
    Ok(runtime)
}

fn execute_standalone_wasm_tool(executor: &PluginExecutor, arguments: &Value) -> Result<String, String> {
    use wasmer::{FunctionEnv, Instance, Module, Store, Value as WasmValue};

    let source_file = executor.source_file.as_deref()
        .ok_or("Go WASM tools require 'source_file' (compiled .wasm path)")?;

    let wasm_bytes = std::fs::read(source_file)
        .map_err(|e| format!("Failed to read WASM file '{}': {}", source_file, e))?;

    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = Module::new(&engine, &wasm_bytes)
        .map_err(|e| format!("WASM compile failed: {}", e))?;
    let mut store = Store::new(engine);

    let env = FunctionEnv::new(&mut store, crate::wasm_runtime::wasi::WasiEnv { memory: None });
    let wasi_imports = crate::wasm_runtime::wasi::build_wasi_imports(&mut store, &env);
    let instance = Instance::new(&mut store, &module, &wasi_imports)
        .map_err(|e| format!("WASM instantiation failed: {}", e))?;
    let memory = instance.exports.get_memory("memory")
        .map_err(|e| format!("WASM module missing memory export: {}", e))?
        .clone();
    env.as_mut(&mut store).memory = Some(memory.clone());

    if let Ok(start_fn) = instance.exports.get_function("_start") {
        let _ = start_fn.call(&mut store, &[]);
    }

    let args_json = serde_json::to_string(arguments)
        .map_err(|e| format!("Failed to serialize arguments: {}", e))?;
    let args_bytes = args_json.as_bytes();

    let prepare = instance.exports.get_function("prepare_call")
        .map_err(|e| format!("WASM module missing prepare_call: {}", e))?;
    prepare.call(&mut store, &[]).map_err(|e| format!("prepare_call failed: {}", e))?;

    let malloc = instance.exports.get_function("malloc")
        .map_err(|e| format!("WASM module missing malloc: {}", e))?;
    let ptr_val = malloc.call(&mut store, &[WasmValue::I32(args_bytes.len() as i32)])
        .map_err(|e| format!("malloc failed: {}", e))?;
    let input_ptr = ptr_val[0].unwrap_i32();

    memory.view(&store).write(input_ptr as u64, args_bytes)
        .map_err(|e| format!("Failed to write input to WASM memory: {}", e))?;

    let execute = instance.exports.get_function("tool_execute")
        .map_err(|e| format!("WASM module missing tool_execute: {}", e))?;
    let result = execute.call(&mut store, &[
        WasmValue::I32(input_ptr),
        WasmValue::I32(args_bytes.len() as i32),
    ]).map_err(|e| format!("tool_execute failed: {}", e))?;

    let encoded = result[0].unwrap_i64();
    let result_ptr = (encoded >> 32) as u32;
    let result_len = (encoded & 0xFFFF_FFFF) as u32;

    let mut result_buf = vec![0u8; result_len as usize];
    memory.view(&store).read(result_ptr as u64, &mut result_buf)
        .map_err(|e| format!("Failed to read WASM result: {}", e))?;

    String::from_utf8(result_buf)
        .map_err(|e| format!("WASM result is not valid UTF-8: {}", e))
}

/// Substitute template variables in a string.
///
/// Replaces `{{variable_name}}` with the corresponding value from arguments.
///
/// # Arguments
///
/// * `template` - Template string with variables
/// * `arguments` - Arguments to substitute
///
/// # Returns
///
/// String with variables substituted
fn substitute_template_variables(template: &str, arguments: &Value) -> String {
    let mut result = template.to_string();

    if let Value::Object(args) = arguments {
        for (key, value) in args {
            let placeholder = format!("{{{{{}}}}}", key);
            let replacement = match value {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => value.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }

    result
}

fn validate_plugin_argument(value: &str, is_shell_command: bool, index: usize) -> Result<(), String> {
    if value.contains('\0') {
        return Err(format!("Plugin argument {} contains null byte — rejected", index));
    }

    if value.len() > 10_000 {
        return Err(format!("Plugin argument {} exceeds 10KB limit ({} bytes)", index, value.len()));
    }

    if is_shell_command {
        let dangerous = [';', '|', '&', '`', '$', '\n', '\r', '(', ')', '{', '}', '<', '>'];
        for ch in &dangerous {
            if value.contains(*ch) {
                tracing::warn!(arg_index = index, char = %ch, "Blocked shell metacharacter in plugin argument");
                return Err(format!(
                    "Plugin argument {} contains shell metacharacter '{}' — \
                    plugins using shell interpreters cannot accept arguments with metacharacters. \
                    Use a non-shell plugin executor or sanitize the input.",
                    index, ch
                ));
            }
        }
    }

    Ok(())
}

/// Convert plugin tools to registry tools.
///
/// # Arguments
///
/// * `plugins` - Loaded plugins
///
/// # Returns
///
/// Vector of registry-compatible tool definitions
pub fn plugins_to_registry_tools(plugins: &[LoadedPlugin]) -> Vec<crate::registry::Tool> {
    let mut tools = Vec::new();

    for plugin in plugins {
        for tool in &plugin.manifest.tools {
            let description = if tool.executor.executor_type == "wasm" {
                if let Some(ref lang) = tool.executor.language {
                    format!("{} [wasm:{}]", tool.description, lang)
                } else {
                    tool.description.clone()
                }
            } else {
                tool.description.clone()
            };
            tools.push(crate::registry::Tool {
                name: tool.name.clone(),
                description,
                input_schema: tool.input_schema.clone(),
            });
        }
    }

    tools
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn test_timer(name: &str) -> impl Drop {
        let start = std::time::Instant::now();
        struct Timer { name: String, start: std::time::Instant }
        impl Drop for Timer {
            fn drop(&mut self) {
                eprintln!("[TEST] {} completed in {:.3}ms", self.name, self.start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        Timer { name: name.to_string(), start }
    }

    #[test]
    fn test_substitute_template_variables() {
        let _t = test_timer("test_substitute_template_variables");
        let template = "Hello {{name}}, you are {{age}} years old";
        let arguments = json!({
            "name": "Alice",
            "age": 30
        });

        let result = substitute_template_variables(template, &arguments);
        assert_eq!(result, "Hello Alice, you are 30 years old");
    }

    #[test]
    fn test_substitute_template_variables_missing() {
        let _t = test_timer("test_substitute_template_variables_missing");
        let template = "Hello {{name}}";
        let arguments = json!({});

        let result = substitute_template_variables(template, &arguments);
        assert_eq!(result, "Hello {{name}}");
    }

    #[test]
    fn test_load_plugin_manifest() {
        let _t = test_timer("test_load_plugin_manifest");
        let manifest_json = r#"{
            "name": "test-plugin",
            "version": "1.0.0",
            "tools": [
                {
                    "name": "test_tool",
                    "description": "A test tool",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "input": { "type": "string" }
                        }
                    },
                    "executor": {
                        "executor_type": "process",
                        "command": "echo",
                        "args": ["{{input}}"]
                    }
                }
            ]
        }"#;

        let manifest: PluginManifest = serde_json::from_str(manifest_json).unwrap();
        assert_eq!(manifest.name, "test-plugin");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.tools.len(), 1);
        assert_eq!(manifest.tools[0].name, "test_tool");
    }

    #[test]
    fn test_execute_plugin_tool_echo() {
        let _t = test_timer("test_execute_plugin_tool_echo");
        let tool = PluginTool {
            name: "echo_tool".to_string(),
            description: "Echo tool".to_string(),
            input_schema: json!({"type": "object", "properties": {"msg": {"type": "string"}}}),
            executor: PluginExecutor {
                executor_type: "process".to_string(),
                command: if cfg!(windows) { "cmd".to_string() } else { "echo".to_string() },
                args: if cfg!(windows) { vec!["/C".to_string(), "echo".to_string(), "{{msg}}".to_string()] } else { vec!["{{msg}}".to_string()] },
                working_dir: None,
                env: HashMap::new(),
                timeout: 5,
                language: None,
                source: None,
                source_file: None,
                handler_function: None,
            },
        };

        let result = execute_plugin_tool(&tool, &json!({"msg": "hello"}));
        assert!(result.is_ok(), "Echo tool should succeed");
        let output = result.unwrap();
        assert!(output.contains("hello"), "Output should contain 'hello': {}", output);
    }

    #[test]
    fn test_execute_plugin_tool_nonzero_exit() {
        let _t = test_timer("test_execute_plugin_tool_nonzero_exit");
        let tool = PluginTool {
            name: "fail_tool".to_string(),
            description: "Failing tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "process".to_string(),
                command: if cfg!(windows) { "cmd".to_string() } else { "false".to_string() },
                args: if cfg!(windows) { vec!["/C".to_string(), "exit".to_string(), "1".to_string()] } else { vec![] },
                working_dir: None,
                env: HashMap::new(),
                timeout: 5,
                language: None,
                source: None,
                source_file: None,
                handler_function: None,
            },
        };

        let result = execute_plugin_tool(&tool, &json!({}));
        assert!(result.is_err(), "Tool should fail with non-zero exit");
    }

    #[test]
    fn test_execute_plugin_tool_timeout() {
        let _t = test_timer("test_execute_plugin_tool_timeout");
        let tool = PluginTool {
            name: "sleep_tool".to_string(),
            description: "Sleep tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "process".to_string(),
                command: if cfg!(windows) { "cmd".to_string() } else { "sleep".to_string() },
                args: if cfg!(windows) {
                    vec!["/C".to_string(), "ping -n 10 127.0.0.1".to_string()]
                } else {
                    vec!["10".to_string()]
                },
                working_dir: None,
                env: HashMap::new(),
                timeout: 1,
                language: None,
                source: None,
                source_file: None,
                handler_function: None,
            },
        };

        let result = execute_plugin_tool(&tool, &json!({}));
        assert!(result.is_err(), "Tool should timeout");
        let err = result.unwrap_err();
        assert!(err.contains("timed out") || err.contains("Timeout"), "Error should mention timeout: {}", err);
    }

    #[test]
    fn test_execute_plugin_unsupported_executor() {
        let _t = test_timer("test_execute_plugin_unsupported_executor");
        let tool = PluginTool {
            name: "bad_tool".to_string(),
            description: "Bad tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "docker".to_string(),
                command: "test".to_string(),
                args: vec![],
                working_dir: None,
                env: HashMap::new(),
                timeout: 5,
                language: None,
                source: None,
                source_file: None,
                handler_function: None,
            },
        };

        let result = execute_plugin_tool(&tool, &json!({}));
        assert!(result.is_err(), "Unsupported executor should fail");
        assert!(result.unwrap_err().contains("Unsupported"));
    }

    #[test]
    fn test_load_plugins_from_directory() {
        let _t = test_timer("test_load_plugins_from_directory");
        let dir = tempdir().unwrap();

        let valid_manifest = r#"{
            "name": "valid-plugin",
            "version": "1.0.0",
            "tools": [{
                "name": "tool1",
                "description": "Tool 1",
                "inputSchema": {"type": "object"},
                "executor": {"executor_type": "process", "command": "echo", "args": []}
            }]
        }"#;

        let invalid_manifest = "not valid json";

        std::fs::write(dir.path().join("valid.json"), valid_manifest).unwrap();
        std::fs::write(dir.path().join("invalid.json"), invalid_manifest).unwrap();
        std::fs::write(dir.path().join("readme.txt"), "not a manifest").unwrap();

        let plugins = load_plugins_from_directory(dir.path());
        assert_eq!(plugins.len(), 1, "Should load only valid JSON manifests");
        assert_eq!(plugins[0].manifest.name, "valid-plugin");
    }

    #[test]
    fn test_load_plugin_manifest_validation() {
        let _t = test_timer("test_load_plugin_manifest_validation");
        let dir = tempdir().unwrap();

        let empty_name = r#"{
            "name": "",
            "version": "1.0.0",
            "tools": [{"name": "t", "description": "d", "inputSchema": {}, "executor": {"executor_type": "process", "command": "echo", "args": []}}]
        }"#;
        let path = dir.path().join("empty_name.json");
        std::fs::write(&path, empty_name).unwrap();
        let result = load_plugin_manifest(&path);
        assert!(result.is_err(), "Empty name should be rejected");
        assert!(result.unwrap_err().contains("name"));

        let empty_tools = r#"{
            "name": "test",
            "version": "1.0.0",
            "tools": []
        }"#;
        let path = dir.path().join("empty_tools.json");
        std::fs::write(&path, empty_tools).unwrap();
        let result = load_plugin_manifest(&path);
        assert!(result.is_err(), "Empty tools should be rejected");
        assert!(result.unwrap_err().contains("at least one tool"));

        let bad_executor = r#"{
            "name": "test",
            "version": "1.0.0",
            "tools": [{"name": "t", "description": "d", "inputSchema": {}, "executor": {"executor_type": "docker", "command": "echo", "args": []}}]
        }"#;
        let path = dir.path().join("bad_executor.json");
        std::fs::write(&path, bad_executor).unwrap();
        let result = load_plugin_manifest(&path);
        assert!(result.is_err(), "Bad executor type should be rejected");
        assert!(result.unwrap_err().contains("Unsupported"));
    }

    #[test]
    fn test_plugins_to_registry_tools() {
        let _t = test_timer("test_plugins_to_registry_tools");
        let plugins = vec![
            LoadedPlugin {
                manifest: PluginManifest {
                    name: "plugin1".to_string(),
                    version: "1.0.0".to_string(),
                    tools: vec![
                        PluginTool {
                            name: "tool_a".to_string(),
                            description: "Tool A".to_string(),
                            input_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
                            executor: PluginExecutor {
                                executor_type: "process".to_string(),
                                command: "echo".to_string(),
                                args: vec![],
                                working_dir: None,
                                env: HashMap::new(),
                                timeout: 5,
                                language: None,
                                source: None,
                                source_file: None,
                                handler_function: None,
                            },
                        },
                    ],
                },
                path: std::path::PathBuf::from("plugin1.json"),
            },
        ];

        let tools = plugins_to_registry_tools(&plugins);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "tool_a");
        assert_eq!(tools[0].description, "Tool A");
    }

    #[test]
    fn test_load_plugins_from_nonexistent_directory() {
        let _t = test_timer("test_load_plugins_from_nonexistent_directory");
        let plugins = load_plugins_from_directory(Path::new("/nonexistent/dir/that/does/not/exist"));
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_load_plugins_from_file_not_directory() {
        let _t = test_timer("test_load_plugins_from_file_not_directory");
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("not_a_dir.txt");
        std::fs::write(&file_path, "hello").unwrap();
        let plugins = load_plugins_from_directory(&file_path);
        assert!(plugins.is_empty());
    }

    #[test]
    fn test_load_plugin_manifest_oversized() {
        let _t = test_timer("test_load_plugin_manifest_oversized");
        let dir = tempdir().unwrap();
        let path = dir.path().join("huge.json");
        let mut f = std::fs::File::create(&path).unwrap();
        use std::io::Write;
        let chunk = vec![b' '; 64 * 1024];
        for _ in 0..17 {
            f.write_all(&chunk).unwrap();
        }
        drop(f);
        assert!(std::fs::metadata(&path).unwrap().len() > 1_048_576);
        let result = load_plugin_manifest(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds 1MB"));
    }

    #[test]
    fn test_load_plugin_manifest_empty_tool_name() {
        let _t = test_timer("test_load_plugin_manifest_empty_tool_name");
        let dir = tempdir().unwrap();
        let json = r#"{
            "name": "test",
            "version": "1.0.0",
            "tools": [{"name": "", "description": "d", "inputSchema": {}, "executor": {"executor_type": "process", "command": "echo", "args": []}}]
        }"#;
        let path = dir.path().join("empty_tool_name.json");
        std::fs::write(&path, json).unwrap();
        let result = load_plugin_manifest(&path);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Tool name"));
    }

    #[test]
    fn test_substitute_template_bool_and_other_types() {
        let _t = test_timer("test_substitute_template_bool_and_other_types");
        let template = "flag={{flag}}, list={{list}}";
        let arguments = json!({
            "flag": true,
            "list": [1, 2, 3]
        });
        let result = substitute_template_variables(template, &arguments);
        assert!(result.contains("true"), "Bool should serialize to 'true': {}", result);
        assert!(result.contains("[1,2,3]"), "Array should serialize: {}", result);
    }

    #[test]
    fn test_substitute_template_non_object_args() {
        let _t = test_timer("test_substitute_template_non_object_args");
        let template = "value={{x}}";
        let arguments = json!("just a string");
        let result = substitute_template_variables(template, &arguments);
        assert_eq!(result, "value={{x}}", "Non-object args should not substitute");
    }

    #[test]
    fn test_validate_plugin_argument_null_byte() {
        let _t = test_timer("test_validate_plugin_argument_null_byte");
        let result = validate_plugin_argument("hello\0world", false, 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("null byte"));
    }

    #[test]
    fn test_validate_plugin_argument_length_limit() {
        let _t = test_timer("test_validate_plugin_argument_length_limit");
        let long_arg = "x".repeat(10_001);
        let result = validate_plugin_argument(&long_arg, false, 0);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("10KB"));
    }

    #[test]
    fn test_validate_plugin_argument_shell_metacharacter() {
        let _t = test_timer("test_validate_plugin_argument_shell_metacharacter");
        let result = validate_plugin_argument("hello; rm -rf /", true, 0);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("shell metacharacter"), "Expected shell metacharacter error, got: {}", err);
    }

    #[test]
    fn test_validate_plugin_argument_safe_when_not_shell() {
        let _t = test_timer("test_validate_plugin_argument_safe_when_not_shell");
        let result = validate_plugin_argument("hello; world", false, 0);
        assert!(result.is_ok(), "Semicolons should be allowed when not a shell command");
    }

    #[test]
    fn test_execute_plugin_tool_with_working_dir_and_env() {
        let _t = test_timer("test_execute_plugin_tool_with_working_dir_and_env");
        let dir = tempdir().unwrap();
        let tool = PluginTool {
            name: "env_tool".to_string(),
            description: "Env tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "process".to_string(),
                command: if cfg!(windows) { "cmd".to_string() } else { "echo".to_string() },
                args: if cfg!(windows) {
                    vec!["/C".to_string(), "echo".to_string(), "%MY_VAR%".to_string()]
                } else {
                    vec!["$MY_VAR".to_string()]
                },
                working_dir: Some(dir.path().to_string_lossy().to_string()),
                env: {
                    let mut m = HashMap::new();
                    m.insert("MY_VAR".to_string(), "test_value_123".to_string());
                    m
                },
                timeout: 5,
                language: None,
                source: None,
                source_file: None,
                handler_function: None,
            },
        };

        let result = execute_plugin_tool(&tool, &json!({}));
        assert!(result.is_ok(), "Tool with working_dir and env should succeed: {:?}", result.err());
    }

}
