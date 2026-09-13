use std::time::Instant;
use velocity_mcp::wasm_runtime::{micropython::MicroPythonRuntime, WasmRuntime};

fn main() {
    let wasm = std::fs::read("bench_tools/micropython_wasm/micropython-1.24.1/ports/webassembly/build-wasi/micropython.wasm")
        .expect("micropython.wasm not found");
    let mut rt = MicroPythonRuntime::cold_start(&wasm).expect("cold start failed");

    // Tool that does actual computation
    let source = r#"def compute_tool(args):
    n = args.get('n', 10)
    total = 0
    for i in range(n):
        total += i * i
    return {'sum': total, 'n': n}
"#;

    rt.register_tool("compute_tool", source)
        .expect("register failed");

    // Verify correctness
    let result = rt
        .call_tool("compute_tool", r#"{"n": 100}"#)
        .expect("call failed");
    println!("Result for n=100: {}", result);
    assert!(
        result.contains("328350"),
        "Expected sum=328350 for n=100, got: {}",
        result
    );

    let result2 = rt
        .call_tool("compute_tool", r#"{"n": 1000}"#)
        .expect("call 2 failed");
    println!("Result for n=1000: {}", result2);
    assert!(
        result2.contains("332833500"),
        "Expected sum=332833500 for n=1000, got: {}",
        result2
    );

    // Benchmark with actual work
    let iters = 100;
    let start = Instant::now();
    for _ in 0..iters {
        let _ = rt
            .call_tool("compute_tool", r#"{"n": 100}"#)
            .expect("call failed");
    }
    let elapsed = start.elapsed();
    let per_call_us = elapsed.as_nanos() as f64 / iters as f64 / 1000.0;

    println!("\nBenchmark with actual computation (n=100):");
    println!("  {} calls in {:?}", iters, elapsed);
    println!("  {:.2} µs/call", per_call_us);

    // Compare with trivial tool
    let trivial_source = "def trivial_tool(args):\n    return {'result': 42}\n";
    rt.register_tool("trivial_tool", trivial_source)
        .expect("register trivial failed");

    let start2 = Instant::now();
    for _ in 0..iters {
        let _ = rt
            .call_tool("trivial_tool", r#"{}"#)
            .expect("trivial call failed");
    }
    let elapsed2 = start2.elapsed();
    let per_call_trivial_us = elapsed2.as_nanos() as f64 / iters as f64 / 1000.0;

    println!("\nBenchmark with trivial tool:");
    println!("  {} calls in {:?}", iters, elapsed2);
    println!("  {:.2} µs/call", per_call_trivial_us);

    println!(
        "\nOverhead of computation: {:.2} µs",
        per_call_us - per_call_trivial_us
    );

    rt.destroy().unwrap();
    println!("\nVerification PASSED - tools are executing code correctly");
}
