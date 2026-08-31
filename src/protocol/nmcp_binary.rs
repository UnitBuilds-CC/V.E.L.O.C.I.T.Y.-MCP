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

/// Per-phase timing recorder for profiling the shmem loop. Enabled by
/// setting VELOCITY_PHASE_TIMING=<output-file>. Adds ~0.3us of Instant::now()
/// overhead per request, so it is off unless the env var is present.
struct PhaseRecorder {
    path: String,
    file: Option<std::fs::File>,
    window_n: u64,
    method: u8,
    wake_read_ns: u64,
    parse_ns: u64,
    dispatch_ns: u64,
    respond_ns: u64,
}

impl PhaseRecorder {
    fn enabled() -> Option<PhaseRecorder> {
        std::env::var("VELOCITY_PHASE_TIMING").ok().map(|path| PhaseRecorder {
            path,
            file: None,
            window_n: 0,
            method: 0,
            wake_read_ns: 0,
            parse_ns: 0,
            dispatch_ns: 0,
            respond_ns: 0,
        })
    }

    fn record(
        &mut self,
        method: u8,
        t_wake: Instant,
        t_read: Instant,
        t_parse: Instant,
        t_dispatch: Instant,
        t_respond: Instant,
    ) {
        self.wake_read_ns += t_read.duration_since(t_wake).as_nanos() as u64;
        self.parse_ns += t_parse.duration_since(t_read).as_nanos() as u64;
        self.dispatch_ns += t_dispatch.duration_since(t_parse).as_nanos() as u64;
        self.respond_ns += t_respond.duration_since(t_dispatch).as_nanos() as u64;
        self.method = method;
        self.window_n += 1;
        if self.window_n >= 64 {
            self.flush_window();
        }
    }

    fn flush_window(&mut self) {
        use std::io::Write;
        if self.window_n == 0 {
            return;
        }
        if self.file.is_none() {
            self.file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        if let Some(f) = self.file.as_mut() {
            let n = self.window_n as f64;
            let _ = writeln!(
                f,
                "PHASE method=0x{:02x} n={} wake_read={:.1}us parse={:.1}us dispatch={:.1}us respond={:.1}us total={:.1}us",
                self.method,
                self.window_n,
                self.wake_read_ns as f64 / 1000.0 / n,
                self.parse_ns as f64 / 1000.0 / n,
                self.dispatch_ns as f64 / 1000.0 / n,
                self.respond_ns as f64 / 1000.0 / n,
                (self.wake_read_ns + self.parse_ns + self.dispatch_ns + self.respond_ns) as f64 / 1000.0 / n,
            );
            let _ = f.flush();
        }
        self.window_n = 0;
        self.wake_read_ns = 0;
        self.parse_ns = 0;
        self.dispatch_ns = 0;
        self.respond_ns = 0;
    }
}

impl Drop for PhaseRecorder {
    fn drop(&mut self) {
        self.flush_window();
    }
}

/// Decode the request data slice into a Value for cold-path methods.
/// Empty data means "no params" (Value::Null), matching the old parser.
fn decode_req_data(data: &[u8]) -> Result<Value, Box<dyn Error>> {
    if data.is_empty() {
        return Ok(Value::Null);
    }
    let (v, _) = nda_native::decode_json_value(data)?;
    Ok(v)
}

fn build_response_value(id_tlv: &[u8], result: &Value) -> Vec<u8> {
    let mut tlv = Vec::new();
    nda_native::encode_json_value(result, &mut tlv);
    nda_native::build_nda_response_raw(nda_native::STATUS_OK, id_tlv, &tlv)
}

fn handle_nda_native(
    buffer: &mut SharedMemoryBuffer,
    raw: &[u8],
    rec: Option<&mut PhaseRecorder>,
    t_wake: Option<Instant>,
    t_read: Option<Instant>,
) -> Result<(), Box<dyn Error>> {
    let req = match nda_native::parse_nda_request_inplace(raw) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "NDA frame parse error");
            let err_frame = nda_native::build_nda_error(&Value::Null, &format!("Parse error: {}", e));
            buffer.write_output_raw(&err_frame)?;
            SharedMemoryBuffer::sync_fence();
            buffer.set_state(shmem::STATE_ERROR);
            buffer.signal_response();
            return Ok(());
        }
    };
    let t_parse = rec.is_some().then(Instant::now);

    debug!(method = nda_native::method_name(req.method), "NDA-native request");

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
        nda_native::METHOD_LOGGING_SET_LEVEL => {
            let level_val = if req.data.is_empty() {
                Value::Null
            } else {
                nda_native::decode_json_value(req.data).map(|(v, _)| v).unwrap_or(Value::Null)
            };
            let level = level_val.as_str().unwrap_or("info");
            info!(level = level, "Log level changed (NDA-native)");
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
        }
        nda_native::METHOD_TOOLS_LIST => {
            // The result TLV is cached keyed by registry generation: the tool
            // set rarely changes, but each request used to rebuild 16 json!
            // schemas, retry failing C# discovery, and re-encode ~8KB.
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
            let arguments = match args_slice {
                Some(bytes) => nda_native::decode_json_value(bytes).map(|(v, _)| v).unwrap_or(Value::Null),
                None => Value::Null,
            };

            if !rate_limit::check_rate_limit() {
                warn!(tool = name, "Rate limit exceeded (NDA-native)");
                audit::record_tool_call(name, Instant::now(), AuditOutcome::Rejected("rate limited".into()));
                nda_native::build_nda_error_raw(req.id_tlv, &format!("Rate limit exceeded for tool '{}'.", name))
            } else {
                let call_start = Instant::now();
                match registry::call_tool(name, &arguments) {
                    Ok(res) => {
                        audit::record_tool_call(name, call_start, AuditOutcome::Success);
                        let result_val: Value = serde_json::from_str(&res).unwrap_or_else(|_| json!(res));
                        let mut result_tlv = Vec::new();
                        nda_native::encode_json_value(&result_val, &mut result_tlv);
                        nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, &result_tlv)
                    }
                    Err(e) => {
                        error!(tool = name, error = %e, "Tool execution failed (NDA-native)");
                        audit::record_tool_call(name, call_start, AuditOutcome::Error(e.to_string()));
                        nda_native::build_nda_error_raw(req.id_tlv, &format!("Error running tool '{}': {}", name, e))
                    }
                }
            }
        }
        nda_native::METHOD_HEALTH_CHECK => {
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::health_result_tlv())
        }
        nda_native::METHOD_RESOURCES_LIST => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let cursor = data.get("cursor").and_then(|c| c.as_str());
                    let result = crate::resources::handle_resources_list(cursor);
                    build_response_value(req.id_tlv, &result)
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_RESOURCES_READ => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let uri = data["uri"].as_str().unwrap_or("");
                    match crate::resources::handle_resources_read(uri) {
                        Ok(result) => build_response_value(req.id_tlv, &result),
                        Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &e),
                    }
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_RESOURCE_TEMPLATES_LIST => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let cursor = data.get("cursor").and_then(|c| c.as_str());
                    let result = crate::resources::handle_resource_templates_list(cursor);
                    build_response_value(req.id_tlv, &result)
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_PROMPTS_LIST => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let cursor = data.get("cursor").and_then(|c| c.as_str());
                    let result = crate::resources::handle_prompts_list(cursor);
                    build_response_value(req.id_tlv, &result)
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_PROMPTS_GET => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let name = data["name"].as_str().unwrap_or("");
                    let arguments = &data["arguments"];
                    match crate::resources::handle_prompts_get(name, arguments) {
                        Ok(result) => build_response_value(req.id_tlv, &result),
                        Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &e),
                    }
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_SAMPLING_CREATE => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    match crate::sampling::handle_sampling_create_message(&data) {
                        Ok(result) => build_response_value(req.id_tlv, &result),
                        Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &e),
                    }
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::NOTIF_CANCELLED => {
            let id_val = nda_native::decode_json_value(req.id_tlv).map(|(v, _)| v).unwrap_or(Value::Null);
            debug!(request_id = %id_val, "Cancellation notification (NDA-native)");
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
        }
        nda_native::NOTIF_PROGRESS => {
            if let Ok(data) = decode_req_data(req.data) {
                crate::streaming::handle_progress_notification(&data);
            }
            nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, nda_native::EMPTY_OBJECT_TLV)
        }
        _ => {
            warn!(method = req.method, "Unknown NDA method");
            nda_native::build_nda_error_raw(req.id_tlv, &format!("Unknown method: 0x{:02x}", req.method))
        }
    };
    let t_dispatch = rec.is_some().then(Instant::now);

    buffer.write_output_raw(&response_frame)?;
    SharedMemoryBuffer::sync_fence();
    buffer.set_state(shmem::STATE_RES_READY);
    // No flush: the client maps the same section, so the write is already
    // visible. FlushViewOfFile only forces disk writeback (~30us cost).
    buffer.signal_response();
    if let (Some(r), Some(tw), Some(tr), Some(tp), Some(td)) = (rec, t_wake, t_read, t_parse, t_dispatch) {
        r.record(req.method, tw, tr, tp, td, Instant::now());
    }
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
        "resources/list" => {
            let cursor = request["params"]["cursor"].as_str();
            crate::resources::handle_resources_list(cursor)
        }
        "resources/read" => {
            let uri = request["params"]["uri"].as_str().unwrap_or("");
            match crate::resources::handle_resources_read(uri) {
                Ok(result) => {
                    json!({"jsonrpc": "2.0", "id": id, "result": result})
                }
                Err(e) => {
                    json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32603, "message": e }
                    })
                }
            }
        }
        "resources/templates/list" => {
            let cursor = request["params"]["cursor"].as_str();
            crate::resources::handle_resource_templates_list(cursor)
        }
        "resources/subscribe" => {
            let uri = request["params"]["uri"].as_str().unwrap_or("");
            let sub_id = request["params"]["subscriberId"].as_str().unwrap_or("default");
            match crate::resources::handle_resources_subscribe(uri, sub_id) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": e }}),
            }
        }
        "resources/unsubscribe" => {
            let uri = request["params"]["uri"].as_str().unwrap_or("");
            let sub_id = request["params"]["subscriberId"].as_str().unwrap_or("default");
            match crate::resources::handle_resources_unsubscribe(uri, sub_id) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": e }}),
            }
        }
        "prompts/list" => {
            let cursor = request["params"]["cursor"].as_str();
            crate::resources::handle_prompts_list(cursor)
        }
        "prompts/get" => {
            let name = request["params"]["name"].as_str().unwrap_or("");
            let arguments = &request["params"]["arguments"];
            match crate::resources::handle_prompts_get(name, arguments) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": e }}),
            }
        }
        "sampling/createMessage" => {
            let params = &request["params"];
            match crate::sampling::handle_sampling_create_message(params) {
                Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
                Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": { "code": -32603, "message": e }}),
            }
        }
        "notifications/cancelled" => {
            let request_id = &request["params"]["requestId"];
            let reason = request["params"]["reason"].as_str().unwrap_or("unknown");
            debug!(request_id = %request_id, reason = reason, "Cancellation notification received (shmem)");
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
        }
        "notifications/progress" => {
            let params = &request["params"];
            crate::streaming::handle_progress_notification(params);
            json!({"jsonrpc": "2.0", "id": id, "result": {}})
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
    buffer.signal_response();
    Ok(())
}

pub fn run_shmem_loop(buffer_path: &str, shutdown: &AtomicBool) -> Result<(), Box<dyn Error>> {
    info!(path = buffer_path, "Initializing Shared Memory Buffer");

    // Enable high-resolution timer for low-latency event waits on Windows
    shmem::enable_high_resolution_timer();

    let mut buffer = SharedMemoryBuffer::create_or_open(buffer_path)?;
    info!("Shared Memory Server initialized. Waiting for host requests...");

    let result = run_shmem_loop_inner(&mut buffer, shutdown);

    // Restore default timer resolution
    shmem::disable_high_resolution_timer();

    // Cleanup buffer file
    drop(buffer);
    if let Err(e) = std::fs::remove_file(buffer_path) {
        warn!(path = buffer_path, error = %e, "Failed to remove shared memory buffer file");
    } else {
        info!(path = buffer_path, "Shared memory buffer file cleaned up");
    }

    result
}

fn run_shmem_loop_inner(buffer: &mut SharedMemoryBuffer, shutdown: &AtomicBool) -> Result<(), Box<dyn Error>> {
    let mut rec = PhaseRecorder::enabled();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            info!("Shutdown signal received, exiting shmem loop");
            break;
        }

        // Auto-reset events consume the signal atomically on a successful
        // wait — do NOT ResetEvent here: a fast client may have already
        // fired SetEvent for the next request, and resetting would erase it.
        buffer.wait_for_request();
        let t_wake = rec.is_some().then(Instant::now);

        let state = buffer.get_state();
        if state == shmem::STATE_REQ_READY {
            // No flush needed: nothing reads PROCESSING; events carry the sync.
            buffer.set_state(shmem::STATE_PROCESSING);

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
                    buffer.signal_response();
                    continue;
                }
            };
            let t_read = rec.is_some().then(Instant::now);

            if nda_native::is_nda_frame(&raw) {
                debug!("Detected NDA-native frame");
                if let Err(e) = handle_nda_native(buffer, &raw, rec.as_mut(), t_wake, t_read) {
                    error!(error = %e, "NDA-native handler error");
                    SharedMemoryBuffer::sync_fence();
                    buffer.set_state(shmem::STATE_ERROR);
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
                        buffer.signal_response();
                        continue;
                    }
                };
                if let Err(e) = handle_json_shmem(buffer, &input_str) {
                    error!(error = %e, "JSON shmem handler error");
                    SharedMemoryBuffer::sync_fence();
                    buffer.set_state(shmem::STATE_ERROR);
                    buffer.signal_response();
                }
            }
        }
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
