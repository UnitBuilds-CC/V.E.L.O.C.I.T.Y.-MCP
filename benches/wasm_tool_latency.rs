//! WASM tool call latency benchmark
//! Measures first-call (cold), second-call (warm), and P99 over 100 calls
//! Compares native tools vs WASM tools

use std::time::Instant;
use velocity_mcp::wasm_runtime::{lua::LuaRuntime, micropython::MicroPythonRuntime, quickjs::QuickJsRuntime, WasmRuntime};

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = (p * sorted.len() as f64) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn bench_runtime<R: WasmRuntime>(
    name: &str,
    rt: &mut R,
    tool_source: &str,
    args_json: &str,
) {
    println!("\n=== {} ===", name);

    // Register tool
    let reg_start = Instant::now();
    rt.register_tool("bench_tool", tool_source).expect("register failed");
    let reg_us = reg_start.elapsed().as_nanos() as f64 / 1000.0;
    println!("Register: {:.1} µs", reg_us);

    // First call (cold - wrapper just compiled)
    let call1_start = Instant::now();
    let result1 = rt.call_tool("bench_tool", args_json).expect("call 1 failed");
    let call1_us = call1_start.elapsed().as_nanos() as f64 / 1000.0;
    println!("Call 1 (cold): {:.1} µs → {} bytes", call1_us, result1.len());

    // Second call (warm - wrapper cached)
    let call2_start = Instant::now();
    let result2 = rt.call_tool("bench_tool", args_json).expect("call 2 failed");
    let call2_us = call2_start.elapsed().as_nanos() as f64 / 1000.0;
    println!("Call 2 (warm): {:.1} µs → {} bytes", call2_us, result2.len());

    // 100 calls for P99
    let mut latencies = Vec::with_capacity(100);
    let bench_start = Instant::now();
    for _ in 0..100 {
        let start = Instant::now();
        let _ = rt.call_tool("bench_tool", args_json).expect("call failed");
        latencies.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }
    let total_us = bench_start.elapsed().as_nanos() as f64 / 1000.0;
    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min = latencies[0];
    let max = latencies[99];
    let p50 = percentile(&latencies, 0.50);
    let p95 = percentile(&latencies, 0.95);
    let p99 = percentile(&latencies, 0.99);
    let avg = total_us / 100.0;

    println!("100 calls:");
    println!("  min:  {:.1} µs", min);
    println!("  avg:  {:.1} µs", avg);
    println!("  p50:  {:.1} µs", p50);
    println!("  p95:  {:.1} µs", p95);
    println!("  p99:  {:.1} µs", p99);
    println!("  max:  {:.1} µs", max);

    rt.destroy().ok();
}

fn main() {
    println!("WASM Tool Call Latency Benchmark");
    println!("=================================");

    // Lua
    let lua_wasm = std::fs::read("bench_tools/lua_wasm/lua.wasm").expect("lua.wasm not found");
    let mut lua_rt = LuaRuntime::new(&lua_wasm).expect("LuaRuntime::new");
    lua_rt.init().expect("init failed");
    let lua_source = "function bench_tool(args)\n    return {size = 64, payload = 'hello'}\nend\n";
    bench_runtime("Lua/WASM", &mut lua_rt, lua_source, r#"{"text": "hello"}"#);

    // MicroPython
    let mp_wasm = std::fs::read("bench_tools/micropython_wasm/micropython-1.24.1/ports/webassembly/build-wasi/micropython.wasm")
        .expect("micropython.wasm not found");
    let mut mp_rt = MicroPythonRuntime::new(&mp_wasm).expect("MicroPythonRuntime::new");
    mp_rt.init().expect("init failed");
    let mp_source = "def bench_tool(args):\n    return {'size': 64, 'payload': 'hello'}\n";
    bench_runtime("MicroPython/WASM", &mut mp_rt, mp_source, r#"{"text": "hello"}"#);

    // QuickJS
    let qjs_wasm = std::fs::read("bench_tools/quickjs_wasm/quickjs.wasm").expect("quickjs.wasm not found");
    let mut qjs_rt = QuickJsRuntime::new(&qjs_wasm).expect("QuickJsRuntime::new");
    qjs_rt.init().expect("init failed");
    let qjs_source = r#"function bench_tool(args) { return {size: 64, payload: "hello"}; }"#;
    bench_runtime("QuickJS/WASM", &mut qjs_rt, qjs_source, r#"{"text": "hello"}"#);

    println!("\n=================================");
    println!("Native tool baselines (from benchmark.rs):");
    println!("  Rust native:  ~950 ns/call");
    println!("  Python (CPython subprocess): ~50-100 ms/call");
    println!("  Lua (native): ~1-2 µs/call");
}
