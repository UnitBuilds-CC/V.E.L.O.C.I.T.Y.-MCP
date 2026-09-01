use serde_json::{json, Value};
use std::io::{self, BufRead};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Mutex};
use std::thread;
use std::time::Instant;
use tracing::{info, warn, debug, error};
use crate::registry;
use crate::audit::{self, AuditOutcome};
use crate::rate_limit;
use crate::resources;
use crate::sampling;
use crate::streaming;
#[cfg(feature = "oauth2")]
use crate::oauth2;

const MAX_REQUEST_SIZE: usize = 1_048_576;
const DEFAULT_PAGE_SIZE: usize = 100;
const MAX_CANCELLED_IDS: usize = 1024;

static LOG_LEVEL: Mutex<tracing::Level> = Mutex::new(tracing::Level::INFO);

static CANCELLED_IDS: std::sync::LazyLock<Mutex<std::collections::HashSet<Value>>> = 
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

pub fn is_cancelled(id: &Value) -> bool {
    if let Ok(ids) = CANCELLED_IDS.lock() {
        ids.contains(id)
    } else {
        false
    }
}

fn add_cancelled(id: Value) {
    if let Ok(mut ids) = CANCELLED_IDS.lock() {
        if ids.len() >= MAX_CANCELLED_IDS {
            // Evict oldest entries (arbitrary since HashSet is unordered)
            ids.clear();
        }
        ids.insert(id);
    }
}

fn remove_cancelled(id: &Value) {
    if let Ok(mut ids) = CANCELLED_IDS.lock() {
        ids.remove(id);
    }
}

pub fn handle_request(request: &Value) -> Option<Value> {
    let method = request["method"].as_str().unwrap_or("");
    let id = &request["id"];

    debug!(method = method, "Processing JSON-RPC request");

    match method {
        "initialize" => {
            let client_name = request["params"]["clientInfo"]["name"].as_str().unwrap_or("unknown");
            let client_version = request["params"]["clientInfo"]["version"].as_str().unwrap_or("unknown");
            info!(client = client_name, version = client_version, "Client initializing");
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": crate::PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "resources": { "subscribe": true, "listChanged": true },
                        "prompts": { "listChanged": true },
                        "sampling": {},
                        "logging": {},
                        "elicitation": {},
                        "roots": { "listChanged": true }
                    },
                    "serverInfo": {
                        "name": "velocity-mcp-rust-server",
                        "version": crate::VERSION
                    }
                }
            }))
        }
        "notifications/initialized" => {
            debug!("Client confirmed initialization");
            None
        }
        "notifications/cancelled" => {
            let request_id = &request["params"]["requestId"];
            if !request_id.is_null() {
                add_cancelled(request_id.clone());
                let reason = request["params"]["reason"].as_str().unwrap_or("");
                debug!(request_id = %request_id, reason = reason, "Request cancelled by client");
            }
            None
        }
        "notifications/progress" => {
            streaming::handle_progress_notification(&request["params"]);
            None
        }
        "ping" => {
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }))
        }
        "logging/setLevel" => {
            let level_str = request["params"]["level"].as_str().unwrap_or("info");
            let level = match level_str {
                "debug" => tracing::Level::DEBUG,
                "info" => tracing::Level::INFO,
                "notice" | "warning" => tracing::Level::WARN,
                "error" => tracing::Level::ERROR,
                "critical" | "alert" | "emergency" => tracing::Level::ERROR,
                _ => {
                    return Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32602,
                            "message": format!("Invalid log level: '{}'", level_str)
                        }
                    }));
                }
            };
            if let Ok(mut current) = LOG_LEVEL.lock() {
                *current = level;
            }
            info!(level = level_str, "Log level changed");
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {}
            }))
        }
        "tools/list" => {
            let all_tools = registry::get_tools();
            let cursor = request["params"]["cursor"].as_str();
            let start = match cursor {
                Some(c) => c.parse::<usize>().unwrap_or(0),
                None => 0,
            };
            let end = (start + DEFAULT_PAGE_SIZE).min(all_tools.len());
            let page_tools = if start < all_tools.len() {
                &all_tools[start..end]
            } else {
                &[]
            };
            let next_cursor = if end < all_tools.len() {
                Some(json!(end.to_string()))
            } else {
                None
            };
            let mut result = json!({
                "tools": page_tools
            });
            if let Some(nc) = next_cursor {
                result["nextCursor"] = nc;
            }
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            }))
        }
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let arguments = &request["params"]["arguments"];

            if is_cancelled(id) {
                remove_cancelled(id);
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{"type": "text", "text": "Request was cancelled"}],
                        "isError": true
                    }
                }));
            }

            if !rate_limit::check_rate_limit() {
                warn!(tool = name, "Rate limit exceeded");
                audit::record_tool_call(name, Instant::now(), AuditOutcome::Rejected("rate limited".into()));
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{
                            "type": "text",
                            "text": format!("Rate limit exceeded for tool '{}'. Please slow down.", name)
                        }],
                        "isError": true
                    }
                }));
            }

            let call_start = Instant::now();
            let mut is_error = false;
            let output_text = match registry::call_tool(name, arguments) {
                Ok(res) => {
                    audit::record_tool_call(name, call_start, AuditOutcome::Success);
                    res
                }
                Err(e) => {
                    is_error = true;
                    let err_msg = e.to_string();
                    error!(tool = name, error = %err_msg, "Tool execution failed");
                    audit::record_tool_call(name, call_start, AuditOutcome::Error(err_msg.clone()));
                    
                    // Classify error and provide actionable hint for the LLM
                    let (error_type, hint) = if err_msg.contains("not found") || err_msg.contains("No such file") {
                        ("NOT_FOUND", "Check that the path or resource exists.")
                    } else if err_msg.contains("Permission denied") || err_msg.contains("access") {
                        ("PERMISSION_DENIED", "Check file permissions or run with elevated privileges.")
                    } else if err_msg.contains("required") || err_msg.contains("missing") {
                        ("INVALID_ARGUMENTS", "Check that all required parameters are provided with correct types.")
                    } else if err_msg.contains("timeout") || err_msg.contains("timed out") {
                        ("TIMEOUT", "The operation took too long. Try again or increase timeout.")
                    } else if err_msg.contains("connection") || err_msg.contains("network") {
                        ("NETWORK_ERROR", "A network error occurred. Check connectivity and retry.")
                    } else {
                        ("EXECUTION_ERROR", "Review the error message and adjust inputs accordingly.")
                    };
                    
                    format!(
                        "Error running tool '{}': {}\n\nError type: {}\nHint: {}",
                        name, err_msg, error_type, hint
                    )
                }
            };

            if is_cancelled(id) {
                remove_cancelled(id);
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{"type": "text", "text": "Request was cancelled during execution"}],
                        "isError": true
                    }
                }));
            }

            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [
                        {
                            "type": "text",
                            "text": output_text
                        }
                    ],
                    "isError": is_error
                }
            }))
        }
        "health/check" => {
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "status": "healthy",
                    "mode": "stdio",
                    "version": crate::VERSION
                }
            }))
        }
        "resources/list" => {
            let cursor = request["params"]["cursor"].as_str();
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": resources::handle_resources_list(cursor)
            }))
        }
        "resources/read" => {
            let uri = request["params"]["uri"].as_str().unwrap_or("");
            match resources::handle_resources_read(uri) {
                Ok(result) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{"type": "text", "text": e}], "isError": true }
                }))
            }
        }
        "resources/templates/list" => {
            let cursor = request["params"]["cursor"].as_str();
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": resources::handle_resource_templates_list(cursor)
            }))
        }
        "resources/subscribe" => {
            let uri = request["params"]["uri"].as_str().unwrap_or("");
            let subscriber_id = request["params"]["subscriberId"].as_str().unwrap_or("default");
            match resources::handle_resources_subscribe(uri, subscriber_id) {
                Ok(result) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{"type": "text", "text": e}], "isError": true }
                }))
            }
        }
        "resources/unsubscribe" => {
            let uri = request["params"]["uri"].as_str().unwrap_or("");
            let subscriber_id = request["params"]["subscriberId"].as_str().unwrap_or("default");
            match resources::handle_resources_unsubscribe(uri, subscriber_id) {
                Ok(result) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{"type": "text", "text": e}], "isError": true }
                }))
            }
        }
        "prompts/list" => {
            let cursor = request["params"]["cursor"].as_str();
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": resources::handle_prompts_list(cursor)
            }))
        }
        "prompts/get" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let arguments = &request["params"]["arguments"];
            match resources::handle_prompts_get(name, arguments) {
                Ok(result) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "content": [{"type": "text", "text": e}], "isError": true }
                }))
            }
        }
        "sampling/createMessage" => {
            let params = &request["params"];
            match sampling::handle_sampling_create_message(params) {
                Ok(result) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32603, "message": e }
                }))
            }
        }
        #[cfg(feature = "oauth2")]
        "connector/call" => {
            let params = &request["params"];
            match oauth2::handle_connector_call(params) {
                Ok(result) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })),
                Err(e) => Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32603, "message": e }
                }))
            }
        }
        "elicitation/create" => {
            // Elicitation allows the server to request user input during tool execution
            let message = request["params"]["message"].as_str().unwrap_or("");
            debug!(message = message, "Elicitation request from client");
            
            // For now, we just acknowledge the elicitation request
            // In a full implementation, this would pause execution and wait for user input
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "action": "accept",
                    "message": "Elicitation acknowledged"
                }
            }))
        }
        "roots/list" => {
            // Roots define the file system roots that the client has access to
            // This helps the server understand the file system structure
            debug!("Client requesting roots list");
            
            // Return an empty roots list for now
            // In a full implementation, this would be configured by the client
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "roots": []
                }
            }))
        }
        _ => {
            if !id.is_null() {
                warn!(method = method, "Method not found");
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method '{}' not found", method)
                    }
                }))
            } else {
                None
            }
        }
    }
}

/// Run the JSON-RPC stdio event loop.
///
/// Spawns a reader thread that sends stdin lines via an mpsc channel.
/// The main loop uses `recv_timeout` to periodically check the shutdown flag,
/// ensuring the server can exit cleanly even when stdin is blocking.
///
/// Requests exceeding the maximum size (1 MB) are rejected before JSON parsing.
/// Parse errors and unknown methods return proper JSON-RPC error responses.
pub fn run_stdio_loop(shutdown: &AtomicBool) -> Result<(), Box<dyn Error>> {
    use std::io::Read;

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();

    let mut peek_buf = [0u8; 4];
    let peeked = stdin_lock.read(&mut peek_buf)?;

    if peeked == 4 {
        let maybe_len = u32::from_be_bytes(peek_buf);
        // NDA frames are at least 37 bytes (magic+sha256+1 byte payload).
        // "NMCP" as BE u32 = 1.3GB and JSON '{"j...' = ~2GB, both >> 10MB.
        if maybe_len > 36 && maybe_len < 10 * 1024 * 1024 {
            info!("NDA-binary stdio mode detected (length-prefixed)");
            return run_stdio_nda_mode(stdin_lock, maybe_len, shutdown);
        }
    }

    info!("JSON-RPC stdio mode");
    run_stdio_json_mode(stdin_lock, &peek_buf[..peeked], shutdown)
}

fn run_stdio_nda_mode(
    mut stdin_lock: std::io::StdinLock<'_>,
    first_frame_len: u32,
    shutdown: &AtomicBool,
) -> Result<(), Box<dyn Error>> {
    use std::io::{Read, Write};

    let mut frame_buf = Vec::with_capacity(65536);

    let mut frame_len = first_frame_len;
    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutdown signal received, exiting NDA stdio loop");
            break;
        }

        frame_buf.resize(frame_len as usize, 0);
        match stdin_lock.read_exact(&mut frame_buf) {
            Ok(_) => {},
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(Box::new(e)),
        }

        let response_frame = handle_nda_frame(&frame_buf)?;

        let mut stdout = io::stdout();
        stdout.write_all(&(response_frame.len() as u32).to_be_bytes())?;
        stdout.write_all(&response_frame)?;
        stdout.flush()?;

        // Read next frame length
        let mut len_buf = [0u8; 4];
        match stdin_lock.read_exact(&mut len_buf) {
            Ok(_) => frame_len = u32::from_be_bytes(len_buf),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(Box::new(e)),
        }
    }

    Ok(())
}

fn handle_nda_frame(raw: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    use crate::protocol::nda_native;
    
    let req = match nda_native::parse_nda_request_inplace(raw) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "NDA frame parse error in stdio");
            return Ok(nda_native::build_nda_error(&Value::Null, &format!("Parse error: {}", e)));
        }
    };
    
    debug!(method = nda_native::method_name(req.method), "NDA stdio request");
    
    let response_frame = match req.method {
        nda_native::METHOD_PING => {
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
        }
        nda_native::METHOD_INITIALIZE => {
            let result = json!({
                "protocolVersion": crate::PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": true },
                    "resources": { "subscribe": true, "listChanged": true },
                    "prompts": { "listChanged": true },
                    "sampling": {},
                    "logging": {}
                },
                "serverInfo": {
                    "name": "velocity-mcp-rust-server",
                    "version": crate::VERSION
                }
            });
            let mut result_tlv = Vec::new();
            nda_native::encode_json_value(&result, &mut result_tlv);
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, &result_tlv)
        }
        nda_native::NOTIF_INITIALIZED => {
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
        }
        nda_native::METHOD_TOOLS_LIST => {
            let mut payload = Vec::with_capacity(8 * 1024);
            payload.push(nda_native::STATUS_OK);
            payload.extend_from_slice(req.id_tlv);
            payload.extend_from_slice(&nda_native::encoded_tools_list_result());
            nda_native::build_nda_frame(&payload)
        }
        nda_native::METHOD_TOOLS_CALL => {
            let (name_slice, args_slice) = nda_native::extract_tools_call_fields(req.data)
                .unwrap_or((None, None));
            let name = name_slice.unwrap_or("");
            let args_buf = args_slice.unwrap_or(&[]);
            let args_json = if args_buf.is_empty() {
                Value::Object(serde_json::Map::new())
            } else {
                nda_native::decode_json_value(args_buf).map(|(v, _)| v).unwrap_or(Value::Null)
            };
            
            let json_req = json!({
                "method": "tools/call",
                "params": {
                    "name": name,
                    "arguments": args_json
                }
            });
            
            if let Some(json_resp) = handle_request(&json_req) {
                if let Some(err) = json_resp.get("error") {
                    let mut err_tlv = Vec::new();
                    nda_native::encode_json_value(err, &mut err_tlv);
                    nda_native::build_nda_response_raw(nda_native::STATUS_ERROR, req.id_tlv, &err_tlv)
                } else {
                    let result = json_resp.get("result").cloned().unwrap_or(Value::Null);
                    let mut result_tlv = Vec::new();
                    nda_native::encode_json_value(&result, &mut result_tlv);
                    nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, &result_tlv)
                }
            } else {
                nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
            }
        }
        _ => {
            // Fall back to JSON-RPC handler for other methods
            let method_name = nda_native::method_name(req.method);
            let (params, _) = if req.data.is_empty() {
                (Value::Null, 0)
            } else {
                nda_native::decode_json_value(req.data).unwrap_or((Value::Null, 0))
            };
            
            let json_req = json!({
                "method": method_name,
                "params": params,
                "id": 1
            });
            
            if let Some(json_resp) = handle_request(&json_req) {
                let result = json_resp.get("result").cloned().unwrap_or(Value::Null);
                let mut result_tlv = Vec::new();
                nda_native::encode_json_value(&result, &mut result_tlv);
                nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, &result_tlv)
            } else {
                nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
            }
        }
    };
    
    Ok(response_frame)
}

fn run_stdio_json_mode(stdin_lock: std::io::StdinLock<'_>, initial_bytes: &[u8], shutdown: &AtomicBool) -> Result<(), Box<dyn Error>> {
    // Drop the inherited stdin lock so the reader thread can acquire its own.
    drop(stdin_lock);
    // Use a reader thread so stdin reads don't block shutdown checks.
    let (tx, rx) = mpsc::channel::<String>();
    let initial_bytes_owned = initial_bytes.to_vec();
    
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut first_line = true;
        loop {
            let mut line = String::new();
            match handle.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if first_line && !initial_bytes_owned.is_empty() {
                        // Prepend the peeked bytes to the first line
                        let mut full_line = String::from_utf8_lossy(&initial_bytes_owned).to_string();
                        full_line.push_str(&line);
                        line = full_line;
                        first_line = false;
                    }
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    info!("JSON-RPC stdio loop started");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutdown signal received, exiting stdio loop");
            break;
        }

        // Use recv_timeout to periodically check the shutdown flag
        let line = match rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                info!("Stdin reader disconnected, exiting stdio loop");
                break;
            }
        };

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        // Enforce maximum request size before parsing
        if trimmed.len() > MAX_REQUEST_SIZE {
            warn!(size = trimmed.len(), "Request exceeds maximum size");
            let err_res = json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": "Request too large" },
                "id": null
            });
            println!("{}", err_res);
            continue;
        }

        let request: Value = match serde_json::from_str(&trimmed) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "JSON parse error");
                let err_res = json!({
                    "jsonrpc": "2.0",
                    "error": { "code": -32700, "message": "Parse error" },
                    "id": null
                });
                println!("{}", err_res);
                continue;
            }
        };

        if let Some(response) = handle_request(&request) {
            println!("{}", response);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialize_returns_protocol_info() {
        let req = json!({"jsonrpc": "2.0", "method": "initialize", "id": 1, "params": {"protocolVersion": crate::PROTOCOL_VERSION, "capabilities": {}, "clientInfo": {"name": "test", "version": "0.1"}}});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["jsonrpc"], "2.0");
        assert_eq!(res["id"], 1);
        assert_eq!(res["result"]["protocolVersion"], crate::PROTOCOL_VERSION);
        assert_eq!(res["result"]["serverInfo"]["name"], "velocity-mcp-rust-server");
        assert_eq!(res["result"]["serverInfo"]["version"], crate::VERSION);
        assert!(res["result"]["capabilities"]["tools"].is_object());
        assert_eq!(res["result"]["capabilities"]["tools"]["listChanged"], true);
        assert!(res["result"]["capabilities"]["logging"].is_object());
    }

    #[test]
    fn test_notifications_initialized_returns_none() {
        let req = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_request(&req).is_none());
    }

    #[test]
    fn test_ping_returns_empty_result() {
        let req = json!({"jsonrpc": "2.0", "method": "ping", "id": 10});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["jsonrpc"], "2.0");
        assert_eq!(res["id"], 10);
        assert!(res["result"].is_object());
        assert_eq!(res["result"].as_object().unwrap().len(), 0);
    }

    #[test]
    fn test_logging_set_level_valid() {
        for level in &["debug", "info", "warning", "error"] {
            let req = json!({"jsonrpc": "2.0", "method": "logging/setLevel", "id": 11, "params": {"level": level}});
            let res = handle_request(&req).unwrap();
            assert_eq!(res["result"], json!({}));
        }
    }

    #[test]
    fn test_logging_set_level_invalid() {
        let req = json!({"jsonrpc": "2.0", "method": "logging/setLevel", "id": 12, "params": {"level": "banana"}});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["error"]["code"], -32602);
        assert!(res["error"]["message"].as_str().unwrap().contains("banana"));
    }

    #[test]
    fn test_notifications_cancelled_tracks_id() {
        let req = json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 42, "reason": "timeout"}});
        assert!(handle_request(&req).is_none());
        assert!(is_cancelled(&json!(42)));
        remove_cancelled(&json!(42));
        assert!(!is_cancelled(&json!(42)));
    }

    #[test]
    fn test_tools_list_returns_registered_tools() {
        let req = json!({"jsonrpc": "2.0", "method": "tools/list", "id": 2});
        let res = handle_request(&req).unwrap();
        let tools = res["result"]["tools"].as_array().unwrap();
        assert!(tools.len() >= 4, "Should have at least 4 built-in tools");
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"convert_to_nda_document"));
        assert!(names.contains(&"convert_to_nda_tool"));
        assert!(names.contains(&"read_nda"));
        assert!(names.contains(&"execute_nda"));
    }

    #[test]
    fn test_tools_list_pagination_no_next_cursor_when_all_fit() {
        let req = json!({"jsonrpc": "2.0", "method": "tools/list", "id": 20});
        let res = handle_request(&req).unwrap();
        assert!(res["result"]["nextCursor"].is_null(), "No nextCursor when all tools fit in one page");
    }

    #[test]
    fn test_tools_list_pagination_cursor_beyond_range() {
        let req = json!({"jsonrpc": "2.0", "method": "tools/list", "id": 21, "params": {"cursor": "99999"}});
        let res = handle_request(&req).unwrap();
        let tools = res["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 0);
        assert!(res["result"]["nextCursor"].is_null());
    }

    #[test]
    fn test_tools_call_unknown_tool_returns_error_content() {
        let req = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "nonexistent", "arguments": {} },
            "id": 3
        });
        let res = handle_request(&req).unwrap();
        assert_eq!(res["result"]["isError"], true);
    }

    #[test]
    fn test_tools_call_missing_param_returns_error_content() {
        let req = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "convert_to_nda_document", "arguments": {} },
            "id": 4
        });
        let res = handle_request(&req).unwrap();
        assert_eq!(res["result"]["isError"], true);
        let text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Error running tool"));
    }

    #[test]
    fn test_tools_call_respects_cancellation() {
        let id = json!(9999);
        if let Ok(mut ids) = CANCELLED_IDS.lock() {
            ids.insert(id.clone());
        }
        let req = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "read_nda", "arguments": {"ndaPath": "x"} },
            "id": 9999
        });
        let res = handle_request(&req).unwrap();
        assert_eq!(res["result"]["isError"], true);
        assert!(res["result"]["content"][0]["text"].as_str().unwrap().contains("cancelled"));
    }

    #[test]
    fn test_health_check_returns_healthy() {
        let req = json!({"jsonrpc": "2.0", "method": "health/check", "id": 5});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["result"]["status"], "healthy");
        assert_eq!(res["result"]["mode"], "stdio");
        assert_eq!(res["result"]["version"], crate::VERSION);
    }

    #[test]
    fn test_unknown_method_with_id_returns_method_not_found() {
        let req = json!({"jsonrpc": "2.0", "method": "bogus/method", "id": 6});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["error"]["code"], -32601);
        let msg = res["error"]["message"].as_str().unwrap();
        assert!(msg.contains("bogus/method"));
    }

    #[test]
    fn test_unknown_method_without_id_returns_none() {
        let req = json!({"jsonrpc": "2.0", "method": "some/notification"});
        assert!(handle_request(&req).is_none());
    }

    #[test]
    fn test_parse_error_response_format() {
        let bad_json = "not valid json{{{";
        let result: Result<Value, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
        let err_res = json!({
            "jsonrpc": "2.0",
            "error": { "code": -32700, "message": "Parse error" },
            "id": null
        });
        assert_eq!(err_res["error"]["code"], -32700);
        assert!(err_res["id"].is_null());
    }
}
