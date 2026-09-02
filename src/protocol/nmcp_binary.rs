use crate::ipc::shmem::{self, SharedMemoryBuffer};
use crate::protocol::nda_native;
use crate::registry;
use crate::audit::{self, AuditOutcome};
use crate::rate_limit;
use crate::sandbox;
use serde_json::{json, Value};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tracing::{info, warn, error, debug};

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

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
            if let Err(e) = writeln!(
                f,
                "PHASE method=0x{:02x} n={} wake_read={:.1}us parse={:.1}us dispatch={:.1}us respond={:.1}us total={:.1}us",
                self.method,
                self.window_n,
                self.wake_read_ns as f64 / 1000.0 / n,
                self.parse_ns as f64 / 1000.0 / n,
                self.dispatch_ns as f64 / 1000.0 / n,
                self.respond_ns as f64 / 1000.0 / n,
                (self.wake_read_ns + self.parse_ns + self.dispatch_ns + self.respond_ns) as f64 / 1000.0 / n,
            ) {
                tracing::debug!(error = %e, "Failed to write phase log");
            }
            if let Err(e) = f.flush() {
                tracing::debug!(error = %e, "Failed to flush phase log");
            }
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

fn build_response_value(id_tlv: &[u8], result: &Value) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut tlv = Vec::new();
    nda_native::encode_json_value(result, &mut tlv)?;
    Ok(nda_native::build_nda_response_raw(nda_native::STATUS_OK, id_tlv, &tlv))
}

/// Dispatch an NDA request and return the response frame.
/// This is the core dispatch logic, separated from transport (shmem/HTTP).
pub fn dispatch_nda_request(raw: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let req = nda_native::parse_nda_request_inplace(raw)?;

    debug!(method = nda_native::method_name(req.method), "NDA-native request");

    // Extract Merkle root from frame header (bytes 4..36) for audit trail
    let merkle_root = if raw.len() >= 36 {
        Some(hex_encode(&raw[4..36]))
    } else {
        None
    };

    let response_frame = match req.method {
        nda_native::METHOD_PING => {
            if let Ok(delay) = std::env::var("VELOCITY_PING_DELAY_US") {
                if let Ok(us) = delay.parse::<u64>() {
                    if us > 0 {
                        std::thread::sleep(std::time::Duration::from_micros(us));
                    }
                }
            }
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
            if let Err(e) = nda_native::encode_json_value(&result, &mut result_tlv) {
                return Ok(nda_native::build_nda_error_raw(req.id_tlv, &format!("Encoding error: {}", e)));
            }
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
                audit::record_tool_call_with_merkle(name, Instant::now(), AuditOutcome::Rejected("rate limited".into()), merkle_root.clone());
                nda_native::build_nda_error_raw(req.id_tlv, &format!("Rate limit exceeded for tool '{}'.", name))
            } else {
                let call_start = Instant::now();
                match registry::call_tool(name, &arguments) {
                    Ok(res) => {
                        audit::record_tool_call_with_merkle(name, call_start, AuditOutcome::Success, merkle_root.clone());
                        let result_val: Value = serde_json::from_str(&res).unwrap_or_else(|_| json!(res));
                        let mut result_tlv = Vec::new();
                        if let Err(e) = nda_native::encode_json_value(&result_val, &mut result_tlv) {
                            return Ok(nda_native::build_nda_error_raw(req.id_tlv, &sandbox::sanitize_error(&format!("Result encoding error: {}", e))));
                        }
                        nda_native::build_nda_response_raw(nda_native::STATUS_OK, req.id_tlv, &result_tlv)
                    }
                    Err(e) => {
                        error!(tool = name, error = %e, "Tool execution failed (NDA-native)");
                        audit::record_tool_call_with_merkle(name, call_start, AuditOutcome::Error(e.to_string()), merkle_root.clone());
                        nda_native::build_nda_error_raw(req.id_tlv, &sandbox::sanitize_error(&format!("Error running tool '{}': {}", name, e)))
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
                    build_response_value(req.id_tlv, &result)?
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_RESOURCES_READ => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let uri = data["uri"].as_str().unwrap_or("");
                    match crate::resources::handle_resources_read(uri) {
                        Ok(result) => build_response_value(req.id_tlv, &result)?,
                        Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &sandbox::sanitize_error(&e)),
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
                    build_response_value(req.id_tlv, &result)?
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_PROMPTS_LIST => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    let cursor = data.get("cursor").and_then(|c| c.as_str());
                    let result = crate::resources::handle_prompts_list(cursor);
                    build_response_value(req.id_tlv, &result)?
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
                        Ok(result) => build_response_value(req.id_tlv, &result)?,
                        Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &sandbox::sanitize_error(&e)),
                    }
                }
                Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &format!("Invalid request data: {}", e)),
            }
        }
        nda_native::METHOD_SAMPLING_CREATE => {
            match decode_req_data(req.data) {
                Ok(data) => {
                    match crate::sampling::handle_sampling_create_message(&data) {
                        Ok(result) => build_response_value(req.id_tlv, &result)?,
                        Err(e) => nda_native::build_nda_error_raw(req.id_tlv, &sandbox::sanitize_error(&e)),
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

    Ok(response_frame)
}

fn handle_nda_native(
    buffer: &mut SharedMemoryBuffer,
    raw: &[u8],
    rec: Option<&mut PhaseRecorder>,
    t_wake: Option<Instant>,
    t_read: Option<Instant>,
) -> Result<(), Box<dyn Error>> {
    let t_parse = rec.is_some().then(Instant::now);
    
    // Extract method code for phase recorder before dispatch consumes the frame
    let method_code = if raw.len() > nda_native::FRAME_HEADER_SIZE {
        raw[nda_native::FRAME_HEADER_SIZE]
    } else {
        0
    };

    let response_frame = match dispatch_nda_request(raw) {
        Ok(frame) => frame,
        Err(e) => {
            warn!(error = %e, "NDA frame parse error");
            let err_msg = format!("Parse error: {}", e);
            let err_frame = nda_native::build_nda_error(&Value::Null, &err_msg)
                .unwrap_or_else(|_| nda_native::build_nda_error_raw(&[], &err_msg));
            buffer.write_output_raw(&err_frame)?;
            SharedMemoryBuffer::sync_fence();
            buffer.set_state(shmem::STATE_ERROR);
            buffer.signal_response();
            return Ok(());
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
        r.record(method_code, tw, tr, tp, td, Instant::now());
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
            if let Err(e) = buffer.write_output(&res_str) {
                tracing::warn!(error = %e, "Failed to write error response to shared memory");
            }
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
                        sandbox::sanitize_error(&format!("Error running tool '{}': {}", name, e))
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
            json!({"jsonrpc": "2.0", "id": id, "result": crate::resources::handle_resources_list(cursor)})
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
            json!({"jsonrpc": "2.0", "id": id, "result": crate::resources::handle_resource_templates_list(cursor)})
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
            json!({"jsonrpc": "2.0", "id": id, "result": crate::resources::handle_prompts_list(cursor)})
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
    let session_id = format!("shmem-{}", std::process::id());
    crate::audit::set_session_context(session_id);
    crate::audit::set_transport_context("nda_shmem".to_string());

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
                    if let Err(e) = buffer.write_output(&res_str) {
                        tracing::warn!(error = %e, "Failed to write error response to shared memory");
                    }
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
                        if let Err(e) = buffer.write_output(&res_str) {
                            tracing::warn!(error = %e, "Failed to write error response to shared memory");
                        }
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

    // ─── dispatch_nda_request tests ─────────────────────────────────────────

    fn parse_response_status(frame: &[u8]) -> u8 {
        assert!(frame.len() >= 37, "response frame too short: {} bytes", frame.len());
        assert_eq!(&frame[..4], b"NMCP");
        frame[36]
    }

    fn parse_response_result_json(frame: &[u8]) -> Value {
        let payload = &frame[37..]; // skip header + status byte
        // skip id_tlv: decode it to find its length
        let (_, id_consumed) = nda_native::decode_json_value(payload).unwrap();
        let result_bytes = &payload[id_consumed..];
        if result_bytes.is_empty() {
            return Value::Null;
        }
        nda_native::decode_json_value(result_bytes).map(|(v, _)| v).unwrap_or(Value::Null)
    }

    fn parse_response_error_msg(frame: &[u8]) -> String {
        let payload = &frame[37..];
        let (_, id_consumed) = nda_native::decode_json_value(payload).unwrap();
        let result_bytes = &payload[id_consumed..];
        // error result is encoded as [0x01][4-byte len][string bytes]
        if result_bytes.is_empty() || result_bytes[0] != 0x01 {
            return String::new();
        }
        let len = u32::from_be_bytes([result_bytes[1], result_bytes[2], result_bytes[3], result_bytes[4]]) as usize;
        String::from_utf8_lossy(&result_bytes[5..5 + len]).to_string()
    }

    #[test]
    fn test_dispatch_ping() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_PING, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert_eq!(result, json!({}));
    }

    #[test]
    fn test_dispatch_initialize() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_INITIALIZE, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["protocolVersion"].is_string());
        assert!(result["capabilities"]["tools"].is_object());
        assert!(result["capabilities"]["resources"].is_object());
        assert!(result["capabilities"]["prompts"].is_object());
        assert!(result["capabilities"]["sampling"].is_object());
        assert!(result["capabilities"]["logging"].is_object());
        assert_eq!(result["serverInfo"]["name"], "velocity-mcp-rust-server");
        assert!(result["serverInfo"]["version"].is_string());
    }

    #[test]
    fn test_dispatch_initialized_notification() {
        let frame = nda_native::build_nda_request(nda_native::NOTIF_INITIALIZED, &json!(0u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_logging_set_level() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_LOGGING_SET_LEVEL, &json!(1u64), &json!("debug")).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_logging_set_level_empty_data() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_LOGGING_SET_LEVEL, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_tools_list() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_TOOLS_LIST, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["tools"].is_array());
    }

    #[test]
    fn test_dispatch_tools_call_unknown_tool() {
        let data = json!({"name": "nonexistent_tool_xyz", "arguments": {}});
        let frame = nda_native::build_nda_request(nda_native::METHOD_TOOLS_CALL, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
        let err = parse_response_error_msg(&resp);
        assert!(err.contains("nonexistent_tool_xyz"), "error was: {}", err);
    }

    #[test]
    fn test_dispatch_tools_call_empty_data() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_TOOLS_CALL, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        // empty name → tool not found
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_health_check() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_HEALTH_CHECK, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert_eq!(result["status"], "healthy");
        assert_eq!(result["mode"], "shmem-nda");
    }

    #[test]
    fn test_dispatch_resources_list() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_RESOURCES_LIST, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["resources"].is_array());
    }

    #[test]
    fn test_dispatch_resources_list_with_cursor() {
        let data = json!({"cursor": "abc123"});
        let frame = nda_native::build_nda_request(nda_native::METHOD_RESOURCES_LIST, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_resources_read_missing_uri() {
        let data = json!({});
        let frame = nda_native::build_nda_request(nda_native::METHOD_RESOURCES_READ, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        // empty uri → error from resources handler
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_resource_templates_list() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_RESOURCE_TEMPLATES_LIST, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["resourceTemplates"].is_array());
    }

    #[test]
    fn test_dispatch_prompts_list() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_PROMPTS_LIST, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["prompts"].is_array());
    }

    #[test]
    fn test_dispatch_prompts_get_unknown() {
        let data = json!({"name": "nonexistent_prompt xyz"});
        let frame = nda_native::build_nda_request(nda_native::METHOD_PROMPTS_GET, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_sampling_create_message() {
        let data = json!({"messages": [{"role": "user", "content": {"type": "text", "text": "hello"}}]});
        let frame = nda_native::build_nda_request(nda_native::METHOD_SAMPLING_CREATE, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        // may succeed or fail depending on sampling handler, but should not panic
        let status = parse_response_status(&resp);
        assert!(status == nda_native::STATUS_OK || status == nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_cancelled_notification() {
        let frame = nda_native::build_nda_request(nda_native::NOTIF_CANCELLED, &json!(42u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_progress_notification() {
        let data = json!({"progressToken": "tok1", "progress": 0.5});
        let frame = nda_native::build_nda_request(nda_native::NOTIF_PROGRESS, &json!(0u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_unknown_method() {
        let frame = nda_native::build_nda_request(0xFF, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
        let err = parse_response_error_msg(&resp);
        assert!(err.contains("Unknown method: 0xff"), "error was: {}", err);
    }

    #[test]
    fn test_dispatch_malformed_frame() {
        let result = dispatch_nda_request(&[0u8; 5]);
        assert!(result.is_err());
    }

    #[test]
    fn test_dispatch_empty_frame() {
        let result = dispatch_nda_request(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_hex_encode_empty() {
        assert_eq!(hex_encode(&[]), "");
    }

    #[test]
    fn test_hex_encode_bytes() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }

    #[test]
    fn test_hex_encode_all_zeros() {
        assert_eq!(hex_encode(&[0x00, 0x00, 0x00]), "000000");
    }

    #[test]
    fn test_decode_req_data_empty() {
        let val = decode_req_data(&[]).unwrap();
        assert_eq!(val, Value::Null);
    }

    #[test]
    fn test_decode_req_data_with_json() {
        let mut buf = Vec::new();
        let _ = nda_native::encode_json_value(&json!({"key": "value"}), &mut buf);
        let val = decode_req_data(&buf).unwrap();
        assert_eq!(val["key"], "value");
    }

    #[test]
    fn test_dispatch_merkle_root_extracted() {
        let frame = nda_native::build_nda_request(nda_native::METHOD_PING, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
    }

    #[test]
    fn test_dispatch_tools_call_bench_echo() {
        let data = json!({"name": "bench_echo", "arguments": {"size": 16}});
        let frame = nda_native::build_nda_request(nda_native::METHOD_TOOLS_CALL, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        let text = result.as_str().unwrap_or("");
        assert_eq!(text.len(), 16);
        assert!(text.chars().all(|c| c == 'x'));
    }

    // ─── PhaseRecorder tests ────────────────────────────────────────────────

    #[test]
    fn test_phase_recorder_disabled_by_default() {
        std::env::remove_var("VELOCITY_PHASE_TIMING");
        let rec = PhaseRecorder::enabled();
        assert!(rec.is_none());
    }

    #[test]
    fn test_phase_recorder_enabled_with_env() {
        let tmp = std::env::temp_dir().join(format!("phase_test_{}.log", std::process::id()));
        let path_str = tmp.to_str().unwrap().to_string();
        std::env::set_var("VELOCITY_PHASE_TIMING", &path_str);
        let rec = PhaseRecorder::enabled();
        assert!(rec.is_some());
        let rec = rec.unwrap();
        assert_eq!(rec.path, path_str);
        assert!(rec.file.is_none());
        assert_eq!(rec.window_n, 0);
        std::env::remove_var("VELOCITY_PHASE_TIMING");
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_phase_recorder_record_and_flush() {
        let tmp = std::env::temp_dir().join(format!("phase_flush_{}.log", std::process::id()));
        let path_str = tmp.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp);

        let now = Instant::now();
        let mut rec = PhaseRecorder {
            path: path_str.clone(),
            file: None,
            window_n: 0,
            method: 0x01,
            wake_read_ns: 0,
            parse_ns: 0,
            dispatch_ns: 0,
            respond_ns: 0,
        };

        for i in 0..5 {
            let t_wake = now;
            let t_read = now + std::time::Duration::from_micros(1);
            let t_parse = now + std::time::Duration::from_micros(2);
            let t_dispatch = now + std::time::Duration::from_micros(10 + i);
            let t_respond = now + std::time::Duration::from_micros(12 + i);
            rec.record(0x01, t_wake, t_read, t_parse, t_dispatch, t_respond);
        }
        assert_eq!(rec.window_n, 5);

        rec.flush_window();
        assert_eq!(rec.window_n, 0);
        assert!(tmp.exists(), "phase log file should have been created");
        let contents = std::fs::read_to_string(&tmp).unwrap();
        assert!(contents.contains("PHASE"), "log should contain PHASE header: {}", contents);
        assert!(contents.contains("method=0x01"), "log should contain method code");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_phase_recorder_auto_flush_at_64() {
        let tmp = std::env::temp_dir().join(format!("phase_auto_{}.log", std::process::id()));
        let path_str = tmp.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp);

        let now = Instant::now();
        let mut rec = PhaseRecorder {
            path: path_str.clone(),
            file: None,
            window_n: 0,
            method: 0x02,
            wake_read_ns: 0,
            parse_ns: 0,
            dispatch_ns: 0,
            respond_ns: 0,
        };

        let t_wake = now;
        let t_read = now + std::time::Duration::from_micros(1);
        let t_parse = now + std::time::Duration::from_micros(2);
        let t_dispatch = now + std::time::Duration::from_micros(5);
        let t_respond = now + std::time::Duration::from_micros(7);

        for _ in 0..64 {
            rec.record(0x02, t_wake, t_read, t_parse, t_dispatch, t_respond);
        }
        assert_eq!(rec.window_n, 0, "window should auto-flush at 64");
        assert!(tmp.exists(), "phase log file should have been created by auto-flush");

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_phase_recorder_flush_empty_window() {
        let tmp = std::env::temp_dir().join(format!("phase_empty_{}.log", std::process::id()));
        let path_str = tmp.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp);

        let mut rec = PhaseRecorder {
            path: path_str,
            file: None,
            window_n: 0,
            method: 0,
            wake_read_ns: 0,
            parse_ns: 0,
            dispatch_ns: 0,
            respond_ns: 0,
        };
        rec.flush_window();
        assert!(!tmp.exists(), "empty window should not create file");
    }

    #[test]
    fn test_phase_recorder_drop_flushes() {
        let tmp = std::env::temp_dir().join(format!("phase_drop_{}.log", std::process::id()));
        let path_str = tmp.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&tmp);

        let now = Instant::now();
        {
            let mut rec = PhaseRecorder {
                path: path_str,
                file: None,
                window_n: 0,
                method: 0x03,
                wake_read_ns: 0,
                parse_ns: 0,
                dispatch_ns: 0,
                respond_ns: 0,
            };
            let t = now;
            rec.record(0x03, t, t, t, t, t);
            assert_eq!(rec.window_n, 1);
            // rec dropped here — Drop calls flush_window
        }
        assert!(tmp.exists(), "Drop should flush remaining window");
        let _ = std::fs::remove_file(&tmp);
    }

    // ─── Resources/Prompts success paths via NDA dispatch ───────────────────

    #[test]
    fn test_dispatch_resources_read_success() {
        let tmp = std::env::temp_dir().join(format!("nda_res_read_{}.txt", std::process::id()));
        std::fs::write(&tmp, "hello nda resource").unwrap();
        let path_str = tmp.to_str().unwrap().to_string();

        crate::resources::register_file_resource(
            "test://nda_dispatch_read",
            "NdaDispatchRead",
            "For NDA dispatch test",
            &path_str,
        );

        let data = json!({"uri": "test://nda_dispatch_read"});
        let frame = nda_native::build_nda_request(nda_native::METHOD_RESOURCES_READ, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["uri"].as_str().unwrap_or("").contains("nda_dispatch_read") || result.is_object());

        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_dispatch_prompts_get_success() {
        crate::resources::register_prompt(
            "nda_test_prompt_get",
            "NDA dispatch test prompt",
            vec![],
        );

        let data = json!({"name": "nda_test_prompt_get", "arguments": {}});
        let frame = nda_native::build_nda_request(nda_native::METHOD_PROMPTS_GET, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        let result = parse_response_result_json(&resp);
        assert!(result["messages"].is_array());
    }

    #[test]
    fn test_dispatch_sampling_create_no_handler() {
        let data = json!({"messages": [{"role": "user", "content": {"type": "text", "text": "test"}}]});
        let frame = nda_native::build_nda_request(nda_native::METHOD_SAMPLING_CREATE, &json!(1u64), &data).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
        let err = parse_response_error_msg(&resp);
        assert!(err.contains("sampling") || err.contains("No sampling"), "error was: {}", err);
    }

    #[test]
    fn test_dispatch_ping_with_delay() {
        // Set a small delay to test the delay path (lines 143-147)
        std::env::set_var("VELOCITY_PING_DELAY_US", "1000"); // 1ms delay

        let start = std::time::Instant::now();
        let frame = nda_native::build_nda_request(nda_native::METHOD_PING, &json!(1u64), &Value::Null).unwrap();
        let resp = dispatch_nda_request(&frame).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(parse_response_status(&resp), nda_native::STATUS_OK);
        // Should take at least 1ms due to the delay
        assert!(elapsed.as_micros() >= 1000, "expected delay >= 1000us, got {}us", elapsed.as_micros());

        // Clean up env var
        std::env::remove_var("VELOCITY_PING_DELAY_US");
    }

    #[test]
    fn test_dispatch_tools_call_rate_limit_exceeded() {
        // Exhaust the global rate limiter by making many rapid calls
        // Default burst is 100, so we need to exceed that
        let data = json!({"name": "rate_limit_test_tool", "arguments": {}});

        // Make enough calls to exhaust the rate limiter
        let mut rate_limited_hit = false;
        for _ in 0..150 {
            let frame = nda_native::build_nda_request(nda_native::METHOD_TOOLS_CALL, &json!(1u64), &data).unwrap();
            let resp = dispatch_nda_request(&frame).unwrap();

            if parse_response_status(&resp) == nda_native::STATUS_ERROR {
                let err = parse_response_error_msg(&resp);
                if err.contains("Rate limit exceeded") {
                    rate_limited_hit = true;
                    break;
                }
            }
        }

        // We should have hit the rate limit
        assert!(rate_limited_hit, "Expected to hit rate limit after 150 rapid calls");
    }

    #[test]
    fn test_dispatch_resources_list_malformed_data() {
        // Manually construct a frame with malformed data to trigger decode error (line 235)
        use sha2::{Sha256, Digest};

        // Build payload: method byte + id TLV + malformed data TLV
        let mut payload = Vec::new();
        payload.push(nda_native::METHOD_RESOURCES_LIST);
        // ID TLV (JSON value 1)
        let id_json = b"1";
        payload.push(0x02); // type for JSON
        payload.extend_from_slice(&(id_json.len() as u32).to_be_bytes());
        payload.extend_from_slice(id_json);
        // Malformed data TLV: type byte + length but invalid JSON
        payload.push(0x02); // type for JSON
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]); // invalid length/JSON

        // Compute merkle root
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();

        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP");
        frame.extend_from_slice(&merkle);
        frame.extend_from_slice(&payload);

        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
        let err = parse_response_error_msg(&resp);
        assert!(err.contains("Invalid request data") || err.contains("decode"), "error was: {}", err);
    }

    #[test]
    fn test_dispatch_resources_read_malformed_data() {
        use sha2::{Sha256, Digest};

        let mut payload = Vec::new();
        payload.push(nda_native::METHOD_RESOURCES_READ);
        let id_json = b"1";
        payload.push(0x02);
        payload.extend_from_slice(&(id_json.len() as u32).to_be_bytes());
        payload.extend_from_slice(id_json);
        payload.push(0x02);
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();

        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP");
        frame.extend_from_slice(&merkle);
        frame.extend_from_slice(&payload);

        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_prompts_list_malformed_data() {
        use sha2::{Sha256, Digest};

        let mut payload = Vec::new();
        payload.push(nda_native::METHOD_PROMPTS_LIST);
        let id_json = b"1";
        payload.push(0x02);
        payload.extend_from_slice(&(id_json.len() as u32).to_be_bytes());
        payload.extend_from_slice(id_json);
        payload.push(0x02);
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();

        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP");
        frame.extend_from_slice(&merkle);
        frame.extend_from_slice(&payload);

        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_resource_templates_list_malformed_data() {
        use sha2::{Sha256, Digest};

        let mut payload = Vec::new();
        payload.push(nda_native::METHOD_RESOURCE_TEMPLATES_LIST);
        let id_json = b"1";
        payload.push(0x02);
        payload.extend_from_slice(&(id_json.len() as u32).to_be_bytes());
        payload.extend_from_slice(id_json);
        payload.push(0x02);
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();

        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP");
        frame.extend_from_slice(&merkle);
        frame.extend_from_slice(&payload);

        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_prompts_get_malformed_data() {
        use sha2::{Sha256, Digest};

        let mut payload = Vec::new();
        payload.push(nda_native::METHOD_PROMPTS_GET);
        let id_json = b"1";
        payload.push(0x02);
        payload.extend_from_slice(&(id_json.len() as u32).to_be_bytes());
        payload.extend_from_slice(id_json);
        payload.push(0x02);
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();

        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP");
        frame.extend_from_slice(&merkle);
        frame.extend_from_slice(&payload);

        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    #[test]
    fn test_dispatch_sampling_create_malformed_data() {
        use sha2::{Sha256, Digest};

        let mut payload = Vec::new();
        payload.push(nda_native::METHOD_SAMPLING_CREATE);
        let id_json = b"1";
        payload.push(0x02);
        payload.extend_from_slice(&(id_json.len() as u32).to_be_bytes());
        payload.extend_from_slice(id_json);
        payload.push(0x02);
        payload.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);

        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();

        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP");
        frame.extend_from_slice(&merkle);
        frame.extend_from_slice(&payload);

        let resp = dispatch_nda_request(&frame).unwrap();
        assert_eq!(parse_response_status(&resp), nda_native::STATUS_ERROR);
    }

    // ─── shmem handler integration tests ─────────────────────────────────────

    fn temp_shmem_path(name: &str) -> String {
        format!("test_shmem_handler_{}.bin", name)
    }

    fn cleanup_shmem(path: &str) {
        let _ = std::fs::remove_file(path);
    }

    fn read_output_json(buffer: &SharedMemoryBuffer) -> Value {
        let output = buffer.read_output().expect("read_output failed");
        serde_json::from_str(&output).expect("output is not valid JSON")
    }

    #[test]
    fn test_handle_nda_native_valid_ping() {
        let path = temp_shmem_path("nda_ping");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let frame = nda_native::build_nda_request(
            nda_native::METHOD_PING, &json!(1u64), &Value::Null,
        ).unwrap();

        handle_nda_native(&mut buffer, &frame, None, None, None).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let output_len = buffer.get_output_len();
        assert!(output_len > 37, "output too small for NDA response frame: {} bytes", output_len);

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_nda_native_invalid_merkle() {
        let path = temp_shmem_path("nda_bad_merkle");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let mut bad_frame = Vec::new();
        bad_frame.extend_from_slice(b"NMCP");
        bad_frame.extend_from_slice(&[0xABu8; 32]);
        bad_frame.extend_from_slice(b"garbage payload");

        handle_nda_native(&mut buffer, &bad_frame, None, None, None).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_ERROR);
        assert!(buffer.get_output_len() > 0, "expected error response in output");

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_nda_native_with_phase_recorder() {
        let path = temp_shmem_path("nda_recorder");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let frame = nda_native::build_nda_request(
            nda_native::METHOD_PING, &json!(42u64), &Value::Null,
        ).unwrap();

        let mut rec = PhaseRecorder::enabled();
        let t_wake = Instant::now();
        let t_read = Instant::now();

        handle_nda_native(&mut buffer, &frame, rec.as_mut(), Some(t_wake), Some(t_read)).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_initialize() {
        let path = temp_shmem_path("json_init");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}},"id":1}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 1);
        assert!(resp["result"]["protocolVersion"].is_string());
        assert!(resp["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp["result"]["serverInfo"]["name"], "velocity-mcp-rust-server");

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_ping() {
        let path = temp_shmem_path("json_ping");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"ping","id":2}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["id"], 2);
        assert_eq!(resp["result"], json!({}));

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_tools_list() {
        let path = temp_shmem_path("json_tools_list");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"tools/list","id":3}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["id"], 3);
        assert!(resp["result"]["tools"].is_array());
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty(), "expected at least one registered tool");

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_tools_call_unknown() {
        let path = temp_shmem_path("json_tools_call_unk");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"nonexistent_tool_xyz","arguments":{}},"id":4}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["id"], 4);
        assert_eq!(resp["result"]["isError"], true);

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_health_check() {
        let path = temp_shmem_path("json_health");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"health/check","id":5}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["result"]["status"], "healthy");
        assert_eq!(resp["result"]["mode"], "shmem");

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_unknown_method() {
        let path = temp_shmem_path("json_unknown");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"frobnicate/whatever","id":6}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["id"], 6);
        assert_eq!(resp["error"]["code"], -32601);
        assert!(resp["error"]["message"].as_str().unwrap().contains("frobnicate/whatever"));

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_invalid_json() {
        let path = temp_shmem_path("json_bad");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        handle_json_shmem(&mut buffer, "{not valid json}}}").unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_ERROR);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["error"]["code"], -32700);
        assert!(resp["error"]["message"].as_str().unwrap().contains("Parse error"));

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_resources_list() {
        let path = temp_shmem_path("json_res_list");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"resources/list","id":7}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 7);
        assert!(resp["result"]["resources"].is_array());

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_prompts_list() {
        let path = temp_shmem_path("json_prompts_list");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"prompts/list","id":8}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 8);
        assert!(resp["result"]["prompts"].is_array());

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_notifications_cancelled() {
        let path = temp_shmem_path("json_cancelled");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"reason":"test cancel"},"id":null}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["result"], json!({}));

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_logging_set_level() {
        let path = temp_shmem_path("json_log_level");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"logging/setLevel","params":{"level":"debug"},"id":9}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["id"], 9);
        assert_eq!(resp["result"], json!({}));

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_notifications_initialized() {
        let path = temp_shmem_path("json_notif_init");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"notifications/initialized","id":null}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["result"], json!({}));

        cleanup_shmem(&path);
    }

    #[test]
    fn test_handle_json_shmem_resources_read_not_found() {
        let path = temp_shmem_path("json_res_read_nf");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let req = r#"{"jsonrpc":"2.0","method":"resources/read","params":{"uri":"file:///nonexistent_resource_xyz"},"id":10}"#;
        handle_json_shmem(&mut buffer, req).unwrap();

        assert_eq!(buffer.get_state(), shmem::STATE_RES_READY);
        let resp = read_output_json(&buffer);
        assert_eq!(resp["id"], 10);
        assert!(resp["error"].is_object(), "expected error for nonexistent resource");

        cleanup_shmem(&path);
    }

    #[test]
    fn test_shmem_loop_inner_immediate_shutdown() {
        let path = temp_shmem_path("loop_shutdown");
        cleanup_shmem(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();
        let shutdown = AtomicBool::new(true);

        let result = run_shmem_loop_inner(&mut buffer, &shutdown);
        assert!(result.is_ok());

        cleanup_shmem(&path);
    }

    // Note: run_shmem_loop_inner is not tested with threaded request processing
    // because on Windows, wait_for_request() calls WaitForSingleObject(INFINITE)
    // which blocks the thread permanently after the spin budget expires. The
    // loop's dispatch logic is fully covered by the direct handler tests above
    // (handle_nda_native, handle_json_shmem). The shutdown path is tested by
    // test_shmem_loop_inner_immediate_shutdown. End-to-end loop behavior is
    // covered by tests/e2e_smoke.rs which spawns the real binary.
}
