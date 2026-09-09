//! WASM runtime abstraction for cross-language tool execution.
//!
//! Each language interpreter (QuickJS, MicroPython, Lua, etc.) is compiled to WASM
//! and runs in-process via Wasmer. The `WasmRuntime` trait provides a uniform interface
//! for registering and calling tools written in any supported language.

pub mod csharp;
pub mod java;
pub mod julia;
pub mod lua;
pub mod micropython;
pub mod perl;
pub mod php;
pub mod quickjs;
pub mod r;
pub mod ruby;
pub mod rust;
pub mod typescript;
pub mod wasi;

use std::collections::HashMap;
use std::error::Error;

/// Uniform interface for language runtimes compiled to WASM.
///
/// Each implementation wraps a specific interpreter (QuickJS, MicroPython, etc.)
/// compiled to a WASM module and running inside Wasmer.
pub trait WasmRuntime: Send {
    /// Initialize the interpreter runtime. Called once after WASM instantiation.
    fn init(&mut self) -> Result<(), Box<dyn Error>>;

    /// Register a tool with the given name and source code.
    /// The source is evaluated/compiled in the interpreter context.
    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>>;

    /// Call a registered tool with JSON arguments. Returns JSON result string.
    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>>;

    /// Destroy the runtime, freeing interpreter resources.
    fn destroy(&mut self) -> Result<(), Box<dyn Error>>;

    /// Language identifier (e.g., "javascript", "python", "lua").
    fn language(&self) -> &str;
}

/// Metadata about a registered tool.
#[derive(Debug, Clone)]
pub struct WasmToolMeta {
    /// Tool name
    pub name: String,
    /// Language runtime that hosts this tool
    pub language: String,
    /// JSON Schema for tool input (provided at registration)
    pub input_schema: serde_json::Value,
    /// Tool description
    pub description: String,
}

/// Registry of WASM-based language runtimes and their tools.
///
/// Manages the lifecycle of interpreter instances and routes tool calls
/// to the appropriate language runtime.
pub struct WasmRuntimeRegistry {
    runtimes: HashMap<String, RuntimeEntry>,
    tools: HashMap<String, WasmToolMeta>,
}

struct RuntimeEntry {
    runtime: Box<dyn WasmRuntime>,
    tool_names: Vec<String>,
}

impl WasmRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
            tools: HashMap::new(),
        }
    }

    /// Register a language runtime.
    pub fn register_runtime(&mut self, mut runtime: Box<dyn WasmRuntime>) -> Result<(), Box<dyn Error>> {
        let lang = runtime.language().to_string();
        runtime.init()?;
        self.runtimes.insert(lang.clone(), RuntimeEntry {
            runtime,
            tool_names: Vec::new(),
        });
        Ok(())
    }

    /// Register a tool in the appropriate language runtime.
    pub fn register_tool(
        &mut self,
        language: &str,
        name: &str,
        source: &str,
        description: &str,
        input_schema: serde_json::Value,
    ) -> Result<(), Box<dyn Error>> {
        let entry = self.runtimes.get_mut(language)
            .ok_or_else(|| format!("No runtime registered for language: {}", language))?;
        entry.runtime.register_tool(name, source)?;
        entry.tool_names.push(name.to_string());
        self.tools.insert(name.to_string(), WasmToolMeta {
            name: name.to_string(),
            language: language.to_string(),
            input_schema,
            description: description.to_string(),
        });
        Ok(())
    }

    /// Call a tool by name. Routes to the correct language runtime.
    pub fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        let meta = self.tools.get(name)
            .ok_or_else(|| format!("Unknown WASM tool: {}", name))?;
        let lang = meta.language.clone();
        let entry = self.runtimes.get_mut(&lang)
            .ok_or_else(|| format!("Runtime missing for language: {}", lang))?;
        entry.runtime.call_tool(name, args_json)
    }

    /// Get all registered WASM tools.
    pub fn get_tools(&self) -> Vec<&WasmToolMeta> {
        self.tools.values().collect()
    }

    /// Check if a tool name is registered as a WASM tool.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Destroy a specific runtime.
    pub fn destroy_runtime(&mut self, language: &str) -> Result<(), Box<dyn Error>> {
        if let Some(mut entry) = self.runtimes.remove(language) {
            for name in &entry.tool_names {
                self.tools.remove(name);
            }
            entry.runtime.destroy()?;
        }
        Ok(())
    }
}

impl Default for WasmRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}
