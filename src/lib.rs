//! V.E.L.O.C.I.T.Y.-MCP library crate.
//!
//! Exposes the public API for integration testing and potential library consumers.

/// JSON-RPC stdio handler and shared memory protocol loop.
pub mod protocol;
/// Memory-mapped shared memory buffer with atomic state machine.
pub mod ipc;
/// Tool registration, path validation, and dispatch.
pub mod registry;
/// Performance benchmark suite.
pub mod benchmark;
/// Native NDA binary document format (compile, read, string pool, Merkle tree).
pub mod nda_document;
/// File-to-NDA converters (CSV, XLSX, DOCX, PDF, Image, Code, Binary).
pub mod nda_converter;
/// NDA payload executor (BinaryPayload via .NET, SourceCode via interpreters).
pub mod nda_executor;
/// Sandboxed process execution (temp isolation, panic catching, output limits).
pub mod sandbox;
/// Audit logging for tool executions (ring buffer, global instance).
pub mod audit;
/// Token bucket rate limiter for MCP tool calls.
pub mod rate_limit;

/// Server version string.
pub const VERSION: &str = "3.0.0";
