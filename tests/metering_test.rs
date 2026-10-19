//! Integration tests for WASM instruction-limit (metering) wiring.
//!
//! These run the real quickjs.wasm module end-to-end: the factory path and the
//! plugin dispatch path (which shares a cached runtime behind a global Mutex).
//! Both fixtures are tracked in the repository (verified 2026-10-11 with `git ls-files`), so these
//! gates run from a clean clone; the `exists()` guards stay only for a working tree where the
//! `bench_tools/` fixture directories were pruned, and a skip there is a local-state fact.

use serde_json::json;
use velocity_mcp::plugins::{execute_plugin_tool, PluginExecutor, PluginTool};
use velocity_mcp::wasm_runtime::create_wasm_runtime_for_language;

const QUICKJS_WASM: &str = "bench_tools/quickjs_wasm/quickjs.wasm";

fn quickjs_available() -> bool {
    std::path::Path::new(QUICKJS_WASM).exists()
}

fn wasm_tool(name: &str, source: &str) -> PluginTool {
    PluginTool {
        name: name.to_string(),
        description: "metering test tool".to_string(),
        input_schema: json!({ "type": "object" }),
        executor: PluginExecutor {
            executor_type: "wasm".to_string(),
            command: String::new(),
            args: Vec::new(),
            working_dir: None,
            env: Default::default(),
            timeout: 30,
            language: Some("javascript".to_string()),
            source: Some(source.to_string()),
            source_file: None,
            handler_function: None,
        },
    }
}

/// The default 10M instruction budget must be generous enough for real tool
/// calls through the factory-built runtime (proves metered engines still run).
#[test]
fn test_quickjs_runs_within_default_instruction_budget() {
    if !quickjs_available() {
        eprintln!(
            "skipping: {} not present in this working tree",
            QUICKJS_WASM
        );
        return;
    }

    let mut runtime = create_wasm_runtime_for_language("javascript", QUICKJS_WASM)
        .expect("QuickJS runtime should build from factory");
    runtime.init().expect("QuickJS runtime init should succeed");

    runtime
        .register_tool(
            "metering_echo",
            "function metering_echo(args) { return args; }",
        )
        .expect("echo tool should register");

    for i in 0..5 {
        let args = json!({ "n": i, "msg": "ping" });
        let out = runtime
            .call_tool("metering_echo", &args.to_string())
            .unwrap_or_else(|e| panic!("call {} failed: {}", i, e));
        let parsed: serde_json::Value = serde_json::from_str(out.trim())
            .unwrap_or_else(|e| panic!("call {} returned non-JSON {:?}: {}", i, out, e));
        assert_eq!(parsed["n"], json!(i), "echo should round-trip args");
    }
}

/// A while(true) tool must be stopped by the metering limit and classified as
/// a resource-limit error. A trap unwinds the interpreter mid-execution, so
/// the poisoned cached runtime must be evicted: the follow-up echo call (same
/// plugin path, same language) only succeeds if the server rebuilt a fresh
/// instance. Sequential on purpose — recovery only holds if the echo runs
/// after the trap on the shared cache.
#[test]
fn test_plugin_trap_classified_and_runtime_recovers() {
    if !quickjs_available() {
        eprintln!(
            "skipping: {} not present in this working tree",
            QUICKJS_WASM
        );
        return;
    }

    let spin = wasm_tool(
        "metering_spin",
        "function metering_spin(args) { while (true) {} }",
    );

    let err = execute_plugin_tool(&spin, &json!({}))
        .expect_err("infinite loop must be stopped by the instruction limit");
    assert!(
        err.contains("exceeded resource limits"),
        "expected metering classification, got: {}",
        err
    );

    let echo = wasm_tool(
        "metering_echo_after_trap",
        "function metering_echo_after_trap(args) { return args; }",
    );

    let out = execute_plugin_tool(&echo, &json!({ "ok": true }))
        .expect("echo must work after a metering trap evicted the cached instance");
    let parsed: serde_json::Value = serde_json::from_str(out.trim())
        .unwrap_or_else(|e| panic!("echo returned non-JSON {:?}: {}", out, e));
    assert_eq!(parsed["ok"], json!(true), "echo should round-trip args");
}

const MICROPYTHON_WASM: &str = "bench_tools/micropython_wasm/wasi-reactor/build/micropython.wasm";

fn python_tool(name: &str, source: &str) -> PluginTool {
    let mut tool = wasm_tool(name, source);
    tool.executor.language = Some("python".to_string());
    tool
}

/// A trap that is *not* a metering limit must evict the cached runtime too.
///
/// Measured on MicroPython: an infinitely recursive tool traps with
/// `RuntimeError: out of bounds memory access` — no metering wording anywhere —
/// and the instance is left unable to serve, every later call failing inside
/// `nlr_push_tail`. Eviction used to key on metering strings only, so one
/// recursive tool took Python down for the life of the process.
#[test]
fn test_non_metering_trap_evicts_cached_runtime() {
    if !std::path::Path::new(MICROPYTHON_WASM).exists() {
        eprintln!(
            "skipping: {} not present in this working tree",
            MICROPYTHON_WASM
        );
        return;
    }

    let recurse = python_tool(
        "trap_recurse",
        "def trap_recurse(args):\n    return trap_recurse(args)\n",
    );

    let err = execute_plugin_tool(&recurse, &json!({}))
        .expect_err("unbounded recursion must not be reported as a success");

    let echo = python_tool(
        "trap_echo_after",
        "def trap_echo_after(args):\n    return {'ok': True}\n",
    );

    let out = execute_plugin_tool(&echo, &json!({})).unwrap_or_else(|e| {
        panic!(
            "Python runtime did not recover after a non-metering trap, so the poisoned \
             instance was left cached.\n  trap error: {err}\n  recovery error: {e}"
        )
    });
    let parsed: serde_json::Value = serde_json::from_str(out.trim())
        .unwrap_or_else(|e| panic!("echo returned non-JSON {:?}: {}", out, e));
    assert_eq!(
        parsed["ok"],
        json!(true),
        "echo should run on a fresh runtime"
    );
}
