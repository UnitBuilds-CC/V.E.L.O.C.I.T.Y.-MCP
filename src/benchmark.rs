use std::time::Instant;
use std::hint::black_box;
use tracing::info;
use crate::protocol::nmcp_binary::NmcpBinaryFrame;
use crate::ipc::shmem::SharedMemoryBuffer;

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
    for _ in 0..json_iterations {
        let val: serde_json::Value = serde_json::from_str(black_box(json_req)).unwrap();
        let _method = black_box(val["method"].as_str());
    }
    let duration_json = start.elapsed();
    let json_avg_ns = (duration_json.as_nanos() as f64) / (json_iterations as f64);
    println!("  Mean Latency (serde_json): {:.2} ns", json_avg_ns);

    // 2. Benchmark NMCP Zero-Alloc Binary Frame Parsing
    let mut binary_buffer = Vec::new();
    binary_buffer.extend_from_slice(b"NMCP"); // Magic
    binary_buffer.extend_from_slice(&[0u8; 32]); // Dummy Merkle root
    binary_buffer.extend_from_slice(b"read_nda C:/invoices/inv-001.nda"); // Payload
    let binary_iterations = 1_000_000;

    println!("\nRunning NMCP Zero-Alloc Binary Frame Parse Benchmark ({} iterations)...", binary_iterations);
    let start_bin = Instant::now();
    for _ in 0..binary_iterations {
        let frame = NmcpBinaryFrame::parse(black_box(&binary_buffer)).unwrap();
        let _magic = black_box(frame.magic);
    }
    let duration_bin = start_bin.elapsed();
    let bin_avg_ns = (duration_bin.as_nanos() as f64) / (binary_iterations as f64);
    println!("  Mean Latency (Zero-Alloc Binary Frame): {:.2} ns", bin_avg_ns);

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

    println!("\n============================================================");
    println!("                       Summary Results");
    println!("============================================================");
    println!("  JSON-RPC Parse (Serde):      {:.2} ns", json_avg_ns);
    println!("  Mmapped Buffer R/W:          {:.2} ns", shmem_avg_ns);
    println!("  Zero-Alloc Binary Parse:     {:.2} ns", bin_avg_ns);
    let speedup_binary = json_avg_ns / bin_avg_ns;
    println!("  Binary Ingestion Speedup:    {:.1}x over JSON-RPC", speedup_binary);
    println!("============================================================");
}
