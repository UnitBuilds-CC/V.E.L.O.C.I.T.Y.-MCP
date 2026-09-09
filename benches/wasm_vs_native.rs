//! WASM vs Native per-language tool execution benchmark.
//!
//! Compares the same `text_analyze` tool running through:
//!   - WASM path: interpreter compiled to WASM, running in-process via Wasmer
//!   - Native path: standard interpreter/runtime as a persistent stdio process
//!
//! Run with: cargo bench --bench wasm_vs_native

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

use velocity_mcp::wasm_runtime::quickjs::QuickJsRuntime;
use velocity_mcp::wasm_runtime::micropython::MicroPythonRuntime;
use velocity_mcp::wasm_runtime::lua::LuaRuntime;
use velocity_mcp::wasm_runtime::WasmRuntime;

const BENCH_INPUT: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump. \
    Bright vixens jump; dozy fowl quack.";

const BENCH_INPUT_JSON: &str = r#"{"text":"The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump. Bright vixens jump; dozy fowl quack."}"#;

const COLD_ITERS: usize = 20;
const HOT_NATIVE_ITERS: usize = 500;
const WARMUP_CALLS: usize = 10;
const RUNS: usize = 3;

// ── Tool sources for WASM runtimes ──

const JS_TOOL_SOURCE: &str = r#"
function text_analyze(args) {
    var text = args.text || '';
    var words = text.split(/\s+/).filter(function(w) { return w.length > 0; });
    return {
        word_count: words.length,
        char_count: text.length,
        line_count: text.length === 0 ? 0 : text.split('\n').length
    };
}"#;

const PY_TOOL_SOURCE: &str = r#"
def text_analyze(args):
    text = args.get('text', '')
    words = text.split()
    return {
        'word_count': len(words),
        'char_count': len(text),
        'line_count': len(text.split('\n')) if text else 0
    }
"#;

const LUA_TOOL_SOURCE: &str = r#"
function text_analyze(args)
    local text = args.text or ''
    local wc = 0
    for _ in text:gmatch('%S+') do wc = wc + 1 end
    local cc = #text
    local lc = 0
    if #text > 0 then
        for _ in text:gmatch('\n') do lc = lc + 1 end
        lc = lc + 1
    end
    return {word_count = wc, char_count = cc, line_count = lc}
end
"#;

// ── Native server request ──

fn make_request(id: u64) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","method":"tools/call","id":{},"params":{{"name":"text_analyze","arguments":{{"text":"{}"}}}}}}"#,
        id,
        BENCH_INPUT.replace('"', "\\\"")
    )
}

// ── Measurement helpers ──

fn median_of(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn find_command(candidates: &[&str]) -> Option<String> {
    for cmd in candidates {
        if Command::new(cmd).arg("--version").output().is_ok() {
            return Some(cmd.to_string());
        }
    }
    None
}

// ── WASM cold start ──

fn wasm_cold_start_quickjs(wasm_bytes: &[u8]) -> f64 {
    QuickJsRuntime::bench_cold_start(wasm_bytes)
}

fn wasm_cold_start_micropython(wasm_bytes: &[u8]) -> f64 {
    MicroPythonRuntime::bench_cold_start(wasm_bytes)
}

fn wasm_cold_start_lua(wasm_bytes: &[u8]) -> f64 {
    LuaRuntime::bench_cold_start(wasm_bytes)
}

// ── WASM hot call (via call_tool) ──

fn wasm_hot_call_quickjs(wasm_bytes: &[u8]) -> Option<f64> {
    let mut rt = QuickJsRuntime::cold_start(wasm_bytes).ok()?;
    rt.register_tool("text_analyze", JS_TOOL_SOURCE).ok()?;

    // warmup
    for _ in 0..WARMUP_CALLS {
        let _ = rt.call_tool("text_analyze", BENCH_INPUT_JSON);
    }

    let iters = 1000;
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iters {
        if let Ok(result) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
            checksum = checksum.wrapping_add(result.len() as u32);
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    std::hint::black_box(checksum);
    Some(ns)
}

fn wasm_hot_call_micropython(wasm_bytes: &[u8]) -> Option<f64> {
    let mut rt = MicroPythonRuntime::cold_start(wasm_bytes).ok()?;
    rt.register_tool("text_analyze", PY_TOOL_SOURCE).ok()?;

    // warmup
    for _ in 0..WARMUP_CALLS {
        let _ = rt.call_tool("text_analyze", BENCH_INPUT_JSON);
    }

    // measurement - use call_tool() like Lua/QuickJS for consistency
    let iters = 500;
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iters {
        if let Ok(result) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
            checksum = checksum.wrapping_add(result.len() as u32);
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    std::hint::black_box(checksum);
    Some(ns)
}

fn wasm_hot_call_lua(wasm_bytes: &[u8]) -> Option<f64> {
    let mut rt = LuaRuntime::cold_start(wasm_bytes).ok()?;
    rt.register_tool("text_analyze", LUA_TOOL_SOURCE).ok()?;

    let lua_code = r#"
local text = "The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump. Bright vixens jump; dozy fowl quack."
local wc = 0
for _ in text:gmatch('%S+') do wc = wc + 1 end
local cc = #text
local lc = 0
if #text > 0 then
    for _ in text:gmatch('\n') do lc = lc + 1 end
    lc = lc + 1
end
print(json_encode({word_count = wc, char_count = cc, line_count = lc}))
"#;

    // warmup
    for _ in 0..5 {
        let _ = rt.call_tool("text_analyze", BENCH_INPUT_JSON);
    }

    let (ns, _checksum) = rt.bench_exec_repeated(lua_code, 1000);
    Some(ns)
}

// ── Native cold start (spawn-per-call) ──

fn native_cold_start(command: &str, args: &[&str], request: &str) -> Option<f64> {
    let mut times = Vec::with_capacity(COLD_ITERS);
    for _ in 0..COLD_ITERS {
        let start = Instant::now();
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", request).as_bytes()).ok()?;
            stdin.flush().ok()?;
        }
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .ok()?;
        let _ = child.wait();
        times.push(start.elapsed().as_nanos() as f64 / 1_000_000.0);
    }
    let mut arr = times;
    Some(median_of(&mut arr))
}

// ── Native hot call (persistent process) ──

fn native_hot_call(command: &str, args: &[&str]) -> Option<f64> {
    let mut child = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // warmup
    for i in 0..WARMUP_CALLS {
        let req = make_request(i as u64);
        writeln!(stdin, "{}", req).ok()?;
        stdin.flush().ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
    }

    // measurement
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for i in 0..HOT_NATIVE_ITERS {
        let req = make_request(i as u64);
        writeln!(stdin, "{}", req).ok()?;
        stdin.flush().ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        checksum = checksum.wrapping_add(line.len() as u32);
        line.clear();
    }
    let ns = start.elapsed().as_nanos() as f64 / HOT_NATIVE_ITERS as f64;
    std::hint::black_box(checksum);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    Some(ns)
}

// ── Go WASM (standalone, per-call instantiation — production path) ──

fn go_wasm_per_call(wasm_bytes: &[u8]) -> Option<f64> {
    use wasmer::{FunctionEnv, Instance, Module, Store, Value as WasmValue};
    use velocity_mcp::wasm_runtime::wasi::{WasiEnv, build_wasi_imports};

    let iters = 10;

    // warmup (3 calls — each compile is ~400ms)
    for _ in 0..3 {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes).ok()?;
        let mut store = Store::new(engine);
        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let wasi_imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &wasi_imports).ok()?;
        let memory = instance.exports.get_memory("memory").ok()?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());
        if let Ok(start_fn) = instance.exports.get_function("_start") {
            let _ = start_fn.call(&mut store, &[]);
        }
        let prepare = instance.exports.get_function("prepare_call").ok()?;
        prepare.call(&mut store, &[]).ok()?;
        let malloc = instance.exports.get_function("malloc").ok()?;
        let args_bytes = BENCH_INPUT_JSON.as_bytes();
        let ptr_val = malloc.call(&mut store, &[WasmValue::I32(args_bytes.len() as i32)]).ok()?;
        let input_ptr = ptr_val[0].unwrap_i32();
        memory.view(&store).write(input_ptr as u64, args_bytes).ok()?;
        let execute = instance.exports.get_function("tool_execute").ok()?;
        let _ = execute.call(&mut store, &[
            WasmValue::I32(input_ptr),
            WasmValue::I32(args_bytes.len() as i32),
        ]).ok()?;
    }

    // measurement
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iters {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes).ok()?;
        let mut store = Store::new(engine);
        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let wasi_imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &wasi_imports).ok()?;
        let memory = instance.exports.get_memory("memory").ok()?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());
        if let Ok(start_fn) = instance.exports.get_function("_start") {
            let _ = start_fn.call(&mut store, &[]);
        }
        let prepare = instance.exports.get_function("prepare_call").ok()?;
        prepare.call(&mut store, &[]).ok()?;
        let malloc = instance.exports.get_function("malloc").ok()?;
        let args_bytes = BENCH_INPUT_JSON.as_bytes();
        let ptr_val = malloc.call(&mut store, &[WasmValue::I32(args_bytes.len() as i32)]).ok()?;
        let input_ptr = ptr_val[0].unwrap_i32();
        memory.view(&store).write(input_ptr as u64, args_bytes).ok()?;
        let execute = instance.exports.get_function("tool_execute").ok()?;
        if let Ok(result) = execute.call(&mut store, &[
            WasmValue::I32(input_ptr),
            WasmValue::I32(args_bytes.len() as i32),
        ]) {
            let encoded = result[0].unwrap_i64();
            let _result_ptr = (encoded >> 32) as u32;
            let result_len = (encoded & 0xFFFF_FFFF) as u32;
            checksum = checksum.wrapping_add(result_len);
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    std::hint::black_box(checksum);
    Some(ns)
}

// ── Go WASM (cached module — compile once, reuse for each call) ──

fn go_wasm_cached(wasm_bytes: &[u8]) -> Option<f64> {
    use wasmer::{FunctionEnv, Instance, Module, Store, Value as WasmValue};
    use velocity_mcp::wasm_runtime::wasi::{WasiEnv, build_wasi_imports};

    let iters = 1000;

    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = Module::new(&engine, wasm_bytes).ok()?;

    // warmup
    for _ in 0..10 {
        let mut store = Store::new(engine.clone());
        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let wasi_imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &wasi_imports).ok()?;
        let memory = instance.exports.get_memory("memory").ok()?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());
        if let Ok(start_fn) = instance.exports.get_function("_start") {
            let _ = start_fn.call(&mut store, &[]);
        }
        let prepare = instance.exports.get_function("prepare_call").ok()?;
        prepare.call(&mut store, &[]).ok()?;
        let malloc = instance.exports.get_function("malloc").ok()?;
        let args_bytes = BENCH_INPUT_JSON.as_bytes();
        let ptr_val = malloc.call(&mut store, &[WasmValue::I32(args_bytes.len() as i32)]).ok()?;
        let input_ptr = ptr_val[0].unwrap_i32();
        memory.view(&store).write(input_ptr as u64, args_bytes).ok()?;
        let execute = instance.exports.get_function("tool_execute").ok()?;
        let _ = execute.call(&mut store, &[
            WasmValue::I32(input_ptr),
            WasmValue::I32(args_bytes.len() as i32),
        ]).ok()?;
    }

    // measurement
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iters {
        let mut store = Store::new(engine.clone());
        let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
        let wasi_imports = build_wasi_imports(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &wasi_imports).ok()?;
        let memory = instance.exports.get_memory("memory").ok()?.clone();
        env.as_mut(&mut store).memory = Some(memory.clone());
        if let Ok(start_fn) = instance.exports.get_function("_start") {
            let _ = start_fn.call(&mut store, &[]);
        }
        let prepare = instance.exports.get_function("prepare_call").ok()?;
        prepare.call(&mut store, &[]).ok()?;
        let malloc = instance.exports.get_function("malloc").ok()?;
        let args_bytes = BENCH_INPUT_JSON.as_bytes();
        let ptr_val = malloc.call(&mut store, &[WasmValue::I32(args_bytes.len() as i32)]).ok()?;
        let input_ptr = ptr_val[0].unwrap_i32();
        memory.view(&store).write(input_ptr as u64, args_bytes).ok()?;
        let execute = instance.exports.get_function("tool_execute").ok()?;
        if let Ok(result) = execute.call(&mut store, &[
            WasmValue::I32(input_ptr),
            WasmValue::I32(args_bytes.len() as i32),
        ]) {
            let encoded = result[0].unwrap_i64();
            let _result_ptr = (encoded >> 32) as u32;
            let result_len = (encoded & 0xFFFF_FFFF) as u32;
            checksum = checksum.wrapping_add(result_len);
        }
    }
    let ns = start.elapsed().as_nanos() as f64 / iters as f64;
    std::hint::black_box(checksum);
    Some(ns)
}

// ── Correctness verification ──

fn verify_wasm_result(result: &str, lang: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(result) {
        let wc = v["word_count"].as_u64().unwrap_or(0);
        let cc = v["char_count"].as_u64().unwrap_or(0);
        if wc == 29 && cc == 159 {
            return true;
        }
        eprintln!("  WARNING: {} WASM returned unexpected result: wc={} cc={}", lang, wc, cc);
    } else {
        eprintln!("  WARNING: {} WASM result not valid JSON: {}", lang, &result[..result.len().min(80)]);
    }
    false
}

fn verify_native_result(line: &str, lang: &str) -> bool {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
        let wc = v["result"]["word_count"].as_u64().unwrap_or(0);
        let cc = v["result"]["char_count"].as_u64().unwrap_or(0);
        if wc == 29 && cc == 159 {
            return true;
        }
        eprintln!("  WARNING: {} native returned unexpected result: wc={} cc={}", lang, wc, cc);
    } else {
        eprintln!("  WARNING: {} native result not valid JSON: {}", lang, &line[..line.len().min(80)]);
    }
    false
}

// ── Output formatting ──

struct LangResult {
    name: &'static str,
    wasm_label: &'static str,
    native_label: &'static str,
    wasm_cold_ms: Option<f64>,
    native_cold_ms: Option<f64>,
    wasm_hot_ns: Option<f64>,
    native_hot_ns: Option<f64>,
    wasm_cached_ns: Option<f64>,
}

fn print_language_block(r: &LangResult) {
    println!("\n─── {} ─────────────────────────────────────────────────", r.name);

    println!("  Cold start (compile/instantiate + first tool call):");
    match (r.wasm_cold_ms, r.native_cold_ms) {
        (Some(w), Some(n)) => {
            println!("    {:20} {:>8.1} ms", r.wasm_label, w);
            println!("    {:20} {:>8.1} ms", r.native_label, n);
            if w < n {
                println!("    WASM faster by {:.1}x", n / w);
            } else {
                println!("    Native faster by {:.1}x", w / n);
            }
        }
        (Some(w), None) => {
            println!("    {:20} {:>8.1} ms", r.wasm_label, w);
            println!("    {:20}  SKIP (runtime not found)", r.native_label);
        }
        (None, Some(n)) => {
            println!("    {:20}  SKIP (WASM not found)", r.wasm_label);
            println!("    {:20} {:>8.1} ms", r.native_label, n);
        }
        (None, None) => {
            println!("    SKIP — neither runtime available");
        }
    }

    println!("\n  Hot call (persistent instance/process):");
    match (r.wasm_hot_ns, r.native_hot_ns) {
        (Some(w), Some(n)) => {
            let w_us = w / 1000.0;
            let n_us = n / 1000.0;
            println!("    {:20} {:>6.1} µs/call  ({:.1}K calls/s)", r.wasm_label, w_us, 1_000_000.0 / w);
            println!("    {:20} {:>6.1} µs/call  ({:.1}K calls/s)", r.native_label, n_us, 1_000_000.0 / n);
            if w < n {
                println!("    WASM faster by {:.1}x", n / w);
            } else {
                println!("    Native faster by {:.1}x", w / n);
            }
        }
        (Some(w), None) => {
            println!("    {:20} {:>6.1} µs/call  ({:.1}K calls/s)", r.wasm_label, w / 1000.0, 1_000_000.0 / w);
            println!("    {:20}  SKIP", r.native_label);
        }
        (None, Some(n)) => {
            println!("    {:20}  SKIP", r.wasm_label);
            println!("    {:20} {:>6.1} µs/call  ({:.1}K calls/s)", r.native_label, n / 1000.0, 1_000_000.0 / n);
        }
        (None, None) => {
            println!("    SKIP — neither runtime available");
        }
    }

    if let Some(cached_ns) = r.wasm_cached_ns {
        let cached_us = cached_ns / 1000.0;
        println!("\n  Cached module (compile once, reuse):");
        println!("    {:20} {:>6.1} µs/call  ({:.1}K calls/s)", r.wasm_label, cached_us, 1_000_000.0 / cached_ns);
        if let Some(uncached) = r.wasm_hot_ns {
            let speedup = uncached / cached_ns;
            println!("    Speedup vs uncached:  {:.1}x", speedup);
        }
    }
}

fn print_summary_table(results: &[LangResult]) {
    println!("\n═══ Summary ═══════════════════════════════════════════════════════");
    println!("{:<14} {:>12} {:>12} {:>12} {:>10} {:>10}", "Language", "WASM hot", "WASM cached", "Native hot", "WASM cold", "Ratio");
    println!("{:<14} {:>12} {:>12} {:>12} {:>10} {:>10}", "", "(µs/call)", "(µs/call)", "(µs/call)", "(ms)", "(W/N)");
    println!("──────────────────────────────────────────────────────────────────");
    for r in results {
        let wasm_hot = r.wasm_hot_ns.map(|v| format!("{:.1}", v / 1000.0)).unwrap_or_else(|| "SKIP".into());
        let wasm_cached = r.wasm_cached_ns.map(|v| format!("{:.1}", v / 1000.0)).unwrap_or_else(|| "—".into());
        let native_hot = r.native_hot_ns.map(|v| format!("{:.1}", v / 1000.0)).unwrap_or_else(|| "SKIP".into());
        let wasm_cold = r.wasm_cold_ms.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "SKIP".into());
        let best_wasm = r.wasm_cached_ns.or(r.wasm_hot_ns);
        let ratio = match (best_wasm, r.native_hot_ns) {
            (Some(w), Some(n)) => format!("{:.2}x", n / w),
            _ => "—".into(),
        };
        println!("{:<14} {:>12} {:>12} {:>12} {:>10} {:>10}", r.name, wasm_hot, wasm_cached, native_hot, wasm_cold, ratio);
    }
    println!("──────────────────────────────────────────────────────────────────");
    println!("  Ratio = native_latency / wasm_latency (>1 means WASM is faster)");
}

// ── Main ──

fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  WASM vs Native Tool Execution Benchmark");
    println!("  Same tool (text_analyze), same input, per-language comparison");
    println!("═══════════════════════════════════════════════════════════════════");

    // Check WASM files
    let quickjs_wasm = std::path::Path::new("bench_tools/quickjs_wasm/quickjs.wasm");
    let micropython_wasm = std::path::Path::new("bench_tools/micropython_wasm/wasi-reactor/build/micropython.wasm");
    let lua_wasm = std::path::Path::new("bench_tools/lua_wasm/lua.wasm");
    let tinygo_wasm = std::path::Path::new("bench_tools/tinygo_wasm/tool.wasm");

    println!("\nWASM modules:");
    println!("  QuickJS:     {}", if quickjs_wasm.exists() { "found" } else { "MISSING" });
    println!("  MicroPython: {}", if micropython_wasm.exists() { "found" } else { "MISSING" });
    println!("  Lua:         {}", if lua_wasm.exists() { "found" } else { "MISSING" });
    println!("  TinyGo:      {}", if tinygo_wasm.exists() { "found" } else { "MISSING" });

    // Check native runtimes
    let python_cmd = find_command(&["python3", "python"]);
    let node_cmd = find_command(&["node"]);
    let lua_cmd = find_command(&["lua5.4", "lua54", "lua"]);
    let go_binary = std::path::Path::new("bench_tools/go_tool");
    let go_available = go_binary.exists();

    println!("\nNative runtimes:");
    println!("  Python:  {}", python_cmd.as_deref().unwrap_or("NOT FOUND"));
    println!("  Node.js: {}", node_cmd.as_deref().unwrap_or("NOT FOUND"));
    println!("  Lua:     {}", lua_cmd.as_deref().unwrap_or("NOT FOUND"));
    println!("  Go:      {}", if go_available { "found (bench_tools/go_tool)" } else { "NOT FOUND (run bench_tools/go_tool_build.sh)" });

    let request = make_request(1);
    let mut results: Vec<LangResult> = Vec::new();

    // ── JavaScript ──
    println!("\n\n─── Measuring JavaScript ─────────────────────────────────────");
    let mut js = LangResult {
        name: "JavaScript",
        wasm_label: "QuickJS/WASM",
        native_label: "Node.js",
        wasm_cold_ms: None,
        native_cold_ms: None,
        wasm_hot_ns: None,
        native_hot_ns: None,
        wasm_cached_ns: None,
    };

    if quickjs_wasm.exists() {
        let wasm_bytes = std::fs::read(quickjs_wasm).unwrap();

        // Correctness check
        let mut rt = QuickJsRuntime::cold_start(&wasm_bytes).expect("QuickJS cold start failed");
        rt.register_tool("text_analyze", JS_TOOL_SOURCE).expect("register failed");
        let result = rt.call_tool("text_analyze", BENCH_INPUT_JSON).expect("call failed");
        if verify_wasm_result(&result, "JS") {
            println!("  JS WASM correctness: OK");
        }
        rt.destroy().ok();

        // Cold start (3 runs, median)
        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            cold_times.push(wasm_cold_start_quickjs(&wasm_bytes));
        }
        js.wasm_cold_ms = Some(median_of(&mut cold_times));

        // Hot call (3 runs, median)
        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = wasm_hot_call_quickjs(&wasm_bytes) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            js.wasm_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    if let Some(ref cmd) = node_cmd {
        // Correctness check
        let mut child = Command::new(cmd)
            .args(["bench_tools/node_tool.js"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn node");
        {
            let stdin = child.stdin.as_mut().unwrap();
            writeln!(stdin, "{}", request).unwrap();
            stdin.flush().unwrap();
        }
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let _ = child.kill();
        let _ = child.wait();
        if verify_native_result(line.trim(), "JS") {
            println!("  JS native correctness: OK");
        }

        // Cold start
        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ms) = native_cold_start(cmd, &["bench_tools/node_tool.js"], &request) {
                cold_times.push(ms);
            }
        }
        if !cold_times.is_empty() {
            js.native_cold_ms = Some(median_of(&mut cold_times));
        }

        // Hot call
        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = native_hot_call(cmd, &["bench_tools/node_tool.js"]) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            js.native_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    print_language_block(&js);
    results.push(js);

    // ── Python ──
    println!("\n\n─── Measuring Python ─────────────────────────────────────────");
    let mut py = LangResult {
        name: "Python",
        wasm_label: "MicroPython/WASM",
        native_label: "CPython",
        wasm_cold_ms: None,
        native_cold_ms: None,
        wasm_hot_ns: None,
        native_hot_ns: None,
        wasm_cached_ns: None,
    };

    if micropython_wasm.exists() {
        let wasm_bytes = std::fs::read(micropython_wasm).unwrap();

        let mut rt = MicroPythonRuntime::cold_start(&wasm_bytes).expect("MicroPython cold start failed");
        rt.register_tool("text_analyze", PY_TOOL_SOURCE).expect("register failed");
        let result = rt.call_tool("text_analyze", BENCH_INPUT_JSON).expect("call failed");
        if verify_wasm_result(&result, "Python") {
            println!("  Python WASM correctness: OK");
        }
        rt.destroy().ok();

        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            cold_times.push(wasm_cold_start_micropython(&wasm_bytes));
        }
        py.wasm_cold_ms = Some(median_of(&mut cold_times));

        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = wasm_hot_call_micropython(&wasm_bytes) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            py.wasm_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    if let Some(ref cmd) = python_cmd {
        let mut child = Command::new(cmd)
            .args(["bench_tools/python_tool.py"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn python");
        {
            let stdin = child.stdin.as_mut().unwrap();
            writeln!(stdin, "{}", request).unwrap();
            stdin.flush().unwrap();
        }
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let _ = child.kill();
        let _ = child.wait();
        if verify_native_result(line.trim(), "Python") {
            println!("  Python native correctness: OK");
        }

        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ms) = native_cold_start(cmd, &["bench_tools/python_tool.py"], &request) {
                cold_times.push(ms);
            }
        }
        if !cold_times.is_empty() {
            py.native_cold_ms = Some(median_of(&mut cold_times));
        }

        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = native_hot_call(cmd, &["bench_tools/python_tool.py"]) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            py.native_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    print_language_block(&py);
    results.push(py);

    // ── Lua ──
    println!("\n\n─── Measuring Lua ────────────────────────────────────────────");
    let mut lua_r = LangResult {
        name: "Lua",
        wasm_label: "Lua/WASM",
        native_label: "Lua 5.4 native",
        wasm_cold_ms: None,
        native_cold_ms: None,
        wasm_hot_ns: None,
        native_hot_ns: None,
        wasm_cached_ns: None,
    };

    if lua_wasm.exists() {
        let wasm_bytes = std::fs::read(lua_wasm).unwrap();

        let mut rt = LuaRuntime::cold_start(&wasm_bytes).expect("Lua cold start failed");
        rt.register_tool("text_analyze", LUA_TOOL_SOURCE).expect("register failed");
        let result = rt.call_tool("text_analyze", BENCH_INPUT_JSON).expect("call failed");
        if verify_wasm_result(&result, "Lua") {
            println!("  Lua WASM correctness: OK");
        }
        rt.destroy().ok();

        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            cold_times.push(wasm_cold_start_lua(&wasm_bytes));
        }
        lua_r.wasm_cold_ms = Some(median_of(&mut cold_times));

        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = wasm_hot_call_lua(&wasm_bytes) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            lua_r.wasm_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    if let Some(ref cmd) = lua_cmd {
        let mut child = Command::new(cmd)
            .args(["bench_tools/lua_tool.lua"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn lua");
        {
            let stdin = child.stdin.as_mut().unwrap();
            writeln!(stdin, "{}", request).unwrap();
            stdin.flush().unwrap();
        }
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let _ = child.kill();
        let _ = child.wait();
        if verify_native_result(line.trim(), "Lua") {
            println!("  Lua native correctness: OK");
        }

        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ms) = native_cold_start(cmd, &["bench_tools/lua_tool.lua"], &request) {
                cold_times.push(ms);
            }
        }
        if !cold_times.is_empty() {
            lua_r.native_cold_ms = Some(median_of(&mut cold_times));
        }

        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = native_hot_call(cmd, &["bench_tools/lua_tool.lua"]) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            lua_r.native_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    print_language_block(&lua_r);
    results.push(lua_r);

    // ── Go ──
    println!("\n\n─── Measuring Go ─────────────────────────────────────────────");
    let mut go_r = LangResult {
        name: "Go",
        wasm_label: "TinyGo/WASM",
        native_label: "Go native",
        wasm_cold_ms: None,
        native_cold_ms: None,
        wasm_hot_ns: None,
        native_hot_ns: None,
        wasm_cached_ns: None,
    };

    if tinygo_wasm.exists() {
        let wasm_bytes = std::fs::read(tinygo_wasm).unwrap();

        // Go WASM per-call (production path — no cache)
        if let Some(ns) = go_wasm_per_call(&wasm_bytes) {
            println!("  Go WASM per-call (no cache): {:.1} µs/call", ns / 1000.0);
            go_r.wasm_hot_ns = Some(ns);
        }

        // Go WASM cached module (compile once, reuse)
        let mut cached_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = go_wasm_cached(&wasm_bytes) {
                cached_times.push(ns);
            }
        }
        if !cached_times.is_empty() {
            go_r.wasm_cached_ns = Some(median_of(&mut cached_times));
            println!("  Go WASM cached module:   {:.1} µs/call", go_r.wasm_cached_ns.unwrap() / 1000.0);
        }

        // Go WASM cold start (per-call instantiation)
        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            let start = Instant::now();
            for _ in 0..COLD_ITERS {
                use wasmer::{FunctionEnv, Instance, Module, Store};
                use velocity_mcp::wasm_runtime::wasi::{WasiEnv, build_wasi_imports};
                let engine = wasmer::Engine::from(wasmer::Cranelift::default());
                let module = Module::new(&engine, &wasm_bytes).unwrap();
                let mut store = Store::new(engine);
                let env = FunctionEnv::new(&mut store, WasiEnv { memory: None });
                let wasi_imports = build_wasi_imports(&mut store, &env);
                let instance = Instance::new(&mut store, &module, &wasi_imports).unwrap();
                let memory = instance.exports.get_memory("memory").unwrap().clone();
                env.as_mut(&mut store).memory = Some(memory);
                if let Ok(start_fn) = instance.exports.get_function("_start") {
                    let _ = start_fn.call(&mut store, &[]);
                }
            }
            cold_times.push(start.elapsed().as_nanos() as f64 / COLD_ITERS as f64 / 1_000_000.0);
        }
        go_r.wasm_cold_ms = Some(median_of(&mut cold_times));
    }

    if go_available {
        let mut child = Command::new("bench_tools/go_tool")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to spawn go_tool");
        {
            let stdin = child.stdin.as_mut().unwrap();
            writeln!(stdin, "{}", request).unwrap();
            stdin.flush().unwrap();
        }
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let _ = child.kill();
        let _ = child.wait();
        if verify_native_result(line.trim(), "Go") {
            println!("  Go native correctness: OK");
        }

        let mut cold_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ms) = native_cold_start("bench_tools/go_tool", &[], &request) {
                cold_times.push(ms);
            }
        }
        if !cold_times.is_empty() {
            go_r.native_cold_ms = Some(median_of(&mut cold_times));
        }

        let mut hot_times = Vec::with_capacity(RUNS);
        for _ in 0..RUNS {
            if let Some(ns) = native_hot_call("bench_tools/go_tool", &[]) {
                hot_times.push(ns);
            }
        }
        if !hot_times.is_empty() {
            go_r.native_hot_ns = Some(median_of(&mut hot_times));
        }
    }

    print_language_block(&go_r);
    results.push(go_r);

    // ── Summary ──
    print_summary_table(&results);

    println!("\nDone.");
}
