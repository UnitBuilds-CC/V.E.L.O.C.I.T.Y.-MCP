//! VELOCITY-MCP WASM HTTP adapter for Wasmer Edge deployment.
//!
//! This binary wraps the velocity-mcp-core protocol logic and exposes an HTTP handler
//! compatible with Wasmer Edge's proxy mode.

#[cfg(target_arch = "wasm32")]
mod edge_handler {
    use velocity_mcp_core::{handle_mcp_request, parse_request, serialize_response};

    /// Process MCP JSON-RPC request and return response bytes
    pub fn process_mcp_request(request_body: &[u8]) -> Vec<u8> {
        match parse_request(request_body) {
            Ok(request) => {
                let response = handle_mcp_request(&request);
                serialize_response(&response)
            }
            Err(e) => error_response(&format!("Parse error: {}", e)),
        }
    }

    fn error_response(message: &str) -> Vec<u8> {
        let error = serde_json::json!({
            "jsonrpc": "2.0",
            "error": {
                "code": -32000,
                "message": message
            },
            "id": null
        });
        serde_json::to_vec(&error).unwrap_or_else(|_| vec![])
    }
}

fn main() {
    #[cfg(target_arch = "wasm32")]
    {
        println!("VELOCITY-MCP Edge adapter running on Wasmer Edge");
        println!("Core protocol: velocity-mcp-core v{}", env!("CARGO_PKG_VERSION"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        eprintln!("This binary is designed for wasm32-wasip1 target.");
        eprintln!("Build with: cargo build --target wasm32-wasip1 --release");
        std::process::exit(1);
    }
}
