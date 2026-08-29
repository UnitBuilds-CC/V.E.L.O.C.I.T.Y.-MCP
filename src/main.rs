use std::env;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
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
    let mut addr = "0.0.0.0:3000";
    let mut tls_cert: Option<&str> = None;
    let mut tls_key: Option<&str> = None;
    let mut benchmark_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                if i + 1 < args.len() {
                    mode = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --mode requires an argument (stdio|shmem|http)");
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
            "--addr" => {
                if i + 1 < args.len() {
                    addr = &args[i + 1];
                    i += 2;
                } else {
                    eprintln!("Error: --addr requires an argument");
                    process::exit(1);
                }
            }
            "--tls-cert" => {
                if i + 1 < args.len() {
                    tls_cert = Some(&args[i + 1]);
                    i += 2;
                } else {
                    eprintln!("Error: --tls-cert requires a path argument");
                    process::exit(1);
                }
            }
            "--tls-key" => {
                if i + 1 < args.len() {
                    tls_key = Some(&args[i + 1]);
                    i += 2;
                } else {
                    eprintln!("Error: --tls-key requires a path argument");
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
        #[cfg(feature = "http")]
        "http" => {
            info!(addr = addr, "HTTP server address");
            let shutdown = Arc::new(AtomicBool::new(false));
            // Copy the current SHUTDOWN state
            if SHUTDOWN.load(Ordering::Relaxed) {
                shutdown.store(true, Ordering::SeqCst);
            }
            // Set up Ctrl+C to also set our local shutdown
            let shutdown_clone = Arc::clone(&shutdown);
            if let Err(e) = ctrlc::set_handler(move || {
                info!("Received shutdown signal, shutting down HTTP server...");
                shutdown_clone.store(true, Ordering::SeqCst);
            }) {
                eprintln!("Warning: Failed to set Ctrl+C handler: {}", e);
            }
            let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            let tls_config = match (tls_cert, tls_key) {
                (Some(cert), Some(key)) => Some(velocity_mcp::transport::http::TlsConfig {
                    cert_path: cert.to_string(),
                    key_path: key.to_string(),
                }),
                (Some(_), None) | (None, Some(_)) => {
                    eprintln!("Error: --tls-cert and --tls-key must both be provided");
                    process::exit(1);
                }
                (None, None) => None,
            };
            if let Err(e) = rt.block_on(velocity_mcp::transport::http::run_http_server(addr, shutdown, None, tls_config)) {
                error!(error = %e, "HTTP server encountered error");
                eprintln!("HTTP server encountered error: {}", e);
                process::exit(1);
            }
        }
        #[cfg(not(feature = "http"))]
        "http" => {
            eprintln!("Error: HTTP transport not enabled. Build with --features http");
            process::exit(1);
        }
        _ => {
            error!(mode = mode, "Invalid mode");
            eprintln!("Error: Invalid mode '{}'. Supported modes: stdio, shmem, http", mode);
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
    println!("  --mode <stdio|shmem|http>     Protocol mode. Default: stdio");
    println!("  --buffer-path <path>          Path to mapped buffer file. Only used in shmem mode. Default: nmcp_buffer.bin");
    println!("  --addr <address>              HTTP listen address. Only used in http mode. Default: 0.0.0.0:3000");
    println!("  --tls-cert <path>             Path to TLS certificate (PEM). Enables HTTPS when paired with --tls-key");
    println!("  --tls-key <path>              Path to TLS private key (PEM). Enables HTTPS when paired with --tls-cert");
    println!("  --benchmark                   Run the performance benchmark suite");
    println!("  -h, --help                    Print this help screen");
    println!();
    println!("Environment Variables:");
    println!("  VELOCITY_CSHARP_PATH          Override path to C# NdaMcpServer.exe");
    println!("  RUST_LOG                      Set log level (e.g., info, debug, trace). Default: info");
}
