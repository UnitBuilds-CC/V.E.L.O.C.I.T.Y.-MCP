use std::time::Instant;
use std::hint::black_box;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use std::thread;
use tracing::info;
use serde_json::{json, Value};
use crate::protocol::nmcp_binary::NmcpBinaryFrame;
use crate::protocol::nda_native;
use crate::ipc::shmem::SharedMemoryBuffer;
use crate::registry;

pub fn run_benchmarks() {
    info!("Starting V.E.L.O.C.I.T.Y.-MCP v3.0.0 Performance Benchmark Suite");
    println!("================================================================");
    println!("     V.E.L.O.C.I.T.Y.-MCP v3.0.0 Performance Benchmark Suite");
    println!("================================================================");

    bench_json_parsing();
    bench_nda_native_parsing();
    bench_protocol_overhead();
    bench_tlv_encoding();
    bench_shmem_throughput();
    bench_nda_native_shmem();
    bench_concurrent_dispatch();
    bench_e2e_tool_calls();

    println!("\n================================================================");
    println!("                        All Benchmarks Complete");
    println!("================================================================");
}

fn bench_json_parsing() {
    println!("\n─── 1. JSON-RPC Parsing ───────────────────────────────────────");

    let json_req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"C:/invoices/inv-001.nda"}},"id":101}"#;
    let iterations = 500_000;

    println!("  serde_json parse ({} iterations)...", iterations);
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iterations {
        let val: Value = serde_json::from_str(black_box(json_req)).unwrap();
        if let Some(method) = val["method"].as_str() {
            for b in method.bytes() { checksum = checksum.wrapping_add(b as u32); }
        }
    }
    let json_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum);
    println!("  JSON-RPC parse:  {:.1} ns/req  ({:.2}M req/s)", json_ns, 1000.0 / json_ns);
}

fn bench_nda_native_parsing() {
    println!("\n─── 2. NDA-Native Binary Frame Parsing ────────────────────────");

    let frame = nda_native::build_nda_request(
        nda_native::METHOD_TOOLS_CALL,
        &json!(101),
        &json!({"name": "read_nda", "arguments": {"ndaPath": "C:/invoices/inv-001.nda"}}),
    );
    let iterations = 1_000_000;

    println!("  Zero-alloc parse + Merkle verify ({} iterations)...", iterations);
    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iterations {
        match nda_native::parse_nda_request(black_box(&frame)) {
            Ok(req) => {
                checksum = checksum.wrapping_add(req.method as u32);
                if let Some(s) = req.request_id.as_i64() {
                    checksum = checksum.wrapping_add(s as u32);
                }
            }
            Err(_) => checksum = checksum.wrapping_add(1),
        }
    }
    let nda_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum);
    println!("  NDA-native parse: {:.1} ns/req  ({:.2}M req/s)", nda_ns, 1000.0 / nda_ns);

    let mut binary_buffer = Vec::new();
    binary_buffer.extend_from_slice(b"NMCP");
    binary_buffer.extend_from_slice(&[0u8; 32]);
    binary_buffer.extend_from_slice(b"read_nda C:/invoices/inv-001.nda");

    println!("  Legacy binary frame parse ({} iterations)...", iterations);
    let start = Instant::now();
    let mut checksum2: u32 = 0;
    for _ in 0..iterations {
        let f = NmcpBinaryFrame::parse(black_box(&binary_buffer)).unwrap();
        for &b in f.payload { checksum2 = checksum2.wrapping_add(b as u32); }
    }
    let legacy_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum2);
    println!("  Legacy frame parse: {:.1} ns/req  ({:.2}M req/s)", legacy_ns, 1000.0 / legacy_ns);
}

fn bench_protocol_overhead() {
    println!("\n─── 3. Protocol Overhead: JSON vs NDA-Native (same tool call) ──");

    let tool_call_json = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"hello_world","arguments":{"message":"Hello, World!","count":42,"flag":true}},"id":1}"#;
    let nda_frame = nda_native::build_nda_request(
        nda_native::METHOD_TOOLS_CALL,
        &json!(1),
        &json!({"name": "hello_world", "arguments": {"message": "Hello, World!", "count": 42, "flag": true}}),
    );

    let iterations = 500_000;

    let start = Instant::now();
    let mut json_checksum: u32 = 0;
    for _ in 0..iterations {
        let val: Value = serde_json::from_str(black_box(tool_call_json)).unwrap();
        if let Some(name) = val["params"]["name"].as_str() {
            for b in name.bytes() { json_checksum = json_checksum.wrapping_add(b as u32); }
        }
        if let Some(args) = val["params"]["arguments"].as_object() {
            for (k, v) in args {
                for b in k.bytes() { json_checksum = json_checksum.wrapping_add(b as u32); }
                if let Some(s) = v.as_str() {
                    for b in s.bytes() { json_checksum = json_checksum.wrapping_add(b as u32); }
                }
            }
        }
    }
    let json_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(json_checksum);

    let start = Instant::now();
    let mut nda_checksum: u32 = 0;
    for _ in 0..iterations {
        match nda_native::parse_nda_request(black_box(&nda_frame)) {
            Ok(req) => {
                if let Some(name) = req.data["name"].as_str() {
                    for b in name.bytes() { nda_checksum = nda_checksum.wrapping_add(b as u32); }
                }
                if let Some(args) = req.data["arguments"].as_object() {
                    for (k, v) in args {
                        for b in k.bytes() { nda_checksum = nda_checksum.wrapping_add(b as u32); }
                        if let Some(s) = v.as_str() {
                            for b in s.bytes() { nda_checksum = nda_checksum.wrapping_add(b as u32); }
                        }
                    }
                }
            }
            Err(_) => nda_checksum = nda_checksum.wrapping_add(1),
        }
    }
    let nda_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(nda_checksum);

    println!("  JSON full parse + extract:   {:.1} ns", json_ns);
    println!("  NDA-native parse + extract:  {:.1} ns", nda_ns);
    println!("  NDA speedup:                 {:.1}x faster", json_ns / nda_ns);
    println!("  JSON frame size:             {} bytes", tool_call_json.len());
    println!("  NDA frame size:              {} bytes", nda_frame.len());
    println!("  Size reduction:              {:.1}x smaller", tool_call_json.len() as f64 / nda_frame.len() as f64);
}

fn bench_tlv_encoding() {
    println!("\n─── 4. TLV Binary Encoding ────────────────────────────────────");

    let value = json!({
        "name": "read_nda",
        "arguments": {
            "ndaPath": "C:/Users/me/documents/report.nda",
            "options": {"verbose": true, "format": "detailed"},
            "tags": ["important", "finance", "2026"]
        }
    });

    let iterations = 500_000;

    let start = Instant::now();
    let mut encoded_size = 0;
    for _ in 0..iterations {
        let mut buf = Vec::new();
        nda_native::encode_json_value(black_box(&value), &mut buf);
        encoded_size = buf.len();
    }
    let encode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;

    let mut buf = Vec::new();
    nda_native::encode_json_value(&value, &mut buf);

    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iterations {
        let (decoded, consumed) = nda_native::decode_json_value(black_box(&buf)).unwrap();
        checksum = checksum.wrapping_add(consumed as u32);
        black_box(decoded);
    }
    let decode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum);

    let json_str = serde_json::to_string(&value).unwrap();
    println!("  TLV encode:        {:.1} ns", encode_ns);
    println!("  TLV decode:        {:.1} ns", decode_ns);
    println!("  TLV size:          {} bytes", encoded_size);
    println!("  JSON size:         {} bytes", json_str.len());
    println!("  Size ratio:        {:.1}x", json_str.len() as f64 / encoded_size as f64);

    let start = Instant::now();
    let mut checksum2: u32 = 0;
    for _ in 0..iterations {
        let _: Value = serde_json::from_str(black_box(&json_str)).unwrap();
        checksum2 = checksum2.wrapping_add(1);
    }
    let json_parse_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum2);
    println!("  JSON parse:        {:.1} ns", json_parse_ns);
    println!("  TLV decode speedup: {:.1}x over JSON parse", json_parse_ns / decode_ns);
}

fn bench_shmem_throughput() {
    println!("\n─── 5. Shared Memory Throughput (JSON-in-shmem) ───────────────");

    let path = "temp_bench_shmem.bin";
    let _ = std::fs::remove_file(path);
    let mut buffer = SharedMemoryBuffer::create_or_open(path).unwrap();

    let json_req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"C:/test.nda"}},"id":1}"#;
    let iterations = 200_000;

    println!("  JSON write+read shmem ({} iterations)...", iterations);
    let start = Instant::now();
    for _ in 0..iterations {
        buffer.write_input(black_box(json_req)).unwrap();
        let _ = black_box(buffer.read_input().unwrap());
    }
    let shmem_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  JSON shmem R/W:    {:.1} ns  ({:.2}M ops/s)", shmem_ns, 1000.0 / shmem_ns);

    let _ = std::fs::remove_file(path);
}

fn bench_nda_native_shmem() {
    println!("\n─── 6. Shared Memory Throughput (NDA-native shmem) ────────────");

    let path = "temp_bench_nda_shmem.bin";
    let _ = std::fs::remove_file(path);
    let mut buffer = SharedMemoryBuffer::create_or_open(path).unwrap();

    let nda_frame = nda_native::build_nda_request(
        nda_native::METHOD_TOOLS_CALL,
        &json!(1),
        &json!({"name": "read_nda", "arguments": {"ndaPath": "C:/test.nda"}}),
    );
    let iterations = 200_000;

    println!("  NDA write+read shmem ({} iterations)...", iterations);
    let start = Instant::now();
    for _ in 0..iterations {
        buffer.write_output_raw(black_box(&nda_frame)).unwrap();
        let _ = black_box(buffer.read_input_raw().unwrap());
    }
    let nda_shmem_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  NDA shmem R/W:     {:.1} ns  ({:.2}M ops/s)", nda_shmem_ns, 1000.0 / nda_shmem_ns);

    let _ = std::fs::remove_file(path);
}

fn bench_concurrent_dispatch() {
    println!("\n─── 7. Concurrent Dispatch (multi-threaded) ───────────────────");

    let thread_counts = [1, 2, 4, 8];
    let requests_per_thread = 10_000;

    for &num_threads in &thread_counts {
        let counter = Arc::new(AtomicU64::new(0));
        let start = Instant::now();

        let handles: Vec<_> = (0..num_threads).map(|t| {
            let counter = Arc::clone(&counter);
            thread::spawn(move || {
                let json_req = format!(
                    r#"{{"jsonrpc":"2.0","method":"tools/call","params":{{"name":"read_nda","arguments":{{"ndaPath":"C:/test_{}.nda"}}}},"id":{}}}"#,
                    t, t
                );
                for _ in 0..requests_per_thread {
                    let val: Value = serde_json::from_str(&json_req).unwrap();
                    let _method = val["method"].as_str().unwrap();
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            })
        }).collect();

        for h in handles { h.join().unwrap(); }
        let elapsed = start.elapsed();
        let total = counter.load(Ordering::Relaxed);
        let throughput = total as f64 / elapsed.as_secs_f64();

        println!("  {} thread(s) x {} reqs:  {:>10.0} req/s  ({:.2} ms total)",
            num_threads, requests_per_thread, throughput, elapsed.as_secs_f64() * 1000.0);
    }

    println!("\n  NDA-native concurrent dispatch:");
    for &num_threads in &thread_counts {
        let counter = Arc::new(AtomicU64::new(0));

        let nda_frame = nda_native::build_nda_request(
            nda_native::METHOD_TOOLS_CALL,
            &json!(1),
            &json!({"name": "read_nda", "arguments": {"ndaPath": "C:/test.nda"}}),
        );

        let start = Instant::now();
        let handles: Vec<_> = (0..num_threads).map(|_| {
            let counter = Arc::clone(&counter);
            let frame = nda_frame.clone();
            thread::spawn(move || {
                for _ in 0..requests_per_thread {
                    let _ = nda_native::parse_nda_request(&frame).unwrap();
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            })
        }).collect();

        for h in handles { h.join().unwrap(); }
        let elapsed = start.elapsed();
        let total = counter.load(Ordering::Relaxed);
        let throughput = total as f64 / elapsed.as_secs_f64();

        println!("  {} thread(s) x {} reqs:  {:>10.0} req/s  ({:.2} ms total)",
            num_threads, requests_per_thread, throughput, elapsed.as_secs_f64() * 1000.0);
    }
}

fn bench_e2e_tool_calls() {
    println!("\n─── 8. End-to-End Tool Calls ──────────────────────────────────");

    let csharp_path = registry::resolve_csharp_path();
    if !std::path::Path::new(&csharp_path).exists() {
        println!("  C# engine not found at: {}", csharp_path);
        println!("  Skipping end-to-end benchmarks (set VELOCITY_CSHARP_PATH to enable)");
        return;
    }

    let test_file = "temp_bench_test.txt";
    let test_nda = "temp_bench_test.nda";
    std::fs::write(test_file, "Benchmark test content for NDA conversion.\n").unwrap();

    let iterations = 10;
    let cwd = std::env::current_dir().unwrap();

    println!("  JSON tool call: convert_to_nda_document ({} iterations)...", iterations);
    let start = Instant::now();
    let mut successes = 0;
    for _ in 0..iterations {
        let args = json!({"filePath": format!("{}\\{}", cwd.display(), test_file)});
        match registry::call_tool("convert_to_nda_document", &args) {
            Ok(_) => successes += 1,
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
    let json_ms = start.elapsed().as_millis() as f64 / iterations as f64;
    println!("  Mean: {:.2} ms ({}/{})", json_ms, successes, iterations);

    if std::path::Path::new(test_nda).exists() {
        println!("  NDA tool call: read_nda ({} iterations)...", iterations);
        let start = Instant::now();
        let mut successes = 0;
        for _ in 0..iterations {
            let args = json!({"ndaPath": format!("{}\\{}", cwd.display(), test_nda)});
            match registry::call_tool("read_nda", &args) {
                Ok(_) => successes += 1,
                Err(e) => eprintln!("  Error: {}", e),
            }
        }
        let nda_ms = start.elapsed().as_millis() as f64 / iterations as f64;
        println!("  Mean: {:.2} ms ({}/{})", nda_ms, successes, iterations);
        println!("  Speedup: {:.2}x", json_ms / nda_ms);
    }

    let _ = std::fs::remove_file(test_file);
    let _ = std::fs::remove_file(test_nda);
}
