use std::time::Instant;
use std::hint::black_box;
use tracing::info;
use crate::protocol::nmcp_binary::NmcpBinaryFrame;
use crate::ipc::shmem::SharedMemoryBuffer;
use crate::registry;
use serde_json::json;

/// Run the performance benchmark suite.
///
/// Measures three operations:
/// 1. JSON-RPC parsing via `serde_json` (500k iterations)
/// 2. Zero-allocation NMCP binary frame parsing (1M iterations)
/// 3. Shared memory mmap read/write round-trip (200k iterations)
///
/// Prints per-operation mean latency and the binary parser speedup factor.
pub fn run_benchmarks() {
    info!("Starting V.E.L.O.C.I.T.Y.-MCP Performance Benchmark Suite");
    println!("============================================================");
    println!("         V.E.L.O.C.I.T.Y.-MCP Performance Benchmark Suite");
    println!("============================================================");

    // 1. Benchmark JSON-RPC String Parsing (serde_json)
    let json_req = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"C:/invoices/inv-001.nda"}},"id":101}"#;
    let json_iterations = 500_000;
    
    println!("Running JSON-RPC Parse Benchmark ({} iterations)...", json_iterations);
    let start = Instant::now();
    let mut json_checksum: u32 = 0;
    for _ in 0..json_iterations {
        let val: serde_json::Value = serde_json::from_str(black_box(json_req)).unwrap();
        // Extract data from the parsed value to prevent dead-code elimination.
        if let Some(method) = val["method"].as_str() {
            for b in method.bytes() {
                json_checksum = json_checksum.wrapping_add(b as u32);
            }
        }
    }
    let duration_json = start.elapsed();
    let json_avg_ns = (duration_json.as_nanos() as f64) / (json_iterations as f64);
    println!("  Mean Latency (serde_json): {:.2} ns", json_avg_ns);
    black_box(json_checksum);

    // 2. Benchmark NMCP Zero-Alloc Binary Frame Parsing
    let mut binary_buffer = Vec::new();
    binary_buffer.extend_from_slice(b"NMCP"); // Magic
    binary_buffer.extend_from_slice(&[0u8; 32]); // Dummy Merkle root
    binary_buffer.extend_from_slice(b"read_nda C:/invoices/inv-001.nda"); // Payload
    let binary_iterations = 1_000_000;

    println!("\nRunning NMCP Zero-Alloc Binary Frame Parse Benchmark ({} iterations)...", binary_iterations);
    let start_bin = Instant::now();
    let mut checksum: u32 = 0;
    for _ in 0..binary_iterations {
        let frame = NmcpBinaryFrame::parse(black_box(&binary_buffer)).unwrap();
        // Force actual data access through the parsed references.
        // Summing bytes prevents the compiler from hoisting or constant-folding.
        for &b in frame.payload {
            checksum = checksum.wrapping_add(b as u32);
        }
        checksum = checksum.wrapping_add(frame.magic[0] as u32);
    }
    let duration_bin = start_bin.elapsed();
    let bin_avg_ns = (duration_bin.as_nanos() as f64) / (binary_iterations as f64);
    println!("  Mean Latency (Zero-Alloc Binary Frame): {:.2} ns", bin_avg_ns);
    black_box(checksum);

    // 2b. Fair comparison: Same tool call via JSON vs native NDA binary format
    // This measures: JSON parse vs native binary parse (no JSON inside NDA)
    println!("\nRunning Protocol Overhead Benchmark (JSON vs native NDA binary)...");
    
    // The same tool call request as JSON
    let tool_call_json = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"hello_world","arguments":{"message":"Hello, World!"}},"id":1}"#;
    
    // Native NDA binary encoding of the same tool call:
    // [4 bytes: magic "NMCP"]
    // [32 bytes: merkle root]
    // [1 byte: method type (1=tools/call)]
    // [2 bytes: tool name length]
    // [N bytes: tool name]
    // [2 bytes: arguments length]  
    // [M bytes: arguments as binary key-value pairs]
    let mut nda_native = Vec::new();
    nda_native.extend_from_slice(b"NMCP");           // Magic
    nda_native.extend_from_slice(&[0u8; 32]);        // Merkle root
    nda_native.push(1u8);                            // Method type: tools/call
    let tool_name = b"hello_world";
    nda_native.extend_from_slice(&(tool_name.len() as u16).to_be_bytes());
    nda_native.extend_from_slice(tool_name);
    let args = b"message=Hello, World!";
    nda_native.extend_from_slice(&(args.len() as u16).to_be_bytes());
    nda_native.extend_from_slice(args);
    
    let overhead_iterations = 500_000;
    
    // Benchmark A: Parse JSON directly
    let start_json_direct = Instant::now();
    let mut json_direct_checksum: u32 = 0;
    for _ in 0..overhead_iterations {
        let val: serde_json::Value = serde_json::from_str(black_box(tool_call_json)).unwrap();
        if let Some(method) = val["method"].as_str() {
            for b in method.bytes() {
                json_direct_checksum = json_direct_checksum.wrapping_add(b as u32);
            }
        }
        if let Some(tool_name) = val["params"]["name"].as_str() {
            for b in tool_name.bytes() {
                json_direct_checksum = json_direct_checksum.wrapping_add(b as u32);
            }
        }
        if let Some(args) = val["params"]["arguments"].as_object() {
            for (k, v) in args {
                for b in k.bytes() {
                    json_direct_checksum = json_direct_checksum.wrapping_add(b as u32);
                }
                if let Some(s) = v.as_str() {
                    for b in s.bytes() {
                        json_direct_checksum = json_direct_checksum.wrapping_add(b as u32);
                    }
                }
            }
        }
    }
    let duration_json_direct = start_json_direct.elapsed();
    let json_direct_avg_ns = (duration_json_direct.as_nanos() as f64) / (overhead_iterations as f64);
    black_box(json_direct_checksum);
    
    // Benchmark B: Parse native NDA binary format (no JSON parsing at all)
    let start_nda_native = Instant::now();
    let mut nda_native_checksum: u32 = 0;
    for _ in 0..overhead_iterations {
        // Zero-copy binary frame parse
        let frame = NmcpBinaryFrame::parse(black_box(&nda_native)).unwrap();
        // Extract method type (1 byte at offset 36)
        let method_type = frame.payload[0];
        nda_native_checksum = nda_native_checksum.wrapping_add(method_type as u32);
        // Extract tool name length (2 bytes)
        let name_len = u16::from_be_bytes([frame.payload[1], frame.payload[2]]) as usize;
        // Extract tool name
        let name_start = 3;
        let name_end = name_start + name_len;
        let tool_name_bytes = &frame.payload[name_start..name_end];
        for &b in tool_name_bytes {
            nda_native_checksum = nda_native_checksum.wrapping_add(b as u32);
        }
        // Extract arguments length (2 bytes)
        let args_len = u16::from_be_bytes([frame.payload[name_end], frame.payload[name_end + 1]]) as usize;
        // Extract arguments
        let args_start = name_end + 2;
        let args_bytes = &frame.payload[args_start..args_start + args_len];
        for &b in args_bytes {
            nda_native_checksum = nda_native_checksum.wrapping_add(b as u32);
        }
    }
    let duration_nda_native = start_nda_native.elapsed();
    let nda_native_avg_ns = (duration_nda_native.as_nanos() as f64) / (overhead_iterations as f64);
    black_box(nda_native_checksum);
    
    println!("  JSON-RPC parse (full extraction):  {:.2} ns", json_direct_avg_ns);
    println!("  NDA native binary parse:           {:.2} ns", nda_native_avg_ns);
    let speedup = json_direct_avg_ns / nda_native_avg_ns;
    println!("  NDA speedup:                       {:.1}x faster than JSON", speedup);

    // 3. Benchmark Shared Memory Mapped Operations (Read/Write)
    let temp_shmem_path = "temp_bench_shmem.bin";
    let shmem_iterations = 200_000;
    
    // Create/init buffer
    let mut buffer = SharedMemoryBuffer::create_or_open(temp_shmem_path).expect("Failed to create temp shmem");
    
    println!("\nRunning Shared Memory Read/Write Operation Benchmark ({} iterations)...", shmem_iterations);
    let start_shmem = Instant::now();
    for _ in 0..shmem_iterations {
        // Write request to the input buffer region
        buffer.write_input(black_box(json_req)).unwrap();
        // Read it back from the input buffer region
        let _input = black_box(buffer.read_input().unwrap());
    }
    let duration_shmem = start_shmem.elapsed();
    let shmem_avg_ns = (duration_shmem.as_nanos() as f64) / (shmem_iterations as f64);
    println!("  Mean Latency (Shared Memory Mmapped R/W): {:.2} ns", shmem_avg_ns);

    // Cleanup temp shmem file
    let _ = std::fs::remove_file(temp_shmem_path);

    // 4. End-to-End Tool Call Benchmarks (requires C# engine)
    println!("\n------------------------------------------------------------");
    println!("           End-to-End Tool Call Benchmarks");
    println!("------------------------------------------------------------");
    
    let csharp_path = registry::resolve_csharp_path();
    if !std::path::Path::new(&csharp_path).exists() {
        println!("  C# engine not found at: {}", csharp_path);
        println!("  Skipping end-to-end benchmarks (set VELOCITY_CSHARP_PATH to enable)");
    } else {
        // Create a test file for conversion
        let test_file = "temp_bench_test.txt";
        let test_nda = "temp_bench_test.nda";
        std::fs::write(test_file, "Benchmark test content for NDA conversion.\n").expect("Failed to create test file");
        
        // Benchmark 1: JSON tool call (convert_to_nda_document)
        let e2e_iterations = 10;
        println!("\nRunning JSON Tool Call Benchmark ({} iterations)...", e2e_iterations);
        let start_json_tool = Instant::now();
        let mut json_successes = 0;
        for _ in 0..e2e_iterations {
            let args = json!({"filePath": format!("{}\\{}", std::env::current_dir().unwrap().display(), test_file)});
            match registry::call_tool("convert_to_nda_document", &args) {
                Ok(_) => json_successes += 1,
                Err(e) => eprintln!("  JSON tool error: {}", e),
            }
        }
        let duration_json_tool = start_json_tool.elapsed();
        let json_tool_avg_ms = (duration_json_tool.as_millis() as f64) / (e2e_iterations as f64);
        println!("  Mean Latency (JSON tool call): {:.2} ms ({}/{} succeeded)", json_tool_avg_ms, json_successes, e2e_iterations);
        
        // Benchmark 2: NDA tool call (read_nda on the converted file)
        if std::path::Path::new(test_nda).exists() {
            println!("\nRunning NDA Tool Call Benchmark ({} iterations)...", e2e_iterations);
            let start_nda_tool = Instant::now();
            let mut nda_successes = 0;
            for _ in 0..e2e_iterations {
                let args = json!({"ndaPath": format!("{}\\{}", std::env::current_dir().unwrap().display(), test_nda)});
                match registry::call_tool("read_nda", &args) {
                    Ok(_) => nda_successes += 1,
                    Err(e) => eprintln!("  NDA tool error: {}", e),
                }
            }
            let duration_nda_tool = start_nda_tool.elapsed();
            let nda_tool_avg_ms = (duration_nda_tool.as_millis() as f64) / (e2e_iterations as f64);
            println!("  Mean Latency (NDA tool call): {:.2} ms ({}/{} succeeded)", nda_tool_avg_ms, nda_successes, e2e_iterations);
            
            let speedup = json_tool_avg_ms / nda_tool_avg_ms;
            println!("  NDA vs JSON Speedup: {:.2}x", speedup);
        }
        
        // Cleanup
        let _ = std::fs::remove_file(test_file);
        let _ = std::fs::remove_file(test_nda);
    }

    println!("\n============================================================");
    println!("                       Summary Results");
    println!("============================================================");
    println!("  JSON-RPC Parse (Serde):      {:.2} ns", json_avg_ns);
    println!("  Mmapped Buffer R/W:          {:.2} ns", shmem_avg_ns);
    println!("  Zero-Alloc Binary Parse:     {:.2} ns", bin_avg_ns);
    let speedup_binary = json_avg_ns / bin_avg_ns;
    println!("  Binary Ingestion Speedup:    {:.1}x over JSON-RPC", speedup_binary);
    println!("------------------------------------------------------------");
    println!("  Protocol Overhead (same tool call):");
    println!("    JSON full parse:           {:.2} ns", json_direct_avg_ns);
    println!("    NDA native binary:         {:.2} ns", nda_native_avg_ns);
    println!("    NDA speedup:               {:.1}x faster", speedup);
    println!("============================================================");
}
