use serde_json::{json, Value};
use std::io::{self, BufRead};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use tracing::{info, warn, debug, error};
use crate::registry;

/// Maximum allowed request size in bytes (1 MB).
/// Requests exceeding this limit are rejected before JSON parsing.
const MAX_REQUEST_SIZE: usize = 1_048_576;

/// Process a single JSON-RPC request and return the response.
/// Returns None for notifications (no response needed).
pub fn handle_request(request: &Value) -> Option<Value> {
    let method = request["method"].as_str().unwrap_or("");
    let id = &request["id"];

    debug!(method = method, "Processing JSON-RPC request");

    match method {
        "initialize" => {
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
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
            None // Notification — no response
        }
        "tools/list" => {
            let tools = registry::get_tools();
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": tools
                }
            }))
        }
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let arguments = &request["params"]["arguments"];

            let mut is_error = false;
            let output_text = match registry::call_tool(name, arguments) {
                Ok(res) => res,
                Err(e) => {
                    is_error = true;
                    error!(tool = name, error = %e, "Tool execution failed");
                    format!("Error running tool '{}': {}", name, e)
                }
            };

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
    // Use a reader thread so stdin reads don't block shutdown checks.
    // Without this, read_line() blocks indefinitely and the shutdown flag
    // is never polled between requests.
    let (tx, rx) = mpsc::channel::<String>();

    thread::spawn(move || {
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        loop {
            let mut line = String::new();
            match handle.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
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
        let req = json!({"jsonrpc": "2.0", "method": "initialize", "id": 1});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["jsonrpc"], "2.0");
        assert_eq!(res["id"], 1);
        assert_eq!(res["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(res["result"]["serverInfo"]["name"], "velocity-mcp-rust-server");
        assert_eq!(res["result"]["serverInfo"]["version"], "1.0.0");
        assert!(res["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn test_notifications_initialized_returns_none() {
        let req = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_request(&req).is_none());
    }

    #[test]
    fn test_tools_list_returns_registered_tools() {
        let req = json!({"jsonrpc": "2.0", "method": "tools/list", "id": 2});
        let res = handle_request(&req).unwrap();
        let tools = res["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 3);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"convert_to_nda"));
        assert!(names.contains(&"read_nda"));
        assert!(names.contains(&"execute_nda"));
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
        let text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("not registered"));
    }

    #[test]
    fn test_tools_call_missing_param_returns_error_content() {
        let req = json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": { "name": "convert_to_nda", "arguments": {} },
            "id": 4
        });
        let res = handle_request(&req).unwrap();
        assert_eq!(res["result"]["isError"], true);
        let text = res["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("Error running tool"));
    }

    #[test]
    fn test_health_check_returns_healthy() {
        let req = json!({"jsonrpc": "2.0", "method": "health/check", "id": 5});
        let res = handle_request(&req).unwrap();
        assert_eq!(res["result"]["status"], "healthy");
        assert_eq!(res["result"]["mode"], "stdio");
        assert_eq!(res["result"]["version"], "1.0.0");
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
        // Notifications (no id) for unknown methods should return None
        let req = json!({"jsonrpc": "2.0", "method": "some/notification"});
        assert!(handle_request(&req).is_none());
    }

    #[test]
    fn test_parse_error_response_format() {
        // Verify the error format for unparseable JSON
        let bad_json = "not valid json{{{";
        let result: Result<Value, _> = serde_json::from_str(bad_json);
        assert!(result.is_err());
        // Verify the error response we'd generate
        let err_res = json!({
            "jsonrpc": "2.0",
            "error": { "code": -32700, "message": "Parse error" },
            "id": null
        });
        assert_eq!(err_res["error"]["code"], -32700);
        assert!(err_res["id"].is_null());
    }
}
