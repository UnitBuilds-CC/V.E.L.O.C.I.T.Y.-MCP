//! V.E.L.O.C.I.T.Y.-MCP library crate.
//!
//! Exposes the public API for integration testing and potential library consumers.

/// JSON-RPC stdio handler and shared memory protocol loop.
pub mod protocol;
/// Memory-mapped shared memory buffer with atomic state machine.
pub mod ipc;
/// Tool registration, path validation, and C# process delegation.
pub mod registry;
/// Performance benchmark suite.
pub mod benchmark;

/// Server version string.
pub const VERSION: &str = "1.0.0";
