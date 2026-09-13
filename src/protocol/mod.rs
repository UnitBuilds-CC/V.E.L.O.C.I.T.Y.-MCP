pub mod json_rpc;
pub mod nda_native;
pub mod nmcp_binary;

use serde_json::json;
use serde_json::Value;

/// Build a standardized initialize response for MCP protocol.
///
/// `include_extra` controls whether elicitation and roots capabilities are included.
/// JSON-RPC stdio mode includes them; NDA modes omit them for minimal wire size.
pub fn build_initialize_response(include_extra: bool) -> Value {
    let mut capabilities = serde_json::Map::new();

    capabilities.insert("tools".to_string(), json!({ "listChanged": true }));
    capabilities.insert(
        "resources".to_string(),
        json!({ "subscribe": true, "listChanged": true }),
    );
    capabilities.insert("prompts".to_string(), json!({ "listChanged": true }));
    capabilities.insert("sampling".to_string(), json!({}));
    capabilities.insert("logging".to_string(), json!({}));

    if include_extra {
        capabilities.insert("elicitation".to_string(), json!({}));
        capabilities.insert("roots".to_string(), json!({ "listChanged": true }));
    }

    json!({
        "protocolVersion": crate::PROTOCOL_VERSION,
        "capabilities": capabilities,
        "serverInfo": {
            "name": "velocity-mcp-rust-server",
            "version": crate::VERSION
        }
    })
}
