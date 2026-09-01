//! Example: Connect to a VELOCITY-MCP server via shared memory.
//!
//! Usage:
//!   1. Start the server: `velocity_mcp --mode shmem --buffer-path velocity_mcp.bin`
//!   2. Run this example: `cargo run --example shmem_client`
//!
//! Environment variables:
//!   - VELOCITY_MCP_BUFFER: Path to the shared memory buffer (default: velocity_mcp.bin)
//!   - VELOCITY_SPIN_US: Spin-wait budget in microseconds (default: 200)

use velocity_mcp_client::{McpClient, ShmemTransport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let buffer_path = std::env::var("VELOCITY_MCP_BUFFER")
        .unwrap_or_else(|_| "velocity_mcp.bin".to_string());

    println!("Connecting to VELOCITY-MCP server via shared memory...");
    println!("Buffer: {}", buffer_path);

    let transport = ShmemTransport::new(&buffer_path)?;
    let mut client = McpClient::new(transport);

    println!("Initializing connection...");
    let init_result = client.initialize().await?;
    println!("Connected to {} v{}", init_result.server_info.name, init_result.server_info.version);
    println!("Protocol: {}", init_result.protocol_version);

    println!("\nListing available tools...");
    let tools = client.list_tools().await?;
    println!("Found {} tools:", tools.len());
    for tool in &tools {
        println!("  - {}: {}", tool.name, tool.description);
    }

    if !tools.is_empty() {
        println!("\nPinging server...");
        client.ping().await?;
        println!("Ping successful!");
    }

    println!("\nClosing connection...");
    client.close().await?;
    println!("Done!");

    Ok(())
}
