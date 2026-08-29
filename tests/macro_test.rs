// Test file to verify the #[mcp_tool] proc-macro works correctly

use velocity_mcp_macros::mcp_tool;

#[mcp_tool(name = "test_tool", description = "A test tool")]
fn test_tool(name: String, count: i64) -> Result<String, String> {
    Ok(format!("Hello {}, count: {}", name, count))
}

#[mcp_tool(name = "optional_params", description = "Tool with optional parameters")]
fn optional_params(required: String, optional: Option<i64>) -> Result<String, String> {
    match optional {
        Some(val) => Ok(format!("{}: {}", required, val)),
        None => Ok(required),
    }
}

#[mcp_tool(name = "math_tool", description = "Mathematical operations")]
fn math_tool(a: f64, b: f64, operation: String) -> Result<String, String> {
    let result = match operation.as_str() {
        "add" => a + b,
        "subtract" => a - b,
        "multiply" => a * b,
        "divide" => {
            if b == 0.0 {
                return Err("Division by zero".to_string());
            }
            a / b
        }
        _ => return Err("Unknown operation".to_string()),
    };
    Ok(format!("{}", result))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_basic_tool() {
        let args = json!({
            "name": "World",
            "count": 42
        });
        let result = test_tool(&args).unwrap();
        assert_eq!(result, "Hello World, count: 42");
    }

    #[test]
    fn test_optional_params_with_value() {
        let args = json!({
            "required": "test",
            "optional": 100
        });
        let result = optional_params(&args).unwrap();
        assert_eq!(result, "test: 100");
    }

    #[test]
    fn test_optional_params_without_value() {
        let args = json!({
            "required": "test"
        });
        let result = optional_params(&args).unwrap();
        assert_eq!(result, "test");
    }

    #[test]
    fn test_math_tool_add() {
        let args = json!({
            "a": 10.0,
            "b": 5.0,
            "operation": "add"
        });
        let result = math_tool(&args).unwrap();
        assert_eq!(result, "15");
    }

    #[test]
    fn test_math_tool_divide_by_zero() {
        let args = json!({
            "a": 10.0,
            "b": 0.0,
            "operation": "divide"
        });
        let result = math_tool(&args);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Division by zero");
    }

    #[test]
    fn test_tool_struct_generation() {
        // Verify that the TOOL static was generated
        assert_eq!(TEST_TOOL.name, "test_tool");
        assert_eq!(TEST_TOOL.description, "A test tool");
        assert!(TEST_TOOL.input_schema["properties"]["name"].is_object());
        assert!(TEST_TOOL.input_schema["properties"]["count"].is_object());
    }

    #[test]
    fn test_optional_tool_struct_generation() {
        assert_eq!(OPTIONAL_PARAMS.name, "optional_params");
        assert_eq!(OPTIONAL_PARAMS.description, "Tool with optional parameters");
        // Check that required field is in the required array
        let required = OPTIONAL_PARAMS.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("required")));
        // optional should not be in required
        assert!(!required.iter().any(|v| v.as_str() == Some("optional")));
    }

    #[test]
    fn test_auto_registration() {
        // Tools should be automatically registered at program startup via ctor
        // Check that the tool is in the registry
        let tools = velocity_mcp::registry::get_tools();
        let found = tools.iter().any(|t| t.name == "test_tool");
        assert!(found, "Tool should be auto-registered at startup");
    }
}
