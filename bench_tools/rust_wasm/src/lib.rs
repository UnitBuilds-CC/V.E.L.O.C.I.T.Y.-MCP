//! Example Rust tool compiled to wasm32-wasip1.
//!
//! Build with: cargo build --target wasm32-wasip1 --release
//! Output: target/wasm32-wasip1/release/rust-wasm-tool.wasm

// Global buffers
static mut INPUT_BUF: [u8; 64 * 1024] = [0; 64 * 1024];  // 64KB input buffer
static mut OUTPUT_BUF: Vec<u8> = Vec::new();

/// Get pointer to input buffer (host writes args here)
#[no_mangle]
pub extern "C" fn get_input_ptr() -> *mut u8 {
    unsafe { INPUT_BUF.as_mut_ptr() }
}

/// Execute the tool with JSON input (already written to INPUT_BUF), return encoded (ptr << 32) | len
#[no_mangle]
pub extern "C" fn tool_execute(input_len: i32) -> i64 {
    let input = unsafe {
        let slice = std::slice::from_raw_parts(INPUT_BUF.as_ptr(), input_len as usize);
        std::str::from_utf8_unchecked(slice)
    };

    // Parse JSON input and process
    let result = process_tool_input(input);

    // Write result to global output buffer
    unsafe {
        OUTPUT_BUF = result.into_bytes();
        let ptr = OUTPUT_BUF.as_ptr() as i64;
        let len = OUTPUT_BUF.len() as i64;
        (ptr << 32) | len
    }
}

/// Simple text analysis tool (matches the benchmark tool)
fn process_tool_input(input: &str) -> String {
    // Minimal JSON parsing (avoid serde for small WASM binary)
    let text = extract_json_string(input, "text").unwrap_or_default();

    let words: Vec<&str> = text.split_whitespace().collect();
    let word_count = words.len();
    let char_count = text.len();
    let line_count = if text.is_empty() { 0 } else { text.lines().count() };

    // Minimal JSON output (avoid serde for small WASM binary)
    format!(
        r#"{{"word_count":{},"char_count":{},"line_count":{}}}"#,
        word_count, char_count, line_count
    )
}

/// Extract a string value from JSON (minimal parser)
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{}":"#, key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];

    // Skip whitespace
    let rest = rest.trim_start();

    if rest.starts_with('"') {
        // String value
        let content = &rest[1..];
        let end = content.find('"')?;
        Some(content[..end].to_string())
    } else {
        None
    }
}
