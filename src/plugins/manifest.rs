//! Plugin manifest data structures.
//!
//! Defines the JSON schema for plugin manifests that describe tools and their executors.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

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
    #[serde(rename = "type")]
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
    /// WASM source file or inline code (for "wasm" type)
    #[serde(default)]
    pub source: Option<String>,
    /// Path to a source file (.js, .py, .lua) or compiled WASM module (.wasm)
    #[serde(default)]
    pub source_file: Option<String>,
    /// Language for WASM execution (e.g., "python", "javascript", "lua")
    #[serde(default)]
    pub language: Option<String>,
}

fn default_timeout() -> u64 {
    30
}
