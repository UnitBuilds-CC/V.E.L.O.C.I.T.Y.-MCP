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
    /// Executor type (currently only "process" is supported)
    pub executor_type: String,
    /// Command to execute
    pub command: String,
    /// Command arguments (supports template variables)
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
        if tool.executor.executor_type != "process" {
            return Err(format!("Unsupported executor type: {}", tool.executor.executor_type));
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

    if executor.executor_type != "process" {
        return Err(format!("Unsupported executor type: {}", executor.executor_type));
    }

    // Build command with template variable substitution
    let args: Vec<String> = executor.args.iter().map(|arg| {
        substitute_template_variables(arg, arguments)
    }).collect();

    let mut cmd = Command::new(&executor.command);
    cmd.args(&args);

    if let Some(working_dir) = &executor.working_dir {
        cmd.current_dir(working_dir);
    }

    for (key, value) in &executor.env {
        cmd.env(key, value);
    }

    // Execute the command
    let output = cmd.output()
        .map_err(|e| format!("Failed to execute plugin tool: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Plugin tool execution failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.to_string())
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
            tools.push(crate::registry::Tool {
                name: tool.name.clone(),
                description: tool.description.clone(),
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

    #[test]
    fn test_substitute_template_variables() {
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
        let template = "Hello {{name}}";
        let arguments = json!({});

        let result = substitute_template_variables(template, &arguments);
        assert_eq!(result, "Hello {{name}}");
    }

    #[test]
    fn test_load_plugin_manifest() {
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
}
