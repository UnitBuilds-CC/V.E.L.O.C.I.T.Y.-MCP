//! Transport layer for MCP protocol.
//!
//! Supports multiple transport mechanisms:
//! - stdio (JSON-RPC over stdin/stdout)
//! - shmem (shared memory with NDA-native or JSON frames)
//! - http (JSON-RPC over HTTP POST + SSE streaming, feature-gated)

#[cfg(feature = "http")]
pub mod http;
