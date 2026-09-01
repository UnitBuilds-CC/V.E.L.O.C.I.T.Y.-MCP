//! JSON transport over shared memory.
//!
//! Sends standard JSON-RPC requests via the VELOCITY-MCP shared memory
//! interface. Useful for debugging and compatibility with servers that
//! don't support the NDA binary protocol.

use crate::error::{Error, Result};
use crate::transport::shmem::ShmemBuffer;
use crate::transport::Transport;
use crate::types::{JsonRpcRequest, JsonRpcResponse};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub struct JsonShmemTransport {
    buffer: Mutex<Option<ShmemBuffer>>,
    closed: AtomicBool,
}

impl JsonShmemTransport {
    pub fn new(buffer_path: &str) -> Result<Self> {
        let buffer = ShmemBuffer::open(buffer_path)?;
        Ok(Self {
            buffer: Mutex::new(Some(buffer)),
            closed: AtomicBool::new(false),
        })
    }
}

#[async_trait::async_trait]
impl Transport for JsonShmemTransport {
    async fn send(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        if self.closed.load(Ordering::Acquire) {
            return Err(Error::ConnectionClosed);
        }

        let json_bytes = serde_json::to_vec(&request)?;

        let response_bytes = {
            let mut guard = self.buffer.lock().map_err(|e| Error::SharedMemory(format!("lock: {}", e)))?;
            let buf = guard.as_mut().ok_or(Error::ConnectionClosed)?;
            buf.send_raw(&json_bytes)?
        };

        let response_str = String::from_utf8(response_bytes)
            .map_err(|e| Error::SharedMemory(format!("Invalid UTF-8 in response: {}", e)))?;

        let response: JsonRpcResponse = serde_json::from_str(&response_str)?;

        if response.jsonrpc != "2.0" {
            return Err(Error::SharedMemory(format!(
                "Invalid JSON-RPC version: expected '2.0', got '{}'",
                response.jsonrpc
            )));
        }

        match (&response.result, &response.error) {
            (None, None) => {
                return Err(Error::SharedMemory(
                    "Response has neither 'result' nor 'error'".into(),
                ));
            }
            (Some(_), Some(_)) => {
                return Err(Error::SharedMemory(
                    "Response has both 'result' and 'error'".into(),
                ));
            }
            _ => {}
        }

        if request.id.is_some() && response.id != request.id {
            return Err(Error::StaleResponse(format!(
                "Response ID {:?} does not match request ID {:?}",
                response.id, request.id
            )));
        }

        Ok(response)
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
