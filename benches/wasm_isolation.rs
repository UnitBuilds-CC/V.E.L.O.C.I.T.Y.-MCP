//! WASM Runtime Isolation Benchmark
//!
//! Measures Wasmer vs Wasmtime across four dimensions to identify where
//! the performance delta originates:
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

// ─── Wasmer ──────────────────────────────────────────────────────────────────

fn bench_wasmer_memory(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports!{}).unwrap();
    let memory = instance.exports.get_memory("memory").unwrap();

    let input = make_input();
    let mut buf = vec![0u8; 256];

    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        memory.view(&store).write(INPUT_PTR as u64, &input).unwrap();
        memory.view(&store).read(INPUT_PTR as u64, &mut buf).unwrap();
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(buf);
    ns
}

fn bench_wasmer_trampoline(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports!{}).unwrap();
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

fn bench_wasmer_execute(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports!{}).unwrap();
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

fn bench_wasmer_combined(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = wasmer::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmer::Store::new(engine);
    let instance = wasmer::Instance::new(&mut store, &module, &wasmer::imports!{}).unwrap();
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

// ─── Wasmtime ────────────────────────────────────────────────────────────────

fn bench_wasmtime_memory(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();

    let input = make_input();
    let mut buf = vec![0u8; 256];

    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        memory.write(&mut store, INPUT_PTR, &input).unwrap();
        memory.read(&store, INPUT_PTR, &mut buf).unwrap();
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(buf);
    ns
}

fn bench_wasmtime_trampoline(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let prepare_fn = instance
        .get_typed_func::<(), ()>(&mut store, "prepare_call")
        .unwrap();

    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        prepare_fn.call(&mut store, ()).unwrap();
    }
    start.elapsed().as_nanos() as f64 / WARM_ITERS as f64
}

fn bench_wasmtime_execute(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let prepare_fn = instance
        .get_typed_func::<(), ()>(&mut store, "prepare_call")
        .unwrap();
    let execute_fn = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "tool_execute")
        .unwrap();

    let input = make_input();
    let input_ptr = INPUT_PTR as i32;
    let input_len = input.len() as i32;
    memory.write(&mut store, INPUT_PTR, &input).unwrap();

    let mut checksum: u32 = 0;
    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        prepare_fn.call(&mut store, ()).unwrap();
        let r = execute_fn.call(&mut store, (input_ptr, input_len)).unwrap();
        checksum = checksum.wrapping_add(r as u32);
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(checksum);
    ns
}

fn bench_wasmtime_combined(wasm_bytes: &[u8]) -> f64 {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, wasm_bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let memory = instance.get_memory(&mut store, "memory").unwrap();
    let prepare_fn = instance
        .get_typed_func::<(), ()>(&mut store, "prepare_call")
        .unwrap();
    let execute_fn = instance
        .get_typed_func::<(i32, i32), i64>(&mut store, "tool_execute")
        .unwrap();

    let input = make_input();
    let input_ptr = INPUT_PTR as i32;
    let input_len = input.len() as i32;

    let mut checksum: u32 = 0;
    let start = Instant::now();
    for _ in 0..WARM_ITERS {
        prepare_fn.call(&mut store, ()).unwrap();
        memory.write(&mut store, INPUT_PTR, &input).unwrap();
        let r = execute_fn.call(&mut store, (input_ptr, input_len)).unwrap();
        let rp = (r >> 32) as u32;
        let rl = (r & 0xFFFF_FFFF) as u32;
        let mut buf = vec![0u8; rl as usize];
        memory.read(&store, rp as usize, &mut buf).unwrap();
        checksum = checksum.wrapping_add(buf.len() as u32);
    }
    let ns = start.elapsed().as_nanos() as f64 / WARM_ITERS as f64;
    black_box(checksum);
    ns
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let wasm_bytes = load_wasm();
    println!("WASM Runtime Isolation Benchmark");
    println!("=================================");
    println!("Module size: {} bytes", wasm_bytes.len());
    println!("Iterations:  {}\n", WARM_ITERS);

    // Run each test 3x and take the median to reduce noise
    let tests: &[(&str, fn(&[u8]) -> f64, fn(&[u8]) -> f64)] = &[
        ("Memory (write+read, no calls)", bench_wasmer_memory, bench_wasmtime_memory),
        ("Trampoline (prepare_call only)", bench_wasmer_trampoline, bench_wasmtime_trampoline),
        ("Execute (prepare+call, no read)", bench_wasmer_execute, bench_wasmtime_execute),
        ("Combined (full pipeline)", bench_wasmer_combined, bench_wasmtime_combined),
    ];

    println!(
        "{:<40} {:>10} {:>10} {:>8} {:>8}",
        "Test", "Wasmer", "Wasmtime", "Delta", "Ratio"
    );
    println!("{:-<86}", "");

    for (name, wasmer_fn, wasmtime_fn) in tests {
        let mut wasmer_results = [0.0f64; 3];
        let mut wasmtime_results = [0.0f64; 3];

        for i in 0..3 {
            wasmer_results[i] = wasmer_fn(&wasm_bytes);
            wasmtime_results[i] = wasmtime_fn(&wasm_bytes);
        }
        wasmer_results.sort_by(|a, b| a.partial_cmp(b).unwrap());
        wasmtime_results.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let w = wasmer_results[1];
        let t = wasmtime_results[1];
        let delta = w - t;
        let ratio = w / t;

        println!(
            "{:<40} {:>8.1}ns {:>8.1}ns {:>+7.1}ns {:>7.2}x",
            name, w, t, delta, ratio
        );
    }

    println!("\nKey: Delta > 0 means Wasmer is slower. Ratio > 1.0 means Wasmer is slower.");
}
