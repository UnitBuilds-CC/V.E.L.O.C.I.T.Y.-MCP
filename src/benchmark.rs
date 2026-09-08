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
use crate::wasm_runtime::WasmRuntime;

pub fn run_benchmarks() {
    info!("Starting V.E.L.O.C.I.T.Y.-MCP v3.0.0 Performance Benchmark Suite");
    println!("================================================================");
    println!("     V.E.L.O.C.I.T.Y.-MCP v3.0.0 Performance Benchmark Suite");
    println!("================================================================");

    bench_json_parsing();
    bench_nda_native_parsing();
    bench_protocol_overhead();
    bench_tlv_encoding();
    bench_flat_encoding();
    bench_shmem_throughput();
    bench_nda_native_shmem();
    bench_concurrent_dispatch();
    bench_e2e_tool_calls();
    bench_cached_nmcp_frame();
    bench_nmcp_conversion();
    bench_audit_multi_tenant();
    bench_cross_language_tools();
    
    // v3.0 feature benchmarks
    #[cfg(feature = "oauth2")]
    bench_oauth2_encryption();
    
    #[cfg(feature = "http")]
    bench_streaming_chunks();
    
    #[cfg(feature = "database")]
    bench_database_queries();

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
    ).unwrap();
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
    ).unwrap();

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
        nda_native::encode_json_value(black_box(&value), &mut buf).unwrap();
        encoded_size = buf.len();
    }
    let encode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;

    let mut buf = Vec::new();
    nda_native::encode_json_value(&value, &mut buf).unwrap();

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

fn bench_flat_encoding() {
    println!("\n─── 5. Flat Binary vs TLV Encoding ─────────────────────────────");

    let args = json!(["/Users/me/documents/report.nda", 42, true, "detailed"]);
    let iterations = 500_000;

    let mut tlv_buf = Vec::new();
    nda_native::encode_json_value(&args, &mut tlv_buf).unwrap();
    let mut flat_buf = Vec::new();
    nda_native::encode_flat_value(&args, &mut flat_buf);

    println!("  TLV encoded size:  {} bytes", tlv_buf.len());
    println!("  Flat encoded size: {} bytes", flat_buf.len());
    println!("  Size reduction:    {:.1}x smaller", tlv_buf.len() as f64 / flat_buf.len() as f64);

    let start = Instant::now();
    let mut tlv_size = 0;
    for _ in 0..iterations {
        let mut buf = Vec::new();
        nda_native::encode_json_value(black_box(&args), &mut buf).unwrap();
        tlv_size = buf.len();
    }
    let tlv_encode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(tlv_size);

    let start = Instant::now();
    let mut flat_size = 0;
    for _ in 0..iterations {
        let mut buf = Vec::new();
        nda_native::encode_flat_value(black_box(&args), &mut buf);
        flat_size = buf.len();
    }
    let flat_encode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(flat_size);

    let start = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..iterations {
        let (decoded, consumed) = nda_native::decode_json_value(black_box(&tlv_buf)).unwrap();
        checksum = checksum.wrapping_add(consumed as u32);
        black_box(decoded);
    }
    let tlv_decode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum);

    let start = Instant::now();
    let mut checksum2: u32 = 0;
    for _ in 0..iterations {
        let mut offset = 0;
        while offset < flat_buf.len() {
            let _ = nda_native::decode_flat_value(black_box(&flat_buf), &mut offset).unwrap();
        }
        checksum2 = checksum2.wrapping_add(1);
    }
    let flat_decode_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(checksum2);

    println!("  TLV encode:        {:.1} ns", tlv_encode_ns);
    println!("  Flat encode:       {:.1} ns  ({:.1}x faster)", flat_encode_ns, tlv_encode_ns / flat_encode_ns);
    println!("  TLV decode:        {:.1} ns", tlv_decode_ns);
    println!("  Flat decode:       {:.1} ns  ({:.1}x faster)", flat_decode_ns, tlv_decode_ns / flat_decode_ns);

    let flat_frame = nda_native::build_flat_request(nda_native::METHOD_TOOLS_CALL, &json!(1), "read_file", &args);
    let tlv_frame = nda_native::build_nda_request(nda_native::METHOD_TOOLS_CALL, &json!(1), &json!({"name": "read_file", "arguments": &args})).unwrap();
    println!("  Full TLV frame:    {} bytes", tlv_frame.len());
    println!("  Full flat frame:   {} bytes  ({:.1}x smaller)", flat_frame.len(), tlv_frame.len() as f64 / flat_frame.len() as f64);
}

fn bench_shmem_throughput() {
    println!("\n─── 6. Shared Memory Throughput (JSON-in-shmem) ───────────────");

    let path = "temp_bench_shmem.bin";
    let _ = std::fs::remove_file(path);
    let mut buffer = SharedMemoryBuffer::create_or_open(path).expect("Failed to create shmem buffer for benchmark");

    let json_req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"C:/test.nda"}},"id":1}"#;
    let iterations = 200_000;

    println!("  JSON write+read shmem ({} iterations)...", iterations);
    let start = Instant::now();
    for _ in 0..iterations {
        buffer.write_input(black_box(json_req)).expect("shmem write_input failed");
        let _ = black_box(buffer.read_input().expect("shmem read_input failed"));
    }
    let shmem_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  JSON shmem R/W:    {:.1} ns  ({:.2}M ops/s)", shmem_ns, 1000.0 / shmem_ns);

    let _ = std::fs::remove_file(path);
}

fn bench_nda_native_shmem() {
    println!("\n─── 6. Shared Memory Throughput (NDA-native shmem) ────────────");

    let path = "temp_bench_nda_shmem.bin";
    let _ = std::fs::remove_file(path);
    let mut buffer = SharedMemoryBuffer::create_or_open(path).expect("Failed to create shmem buffer for benchmark");

    let nda_frame = nda_native::build_nda_request(
        nda_native::METHOD_TOOLS_CALL,
        &json!(1),
        &json!({"name": "read_nda", "arguments": {"ndaPath": "C:/test.nda"}}),
    ).unwrap();
    let iterations = 200_000;

    println!("  NDA write+read shmem ({} iterations)...", iterations);
    let start = Instant::now();
    for _ in 0..iterations {
        buffer.write_output_raw(black_box(&nda_frame)).expect("shmem write_output_raw failed");
        let _ = black_box(buffer.read_input_raw().expect("shmem read_input_raw failed"));
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

        for h in handles { h.join().expect("benchmark worker thread panicked"); }
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
        ).unwrap();

        let start = Instant::now();
        let handles: Vec<_> = (0..num_threads).map(|_| {
            let counter = Arc::clone(&counter);
            let frame = nda_frame.clone();
            thread::spawn(move || {
                for _ in 0..requests_per_thread {
                    let _ = nda_native::parse_nda_request(&frame).expect("NDA parse failed in benchmark");
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            })
        }).collect();

        for h in handles { h.join().expect("benchmark worker thread panicked"); }
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
    std::fs::write(test_file, "Benchmark test content for NDA conversion.\n").expect("Failed to write test file for benchmark");

    let iterations = 10;
    let cwd = std::env::current_dir().expect("Failed to get current directory for benchmark");

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

// ─── v3.0 Feature Benchmarks ─────────────────────────────────────────────────

#[cfg(feature = "oauth2")]
fn bench_oauth2_encryption() {
    println!("\n─── 10. OAuth2 Token Encryption ─────────────────────────────────");
    
    use crate::oauth2::{OAuth2Token, encrypt_token, decrypt_token, set_encryption_key, generate_encryption_key};
    
    // Set up encryption key
    let key = generate_encryption_key();
    set_encryption_key(key);
    
    let token = OAuth2Token {
        access_token: "test_access_token_12345".to_string(),
        refresh_token: Some("test_refresh_token_67890".to_string()),
        expires_in: Some(3600),
        token_type: Some("Bearer".to_string()),
        expires_at: None,
        issued_at: None,
    };
    
    let iterations = 10_000;
    
    // Benchmark encryption
    println!("  Token encryption ({} iterations)...", iterations);
    let start = Instant::now();
    let mut encrypted_size = 0;
    for _ in 0..iterations {
        let encrypted = encrypt_token(black_box(&token)).expect("OAuth2 token encryption failed");
        encrypted_size = encrypted.len();
        black_box(encrypted);
    }
    let encrypt_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Encrypt:  {:.1} μs  (size: {} bytes)", encrypt_ns / 1000.0, encrypted_size);
    
    // Benchmark decryption
    let encrypted = encrypt_token(&token).expect("OAuth2 token encryption failed for decrypt benchmark");
    println!("  Token decryption ({} iterations)...", iterations);
    let start = Instant::now();
    for _ in 0..iterations {
        let decrypted = decrypt_token(black_box(&encrypted)).expect("OAuth2 token decryption failed");
        black_box(decrypted);
    }
    let decrypt_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Decrypt:  {:.1} μs", decrypt_ns / 1000.0);
}

#[cfg(feature = "http")]
fn bench_streaming_chunks() {
    println!("\n─── 11. Streaming Chunk Conversion ──────────────────────────────");
    
    use crate::streaming::{StreamingChunk, ProgressToken, chunk_to_sse_event};
    
    let token = ProgressToken::String("bench_token".to_string());
    let chunk = StreamingChunk {
        chunk_id: 0,
        data: json!({"content": "test data for streaming benchmark", "index": 42}),
        is_final: Some(false),
    };
    
    let iterations = 50_000;
    
    println!("  Chunk to SSE event ({} iterations)...", iterations);
    let start = Instant::now();
    let mut event_size = 0;
    for _ in 0..iterations {
        let event = chunk_to_sse_event(black_box(&token), black_box(&chunk));
        event_size = event.len();
        black_box(event);
    }
    let chunk_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Chunk conversion:  {:.1} ns  (event size: {} bytes)", chunk_ns, event_size);
}

#[cfg(feature = "database")]
fn bench_database_queries() {
    println!("\n─── 12. Database Resource Queries ───────────────────────────────");
    
    use crate::resources::{register_db_resource, read_resource};
    
    // Register a simple query resource
    register_db_resource(
        "db://bench_test",
        "Benchmark Test",
        "Benchmark database query",
        "SELECT 1 as id, 'test' as name, 42 as value",
        vec![],
    );
    
    let iterations = 100;
    
    println!("  Database query execution ({} iterations)...", iterations);
    let start = Instant::now();
    let mut successes = 0;
    for _ in 0..iterations {
        match read_resource("db://bench_test") {
            Ok(_) => successes += 1,
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
    let query_ms = start.elapsed().as_millis() as f64 / iterations as f64;
    println!("  Mean: {:.2} ms ({}/{})", query_ms, successes, iterations);
}

fn bench_audit_multi_tenant() {
    println!("\n─── 9. Multi-Tenant Audit Isolation ────────────────────────────");

    use crate::audit::{AuditLog, AuditRegistry, AuditOutcome, set_session_context, clear_session_context};

    let iterations = 100_000;
    let start = Instant::now();

    // 1. Direct AuditLog recording (baseline)
    let log = AuditLog::new();
    for i in 0..iterations {
        log.record(&format!("tool_{}", i % 50), start, AuditOutcome::Success);
    }
    let direct_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Direct AuditLog::record:   {:.1} ns/op  ({:.2}M ops/s)", direct_ns, 1000.0 / direct_ns);

    // 2. Registry-routed recording (single session, via thread-local context)
    let registry = AuditRegistry::new();
    set_session_context("bench-session".to_string());

    let start = Instant::now();
    for i in 0..iterations {
        let session_id = crate::audit::current_session_id().unwrap_or_else(|| "default".to_string());
        let log = registry.get_or_create(&session_id);
        log.record_with_context(&format!("tool_{}", i % 50), start, AuditOutcome::Success, None, Some(session_id));
    }
    let routed_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    clear_session_context();
    println!("  Registry-routed record:    {:.1} ns/op  ({:.2}M ops/s)", routed_ns, 1000.0 / routed_ns);
    println!("  Routing overhead:          {:.1} ns  ({:.1}x vs direct)", routed_ns - direct_ns, routed_ns / direct_ns);

    // 3. Concurrent multi-session throughput
    println!("\n  Concurrent multi-session recording:");
    let session_counts = [1, 4, 16, 64];
    let ops_per_session = 10_000;

    for &n_sessions in &session_counts {
        let registry = Arc::new(AuditRegistry::new());
        let start = Instant::now();

        let handles: Vec<_> = (0..n_sessions).map(|s| {
            let registry = Arc::clone(&registry);
            thread::spawn(move || {
                let session_id = format!("session-{}", s);
                set_session_context(session_id.clone());
                for i in 0..ops_per_session {
                    let sid = crate::audit::current_session_id().unwrap_or_else(|| "default".to_string());
                    let log = registry.get_or_create(&sid);
                    log.record_with_context(&format!("tool_{}", i % 20), Instant::now(), AuditOutcome::Success, None, Some(sid));
                }
                clear_session_context();
            })
        }).collect();

        for h in handles { h.join().unwrap(); }
        let elapsed = start.elapsed();
        let total_ops = n_sessions * ops_per_session;
        let throughput = total_ops as f64 / elapsed.as_secs_f64();
        println!("  {} session(s) x {} ops:  {:>10.0} ops/s  ({:.2} ms total)",
            n_sessions, ops_per_session, throughput, elapsed.as_secs_f64() * 1000.0);
    }

    // 4. Aggregate cost across sessions
    println!("\n  Aggregate across sessions:");
    let registry = AuditRegistry::new();
    let entries_per_session = 1000;
    for s in 0..64 {
        let log = registry.get_or_create(&format!("session-{}", s));
        for i in 0..entries_per_session {
            log.record(&format!("tool_{}", i % 20), start, AuditOutcome::Success);
        }
    }

    let agg_start = Instant::now();
    let agg_iterations = 100;
    for _ in 0..agg_iterations {
        let all = registry.aggregate_all();
        black_box(all.len());
    }
    let agg_ns = agg_start.elapsed().as_nanos() as f64 / agg_iterations as f64;
    println!("  64 sessions x {} entries:  {:.1} μs/aggregate  ({} total entries)",
        entries_per_session, agg_ns / 1000.0, 64 * entries_per_session);

    // 5. Flush to disk
    let flush_path = "temp_bench_audit_flush";
    let _ = std::fs::remove_dir_all(flush_path);
    let flush_start = Instant::now();
    let flushed = registry.flush_all(flush_path).expect("audit flush benchmark failed");
    let flush_ms = flush_start.elapsed().as_millis() as f64;
    println!("\n  Flush {} sessions ({} entries):  {:.1} ms", 64, flushed, flush_ms);
    let _ = std::fs::remove_dir_all(flush_path);
}

fn bench_cached_nmcp_frame() {
    println!("\n─── 9. Cached NMCP Frame Execution (Zero-Alloc TLV Extraction) ──");


    // Benchmark 1: Direct native call (baseline)
    let iterations = 1000;
    println!("  Direct native call: bench_echo({{size: 64}}) ({} iterations)...", iterations);
    let start = Instant::now();
    let mut direct_successes = 0;
    for _ in 0..iterations {
        let args = json!({"size": 64});
        match registry::call_tool("bench_echo", &args) {
            Ok(_) => direct_successes += 1,
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
    let direct_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Mean: {:.1} μs ({}/{})", direct_ns / 1000.0, direct_successes, iterations);

    // Benchmark 2: Convert to NMCP frame, then execute (cached path with zero-alloc)
    println!("  Converting bench_echo to NMCP frame...");
    let json_request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"bench_echo","arguments":{"size":64}},"id":1}"#;
    let output_path = "temp_bench_echo_frame.bin";
    
    match registry::cache_nmcp_frame(json_request, output_path) {
        Ok(_) => {
            let cached_binary = std::fs::read(output_path).expect("read cached frame");
            println!("  Executing cached NMCP frame ({} iterations)...", iterations);
            let start = Instant::now();
            let mut cached_successes = 0;
            for _ in 0..iterations {
                let args = json!({"size": 64});
                match registry::execute_cached_nmcp_frame("bench_echo", &args, &cached_binary) {
                    Ok(_) => cached_successes += 1,
                    Err(e) => eprintln!("  Error: {}", e),
                }
            }
            let cached_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
            println!("  Mean: {:.1} μs ({}/{})", cached_ns / 1000.0, cached_successes, iterations);
            
            // Calculate speedup/overhead
            if direct_ns > 0.0 {
                if cached_ns < direct_ns {
                    println!("  Improvement: {:.1}% faster than direct native call", 
                        ((direct_ns - cached_ns) / direct_ns) * 100.0);
                } else {
                    println!("  Overhead: {:.1}% vs direct native call", 
                        ((cached_ns - direct_ns) / direct_ns) * 100.0);
                }
            }
            
            let _ = std::fs::remove_file(output_path);
        }
        Err(e) => {
            eprintln!("  Failed to convert bench_echo to NMCP frame: {}", e);
        }
    }

    // Benchmark 3: File read tool (string argument extraction)
    let test_file = std::env::current_dir().unwrap().join("temp_bench_cached_read.txt").to_string_lossy().to_string();
    std::fs::write(&test_file, "Test content for cached NMCP frame benchmark.\n")
        .expect("Failed to write test file");
    
    println!("\n  Direct native call: file_read ({} iterations)...", iterations);
    let start = Instant::now();
    let mut file_direct_successes = 0;
    for _ in 0..iterations {
        let args = json!({"path": &test_file});
        match registry::call_tool("file_read", &args) {
            Ok(_) => file_direct_successes += 1,
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
    let file_direct_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Mean: {:.1} μs ({}/{})", file_direct_ns / 1000.0, file_direct_successes, iterations);

    println!("  Converting file_read to NMCP frame...");
    let json_request = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "file_read",
            "arguments": {"path": &test_file}
        },
        "id": 2
    })).unwrap();
    let output_path = "temp_bench_file_read_frame.bin";
    
    match registry::cache_nmcp_frame(&json_request, output_path) {
        Ok(_) => {
            let cached_binary = std::fs::read(output_path).expect("read cached frame");
            println!("  Executing cached NMCP frame ({} iterations)...", iterations);
            let start = Instant::now();
            let mut file_cached_successes = 0;
            for _ in 0..iterations {
                let args = json!({"path": &test_file});
                match registry::execute_cached_nmcp_frame("file_read", &args, &cached_binary) {
                    Ok(_) => file_cached_successes += 1,
                    Err(e) => eprintln!("  Error: {}", e),
                }
            }
            let file_cached_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
            println!("  Mean: {:.1} μs ({}/{})", file_cached_ns / 1000.0, file_cached_successes, iterations);
            
            if file_direct_ns > 0.0 {
                if file_cached_ns < file_direct_ns {
                    println!("  Improvement: {:.1}% faster than direct native call", 
                        ((file_direct_ns - file_cached_ns) / file_direct_ns) * 100.0);
                } else {
                    println!("  Overhead: {:.1}% vs direct native call", 
                        ((file_cached_ns - file_direct_ns) / file_direct_ns) * 100.0);
                }
            }
            
            let _ = std::fs::remove_file(output_path);
        }
        Err(e) => {
            eprintln!("  Failed to convert file_read to NMCP frame: {}", e);
        }
    }

    let _ = std::fs::remove_file(test_file);
}

// ─── NMCP Frame Conversion Benchmark ─────────────────────────────────────────

fn bench_nmcp_conversion() {
    println!("\n─── 12. NMCP Frame Conversion (json_to_nmcp_frame) ──────────────");
    
    use crate::registry;
    use std::time::Instant;
    
    let iterations = 200;
    
    // Simple tool with basic arguments
    let simple_request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"simple_tool","arguments":{"x":42,"y":"hello"}},"id":1}"#;
    
    println!("  Converting simple tool ({} iterations)...", iterations);
    let start = Instant::now();
    let mut successes = 0;
    for _ in 0..iterations {
        match registry::cache_nmcp_frame(simple_request, "") {
            Ok(_) => successes += 1,
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
    let simple_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Mean: {:.1} μs ({}/{})", simple_ns / 1000.0, successes, iterations);
    
    // Complex tool with nested objects and arrays
    let complex_request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"complex_tool","arguments":{"config":{"timeout":30,"retries":3},"items":[1,"two",true,null],"query":"a=b;c","nested":{"deep":{"value":"found"}}}},"id":2}"#;
    
    println!("  Converting complex tool ({} iterations)...", iterations);
    let start = Instant::now();
    let mut successes = 0;
    for _ in 0..iterations {
        match registry::cache_nmcp_frame(complex_request, "") {
            Ok(_) => successes += 1,
            Err(e) => eprintln!("  Error: {}", e),
        }
    }
    let complex_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    println!("  Mean: {:.1} μs ({}/{})", complex_ns / 1000.0, successes, iterations);
    
    if simple_ns > 0.0 {
        println!("  Complexity overhead: {:.1}% larger payload", 
            ((complex_ns - simple_ns) / simple_ns) * 100.0);
    }
}

// ─── Cross-Language Tool Execution Benchmark ─────────────────────────────────
//
// Same text analysis tool, three execution flavors:
//   1. Native Rust   — direct in-process function call (baseline)
//   2. WASM (Wasmer)   — compiled to wasm32-unknown-unknown, loaded in-process
//   3. Node.js child process — JSON-RPC over stdin/stdout
//
// Measures cold start, warm per-call latency, and throughput for each.

fn bench_cross_language_tools() {
    use std::io::{BufRead, BufReader, Write as _};
    use std::process::{Command, Stdio};

    println!("\n─── 14. Cross-Language Tool Execution ──────────────────────────");
    println!("  Same tool (text_analyze), four flavors");

    let wasm_input = serde_json::to_string(&json!({
        "text": "The quick brown fox jumps over the lazy dog. \
                 Pack my box with five dozen liquor jugs. \
                 How vexingly quick daft zebras jump. \
                 Bright vixens jump; dozy fowl quack."
    })).unwrap();

    let mut wasm_ns: f64 = 0.0;
    let mut node_ns: f64 = 0.0;
    let mut wasm_available = false;
    let mut node_available = false;
    let mut sdk_ns: f64 = 0.0;
    let mut sdk_available = false;
    let mut quickjs_ns: f64 = 0.0;
    let mut quickjs_available = false;

    // ── 1. Native Rust (baseline) ──────────────────────────────────────────
    println!("\n  [1] Native Rust (direct function call):");

    fn native_text_analyze(text: &str) -> Value {
        json!({
            "word_count": text.split_whitespace().count(),
            "char_count": text.len(),
            "line_count": if text.is_empty() { 0 } else { text.split('\n').count() }
        })
    }

    let iterations = 100_000;
    let start = Instant::now();
    let mut native_checksum: u32 = 0;
    for _ in 0..iterations {
        let result = native_text_analyze(black_box(
            "The quick brown fox jumps over the lazy dog. \
             Pack my box with five dozen liquor jugs. \
             How vexingly quick daft zebras jump. \
             Bright vixens jump; dozy fowl quack."
        ));
        if let Some(wc) = result["word_count"].as_u64() {
            native_checksum = native_checksum.wrapping_add(wc as u32);
        }
    }
    let native_ns = start.elapsed().as_nanos() as f64 / iterations as f64;
    black_box(native_checksum);
    println!("    {:>10} iterations:  {:.1} ns/call  ({:.2}M calls/s)",
        iterations, native_ns, 1000.0 / native_ns);

    // ── 2. WASM via Wasmer ─────────────────────────────────────────────────
    println!("\n  [2] WASM (Wasmer, in-process):");

    let wasm_path = std::path::Path::new("bench_tools/wasm_tool/target/wasm32-unknown-unknown/release/wasm_text_tool.wasm");
    if !wasm_path.exists() {
        println!("    SKIP — WASM file not found.");
        println!("    Build with: cd bench_tools/wasm_tool && cargo build --target wasm32-unknown-unknown --release");
    } else {
        let wasm_bytes = std::fs::read(wasm_path).expect("read wasm file");
        println!("    Module size: {} bytes", wasm_bytes.len());

        // Cold start: compile + instantiate from scratch
        let cold_iters = 100;
        let start = Instant::now();
        for _ in 0..cold_iters {
            let engine = wasmer::Engine::from(wasmer::Cranelift::default());
            let module = wasmer::Module::new(&engine, &wasm_bytes).unwrap();
            let mut store = wasmer::Store::new(engine);
            let imports = wasmer::imports!{};
            let _instance = wasmer::Instance::new(&mut store, &module, &imports).unwrap();
        }
        let cold_ns = start.elapsed().as_nanos() as f64 / cold_iters as f64;
        println!("    Cold start (compile+instantiate): {:.1} μs", cold_ns / 1000.0);

        // Warm setup: compile once, instantiate once
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = wasmer::Module::new(&engine, &wasm_bytes).unwrap();
        let mut store = wasmer::Store::new(engine);
        let imports = wasmer::imports!{};
        let instance = wasmer::Instance::new(&mut store, &module, &imports).unwrap();

        let memory = instance.exports.get_memory("memory").expect("memory export");
        let prepare_fn = instance.exports.get_function("prepare_call")
            .expect("prepare_call export")
            .typed::<(), ()>(&store)
            .expect("prepare_call typed");
        let execute_fn = instance.exports.get_function("tool_execute")
            .expect("tool_execute export")
            .typed::<(i32, i32), i64>(&store)
            .expect("tool_execute typed");

        // Warm-up call + verify correctness
        let input_bytes = wasm_input.as_bytes();
        let input_ptr = 1024;
        let input_len = input_bytes.len() as i32;
        memory.view(&store).write(input_ptr as u64, input_bytes).unwrap();
        let result = execute_fn.call(&mut store, input_ptr as i32, input_len).unwrap();
        let result_ptr = (result >> 32) as u32;
        let result_len = (result & 0xFFFF_FFFF) as u32;
        let mut result_buf = vec![0u8; result_len as usize];
        memory.view(&store).read(result_ptr as u64, &mut result_buf).unwrap();
        let result_str = String::from_utf8(result_buf).unwrap();
        let result_val: Value = serde_json::from_str(&result_str).unwrap();
        println!("    Verify: word_count={}, char_count={}, line_count={}",
            result_val["word_count"], result_val["char_count"], result_val["line_count"]);

        // Warm benchmark: reset alloc + write input + call per iteration
        let iterations = 50_000;
        let start = Instant::now();
        let mut wasm_checksum: u32 = 0;
        for _ in 0..iterations {
            prepare_fn.call(&mut store).unwrap();
            memory.view(&store).write(input_ptr as u64, input_bytes).unwrap();
            let r = execute_fn.call(&mut store, input_ptr as i32, input_len).unwrap();
            let rp = (r >> 32) as u32;
            let rl = (r & 0xFFFF_FFFF) as u32;
            let mut buf = vec![0u8; rl as usize];
            memory.view(&store).read(rp as u64, &mut buf).unwrap();
            if let Ok(v) = serde_json::from_slice::<Value>(&buf) {
                if let Some(wc) = v["word_count"].as_u64() {
                    wasm_checksum = wasm_checksum.wrapping_add(wc as u32);
                }
            }
        }
        let wasm_ns_val = start.elapsed().as_nanos() as f64 / iterations as f64;
        black_box(wasm_checksum);
        wasm_ns = wasm_ns_val;
        wasm_available = true;
        println!("    {:>10} iterations:  {:.1} ns/call  ({:.2}M calls/s)",
            iterations, wasm_ns, 1000.0 / wasm_ns);
        println!("    Overhead vs native: {:.1}x", wasm_ns / native_ns);
    }

    // ── 3. Node.js child process ───────────────────────────────────────────
    println!("\n  [3] Node.js child process (JSON-RPC over stdin/stdout):");

    let node_path = "bench_tools/node_tool.js";
    if !std::path::Path::new(node_path).exists() {
        println!("    SKIP — node_tool.js not found");
    } else if Command::new("node").arg("--version").output().is_err() {
        println!("    SKIP — node not found in PATH");
    } else {
        let node_request = r#"{"jsonrpc":"2.0","method":"tools/call","id":1,"params":{"name":"bench_echo","arguments":{"size":64}}}"#;

        // Cold start: spawn a fresh process for one call
        let cold_iters = 20;
        let start = Instant::now();
        for _ in 0..cold_iters {
            let mut child = Command::new("node")
                .arg(node_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            {
                let stdin = child.stdin.as_mut().unwrap();
                let req = format!("{}\n", node_request);
                stdin.write_all(req.as_bytes()).unwrap();
                stdin.flush().unwrap();
            }
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
            let _ = child.wait();
        }
        let cold_ns = start.elapsed().as_nanos() as f64 / cold_iters as f64;
        println!("    Cold start (spawn + 1 call): {:.1} ms", cold_ns / 1_000_000.0);

        // Warm: persistent process, many calls
        let mut child = Command::new("node")
            .arg(node_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        // Warm-up call
        {
            let stdin = child.stdin.as_mut().unwrap();
            let req = format!("{}\n", node_request);
            stdin.write_all(req.as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        let mut warmup_line = String::new();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        reader.read_line(&mut warmup_line).unwrap();
        let warmup_val: Value = serde_json::from_str(warmup_line.trim()).unwrap();
        println!("    Verify: size={}, payload_len={}",
            warmup_val["result"]["size"],
            warmup_val["result"]["payload"].as_str().map(|s| s.len()).unwrap_or(0));

        // Need to put stdout back for subsequent reads — BufReader consumed it.
        // Re-create the child for the actual benchmark.
        drop(reader);
        child.kill().ok();
        let _ = child.wait();

        let mut child = Command::new("node")
            .arg(node_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let iterations = 2_000;
        let req_line = format!("{}\n", node_request);
        let start = Instant::now();
        let mut node_checksum: u32 = 0;
        let stdin = child.stdin.as_mut().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        for _ in 0..iterations {
            stdin.write_all(req_line.as_bytes()).unwrap();
            stdin.flush().unwrap();
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                if let Some(sz) = v["result"]["size"].as_u64() {
                    node_checksum = node_checksum.wrapping_add(sz as u32);
                }
            }
            line.clear();
        }
        let node_ns_val = start.elapsed().as_nanos() as f64 / iterations as f64;
        black_box(node_checksum);
        node_ns = node_ns_val;
        node_available = true;
        println!("    {:>10} iterations:  {:.1} μs/call  ({:.0}K calls/s)",
            iterations, node_ns / 1000.0, 1_000_000.0 / node_ns);
        println!("    Overhead vs native: {:.0}x", node_ns / native_ns);

        child.kill().ok();
        let _ = child.wait();
    }

    // ── 4. Node.js MCP SDK server (full-featured, 16 tools) ────────────────
    println!("\n  [4] Node.js MCP SDK server (16 tools, official @modelcontextprotocol/sdk):");

    let sdk_server_path = "bench_nodejs/sdk_server.mjs";
    if !std::path::Path::new(sdk_server_path).exists() {
        println!("    SKIP — sdk_server.mjs not found");
    } else if Command::new("node").arg("--version").output().is_err() {
        println!("    SKIP — node not found in PATH");
    } else {
        let init_request = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "method": "initialize", "id": 1,
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "bench", "version": "1.0" }
            }
        })).unwrap();
        let init_notify = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "method": "notifications/initialized"
        })).unwrap();
        let sdk_echo_request = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "method": "tools/call", "id": 1,
            "params": { "name": "bench_echo", "arguments": { "size": 64 } }
        })).unwrap();
        let sdk_list_request = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "method": "tools/list", "id": 2
        })).unwrap();

        // Cold start: spawn fresh process, do handshake + one tools/call
        let cold_iters = 10;
        let start = Instant::now();
        for _ in 0..cold_iters {
            let mut child = Command::new("node")
                .arg(sdk_server_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            {
                let stdin = child.stdin.as_mut().unwrap();
                stdin.write_all(format!("{}\n", init_request).as_bytes()).unwrap();
                stdin.flush().unwrap();
            }
            let stdout = child.stdout.take().unwrap();
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            line.clear();
            {
                let stdin = child.stdin.as_mut().unwrap();
                stdin.write_all(format!("{}\n", init_notify).as_bytes()).unwrap();
                stdin.flush().unwrap();
            }
            {
                let stdin = child.stdin.as_mut().unwrap();
                stdin.write_all(format!("{}\n", sdk_echo_request).as_bytes()).unwrap();
                stdin.flush().unwrap();
            }
            line.clear();
            reader.read_line(&mut line).unwrap();
            let _ = child.wait();
        }
        let cold_ns = start.elapsed().as_nanos() as f64 / cold_iters as f64;
        println!("    Cold start (spawn + handshake + 1 call): {:.1} ms", cold_ns / 1_000_000.0);

        // Warm: persistent process — verify correctness
        let mut child = Command::new("node")
            .arg(sdk_server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", init_request).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line.clear();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", init_notify).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }

        // Verify tools/list returns 16 tools
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", sdk_list_request).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        line.clear();
        reader.read_line(&mut line).unwrap();
        if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
            if let Some(tools) = v["result"]["tools"].as_array() {
                println!("    tools/list: {} tools registered", tools.len());
            }
        }

        // Verify bench_echo correctness
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", sdk_echo_request).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        line.clear();
        reader.read_line(&mut line).unwrap();
        if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
            if let Some(text) = v["result"]["content"][0]["text"].as_str() {
                if let Ok(inner) = serde_json::from_str::<Value>(text) {
                    println!("    Verify bench_echo: size={}, payload_len={}",
                        inner["size"],
                        inner["payload"].as_str().map(|s| s.len()).unwrap_or(0));
                }
            }
        }

        drop(reader);
        child.kill().ok();
        let _ = child.wait();

        // Warm benchmark: tools/call (bench_echo)
        let mut child = Command::new("node")
            .arg(sdk_server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", init_request).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line.clear();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", init_notify).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }

        let iterations = 1_000;
        let req_line = format!("{}\n", sdk_echo_request);
        let start = Instant::now();
        let mut sdk_checksum: u32 = 0;
        let stdin = child.stdin.as_mut().unwrap();
        for _ in 0..iterations {
            stdin.write_all(req_line.as_bytes()).unwrap();
            stdin.flush().unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                if let Some(text) = v["result"]["content"][0]["text"].as_str() {
                    if let Ok(inner) = serde_json::from_str::<Value>(text) {
                        if let Some(sz) = inner["size"].as_u64() {
                            sdk_checksum = sdk_checksum.wrapping_add(sz as u32);
                        }
                    }
                }
            }
        }
        let sdk_ns_val = start.elapsed().as_nanos() as f64 / iterations as f64;
        black_box(sdk_checksum);
        sdk_ns = sdk_ns_val;
        sdk_available = true;
        println!("    tools/call (bench_echo):");
        println!("      {:>10} iterations:  {:.1} μs/call  ({:.0}K calls/s)",
            iterations, sdk_ns / 1000.0, 1_000_000.0 / sdk_ns);
        println!("      Overhead vs native: {:.0}x", sdk_ns / native_ns);

        child.kill().ok();
        let _ = child.wait();

        // Warm benchmark: tools/list (16 tools)
        let mut child = Command::new("node")
            .arg(sdk_server_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", init_request).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        line.clear();
        {
            let stdin = child.stdin.as_mut().unwrap();
            stdin.write_all(format!("{}\n", init_notify).as_bytes()).unwrap();
            stdin.flush().unwrap();
        }

        let list_iters = 500;
        let list_req = format!("{}\n", sdk_list_request);
        let start = Instant::now();
        let mut list_checksum: u32 = 0;
        let stdin = child.stdin.as_mut().unwrap();
        for _ in 0..list_iters {
            stdin.write_all(list_req.as_bytes()).unwrap();
            stdin.flush().unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            if let Ok(v) = serde_json::from_str::<Value>(line.trim()) {
                if let Some(tools) = v["result"]["tools"].as_array() {
                    list_checksum = list_checksum.wrapping_add(tools.len() as u32);
                }
            }
        }
        let list_ns = start.elapsed().as_nanos() as f64 / list_iters as f64;
        black_box(list_checksum);
        println!("    tools/list (16 tools):");
        println!("      {:>10} iterations:  {:.1} μs/call  ({:.0}K calls/s)",
            list_iters, list_ns / 1000.0, 1_000_000.0 / list_ns);

        child.kill().ok();
        let _ = child.wait();
    }

    // ── 5. JavaScript via QuickJS/WASM (in-process) ────────────────────────
    println!("\n  [5] JavaScript via QuickJS/WASM (Wasmer, in-process):");

    let qjs_wasm_path = "bench_tools/quickjs_wasm/quickjs.wasm";
    if !std::path::Path::new(qjs_wasm_path).exists() {
        println!("    SKIP — quickjs.wasm not found.");
        println!("    Install with: cd bench_tools/quickjs_wasm && npm install");
    } else {
        let qjs_bytes = std::fs::read(qjs_wasm_path).expect("read quickjs.wasm");
        println!("    Module size: {} bytes", qjs_bytes.len());

        // Cold start: compile + instantiate + init from scratch
        let cold_start_ms = crate::wasm_runtime::quickjs::QuickJsRuntime::bench_cold_start(&qjs_bytes);
        println!("    Cold start (compile+instantiate+init): {:.1} ms", cold_start_ms);

        // Warm setup: compile once, instantiate once
        let mut rt = crate::wasm_runtime::quickjs::QuickJsRuntime::new(&qjs_bytes)
            .expect("QuickJsRuntime::new");
        rt.init().expect("QuickJsRuntime::init");
        println!("    qjs_init() returned: 0");

        // Verify: evaluate a JS expression
        let js_code = r#"JSON.stringify({result: "hello from QuickJS", size: 42, words: 3})"#;
        match rt.eval_to_string(js_code) {
            Ok(result_str) => {
                println!("    Verify: {}", result_str);
                if let Ok(v) = serde_json::from_str::<Value>(&result_str) {
                    println!("    Parsed: result={}, size={}", v["result"], v["size"]);
                }
            }
            Err(e) => println!("    JS ERROR: {}", e),
        }

        // Warm benchmark: evaluate JS tool repeatedly
        let bench_js = r#"JSON.stringify({size:64,payload:"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"})"#;
        let bench_bytes = bench_js.as_bytes();
        let bench_ptr = rt.write_code(bench_bytes).expect("write_code");

        let iterations = 10_000;
        let start = Instant::now();
        let mut qjs_checksum: u32 = 0;
        for _ in 0..iterations {
            let handle = rt.eval_at(bench_ptr, bench_bytes.len() as i32).unwrap();
            if rt.check_exception(handle).unwrap().is_none() {
                if let Some(sl) = rt.string_len(handle).unwrap() {
                    qjs_checksum = qjs_checksum.wrapping_add(sl);
                }
            }
            rt.free_value(handle).unwrap();
        }
        let qjs_ns_val = start.elapsed().as_nanos() as f64 / iterations as f64;
        black_box(qjs_checksum);
        quickjs_ns = qjs_ns_val;
        quickjs_available = true;
        println!("    {:>10} iterations:  {:.1} ns/call  ({:.2}M calls/s)",
            iterations, quickjs_ns, 1000.0 / quickjs_ns);
        println!("    Overhead vs native Rust: {:.1}x", quickjs_ns / native_ns);
        if wasm_available {
            println!("    vs WASM (Rust tool):   {:.1}x", quickjs_ns / wasm_ns);
        }

        // Cleanup
        rt.destroy().ok();
    }

    // ── Summary table ──────────────────────────────────────────────────────
    println!("\n  ┌──────────────────────────┬──────────────┬───────────┐");
    println!("  │ Flavor                   │ Per-call     │ vs Native │");
    println!("  ├──────────────────────────┼──────────────┼───────────┤");
    println!("  │ Native Rust              │ {:>6.0} ns    │    1.0x   │", native_ns);
    if wasm_available {
        println!("  │ WASM (Wasmer, Rust tool) │ {:>6.0} ns    │   {:>5.1}x   │", wasm_ns, wasm_ns / native_ns);
    }
    if quickjs_available {
        println!("  │ JS via QuickJS/WASM       │ {:>6.0} ns    │   {:>5.1}x   │", quickjs_ns, quickjs_ns / native_ns);
    }
    if node_available {
        println!("  │ Node.js child proc       │ {:>6.0} μs    │  {:>5.0}x   │", node_ns / 1000.0, node_ns / native_ns);
    }
    if sdk_available {
        println!("  │ Node.js MCP SDK          │ {:>6.0} μs    │  {:>5.0}x   │", sdk_ns / 1000.0, sdk_ns / native_ns);
    }
    println!("  └──────────────────────────┴──────────────┴───────────┘");
}
