//! WASM Runtime Isolation Benchmark
//!
//! Measures Wasmer performance across four dimensions:
//!
//!   1. Memory-only:    write + read linear memory, no function calls
//!   2. Trampoline-only: call prepare_call() (atomic store), no memory access
//!   3. Execute-only:   call tool_execute() with data pre-loaded in memory
//!   4. Combined:       full pipeline (prepare + write + execute + read)
//!
//! Run: cargo bench --bench wasm_isolation

use std::hint::black_box;
use std::time::Instant;

const WARM_ITERS: u64 = 100_000;
const INPUT_PTR: usize = 1024;

fn load_wasm() -> Vec<u8> {
    let path = "bench_tools/wasm_tool/target/wasm32-unknown-unknown/release/wasm_text_tool.wasm";
    std::fs::read(path).expect("WASM file not found — build with: cd bench_tools/wasm_tool && cargo build --target wasm32-unknown-unknown --release")
}

fn make_input() -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "text": "The quick brown fox jumps over the lazy dog. \
                 Pack my box with five dozen liquor jugs. \
                 How vexingly quick daft zebras jump. \
                 Bright vixens jump; dozy fowl quack."
    }))
    .unwrap()
}

fn bench_memory(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports! {}).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();

    let input = make_input();
    let mut buf = vec![0u8; 256];

    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        memory.view(&store).write(INPUT_PTR as u64, &input).unwrap();
        memory
            .view(&store)
            .read(INPUT_PTR as u64, &mut buf)
            .unwrap();
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(buf);
    ns
}

fn bench_trampoline(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports! {}).unwrap();
    let prepare_fn = instance
        .exports
        .get_function("prepare_call")
        .unwrap()
        .typed::<(), ()>(&store)
        .unwrap();

    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        prepare_fn.call(&mut store).unwrap();
    }
    start.elapsed().as_nanos() as f64 / WARM_ITERS as f64
}

fn bench_execute(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports! {}).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();
    let prepare_fn = instance
        .exports
        .get_function("prepare_call")
        .unwrap()
        .typed::<(), ()>(&store)
        .unwrap();
    let execute_fn = instance
        .exports
        .get_function("tool_execute")
        .unwrap()
        .typed::<(i32, i32), i64>(&store)
        .unwrap();

    let input = make_input();
    let input_ptr = INPUT_PTR as i32;
    let input_len = input.len() as i32;
    memory.view(&store).write(INPUT_PTR as u64, &input).unwrap();

    let mut checksum: u32 = 0;
    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        prepare_fn.call(&mut store).unwrap();
        let r = execute_fn.call(&mut store, input_ptr, input_len).unwrap();
        checksum = checksum.wrapping_add(r as u32);
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(checksum);
    ns
}

fn bench_combined(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports! {}).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();
    let prepare_fn = instance
        .exports
        .get_function("prepare_call")
        .unwrap()
        .typed::<(), ()>(&store)
        .unwrap();
    let execute_fn = instance
        .exports
        .get_function("tool_execute")
        .unwrap()
        .typed::<(i32, i32), i64>(&store)
        .unwrap();

    let input = make_input();
    let input_ptr = INPUT_PTR as i32;
    let input_len = input.len() as i32;

    let mut checksum: u32 = 0;
    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        prepare_fn.call(&mut store).unwrap();
        memory.view(&store).write(INPUT_PTR as u64, &input).unwrap();
        let r = execute_fn.call(&mut store, input_ptr, input_len).unwrap();
        let rp = (r >> 32) as u32;
        let rl = (r & 0xFFFF_FFFF) as u32;
        let mut buf = vec![0u8; rl as usize];
        memory.view(&store).read(rp as u64, &mut buf).unwrap();
        checksum = checksum.wrapping_add(buf.len() as u32);
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(checksum);
    ns
}

fn main() {
    let wasm_bytes = load_wasm();
    println!("WASM Runtime Isolation Benchmark (Wasmer)");
    println!("==========================================");
    println!("Module size: {} bytes", wasm_bytes.len());
    println!("Iterations:  {}\n", WARM_ITERS);

    let tests: &[(&str, fn(&[u8]) -> f64)] = &[
        ("Memory (write+read, no calls)", bench_memory),
        ("Trampoline (prepare_call only)", bench_trampoline),
        ("Execute (prepare+call, no read)", bench_execute),
        ("Combined (full pipeline)", bench_combined),
    ];

    println!("{:<40} {:>12}", "Test", "Wasmer (ns)");
    println!("{:-<54}", "");

    for (name, bench_fn) in tests {
        let mut results = [0.0f64; 3];
        for i in 0..3 {
            results[i] = bench_fn(&wasm_bytes);
        }
        results.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = results[1];
        println!("{:<40} {:>10.1}ns", name, median);
    }
}
