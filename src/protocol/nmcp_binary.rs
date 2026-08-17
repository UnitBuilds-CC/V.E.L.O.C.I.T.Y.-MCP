use crate::ipc::shmem::{self, SharedMemoryBuffer};
use crate::registry;
use serde_json::{json, Value};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tracing::{info, warn, error, debug};

/// Run the shared memory protocol loop.
///
/// Polls the shared memory buffer for incoming requests (state = `REQ_READY`),
/// processes them through the JSON-RPC handler, and writes responses back.
/// Uses `SeqCst` fences between write_output and set_state to ensure
/// correct cross-process synchronization of length fields.
///
/// On graceful shutdown, drops the mmap and removes the buffer file.
pub fn run_shmem_loop(buffer_path: &str, shutdown: &AtomicBool) -> Result<(), Box<dyn Error>> {
    info!(path = buffer_path, "Initializing Shared Memory Buffer");
    let mut buffer = SharedMemoryBuffer::create_or_open(buffer_path)?;
    info!("Shared Memory Server initialized. Waiting for host requests...");

    loop {
        // Check for shutdown signal
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutdown signal received, exiting shmem loop");
            break;
        }

        let state = buffer.get_state();
        if state == shmem::STATE_REQ_READY {
            // Set state to processing instantly to lock the buffer
            buffer.set_state(shmem::STATE_PROCESSING);
            buffer.flush()?;

            // Read the binary JSON-RPC input from the shared memory request region
            match buffer.read_input() {
                Ok(input_str) => {
                    debug!("Received request from shared memory");
                    let request: Value = match serde_json::from_str(&input_str) {
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
                            continue;
                        }
                    };

                    let method = request["method"].as_str().unwrap_or("");
                    let id = &request["id"];

                    debug!(method = method, "Processing shmem request");

                    let response = match method {
                        "initialize" => {
                            json!({
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
                            })
                        }
                        "notifications/initialized" => {
                            // No response for notifications, but write an ack for shmem protocol
                            debug!("Client confirmed initialization via shmem");
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {}
                            })
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

                            let mut is_error = false;
                            let output_text = match registry::call_tool(name, arguments) {
                                Ok(res) => res,
                                Err(e) => {
                                    is_error = true;
                                    error!(tool = name, error = %e, "Tool execution failed in shmem");
                                    format!("Error running tool '{}': {}", name, e)
                                }
                            };

                            json!({
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
                            })
                        }
                        "health/check" => {
                            json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": {
                                    "status": "healthy",
                                    "mode": "shmem",
                                    "version": crate::VERSION,
                                    "buffer_path": buffer_path
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
                    // SeqCst fence: ensures length field writes are globally
                    // visible before the state byte transitions to RES_READY.
                    SharedMemoryBuffer::sync_fence();
                    buffer.set_state(shmem::STATE_RES_READY);
                    buffer.flush()?;
                    debug!("Response written to shared memory");
                }
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
                }
            }
        } else {
            // Adaptive backoff: Sleep for 100 microseconds to prevent CPU pegging while maintaining low latency
            thread::sleep(Duration::from_micros(100));
        }
    }

    // Cleanup: drop the mmap to flush all pending writes, then remove the buffer file
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
