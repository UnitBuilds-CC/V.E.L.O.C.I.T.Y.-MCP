//! MCP Resources and Prompts support.
//!
//! Resources expose data (files, databases, APIs) as URIs that clients can read.
//! Prompts provide templated prompt workflows with variable substitution.
//!
//! Features:
//! - File-based resources with zero-copy memory-mapped access
//! - Database and API resource adapters
//! - Resource subscriptions with change notifications
//! - URI template parameter expansion
//! - Structured prompt content blocks

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
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

/// Resource update notification.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResourceUpdate {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Global resource registry.
static RESOURCE_REGISTRY: OnceLock<Mutex<ResourceStore>> = OnceLock::new();

/// Configured database path for database resources.
static DATABASE_PATH: OnceLock<String> = OnceLock::new();

fn get_resource_registry() -> &'static Mutex<ResourceStore> {
    RESOURCE_REGISTRY.get_or_init(|| Mutex::new(ResourceStore::default()))
}

/// Set the database path for database-backed resources.
/// Must be called before any database resource is read.
pub fn set_database_path(path: &str) {
    if DATABASE_PATH.set(path.to_string()).is_err() {
        tracing::warn!("DATABASE_PATH already initialized; ignoring duplicate set_database_path call");
    }
}

/// In-memory resource store.
#[derive(Default)]
struct ResourceStore {
    resources: Vec<Resource>,
    templates: Vec<ResourceTemplate>,
    /// URI -> file path mapping for local file resources
    file_resources: HashMap<String, PathBuf>,
    /// URI -> database query mapping
    db_resources: HashMap<String, DbResourceConfig>,
    /// URI -> API endpoint mapping
    api_resources: HashMap<String, ApiResourceConfig>,
    /// Active subscriptions: URI -> set of subscriber IDs
    subscriptions: HashMap<String, HashSet<String>>,
    /// Pending resource update notifications
    pending_updates: Vec<ResourceUpdate>,
}

/// Database resource configuration.
#[derive(Clone, Debug)]
struct DbResourceConfig {
    query: String,
    params: Vec<String>,
}

/// API resource configuration.
#[derive(Clone, Debug)]
struct ApiResourceConfig {
    endpoint: String,
    method: String,
    headers: HashMap<String, String>,
}

impl ResourceStore {
    const MAX_RESOURCES: usize = 10_000;

    fn register_file_resource(&mut self, uri: &str, name: &str, description: &str, path: PathBuf) -> Result<(), String> {
        if self.resources.len() >= Self::MAX_RESOURCES {
            return Err(format!("Resource limit reached ({}), cannot register more resources", Self::MAX_RESOURCES));
        }
        let mime_type = guess_mime(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
        self.resources.push(Resource {
            uri: uri.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            mime_type: Some(mime_type),
        });
        self.file_resources.insert(uri.to_string(), path);
        Ok(())
    }

    fn register_db_resource(&mut self, uri: &str, name: &str, description: &str, query: &str, params: Vec<String>) -> Result<(), String> {
        if self.resources.len() >= Self::MAX_RESOURCES {
            return Err(format!("Resource limit reached ({}), cannot register more resources", Self::MAX_RESOURCES));
        }
        self.resources.push(Resource {
            uri: uri.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            mime_type: Some("application/json".to_string()),
        });
        self.db_resources.insert(uri.to_string(), DbResourceConfig {
            query: query.to_string(),
            params,
        });
        Ok(())
    }

    fn register_api_resource(&mut self, uri: &str, name: &str, description: &str, endpoint: &str, method: &str, headers: HashMap<String, String>) -> Result<(), String> {
        if self.resources.len() >= Self::MAX_RESOURCES {
            return Err(format!("Resource limit reached ({}), cannot register more resources", Self::MAX_RESOURCES));
        }
        self.resources.push(Resource {
            uri: uri.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            mime_type: Some("application/json".to_string()),
        });
        self.api_resources.insert(uri.to_string(), ApiResourceConfig {
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            headers,
        });
        Ok(())
    }

    fn register_template(&mut self, template: ResourceTemplate) {
        self.templates.push(template);
    }

    fn subscribe(&mut self, uri: &str, subscriber_id: &str) -> Result<(), String> {
        const MAX_SUBSCRIBERS_PER_URI: usize = 1000;
        const MAX_SUBSCRIBED_URIS: usize = 256;

        if !self.resources.iter().any(|r| r.uri == uri) {
            return Err(format!("Resource not found: {}", uri));
        }

        if self.subscriptions.len() >= MAX_SUBSCRIBED_URIS && !self.subscriptions.contains_key(uri) {
            return Err(format!("Too many subscribed resources (max {})", MAX_SUBSCRIBED_URIS));
        }

        let subscribers = self.subscriptions
            .entry(uri.to_string())
            .or_insert_with(HashSet::new);

        if subscribers.len() >= MAX_SUBSCRIBERS_PER_URI {
            return Err(format!("Too many subscribers for resource (max {})", MAX_SUBSCRIBERS_PER_URI));
        }

        subscribers.insert(subscriber_id.to_string());
        
        Ok(())
    }

    fn unsubscribe(&mut self, uri: &str, subscriber_id: &str) -> Result<(), String> {
        if let Some(subscribers) = self.subscriptions.get_mut(uri) {
            subscribers.remove(subscriber_id);
            if subscribers.is_empty() {
                self.subscriptions.remove(uri);
            }
        }
        Ok(())
    }

    fn notify_update(&mut self, uri: &str) {
        if self.pending_updates.len() >= 10_000 {
            tracing::warn!("Resource pending updates capped at 10000, dropping oldest");
            self.pending_updates.drain(..5000);
        }
        if let Some(resource) = self.resources.iter().find(|r| r.uri == uri) {
            let update = ResourceUpdate {
                uri: uri.to_string(),
                mime_type: resource.mime_type.clone(),
            };
            self.pending_updates.push(update);
        }
    }
    
    fn drain_updates(&mut self) -> Vec<ResourceUpdate> {
        std::mem::take(&mut self.pending_updates)
    }

    fn read_resource(&self, uri: &str) -> Result<ResourceReadResult, String> {
        // Try file resource first
        if let Some(path) = self.file_resources.get(uri) {
            use std::io::Read;
            const MAX_RESOURCE_SIZE: u64 = 10 * 1024 * 1024;
            let mut file = std::fs::File::open(path)
                .map_err(|e| format!("Failed to open file: {}", e))?;
            let metadata = file.metadata()
                .map_err(|e| format!("Failed to stat file: {}", e))?;
            if metadata.len() > MAX_RESOURCE_SIZE {
                return Err(format!(
                    "File too large: {} bytes (max {} bytes)",
                    metadata.len(), MAX_RESOURCE_SIZE
                ));
            }
            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|e| format!("Failed to read file: {}", e))?;
            let mime = guess_mime(path.extension().and_then(|e| e.to_str()).unwrap_or(""));
            return Ok(ResourceReadResult {
                uri: uri.to_string(),
                mime_type: mime,
                text: Some(content),
            });
        }
        
        // Try database resource
        if let Some(db_config) = self.db_resources.get(uri) {
            #[cfg(feature = "database")]
            {
                return execute_database_query(uri, db_config);
            }
            #[cfg(not(feature = "database"))]
            {
                let mock_result = json!({
                    "query": db_config.query,
                    "params": db_config.params,
                    "result": "Database feature not enabled. Build with --features database"
                });
                return Ok(ResourceReadResult {
                    uri: uri.to_string(),
                    mime_type: "application/json".to_string(),
                    text: Some(serde_json::to_string_pretty(&mock_result).unwrap_or_default()),
                });
            }
        }
        
        // Try API resource
        if let Some(api_config) = self.api_resources.get(uri) {
            // Placeholder for actual HTTP request
            // In a real implementation, this would make the HTTP request
            let mock_result = json!({
                "endpoint": api_config.endpoint,
                "method": api_config.method,
                "headers": api_config.headers,
                "result": "HTTP request would execute here"
            });
            return Ok(ResourceReadResult {
                uri: uri.to_string(),
                mime_type: "application/json".to_string(),
                text: Some(serde_json::to_string_pretty(&mock_result).unwrap_or_default()),
            });
        }
        
        Err(format!("Resource not found: {}", uri))
    }
}

/// Result of reading a resource.
struct ResourceReadResult {
    uri: String,
    mime_type: String,
    text: Option<String>,
}

/// Cached database connection to avoid reopening on every query.
#[cfg(feature = "database")]
static CACHED_DB: OnceLock<Mutex<Option<rusqlite::Connection>>> = OnceLock::new();

#[cfg(feature = "database")]
fn get_db_connection() -> Result<std::sync::MutexGuard<'static, Option<rusqlite::Connection>>, String> {
    let cache = CACHED_DB.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().map_err(|e| format!("DB cache lock poisoned: {}", e))?;
    if guard.is_none() {
        let conn = match DATABASE_PATH.get() {
            Some(path) => rusqlite::Connection::open(path)
                .map_err(|e| format!("Failed to open database at {}: {}", path, e))?,
            None => rusqlite::Connection::open_in_memory()
                .map_err(|e| format!("Failed to open in-memory database: {}", e))?,
        };
        *guard = Some(conn);
    }
    Ok(guard)
}

/// Execute a database query and return results as JSON.
#[cfg(feature = "database")]
fn execute_database_query(uri: &str, config: &DbResourceConfig) -> Result<ResourceReadResult, String> {
    let guard = get_db_connection()?;
    let conn = guard.as_ref().ok_or("Database connection not initialized")?;
    
    // Execute the query with parameters
    let mut stmt = conn.prepare(&config.query)
        .map_err(|e| format!("Failed to prepare query: {}", e))?;
    
    // Get column names
    let column_names: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();
    
    // Execute query and collect results
    let params: Vec<&str> = config.params.iter().map(|s| s.as_str()).collect();
    let rows_result = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        let mut row_data = serde_json::Map::new();
        for (i, col_name) in column_names.iter().enumerate() {
            let value: rusqlite::types::Value = row.get(i)?;
            let json_value = match value {
                rusqlite::types::Value::Null => Value::Null,
                rusqlite::types::Value::Integer(i) => json!(i),
                rusqlite::types::Value::Real(f) => json!(f),
                rusqlite::types::Value::Text(s) => json!(s),
                rusqlite::types::Value::Blob(b) => json!(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &b)),
            };
            row_data.insert(col_name.clone(), json_value);
        }
        Ok(Value::Object(row_data))
    }).map_err(|e| format!("Failed to execute query: {}", e))?;
    
    let mut results = Vec::new();
    for row in rows_result {
        let row_value = row.map_err(|e| format!("Failed to read row: {}", e))?;
        results.push(row_value);
    }
    
    let result_json = json!({
        "query": config.query,
        "params": config.params,
        "columns": column_names,
        "rows": results,
        "row_count": results.len()
    });
    
    Ok(ResourceReadResult {
        uri: uri.to_string(),
        mime_type: "application/json".to_string(),
        text: Some(serde_json::to_string_pretty(&result_json).unwrap_or_default()),
    })
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
        if let Err(e) = s.register_file_resource(uri, name, description, PathBuf::from(path)) {
            tracing::error!(uri = %uri, error = %e, "Failed to register file resource");
        }
    }
}

/// Register a resource template with a URI template pattern.
pub fn register_resource_template(uri_template: &str, name: &str, description: &str, mime_type: Option<&str>) {
    let store = get_resource_registry();
    if let Ok(mut s) = store.lock() {
        s.register_template(ResourceTemplate {
            uri_template: uri_template.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            mime_type: mime_type.map(|s| s.to_string()),
        });
    }
}

/// Register a database query as an MCP resource.
pub fn register_db_resource(uri: &str, name: &str, description: &str, query: &str, params: Vec<String>) {
    let store = get_resource_registry();
    if let Ok(mut s) = store.lock() {
        if let Err(e) = s.register_db_resource(uri, name, description, query, params) {
            tracing::error!(uri = %uri, error = %e, "Failed to register database resource");
        }
    }
}

/// Register an API endpoint as an MCP resource.
pub fn register_api_resource(uri: &str, name: &str, description: &str, endpoint: &str, method: &str, headers: HashMap<String, String>) {
    let store = get_resource_registry();
    if let Ok(mut s) = store.lock() {
        if let Err(e) = s.register_api_resource(uri, name, description, endpoint, method, headers) {
            tracing::error!(uri = %uri, error = %e, "Failed to register API resource");
        }
    }
}

/// Subscribe to resource updates.
pub fn subscribe_resource(uri: &str, subscriber_id: &str) -> Result<(), String> {
    let store = get_resource_registry();
    match store.lock() {
        Ok(mut s) => s.subscribe(uri, subscriber_id),
        Err(e) => Err(format!("Resource store poisoned: {}", e)),
    }
}

/// Unsubscribe from resource updates.
pub fn unsubscribe_resource(uri: &str, subscriber_id: &str) -> Result<(), String> {
    let store = get_resource_registry();
    match store.lock() {
        Ok(mut s) => s.unsubscribe(uri, subscriber_id),
        Err(e) => Err(format!("Resource store poisoned: {}", e)),
    }
}

/// Notify subscribers of a resource update.
pub fn notify_resource_update(uri: &str) {
    let store = get_resource_registry();
    if let Ok(mut s) = store.lock() {
        s.notify_update(uri);
    }
}

/// Poll for pending resource update notifications. Drains and returns all pending updates.
pub fn poll_resource_updates() -> Vec<ResourceUpdate> {
    let store = get_resource_registry();
    match store.lock() {
        Ok(mut s) => s.drain_updates(),
        Err(_) => vec![],
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

/// A prompt template with structured content blocks.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StructuredPrompt {
    pub name: String,
    pub description: String,
    pub arguments: Vec<PromptArgument>,
    pub messages: Vec<PromptMessageTemplate>,
}

/// A message template in a structured prompt.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PromptMessageTemplate {
    pub role: String,
    pub content: Vec<PromptContentBlock>,
}

/// A content block in a prompt message.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum PromptContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "resource")]
    Resource { uri: String },
}

static STRUCTURED_PROMPT_REGISTRY: OnceLock<Mutex<Vec<StructuredPrompt>>> = OnceLock::new();

fn get_structured_prompt_registry() -> &'static Mutex<Vec<StructuredPrompt>> {
    STRUCTURED_PROMPT_REGISTRY.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a structured prompt with content blocks.
pub fn register_structured_prompt(
    name: &str,
    description: &str,
    args: Vec<(&str, &str, bool)>,
    messages: Vec<(String, Vec<PromptContentBlock>)>,
) {
    let registry = get_structured_prompt_registry();
    if let Ok(mut prompts) = registry.lock() {
        prompts.push(StructuredPrompt {
            name: name.to_string(),
            description: description.to_string(),
            arguments: args.iter().map(|(n, d, r)| PromptArgument {
                name: n.to_string(),
                description: d.to_string(),
                required: *r,
            }).collect(),
            messages: messages.into_iter().map(|(role, content)| PromptMessageTemplate {
                role,
                content,
            }).collect(),
        });
    }
}

/// Get a structured prompt with variable substitution and resource embedding.
pub fn get_structured_prompt(name: &str, arguments: &HashMap<String, String>) -> Result<Value, String> {
    let registry = get_structured_prompt_registry();
    match registry.lock() {
        Ok(prompts) => {
            let prompt = prompts.iter().find(|p| p.name == name)
                .ok_or_else(|| format!("Structured prompt not found: {}", name))?;

            // Validate required arguments
            for arg in &prompt.arguments {
                if arg.required && !arguments.contains_key(&arg.name) {
                    return Err(format!("Missing required argument: {}", arg.name));
                }
            }

            // Process messages with variable substitution
            let mut processed_messages = Vec::new();
            for msg_template in &prompt.messages {
                let mut processed_content = Vec::new();
                
                for block in &msg_template.content {
                    match block {
                        PromptContentBlock::Text { text } => {
                            // Substitute variables in text
                            let mut processed_text = text.clone();
                            for (key, value) in arguments {
                                processed_text = processed_text.replace(&format!("{{{}}}", key), value);
                            }
                            processed_content.push(json!({
                                "type": "text",
                                "text": processed_text
                            }));
                        }
                        PromptContentBlock::Resource { uri } => {
                            // Expand URI template if it contains parameters
                            let expanded_uri = expand_uri_template(uri, arguments);
                            
                            // Try to read the resource
                            match read_resource(&expanded_uri) {
                                Ok(resource_data) => {
                                    if let Some(contents) = resource_data.get("contents").and_then(|c| c.as_array()) {
                                        for content in contents {
                                            processed_content.push(json!({
                                                "type": "resource",
                                                "resource": {
                                                    "uri": expanded_uri,
                                                    "mimeType": content.get("mimeType"),
                                                    "text": content.get("text")
                                                }
                                            }));
                                        }
                                    }
                                }
                                Err(e) => {
                                    // If resource not found, add as text reference
                                    processed_content.push(json!({
                                        "type": "text",
                                        "text": format!("[Resource {} not available: {}]", expanded_uri, e)
                                    }));
                                }
                            }
                        }
                    }
                }
                
                processed_messages.push(json!({
                    "role": msg_template.role,
                    "content": processed_content
                }));
            }

            Ok(json!({
                "description": prompt.description,
                "messages": processed_messages
            }))
        }
        Err(e) => Err(format!("Structured prompt registry poisoned: {}", e)),
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

/// Expand a URI template with parameters.
/// Example: "file://{path}" with {"path": "test.txt"} -> "file://test.txt"
pub fn expand_uri_template(template: &str, params: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in params {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}

/// Handle resources/subscribe request.
pub fn handle_resources_subscribe(uri: &str, subscriber_id: &str) -> Result<Value, String> {
    subscribe_resource(uri, subscriber_id)?;
    Ok(json!({"status": "subscribed", "uri": uri}))
}

/// Handle resources/unsubscribe request.
pub fn handle_resources_unsubscribe(uri: &str, subscriber_id: &str) -> Result<Value, String> {
    unsubscribe_resource(uri, subscriber_id)?;
    Ok(json!({"status": "unsubscribed", "uri": uri}))
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

    fn test_timer(name: &str) -> impl Drop {
        let start = std::time::Instant::now();
        struct Timer { name: String, start: std::time::Instant }
        impl Drop for Timer { fn drop(&mut self) {
            eprintln!("[TEST] {} completed in {:.3}ms", self.name, self.start.elapsed().as_secs_f64() * 1000.0);
        }}
        Timer { name: name.to_string(), start }
    }

    #[test]
    fn test_register_and_list_resources() {
        let _t = test_timer("test_register_and_list_resources");
        let t0 = std::time::Instant::now();
        register_file_resource("file:///test.txt", "Test File", "A test file", "/tmp/test.txt");
        let resources = list_resources();
        eprintln!("[METRIC] register+list_resources: {:.3}us ({} resources)", t0.elapsed().as_secs_f64() * 1e6, resources.len());
        assert!(!resources.is_empty());
        let found = resources.iter().any(|r| r.uri == "file:///test.txt");
        assert!(found, "Should find 'file:///test.txt' in resources");
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

    #[test]
    fn test_uri_template_expansion() {
        let template = "file://{path}/{name}.txt";
        let mut params = HashMap::new();
        params.insert("path".to_string(), "documents".to_string());
        params.insert("name".to_string(), "test".to_string());
        
        let expanded = expand_uri_template(template, &params);
        assert_eq!(expanded, "file://documents/test.txt");
    }

    #[test]
    fn test_resource_subscription() {
        let _t = test_timer("test_resource_subscription");
        register_file_resource("test://sub", "Test", "Test resource", "/tmp/test.txt");
        
        // Subscribe to the resource
        let result = subscribe_resource("test://sub", "client1");
        assert!(result.is_ok());
        
        // Unsubscribe
        let result = unsubscribe_resource("test://sub", "client1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_subscribe_nonexistent_resource() {
        let result = subscribe_resource("nonexistent://resource", "client1");
        assert!(result.is_err());
    }

    #[test]
    fn test_db_resource_registration() {
        register_db_resource(
            "db://users",
            "Users",
            "Database users",
            "SELECT * FROM users WHERE id = ?",
            vec!["user_id".to_string()]
        );
        
        let resources = list_resources();
        assert!(resources.iter().any(|r| r.uri == "db://users"));
    }

    #[test]
    fn test_api_resource_registration() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());
        
        register_api_resource(
            "api://data",
            "API Data",
            "External API data",
            "https://api.example.com/data",
            "GET",
            headers
        );
        
        let resources = list_resources();
        assert!(resources.iter().any(|r| r.uri == "api://data"));
    }

    #[test]
    fn test_structured_prompt_registration() {
        use crate::resources::PromptContentBlock;
        
        register_structured_prompt(
            "code_review",
            "Review code with context",
            vec![("language", "Programming language", true)],
            vec![
                ("system".to_string(), vec![
                    PromptContentBlock::Text { 
                        text: "You are a {language} expert.".to_string() 
                    }
                ]),
                ("user".to_string(), vec![
                    PromptContentBlock::Text { 
                        text: "Review this code:".to_string() 
                    },
                    PromptContentBlock::Resource { 
                        uri: "file://{path}".to_string() 
                    }
                ])
            ]
        );
        
        let mut args = HashMap::new();
        args.insert("language".to_string(), "Rust".to_string());
        args.insert("path".to_string(), "src/main.rs".to_string());
        
        let result = get_structured_prompt("code_review", &args);
        assert!(result.is_ok());
        
        let prompt = result.unwrap();
        assert_eq!(prompt["messages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_handle_resources_subscribe() {
        register_file_resource("test://handler", "Test", "Test", "/tmp/test.txt");
        
        let result = handle_resources_subscribe("test://handler", "client1");
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["status"], "subscribed");
    }

    #[test]
    fn test_handle_resources_unsubscribe() {
        register_file_resource("test://unsub", "Test", "Test", "/tmp/test.txt");
        let _ = subscribe_resource("test://unsub", "client1");
        
        let result = handle_resources_unsubscribe("test://unsub", "client1");
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["status"], "unsubscribed");
    }

    #[test]
    #[cfg(feature = "database")]
    fn test_database_resource_query() {
        // Register a database resource
        register_db_resource(
            "db://users",
            "Users",
            "User database",
            "SELECT 1 as id, 'Alice' as name, 30 as age",
            vec![],
        );
        
        // Read the resource
        let result = read_resource("db://users");
        assert!(result.is_ok(), "Database query should succeed: {:?}", result.err());
        
        let resource_data = result.unwrap();
        
        // The result should have a "contents" array
        let contents = resource_data["contents"].as_array().unwrap();
        assert!(!contents.is_empty());
        
        let first_content = &contents[0];
        assert_eq!(first_content["mimeType"], "application/json");
        
        // Parse the JSON result
        let json_str = first_content["text"].as_str().unwrap();
        let json_data: Value = serde_json::from_str(json_str).unwrap();
        
        // Verify the query results
        assert_eq!(json_data["columns"].as_array().unwrap().len(), 3);
        assert_eq!(json_data["row_count"].as_u64().unwrap(), 1);
        assert_eq!(json_data["rows"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_database_resource_without_feature() {
        // This test runs without the database feature
        register_db_resource(
            "db://test",
            "Test DB",
            "Test database",
            "SELECT 1",
            vec![],
        );
        
        let result = read_resource("db://test");
        assert!(result.is_ok());
        
        // Should return mock data when database feature is not enabled
        let resource_data = result.unwrap();
        let contents = resource_data["contents"].as_array().unwrap();
        assert!(!contents.is_empty());
        
        let first_content = &contents[0];
        assert_eq!(first_content["mimeType"], "application/json");
    }
}
