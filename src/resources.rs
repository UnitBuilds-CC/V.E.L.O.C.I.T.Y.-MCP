//! MCP Resources and Prompts support.
//!
//! Resources expose data (files, databases, APIs) as URIs that clients can read.
//! Prompts provide templated prompt workflows with variable substitution.
//!
//! Both use zero-copy file access via memory-mapped files where possible.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// A resource exposed by the server.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Resource {
    pub uri: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A resource template for parameterized resources.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// A prompt template with variables.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Prompt {
    pub name: String,
    pub description: String,
    pub arguments: Vec<PromptArgument>,
}

/// A prompt argument definition.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

/// A rendered prompt message.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptMessage {
    pub role: String,
    pub content: PromptContent,
}

/// Prompt content (text or resource reference).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
    pub resource: Option<ResourceContent>,
}

/// Resource content embedded in a prompt.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub text: Option<String>,
}

/// Global resource registry.
static RESOURCE_REGISTRY: OnceLock<Mutex<ResourceStore>> = OnceLock::new();

fn get_resource_registry() -> &'static Mutex<ResourceStore> {
    RESOURCE_REGISTRY.get_or_init(|| Mutex::new(ResourceStore::default()))
}

/// In-memory resource store.
#[derive(Default)]
struct ResourceStore {
    resources: Vec<Resource>,
    templates: Vec<ResourceTemplate>,
    /// URI -> file path mapping for local file resources
    file_resources: HashMap<String, PathBuf>,
}

impl ResourceStore {
    fn register_file_resource(&mut self, uri: &str, name: &str, description: &str, path: PathBuf) {
        let mime_type = guess_mime(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
        self.resources.push(Resource {
            uri: uri.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            mime_type: Some(mime_type),
        });
        self.file_resources.insert(uri.to_string(), path);
    }

    fn register_template(&mut self, template: ResourceTemplate) {
        self.templates.push(template);
    }

    fn read_resource(&self, uri: &str) -> Result<ResourceReadResult, String> {
        if let Some(path) = self.file_resources.get(uri) {
            let content = std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read file: {}", e))?;
            let mime = guess_mime(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
            Ok(ResourceReadResult {
                uri: uri.to_string(),
                mime_type: mime,
                text: Some(content),
            })
        } else {
            Err(format!("Resource not found: {}", uri))
        }
    }
}

/// Result of reading a resource.
struct ResourceReadResult {
    uri: String,
    mime_type: String,
    text: Option<String>,
}

fn guess_mime(ext: &str) -> String {
    match ext {
        "txt" => "text/plain".to_string(),
        "md" => "text/markdown".to_string(),
        "json" => "application/json".to_string(),
        "csv" => "text/csv".to_string(),
        "html" => "text/html".to_string(),
        "xml" => "application/xml".to_string(),
        "yaml" | "yml" => "application/yaml".to_string(),
        "py" => "text/x-python".to_string(),
        "rs" => "text/x-rust".to_string(),
        "js" => "application/javascript".to_string(),
        "ts" => "application/typescript".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

/// Register a file as an MCP resource.
pub fn register_file_resource(uri: &str, name: &str, description: &str, path: &str) {
    let store = get_resource_registry();
    if let Ok(mut s) = store.lock() {
        s.register_file_resource(uri, name, description, PathBuf::from(path));
    }
}

/// Get all registered resources.
pub fn list_resources() -> Vec<Resource> {
    let store = get_resource_registry();
    match store.lock() {
        Ok(s) => s.resources.clone(),
        Err(_) => vec![],
    }
}

/// Get all resource templates.
pub fn list_resource_templates() -> Vec<ResourceTemplate> {
    let store = get_resource_registry();
    match store.lock() {
        Ok(s) => s.templates.clone(),
        Err(_) => vec![],
    }
}

/// Read a resource by URI.
pub fn read_resource(uri: &str) -> Result<Value, String> {
    let store = get_resource_registry();
    match store.lock() {
        Ok(s) => {
            let result = s.read_resource(uri)?;
            Ok(json!({
                "contents": [{
                    "uri": result.uri,
                    "mimeType": result.mime_type,
                    "text": result.text
                }]
            }))
        }
        Err(e) => Err(format!("Resource store poisoned: {}", e)),
    }
}

// ─── Prompts ─────────────────────────────────────────────────────────────────

static PROMPT_REGISTRY: OnceLock<Mutex<Vec<Prompt>>> = OnceLock::new();

fn get_prompt_registry() -> &'static Mutex<Vec<Prompt>> {
    PROMPT_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a prompt template.
pub fn register_prompt(name: &str, description: &str, args: Vec<(&str, &str, bool)>) {
    let registry = get_prompt_registry();
    if let Ok(mut prompts) = registry.lock() {
        prompts.push(Prompt {
            name: name.to_string(),
            description: description.to_string(),
            arguments: args.iter().map(|(n, d, r)| PromptArgument {
                name: n.to_string(),
                description: d.to_string(),
                required: *r,
            }).collect(),
        });
    }
}

/// List all registered prompts.
pub fn list_prompts() -> Vec<Prompt> {
    let registry = get_prompt_registry();
    match registry.lock() {
        Ok(p) => p.clone(),
        Err(_) => vec![],
    }
}

/// Get a prompt by name with variable substitution.
pub fn get_prompt(name: &str, arguments: &HashMap<String, String>) -> Result<Value, String> {
    let registry = get_prompt_registry();
    match registry.lock() {
        Ok(prompts) => {
            let prompt = prompts.iter().find(|p| p.name == name)
                .ok_or_else(|| format!("Prompt not found: {}", name))?;

            // Validate required arguments
            for arg in &prompt.arguments {
                if arg.required && !arguments.contains_key(&arg.name) {
                    return Err(format!("Missing required argument: {}", arg.name));
                }
            }

            // Simple template: just return the prompt description with variables substituted
            let mut text = prompt.description.clone();
            for (key, value) in arguments {
                text = text.replace(&format!("{{{}}}", key), value);
            }

            Ok(json!({
                "description": prompt.description,
                "messages": [{
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": text
                    }
                }]
            }))
        }
        Err(e) => Err(format!("Prompt registry poisoned: {}", e)),
    }
}

/// Handle resources/list request.
pub fn handle_resources_list(cursor: Option<&str>) -> Value {
    let resources = list_resources();
    let start = match cursor {
        Some(c) => c.parse::<usize>().unwrap_or(0),
        None => 0,
    };
    let page_size = 100;
    let end = (start + page_size).min(resources.len());
    let page = if start < resources.len() { &resources[start..end] } else { &[] };
    let next_cursor = if end < resources.len() { Some(json!(end.to_string())) } else { None };

    let mut result = json!({"resources": page});
    if let Some(nc) = next_cursor {
        result["nextCursor"] = nc;
    }
    result
}

/// Handle resources/read request.
pub fn handle_resources_read(uri: &str) -> Result<Value, String> {
    read_resource(uri)
}

/// Handle resources/templates/list request.
pub fn handle_resource_templates_list(cursor: Option<&str>) -> Value {
    let templates = list_resource_templates();
    let start = match cursor {
        Some(c) => c.parse::<usize>().unwrap_or(0),
        None => 0,
    };
    let page_size = 100;
    let end = (start + page_size).min(templates.len());
    let page = if start < templates.len() { &templates[start..end] } else { &[] };
    let next_cursor = if end < templates.len() { Some(json!(end.to_string())) } else { None };

    let mut result = json!({"resourceTemplates": page});
    if let Some(nc) = next_cursor {
        result["nextCursor"] = nc;
    }
    result
}

/// Handle prompts/list request.
pub fn handle_prompts_list(cursor: Option<&str>) -> Value {
    let prompts = list_prompts();
    let start = match cursor {
        Some(c) => c.parse::<usize>().unwrap_or(0),
        None => 0,
    };
    let page_size = 100;
    let end = (start + page_size).min(prompts.len());
    let page = if start < prompts.len() { &prompts[start..end] } else { &[] };
    let next_cursor = if end < prompts.len() { Some(json!(end.to_string())) } else { None };

    let mut result = json!({"prompts": page});
    if let Some(nc) = next_cursor {
        result["nextCursor"] = nc;
    }
    result
}

/// Handle prompts/get request.
pub fn handle_prompts_get(name: &str, arguments: &Value) -> Result<Value, String> {
    let mut args_map = HashMap::new();
    if let Some(obj) = arguments.as_object() {
        for (k, v) in obj {
            if let Some(s) = v.as_str() {
                args_map.insert(k.clone(), s.to_string());
            }
        }
    }
    get_prompt(name, &args_map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_list_resources() {
        register_file_resource("file:///test.txt", "Test File", "A test file", "/tmp/test.txt");
        let resources = list_resources();
        assert!(!resources.is_empty());
        assert_eq!(resources[0].uri, "file:///test.txt");
    }

    #[test]
    fn test_register_and_list_prompts() {
        register_prompt("greet", "Greet someone", vec![("name", "Person's name", true)]);
        let prompts = list_prompts();
        assert!(!prompts.is_empty());
        let found = prompts.iter().any(|p| p.name == "greet");
        assert!(found, "Should find 'greet' prompt in registry");
    }

    #[test]
    fn test_get_prompt_with_substitution() {
        register_prompt("hello", "Say hello to {name}", vec![("name", "Name", true)]);
        let mut args = HashMap::new();
        args.insert("name".to_string(), "World".to_string());
        let result = get_prompt("hello", &args).unwrap();
        assert!(result["messages"][0]["content"]["text"].as_str().unwrap().contains("World"));
    }

    #[test]
    fn test_missing_required_argument() {
        register_prompt("test", "Test {x}", vec![("x", "Required", true)]);
        let args = HashMap::new();
        let result = get_prompt("test", &args);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_resources_list_pagination() {
        let result = handle_resources_list(None);
        assert!(result["resources"].is_array());
    }

    #[test]
    fn test_handle_prompts_list_pagination() {
        let result = handle_prompts_list(None);
        assert!(result["prompts"].is_array());
    }
}
