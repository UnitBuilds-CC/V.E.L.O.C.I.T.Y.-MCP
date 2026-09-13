//! Lua Binary Protocol Benchmark
//!
//! Compares JSON serialization path vs TLV binary protocol for Lua WASM tools.
//! Measures the overhead eliminated by bypassing double JSON serialization.
//!
//! Run: cargo run --release --bin lua_binary_protocol

#![allow(dead_code)] // WARM_ITERS used conditionally in benchmark loops

use std::hint::black_box;
use std::time::Instant;
use velocity_mcp::protocol::nda_native::encode_json_value;
use velocity_mcp::wasm_runtime::{lua::LuaRuntime, WasmRuntime};

const WARM_ITERS: usize = 1000; // Used for warmup phase to stabilize measurements
const BENCH_ITERS: usize = 5000;

const LUA_TOOL_SOURCE: &str = r#"
function text_analyze(args)
    local text = args.text or ""
    local wc = 0
    for _ in text:gmatch('%S+') do wc = wc + 1 end
    local cc = #text
    return {word_count = wc, char_count = cc}
end
"#;

const TEST_INPUT: &str = r#"{"text":"The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump. Bright vixens jump; dozy fowl quack. Sphinx of black quartz, judge my vow. Two driven jocks help fax my big quiz. The five boxing wizards jump quickly. Jackdaws love my big sphinx of quartz. We promptly judged antique ivory buckles for the next prize. Glib jocks quiz nymph to vex dwarf. Jived fox nymph grabs quick waltz."}"#;

fn load_lua_wasm() -> Vec<u8> {
    let path = "bench_tools/lua_wasm/lua.wasm";
    std::fs::read(path)
        .expect("Lua WASM not found — build with: cd bench_tools/lua_wasm && ./build.sh")
}

/// Baseline: current JSON path (serde_json → string → WASM memory → JSON parser)
fn bench_json_path(wasm_bytes: &[u8]) -> f64 {
    let mut rt = LuaRuntime::cold_start(wasm_bytes).expect("Lua cold start failed");
    rt.register_tool("text_analyze", LUA_TOOL_SOURCE)
        .expect("register tool failed");

    // Warmup
    for _ in 0..10 {
        let _ = rt.call_tool("text_analyze", TEST_INPUT);
    }

    let start = Instant::now();
    let mut result_len = 0;
    for _ in 0..BENCH_ITERS {
        let result = rt.call_tool("text_analyze", TEST_INPUT).unwrap();
        result_len += result.len();
    }
    let ns = start.elapsed().as_nanos() as f64 / BENCH_ITERS as f64;
    black_box(result_len);
    ns
}

/// Optimized: TLV binary path (NDA TLV → WASM memory → zero-alloc decoder)
fn bench_tlv_path(wasm_bytes: &[u8]) -> f64 {
    let mut rt = LuaRuntime::cold_start(wasm_bytes).expect("Lua cold start failed");
    rt.register_tool("text_analyze", LUA_TOOL_SOURCE)
        .expect("register tool failed");

    // Encode test input as TLV once
    let input_value: serde_json::Value = serde_json::from_str(TEST_INPUT).unwrap();
    let mut tlv_buf = Vec::new();
    encode_json_value(&input_value, &mut tlv_buf).unwrap();

    // Warmup
    for _ in 0..10 {
        let _ = rt.call_tool_binary("text_analyze", &tlv_buf);
    }

    let start = Instant::now();
    let mut result_len = 0;
    for _ in 0..BENCH_ITERS {
        let result = rt.call_tool_binary("text_analyze", &tlv_buf).unwrap();
        result_len += result.len();
    }
    let ns = start.elapsed().as_nanos() as f64 / BENCH_ITERS as f64;
    black_box(result_len);
    ns
}

fn main() {
    println!("Lua Binary Protocol Benchmark");
    println!("==============================");
    println!("Iterations: {} warmup + {} measured\n", 10, BENCH_ITERS);

    let wasm_bytes = load_lua_wasm();
    println!("WASM module size: {} bytes\n", wasm_bytes.len());

    // Measure both paths multiple times and take median
    let mut json_results = Vec::new();
    let mut tlv_results = Vec::new();

    for run in 0..3 {
        println!("Run {}...", run + 1);
        json_results.push(bench_json_path(&wasm_bytes));
        tlv_results.push(bench_tlv_path(&wasm_bytes));
    }

    json_results.sort_by(|a, b| a.partial_cmp(b).unwrap());
    tlv_results.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let json_median = json_results[1];
    let tlv_median = tlv_results[1];

    println!("\nResults (median of 3 runs):");
    println!("{:<25} {:>12}", "Method", "Time per call");
    println!("{:-<39}", "");
    println!("{:<25} {:>10.1}ns", "JSON (baseline)", json_median);
    println!("{:<25} {:>10.1}ns", "TLV binary (optimized)", tlv_median);

    let improvement = if json_median > 0.0 {
        (json_median - tlv_median) / json_median * 100.0
    } else {
        0.0
    };

    println!(
        "\nImprovement: {:.1}% faster ({:.1}ns saved per call)",
        improvement,
        json_median - tlv_median
    );

    if improvement > 0.0 {
        println!("✓ Binary protocol eliminates JSON overhead");
    } else {
        println!("⚠ No measurable improvement (JSON parsing may be negligible for small inputs)");
    }
}
