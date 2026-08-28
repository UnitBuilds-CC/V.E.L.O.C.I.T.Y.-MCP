use crate::ipc::shmem::{self, SharedMemoryBuffer};
use crate::protocol::nda_native;
use crate::registry;
use crate::audit::{self, AuditOutcome};
use crate::rate_limit;
use serde_json::{json, Value};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tracing::{info, warn, error, debug};

fn handle_nda_native(buffer: &mut SharedMemoryBuffer, raw: &[u8]) -> Result<(), Box<dyn Error>> {
    let req = match nda_native::parse_nda_request(raw) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "NDA frame parse error");
            let err_frame = nda_native::build_nda_error(&Value::Null, &format!("Parse error: {}", e));
            buffer.write_output_raw(&err_frame)?;
            SharedMemoryBuffer::sync_fence();
            buffer.set_state(shmem::STATE_ERROR);
            buffer.flush()?;
            buffer.signal_response();
            return Ok(());
        }
    };

    debug!(method = nda_native::method_name(req.method), "NDA-native request");

    let response_frame = match req.method {
        nda_native::METHOD_PING => {
            nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &json!({}))
        }
        nda_native::METHOD_INITIALIZE => {
            let result = json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": { "listChanged": true },
                    "logging": {}
                },
                "serverInfo": {
                    "name": "velocity-mcp-rust-server",
                    "version": crate::VERSION
                }
            });
            nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &result)
        }
        nda_native::NOTIF_INITIALIZED => {
            nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &json!({}))
        }
        nda_native::METHOD_LOGGING_SET_LEVEL => {
            let level = req.data.as_str().unwrap_or("info");
            info!(level = level, "Log level changed (NDA-native)");
            nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &json!({}))
        }
        nda_native::METHOD_TOOLS_LIST => {
            let tools = registry::get_tools();
            let tools_json: Vec<Value> = tools.iter().map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.input_schema
                })
            }).collect();
            nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &json!({"tools": tools_json}))
        }
        nda_native::METHOD_TOOLS_CALL => {
            let name = req.data["name"].as_str().unwrap_or("");
            let arguments = &req.data["arguments"];

            if !rate_limit::check_rate_limit() {
                warn!(tool = name, "Rate limit exceeded (NDA-native)");
                audit::record_tool_call(name, Instant::now(), AuditOutcome::Rejected("rate limited".into()));
                nda_native::build_nda_error(&req.request_id, &format!("Rate limit exceeded for tool '{}'.", name))
            } else {
                let call_start = Instant::now();
                match registry::call_tool(name, arguments) {
                    Ok(res) => {
                        audit::record_tool_call(name, call_start, AuditOutcome::Success);
                        let result_val: Value = serde_json::from_str(&res).unwrap_or_else(|_| json!(res));
                        nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &result_val)
                    }
                    Err(e) => {
                        error!(tool = name, error = %e, "Tool execution failed (NDA-native)");
                        audit::record_tool_call(name, call_start, AuditOutcome::Error(e.to_string()));
                        nda_native::build_nda_error(&req.request_id, &format!("Error running tool '{}': {}", name, e))
                    }
                }
            }
        }
        nda_native::METHOD_HEALTH_CHECK => {
            nda_native::build_nda_response(nda_native::STATUS_OK, &req.request_id, &json!({
                "status": "healthy",
                "mode": "shmem-nda",
                "version": crate::VERSION
            }))
        }
        _ => {
            warn!(method = req.method, "Unknown NDA method");
            nda_native::build_nda_error(&req.request_id, &format!("Unknown method: 0x{:02x}", req.method))
        }
    };

    buffer.write_output_raw(&response_frame)?;
    SharedMemoryBuffer::sync_fence();
    buffer.set_state(shmem::STATE_RES_READY);
    buffer.flush()?;
    buffer.signal_response();
    Ok(())
}

fn handle_json_shmem(buffer: &mut SharedMemoryBuffer, input_str: &str) -> Result<(), Box<dyn Error>> {
    let request: Value = match serde_json::from_str(input_str) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "JSON parse error in shmem request");
            let err_res = json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": format!("Parse error: {}", e) },
                "id": null
            });
            let res_str = serde_json::to_string(&err_res)?;
            let _ = buffer.write_output(&res_str);
            SharedMemoryBuffer::sync_fence();
            buffer.set_state(shmem::STATE_ERROR);
            let _ = buffer.flush();
            buffer.signal_response();
            return Ok(());
        }
    };

    let method = request["method"].as_str().unwrap_or("");
    let id = &request["id"];

    debug!(method = method, "Processing shmem request (JSON)");

    let response = match method {
        "initialize" => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "logging": {}
                    },
                    "serverInfo": {
                        "name": "velocity-mcp-rust-server",
                        "version": crate::VERSION
                    }
                }
            })
        }
        "notifications/initialized" => {
            debug!("Client confirmed initialization via shmem");
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
        }
        "ping" => {
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
        }
        "logging/setLevel" => {
            let level_str = request["params"]["level"].as_str().unwrap_or("info");
            info!(level = level_str, "Log level changed (shmem)");
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
        }
        "tools/list" => {
            let tools = registry::get_tools();
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": tools }
            })
        }
        "tools/call" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let arguments = &request["params"]["arguments"];

            if !rate_limit::check_rate_limit() {
                warn!(tool = name, "Rate limit exceeded (shmem)");
                audit::record_tool_call(name, Instant::now(), audit::AuditOutcome::Rejected("rate limited".into()));
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{"type": "text", "text": format!("Rate limit exceeded for tool '{}'.", name)}],
                        "isError": true
                    }
                })
            } else {
                let call_start = Instant::now();
                let mut is_error = false;
                let output_text = match registry::call_tool(name, arguments) {
                    Ok(res) => {
                        audit::record_tool_call(name, call_start, AuditOutcome::Success);
                        res
                    }
                    Err(e) => {
                        is_error = true;
                        error!(tool = name, error = %e, "Tool execution failed in shmem");
                        audit::record_tool_call(name, call_start, AuditOutcome::Error(e.to_string()));
                        format!("Error running tool '{}': {}", name, e)
                    }
                };

                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{"type": "text", "text": output_text}],
                        "isError": is_error
                    }
                })
            }
        }
        "health/check" => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "status": "healthy",
                    "mode": "shmem",
                    "version": crate::VERSION
                }
            })
        }
        _ => {
            warn!(method = method, "Method not supported in shmem mode");
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("Method '{}' not supported", method) }
            })
        }
    };

    let res_str = serde_json::to_string(&response)?;
    buffer.write_output(&res_str)?;
    SharedMemoryBuffer::sync_fence();
    buffer.set_state(shmem::STATE_RES_READY);
    buffer.flush()?;
    buffer.signal_response();
    Ok(())
}

pub fn run_shmem_loop(buffer_path: &str, shutdown: &AtomicBool) -> Result<(), Box<dyn Error>> {
    info!(path = buffer_path, "Initializing Shared Memory Buffer");
    let mut buffer = SharedMemoryBuffer::create_or_open(buffer_path)?;
    info!("Shared Memory Server initialized. Waiting for host requests...");

    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutdown signal received, exiting shmem loop");
            break;
        }

        buffer.wait_for_request();

        let state = buffer.get_state();
        if state == shmem::STATE_REQ_READY {
            buffer.set_state(shmem::STATE_PROCESSING);
            buffer.flush()?;

            let raw = match buffer.read_input_raw() {
                Ok(r) => r,
                Err(e) => {
                    error!(error = %e, "Failed to read input from shared memory");
                    let err_res = json!({
                        "jsonrpc": "2.0",
                        "error": { "code": -32603, "message": format!("Internal memory error: {}", e) },
                        "id": null
                    });
                    let res_str = serde_json::to_string(&err_res)?;
                    let _ = buffer.write_output(&res_str);
                    SharedMemoryBuffer::sync_fence();
                    buffer.set_state(shmem::STATE_ERROR);
                    let _ = buffer.flush();
                    buffer.signal_response();
                    continue;
                }
            };

            if nda_native::is_nda_frame(&raw) {
                debug!("Detected NDA-native frame");
                if let Err(e) = handle_nda_native(&mut buffer, &raw) {
                    error!(error = %e, "NDA-native handler error");
                    SharedMemoryBuffer::sync_fence();
                    buffer.set_state(shmem::STATE_ERROR);
                    let _ = buffer.flush();
                    buffer.signal_response();
                }
            } else {
                let input_str = match String::from_utf8(raw) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!(error = %e, "Invalid UTF-8 in shmem request");
                        let err_res = json!({
                            "jsonrpc": "2.0",
                            "error": { "code": -32700, "message": "Invalid UTF-8 in request" },
                            "id": null
                        });
                        let res_str = serde_json::to_string(&err_res)?;
                        let _ = buffer.write_output(&res_str);
                        SharedMemoryBuffer::sync_fence();
                        buffer.set_state(shmem::STATE_ERROR);
                        let _ = buffer.flush();
                        buffer.signal_response();
                        continue;
                    }
                };
                if let Err(e) = handle_json_shmem(&mut buffer, &input_str) {
                    error!(error = %e, "JSON shmem handler error");
                    SharedMemoryBuffer::sync_fence();
                    buffer.set_state(shmem::STATE_ERROR);
                    let _ = buffer.flush();
                    buffer.signal_response();
                }
            }
        }
    }

    drop(buffer);
    if let Err(e) = std::fs::remove_file(buffer_path) {
        warn!(path = buffer_path, error = %e, "Failed to remove shared memory buffer file");
    } else {
        info!(path = buffer_path, "Shared memory buffer file cleaned up");
    }

    Ok(())
}

// Zero-allocation binary parser specifications for custom high-speed binary drivers
#[derive(Debug)]
pub struct NmcpBinaryFrame<'a> {
    pub magic: &'a [u8; 4],
    pub merkle_root: &'a [u8; 32],
    pub payload: &'a [u8],
}

impl<'a> NmcpBinaryFrame<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, &'static str> {
        if bytes.len() < 36 {
            return Err("Buffer too small for NMCP binary frame header");
        }

        // Safety: We verified bytes.len() >= 36 above.
        // The pointer cast `*const u8` -> `*const [u8; 4]` is safe because:
        // - The slice guarantees contiguous, in-bounds memory for at least 36 bytes.
        // - `[u8; 4]` has alignment 1, so any `*const u8` is valid.
        // - The reference only lives for `'a`, bounded by the input slice lifetime.
        let magic = unsafe { &*(bytes[0..4].as_ptr() as *const [u8; 4]) };
        if magic != b"NMCP" {
            return Err("Invalid NMCP magic signature");
        }

        // Safety: Same rationale as magic. `[u8; 32]` has alignment 1, and
        // bytes[4..36] is a valid 32-byte sub-slice of the input.
        let merkle_root = unsafe { &*(bytes[4..36].as_ptr() as *const [u8; 32]) };
        let payload = &bytes[36..];

        Ok(NmcpBinaryFrame {
            magic,
            merkle_root,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nmcp_binary_frame_parse_valid() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"NMCP");
        buffer.extend_from_slice(&[0u8; 32]); // Dummy Merkle root
        buffer.extend_from_slice(b"test payload");

        let frame = NmcpBinaryFrame::parse(&buffer).unwrap();
        assert_eq!(frame.magic, b"NMCP");
        assert_eq!(frame.payload, b"test payload");
    }

    #[test]
    fn test_nmcp_binary_frame_parse_too_small() {
        let buffer = vec![0u8; 10]; // Less than 36 bytes
        let result = NmcpBinaryFrame::parse(&buffer);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too small"));
    }

    #[test]
    fn test_nmcp_binary_frame_parse_invalid_magic() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"XXXX"); // Wrong magic
        buffer.extend_from_slice(&[0u8; 32]);
        buffer.extend_from_slice(b"payload");

        let result = NmcpBinaryFrame::parse(&buffer);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid NMCP magic"));
    }

    #[test]
    fn test_nmcp_binary_frame_parse_empty_payload() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"NMCP");
        buffer.extend_from_slice(&[0xABu8; 32]); // Non-zero Merkle root

        let frame = NmcpBinaryFrame::parse(&buffer).unwrap();
        assert_eq!(frame.magic, b"NMCP");
        assert_eq!(frame.merkle_root, &[0xABu8; 32]);
        assert!(frame.payload.is_empty());
    }

    #[test]
    fn test_nmcp_binary_frame_merkle_root_preserved() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(b"NMCP");
        let merkle = [42u8; 32];
        buffer.extend_from_slice(&merkle);
        buffer.extend_from_slice(b"data");

        let frame = NmcpBinaryFrame::parse(&buffer).unwrap();
        assert_eq!(frame.merkle_root, &merkle);
    }
}
