use velocity_mcp::registry;
use serde_json::json;

fn main() {
    // Test JSON tool call
    let json_request = r#"{
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "hello_world",
            "arguments": {
                "message": "Hello, World!",
                "count": 42
            }
        },
        "id": 1
    }"#;

    println!("=== Testing convert_tool_to_nda ===\n");
    
    // Convert JSON to NDA
    let args = json!({
        "jsonRequest": json_request,
        "outputPath": "test_tool.nda"
    });
    
    match registry::call_tool("convert_tool_to_nda", &args) {
        Ok(result) => {
            println!("✓ Conversion successful: {}", result);
            
            // Check file size
            if let Ok(metadata) = std::fs::metadata("test_tool.nda") {
                println!("✓ NDA file size: {} bytes", metadata.len());
            }
            
            // Read and display hex dump
            if let Ok(data) = std::fs::read("test_tool.nda") {
                println!("\n=== NDA Binary Structure ===");
                println!("Magic: {:?}", String::from_utf8_lossy(&data[0..4]));
                println!("Merkle root: {:02x?}", &data[4..36]);
                println!("Method type: {}", data[36]);
                
                let name_len = u16::from_be_bytes([data[37], data[38]]) as usize;
                println!("Tool name length: {}", name_len);
                println!("Tool name: {:?}", String::from_utf8_lossy(&data[39..39+name_len]));
                
                let args_start = 39 + name_len;
                let args_len = u16::from_be_bytes([data[args_start], data[args_start + 1]]) as usize;
                println!("Arguments length: {}", args_len);
                println!("Arguments: {:?}", String::from_utf8_lossy(&data[args_start+2..args_start+2+args_len]));
            }
        }
        Err(e) => {
            println!("✗ Conversion failed: {}", e);
        }
    }
    
    // Clean up
    let _ = std::fs::remove_file("test_tool.nda");
}
