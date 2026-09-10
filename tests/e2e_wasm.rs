//! E2E test: WASM plugin tools across all language runtimes.
//!
//! Spawns the real velocity_mcp binary over stdio, verifies all WASM-backed
//! plugin tools appear in tools/list, and executes one tool per language
//! runtime through the live JSON-RPC stack.
//!
//! Runtime capability split (all verified through this test):
//! - Real engines (tool source executes, arguments are passed through):
//!   QuickJS (js/ts), MicroPython (py), Lua, mruby (rb), Rust, TinyGo (go)
//! - Minimal interpreters (~300-line toy WASI shims in bench_tools/*_wasi.c;
//!   register_tool stores a built-in demo wrapper that runs instead of the
//!   tool source, and arguments are never exposed to the interpreted code):
//!   php, csharp, java, r, julia, perl

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

use serde_json::{json, Value};

const SAMPLE_TEXT: &str = "hello wasm world";

struct ServerProcess {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl ServerProcess {
    fn spawn() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_velocity_mcp"))
            .args(["--mode", "stdio"])
            .env("RUST_LOG", "error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn velocity_mcp binary");

        let stdout = child.stdout.take().unwrap();
        let mut server = ServerProcess {
            child,
            reader: BufReader::new(stdout),
            next_id: 1,
        };

        let response = server.request(json!({
            "jsonrpc": "2.0",
            "method": "initialize",
            "id": 0,
        }));
        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        server.send_raw(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        server
    }

    fn request(&mut self, request: Value) -> Value {
        self.next_id += 1;
        let mut req = request;
        req["id"] = json!(self.next_id);
        self.send(req)
    }

    fn send(&mut self, request: Value) -> Value {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        let line = serde_json::to_string(&request).unwrap();
        writeln!(stdin, "{}", line).expect("failed to write to stdin");
        stdin.flush().unwrap();

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("failed to read response");
        serde_json::from_str(&response_line)
            .unwrap_or_else(|e| panic!("response is not valid JSON: {} — got: {}", e, response_line))
    }

    fn send_raw(&mut self, raw: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        writeln!(stdin, "{}", raw).expect("failed to write to stdin");
        stdin.flush().unwrap();
    }

    /// Call a tool and return the raw first-content-block text.
    fn call_tool_raw(&mut self, name: &str, arguments: Value) -> String {
        let response = self.request(json!({
            "jsonrpc": "2.0",
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        }));

        if let Some(err) = response.get("error") {
            panic!("tools/call '{}' returned JSON-RPC error: {}", name, err);
        }

        let result = &response["result"];
        if result["isError"].as_bool().unwrap_or(false) {
            panic!(
                "tool '{}' reported execution error: {:?}",
                name,
                result["content"][0]["text"].as_str()
            );
        }

        result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tool '{}' returned no text content: {}", name, result))
            .to_string()
    }

    /// Call a tool and return the parsed JSON payload from the first content block.
    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        let text = self.call_tool_raw(name, arguments);
        serde_json::from_str(&text)
            .unwrap_or_else(|e| panic!("tool '{}' result is not valid JSON ({}): {}", name, e, text))
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.child.kill().ok();
        let _ = self.child.wait();
    }
}

#[test]
fn test_e2e_wasm_tools_listed() {
    let mut server = ServerProcess::spawn();

    let response = server.request(json!({"jsonrpc": "2.0", "method": "tools/list"}));
    let tools = response["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    let expected = [
        "js_string_transform",
        "lua_string_utils",
        "py_text_analyzer",
        "ts_text_stats",
        "rb_text_stats",
        "rust_text_stats",
        "php_hello",
        "csharp_hello",
        "java_hello",
        "r_hello",
        "julia_hello",
        "perl_hello",
        "go_text_stats",
    ];
    for tool in expected {
        assert!(
            names.contains(&tool),
            "expected WASM tool '{}' in tools/list, got: {:?}",
            tool,
            names
        );
    }
}

#[test]
fn test_e2e_wasm_call_real_engine_runtimes() {
    let mut server = ServerProcess::spawn();

    // JavaScript (QuickJS)
    let result = server.call_tool("js_string_transform", json!({"action": "upper", "input": "hello"}));
    assert_eq!(result["result"], "HELLO", "javascript result: {}", result);

    // Lua
    let result = server.call_tool("lua_string_utils", json!({"action": "upper", "input": "hello"}));
    assert_eq!(result["result"], "HELLO", "lua result: {}", result);

    // Python (MicroPython)
    let result = server.call_tool("py_text_analyzer", json!({"text": SAMPLE_TEXT}));
    assert_eq!(result["words"], 3, "python result: {}", result);
    assert_eq!(result["chars"], 16, "python result: {}", result);

    // TypeScript (QuickJS + transpiler)
    let result = server.call_tool("ts_text_stats", json!({"text": SAMPLE_TEXT}));
    assert_eq!(result["chars"], 16, "typescript result: {}", result);
    assert_eq!(result["words"], 3, "typescript result: {}", result);
    assert_eq!(result["upper"], "HELLO WASM WORLD", "typescript result: {}", result);

    // Ruby (mruby)
    let result = server.call_tool("rb_text_stats", json!({"text": SAMPLE_TEXT}));
    assert_eq!(result["chars"], 16, "ruby result: {}", result);
    assert_eq!(result["words"], 3, "ruby result: {}", result);
    assert_eq!(result["upper"], "HELLO WASM WORLD", "ruby result: {}", result);
}

#[test]
fn test_e2e_wasm_call_minimal_interpreter_runtimes() {
    // These six runtimes are minimal script interpreters (bench_tools/*_wasi.c).
    // register_tool stores a built-in demo wrapper that executes instead of the
    // tool source, and arguments are copied but never exposed to the script.
    // The assertions pin that real, current behavior through the live binary —
    // a regression here means the plumbing (plugin manifest -> registry ->
    // runtime -> WASM -> JSON-RPC) broke, not the tool logic.
    let mut server = ServerProcess::spawn();

    // php/csharp/java/r/perl wrappers use `return "..."` which the shims wrap
    // as {"message": "<result>"}.
    for (tool, greeting) in [
        ("php_hello", "Hello, PHP!"),
        ("csharp_hello", "Hello, C#!"),
        ("java_hello", "Hello, Java!"),
        ("r_hello", "Hello, R!"),
        ("perl_hello", "Hello, Perl!"),
    ] {
        let result = server.call_tool(tool, json!({"text": SAMPLE_TEXT}));
        assert_eq!(result["message"], greeting, "{} result: {}", tool, result);
    }

    // Julia registers source + wrapper combined, so the manifest source
    // genuinely executes at call time, followed by the demo greeting.
    let text = server.call_tool_raw("julia_hello", json!({"text": SAMPLE_TEXT}));
    assert_eq!(text, "julia tool ready\nHello, Julia!", "julia result: {}", text);
}

#[test]
fn test_e2e_wasm_call_compiled_runtimes() {
    let mut server = ServerProcess::spawn();

    // Rust (wasm32-wasi example tool, path-based source)
    let result = server.call_tool("rust_text_stats", json!({"text": SAMPLE_TEXT}));
    assert_eq!(result["word_count"], 3, "rust result: {}", result);
    assert_eq!(result["char_count"], 16, "rust result: {}", result);
    assert_eq!(result["line_count"], 1, "rust result: {}", result);

    // Go (TinyGo standalone ABI)
    let result = server.call_tool("go_text_stats", json!({"text": SAMPLE_TEXT}));
    assert_eq!(result["word_count"], 3, "go result: {}", result);
    assert_eq!(result["char_count"], 16, "go result: {}", result);
    assert_eq!(result["line_count"], 1, "go result: {}", result);
}
