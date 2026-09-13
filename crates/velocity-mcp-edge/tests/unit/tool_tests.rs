/// Unit tests for edge tool implementations.
use serde_json::json;
use velocity_mcp_edge::tools::{dispatch_edge_tool, edge_tool_definitions};

#[test]
fn test_echo_tool() {
    let args = json!({"message": "hello world"});
    let result = dispatch_edge_tool("echo", &args).unwrap();
    assert_eq!(result, "hello world");
}

#[test]
fn test_echo_tool_missing_message() {
    let args = json!({});
    let result = dispatch_edge_tool("echo", &args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("message"));
}

#[test]
fn test_json_format_pretty_print() {
    let args = json!({"json": "{\"key\":\"value\"}", "indent": 4});
    let result = dispatch_edge_tool("json_format", &args).unwrap();
    assert!(result.contains("{"));
    assert!(result.contains("}"));
    // Should have newlines and indentation
    assert!(result.contains("\n"));
}

#[test]
fn test_json_format_invalid_json() {
    let args = json!({"json": "not valid json"});
    let result = dispatch_edge_tool("json_format", &args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid JSON"));
}

#[test]
fn test_json_format_missing_json_arg() {
    let args = json!({"indent": 2});
    let result = dispatch_edge_tool("json_format", &args);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("json"));
}

#[test]
fn test_text_count_words() {
    let args = json!({"text": "hello world foo bar", "mode": "words"});
    let result = dispatch_edge_tool("text_count", &args).unwrap();
    assert!(result.contains("4"));
}

#[test]
fn test_text_count_chars() {
    let args = json!({"text": "hello", "mode": "chars"});
    let result = dispatch_edge_tool("text_count", &args).unwrap();
    assert!(result.contains("5"));
}

#[test]
fn test_text_count_lines() {
    let args = json!({"text": "line1\nline2\nline3", "mode": "lines"});
    let result = dispatch_edge_tool("text_count", &args).unwrap();
    assert!(result.contains("3"));
}

#[test]
fn test_text_count_missing_text() {
    let args = json!({"mode": "words"});
    let result = dispatch_edge_tool("text_count", &args);
    assert!(result.is_err());
}

#[test]
fn test_text_transform_uppercase() {
    let args = json!({"text": "hello world", "operation": "upper"});
    let result = dispatch_edge_tool("text_transform", &args).unwrap();
    assert_eq!(result, "HELLO WORLD");
}

#[test]
fn test_text_transform_lowercase() {
    let args = json!({"text": "HELLO WORLD", "operation": "lower"});
    let result = dispatch_edge_tool("text_transform", &args).unwrap();
    assert_eq!(result, "hello world");
}

#[test]
fn test_text_transform_reverse() {
    let args = json!({"text": "abc", "operation": "reverse"});
    let result = dispatch_edge_tool("text_transform", &args).unwrap();
    assert_eq!(result, "cba");
}

#[test]
fn test_text_transform_trim() {
    let args = json!({"text": "  hello  ", "operation": "trim"});
    let result = dispatch_edge_tool("text_transform", &args).unwrap();
    assert_eq!(result, "hello");
}

#[test]
fn test_math_eval_addition() {
    let args = json!({"expression": "2 + 3"});
    let result = dispatch_edge_tool("math_eval", &args).unwrap();
    assert!(result.contains("5"));
}

#[test]
fn test_math_eval_multiplication() {
    let args = json!({"expression": "4 * 5"});
    let result = dispatch_edge_tool("math_eval", &args).unwrap();
    assert!(result.contains("20"));
}

#[test]
fn test_math_eval_complex() {
    let args = json!({"expression": "(10 + 5) * 2"});
    let result = dispatch_edge_tool("math_eval", &args).unwrap();
    assert!(result.contains("30"));
}

#[test]
fn test_math_eval_missing_expression() {
    let args = json!({});
    let result = dispatch_edge_tool("math_eval", &args);
    assert!(result.is_err());
}

#[test]
fn test_bench_echo() {
    let args = json!({"size": 12});
    let result = dispatch_edge_tool("bench_echo", &args).unwrap();
    assert_eq!(result.len(), 12);
}

#[test]
fn test_timestamp() {
    let args = json!({});
    let result = dispatch_edge_tool("timestamp", &args).unwrap();
    // Should be an ISO 8601 timestamp
    assert!(result.contains("T"));
    assert!(result.contains("Z") || result.contains("+"));
}

#[test]
fn test_base64_encode() {
    let args = json!({"text": "hello world"});
    let result = dispatch_edge_tool("base64_encode", &args).unwrap();
    assert_eq!(result, "aGVsbG8gd29ybGQ=");
}

#[test]
fn test_base64_decode() {
    let args = json!({"base64": "aGVsbG8gd29ybGQ="});
    let result = dispatch_edge_tool("base64_decode", &args).unwrap();
    assert_eq!(result, "hello world");
}

#[test]
fn test_base64_encode_missing_text() {
    let args = json!({});
    let result = dispatch_edge_tool("base64_encode", &args);
    assert!(result.is_err());
}

#[test]
fn test_hash_text_sha256() {
    let args = json!({"text": "hello", "algorithm": "sha256"});
    let result = dispatch_edge_tool("hash_text", &args).unwrap();
    // SHA-256 produces 64 hex characters
    assert_eq!(result.len(), 64);
}

#[test]
fn test_hash_text_md5() {
    let args = json!({"text": "hello", "algorithm": "md5"});
    let result = dispatch_edge_tool("hash_text", &args).unwrap();
    // MD5 produces 32 hex characters
    assert_eq!(result.len(), 32);
}

#[test]
fn test_hash_text_missing_text() {
    let args = json!({"algorithm": "sha256"});
    let result = dispatch_edge_tool("hash_text", &args);
    assert!(result.is_err());
}

#[test]
fn test_unknown_tool() {
    let args = json!({});
    let result = dispatch_edge_tool("nonexistent_tool", &args);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("Unknown tool"));
    assert!(err.contains("Available tools"));
}

#[test]
fn test_edge_tool_definitions() {
    let tools = edge_tool_definitions();
    assert!(!tools.is_empty());
    
    // Verify all expected tools are present
    let tool_names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"echo"));
    assert!(tool_names.contains(&"json_format"));
    assert!(tool_names.contains(&"text_count"));
    assert!(tool_names.contains(&"text_transform"));
    assert!(tool_names.contains(&"math_eval"));
    assert!(tool_names.contains(&"bench_echo"));
    assert!(tool_names.contains(&"timestamp"));
    assert!(tool_names.contains(&"base64_encode"));
    assert!(tool_names.contains(&"base64_decode"));
    assert!(tool_names.contains(&"hash_text"));
}

#[test]
fn test_all_tools_have_descriptions() {
    let tools = edge_tool_definitions();
    for tool in &tools {
        assert!(!tool.description.is_empty(), "Tool '{}' has empty description", tool.name);
    }
}

#[test]
fn test_all_tools_have_input_schema() {
    let tools = edge_tool_definitions();
    for tool in &tools {
        assert!(tool.input_schema.is_object(), "Tool '{}' has invalid input schema", tool.name);
    }
}
