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

/// Maps WASM tool names to their language runtime (e.g., "lua", "python")
static WASM_TOOL_METADATA: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

struct CachedWasmModule {
    engine: wasmer::Engine,
    module: wasmer::Module,
    mtime: Option<std::time::SystemTime>,
}

static WASM_MODULE_CACHE: std::sync::LazyLock<Mutex<HashMap<String, CachedWasmModule>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

fn execute_wasm_plugin_tool(tool: &PluginTool, arguments: &Value) -> Result<String, String> {
    let executor = &tool.executor;
    let language = executor.language.as_deref()
        .ok_or_else(|| "WASM executor requires 'language' field".to_string())?;

    // All languages now use the same WasmRuntime interface
    let source = resolve_wasm_source(executor)?;

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

    // Register tool metadata for binary protocol lookup
    {
        let mut metadata = WASM_TOOL_METADATA.lock()
            .map_err(|e| format!("WASM tool metadata cache poisoned: {}", e))?;
        metadata.insert(tool.name.clone(), language.to_string());
    }

    let args_json = serde_json::to_string(arguments)
        .map_err(|e| format!("Failed to serialize arguments: {}", e))?;

    runtime.call_tool(&tool.name, &args_json)
        .map_err(|e| format!("WASM tool '{}' execution failed: {}", tool.name, e))
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

static WASM_RUNTIMES_CONFIG: std::sync::OnceLock<crate::config::WasmRuntimesConfig> =
    std::sync::OnceLock::new();

/// Provide the server's WASM runtime configuration to the plugin registry.
///
/// Must be called before plugins load; later calls are ignored so the
/// first configuration wins for the process lifetime.
pub fn set_wasm_runtimes_config(config: crate::config::WasmRuntimesConfig) {
    let _ = WASM_RUNTIMES_CONFIG.set(config);
}

fn wasm_runtimes_config() -> &'static crate::config::WasmRuntimesConfig {
    WASM_RUNTIMES_CONFIG.get_or_init(crate::config::WasmRuntimesConfig::default)
}

fn create_wasm_runtime(language: &str) -> Result<Box<dyn crate::wasm_runtime::WasmRuntime>, String> {
    let (enabled, wasm_path) = wasm_runtimes_config()
        .resolve_language(language)
        .ok_or_else(|| format!("Unsupported WASM language: {}", language))?;
    if !enabled {
        return Err(format!("WASM language '{}' is disabled in configuration", language));
    }

    crate::wasm_runtime::create_wasm_runtime_for_language(language, &wasm_path)
        .map_err(|e| format!("Failed to create WASM runtime for '{}': {}", language, e))
}

/// Call a WASM tool with binary TLV arguments (optimized path).
pub fn call_wasm_tool_binary(name: &str, language: &str, args_tlv: &[u8]) -> Result<String, String> {
    let mut cache = WASM_RUNTIME_CACHE.lock()
        .map_err(|e| format!("WASM runtime cache poisoned: {}", e))?;

    if !cache.contains_key(language) {
        let runtime = create_wasm_runtime(language)?;
        cache.insert(language.to_string(), runtime);
    }

    let runtime = cache.get_mut(language)
        .ok_or_else(|| format!("No runtime for language: {}", language))?;

    // Use binary protocol - falls back to JSON if not supported
    runtime.call_tool_binary(name, args_tlv)
        .map_err(|e| format!("WASM tool '{}' execution failed (binary): {}", name, e))
}

/// Get the language runtime for a registered WASM tool.
pub fn get_wasm_tool_language(name: &str) -> Option<String> {
    let metadata = WASM_TOOL_METADATA.lock().ok()?;
    metadata.get(name).cloned()
}

fn execute_standalone_wasm_tool(source_file: &str, tool_name: &str, arguments: &Value) -> Result<String, String> {
    use wasmer::{FunctionEnv, Instance, Store, Value as WasmValue};

    let current_mtime = std::fs::metadata(source_file)
        .ok().and_then(|m| m.modified().ok());

    let (engine, module) = {
        let mut cache = WASM_MODULE_CACHE.lock()
            .map_err(|e| format!("WASM module cache poisoned: {}", e))?;

        let cached = cache.get(source_file);
        let needs_recompile = match cached {
            Some(entry) => entry.mtime != current_mtime,
            None => true,
        };

        if needs_recompile {
            let wasm_bytes = std::fs::read(source_file)
                .map_err(|e| format!("Failed to read WASM file '{}': {}", source_file, e))?;
            let engine = wasmer::Engine::from(wasmer::Cranelift::default());
            let module = wasmer::Module::new(&engine, &wasm_bytes)
                .map_err(|e| format!("WASM compile failed: {}", e))?;
            cache.insert(source_file.to_string(), CachedWasmModule {
                engine: engine.clone(),
                module: module.clone(),
                mtime: current_mtime,
            });
            (engine, module)
        } else {
            let entry = cached.unwrap();
            (entry.engine.clone(), entry.module.clone())
        }
    };

    let mut store = Store::new(engine);

    let env = FunctionEnv::new(&mut store, crate::wasm_runtime::wasi::WasiEnv::new());
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

    // Include tool name in the payload for dynamic dispatch
    let mut payload = serde_json::Map::new();
    payload.insert("_tool_name".to_string(), serde_json::Value::String(tool_name.to_string()));
    if let Value::Object(args) = arguments {
        for (k, v) in args {
            payload.insert(k.clone(), v.clone());
        }
    }
    let args_json = serde_json::to_string(&payload)
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

    // ── WASM real-world integration tests ──────────────────────────────

    fn wasm_js_tool(name: &str, source: &str) -> PluginTool {
        PluginTool {
            name: name.to_string(),
            description: "WASM JS test tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "wasm".to_string(),
                command: String::new(),
                args: vec![],
                working_dir: None,
                env: HashMap::new(),
                timeout: 5,
                language: Some("javascript".to_string()),
                source: Some(source.to_string()),
                source_file: None,
                handler_function: None,
            },
        }
    }

    fn wasm_py_tool(name: &str, source: &str) -> PluginTool {
        PluginTool {
            name: name.to_string(),
            description: "WASM Python test tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "wasm".to_string(),
                command: String::new(),
                args: vec![],
                working_dir: None,
                env: HashMap::new(),
                timeout: 5,
                language: Some("python".to_string()),
                source: Some(source.to_string()),
                source_file: None,
                handler_function: None,
            },
        }
    }

    fn wasm_lua_tool(name: &str, source: &str) -> PluginTool {
        PluginTool {
            name: name.to_string(),
            description: "WASM Lua test tool".to_string(),
            input_schema: json!({"type": "object"}),
            executor: PluginExecutor {
                executor_type: "wasm".to_string(),
                command: String::new(),
                args: vec![],
                working_dir: None,
                env: HashMap::new(),
                timeout: 5,
                language: Some("lua".to_string()),
                source: Some(source.to_string()),
                source_file: None,
                handler_function: None,
            },
        }
    }

    // ── JavaScript (QuickJS) ──

    #[test]
    #[ignore] // requires QuickJS WASM binary
    fn test_wasm_js_string_transform() {
        let _t = test_timer("test_wasm_js_string_transform");
        let source = r#"function js_string_transform(args) {
            var s = args.input || '';
            switch (args.action) {
                case 'upper': return {result: s.toUpperCase()};
                case 'lower': return {result: s.toLowerCase()};
                case 'reverse': return {result: s.split('').reverse().join('')};
                case 'slug': return {result: s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')};
                case 'capitalize': return {result: s.charAt(0).toUpperCase() + s.slice(1)};
                case 'title': return {result: s.replace(/\b\w/g, function(c) { return c.toUpperCase(); })};
                default: return {error: 'Unknown action: ' + args.action};
            }
        }"#;
        let tool = wasm_js_tool("js_string_transform", source);

        let r1 = execute_plugin_tool(&tool, &json!({"action": "upper", "input": "hello world"}));
        assert!(r1.is_ok(), "upper failed: {:?}", r1.err());
        assert!(r1.unwrap().contains("HELLO WORLD"));

        let r2 = execute_plugin_tool(&tool, &json!({"action": "slug", "input": "Hello World! Foo Bar"}));
        assert!(r2.is_ok(), "slug failed: {:?}", r2.err());
        assert!(r2.unwrap().contains("hello-world-foo-bar"));

        let r3 = execute_plugin_tool(&tool, &json!({"action": "reverse", "input": "abcdef"}));
        assert!(r3.is_ok(), "reverse failed: {:?}", r3.err());
        assert!(r3.unwrap().contains("fedcba"));

        let r4 = execute_plugin_tool(&tool, &json!({"action": "title", "input": "the quick brown fox"}));
        assert!(r4.is_ok(), "title failed: {:?}", r4.err());
        assert!(r4.unwrap().contains("The Quick Brown Fox"));
    }

    #[test]
    #[ignore] // requires QuickJS WASM binary
    fn test_wasm_js_calculator() {
        let _t = test_timer("test_wasm_js_calculator");
        let source = r#"function js_calculator(args) {
            var expr = args.expression || '';
            if (!/^[0-9+\-*/().\s%^]+$/.test(expr)) return {error: 'Invalid characters'};
            try {
                var result = Function('"use strict"; return (' + expr + ')')();
                return {result: result, expression: expr};
            } catch(e) { return {error: e.message}; }
        }"#;
        let tool = wasm_js_tool("js_calculator", source);

        let r1 = execute_plugin_tool(&tool, &json!({"expression": "2 + 3 * 4"}));
        assert!(r1.is_ok(), "calc 2+3*4 failed: {:?}", r1.err());
        assert!(r1.unwrap().contains("14"));

        let r2 = execute_plugin_tool(&tool, &json!({"expression": "(100 - 25) / 5"}));
        assert!(r2.is_ok(), "calc (100-25)/5 failed: {:?}", r2.err());
        assert!(r2.unwrap().contains("15"));

        let r3 = execute_plugin_tool(&tool, &json!({"expression": "2 ** 10"}));
        assert!(r3.is_ok(), "calc 2**10 failed: {:?}", r3.err());
        assert!(r3.unwrap().contains("1024"));
    }

    #[test]
    #[ignore] // requires QuickJS WASM binary
    fn test_wasm_js_data_flatten() {
        let _t = test_timer("test_wasm_js_data_flatten");
        let source = r#"function js_data_flatten(args) {
            var result = {};
            function flatten(obj, pre) {
                for (var k in obj) {
                    if (obj[k] && typeof obj[k] === 'object' && !Array.isArray(obj[k])) {
                        flatten(obj[k], pre + k + '.');
                    } else {
                        result[pre + k] = obj[k];
                    }
                }
            }
            flatten(args.data || {}, args.prefix || '');
            return {result: result};
        }"#;
        let tool = wasm_js_tool("js_data_flatten", source);

        let nested = json!({
            "name": "test",
            "address": {
                "street": "123 Main St",
                "city": "Henties Bay",
                "geo": {"lat": -22.0, "lon": 14.27}
            },
            "tags": ["rust", "wasm"]
        });
        let r = execute_plugin_tool(&tool, &json!({"data": nested}));
        assert!(r.is_ok(), "flatten failed: {:?}", r.err());
        let output = r.unwrap();
        assert!(output.contains("address.street"));
        assert!(output.contains("123 Main St"));
        assert!(output.contains("address.geo.lat"));
        assert!(output.contains("Henties Bay"));
        assert!(output.contains("tags"));
    }

    // ── Python (MicroPython) ──

    #[test]
    #[ignore] // requires MicroPython WASM binary
    fn test_wasm_py_text_analyzer() {
        let _t = test_timer("test_wasm_py_text_analyzer");
        let source = r#"
def py_text_analyzer(args):
    text = args.get('text', '')
    words = text.split()
    return {'words': len(words), 'chars': len(text), 'lines': text.count('\n') + 1 if text else 0, 'unique_words': len(set(w.lower() for w in words))}
"#;
        let tool = wasm_py_tool("py_text_analyzer", source);

        let r = execute_plugin_tool(&tool, &json!({"text": "the quick brown fox jumps over the lazy dog\nthe fox is quick"}));
        assert!(r.is_ok(), "text analyzer failed: {:?}", r.err());
        let output = r.unwrap();
        assert!(output.contains("\"words\""), "should have word count: {}", output);
        assert!(output.contains("\"chars\""), "should have char count: {}", output);
        assert!(output.contains("\"unique_words\""), "should have unique words: {}", output);
    }

    #[test]
    #[ignore] // requires MicroPython WASM binary
    fn test_wasm_py_unit_converter() {
        let _t = test_timer("test_wasm_py_unit_converter");
        let source = r#"
def py_unit_converter(args):
    v = args.get('value', 0)
    f = args.get('from_unit', '')
    t = args.get('to_unit', '')
    temp = {'celsius': 'c', 'fahrenheit': 'f', 'kelvin': 'k'}
    length = {'meter': 'm', 'foot': 'ft', 'inch': 'in', 'km': 'km', 'mile': 'mi'}
    if f in temp and t in temp:
        c = v if temp[f] == 'c' else (v - 32) * 5/9 if temp[f] == 'f' else v - 273.15
        r = c if temp[t] == 'c' else c * 9/5 + 32 if temp[t] == 'f' else c + 273.15
        return {'result': round(r, 4), 'from': f, 'to': t}
    if f in length and t in length:
        to_m = {'m': 1, 'ft': 0.3048, 'in': 0.0254, 'km': 1000, 'mi': 1609.344}
        meters = v * to_m[length[f]]
        r = meters / to_m[length[t]]
        return {'result': round(r, 6), 'from': f, 'to': t}
    return {'error': 'incompatible units'}
"#;
        let tool = wasm_py_tool("py_unit_converter", source);

        let r1 = execute_plugin_tool(&tool, &json!({"value": 100, "from_unit": "celsius", "to_unit": "fahrenheit"}));
        assert!(r1.is_ok(), "C→F failed: {:?}", r1.err());
        assert!(r1.unwrap().contains("212"), "100°C should be 212°F");

        let r2 = execute_plugin_tool(&tool, &json!({"value": 1, "from_unit": "mile", "to_unit": "km"}));
        assert!(r2.is_ok(), "mi→km failed: {:?}", r2.err());
        assert!(r2.unwrap().contains("1.609"), "1 mile should be ~1.609 km");
    }

    #[test]
    #[ignore] // requires MicroPython WASM binary
    fn test_wasm_py_list_operations() {
        let _t = test_timer("test_wasm_py_list_operations");
        let source = r#"
def py_list_operations(args):
    op = args.get('operation', '')
    items = args.get('items', [])
    if op == 'sort': return {'result': sorted(items)}
    if op == 'reverse': return {'result': list(reversed(items))}
    if op == 'unique': return {'result': sorted(set(items))}
    if op == 'min': return {'result': min(items)}
    if op == 'max': return {'result': max(items)}
    if op == 'sum': return {'result': sum(items)}
    if op == 'average': return {'result': sum(items) / len(items) if items else 0}
    return {'error': 'unknown operation'}
"#;
        let tool = wasm_py_tool("py_list_operations", source);

        let r1 = execute_plugin_tool(&tool, &json!({"operation": "sort", "items": [5, 3, 1, 4, 2]}));
        assert!(r1.is_ok(), "sort failed: {:?}", r1.err());
        assert!(r1.unwrap().contains("[1, 2, 3, 4, 5]"));

        let r2 = execute_plugin_tool(&tool, &json!({"operation": "sum", "items": [10, 20, 30, 40]}));
        assert!(r2.is_ok(), "sum failed: {:?}", r2.err());
        assert!(r2.unwrap().contains("100"));

        let r3 = execute_plugin_tool(&tool, &json!({"operation": "average", "items": [10, 20, 30, 40]}));
        assert!(r3.is_ok(), "average failed: {:?}", r3.err());
        assert!(r3.unwrap().contains("25"));
    }

    // ── Lua ──

    #[test]
    #[ignore] // requires Lua WASM binary
    fn test_wasm_lua_string_utils() {
        let _t = test_timer("test_wasm_lua_string_utils");
        let source = "function lua_string_utils(args)\n\
            local s = args.input or ''\n\
            local a = args.action or ''\n\
            if a == 'upper' then return {result = string.upper(s)}\n\
            elseif a == 'lower' then return {result = string.lower(s)}\n\
            elseif a == 'reverse' then return {result = string.reverse(s)}\n\
            elseif a == 'repeat' then\n\
                local n = args.count or 2\n\
                return {result = string.rep(s, n)}\n\
            elseif a == 'trim' then\n\
                local t = s:match('^%s*(.-)%s*$')\n\
                return {result = t}\n\
            elseif a == 'word_count' then\n\
                local c = 0\n\
                for _ in s:gmatch('%S+') do c = c + 1 end\n\
                return {result = c}\n\
            else\n\
                return {error = 'unknown action: ' .. a}\n\
            end\n\
          end";
        let tool = wasm_lua_tool("lua_string_utils", source);

        let r1 = execute_plugin_tool(&tool, &json!({"action": "upper", "input": "hello world"}));
        assert!(r1.is_ok(), "upper failed: {:?}", r1.err());
        let out1 = r1.unwrap();
        assert!(out1.contains("HELLO WORLD"), "expected HELLO WORLD in: {}", out1);

        let r2 = execute_plugin_tool(&tool, &json!({"action": "reverse", "input": "abcdef"}));
        assert!(r2.is_ok(), "reverse failed: {:?}", r2.err());
        let out2 = r2.unwrap();
        assert!(out2.contains("fedcba"), "expected fedcba in: {}", out2);

        let r3 = execute_plugin_tool(&tool, &json!({"action": "word_count", "input": "the quick brown fox"}));
        assert!(r3.is_ok(), "word_count failed: {:?}", r3.err());
        let out3 = r3.unwrap();
        assert!(out3.contains("4"), "expected 4 in: {}", out3);
    }

    #[test]
    #[ignore] // requires Lua WASM binary
    fn test_wasm_lua_math_tools() {
        let _t = test_timer("test_wasm_lua_math_tools");
        let source = "function lua_math_tools(args)\n\
            local op = args.operation or ''\n\
            local v = args.value or 0\n\
            if op == 'power' then return {result = v ^ (args.exponent or 2)}\n\
            elseif op == 'sqrt' then return {result = math.sqrt(v)}\n\
            elseif op == 'round' then return {result = math.floor(v + 0.5)}\n\
            elseif op == 'floor' then return {result = math.floor(v)}\n\
            elseif op == 'ceil' then return {result = math.ceil(v)}\n\
            elseif op == 'abs' then return {result = math.abs(v)}\n\
            elseif op == 'fibonacci' then\n\
                local n = math.floor(v)\n\
                if n <= 0 then return {result = 0}\n\
                elseif n == 1 then return {result = 1}\n\
                end\n\
                local a, b = 0, 1\n\
                for _ = 2, n do a, b = b, a + b end\n\
                return {result = b}\n\
            else\n\
                return {error = 'unknown operation: ' .. op}\n\
            end\n\
          end";
        let tool = wasm_lua_tool("lua_math_tools", source);

        let r1 = execute_plugin_tool(&tool, &json!({"operation": "sqrt", "value": 144}));
        assert!(r1.is_ok(), "sqrt failed: {:?}", r1.err());
        let out1 = r1.unwrap();
        assert!(out1.contains("12"), "expected 12 in: {}", out1);

        let r2 = execute_plugin_tool(&tool, &json!({"operation": "power", "value": 2, "exponent": 10}));
        assert!(r2.is_ok(), "power failed: {:?}", r2.err());
        let out2 = r2.unwrap();
        assert!(out2.contains("1024"), "expected 1024 in: {}", out2);

        let r3 = execute_plugin_tool(&tool, &json!({"operation": "fibonacci", "value": 10}));
        assert!(r3.is_ok(), "fibonacci failed: {:?}", r3.err());
        let out3 = r3.unwrap();
        assert!(out3.contains("55"), "fib(10) should be 55, got: {}", out3);
    }

    // ── Manifest loading ──

    #[test]
    fn test_wasm_plugin_manifests_parse() {
        let _t = test_timer("test_wasm_plugin_manifests_parse");

        let js_manifest = include_str!("../../plugins/wasm_javascript_tools.json");
        let js: PluginManifest = serde_json::from_str(js_manifest).expect("JS manifest should parse");
        assert_eq!(js.name, "wasm-javascript-tools");
        assert_eq!(js.tools.len(), 3);
        for tool in &js.tools {
            assert_eq!(tool.executor.executor_type, "wasm");
            assert_eq!(tool.executor.language.as_deref(), Some("javascript"));
            assert!(tool.executor.source.is_some(), "JS tools should have inline source");
        }

        let py_manifest = include_str!("../../plugins/wasm_python_tools.json");
        let py: PluginManifest = serde_json::from_str(py_manifest).expect("Python manifest should parse");
        assert_eq!(py.name, "wasm-python-tools");
        assert_eq!(py.tools.len(), 3);
        for tool in &py.tools {
            assert_eq!(tool.executor.executor_type, "wasm");
            assert_eq!(tool.executor.language.as_deref(), Some("python"));
        }

        let lua_manifest = include_str!("../../plugins/wasm_lua_tools.json");
        let lua: PluginManifest = serde_json::from_str(lua_manifest).expect("Lua manifest should parse");
        assert_eq!(lua.name, "wasm-lua-tools");
        assert_eq!(lua.tools.len(), 2);
        for tool in &lua.tools {
            assert_eq!(tool.executor.executor_type, "wasm");
            assert_eq!(tool.executor.language.as_deref(), Some("lua"));
        }
    }

    // ── Cross-language same-tool test ──

    #[test]
    #[ignore] // requires all WASM binaries
    fn test_wasm_cross_language_string_reverse() {
        let _t = test_timer("test_wasm_cross_language_string_reverse");

        let js_source = r#"function reverse_string(args) {
            return {result: (args.input || '').split('').reverse().join('')};
        }"#;
        let py_source = r#"
def reverse_string(args):
    s = args.get('input', '')
    return {'result': ''.join(reversed(s))}
"#;
        let lua_source = r#"
function reverse_string(args)
    return {result = string.reverse(args.input or '')}
end
"#;

        let js_tool = wasm_js_tool("reverse_string", js_source);
        let py_tool = wasm_py_tool("reverse_string", py_source);
        let lua_tool = wasm_lua_tool("reverse_string", lua_source);

        let input = json!({"input": "Hello, Henties Bay!"});

        let js_r = execute_plugin_tool(&js_tool, &input).expect("JS reverse failed");
        let py_r = execute_plugin_tool(&py_tool, &input).expect("Python reverse failed");
        let lua_r = execute_plugin_tool(&lua_tool, &input).expect("Lua reverse failed");

        assert!(js_r.contains("yaB seitneH ,olleH"), "JS result: {}", js_r);
        assert!(py_r.contains("yaB seitneH ,olleH"), "Python result: {}", py_r);
        assert!(lua_r.contains("yaB seitneH ,olleH"), "Lua result: {}", lua_r);
    }

}

