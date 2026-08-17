use std::env;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, error};

use velocity_mcp::{protocol, registry, benchmark};

/// Server version string, referenced by all protocol handlers and help text.
pub const VERSION: &str = velocity_mcp::VERSION;

/// Global shutdown flag, set to true when Ctrl+C is received.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

fn main() {
    // Initialize structured logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let args: Vec<String> = env::args().collect();
    let mut mode = "stdio";
    let mut buffer_path = "nmcp_buffer.bin";
    let mut benchmark_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                if i + 1 < args.len() {
                    mode = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --mode requires an argument (stdio|shmem)");
                    process::exit(1);
                }
            }
            "--buffer-path" => {
                if i + 1 < args.len() {
                    buffer_path = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --buffer-path requires an argument");
                    process::exit(1);
                }
            }
            "--benchmark" => {
                benchmark_mode = true;
                i += 1;
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                print_help();
                process::exit(1);
            }
        }
    }

    if benchmark_mode {
        benchmark::run_benchmarks();
        return;
    }

    // Install Ctrl+C handler for graceful shutdown
    if let Err(e) = ctrlc::set_handler(move || {
        info!("Received shutdown signal, shutting down gracefully...");
        SHUTDOWN.store(true, Ordering::SeqCst);
    }) {
        eprintln!("Warning: Failed to set Ctrl+C handler: {}", e);
    }

    info!("Starting V.E.L.O.C.I.T.Y. NMCP Server...");
    info!(mode = mode, "Protocol mode selected");

    // Log C# engine path for diagnostics
    let csharp_path = registry::resolve_csharp_path();
    info!(csharp_path = %csharp_path, "C# core engine path resolved");

    match mode {
        "stdio" => {
            if let Err(e) = protocol::json_rpc::run_stdio_loop(&SHUTDOWN) {
                error!(error = %e, "Stdio loop encountered error");
                eprintln!("Stdio loop encountered error: {}", e);
                process::exit(1);
            }
        }
        "shmem" => {
            info!(buffer_path = buffer_path, "Shared Memory Path");
            if let Err(e) = protocol::nmcp_binary::run_shmem_loop(buffer_path, &SHUTDOWN) {
                error!(error = %e, "Shared Memory loop encountered error");
                eprintln!("Shared Memory loop encountered error: {}", e);
                process::exit(1);
            }
        }
        _ => {
            error!(mode = mode, "Invalid mode");
            eprintln!("Error: Invalid mode '{}'. Supported modes: stdio, shmem", mode);
            process::exit(1);
        }
    }

    info!("V.E.L.O.C.I.T.Y. NMCP Server shut down cleanly");
}

fn print_help() {
    println!("V.E.L.O.C.I.T.Y.-MCP Server v{}", crate::VERSION);
    println!("Usage:");
    println!("  velocity_mcp [options]");
    println!();
    println!("Options:");
    println!("  --mode <stdio|shmem>        Protocol mode. stdio (JSON-RPC) or shmem (Shared Memory binary). Default: stdio");
    println!("  --buffer-path <path>        Path to mapped buffer file. Only used in shmem mode. Default: nmcp_buffer.bin");
    println!("  --benchmark                 Run the performance benchmark suite");
    println!("  -h, --help                  Print this help screen");
    println!();
    println!("Environment Variables:");
    println!("  VELOCITY_CSHARP_PATH        Override path to C# NdaMcpServer.exe");
    println!("  RUST_LOG                    Set log level (e.g., info, debug, trace). Default: info");
}
