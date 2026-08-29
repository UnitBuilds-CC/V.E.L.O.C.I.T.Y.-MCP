// Test file for enhanced proc macro features

use velocity_mcp_macros::mcp_tool;

#[mcp_tool(
    name = "process_items",
    description = "Process a list of items"
)]
fn process_items(items: Vec<String>, count: Option<i64>) -> Result<String, String> {
    let total = count.unwrap_or(items.len() as i64);
    Ok(format!("Processing {} items: {:?}", total, items))
}

#[mcp_tool(
    name = "process_numbers",
    description = "Process a list of numbers"
)]
fn process_numbers(numbers: Vec<i64>) -> Result<String, String> {
    let sum: i64 = numbers.iter().sum();
    Ok(format!("Sum: {}", sum))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_vec_string_param() {
        let args = json!({
            "items": ["apple", "banana", "cherry"],
            "count": 3
        });
        let result = process_items(&args).unwrap();
        assert!(result.contains("3 items"));
    }

    #[test]
    fn test_vec_with_optional() {
        let args = json!({
            "items": ["one", "two"]
        });
        let result = process_items(&args).unwrap();
        assert!(result.contains("2 items"));
    }

    #[test]
    fn test_vec_numbers() {
        let args = json!({
            "numbers": [1, 2, 3, 4, 5]
        });
        let result = process_numbers(&args).unwrap();
        assert_eq!(result, "Sum: 15");
    }

    #[test]
    fn test_vec_schema_generation() {
        let schema = &PROCESS_ITEMS.input_schema;
        let props = schema["properties"].as_object().unwrap();
        
        // Check that items is an array
        let items_schema = &props["items"];
        assert_eq!(items_schema["type"], "array");
        
        // Check that items has item type schema
        assert!(items_schema["items"].is_object());
        assert_eq!(items_schema["items"]["type"], "string");
    }
}
