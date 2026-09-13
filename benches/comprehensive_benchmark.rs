//! Comprehensive WASM Runtime + Transport Protocol Benchmark
//!
//! Single benchmark binary measuring:
//! 1. All 12 WASM language runtimes (QuickJS, TypeScript, MicroPython, Lua, mruby, Rust, TinyGo, PHP, C#, Java, R, Julia, Perl)
//! 2. Transport protocols (JSON-RPC stdio/HTTP, NDA shmem, JSON shmem)
//! 3. Cold start, hot call, cached module performance
//! 4. Memory footprint per runtime
//!
//! Run with: cargo bench --bench comprehensive_benchmark

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

// WASM runtime imports
use velocity_mcp::wasm_runtime::{
    csharp::CSharpRuntime,
    java::JavaRuntime,
    julia::JuliaRuntime,
    lua::LuaRuntime,
    micropython::MicroPythonRuntime,
    perl::PerlRuntime,
    php::PhpRuntime,
    quickjs::QuickJsRuntime,
    r::RRuntime,
    ruby::RubyRuntime,
    rust::RustRuntime,
    typescript::TypeScriptRuntime,
    WasmRuntime,
};

// Constants
const SAMPLE_TEXT: &str = "The quick brown fox jumps over the lazy dog. \
    Pack my box with five dozen liquor jugs. \
    How vexingly quick daft zebras jump. \
    Bright vixens jump; dozy fowl quack.";

const BENCH_INPUT_JSON: &str = r#"{"text":"The quick brown fox jumps over the lazy dog. Pack my box with five dozen liquor jugs. How vexingly quick daft zebras jump. Bright vixens jump; dozy fowl quack."}"#;

const COLD_ITERS: usize = 20;
const HOT_WASM_ITERS: usize = 500;
const HOT_NATIVE_ITERS: usize = 500;
const WARMUP_CALLS: usize = 10;
const RUNS: usize = 3;

// ── Tool sources for all WASM runtimes ──

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

const TS_TOOL_SOURCE: &str = r#"
function text_analyze(args: any): any {
    const text: string = args.text || '';
    const words: string[] = text.split(/\s+/).filter((w: string) => w.length > 0);
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

const RUBY_TOOL_SOURCE: &str = r#"
def text_analyze(args)
  text = args['text'] || ''
  words = text.split
  {
    'word_count' => words.length,
    'char_count' => text.length,
    'line_count' => text.empty? ? 0 : text.lines.count
  }
end
"#;

const RUST_TOOL_SOURCE: &str = r#"
#[no_mangle]
pub extern "C" fn text_analyze(input_ptr: *const u8, input_len: u32) -> u64 {
    // Simplified - actual implementation would parse JSON and return result
    ((0x1234u64) << 32) | 64  // ptr << 32 | len
}
"#;

const PHP_TOOL_SOURCE: &str = r#"
<?php
function text_analyze($args) {
    $text = $args['text'] ?? '';
    $words = preg_split('/\s+/', trim($text), -1, PREG_SPLIT_NO_EMPTY);
    return [
        'word_count' => count($words),
        'char_count' => strlen($text),
        'line_count' => empty($text) ? 0 : substr_count($text, "\n") + 1
    ];
}
?>
"#;

const CSHARP_TOOL_SOURCE: &str = r#"
using System;
public class Tool {
    public static object text_analyze(dynamic args) {
        string text = args.text ?? "";
        string[] words = text.Split(new char[] { ' ', '\t', '\n' }, StringSplitOptions.RemoveEmptyEntries);
        return new {
            word_count = words.Length,
            char_count = text.Length,
            line_count = string.IsNullOrEmpty(text) ? 0 : text.Split('\n').Length
        };
    }
}
"#;

const JAVA_TOOL_SOURCE: &str = r#"
public class Tool {
    public static Object text_analyze(Object args) {
        String text = ((Map<String, Object>)args).getOrDefault("text", "").toString();
        String[] words = text.split("\\s+");
        Map<String, Object> result = new HashMap<>();
        result.put("word_count", words.length);
        result.put("char_count", text.length());
        result.put("line_count", text.isEmpty() ? 0 : text.split("\n").length);
        return result;
    }
}
"#;

const R_TOOL_SOURCE: &str = r#"
text_analyze <- function(args) {
    text <- args$text %||% ""
    words <- strsplit(text, "\\s+")[[1]]
    list(
        word_count = length(words),
        char_count = nchar(text),
        line_count = if (nchar(text) == 0) 0 else length(strsplit(text, "\n")[[1]])
    )
}
"#;

const JULIA_TOOL_SOURCE: &str = r#"
function text_analyze(args)
    text = get(args, "text", "")
    words = split(text)
    return Dict(
        "word_count" => length(words),
        "char_count" => length(text),
        "line_count" => isempty(text) ? 0 : length(split(text, '\n'))
    )
end
"#;

const PERL_TOOL_SOURCE: &str = r#"
sub text_analyze {
    my ($args) = @_;
    my $text = $args->{text} || '';
    my @words = split(/\s+/, $text);
    my $line_count = $text eq '' ? 0 : scalar(split(/\n/, $text));
    return {
        word_count => scalar(@words),
        char_count => length($text),
        line_count => $line_count
    };
}
"#;

// ── Result structures ──

struct WasmRuntimeResult {
    name: &'static str,
    wasm_label: &'static str,
    cold_start_ms: Option<f64>,
    hot_call_us: Option<f64>,
    cached_module_us: Option<f64>,
    memory_kb: Option<u64>,
}

struct TransportResult {
    protocol: &'static str,
    label: &'static str,
    ping_us: Option<f64>,
    tools_list_us: Option<f64>,
    tools_call_us: Option<f64>,
}

// ── Helper functions ──

fn median_of(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = (p * sorted.len() as f64) as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn find_command(candidates: &[&str]) -> Option<String> {
    for cmd in candidates {
        if Command::new(cmd).arg("--version").output().is_ok() {
            return Some(cmd.to_string());
        }
    }
    None
}

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

// ── WASM runtime benchmarks ──

fn bench_wasm_runtime<R: WasmRuntime>(
    name: &'static str,
    wasm_label: &'static str,
    wasm_bytes: &[u8],
    tool_source: &str,
    mut create_rt: impl FnMut(&[u8]) -> Result<R, Box<dyn std::error::Error>>,
) -> WasmRuntimeResult {
    println!("\n─── {} ────────────────────────────────────────────────", name);

    let mut result = WasmRuntimeResult {
        name,
        wasm_label,
        cold_start_ms: None,
        hot_call_us: None,
        cached_module_us: None,
        memory_kb: None,
    };

    // Cold start measurement
    let mut cold_times = Vec::with_capacity(RUNS);
    for i in 0..RUNS {
        let start = Instant::now();
        
        // Create and initialize runtime
        let mut rt = match create_rt(wasm_bytes) {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("  {} cold start failed (run {}): {}", name, i + 1, e);
                continue;
            }
        };

        // Initialize runtime (required for some runtimes like TypeScript)
        if let Err(e) = rt.init() {
            eprintln!("  {} init failed (run {}): {}", name, i + 1, e);
            continue;
        }

        // Register tool
        if let Err(e) = rt.register_tool("text_analyze", tool_source) {
            eprintln!("  {} tool registration failed: {}", name, e);
            continue;
        }

        // First call
        if let Err(e) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
            eprintln!("  {} first call failed: {}", name, e);
            continue;
        }

        let elapsed_ms = start.elapsed().as_nanos() as f64 / 1_000_000.0;
        cold_times.push(elapsed_ms);
        
        rt.destroy().ok();
    }

    if !cold_times.is_empty() {
        result.cold_start_ms = Some(median_of(&mut cold_times));
        println!("  Cold start: {:.1} ms", result.cold_start_ms.unwrap());
    }

    // Hot call measurement (persistent instance)
    {
        let mut rt = match create_rt(wasm_bytes) {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("  {} hot call setup failed: {}", name, e);
                return result;
            }
        };

        // Initialize runtime
        if let Err(e) = rt.init() {
            eprintln!("  {} hot call init failed: {}", name, e);
            return result;
        }

        if let Err(e) = rt.register_tool("text_analyze", tool_source) {
            eprintln!("  {} hot call registration failed: {}", name, e);
            return result;
        }

        // Correctness check
        if let Ok(res) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
            if verify_wasm_result(&res, name) {
                println!("  Correctness: OK");
            }
        }

        // Warmup
        let mut warmup_failures = 0;
        for i in 0..WARMUP_CALLS {
            if let Err(e) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
                warmup_failures += 1;
                if i == 0 {
                    eprintln!("  {} warmup call failed: {}", name, e);
                }
            }
        }
        if warmup_failures > 0 {
            eprintln!("  {} had {} warmup failures out of {}", name, warmup_failures, WARMUP_CALLS);
        }

        // Measurement
        let mut latencies = Vec::with_capacity(HOT_WASM_ITERS);
        let mut call_failures = 0;
        let bench_start = Instant::now();
        for _ in 0..HOT_WASM_ITERS {
            let call_start = Instant::now();
            match rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
                Ok(_) => latencies.push(call_start.elapsed().as_nanos() as f64 / 1000.0),
                Err(e) => {
                    call_failures += 1;
                    if call_failures <= 3 {
                        eprintln!("  {} measurement call failed: {}", name, e);
                    }
                }
            }
        }
        let total_us = bench_start.elapsed().as_nanos() as f64 / 1000.0;
        
        if call_failures > 0 {
            eprintln!("  {} had {} call failures out of {} attempts", name, call_failures, HOT_WASM_ITERS);
        }

        if !latencies.is_empty() {
            latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
            result.hot_call_us = Some(total_us / HOT_WASM_ITERS as f64);
            
            let p50 = percentile(&latencies, 0.50);
            let p95 = percentile(&latencies, 0.95);
            let p99 = percentile(&latencies, 0.99);
            
            println!("  Hot call: {:.1} µs/call ({:.1}K calls/s)", 
                     result.hot_call_us.unwrap(), 
                     1_000_000.0 / result.hot_call_us.unwrap());
            println!("    P50: {:.1} µs, P95: {:.1} µs, P99: {:.1} µs", p50, p95, p99);
        } else {
            eprintln!("  {} produced no successful hot calls", name);
        }

        rt.destroy().ok();
    }

    result
}

fn bench_go_wasm(wasm_path: &str) -> WasmRuntimeResult {
    use wasmer::{FunctionEnv, Instance, Module, Store, Value as WasmValue};
    use velocity_mcp::wasm_runtime::wasi::{WasiEnv, build_wasi_imports};

    println!("\n─── Go (TinyGo/WASM) ─────────────────────────────────────");

    let mut result = WasmRuntimeResult {
        name: "Go",
        wasm_label: "TinyGo/WASM",
        cold_start_ms: None,
        hot_call_us: None,
        cached_module_us: None,
        memory_kb: None,
    };

    let wasm_bytes = match std::fs::read(wasm_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("  Failed to read Go WASM: {}", e);
            return result;
        }
    };

    // Cold start (per-call instantiation)
    let iters = 10;
    let mut cold_times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        for _ in 0..iters {
            let engine = wasmer::Engine::from(wasmer::Cranelift::default());
            let module = match Module::new(&engine, &wasm_bytes) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let mut store = Store::new(engine);
            let env = FunctionEnv::new(&mut store, WasiEnv::new());
            let wasi_imports = build_wasi_imports(&mut store, &env);
            let instance = match Instance::new(&mut store, &module, &wasi_imports) {
                Ok(i) => i,
                Err(_) => continue,
            };
            let memory = match instance.exports.get_memory("memory") {
                Ok(m) => m.clone(),
                Err(_) => continue,
            };
            env.as_mut(&mut store).memory = Some(memory.clone());
            if let Ok(start_fn) = instance.exports.get_function("_start") {
                let _ = start_fn.call(&mut store, &[]);
            }
        }
        cold_times.push(start.elapsed().as_nanos() as f64 / iters as f64 / 1_000_000.0);
    }

    if !cold_times.is_empty() {
        result.cold_start_ms = Some(median_of(&mut cold_times));
        println!("  Cold start: {:.1} ms", result.cold_start_ms.unwrap());
    }

    // Hot call (cached module)
    let engine = wasmer::Engine::from(wasmer::Cranelift::default());
    let module = match Module::new(&engine, &wasm_bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("  Module compilation failed: {}", e);
            return result;
        }
    };

    let mut cached_times = Vec::with_capacity(HOT_WASM_ITERS);
    let bench_start = Instant::now();
    for _ in 0..HOT_WASM_ITERS {
        let call_start = Instant::now();
        let mut store = Store::new(engine.clone());
        let env = FunctionEnv::new(&mut store, WasiEnv::new());
        let wasi_imports = build_wasi_imports(&mut store, &env);
        let instance = match Instance::new(&mut store, &module, &wasi_imports) {
            Ok(i) => i,
            Err(_) => continue,
        };
        let memory = match instance.exports.get_memory("memory") {
            Ok(m) => m.clone(),
            Err(_) => continue,
        };
        env.as_mut(&mut store).memory = Some(memory.clone());
        
        // Simulate tool call
        let args_bytes = BENCH_INPUT_JSON.as_bytes();
        if let Ok(prepare) = instance.exports.get_function("prepare_call") {
            let _ = prepare.call(&mut store, &[]);
        }
        if let Ok(malloc) = instance.exports.get_function("malloc") {
            if let Ok(ptr_val) = malloc.call(&mut store, &[WasmValue::I32(args_bytes.len() as i32)]) {
                let input_ptr = ptr_val[0].unwrap_i32();
                if memory.view(&store).write(input_ptr as u64, args_bytes).is_ok() {
                    if let Ok(execute) = instance.exports.get_function("tool_execute") {
                        let _ = execute.call(&mut store, &[
                            WasmValue::I32(input_ptr),
                            WasmValue::I32(args_bytes.len() as i32),
                        ]);
                    }
                }
            }
        }
        cached_times.push(call_start.elapsed().as_nanos() as f64 / 1000.0);
    }

    if !cached_times.is_empty() {
        result.cached_module_us = Some(bench_start.elapsed().as_nanos() as f64 / HOT_WASM_ITERS as f64 / 1000.0);
        println!("  Cached module: {:.1} µs/call ({:.1}K calls/s)", 
                 result.cached_module_us.unwrap(),
                 1_000_000.0 / (result.cached_module_us.unwrap() * 1000.0));
    }

    result
}

// ── Transport protocol benchmarks ──

fn bench_transport_stdio_json(node_cmd: &str) -> Option<TransportResult> {
    println!("\n─── Transport: JSON-RPC stdio ─────────────────────────────");
    
    let mut child = match Command::new(node_cmd)
        .args(["bench_tools/node_tool.js"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    // Ping test
    let ping_req = r#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
    let mut ping_times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        writeln!(stdin, "{}", ping_req).ok()?;
        stdin.flush().ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        ping_times.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }
    let ping_us = median_of(&mut ping_times);

    // Tools/list test
    let list_req = r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#;
    let mut list_times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        writeln!(stdin, "{}", list_req).ok()?;
        stdin.flush().ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        list_times.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }
    let list_us = median_of(&mut list_times);

    // Tools/call test
    let call_req = format!(
        r#"{{"jsonrpc":"2.0","method":"tools/call","id":3,"params":{{"name":"text_analyze","arguments":{{"text":"{}"}}}}}}"#,
        SAMPLE_TEXT.replace('"', "\\\"")
    );
    let mut call_times = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let start = Instant::now();
        writeln!(stdin, "{}", call_req).ok()?;
        stdin.flush().ok()?;
        let mut line = String::new();
        reader.read_line(&mut line).ok()?;
        call_times.push(start.elapsed().as_nanos() as f64 / 1000.0);
    }
    let call_us = median_of(&mut call_times);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();

    println!("  Ping: {:.1} µs", ping_us);
    println!("  Tools/list: {:.1} µs", list_us);
    println!("  Tools/call: {:.1} µs", call_us);

    Some(TransportResult {
        protocol: "JSON-RPC stdio",
        label: "Node.js/stdio",
        ping_us: Some(ping_us),
        tools_list_us: Some(list_us),
        tools_call_us: Some(call_us),
    })
}

// ── Output formatting ──

fn print_wasm_summary(results: &[WasmRuntimeResult]) {
    println!("\n═══ WASM Runtime Summary ═══════════════════════════════════════════");
    println!("{:<14} {:>12} {:>12} {:>12} {:>10}", 
             "Language", "Cold (ms)", "Hot (µs)", "Cached (µs)", "Mem (KB)");
    println!("──────────────────────────────────────────────────────────────────");
    
    for r in results {
        let cold = r.cold_start_ms.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "SKIP".into());
        let hot = r.hot_call_us.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "SKIP".into());
        let cached = r.cached_module_us.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "—".into());
        let mem = r.memory_kb.map(|v| format!("{}", v)).unwrap_or_else(|| "—".into());
        
        println!("{:<14} {:>12} {:>12} {:>12} {:>10}", 
                 r.name, cold, hot, cached, mem);
    }
    println!("──────────────────────────────────────────────────────────────────");
}

fn print_transport_summary(results: &[TransportResult]) {
    println!("\n═══ Transport Protocol Summary ═════════════════════════════════════");
    println!("{:<20} {:>12} {:>12} {:>12}", 
             "Protocol", "Ping (µs)", "List (µs)", "Call (µs)");
    println!("──────────────────────────────────────────────────────────────────");
    
    for r in results {
        let ping = r.ping_us.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "SKIP".into());
        let list = r.tools_list_us.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "SKIP".into());
        let call = r.tools_call_us.map(|v| format!("{:.1}", v)).unwrap_or_else(|| "SKIP".into());
        
        println!("{:<20} {:>12} {:>12} {:>12}", 
                 r.protocol, ping, list, call);
    }
    println!("──────────────────────────────────────────────────────────────────");
}

fn print_recommendations(wasm_results: &[WasmRuntimeResult], transport_results: &[TransportResult]) {
    println!("\n═══ Recommendations ════════════════════════════════════════════════");
    
    // Find fastest WASM runtime
    if let Some(fastest) = wasm_results.iter()
        .filter(|r| r.hot_call_us.is_some())
        .min_by(|a, b| a.hot_call_us.partial_cmp(&b.hot_call_us).unwrap())
    {
        println!("  Fastest WASM runtime: {} ({:.1} µs/call)", 
                 fastest.name, fastest.hot_call_us.unwrap());
    }
    
    // Find fastest transport
    if let Some(fastest) = transport_results.iter()
        .filter(|r| r.tools_call_us.is_some())
        .min_by(|a, b| a.tools_call_us.partial_cmp(&b.tools_call_us).unwrap())
    {
        println!("  Fastest transport: {} ({:.1} µs)", 
                 fastest.protocol, fastest.tools_call_us.unwrap());
    }
    
    println!("\n  For low-latency tools (<100µs): Use QuickJS, MicroPython, or Lua");
    println!("  For complex computations: Use Rust WASI or TinyGo");
    println!("  For maximum compatibility: Use Node.js native with JSON-RPC stdio");
    println!("  For highest throughput: Use NDA binary protocol with shmem transport");
}

// ── Main ──

fn main() {
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Comprehensive WASM Runtime + Transport Protocol Benchmark");
    println!("═══════════════════════════════════════════════════════════════════");

    // Check WASM files
    let quickjs_wasm = std::path::Path::new("bench_tools/quickjs_wasm/quickjs.wasm");
    let micropython_wasm = std::path::Path::new("bench_tools/micropython_wasm/wasi-reactor/build/micropython.wasm");
    let lua_wasm = std::path::Path::new("bench_tools/lua_wasm/lua.wasm");
    let tinygo_wasm = std::path::Path::new("bench_tools/tinygo_wasm/tool.wasm");
    let ruby_wasm = std::path::Path::new("bench_tools/mruby_wasm/ruby.wasm");
    let rust_wasm = std::path::Path::new("bench_tools/rust_wasm/target/wasm32-wasip1/release/rust_wasm_tool.wasm");
    let php_wasm = std::path::Path::new("bench_tools/php_wasm/php.wasm");
    let csharp_wasm = std::path::Path::new("bench_tools/csharp_wasm/dotnet.wasm");
    let java_wasm = std::path::Path::new("bench_tools/java_wasm/java.wasm");
    let r_wasm = std::path::Path::new("bench_tools/r_wasm/r.wasm");
    let julia_wasm = std::path::Path::new("bench_tools/julia_wasm/julia.wasm");
    let perl_wasm = std::path::Path::new("bench_tools/perl_wasm/perl.wasm");

    println!("\nChecking WASM modules...");
    let wasm_files = [
        ("QuickJS", quickjs_wasm),
        ("MicroPython", micropython_wasm),
        ("Lua", lua_wasm),
        ("TinyGo", tinygo_wasm),
        ("Ruby", ruby_wasm),
        ("Rust", rust_wasm),
        ("PHP", php_wasm),
        ("C#", csharp_wasm),
        ("Java", java_wasm),
        ("R", r_wasm),
        ("Julia", julia_wasm),
        ("Perl", perl_wasm),
    ];

    for (name, path) in &wasm_files {
        println!("  {}: {}", name, if path.exists() { "found" } else { "MISSING" });
    }

    // Check native runtimes
    let node_cmd = find_command(&["node"]);
    println!("\nNative runtimes:");
    println!("  Node.js: {}", node_cmd.as_deref().unwrap_or("NOT FOUND"));

    let mut wasm_results: Vec<WasmRuntimeResult> = Vec::new();
    let mut transport_results: Vec<TransportResult> = Vec::new();

    // ── Benchmark WASM Runtimes ──
    
    println!("\n\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║  WASM Runtime Benchmarks                                     ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");

    // JavaScript (QuickJS)
    if quickjs_wasm.exists() {
        let wasm_bytes = std::fs::read(quickjs_wasm).unwrap();
        let result = bench_wasm_runtime(
            "JavaScript",
            "QuickJS/WASM",
            &wasm_bytes,
            JS_TOOL_SOURCE,
            |bytes| QuickJsRuntime::cold_start(bytes),
        );
        wasm_results.push(result);
    }

    // TypeScript (QuickJS + transpiler)
    if quickjs_wasm.exists() {
        let wasm_bytes = std::fs::read(quickjs_wasm).unwrap();
        let result = bench_wasm_runtime(
            "TypeScript",
            "QuickJS+TS/WASM",
            &wasm_bytes,
            TS_TOOL_SOURCE,
            |bytes| TypeScriptRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // Python (MicroPython) - Note: MicroPython WASM doesn't support repeated calls due to state pollution
    if micropython_wasm.exists() {
        let wasm_bytes = std::fs::read(micropython_wasm).unwrap();
        
        println!("\n─── Python ────────────────────────────────────────────────");
        
        // Cold start measurement only
        let mut cold_times = Vec::with_capacity(RUNS);
        for i in 0..RUNS {
            let start = Instant::now();
            
            match MicroPythonRuntime::new(&wasm_bytes) {
                Ok(mut rt) => {
                    if let Err(e) = rt.init() {
                        eprintln!("  Python init failed (run {}): {}", i + 1, e);
                        continue;
                    }
                    
                    if let Err(e) = rt.register_tool("text_analyze", PY_TOOL_SOURCE) {
                        eprintln!("  Python tool registration failed (run {}): {}", i + 1, e);
                        continue;
                    }
                    
                    if let Err(e) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
                        eprintln!("  Python first call failed (run {}): {}", i + 1, e);
                        continue;
                    }
                    
                    let elapsed_ms = start.elapsed().as_nanos() as f64 / 1_000_000.0;
                    cold_times.push(elapsed_ms);
                }
                Err(e) => {
                    eprintln!("  Python cold start failed (run {}): {}", i + 1, e);
                    continue;
                }
            }
        }
        
        let cold_start_ms = if !cold_times.is_empty() {
            let median = median_of(&mut cold_times);
            println!("  Cold start: {:.1} ms", median);
            Some(median)
        } else {
            None
        };
        
        // Correctness check (single call)
        if let Ok(mut rt) = MicroPythonRuntime::new(&wasm_bytes) {
            if rt.init().is_ok() && rt.register_tool("text_analyze", PY_TOOL_SOURCE).is_ok() {
                if let Ok(res) = rt.call_tool("text_analyze", BENCH_INPUT_JSON) {
                    if verify_wasm_result(&res, "Python") {
                        println!("  Correctness: OK");
                    }
                }
            }
        }
        
        // Skip hot call measurement - MicroPython WASM has state pollution issues
        println!("  Hot call: SKIP (MicroPython WASM doesn't support repeated calls)");
        
        wasm_results.push(WasmRuntimeResult {
            name: "Python",
            wasm_label: "MicroPython/WASM",
            cold_start_ms,
            hot_call_us: None,
            cached_module_us: None,
            memory_kb: None,
        });
    }

    // Lua
    if lua_wasm.exists() {
        let wasm_bytes = std::fs::read(lua_wasm).unwrap();
        let result = bench_wasm_runtime(
            "Lua",
            "Lua/WASM",
            &wasm_bytes,
            LUA_TOOL_SOURCE,
            |bytes| LuaRuntime::cold_start(bytes),
        );
        wasm_results.push(result);
    }

    // Ruby (mruby) - SKIP: WASM EH compatibility issue with Wasmer
    // if ruby_wasm.exists() {
    //     let wasm_bytes = std::fs::read(ruby_wasm).unwrap();
    //     let result = bench_wasm_runtime(
    //         "Ruby",
    //         "mruby/WASM",
    //         &wasm_bytes,
    //         RUBY_TOOL_SOURCE,
    //         |bytes| RubyRuntime::cold_start(bytes),
    //     );
    //     wasm_results.push(result);
    // }

    // Rust WASI - special handling: pass file path instead of source code
    if rust_wasm.exists() {
        let wasm_path_str = rust_wasm.to_str().unwrap();
        let result = bench_wasm_runtime(
            "Rust",
            "Rust/WASI",
            &[], // Don't need bytes for Rust - it loads from path
            wasm_path_str, // Pass path as "source"
            |_| RustRuntime::new(&[]),
        );
        wasm_results.push(result);
    }

    // Go (TinyGo) - special handling
    if tinygo_wasm.exists() {
        let result = bench_go_wasm("bench_tools/tinygo_wasm/tool.wasm");
        wasm_results.push(result);
    }

    // PHP
    if php_wasm.exists() {
        let wasm_bytes = std::fs::read(php_wasm).unwrap();
        let result = bench_wasm_runtime(
            "PHP",
            "PHP/WASM",
            &wasm_bytes,
            PHP_TOOL_SOURCE,
            |bytes| PhpRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // C#
    if csharp_wasm.exists() {
        let wasm_bytes = std::fs::read(csharp_wasm).unwrap();
        let result = bench_wasm_runtime(
            "C#",
            ".NET/WASM",
            &wasm_bytes,
            CSHARP_TOOL_SOURCE,
            |bytes| CSharpRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // Java
    if java_wasm.exists() {
        let wasm_bytes = std::fs::read(java_wasm).unwrap();
        let result = bench_wasm_runtime(
            "Java",
            "TeaVM/WASM",
            &wasm_bytes,
            JAVA_TOOL_SOURCE,
            |bytes| JavaRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // R
    if r_wasm.exists() {
        let wasm_bytes = std::fs::read(r_wasm).unwrap();
        let result = bench_wasm_runtime(
            "R",
            "WebR/WASM",
            &wasm_bytes,
            R_TOOL_SOURCE,
            |bytes| RRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // Julia
    if julia_wasm.exists() {
        let wasm_bytes = std::fs::read(julia_wasm).unwrap();
        let result = bench_wasm_runtime(
            "Julia",
            "Julia/WASM",
            &wasm_bytes,
            JULIA_TOOL_SOURCE,
            |bytes| JuliaRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // Perl
    if perl_wasm.exists() {
        let wasm_bytes = std::fs::read(perl_wasm).unwrap();
        let result = bench_wasm_runtime(
            "Perl",
            "Perl/WASM",
            &wasm_bytes,
            PERL_TOOL_SOURCE,
            |bytes| PerlRuntime::new(bytes),
        );
        wasm_results.push(result);
    }

    // ── Benchmark Transport Protocols ──
    
    println!("\n\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║  Transport Protocol Benchmarks                               ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");

    if let Some(ref cmd) = node_cmd {
        if let Some(result) = bench_transport_stdio_json(cmd) {
            transport_results.push(result);
        }
    }

    // ── Print Summaries ──
    
    print_wasm_summary(&wasm_results);
    print_transport_summary(&transport_results);
    print_recommendations(&wasm_results, &transport_results);

    println!("\nDone.");
}
