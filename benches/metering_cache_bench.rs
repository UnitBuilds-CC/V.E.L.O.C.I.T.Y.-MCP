//! Metering & Module-Cache Benchmark
//!
//! Measures the two Wasmer 5 features shipped with the WASM runtime:
//!
//!   A. Module primitives (quickjs.wasm, lua.wasm):
//!        compile unmetered vs compile metered vs deserialize cached artifact
//!   B. Runtime cold start via create_wasm_runtime_for_language:
//!        first creation (compile) vs later creation (cache hit) per language
//!   C. Per-call metering overhead through the real dispatch path
//!        (WasmRuntimeRegistry::call_tool, includes reset_instruction_budget)
//!   D. E2E stdio sanity with the real server binary
//!        (VELOCITY_WASM_INSTRUCTION_LIMIT=10000000 vs 0)
//!
//! Run: cargo bench --bench metering_cache_bench

use std::hint::black_box;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use serde_json::json;
use velocity_mcp::wasm_runtime::{
    build_metered_engine, create_wasm_runtime_for_language, lua::LuaRuntime,
    quickjs::QuickJsRuntime, set_instruction_limit, WasmRuntimeRegistry,
};

const QUICKJS_WASM: &str = "bench_tools/quickjs_wasm/quickjs.wasm";
const LUA_WASM: &str = "bench_tools/lua_wasm/lua.wasm";

/// Production default limit (WASM_INSTRUCTION_LIMIT initial value).
const LIMIT: u64 = 10_000_000;

const CALL_WARMUP: usize = 200;
const CALL_ITERS: usize = 5_000;

// Function name must match the registered tool name (both runtimes call
// the function by name through their wrapper).
const JS_TOOL_SOURCE: &str =
    "function js_bench_tool(args) { var s = args.input || ''; return {result: s.toUpperCase(), len: s.length}; }";
const LUA_TOOL_SOURCE: &str = "function lua_bench_tool(args)\n  local s = args.input or ''\n  return {result = string.upper(s), len = #s}\nend";
const TOOL_ARGS: &str = r#"{"input":"hello wasm metering benchmark"}"#;
const EXPECTED_RESULT: &str = "HELLO WASM METERING BENCHMARK";

fn load_wasm(path: &str) -> Vec<u8> {
    std::fs::read(path)
        .unwrap_or_else(|e| panic!("WASM file not found: {} — {} (build bench_tools first)", path, e))
}

/// (mean ns, p50 ns, p99 ns)
fn percentiles(ns: &[f64]) -> (f64, f64, f64) {
    let mut s = ns.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).expect("no NaN durations"));
    let mean = s.iter().sum::<f64>() / s.len() as f64;
    let pick = |f: f64| s[((s.len() as f64 - 1.0) * f) as usize];
    (mean, pick(0.5), pick(0.99))
}

fn us(us_val: f64) -> String {
    format!("{:>10.1}", us_val)
}

// ─────────────────────────────────────────────────────────────────────────
// Part A: Module primitives
// ─────────────────────────────────────────────────────────────────────────

fn part_a_module_primitives(label: &str, wasm_bytes: &[u8], iters: usize) {
    println!("\n─── Part A: Module primitives ({}) ────────────────────", label);

    let t = Instant::now();
    for _ in 0..iters {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        black_box(wasmer::Module::new(&engine, wasm_bytes).expect("compile unmetered"));
    }
    let compile_unmetered_us = t.elapsed().as_secs_f64() * 1e6 / iters as f64;

    let t = Instant::now();
    for _ in 0..iters {
        let engine = build_metered_engine(Some(LIMIT));
        black_box(wasmer::Module::new(&engine, wasm_bytes).expect("compile metered"));
    }
    let compile_metered_us = t.elapsed().as_secs_f64() * 1e6 / iters as f64;

    // Deserialize path, engine rebuilt per iteration exactly as
    // create_wasm_instance does in production (artifact materialized to
    // Vec<u8> once, then handed over as &[u8] like MODULE_CACHE does).
    let artifact = {
        let engine = build_metered_engine(Some(LIMIT));
        let module = wasmer::Module::new(&engine, wasm_bytes).expect("compile for serialize");
        module.serialize().expect("serialize module").to_vec()
    };
    let t = Instant::now();
    for _ in 0..iters {
        let engine = build_metered_engine(Some(LIMIT));
        black_box(
            unsafe { wasmer::Module::deserialize(&engine, artifact.as_slice()) }
                .expect("deserialize cached module"),
        );
    }
    let deserialize_us = t.elapsed().as_secs_f64() * 1e6 / iters as f64;

    let metered_overhead_pct = (compile_metered_us / compile_unmetered_us - 1.0) * 100.0;
    println!("  compile (unmetered):   {} µs", us(compile_unmetered_us));
    println!(
        "  compile (metered):     {} µs  ({:+.1}% metering instrumentation)",
        us(compile_metered_us),
        metered_overhead_pct
    );
    println!(
        "  deserialize (cached):  {} µs  ({:.1}x faster than compile)",
        us(deserialize_us),
        compile_unmetered_us / deserialize_us
    );
}

// ─────────────────────────────────────────────────────────────────────────
// Part B: Runtime cold start (create + init), first vs cache-hit
// ─────────────────────────────────────────────────────────────────────────

fn cold_start(language: &str, path: &str) -> f64 {
    let t = Instant::now();
    let rt = create_wasm_runtime_for_language(language, path)
        .unwrap_or_else(|e| panic!("{} runtime creation failed: {}", language, e));
    black_box(&rt);
    drop(rt);
    t.elapsed().as_secs_f64() * 1e6
}

fn part_b_runtime_cold_start() {
    println!("\n─── Part B: Runtime cold start (create + init) ────────");
    // Unmetered so the cache comparison isolates compile-vs-deserialize.
    set_instruction_limit(None);

    // Lua: the only runtime wired through create_wasm_instance (module cache).
    let lua_first = cold_start("lua", LUA_WASM);
    let lua_second = cold_start("lua", LUA_WASM);
    let lua_third = cold_start("lua", LUA_WASM);
    let lua_cache_hit = lua_second.min(lua_third);

    // QuickJS: compiles directly via Module::new, module cache not consulted.
    let qjs_first = cold_start("javascript", QUICKJS_WASM);
    let qjs_second = cold_start("javascript", QUICKJS_WASM);

    println!("  Lua (cache-backed path):");
    println!("    first (compile):     {} µs", us(lua_first));
    println!(
        "    cache hit:           {} µs  ({:.1}x faster cold start)",
        us(lua_cache_hit),
        lua_first / lua_cache_hit
    );
    println!("  QuickJS (no cache path — both runs compile):");
    println!("    first:               {} µs", us(qjs_first));
    println!("    second:              {} µs", us(qjs_second));
}

// ─────────────────────────────────────────────────────────────────────────
// Part C: Per-call metering overhead through the real dispatch path
// ─────────────────────────────────────────────────────────────────────────

fn build_registry(limit: Option<u64>, js_bytes: &[u8], lua_bytes: &[u8]) -> WasmRuntimeRegistry {
    set_instruction_limit(limit);
    let mut registry = WasmRuntimeRegistry::new();
    let js_rt = QuickJsRuntime::new(js_bytes).expect("QuickJS runtime creation");
    registry
        .register_runtime(Box::new(js_rt))
        .expect("register QuickJS runtime");
    let lua_rt = LuaRuntime::new(lua_bytes).expect("Lua runtime creation");
    registry
        .register_runtime(Box::new(lua_rt))
        .expect("register Lua runtime");
    registry
        .register_tool(
            "javascript",
            "js_bench_tool",
            JS_TOOL_SOURCE,
            "benchmark tool",
            json!({}),
        )
        .expect("register JS tool");
    registry
        .register_tool(
            "lua",
            "lua_bench_tool",
            LUA_TOOL_SOURCE,
            "benchmark tool",
            json!({}),
        )
        .expect("register Lua tool");
    registry
}

fn verify_dispatch(registry: &mut WasmRuntimeRegistry, tool: &str, expect: &str) {
    let result = registry
        .call_tool(tool, TOOL_ARGS)
        .unwrap_or_else(|e| panic!("{} verification call failed: {}", tool, e));
    assert!(
        result.contains(expect),
        "{} returned unexpected result: {}",
        tool,
        result
    );
}

fn time_dispatch(registry: &mut WasmRuntimeRegistry, tool: &str) -> (f64, f64, f64) {
    for _ in 0..CALL_WARMUP {
        black_box(registry.call_tool(tool, TOOL_ARGS).expect("warmup call"));
    }
    let mut samples = Vec::with_capacity(CALL_ITERS);
    for _ in 0..CALL_ITERS {
        let t = Instant::now();
        let r = registry.call_tool(tool, TOOL_ARGS).expect("timed call");
        samples.push(t.elapsed().as_secs_f64() * 1e9);
        black_box(r);
    }
    percentiles(&samples)
}

fn part_c_metering_overhead(js_bytes: &[u8], lua_bytes: &[u8]) {
    println!("\n─── Part C: Per-call metering overhead (real dispatch) ─");
    println!(
        "  {} warmup + {} timed calls via WasmRuntimeRegistry::call_tool\n",
        CALL_WARMUP, CALL_ITERS
    );

    let mut metered = build_registry(Some(LIMIT), js_bytes, lua_bytes);
    verify_dispatch(&mut metered, "js_bench_tool", EXPECTED_RESULT);
    verify_dispatch(&mut metered, "lua_bench_tool", EXPECTED_RESULT);
    let (m_js, m_js_p50, m_js_p99) = time_dispatch(&mut metered, "js_bench_tool");
    let (m_lua, m_lua_p50, m_lua_p99) = time_dispatch(&mut metered, "lua_bench_tool");
    drop(metered);

    let mut unmetered = build_registry(None, js_bytes, lua_bytes);
    verify_dispatch(&mut unmetered, "js_bench_tool", EXPECTED_RESULT);
    verify_dispatch(&mut unmetered, "lua_bench_tool", EXPECTED_RESULT);
    let (u_js, u_js_p50, u_js_p99) = time_dispatch(&mut unmetered, "js_bench_tool");
    let (u_lua, u_lua_p50, u_lua_p99) = time_dispatch(&mut unmetered, "lua_bench_tool");
    drop(unmetered);

    set_instruction_limit(None);

    for (lang, m, m50, m99, u, u50, u99) in [
        ("QuickJS", m_js, m_js_p50, m_js_p99, u_js, u_js_p50, u_js_p99),
        ("Lua", m_lua, m_lua_p50, m_lua_p99, u_lua, u_lua_p50, u_lua_p99),
    ] {
        let delta_pct = (m / u - 1.0) * 100.0;
        println!("  {}:", lang);
        println!("    metered ({}):  mean {:>9.1} ns  p50 {:>9.1} ns  p99 {:>9.1} ns", LIMIT, m, m50, m99);
        println!("    unmetered:          mean {:>9.1} ns  p50 {:>9.1} ns  p99 {:>9.1} ns", u, u50, u99);
        println!("    overhead:           {:+.1}% mean\n", delta_pct);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Part D: E2E stdio sanity with the real server binary
// ─────────────────────────────────────────────────────────────────────────

struct ServerProcess {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl ServerProcess {
    fn spawn_with_env(extra_env: &[(&str, &str)]) -> Self {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_velocity_mcp"));
        cmd.args(["--mode", "stdio"])
            .env("RUST_LOG", "error")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in extra_env {
            cmd.env(key, value);
        }
        let mut child = cmd.spawn().expect("failed to spawn velocity_mcp binary");
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

    fn request(&mut self, mut request: serde_json::Value) -> serde_json::Value {
        self.next_id += 1;
        request["id"] = json!(self.next_id);
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        writeln!(stdin, "{}", serde_json::to_string(&request).unwrap())
            .expect("failed to write to stdin");
        stdin.flush().unwrap();

        let mut response_line = String::new();
        self.reader
            .read_line(&mut response_line)
            .expect("failed to read response");
        serde_json::from_str(&response_line).unwrap_or_else(|e| {
            panic!("response is not valid JSON: {} — got: {}", e, response_line)
        })
    }

    fn send_raw(&mut self, raw: &str) {
        let stdin = self.child.stdin.as_mut().expect("stdin pipe");
        writeln!(stdin, "{}", raw).expect("failed to write to stdin");
        stdin.flush().unwrap();
    }

    fn call_tool(&mut self, name: &str, arguments: serde_json::Value) -> serde_json::Value {
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
                name, result["content"][0]["text"]
            );
        }
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("tool '{}' returned no text content", name));
        serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("tool '{}' result not valid JSON ({}): {}", name, e, text))
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.child.kill().ok();
        let _ = self.child.wait();
    }
}

fn part_d_e2e_sanity() {
    println!("\n─── Part D: E2E stdio (real binary, full stack) ───────");
    const E2E_WARMUP: usize = 20;
    const E2E_ITERS: usize = 300;

    for (label, limit_env) in [("metered (limit 10M)", "10000000"), ("metering disabled (0)", "0")] {
        let mut server = ServerProcess::spawn_with_env(&[
            ("VELOCITY_WASM_INSTRUCTION_LIMIT", limit_env),
            ("VELOCITY_RATE_LIMIT", "1000000"),
            ("VELOCITY_RATE_BURST", "100000"),
        ]);

        let result = server.call_tool(
            "js_string_transform",
            json!({"action": "upper", "input": "hello"}),
        );
        assert_eq!(result["result"], "HELLO", "unexpected E2E result: {}", result);

        for _ in 0..E2E_WARMUP {
            black_box(server.call_tool(
                "js_string_transform",
                json!({"action": "upper", "input": "hello"}),
            ));
        }
        let mut samples = Vec::with_capacity(E2E_ITERS);
        for _ in 0..E2E_ITERS {
            let t = Instant::now();
            let r = server.call_tool(
                "js_string_transform",
                json!({"action": "upper", "input": "hello"}),
            );
            samples.push(t.elapsed().as_secs_f64() * 1e9);
            black_box(r);
        }
        let (mean, p50, p99) = percentiles(&samples);
        println!(
            "  {}:  mean {:>9.1} ns  p50 {:>9.1} ns  p99 {:>9.1} ns ({} calls)",
            label, mean, p50, p99, E2E_ITERS
        );
    }
}

fn main() {
    println!("================================================================");
    println!("   Metering & Module-Cache Benchmark");
    println!("================================================================");

    let js_bytes = load_wasm(QUICKJS_WASM);
    let lua_bytes = load_wasm(LUA_WASM);

    part_a_module_primitives("quickjs.wasm", &js_bytes, 5);
    part_a_module_primitives("lua.wasm", &lua_bytes, 10);
    part_b_runtime_cold_start();
    part_c_metering_overhead(&js_bytes, &lua_bytes);
    part_d_e2e_sanity();

    println!("\n================================================================");
    println!("                      Benchmark Complete");
    println!("================================================================");
}
