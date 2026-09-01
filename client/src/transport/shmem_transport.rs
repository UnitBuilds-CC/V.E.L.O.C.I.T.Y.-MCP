//! NDA binary transport over shared memory.
//!
//! Sends JSON-RPC requests as NDA binary TLV frames via the VELOCITY-MCP
//! shared memory interface. Achieves ~1µs round-trip latency.

use crate::error::{Error, Result};
use crate::transport::nda_codec;
use crate::transport::shmem::ShmemBuffer;
use crate::transport::Transport;
use crate::types::{JsonRpcRequest, JsonRpcResponse};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct ShmemTransport {
    buffer: Mutex<Option<ShmemBuffer>>,
    closed: AtomicBool,
}

impl ShmemTransport {
    pub fn new(buffer_path: &str) -> Result<Self> {
        let buffer = ShmemBuffer::open(buffer_path)?;
        Ok(Self {
            buffer: Mutex::new(Some(buffer)),
            closed: AtomicBool::new(false),
        })
    }
}

#[async_trait::async_trait]
impl Transport for ShmemTransport {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::ConnectionClosed);
        }

        let method = request.method.clone();
        let is_notification = request.id.is_none();
        let id = request.id.clone().unwrap_or(Value::Null);
        let params = request.params.clone().unwrap_or(json!({}));

        let method_code = nda_codec::method_to_code(&method).ok_or_else(|| {
            Error::NdaProtocol(format!("Unsupported method for NDA transport: {}", method))
        })?;

        let frame = nda_codec::build_nda_request(method_code, &id, &params)?;

        let response_bytes = {
            let mut guard = self.buffer.lock().map_err(|e| Error::SharedMemory(format!("lock: {}", e)))?;
            let buf = guard.as_mut().ok_or(Error::ConnectionClosed)?;
            buf.send_raw(&frame)?
        };

        if is_notification {
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: Some(json!({})),
                error: None,
                id: None,
            });
        }

        let nda_resp = nda_codec::parse_nda_response(&response_bytes)?;

        if nda_resp.status == nda_codec::STATUS_ERROR {
            let err_msg = nda_resp.result.as_str().unwrap_or("Unknown NDA error");
            return Ok(JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(crate::types::JsonRpcError {
                    code: -1,
                    message: err_msg.to_string(),
                    data: None,
                }),
                id: Some(nda_resp.request_id),
            });
        }

        Ok(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(nda_resp.result),
            error: None,
            id: Some(nda_resp.request_id),
        })
    }

    async fn close(&self) -> Result<()> {
        self.closed.store(true, Ordering::Release);
        let mut guard = self.buffer.lock().map_err(|e| Error::SharedMemory(format!("lock: {}", e)))?;
        if let Some(buf) = guard.take() {
            drop(buf);
        }
        Ok(())
    }
}
