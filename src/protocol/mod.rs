pub mod json_rpc;
pub mod nda_native;
pub mod nmcp_binary;

use serde_json::json;
use serde_json::Value;

/// Build a standardized initialize response for MCP protocol.
///
/// Only capabilities the server can actually emit are advertised: a `true`
/// flag here is a promise that the corresponding notification will arrive.
///
/// `include_extra` controls whether the elicitation capability is included.
/// JSON-RPC stdio mode includes it; NDA modes omit it for minimal wire size.
///
/// `roots` is deliberately absent: in the MCP spec it is a *client*
/// capability, advertised by the client in the initialize request, and has no
/// place in this server-side map.
pub fn build_initialize_response(include_extra: bool) -> Value {
    let mut capabilities = serde_json::Map::new();

    // `register_tool_lazy` bumps the registry generation and the HTTP server
    // broadcasts `notifications/tools/list_changed` when it moves.
    capabilities.insert("tools".to_string(), json!({ "listChanged": true }));
    // Resource subscriptions are recorded in-process but no transport poller
    // delivers them, so neither side of the promise can be kept.
    capabilities.insert(
        "resources".to_string(),
        json!({ "subscribe": false, "listChanged": false }),
    );
    capabilities.insert("prompts".to_string(), json!({ "listChanged": false }));
    capabilities.insert("sampling".to_string(), json!({}));
    capabilities.insert("logging".to_string(), json!({}));

    if include_extra {
        capabilities.insert("elicitation".to_string(), json!({}));
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

/// Parse a `logging/setLevel` parameter, store it, and build the reply.
///
/// Shared so a level the JSON-RPC dispatcher rejects is rejected on the
/// shared-memory dispatcher too; `nmcp_binary::tests` pins that the two answer
/// the same request identically. Storing does not retune the live `tracing`
/// filter, which is fixed at startup -- see docs/API.md, "Declared Capabilities".
pub(crate) fn logging_set_level_response(id: &Value, level_str: &str) -> Value {
    let level = match level_str {
        "debug" => tracing::Level::DEBUG,
        "info" => tracing::Level::INFO,
        "notice" | "warning" => tracing::Level::WARN,
        "error" => tracing::Level::ERROR,
        "critical" | "alert" | "emergency" => tracing::Level::ERROR,
        _ => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32602,
                    "message": format!("Invalid log level: '{}'", level_str)
                }
            });
        }
    };
    if let Ok(mut current) = json_rpc::LOG_LEVEL.lock() {
        *current = level;
    }
    tracing::info!(level = level_str, "Log level changed");
    json!({"jsonrpc": "2.0", "id": id, "result": {}})
}

/// Page a `tools/list` reply the way the JSON-RPC dispatcher pages it.
///
/// The shared-memory arm used to return the whole registry and ignore
/// `params.cursor`, so one request got two answers -- and past the output
/// buffer ceiling in `SharedMemoryBuffer::write_output_raw`, no answer at all.
/// This duplicates the JSON-RPC arm instead of replacing it because that arm's
/// file is cited by line number downstream; `nmcp_binary`'s parity test pins
/// that the two stay equal.
pub(crate) fn tools_list_page_response(id: &Value, cursor: Option<&str>) -> Value {
    let all_tools = crate::registry::get_tools();
    let start = match cursor {
        Some(c) => c.parse::<usize>().unwrap_or(0),
        None => 0,
    };
    let end = start
        .saturating_add(json_rpc::DEFAULT_PAGE_SIZE)
        .min(all_tools.len());
    let page_tools = if start < all_tools.len() {
        &all_tools[start..end]
    } else {
        &[]
    };
    let mut result = json!({ "tools": page_tools });
    if end < all_tools.len() {
        result["nextCursor"] = json!(end.to_string());
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}
