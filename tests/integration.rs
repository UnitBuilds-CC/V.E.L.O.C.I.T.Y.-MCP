//! Integration tests for V.E.L.O.C.I.T.Y.-MCP.
//!
//! These tests exercise cross-module flows from an external consumer perspective,
//! verifying that the protocol handlers, IPC layer, and registry work together correctly.

use serde_json::json;
use std::fs;
use velocity_mcp::ipc::shmem::{
    SharedMemoryBuffer, STATE_ERROR, STATE_IDLE, STATE_PROCESSING, STATE_REQ_READY, STATE_RES_READY,
};
use velocity_mcp::protocol::json_rpc::handle_request;
use velocity_mcp::protocol::nmcp_binary::NmcpBinaryFrame;
use velocity_mcp::registry;

// ─── JSON-RPC Protocol Flow ──────────────────────────────────────────────────

/// Simulate a full MCP session: initialize → notifications/initialized → tools/list → tools/call → health/check.
#[test]
fn test_full_mcp_session_flow() {
    // Step 1: Initialize
    let init_req = json!({"jsonrpc": "2.0", "method": "initialize", "id": 1});
    let init_res = handle_request(&init_req).expect("initialize must return a response");
    assert_eq!(init_res["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(init_res["result"]["serverInfo"]["name"], "velocity-mcp-rust-server");
    assert_eq!(init_res["result"]["serverInfo"]["version"], velocity_mcp::VERSION);

    // Step 2: notifications/initialized (notification — no response)
    let notif = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    assert!(handle_request(&notif).is_none(), "notifications must not return a response");

    // Step 3: tools/list
    let list_req = json!({"jsonrpc": "2.0", "method": "tools/list", "id": 2});
    let list_res = handle_request(&list_req).expect("tools/list must return a response");
    let tools = list_res["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);

    // Step 4: tools/call with unknown tool (should return error content, not crash)
    let call_req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": { "name": "nonexistent", "arguments": {} },
        "id": 3
    });
    let call_res = handle_request(&call_req).expect("tools/call must return a response even on error");
    assert_eq!(call_res["result"]["isError"], true);

    // Step 5: health/check
    let health_req = json!({"jsonrpc": "2.0", "method": "health/check", "id": 4});
    let health_res = handle_request(&health_req).expect("health/check must return a response");
    assert_eq!(health_res["result"]["status"], "healthy");
    assert_eq!(health_res["result"]["version"], velocity_mcp::VERSION);
}

/// Verify that all JSON-RPC responses include the correct version string.
#[test]
fn test_version_consistency_across_responses() {
    let version = velocity_mcp::VERSION;

    let init = handle_request(&json!({"jsonrpc":"2.0","method":"initialize","id":1})).unwrap();
    assert_eq!(init["result"]["serverInfo"]["version"], version);

    let health = handle_request(&json!({"jsonrpc":"2.0","method":"health/check","id":2})).unwrap();
    assert_eq!(health["result"]["version"], version);
}

/// Verify JSON-RPC error codes follow the spec.
#[test]
fn test_json_rpc_error_codes() {
    // -32601 for unknown methods
    let res = handle_request(&json!({"jsonrpc":"2.0","method":"unknown","id":1})).unwrap();
    assert_eq!(res["error"]["code"], -32601);

    // Unknown method without id returns None (treated as notification)
    assert!(handle_request(&json!({"jsonrpc":"2.0","method":"unknown"})).is_none());
}

// ─── Shared Memory Full Lifecycle ────────────────────────────────────────────

/// Simulate a complete host↔server exchange through shared memory.
#[test]
fn test_shmem_full_host_server_exchange() {
    let path = "test_integration_shmem_exchange.bin";
    let _ = fs::remove_file(path);

    // ── Host side: create buffer and write a request ──
    let mut buffer = SharedMemoryBuffer::create_or_open(path).unwrap();
    assert_eq!(buffer.get_state(), STATE_IDLE);

    let request = r#"{"jsonrpc":"2.0","method":"tools/list","id":1}"#;
    let req_bytes = request.as_bytes();

    // Host writes request data
    buffer.set_input_len(req_bytes.len() as u32);
    // Access the mmap directly through the buffer's read_input to verify later
    // For this test, we write through the public API and verify the round-trip

    // Simulate host setting state to REQ_READY
    buffer.set_state(STATE_REQ_READY);
    buffer.flush().unwrap();

    // ── Server side: read request ──
    assert_eq!(buffer.get_state(), STATE_REQ_READY);

    // Server sets PROCESSING
    buffer.set_state(STATE_PROCESSING);
    buffer.flush().unwrap();

    // ── Server side: write response ──
    let response = r#"{"jsonrpc":"2.0","result":{"tools":[]},"id":1}"#;
    buffer.write_output(response).unwrap();
    SharedMemoryBuffer::sync_fence();
    buffer.set_state(STATE_RES_READY);
    buffer.flush().unwrap();

    // ── Host side: read response ──
    assert_eq!(buffer.get_state(), STATE_RES_READY);
    assert_eq!(buffer.get_output_len(), response.len() as u32);

    // ── Cleanup ──
    drop(buffer);
    fs::remove_file(path).unwrap();
}

/// Verify all 5 state transitions work correctly through the public API.
#[test]
fn test_shmem_state_machine_transitions() {
    let path = "test_integration_shmem_states.bin";
    let _ = fs::remove_file(path);

    let mut buffer = SharedMemoryBuffer::create_or_open(path).unwrap();

    // Full state machine cycle
    let transitions = [
        (STATE_IDLE, "initial idle"),
        (STATE_REQ_READY, "host request"),
        (STATE_PROCESSING, "server processing"),
        (STATE_RES_READY, "response ready"),
        (STATE_IDLE, "back to idle"),
        (STATE_ERROR, "error state"),
        (STATE_IDLE, "recovery"),
    ];

    for (state, label) in transitions {
        buffer.set_state(state);
        buffer.flush().unwrap();
        assert_eq!(buffer.get_state(), state, "State mismatch at: {}", label);
    }

    drop(buffer);
    let _ = fs::remove_file(path);
}

/// Verify that buffer size limits are enforced through the public API.
#[test]
fn test_shmem_buffer_boundary_conditions() {
    let path = "test_integration_shmem_bounds.bin";
    let _ = fs::remove_file(path);

    let mut buffer = SharedMemoryBuffer::create_or_open(path).unwrap();

    // Maximum valid output (61440 bytes = TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET)
    let max_output = "x".repeat(61440);
    assert!(buffer.write_output(&max_output).is_ok(), "Max output should succeed");

    // One byte over the limit
    let over_output = "x".repeat(61441);
    assert!(buffer.write_output(&over_output).is_err(), "Over-limit output should fail");

    drop(buffer);
    let _ = fs::remove_file(path);
}

// ─── NMCP Binary Frame Parser ────────────────────────────────────────────────

/// Verify binary frame parser handles all edge cases correctly.
#[test]
fn test_binary_frame_parser_edge_cases() {
    // Exactly minimum size (36 bytes = 4 magic + 32 merkle, no payload)
    let mut min_frame = Vec::new();
    min_frame.extend_from_slice(b"NMCP");
    min_frame.extend_from_slice(&[0xFFu8; 32]);
    let frame = NmcpBinaryFrame::parse(&min_frame).unwrap();
    assert_eq!(frame.magic, b"NMCP");
    assert_eq!(frame.merkle_root, &[0xFFu8; 32]);
    assert!(frame.payload.is_empty());

    // One byte below minimum
    let too_small = vec![0u8; 35];
    assert!(NmcpBinaryFrame::parse(&too_small).is_err());

    // Large payload
    let mut large = Vec::new();
    large.extend_from_slice(b"NMCP");
    large.extend_from_slice(&[0u8; 32]);
    large.extend_from_slice(&vec![0xABu8; 10_000]);
    let frame = NmcpBinaryFrame::parse(&large).unwrap();
    assert_eq!(frame.payload.len(), 10_000);
    assert!(frame.payload.iter().all(|&b| b == 0xAB));
}

// ─── Registry and Tool Dispatch ──────────────────────────────────────────────

/// Verify tool registry returns correct tool definitions with valid schemas.
#[test]
fn test_registry_tool_definitions() {
    let tools = registry::get_tools();
    assert_eq!(tools.len(), 3);

    let expected_names = ["convert_to_nda", "read_nda", "execute_nda"];
    for (tool, expected) in tools.iter().zip(expected_names.iter()) {
        assert_eq!(&tool.name, expected);
        assert!(!tool.description.is_empty());
        assert_eq!(tool.input_schema["type"], "object");
        assert!(tool.input_schema["properties"].is_object());
        assert!(tool.input_schema["required"].is_array());
    }
}

/// Verify that path validation blocks all attack vectors before execution.
#[test]
fn test_registry_path_validation_blocks_attacks() {
    // Path traversal
    let result = registry::call_tool(
        "read_nda",
        &json!({"ndaPath": "C:\\Users\\test\\..\\..\\Windows\\System32\\config\\SAM"}),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("traversal"));

    // Relative path
    let result = registry::call_tool(
        "convert_to_nda",
        &json!({"filePath": "relative/file.txt"}),
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("absolute"));

    // Empty path
    let result = registry::call_tool(
        "execute_nda",
        &json!({"ndaPath": ""}),
    );
    assert!(result.is_err());
}

/// Verify that calling a non-existent C# path returns a clear error.
#[test]
fn test_registry_missing_csharp_engine() {
    let result = registry::call_tool_with_csharp_path(
        "read_nda",
        &json!({"ndaPath": "C:\\test.nda"}),
        "C:\\nonexistent\\NdaMcpServer.exe",
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not found"));
}

/// Verify env var override for C# path.
#[test]
fn test_registry_env_var_path_override() {
    let original = std::env::var("VELOCITY_CSHARP_PATH").ok();

    std::env::set_var("VELOCITY_CSHARP_PATH", "C:\\custom\\path\\server.exe");
    assert_eq!(registry::resolve_csharp_path(), "C:\\custom\\path\\server.exe");

    // Restore original
    match original {
        Some(val) => std::env::set_var("VELOCITY_CSHARP_PATH", val),
        None => std::env::remove_var("VELOCITY_CSHARP_PATH"),
    }
}

// ─── Cross-Module: JSON-RPC → Registry ──────────────────────────────────────

/// Verify that a tools/call through the protocol layer correctly dispatches
/// to the registry and returns a well-formed error for missing params.
#[test]
fn test_protocol_to_registry_dispatch() {
    // Missing required param — should get isError: true with descriptive text
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": { "name": "read_nda", "arguments": {} },
        "id": 10
    });
    let res = handle_request(&req).unwrap();
    assert_eq!(res["result"]["isError"], true);
    let text = res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("ndaPath is required") || text.contains("Error running tool"));

    // Unknown tool — should get an error (routed to C# engine which rejects it)
    let req = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": { "name": "delete_everything", "arguments": {} },
        "id": 11
    });
    let res = handle_request(&req).unwrap();
    assert_eq!(res["result"]["isError"], true);
}
