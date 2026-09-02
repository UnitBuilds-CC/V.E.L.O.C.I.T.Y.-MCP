//! E2E smoke test: spawns the actual velocity_mcp binary and talks to it over stdio.
//!
//! This verifies the full stack: binary startup, CLI arg parsing, stdio transport,
//! JSON-RPC parsing, protocol dispatch, and response serialization.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct ServerProcess {
    child: Child,
    reader: Option<BufReader<std::process::ChildStdout>>,
}

impl ServerProcess {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_velocity_mcp"))
            .args(["--mode", "stdio"])
            .env("RUST_LOG", "error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn velocity_mcp binary");

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        ServerProcess {
            child,
            reader: Some(reader),
        }
    }

    fn send(&mut self, request: serde_json::Value) -> serde_json::Value {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        let line = serde_json::to_string(&request).unwrap();
        writeln!(stdin, "{}", line).expect("failed to write to stdin");
        stdin.flush().unwrap();
        self.read_response()
    }

    fn send_raw(&mut self, raw: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        writeln!(stdin, "{}", raw).expect("failed to write to stdin");
        stdin.flush().unwrap();
    }

    fn read_response(&mut self) -> serde_json::Value {
        let reader = self.reader.as_mut().expect("reader");
        let mut line = String::new();
        reader.read_line(&mut line).expect("failed to read response");
        serde_json::from_str(&line).expect("response is not valid JSON")
    }

}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.child.kill().ok();
        let _ = self.child.wait();
    }
}

trait ChildWaitTimeout {
    fn wait_timeout(&mut self, timeout: Duration) -> std::io::Result<Option<std::process::ExitStatus>>;
}

impl ChildWaitTimeout for Child {
    fn wait_timeout(&mut self, timeout: Duration) -> std::io::Result<Option<std::process::ExitStatus>> {
        let start = std::time::Instant::now();
        while start.elapsed() < timeout {
            match self.try_wait()? {
                Some(status) => return Ok(Some(status)),
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        }
        Ok(None)
    }
}

#[test]
fn test_e2e_stdio_initialize() {
    let mut server = ServerProcess::spawn();

    let response = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "initialize", "id": 1}),
    );

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(response["result"]["serverInfo"]["name"], "velocity-mcp-rust-server");
}

#[test]
fn test_e2e_stdio_tools_list() {
    let mut server = ServerProcess::spawn();

    let _ = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "initialize", "id": 1}),
    );

    // notifications/initialized produces no response
    server.send_raw(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);

    let response = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "tools/list", "id": 2}),
    );

    assert_eq!(response["id"], 2);
    let tools = response["result"]["tools"].as_array().unwrap();
    assert!(tools.len() >= 4, "expected at least 4 built-in tools, got {}", tools.len());

    let tool_names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(tool_names.contains(&"convert_to_nda_document"));
    assert!(tool_names.contains(&"read_nda"));
    assert!(tool_names.contains(&"execute_nda"));
}

#[test]
fn test_e2e_stdio_health_check() {
    let mut server = ServerProcess::spawn();

    let response = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "health/check", "id": 1}),
    );

    assert_eq!(response["result"]["status"], "healthy");
    assert_eq!(response["result"]["version"], velocity_mcp::VERSION);
}

#[test]
fn test_e2e_stdio_error_handling() {
    let mut server = ServerProcess::spawn();

    // Unknown method → JSON-RPC error
    let response = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "nonexistent/method", "id": 1}),
    );
    assert_eq!(response["error"]["code"], -32601);

    // Malformed JSON → parse error, server must not crash
    server.send_raw("this is not json at all");
    let err_resp = server.read_response();
    assert_eq!(err_resp["error"]["code"], -32700);

    // Server still works after a parse error
    let response = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "health/check", "id": 2}),
    );
    assert_eq!(response["result"]["status"], "healthy");
}

#[test]
fn test_e2e_stdio_multiple_requests() {
    let mut server = ServerProcess::spawn();

    for i in 1..=10 {
        let response = server.send(
            serde_json::json!({"jsonrpc": "2.0", "method": "health/check", "id": i}),
        );
        assert_eq!(response["id"], i);
        assert_eq!(response["result"]["status"], "healthy");
    }
}

#[test]
fn test_e2e_stdio_clean_shutdown_on_eof() {
    let mut server = ServerProcess::spawn();

    let response = server.send(
        serde_json::json!({"jsonrpc": "2.0", "method": "health/check", "id": 1}),
    );
    assert_eq!(response["result"]["status"], "healthy");

    // Close stdin → server should exit cleanly
    drop(server.child.stdin.take());
    // Drop the reader so it doesn't hold stdout
    drop(server.reader.take());

    let exit_status = server.child
        .wait_timeout(Duration::from_secs(5))
        .expect("wait_timeout failed");

    assert!(exit_status.is_some(), "server should exit within 5s after stdin closes");
}
