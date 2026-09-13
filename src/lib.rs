//! V.E.L.O.C.I.T.Y.-MCP library crate.
//!
//! Exposes the public API for integration testing and potential library consumers.

#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::needless_range_loop)]

/// Audit logging for tool executions (ring buffer, global instance).
pub mod audit;
/// Performance benchmark suite.
pub mod benchmark;
/// Configuration management.
pub mod config;
/// Comprehensive error handling.
pub mod error;
/// Memory-mapped shared memory buffer with atomic state machine.
pub mod ipc;
/// Advanced middleware and features.
#[cfg(feature = "http")]
pub mod middleware;
/// File-to-NDA converters (CSV, XLSX, DOCX, PDF, Image, Code, Binary).
pub mod nda_converter;
/// Native NDA binary document format (compile, read, string pool, Merkle tree).
pub mod nda_document;
/// NDA payload executor (BinaryPayload via .NET, SourceCode via interpreters).
pub mod nda_executor;
/// OAuth2 Connector Framework.
#[cfg(feature = "oauth2")]
pub mod oauth2;
/// Observability (OpenTelemetry integration).
#[cfg(feature = "observability")]
pub mod observability;
/// Plugin system for dynamic tool loading.
pub mod plugins;
/// JSON-RPC stdio handler and shared memory protocol loop.
pub mod protocol;
/// Token bucket rate limiter for MCP tool calls.
pub mod rate_limit;
/// Tool registration, path validation, and dispatch.
pub mod registry;
/// MCP Resources and Prompts support.
pub mod resources;
/// MCP Sampling protocol support.
pub mod sampling;
/// Sandboxed process execution (temp isolation, panic catching, output limits).
pub mod sandbox;
/// MCP Streaming and Progress Token support.
pub mod streaming;
/// Transport layer (stdio, shmem, HTTP/SSE).
#[cfg(feature = "http")]
pub mod transport;
/// WASM runtime for cross-language tool execution (QuickJS, MicroPython, Lua via Wasmer).
pub mod wasm_runtime;

/// Server version string.
pub const VERSION: &str = "3.1.0";

/// MCP protocol version supported by this server.
pub const PROTOCOL_VERSION: &str = "2024-11-05";
