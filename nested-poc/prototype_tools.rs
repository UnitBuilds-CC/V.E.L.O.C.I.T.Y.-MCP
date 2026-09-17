//! Edge tool implementations — pure Rust, no OS dependencies.
//!
//! These tools run in any deployment target including WASM/WASIX edge functions.
//! They are pure compute: no filesystem, no process spawning, no network.
//! All tools enforce the 5-second execution timeout constraint by design
//! (they complete in microseconds).
//!
//! ## Architecture Decision
//!
//! **Why not embed Wasmer in edge?** Wasmer's Cranelift backend JIT-compiles WASM
//! to native machine code. Inside a WASM/WASIX environment, there is no native
//! execution environment — you cannot JIT to code you cannot run. Wasmer has no
//! pure-WASM interpreter backend for nested execution.
//!
//! **Why not load pre-compiled WASM modules?** WASM modules cannot be loaded or
//! instantiated from within a WASM sandbox. There is no `wasm32-wasip1` API for
//! dynamic module loading.
//!
//! **Chosen approach: Pure Rust built-in tools.** These tools implement useful
//! AI-agent functionality (text processing, math, encoding, etc.) directly in
//! Rust. They compile to both native and WASM targets with zero external
//! dependencies beyond serde_json. Memory overhead is <1MB total.
//!
//! Future enhancement: the C tree-walk interpreter from `bench_tools/interp_core`
//! could be compiled to WASM and embedded as a byte slice, enabling user-defined
//! tool scripts in 6 languages (PHP, C#, Java, R, Julia, Perl dialects).

use serde_json::Value;
use velocity_mcp_core::{ToolDefinition, ToolExecutor};

// ---------------------------------------------------------------------------
// EdgeToolExecutor — implements ToolExecutor for edge deployment
// ---------------------------------------------------------------------------

/// Tool executor for edge deployment. All tools are pure Rust with no OS
/// dependencies. Suitable for WASM/WASIX compilation.
pub struct EdgeToolExecutor;

impl EdgeToolExecutor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for EdgeToolExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolExecutor for EdgeToolExecutor {
    fn list_tools(&self) -> Vec<ToolDefinition> {
        let mut tools = edge_tool_definitions();
        #[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
        {
            tools.push(ToolDefinition {
                name: "register_plugin".to_string(),
                description: "Register a dynamic Python (MicroPython) tool without restarting the server. Provide a tool name and Python source that prints a result and may read a dict `args`.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "code": {"type": "string"}
                    },
                    "required": ["name", "code"]
                }),
            });
            tools.push(ToolDefinition {
                name: "unregister_plugin".to_string(),
                description: "Remove a previously registered dynamic tool by name.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": { "name": {"type": "string"} },
                    "required": ["name"]
                }),
            });
            for name in nested_plugin_names() {
                tools.push(ToolDefinition {
                    name,
                    description: "Dynamic MicroPython plugin tool.".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": { "args": {"type": "object", "description": "Arguments passed to the plugin as dict `args`."} },
                        "required": []
                    }),
                });
            }
        }
        tools
    }

    fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, String> {
        #[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
        {
            if name == "register_plugin" {
                let pn = arguments.get("name").and_then(|v| v.as_str()).ok_or("missing 'name'")?;
                let code = arguments.get("code").and_then(|v| v.as_str()).ok_or("missing 'code'")?;
                return nested_register(pn, code);
            }
            if name == "unregister_plugin" {
                let pn = arguments.get("name").and_then(|v| v.as_str()).ok_or("missing 'name'")?;
                return nested_unregister(pn);
            }
            if name == "pg_probe" {
                return pg_probe();
            }
            if nested_plugin_names().iter().any(|p| p == name) {
                let args = arguments
                    .get("args")
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!({}));
                return nested_call_plugin(name, &args.to_string());
            }
        }
        dispatch_edge_tool(name, arguments)
    }
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

/// Return the list of all edge-available tool definitions.
pub fn edge_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echo back the input message. Useful for testing connectivity and verifying tool execution works end-to-end.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to echo back.",
                        "maxLength": 1048576
                    }
                },
                "required": ["message"]
            }),
        },
        ToolDefinition {
            name: "json_format".to_string(),
            description: "Parse and pretty-print a JSON string with configurable indentation. Validates JSON syntax and returns human-readable output. Use for inspecting API responses or debugging JSON data.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "json": {
                        "type": "string",
                        "description": "JSON string to format."
                    },
                    "indent": {
                        "type": "integer",
                        "description": "Number of spaces for indentation (default: 2).",
                        "minimum": 0,
                        "maximum": 8
                    }
                },
                "required": ["json"]
            }),
        },
        ToolDefinition {
            name: "text_count".to_string(),
            description: "Count characters, words, lines, and bytes in a text string. Returns detailed statistics. Use for measuring text size or analyzing document structure.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to analyze."
                    }
                },
                "required": ["text"]
            }),
        },
        ToolDefinition {
            name: "text_transform".to_string(),
            description: "Transform text using various operations: upper, lower, title, reverse, trim, truncate, replace, slug. Use for text processing and formatting.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to transform."
                    },
                    "operation": {
                        "type": "string",
                        "enum": ["upper", "lower", "title", "reverse", "trim", "truncate", "replace", "slug"],
                        "description": "Transformation to apply."
                    },
                    "max_length": {
                        "type": "integer",
                        "description": "For truncate: maximum output length.",
                        "minimum": 0,
                        "maximum": 1048576
                    },
                    "old": {
                        "type": "string",
                        "description": "For replace: text to find."
                    },
                    "new": {
                        "type": "string",
                        "description": "For replace: replacement text."
                    }
                },
                "required": ["text", "operation"]
            }),
        },
        ToolDefinition {
            name: "math_eval".to_string(),
            description: "Safely evaluate a mathematical expression. Supports +, -, *, /, %, ^ (power), parentheses, and common functions (sqrt, sin, cos, tan, log, abs, floor, ceil, round). No variables or assignments allowed — pure expression evaluation only.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": {
                        "type": "string",
                        "description": "Mathematical expression to evaluate (e.g., '(2 + 3) * 4', 'sqrt(16)', '2^10')."
                    }
                },
                "required": ["expression"]
            }),
        },
        ToolDefinition {
            name: "bench_echo".to_string(),
            description: "Benchmark tool: generates a text payload of the requested size in bytes. Used for measuring serialization throughput at different payload sizes. Returns a string of exactly the requested byte length.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "size": {
                        "type": "integer",
                        "description": "Response payload size in bytes (default: 64, max: 1048576).",
                        "minimum": 0,
                        "maximum": 1048576
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "timestamp".to_string(),
            description: "Return the current UTC timestamp in ISO 8601 format, along with Unix epoch seconds and milliseconds. Useful for time-based operations and logging.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "base64_encode".to_string(),
            description: "Encode text or binary data to Base64. Accepts a UTF-8 string and returns its Base64 representation. Use for encoding data for transport or storage.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to encode to Base64."
                    }
                },
                "required": ["text"]
            }),
        },
        ToolDefinition {
            name: "base64_decode".to_string(),
            description: "Decode a Base64 string back to UTF-8 text. Returns an error if the input is not valid Base64 or does not decode to valid UTF-8.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "base64": {
                        "type": "string",
                        "description": "Base64 string to decode."
                    }
                },
                "required": ["base64"]
            }),
        },
        ToolDefinition {
            name: "hash_text".to_string(),
            description: "Compute a hash digest of the input text. Supports SHA-256 (default), SHA-1, and MD5 (via simple implementations). Returns hex-encoded digest. Useful for checksums, deduplication, and integrity verification.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text to hash."
                    },
                    "algorithm": {
                        "type": "string",
                        "enum": ["sha256", "sha1", "md5"],
                        "description": "Hash algorithm (default: sha256)."
                    }
                },
                "required": ["text"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Tool dispatch
// ---------------------------------------------------------------------------

/// Dispatch a tool call to the appropriate pure-Rust implementation.
///
/// All tools here are pure compute — no filesystem, no processes, no network.
/// They complete in microseconds and never need the 5-second timeout.
pub fn dispatch_edge_tool(name: &str, args: &Value) -> Result<String, String> {
    match name {
        "echo" => tool_echo(args),
        "json_format" => tool_json_format(args),
        "text_count" => tool_text_count(args),
        "text_transform" => tool_text_transform(args),
        "math_eval" => tool_math_eval(args),
        "bench_echo" => tool_bench_echo(args),
        "timestamp" => tool_timestamp(args),
        "base64_encode" => tool_base64_encode(args),
        "base64_decode" => tool_base64_decode(args),
        "hash_text" => tool_hash_text(args),
        _ => Err(format!("Unknown tool: '{}'. Available tools: echo, json_format, text_count, text_transform, math_eval, bench_echo, timestamp, base64_encode, base64_decode, hash_text", name)),
    }
}

// ---------------------------------------------------------------------------
// Individual tool implementations
// ---------------------------------------------------------------------------

fn tool_echo(args: &Value) -> Result<String, String> {
    let message = args
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'message' (string)".to_string())?;
    Ok(message.to_string())
}

fn tool_json_format(args: &Value) -> Result<String, String> {
    let json_str = args
        .get("json")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'json' (string)".to_string())?;

    let indent = args.get("indent").and_then(|v| v.as_u64()).unwrap_or(2) as usize;

    // Clamp indent to reasonable range
    let indent = indent.min(8);

    let parsed: Value =
        serde_json::from_str(json_str).map_err(|e| format!("Invalid JSON: {}", e))?;

    // Pretty-print with the requested indent
    let formatted = pretty_print_json(&parsed, indent);
    Ok(formatted)
}

/// Maximum JSON nesting depth for pretty-printing (prevents stack overflow DoS).
const MAX_JSON_DEPTH: usize = 200;

/// Pretty-print JSON with configurable indent (no external formatter needed).
fn pretty_print_json(value: &Value, indent: usize) -> String {
    let mut out = String::new();
    pretty_print_inner(value, &mut out, indent, 0);
    out
}

fn pretty_print_inner(value: &Value, out: &mut String, indent: usize, depth: usize) {
    // Guard against deeply nested structures that could cause stack overflow
    if depth > MAX_JSON_DEPTH {
        out.push_str("...");
        return;
    }

    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => {
            out.push('"');
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if c < '\x20' => {
                        out.push_str(&format!("\\u{:04x}", c as u32));
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push_str("[\n");
            let inner_indent = " ".repeat(indent * (depth + 1));
            for (i, item) in arr.iter().enumerate() {
                out.push_str(&inner_indent);
                pretty_print_inner(item, out, indent, depth + 1);
                if i + 1 < arr.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            let outer_indent = " ".repeat(indent * depth);
            out.push_str(&outer_indent);
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{\n");
            let inner_indent = " ".repeat(indent * (depth + 1));
            for (i, (key, val)) in map.iter().enumerate() {
                out.push_str(&inner_indent);
                out.push('"');
                out.push_str(key);
                out.push_str("\": ");
                pretty_print_inner(val, out, indent, depth + 1);
                if i + 1 < map.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            let outer_indent = " ".repeat(indent * depth);
            out.push_str(&outer_indent);
            out.push('}');
        }
    }
}

fn tool_text_count(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'text' (string)".to_string())?;

    let chars = text.chars().count();
    let bytes = text.len();
    let lines = if text.is_empty() {
        0
    } else {
        text.lines().count()
    };
    let words = text.split_whitespace().count();

    let result = serde_json::json!({
        "characters": chars,
        "bytes": bytes,
        "words": words,
        "lines": lines
    });

    Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
}

fn tool_text_transform(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'text' (string)".to_string())?;

    let operation = args
        .get("operation")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'operation' (string)".to_string())?;

    match operation {
        "upper" => Ok(text.to_uppercase()),
        "lower" => Ok(text.to_lowercase()),
        "title" => Ok(to_title_case(text)),
        "reverse" => Ok(text.chars().rev().collect()),
        "trim" => Ok(text.trim().to_string()),
        "truncate" => {
            let max_len = args
                .get("max_length")
                .and_then(|v| v.as_u64())
                .unwrap_or(100) as usize;
            if text.len() <= max_len {
                Ok(text.to_string())
            } else {
                let truncated: String = text.chars().take(max_len).collect();
                Ok(format!("{}...", truncated))
            }
        }
        "replace" => {
            let old = args
                .get("old")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "'replace' operation requires 'old' argument".to_string())?;
            let new = args
                .get("new")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "'replace' operation requires 'new' argument".to_string())?;

            // Prevent string amplification DoS: reject empty 'old' which would cause infinite replacement
            if old.is_empty() {
                return Err("'replace' operation: 'old' argument cannot be empty (would cause unbounded growth)".to_string());
            }

            // Limit output size to prevent memory exhaustion
            let result = text.replace(old, new);
            if result.len() > 10_485_760 { // 10MB limit
                return Err(format!("Replace result too large ({} bytes, max 10MB)", result.len()));
            }
            Ok(result)
        }
        "slug" => Ok(to_slug(text)),
        _ => Err(format!(
            "Unknown operation: '{}'. Valid operations: upper, lower, title, reverse, trim, truncate, replace, slug",
            operation
        )),
    }
}

fn to_title_case(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_whitespace = true;
    for ch in s.chars() {
        if prev_whitespace && ch.is_ascii_alphabetic() {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
        } else {
            result.push(ch);
        }
        prev_whitespace = ch.is_ascii_whitespace();
    }
    result
}

fn to_slug(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

fn tool_math_eval(args: &Value) -> Result<String, String> {
    let expression = args
        .get("expression")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'expression' (string)".to_string())?;

    let result = eval_math_expr(expression.trim())?;

    // Format: if the result is an integer value, show it without decimals
    if result == result.floor() && result.is_finite() && result.abs() < 1e15 {
        Ok(format!("{}", result as i64))
    } else {
        Ok(format!("{}", result))
    }
}

// ---------------------------------------------------------------------------
// Safe math expression evaluator (recursive descent parser)
// ---------------------------------------------------------------------------

/// Maximum recursion depth to prevent stack overflow attacks.
const MAX_MATH_RECURSION_DEPTH: usize = 100;

/// Evaluate a mathematical expression safely. No variables, no assignments,
/// no function calls beyond the built-in set. Completes in microseconds.
fn eval_math_expr(input: &str) -> Result<f64, String> {
    let tokens = tokenize_math(input)?;
    let mut pos = 0;
    let result = parse_expr(&tokens, &mut pos, 0)?;
    if pos < tokens.len() {
        return Err(format!(
            "Unexpected token '{}' at position {}",
            token_str(&tokens[pos]),
            pos
        ));
    }
    Ok(result)
}

#[derive(Debug, Clone)]
enum MathToken {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    LParen,
    RParen,
    Comma,
    Ident(String),
}

fn token_str(t: &MathToken) -> String {
    match t {
        MathToken::Number(n) => format!("{}", n),
        MathToken::Plus => "+".into(),
        MathToken::Minus => "-".into(),
        MathToken::Star => "*".into(),
        MathToken::Slash => "/".into(),
        MathToken::Percent => "%".into(),
        MathToken::Caret => "^".into(),
        MathToken::LParen => "(".into(),
        MathToken::RParen => ")".into(),
        MathToken::Comma => ",".into(),
        MathToken::Ident(s) => s.clone(),
    }
}

fn tokenize_math(input: &str) -> Result<Vec<MathToken>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' => {
                i += 1;
            }
            '+' => {
                tokens.push(MathToken::Plus);
                i += 1;
            }
            '-' => {
                tokens.push(MathToken::Minus);
                i += 1;
            }
            '*' => {
                tokens.push(MathToken::Star);
                i += 1;
            }
            '/' => {
                tokens.push(MathToken::Slash);
                i += 1;
            }
            '%' => {
                tokens.push(MathToken::Percent);
                i += 1;
            }
            '^' => {
                tokens.push(MathToken::Caret);
                i += 1;
            }
            '(' => {
                tokens.push(MathToken::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(MathToken::RParen);
                i += 1;
            }
            ',' => {
                tokens.push(MathToken::Comma);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let start = i;
                let mut has_dot = c == '.';
                i += 1;
                while i < chars.len()
                    && (chars[i].is_ascii_digit() || (chars[i] == '.' && !has_dot))
                {
                    if chars[i] == '.' {
                        has_dot = true;
                    }
                    i += 1;
                }
                let num_str: String = chars[start..i].iter().collect();
                let num: f64 = num_str
                    .parse()
                    .map_err(|_| format!("Invalid number: '{}'", num_str))?;
                tokens.push(MathToken::Number(num));
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                let ident: String = chars[start..i].iter().collect();
                tokens.push(MathToken::Ident(ident));
            }
            c => return Err(format!("Unexpected character: '{}'", c)),
        }
    }

    Ok(tokens)
}

// Recursive descent parser:
// expr     -> term (('+' | '-') term)*
// term     -> power (('*' | '/' | '%') power)*
// power    -> unary ('^' unary)*
// unary    -> ('-' | '+') unary | primary
// primary  -> NUMBER | FUNC '(' args ')' | '(' expr ')'

fn parse_expr(tokens: &[MathToken], pos: &mut usize, depth: usize) -> Result<f64, String> {
    if depth > MAX_MATH_RECURSION_DEPTH {
        return Err(format!(
            "Expression too complex (max recursion depth {})",
            MAX_MATH_RECURSION_DEPTH
        ));
    }
    let mut result = parse_term(tokens, pos, depth + 1)?;
    while *pos < tokens.len() {
        match tokens[*pos] {
            MathToken::Plus => {
                *pos += 1;
                result += parse_term(tokens, pos, depth + 1)?;
            }
            MathToken::Minus => {
                *pos += 1;
                result -= parse_term(tokens, pos, depth + 1)?;
            }
            _ => break,
        }
    }
    Ok(result)
}

fn parse_term(tokens: &[MathToken], pos: &mut usize, depth: usize) -> Result<f64, String> {
    if depth > MAX_MATH_RECURSION_DEPTH {
        return Err(format!(
            "Expression too complex (max recursion depth {})",
            MAX_MATH_RECURSION_DEPTH
        ));
    }
    let mut result = parse_power(tokens, pos, depth + 1)?;
    while *pos < tokens.len() {
        match tokens[*pos] {
            MathToken::Star => {
                *pos += 1;
                result *= parse_power(tokens, pos, depth + 1)?;
            }
            MathToken::Slash => {
                *pos += 1;
                let divisor = parse_power(tokens, pos, depth + 1)?;
                if divisor == 0.0 {
                    return Err("Division by zero".to_string());
                }
                result /= divisor;
            }
            MathToken::Percent => {
                *pos += 1;
                let divisor = parse_power(tokens, pos, depth + 1)?;
                if divisor == 0.0 {
                    return Err("Modulo by zero".to_string());
                }
                result %= divisor;
            }
            _ => break,
        }
    }
    Ok(result)
}

fn parse_power(tokens: &[MathToken], pos: &mut usize, depth: usize) -> Result<f64, String> {
    if depth > MAX_MATH_RECURSION_DEPTH {
        return Err(format!(
            "Expression too complex (max recursion depth {})",
            MAX_MATH_RECURSION_DEPTH
        ));
    }
    let base = parse_unary(tokens, pos, depth + 1)?;
    if *pos < tokens.len() {
        if let MathToken::Caret = tokens[*pos] {
            *pos += 1;
            let exp = parse_power(tokens, pos, depth + 1)?; // Right-associative
            return Ok(base.powf(exp));
        }
    }
    Ok(base)
}

fn parse_unary(tokens: &[MathToken], pos: &mut usize, depth: usize) -> Result<f64, String> {
    if depth > MAX_MATH_RECURSION_DEPTH {
        return Err(format!(
            "Expression too complex (max recursion depth {})",
            MAX_MATH_RECURSION_DEPTH
        ));
    }
    if *pos < tokens.len() {
        match tokens[*pos] {
            MathToken::Minus => {
                *pos += 1;
                return Ok(-parse_unary(tokens, pos, depth + 1)?);
            }
            MathToken::Plus => {
                *pos += 1;
                return parse_unary(tokens, pos, depth + 1);
            }
            _ => {}
        }
    }
    parse_primary(tokens, pos, depth + 1)
}

fn parse_primary(tokens: &[MathToken], pos: &mut usize, depth: usize) -> Result<f64, String> {
    if depth > MAX_MATH_RECURSION_DEPTH {
        return Err(format!(
            "Expression too complex (max recursion depth {})",
            MAX_MATH_RECURSION_DEPTH
        ));
    }
    if *pos >= tokens.len() {
        return Err("Unexpected end of expression".to_string());
    }

    match &tokens[*pos] {
        MathToken::Number(n) => {
            let val = *n;
            *pos += 1;
            Ok(val)
        }
        MathToken::Ident(name) => {
            let func_name = name.clone();
            *pos += 1;
            // Expect '('
            if *pos >= tokens.len() {
                return Err(format!("Expected '(' after function '{}'", func_name));
            }
            if let MathToken::LParen = tokens[*pos] {
                *pos += 1;
            } else {
                return Err(format!("Unknown identifier: '{}'", func_name));
            }

            // Parse arguments
            let mut args = Vec::new();
            if *pos < tokens.len() {
                if let MathToken::RParen = tokens[*pos] {
                    // No arguments
                } else {
                    args.push(parse_expr(tokens, pos, depth + 1)?);
                    while *pos < tokens.len() {
                        if let MathToken::Comma = tokens[*pos] {
                            *pos += 1;
                            args.push(parse_expr(tokens, pos, depth + 1)?);
                        } else {
                            break;
                        }
                    }
                }
            }

            // Expect ')'
            if *pos >= tokens.len() || !matches!(tokens[*pos], MathToken::RParen) {
                return Err(format!("Expected ')' after arguments to '{}'", func_name));
            }
            *pos += 1;

            // Evaluate built-in function
            eval_math_func(&func_name, &args)
        }
        MathToken::LParen => {
            *pos += 1;
            let result = parse_expr(tokens, pos, depth + 1)?;
            if *pos >= tokens.len() || !matches!(tokens[*pos], MathToken::RParen) {
                return Err("Expected ')'".to_string());
            }
            *pos += 1;
            Ok(result)
        }
        other => Err(format!("Unexpected token: '{}'", token_str(other))),
    }
}

fn eval_math_func(name: &str, args: &[f64]) -> Result<f64, String> {
    match name {
        "sqrt" => {
            require_args(name, args, 1)?;
            let v = args[0];
            if v < 0.0 { return Err("sqrt of negative number".to_string()); }
            Ok(v.sqrt())
        }
        "abs" => {
            require_args(name, args, 1)?;
            Ok(args[0].abs())
        }
        "floor" => {
            require_args(name, args, 1)?;
            Ok(args[0].floor())
        }
        "ceil" => {
            require_args(name, args, 1)?;
            Ok(args[0].ceil())
        }
        "round" => {
            require_args(name, args, 1)?;
            Ok(args[0].round())
        }
        "sin" => {
            require_args(name, args, 1)?;
            Ok(args[0].sin())
        }
        "cos" => {
            require_args(name, args, 1)?;
            Ok(args[0].cos())
        }
        "tan" => {
            require_args(name, args, 1)?;
            Ok(args[0].tan())
        }
        "log" => {
            require_args(name, args, 1)?;
            let v = args[0];
            if v <= 0.0 { return Err("log of non-positive number".to_string()); }
            Ok(v.ln())
        }
        "log10" => {
            require_args(name, args, 1)?;
            let v = args[0];
            if v <= 0.0 { return Err("log10 of non-positive number".to_string()); }
            Ok(v.log10())
        }
        "log2" => {
            require_args(name, args, 1)?;
            let v = args[0];
            if v <= 0.0 { return Err("log2 of non-positive number".to_string()); }
            Ok(v.log2())
        }
        "exp" => {
            require_args(name, args, 1)?;
            Ok(args[0].exp())
        }
        "min" => {
            if args.is_empty() { return Err("min requires at least 1 argument".to_string()); }
            Ok(args.iter().cloned().fold(f64::INFINITY, f64::min))
        }
        "max" => {
            if args.is_empty() { return Err("max requires at least 1 argument".to_string()); }
            Ok(args.iter().cloned().fold(f64::NEG_INFINITY, f64::max))
        }
        "pow" => {
            require_args(name, args, 2)?;
            Ok(args[0].powf(args[1]))
        }
        "pi" => {
            if !args.is_empty() { return Err("pi takes no arguments".to_string()); }
            Ok(std::f64::consts::PI)
        }
        "e" => {
            if !args.is_empty() { return Err("e takes no arguments".to_string()); }
            Ok(std::f64::consts::E)
        }
        _ => Err(format!("Unknown function: '{}'. Available: sqrt, abs, floor, ceil, round, sin, cos, tan, log, log10, log2, exp, min, max, pow, pi, e", name)),
    }
}

fn require_args(name: &str, args: &[f64], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        Err(format!(
            "{} expects {} argument(s), got {}",
            name,
            expected,
            args.len()
        ))
    } else {
        Ok(())
    }
}

fn tool_bench_echo(args: &Value) -> Result<String, String> {
    let size = args.get("size").and_then(|v| v.as_u64()).unwrap_or(64) as usize;

    // Cap at 1MB to prevent memory issues on constrained edge instances
    let size = size.min(1_048_576);

    // Generate a repeating pattern of alphanumeric characters
    let pattern = b"abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut output = Vec::with_capacity(size);
    for i in 0..size {
        output.push(pattern[i % pattern.len()]);
    }
    // SAFETY: pattern is all ASCII, so the output is valid UTF-8
    Ok(unsafe { String::from_utf8_unchecked(output) })
}

fn tool_timestamp(_args: &Value) -> Result<String, String> {
    // On WASM/WASIX, std::time may not be available or accurate.
    // Use a compile-time constant + best-effort runtime clock.
    // For edge deployment, we use the WASI clock if available, otherwise
    // return a placeholder explaining the limitation.

    // Try to get time from WASI clock (works on WASIX)
    #[cfg(target_os = "wasi")]
    {
        // On WASI, we can use the raw clock via the __wasi API
        // For now, return a static response since we can't link WASI clock
        // without the wasi-libc bindings in this pure-Rust context
        let result = serde_json::json!({
            "note": "Timestamp not available in WASI sandbox without clock imports",
            "hint": "Use client-side timestamps for accurate time"
        });
        return Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()));
    }

    #[cfg(not(target_os = "wasi"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();

        let secs = now.as_secs();
        let millis = now.as_millis();

        // Format ISO 8601 UTC
        let iso = format_epoch_to_iso8601(secs);

        let result = serde_json::json!({
            "iso8601": iso,
            "unix_seconds": secs,
            "unix_milliseconds": millis
        });
        Ok(serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()))
    }
}

/// Format Unix epoch seconds to ISO 8601 string (UTC).
/// Pure computation — no OS dependencies.
#[cfg(not(target_os = "wasi"))]
fn format_epoch_to_iso8601(epoch_secs: u64) -> String {
    // Days since epoch
    let days = epoch_secs / 86400;
    let day_secs = epoch_secs % 86400;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    // Convert days since 1970-01-01 to year/month/day
    // Using the algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64; // day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hours, minutes, seconds
    )
}

fn tool_base64_encode(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'text' (string)".to_string())?;

    Ok(base64_encode(text.as_bytes()))
}

fn tool_base64_decode(args: &Value) -> Result<String, String> {
    let encoded = args
        .get("base64")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'base64' (string)".to_string())?;

    let decoded = base64_decode(encoded)?;
    String::from_utf8(decoded).map_err(|e| format!("Decoded bytes are not valid UTF-8: {}", e))
}

// Minimal Base64 implementation (no external crate needed)
const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    let chunks = data.chunks(3);
    for chunk in chunks {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        result.push(B64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(B64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(B64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(B64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

fn base64_decode(encoded: &str) -> Result<Vec<u8>, String> {
    let encoded = encoded.trim_end_matches('=');
    let mut result = Vec::with_capacity(encoded.len() * 3 / 4);

    let mut buf: u32 = 0;
    let mut bits_collected: u32 = 0;

    for ch in encoded.chars() {
        let val = match ch {
            'A'..='Z' => (ch as u32) - ('A' as u32),
            'a'..='z' => (ch as u32) - ('a' as u32) + 26,
            '0'..='9' => (ch as u32) - ('0' as u32) + 52,
            '+' => 62,
            '/' => 63,
            ' ' | '\n' | '\r' | '\t' => continue, // Skip whitespace
            _ => return Err(format!("Invalid Base64 character: '{}'", ch)),
        };

        buf = (buf << 6) | val;
        bits_collected += 6;

        if bits_collected >= 8 {
            bits_collected -= 8;
            result.push(((buf >> bits_collected) & 0xFF) as u8);
        }
    }

    Ok(result)
}

fn tool_hash_text(args: &Value) -> Result<String, String> {
    let text = args
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing required argument: 'text' (string)".to_string())?;

    let algorithm = args
        .get("algorithm")
        .and_then(|v| v.as_str())
        .unwrap_or("sha256");

    match algorithm {
        "sha256" => Ok(sha256_hex(text.as_bytes())),
        "sha1" => Ok(sha1_hex(text.as_bytes())),
        "md5" => Ok(md5_hex(text.as_bytes())),
        _ => Err(format!(
            "Unknown algorithm: '{}'. Supported: sha256, sha1, md5",
            algorithm
        )),
    }
}

// ---------------------------------------------------------------------------
// Minimal hash implementations (no external crate needed)
// ---------------------------------------------------------------------------

#[allow(clippy::chunks_exact_to_as_chunks)]
#[allow(clippy::needless_range_loop)]
fn sha256_hex(data: &[u8]) -> String {
    // SHA-256 implementation
    let h0: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    // Pre-processing: padding
    let msg_len_bits = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&msg_len_bits.to_be_bytes());

    // Process each 512-bit (64-byte) block
    let mut h = h0;
    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = String::with_capacity(64);
    for val in &h {
        result.push_str(&format!("{:08x}", val));
    }
    result
}

#[allow(clippy::chunks_exact_to_as_chunks)]
#[allow(clippy::needless_range_loop)]
fn sha1_hex(data: &[u8]) -> String {
    let mut h0: u32 = 0x67452301;
    let mut h1: u32 = 0xEFCDAB89;
    let mut h2: u32 = 0x98BADCFE;
    let mut h3: u32 = 0x10325476;
    let mut h4: u32 = 0xC3D2E1F0;

    let msg_len_bits = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&msg_len_bits.to_be_bytes());

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let (mut a, mut b, mut c, mut d, mut e) = (h0, h1, h2, h3, h4);

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };

            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }

        h0 = h0.wrapping_add(a);
        h1 = h1.wrapping_add(b);
        h2 = h2.wrapping_add(c);
        h3 = h3.wrapping_add(d);
        h4 = h4.wrapping_add(e);
    }

    format!("{:08x}{:08x}{:08x}{:08x}{:08x}", h0, h1, h2, h3, h4)
}

#[allow(clippy::chunks_exact_to_as_chunks)]
#[allow(clippy::needless_range_loop)]
#[allow(clippy::unnecessary_cast)]
fn md5_hex(data: &[u8]) -> String {
    // MD5 implementation
    let s: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];

    let k_vals: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];

    let msg_len_bits = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&msg_len_bits.to_le_bytes());

    let mut a0: u32 = 0x67452301;
    let mut b0: u32 = 0xefcdab89;
    let mut c0: u32 = 0x98badcfe;
    let mut d0: u32 = 0x10325476;

    for chunk in padded.chunks_exact(64) {
        let mut m = [0u32; 16];
        for i in 0..16 {
            m[i] = u32::from_le_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }

        let (mut a, mut b, mut c, mut d) = (a0, b0, c0, d0);

        for i in 0..64 {
            let (f, g) = match i {
                0..=15 => ((b & c) | ((!b) & d), i),
                16..=31 => ((d & b) | ((!d) & c), (5 * i + 1) % 16),
                32..=47 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | (!d)), (7 * i) % 16),
            };

            let f = f.wrapping_add(a).wrapping_add(k_vals[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(f.rotate_left(s[i]));
        }

        a0 = a0.wrapping_add(a);
        b0 = b0.wrapping_add(b);
        c0 = c0.wrapping_add(c);
        d0 = d0.wrapping_add(d);
    }

    // MD5 outputs each 32-bit word in little-endian byte order
    fn le_hex(w: u32) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}",
            w & 0xff,
            (w >> 8) & 0xff,
            (w >> 16) & 0xff,
            (w >> 24) & 0xff
        )
    }
    format!("{}{}{}{}", le_hex(a0), le_hex(b0), le_hex(c0), le_hex(d0))
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
const MP_WASM: &[u8] = include_bytes!("nested_mp.wasm");

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
const FUEL_LIMIT: u64 = 2_000_000_000; // per-call wasmi fuel budget

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
struct NestedRuntime {
    engine: wasmi::Engine,
    module: wasmi::Module,
}

// Compile the inner module ONCE (module cache), share the engine. The plugin
// registry itself lives ONLY in the volume file (registry.json), never cached
// in memory, so concurrent instances cannot serve stale or boot-pinned state.
#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
static NESTED: std::sync::OnceLock<std::sync::Mutex<NestedRuntime>> = std::sync::OnceLock::new();

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn registry_path() -> std::path::PathBuf {
    let dir = std::env::var("VELOCITY_STATE_DIR").unwrap_or_else(|_| "/data".to_string());
    std::path::Path::new(&dir).join("registry.json")
}

// --- Fix #4: external transactional store (Postgres) ---------------------
// When VELOCITY_PG_DSN is set the registry lives in Postgres (one row per
// plugin) instead of a volume file. Row-per-plugin + a single-statement
// `INSERT ... ON CONFLICT DO UPDATE` gives atomicity/isolation ACROSS instances
// that do NOT share a filesystem — the property file locking (#3) cannot provide
// because a lock is only advisory within one shared FS. Native only: the
// postgres client cannot be compiled for the wasm32 Edge target, so the Edge
// path keeps using the file + lock backend (see the docs' relay note for #4 on Edge).
#[cfg(not(target_arch = "wasm32"))]
mod pgreg {
    use std::collections::HashMap;

    pub fn dsn() -> Option<String> {
        std::env::var("VELOCITY_PG_DSN").ok().filter(|s| !s.is_empty())
    }
    fn table() -> String {
        std::env::var("VELOCITY_REG_TABLE").unwrap_or_else(|_| "mcp_registry".to_string())
    }
    fn client(dsn: &str) -> Result<postgres::Client, String> {
        postgres::Client::connect(dsn, postgres::NoTls).map_err(|e| format!("pg connect: {e}"))
    }

    pub fn ensure_table() -> Result<(), String> {
        let dsn = dsn().ok_or("no dsn")?;
        let mut c = client(&dsn)?;
        let q = format!(
            "CREATE TABLE IF NOT EXISTS {t}(name text primary key, code text not null, \
             version bigint not null default 1, updated_at timestamptz not null default now())",
            t = table()
        );
        c.batch_execute(&q).map_err(|e| e.to_string())
    }

    pub fn load_all() -> HashMap<String, String> {
        let Some(dsn) = dsn() else { return HashMap::new() };
        let Ok(mut c) = client(&dsn) else { return HashMap::new() };
        let q = format!("SELECT name, code FROM {}", table());
        match c.query(&q, &[]) {
            Ok(rows) => rows
                .iter()
                .map(|r| (r.get::<_, String>("name"), r.get::<_, String>("code")))
                .collect(),
            Err(_) => HashMap::new(),
        }
    }

    fn count(c: &mut postgres::Client) -> Result<usize, String> {
        let q = format!("SELECT count(*) FROM {}", table());
        let row = c.query_one(&q, &[]).map_err(|e| e.to_string())?;
        Ok(row.get::<_, i64>(0) as usize)
    }

    pub fn register(name: &str, code: &str) -> Result<usize, String> {
        let dsn = dsn().ok_or("no dsn")?;
        let mut c = client(&dsn)?;
        let q = format!(
            "INSERT INTO {t}(name, code, version) VALUES($1, $2, 1) \
             ON CONFLICT(name) DO UPDATE SET code = EXCLUDED.code, \
             version = {t}.version + 1, updated_at = now()",
            t = table()
        );
        c.execute(&q, &[&name.to_string(), &code.to_string()])
            .map_err(|e| e.to_string())?;
        count(&mut c)
    }

    pub fn unregister(name: &str) -> Result<Option<usize>, String> {
        let dsn = dsn().ok_or("no dsn")?;
        let mut c = client(&dsn)?;
        let q = format!("DELETE FROM {} WHERE name = $1", table());
        let n = c.execute(&q, &[&name.to_string()]).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(count(&mut c)?))
    }

    #[cfg(test)]
    pub fn truncate() {
        if let Some(dsn) = dsn() {
            if let Ok(mut c) = client(&dsn) {
                let _ = c.batch_execute(&format!("TRUNCATE {}", table()));
            }
        }
    }

    #[cfg(test)]
    pub fn version_of(name: &str) -> i64 {
        let Some(dsn) = dsn() else { return 0 };
        let Ok(mut c) = client(&dsn) else { return 0 };
        let q = format!("SELECT version FROM {} WHERE name = $1", table());
        match c.query_one(&q, &[&name.to_string()]) {
            Ok(r) => r.get(0),
            Err(_) => 0,
        }
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn load_registry() -> std::collections::HashMap<String, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if pgreg::dsn().is_some() {
        return pgreg::load_all();
    }
    #[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
    if std::env::var("DB_HOST").map(|h| !h.is_empty()).unwrap_or(false) {
        return pgedge::load_all();
    }
    match std::fs::read_to_string(registry_path()) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => std::collections::HashMap::new(),
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn save_registry(map: &std::collections::HashMap<String, String>) -> Result<(), String> {
    let path = registry_path();
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let body = serde_json::to_vec(map).map_err(|e| e.to_string())?;
    // Fix #3(a) — atomic publish. Write to a sibling temp file, then rename over
    // the target. rename is atomic within a filesystem, so a concurrent reader
    // sees either the old or the new registry.json, never a torn/partial file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &body).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn lock_path() -> std::path::PathBuf {
    registry_path().with_extension("lock")
}

// Fix #3(b) — cross-process exclusive lock for the read-modify-write. Created
// with `create_new` (O_CREAT|O_EXCL), which the OS makes atomic for both threads
// and separate processes, so only one writer enters the critical section at a
// time. Losers spin briefly; a lock older than STALE_MS is presumed abandoned by
// a crashed holder and reclaimed so the registry can never wedge permanently.
#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn acquire_registry_lock() -> Result<std::path::PathBuf, String> {
    use std::time::{Duration, Instant};
    const STALE_MS: u64 = 5_000;
    const TIMEOUT: Duration = Duration::from_millis(10_000);
    let lock = lock_path();
    if let Some(p) = lock.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let start = Instant::now();
    loop {
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&lock) {
            Ok(_) => return Ok(lock),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let aged = std::fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .map(|t| t.elapsed().map(|d| d.as_millis() as u64).unwrap_or(0))
                    .unwrap_or(0);
                if aged > STALE_MS {
                    let _ = std::fs::remove_file(&lock);
                    continue;
                }
                if start.elapsed() > TIMEOUT {
                    return Err("registry lock timeout".into());
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(e) => return Err(format!("lock {}: {e}", lock.display())),
        }
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn release_registry_lock(lock: &std::path::Path) {
    let _ = std::fs::remove_file(lock);
}

// Fix #2 + #3 — atomic read-modify-write merge: serialise with the exclusive
// lock, re-read the volume file, apply the single change, publish it atomically.
#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn merge_register(name: &str, code: &str) -> Result<usize, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if pgreg::dsn().is_some() {
        return pgreg::register(name, code);
    }
    #[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
    if std::env::var("DB_HOST").map(|h| !h.is_empty()).unwrap_or(false) {
        return pgedge::register(name, code);
    }
    let lock = acquire_registry_lock()?;
    let result = (|| {
        let mut cur = load_registry();
        cur.insert(name.to_string(), code.to_string());
        save_registry(&cur)?;
        Ok(cur.len())
    })();
    release_registry_lock(&lock);
    result
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn merge_unregister(name: &str) -> Result<Option<usize>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if pgreg::dsn().is_some() {
        return pgreg::unregister(name);
    }
    #[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
    if std::env::var("DB_HOST").map(|h| !h.is_empty()).unwrap_or(false) {
        return pgedge::unregister(name);
    }
    let lock = acquire_registry_lock()?;
    let result = (|| {
        let mut cur = load_registry();
        match cur.remove(name) {
            Some(_) => {
                save_registry(&cur)?;
                Ok(Some(cur.len()))
            }
            None => Ok(None),
        }
    })();
    release_registry_lock(&lock);
    result
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn nested_rt() -> &'static std::sync::Mutex<NestedRuntime> {
    NESTED.get_or_init(|| {
        let mut cfg = wasmi::Config::default();
        cfg.consume_fuel(true);
        let engine = wasmi::Engine::new(&cfg);
        // The one-time cost (the 12-18 ms we measured) now happens once per process, not per call.
        let module = wasmi::Module::new(&engine, MP_WASM)
            .expect("failed to compile nested micropython module");
        std::sync::Mutex::new(NestedRuntime {
            engine,
            module,
        })
    })
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o
}

/// Run MicroPython source with a fuel budget on the cached module. Returns
/// (stdout output, fuel consumed). Err on trap / out-of-fuel.
#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn nested_run_src(rt: &NestedRuntime, src: &str) -> Result<(String, u64), String> {
    use wasmi::{Caller, Linker, Store};
    const PYSTACK: i32 = 16384;
    const HEAP: i32 = 256 * 1024;
    const EXEC_SLOT: usize = 512 * 1024;

    let mut store = Store::new(&rt.engine, ());
    store
        .set_fuel(FUEL_LIMIT)
        .map_err(|e| format!("set_fuel: {e}"))?;

    let mut linker = <Linker<()>>::new(&rt.engine);
    macro_rules! wstub {
        ($n:expr, $c:expr) => {
            linker
                .func_wrap("wasi_snapshot_preview1", $n, $c)
                .map_err(|e| format!("stub {}: {e}", $n))?;
        };
    }
    wstub!("args_get", |_c: Caller<'_, ()>, _a: i32, _b: i32| -> i32 { 0 });
    wstub!("args_sizes_get", |_c: Caller<'_, ()>, _a: i32, _b: i32| -> i32 { 0 });
    wstub!("clock_time_get", |_c: Caller<'_, ()>, _i: i32, _p: i64, _o: i32| -> i32 { 0 });
    wstub!("fd_close", |_c: Caller<'_, ()>, _f: i32| -> i32 { 0 });
    wstub!("fd_seek", |_c: Caller<'_, ()>, _f: i32, _o: i64, _w: i32, _p: i32| -> i32 { 0 });
    wstub!("fd_write", |_c: Caller<'_, ()>, _f: i32, _i: i32, _l: i32, _n: i32| -> i32 { 0 });
    wstub!("proc_exit", |_c: Caller<'_, ()>, _code: i32| {});

    let instance = linker
        .instantiate(&mut store, &rt.module)
        .and_then(|p| p.start(&mut store))
        .map_err(|e| format!("instantiate: {e}"))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or_else(|| "no memory".to_string())?;

    let bytes = src.as_bytes();
    let need = EXEC_SLOT + bytes.len() + 4096;
    loop {
        let cur = memory.data(&store).len();
        if cur >= need {
            break;
        }
        let delta = ((need - cur) / 65536 + 1) as u32;
        memory
            .grow(&mut store, wasmi::core::Pages::new(delta).unwrap())
            .map_err(|_| "memory grow failed".to_string())?;
    }

    let init = instance
        .get_typed_func::<(i32, i32), i32>(&store, "mp_wasi_init")
        .map_err(|e| e.to_string())?;
    if init.call(&mut store, (PYSTACK, HEAP)).map_err(|e| e.to_string())? != 0 {
        return Err("mp_wasi_init failed".into());
    }
    {
        let mem = memory.data_mut(&mut store);
        mem[EXEC_SLOT..EXEC_SLOT + bytes.len()].copy_from_slice(bytes);
    }
    let exec = instance
        .get_typed_func::<(i32, i32), i32>(&store, "mp_wasi_exec")
        .map_err(|e| e.to_string())?;
    let rc = exec
        .call(&mut store, (EXEC_SLOT as i32, bytes.len() as i32))
        .map_err(|e| format!("exec trap: {e}"))?;
    let get_out = instance
        .get_typed_func::<(), i32>(&store, "mp_wasi_get_output")
        .map_err(|e| e.to_string())?;
    let get_len = instance
        .get_typed_func::<(), i32>(&store, "mp_wasi_get_output_len")
        .map_err(|e| e.to_string())?;
    let op = get_out.call(&mut store, ()).map_err(|e| e.to_string())? as usize;
    let ol = get_len.call(&mut store, ()).map_err(|e| e.to_string())? as usize;
    let output = {
        let mem = memory.data(&store);
        let end = (op + ol).min(mem.len());
        String::from_utf8_lossy(&mem[op.min(mem.len())..end]).to_string()
    };
    let consumed = FUEL_LIMIT.saturating_sub(store.get_fuel().unwrap_or(0));
    if rc != 0 {
        return Err(format!("python error (rc {rc}): {}", output.trim()));
    }
    Ok((output, consumed))
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn nested_register(name: &str, code: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 64 {
        return Err("invalid plugin name".into());
    }
    let rt = nested_rt();
    let _g = rt.lock().map_err(|_| "registry poisoned".to_string())?;
    // Fix #2: merge into whatever is on disk right now, not a whole-map dump.
    let total = merge_register(name, code)?;
    Ok(format!("registered plugin '{}' (total {}; persisted=true)", name, total))
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn nested_unregister(name: &str) -> Result<String, String> {
    let rt = nested_rt();
    let _g = rt.lock().map_err(|_| "registry poisoned".to_string())?;
    match merge_unregister(name)? {
        Some(total) => {
            Ok(format!("unregistered plugin '{}' (total {}; persisted=true)", name, total))
        }
        None => Err(format!("no plugin named '{}'", name)),
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn nested_plugin_names() -> Vec<String> {
    // Fix #1: refresh-through — always read the live volume file, never a cache.
    let mut names: Vec<String> = load_registry().into_keys().collect();
    names.sort();
    names
}

/// Guest-side probe of the Wasmer-managed Postgres: can the WASIX binary reach
/// the DB over raw TCP, and does the server require TLS? Sends the 8-byte PG
/// SSLRequest magic and reports the server's single-byte reply.
#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn pg_probe() -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let host = std::env::var("DB_HOST").unwrap_or_default();
    let port = std::env::var("DB_PORT").unwrap_or_else(|_| "5432".into());
    let user = std::env::var("DB_USERNAME").unwrap_or_default();
    let dbname = std::env::var("DB_NAME").unwrap_or_default();
    let pw_set = std::env::var("DB_PASSWORD").map(|p| !p.is_empty()).unwrap_or(false);
    if host.is_empty() {
        return Ok("DB_HOST not set (postgres capability not injected into this instance)".into());
    }
    let addr = format!("{}:{}", host, port);
    let mut sock = match TcpStream::connect(&addr) {
        Ok(s) => s,
        Err(e) => return Ok(format!("addr={} connect=FAIL err={}", addr, e)),
    };
    let _ = sock.set_read_timeout(Some(Duration::from_secs(8)));
    let _ = sock.set_write_timeout(Some(Duration::from_secs(8)));

    // Postgres SSLRequest: Int32 length=8, Int32 code=80877103.
    let mut req = Vec::with_capacity(8);
    req.extend_from_slice(&8i32.to_be_bytes());
    req.extend_from_slice(&80877103i32.to_be_bytes());
    if let Err(e) = sock.write_all(&req) {
        return Ok(format!("addr={} connect=ok write_sslreq=FAIL {}", addr, e));
    }
    let mut b = [0u8; 1];
    if let Err(e) = sock.read_exact(&mut b) {
        return Ok(format!("addr={} connect=ok sslreq_read=FAIL {}", addr, e));
    }
    let reply = b[0] as char; // 'S' = TLS required, 'N' = plaintext allowed
    let mut out = format!(
        "addr={} connect=OK env(user={},db={},pw_set={}) sslrequest_reply='{}' => {}",
        addr, user, dbname, pw_set, reply,
        if reply == 'S' { "TLS REQUIRED" } else if reply == 'N' { "PLAINTEXT ALLOWED" } else { "UNEXPECTED" }
    );

    if reply == 'N' {
        // Try a minimal plaintext StartupMessage (protocol 3.0).
        let sm = pg_startup_message(&user, &dbname);
        let _ = sock.write_all(&sm);
        let mut rb = [0u8; 5];
        out.push_str(&match sock.read_exact(&mut rb) {
            Ok(()) => format!(" plaintext_startup_msg_type='{}'", rb[0] as char),
            Err(e) => format!(" plaintext_startup_read=FAIL {}", e),
        });
        return Ok(out);
    }

    // reply == 'S': complete a TLS handshake and send StartupMessage over it.
    #[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
    {
        out.push_str(&tls_startup_probe(sock, &host, &user, &dbname));
    }
    #[cfg(not(all(target_arch = "wasm32", target_vendor = "wasmer")))]
    {
        let _ = (&user, &dbname);
        out.push_str(" (tls-startup-probe available on wasmer target only)");
    }
    Ok(out)
}

/// Build a Postgres v3 StartupMessage (no length-prefixed per-field; it is a raw
/// message type). Returns bytes WITHOUT a message type byte (StartupMessage has none).
#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn pg_startup_message(user: &str, db: &str) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(&196608i32.to_be_bytes()); // 3 << 16
    for (k, v) in [("user", user), ("database", db), ("application_name", "velprobe")] {
        body.extend_from_slice(k.as_bytes());
        body.push(0);
        body.extend_from_slice(v.as_bytes());
        body.push(0);
    }
    body.push(0);
    let mut m = Vec::new();
    m.extend_from_slice(&((body.len() + 4) as i32).to_be_bytes());
    m.extend_from_slice(&body);
    m
}

/// Read a single Postgres backend message (type byte + Int32 length + payload).
#[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
fn pg_read_msg<R: std::io::Read>(r: &mut R) -> Result<(u8, Vec<u8>), String> {
    let mut t = [0u8; 1];
    r.read_exact(&mut t).map_err(|e| format!("msgtype read: {e}"))?;
    let mut l = [0u8; 4];
    r.read_exact(&mut l).map_err(|e| format!("msglen read: {e}"))?;
    let len = i32::from_be_bytes(l) as usize;
    if len < 4 || len > 1_000_000 {
        return Err(format!("bad msg len {len}"));
    }
    let mut payload = vec![0u8; len - 4];
    r.read_exact(&mut payload).map_err(|e| format!("payload read: {e}"))?;
    Ok((t[0], payload))
}

/// rustls TLS handshake + PG StartupMessage over it; report the auth request type.
#[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
fn tls_startup_probe(sock: std::net::TcpStream, host: &str, user: &str, db: &str) -> String {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::ring;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
    use std::io::Write;
    use std::sync::Arc;

    #[derive(Debug)]
    struct AcceptAll;
    impl ServerCertVerifier for AcceptAll {
        fn verify_server_cert(
            &self,
            _e: &CertificateDer<'_>,
            _i: &[CertificateDer<'_>],
            _n: &ServerName<'_>,
            _o: &[u8],
            _t: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _m: &[u8],
            _c: &CertificateDer<'_>,
            _d: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _m: &[u8],
            _c: &CertificateDer<'_>,
            _d: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            ring::default_provider().signature_verification_algorithms.supported_schemes()
        }
    }

    let provider = ring::default_provider();
    let cfg = match ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
    {
        Ok(b) => b
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAll))
            .with_no_client_auth(),
        Err(e) => return format!(" tls=CFG_ERR {e}"),
    };

    let sni = match ServerName::try_from(host.to_string()) {
        Ok(s) => s,
        Err(e) => return format!(" tls=SNI_ERR {e}"),
    };
    let cc = match ClientConnection::new(Arc::new(cfg), sni) {
        Ok(c) => c,
        Err(e) => return format!(" tls=CONN_ERR {e}"),
    };
    let mut tls = StreamOwned::new(cc, sock);
    if let Err(e) = tls.write_all(&pg_startup_message(user, db)) {
        return format!(" tls=HANDSHAKE/WRITE_ERR {e}");
    }
    let _ = tls.flush();
    match pg_read_msg(&mut tls) {
        Ok((t, payload)) => {
            if t == b'R' && payload.len() >= 4 {
                let code = i32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]);
                let meaning: String = match code {
                    0 => "Ok".into(),
                    3 => "CleartextPassword".into(),
                    5 => "MD5Password".into(),
                    10 => "SASL".into(),
                    c => format!("code{c}"),
                };
                let mut s = format!(" tls=OK startup_replied='R' auth={meaning}({code})");
                if code == 10 {
                    let rest = &payload[4..];
                    if let Some(first) = rest.split(|b| *b == 0).next() {
                        s.push_str(&format!(" sasl_mech='{}'", String::from_utf8_lossy(first)));
                    }
                }
                s
            } else if t == b'E' {
                format!(
                    " tls=OK startup_replied='E'(ErrorResponse) {}",
                    String::from_utf8_lossy(&payload)
                )
            } else {
                format!(" tls=OK startup_replied='{}'", t as char)
            }
        }
        Err(e) => format!(" tls=OK msg_read_FAIL {e}"),
    }
}

/// In-guest Postgres-over-TLS client (fix #4 running INSIDE the Edge WASM).
/// rustls TLS + SCRAM-SHA-256 + parameterized extended-query, no external DB crate.
#[cfg(all(target_arch = "wasm32", target_vendor = "wasmer"))]
mod pgedge {
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    use base64::engine::general_purpose::STANDARD as B64;
    use base64::Engine;
    use hmac::{Hmac, Mac};
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::crypto::ring;
    use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
    use rustls::{ClientConfig, ClientConnection, DigitallySignedStruct, SignatureScheme, StreamOwned};
    use sha2::{Digest, Sha256};

    type Tls = StreamOwned<ClientConnection, TcpStream>;
    static CTR: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    struct AcceptAll;
    impl ServerCertVerifier for AcceptAll {
        fn verify_server_cert(&self, _e: &CertificateDer<'_>, _i: &[CertificateDer<'_>], _n: &ServerName<'_>, _o: &[u8], _t: UnixTime) -> Result<ServerCertVerified, rustls::Error> { Ok(ServerCertVerified::assertion()) }
        fn verify_tls12_signature(&self, _m: &[u8], _c: &CertificateDer<'_>, _d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> { Ok(HandshakeSignatureValid::assertion()) }
        fn verify_tls13_signature(&self, _m: &[u8], _c: &CertificateDer<'_>, _d: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, rustls::Error> { Ok(HandshakeSignatureValid::assertion()) }
        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> { ring::default_provider().signature_verification_algorithms.supported_schemes() }
    }

    fn env(k: &str, d: &str) -> String { std::env::var(k).unwrap_or_else(|_| d.to_string()) }

    fn hmac256(key: &[u8], msg: &[u8]) -> Vec<u8> {
        let mut m = Hmac::<Sha256>::new_from_slice(key).unwrap();
        m.update(msg);
        m.finalize().into_bytes().to_vec()
    }
    fn sha256(data: &[u8]) -> Vec<u8> { Sha256::digest(data).to_vec() }
    fn xor(a: &[u8], b: &[u8]) -> Vec<u8> { a.iter().zip(b).map(|(x, y)| x ^ y).collect() }
    fn scram_escape(s: &str) -> String { s.replace('=', "=3D").replace(',', "=2C") }

    // ---- message framing ----
    fn send_msg(w: &mut impl Write, ty: u8, body: &[u8]) -> std::io::Result<()> {
        w.write_all(&[ty])?;
        w.write_all(&((body.len() as i32 + 4) as i32).to_be_bytes())?;
        w.write_all(body)
    }
    fn read_msg(r: &mut impl Read) -> Result<(u8, Vec<u8>), String> {
        let mut t = [0u8; 1];
        r.read_exact(&mut t).map_err(|e| format!("type: {e}"))?;
        let mut l = [0u8; 4];
        r.read_exact(&mut l).map_err(|e| format!("len: {e}"))?;
        let len = i32::from_be_bytes(l) as usize;
        if len < 4 || len > 10_000_000 {
            return Err(format!("bad len {len}"));
        }
        let mut p = vec![0u8; len - 4];
        r.read_exact(&mut p).map_err(|e| format!("body: {e}"))?;
        Ok((t[0], p))
    }
    fn put_cstr(v: &mut Vec<u8>, s: &str) { v.extend_from_slice(s.as_bytes()); v.push(0); }
    fn err_text(p: &[u8]) -> String {
        let mut msg = String::new();
        let mut it = p.split(|b| *b == 0);
        while let Some(f) = it.next() {
            if f.len() >= 2 && f[0] == b'M' {
                msg = String::from_utf8_lossy(&f[1..]).to_string();
            }
        }
        msg
    }

    // ---- connect + TLS + SCRAM auth; returns authenticated stream, ready for queries ----
    fn connect_auth() -> Result<Tls, String> {
        let host = env("DB_HOST", "");
        let port = env("DB_PORT", "5432");
        let user = env("DB_USERNAME", "");
        let pass = env("DB_PASSWORD", "");
        let db = env("DB_NAME", "");
        if host.is_empty() {
            return Err("DB_HOST not set".into());
        }
        let sock = TcpStream::connect(format!("{}:{}", host, port)).map_err(|e| format!("tcp: {e}"))?;
        let mut sock = sock;
        sock.write_all(&8i32.to_be_bytes()).map_err(|e| e.to_string())?;
        sock.write_all(&80877103i32.to_be_bytes()).map_err(|e| e.to_string())?;
        let mut s = [0u8; 1];
        sock.read_exact(&mut s).map_err(|e| format!("sslresp: {e}"))?;
        if s[0] != b'S' {
            return Err(format!("server refused TLS (reply '{}')", s[0] as char));
        }
        let provider = ring::default_provider();
        let cfg = ClientConfig::builder_with_provider(Arc::new(provider))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAll))
            .with_no_client_auth();
        let sni = ServerName::try_from(host).map_err(|e| e.to_string())?;
        let cc = ClientConnection::new(Arc::new(cfg), sni).map_err(|e| e.to_string())?;
        let mut tls = StreamOwned::new(cc, sock);

        // StartupMessage (protocol 3.0)
        let mut sb: Vec<u8> = Vec::new();
        sb.extend_from_slice(&196608i32.to_be_bytes());
        for (k, v) in [("user", user.as_str()), ("database", db.as_str()), ("application_name", "veledge")] {
            put_cstr(&mut sb, k);
            put_cstr(&mut sb, v);
        }
        sb.push(0);
        let mut start = Vec::new();
        start.extend_from_slice(&((sb.len() + 4) as i32).to_be_bytes());
        start.extend_from_slice(&sb);
        tls.write_all(&start).map_err(|e| format!("startup: {e}"))?;
        tls.flush().map_err(|e| e.to_string())?;

        let client_nonce = {
            let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
            format!("{:016x}{:08x}", t, CTR.fetch_add(1, Ordering::Relaxed))
        };
        let cf_bare = format!("n={},r={}", scram_escape(&user), client_nonce);

        // Read until AuthenticationSASL (R code 10), then do the SCRAM dance.
        loop {
            let (ty, body) = read_msg(&mut tls)?;
            match ty {
                b'R' => {
                    let code = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
                    match code {
                        0 => {} // AuthOk — keep draining to the startup ReadyForQuery below
                        10 => {
                            // SASL: choose SCRAM-SHA-256 (plain, no channel binding)
                            let mut p: Vec<u8> = Vec::new();
                            put_cstr(&mut p, "SCRAM-SHA-256");
                            let first = format!("n,,{}", cf_bare);
                            p.extend_from_slice(&(first.len() as i32).to_be_bytes());
                            p.extend_from_slice(first.as_bytes());
                            send_msg(&mut tls, b'p', &p).map_err(|e| e.to_string())?;
                            tls.flush().map_err(|e| e.to_string())?;
                        }
                        11 => {
                            // SASLContinue: server-first-message
                            let sf = String::from_utf8_lossy(&body[4..]).to_string();
                            let mut server_nonce = String::new();
                            let mut salt_b64 = String::new();
                            let mut iters: u32 = 4096;
                            for kv in sf.split(',') {
                                if let Some(v) = kv.strip_prefix("r=") { server_nonce = v.to_string(); }
                                else if let Some(v) = kv.strip_prefix("s=") { salt_b64 = v.to_string(); }
                                else if let Some(v) = kv.strip_prefix("i=") { iters = v.parse().unwrap_or(4096); }
                            }
                            let salt = B64.decode(&salt_b64).map_err(|e| format!("salt: {e}"))?;
                            let salted = {
                                let mut out = [0u8; 32];
                                pbkdf2::pbkdf2_hmac::<Sha256>(pass.as_bytes(), &salt, iters, &mut out);
                                out.to_vec()
                            };
                            let client_key = hmac256(&salted, b"Client Key");
                            let stored_key = sha256(&client_key);
                            let cfp = format!("c={},r={}", B64.encode("n,,"), server_nonce);
                            // AuthMessage = client-first-bare, server-first, client-final-without-proof
                            let auth_msg = format!("{},{},{}", cf_bare, sf, cfp);
                            let client_sig = hmac256(&stored_key, auth_msg.as_bytes());
                            let proof = xor(&client_key, &client_sig);
                            let final_msg = format!("{},p={}", cfp, B64.encode(&proof));
                            send_msg(&mut tls, b'p', final_msg.as_bytes()).map_err(|e| e.to_string())?;
                            tls.flush().map_err(|e| e.to_string())?;
                            // stash auth_msg for server-key verification
                            LAST_AUTH_MSG.with(|c| *c.borrow_mut() = auth_msg);
                            LAST_SALTED.with(|c| *c.borrow_mut() = salted);
                        }
                        12 => {
                            // SASLFinal: verify server signature
                            let sv = String::from_utf8_lossy(&body[4..]).to_string();
                            let salted = LAST_SALTED.with(|c| c.borrow().clone());
                            let auth_msg = LAST_AUTH_MSG.with(|c| c.borrow().clone());
                            let server_key = hmac256(&salted, b"Server Key");
                            let server_sig = hmac256(&server_key, auth_msg.as_bytes());
                            if let Some(v) = sv.strip_prefix("v=") {
                                if B64.decode(v).map(|d| d != server_sig).unwrap_or(true) {
                                    return Err("SCRAM server signature mismatch".into());
                                }
                            }
                        }
                        c => return Err(format!("unexpected auth code {c}")),
                    }
                }
                b'E' => return Err(format!("pg error at auth: {}", err_text(&body))),
                b'Z' => break, // ReadyForQuery (some servers)
                _ => {} // S ParameterStatus, K BackendKeyData, etc.
            }
        }
        Ok(tls)
    }

    thread_local! {
        static LAST_AUTH_MSG: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
        static LAST_SALTED: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::new(Vec::new());
    }

    // Parameterized query: returns rows of text columns.
    fn query(tls: &mut Tls, sql: &str, params: &[&str]) -> Result<Vec<Vec<Option<String>>>, String> {
        // Parse
        let mut p: Vec<u8> = Vec::new();
        p.push(0); // unnamed statement
        put_cstr(&mut p, sql);
        p.extend_from_slice(&(params.len() as i16).to_be_bytes());
        for _ in params { p.extend_from_slice(&0i32.to_be_bytes()); }
        send_msg(tls, b'P', &p).map_err(|e| e.to_string())?;
        // Bind
        let mut b: Vec<u8> = Vec::new();
        b.push(0); // portal
        b.push(0); // statement
        b.extend_from_slice(&0i16.to_be_bytes()); // no param format codes (all text)
        b.extend_from_slice(&(params.len() as i16).to_be_bytes());
        for v in params {
            b.extend_from_slice(&(v.len() as i32).to_be_bytes());
            b.extend_from_slice(v.as_bytes());
        }
        b.extend_from_slice(&0i16.to_be_bytes()); // no result format codes (text)
        send_msg(tls, b'B', &b).map_err(|e| e.to_string())?;
        // Describe portal + Execute + Sync
        send_msg(tls, b'D', &[b'P', 0]).map_err(|e| e.to_string())?;
        let mut e: Vec<u8> = Vec::new();
        e.push(0);
        e.extend_from_slice(&0i32.to_be_bytes()); // all rows
        send_msg(tls, b'E', &e).map_err(|e| e.to_string())?;
        send_msg(tls, b'S', &[]).map_err(|e| e.to_string())?;
        tls.flush().map_err(|e| e.to_string())?;

        let mut rows: Vec<Vec<Option<String>>> = Vec::new();
        let mut err: Option<String> = None;
        loop {
            let (ty, body) = read_msg(tls)?;
            match ty {
                b'D' => {
                    // DataRow: Int16 ncols, then per col Int32 len + bytes (-1 null)
                    let n = i16::from_be_bytes([body[0], body[1]]) as usize;
                    let mut off = 2;
                    let mut row = Vec::with_capacity(n);
                    for _ in 0..n {
                        let l = i32::from_be_bytes([body[off], body[off+1], body[off+2], body[off+3]]) as i64;
                        off += 4;
                        if l < 0 { row.push(None); }
                        else {
                            let le = l as usize;
                            row.push(Some(String::from_utf8_lossy(&body[off..off+le]).to_string()));
                            off += le;
                        }
                    }
                    rows.push(row);
                }
                b'E' => err = Some(err_text(&body)),
                b'C' => {} // CommandComplete
                b'Z' => break, // ReadyForQuery
                _ => {}
            }
        }
        if let Some(m) = err { return Err(m); }
        Ok(rows)
    }

    fn ensure_table(tls: &mut Tls) -> Result<(), String> {
        query(tls,
            "CREATE TABLE IF NOT EXISTS mcp_registry(name text primary key, code text not null, version bigint not null default 1, updated_at timestamptz not null default now())",
            &[])?;
        Ok(())
    }

    // Fresh, authenticated, table-ready connection.
    fn fresh_conn() -> Result<Tls, String> {
        let mut tls = connect_auth()?;
        ensure_table(&mut tls)?;
        Ok(tls)
    }

    // Per-worker keep-alive: reuse one authenticated connection so warm ops skip
    // the TLS + SCRAM handshake. If a reused connection errors (idle-closed /
    // stale), discard it, reconnect once, and retry.
    thread_local! {
        static CONN: std::cell::RefCell<Option<Tls>> = const { std::cell::RefCell::new(None) };
    }

    fn with_conn<F, R>(f: F) -> Result<R, String>
    where
        F: for<'a> Fn(&'a mut Tls) -> Result<R, String>,
    {
        if let Some(mut c) = CONN.with(|x| x.borrow_mut().take()) {
            match f(&mut c) {
                Ok(r) => {
                    CONN.with(|x| *x.borrow_mut() = Some(c));
                    return Ok(r);
                }
                Err(_) => {} // assume stale: drop c, fall through to a fresh one
            }
        }
        let mut c = fresh_conn()?;
        let r = f(&mut c)?;
        CONN.with(|x| *x.borrow_mut() = Some(c));
        Ok(r)
    }

    pub fn load_all() -> HashMap<String, String> {
        let mut out = HashMap::new();
        if let Ok(rows) = with_conn(|t| query(t, "SELECT name, code FROM mcp_registry", &[])) {
            for r in rows {
                if r.len() >= 2 {
                    if let (Some(n), Some(c)) = (r[0].clone(), r[1].clone()) {
                        out.insert(n, c);
                    }
                }
            }
        }
        out
    }

    pub fn register(name: &str, code: &str) -> Result<usize, String> {
        with_conn(|t| {
            query(t,
                "INSERT INTO mcp_registry(name,code,version) VALUES($1,$2,1) ON CONFLICT(name) DO UPDATE SET code=EXCLUDED.code, version=mcp_registry.version+1, updated_at=now()",
                &[name, code])?;
            let rows = query(t, "SELECT count(*) FROM mcp_registry", &[])?;
            rows.first()
                .and_then(|r| r.first())
                .and_then(|v| v.as_deref())
                .and_then(|s| s.parse::<usize>().ok())
                .ok_or_else(|| "no count row".to_string())
        })
    }

    pub fn unregister(name: &str) -> Result<Option<usize>, String> {
        with_conn(|t| {
            let exists = query(t, "SELECT 1 FROM mcp_registry WHERE name=$1", &[name])?;
            if exists.is_empty() {
                return Ok(None);
            }
            query(t, "DELETE FROM mcp_registry WHERE name=$1", &[name])?;
            let cnt = query(t, "SELECT count(*) FROM mcp_registry", &[])?;
            let remaining = cnt
                .first()
                .and_then(|r| r.first())
                .and_then(|v| v.as_deref())
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0);
            Ok(Some(remaining))
        })
    }
}

#[cfg(any(not(target_arch = "wasm32"), target_vendor = "wasmer"))]
fn nested_call_plugin(name: &str, args_json: &str) -> Result<String, String> {
    // Fix #1: read the plugin source fresh from disk on every call.
    let code = load_registry()
        .remove(name)
        .ok_or_else(|| format!("no plugin '{}'", name))?;
    let wrapped = format!(
        "import json\nargs = json.loads(\"{}\")\n{}\n",
        json_escape(args_json),
        code
    );
    let rt = nested_rt();
    let g = rt.lock().map_err(|_| "registry poisoned".to_string())?;
    match nested_run_src(&g, &wrapped) {
        Ok((out, fuel)) => Ok(format!("{}|fuel={}", out.trim_end(), fuel)),
        Err(e) => {
            // out-of-fuel traps surface here; give a resource-limit message.
            let msg = if e.contains("fuel") || e.contains("Resource") || e.contains("trap") {
                format!("resource limit exceeded (fuel<{}): {}", FUEL_LIMIT, e)
            } else {
                e
            };
            Err(msg)
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_echo() {
        let args = serde_json::json!({"message": "hello world"});
        assert_eq!(tool_echo(&args).unwrap(), "hello world");
    }

    #[test]
    fn test_echo_missing_arg() {
        let args = serde_json::json!({});
        assert!(tool_echo(&args).is_err());
    }

    #[test]
    fn test_json_format() {
        let args = serde_json::json!({"json": r#"{"a":1,"b":[2,3]}"#});
        let result = tool_json_format(&args).unwrap();
        assert!(result.contains("\"a\": 1"));
        assert!(result.contains("\"b\": ["));
    }

    #[test]
    fn test_json_format_invalid() {
        let args = serde_json::json!({"json": "not json"});
        assert!(tool_json_format(&args).is_err());
    }

    #[test]
    fn test_text_count() {
        let args = serde_json::json!({"text": "hello world\nfoo bar"});
        let result = tool_text_count(&args).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["words"], 4);
        assert_eq!(parsed["lines"], 2);
        assert_eq!(parsed["characters"], 19);
    }

    #[test]
    fn test_text_transform_upper() {
        let args = serde_json::json!({"text": "hello", "operation": "upper"});
        assert_eq!(tool_text_transform(&args).unwrap(), "HELLO");
    }

    #[test]
    fn test_text_transform_lower() {
        let args = serde_json::json!({"text": "HELLO", "operation": "lower"});
        assert_eq!(tool_text_transform(&args).unwrap(), "hello");
    }

    #[test]
    fn test_text_transform_reverse() {
        let args = serde_json::json!({"text": "hello", "operation": "reverse"});
        assert_eq!(tool_text_transform(&args).unwrap(), "olleh");
    }

    #[test]
    fn test_text_transform_slug() {
        let args = serde_json::json!({"text": "Hello World! 123", "operation": "slug"});
        assert_eq!(tool_text_transform(&args).unwrap(), "hello-world-123");
    }

    #[test]
    fn test_text_transform_replace() {
        let args = serde_json::json!({"text": "foo bar foo", "operation": "replace", "old": "foo", "new": "baz"});
        assert_eq!(tool_text_transform(&args).unwrap(), "baz bar baz");
    }

    #[test]
    fn test_text_transform_truncate() {
        let args =
            serde_json::json!({"text": "hello world", "operation": "truncate", "max_length": 5});
        assert_eq!(tool_text_transform(&args).unwrap(), "hello...");
    }

    #[test]
    fn test_math_eval_basic() {
        let args = serde_json::json!({"expression": "2 + 3 * 4"});
        assert_eq!(tool_math_eval(&args).unwrap(), "14");
    }

    #[test]
    fn test_math_eval_parentheses() {
        let args = serde_json::json!({"expression": "(2 + 3) * 4"});
        assert_eq!(tool_math_eval(&args).unwrap(), "20");
    }

    #[test]
    fn test_math_eval_power() {
        let args = serde_json::json!({"expression": "2^10"});
        assert_eq!(tool_math_eval(&args).unwrap(), "1024");
    }

    #[test]
    fn test_math_eval_functions() {
        let args = serde_json::json!({"expression": "sqrt(16)"});
        assert_eq!(tool_math_eval(&args).unwrap(), "4");
    }

    #[test]
    fn test_math_eval_division_by_zero() {
        let args = serde_json::json!({"expression": "1/0"});
        assert!(tool_math_eval(&args).is_err());
    }

    #[test]
    fn test_math_eval_nested() {
        let args = serde_json::json!({"expression": "sqrt(3^2 + 4^2)"});
        assert_eq!(tool_math_eval(&args).unwrap(), "5");
    }

    #[test]
    fn test_bench_echo_default() {
        let args = serde_json::json!({});
        let result = tool_bench_echo(&args).unwrap();
        assert_eq!(result.len(), 64);
    }

    #[test]
    fn test_bench_echo_custom_size() {
        let args = serde_json::json!({"size": 1000});
        let result = tool_bench_echo(&args).unwrap();
        assert_eq!(result.len(), 1000);
    }

    #[test]
    fn test_bench_echo_capped() {
        let args = serde_json::json!({"size": 2000000});
        let result = tool_bench_echo(&args).unwrap();
        assert_eq!(result.len(), 1_048_576); // capped at 1MB
    }

    #[test]
    fn test_timestamp() {
        let args = serde_json::json!({});
        let result = tool_timestamp(&args).unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();
        // On non-WASI, should have ISO 8601 timestamp
        #[cfg(not(target_os = "wasi"))]
        {
            assert!(parsed["iso8601"].as_str().unwrap().ends_with('Z'));
            assert!(parsed["unix_seconds"].as_u64().unwrap() > 1700000000);
        }
    }

    #[test]
    fn test_base64_roundtrip() {
        let args = serde_json::json!({"text": "Hello, World!"});
        let encoded = tool_base64_encode(&args).unwrap();
        assert_eq!(encoded, "SGVsbG8sIFdvcmxkIQ==");

        let decode_args = serde_json::json!({"base64": encoded});
        let decoded = tool_base64_decode(&decode_args).unwrap();
        assert_eq!(decoded, "Hello, World!");
    }

    #[test]
    fn test_base64_decode_invalid() {
        let args = serde_json::json!({"base64": "not!valid!base64!!!@##"});
        assert!(tool_base64_decode(&args).is_err());
    }

    #[test]
    fn test_sha256_known_vector() {
        // SHA-256 of empty string
        let result = sha256_hex(b"");
        assert_eq!(
            result,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello() {
        let result = sha256_hex(b"hello");
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha1_known_vector() {
        let result = sha1_hex(b"");
        assert_eq!(result, "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    }

    #[test]
    fn test_md5_known_vector() {
        let result = md5_hex(b"");
        assert_eq!(result, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn test_md5_hello() {
        let result = md5_hex(b"hello");
        assert_eq!(result, "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_hash_text_default_algorithm() {
        let args = serde_json::json!({"text": "hello"});
        let result = tool_hash_text(&args).unwrap();
        assert_eq!(result.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn test_hash_text_md5() {
        let args = serde_json::json!({"text": "hello", "algorithm": "md5"});
        let result = tool_hash_text(&args).unwrap();
        assert_eq!(result, "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_dispatch_unknown_tool() {
        let args = serde_json::json!({});
        let err = dispatch_edge_tool("nonexistent", &args).unwrap_err();
        assert!(err.contains("Unknown tool"));
    }

    #[test]
    fn test_edge_tool_executor_list() {
        let executor = EdgeToolExecutor::new();
        let tools = executor.list_tools();
        assert!(tools.len() >= 10);
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"json_format"));
        assert!(names.contains(&"math_eval"));
        assert!(names.contains(&"hash_text"));
    }

    #[test]
    fn test_edge_tool_executor_call() {
        let executor = EdgeToolExecutor::new();
        let args = serde_json::json!({"message": "test"});
        let result = executor.call_tool("echo", &args).unwrap();
        assert_eq!(result, "test");
    }

    #[test]
    fn test_format_epoch_iso8601() {
        #[cfg(not(target_os = "wasi"))]
        {
            // 2024-01-01T00:00:00Z = 1704067200
            let iso = format_epoch_to_iso8601(1704067200);
            assert_eq!(iso, "2024-01-01T00:00:00Z");

            // Unix epoch
            let iso0 = format_epoch_to_iso8601(0);
            assert_eq!(iso0, "1970-01-01T00:00:00Z");
        }
    }

    #[test]
    fn test_math_eval_negative() {
        let args = serde_json::json!({"expression": "-5 + 3"});
        assert_eq!(tool_math_eval(&args).unwrap(), "-2");
    }

    #[test]
    fn test_math_eval_complex() {
        let args = serde_json::json!({"expression": "2 * (3 + 4) - 1"});
        assert_eq!(tool_math_eval(&args).unwrap(), "13");
    }

    #[test]
    fn test_math_eval_all_functions() {
        let test_cases = vec![
            ("abs(-5)", "5"),
            ("floor(3.7)", "3"),
            ("ceil(3.2)", "4"),
            ("round(3.5)", "4"),
            ("min(3, 1, 2)", "1"),
            ("max(3, 1, 2)", "3"),
            ("pow(2, 8)", "256"),
        ];

        for (expr, expected) in test_cases {
            let args = serde_json::json!({"expression": expr});
            let result = tool_math_eval(&args).unwrap();
            assert_eq!(result, expected, "Failed for expression: {}", expr);
        }
    }

    #[test]
    fn test_base64_empty() {
        let args = serde_json::json!({"text": ""});
        let encoded = tool_base64_encode(&args).unwrap();
        assert_eq!(encoded, "");

        let decode_args = serde_json::json!({"base64": ""});
        let decoded = tool_base64_decode(&decode_args).unwrap();
        assert_eq!(decoded, "");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tmp_dynamic_reg_native() {
        let ex = EdgeToolExecutor::new();
        let code = "import json\ntotal=0\nfor x in args['n']: total+=x\nprint(total)\n";
        let r = ex.call_tool(
            "register_plugin",
            &serde_json::json!({"name": "adder", "code": code}),
        );
        println!("register = {r:?}");
        assert!(r.is_ok());

        let names: Vec<String> = ex.list_tools().into_iter().map(|t| t.name).collect();
        assert!(names.contains(&"adder".to_string()), "list missing adder: {names:?}");

        let out = ex.call_tool("adder", &serde_json::json!({"args": {"n": [1, 2, 3]}}));
        println!("call adder = {out:?}");
        let s = out.unwrap();
        assert!(s.contains("6"), "expected 6 in {s}");
        assert!(s.contains("fuel="), "expected fuel report in {s}");

        let u = ex.call_tool("unregister_plugin", &serde_json::json!({"name": "adder"}));
        assert!(u.is_ok(), "unregister failed: {u:?}");
        let names2: Vec<String> = ex.list_tools().into_iter().map(|t| t.name).collect();
        assert!(!names2.contains(&"adder".to_string()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tmp_registry_persist_native() {
        let dir = std::env::temp_dir().join(format!("velreg-{}", std::process::id()));
        std::env::set_var("VELOCITY_STATE_DIR", &dir);
        let mut m = std::collections::HashMap::new();
        m.insert("persisted_tool".to_string(), "print('hi')".to_string());
        save_registry(&m).expect("save_registry should succeed");
        let loaded = load_registry();
        assert_eq!(loaded.get("persisted_tool").map(|s| s.as_str()), Some("print('hi')"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Proves fix #2 (read-modify-write merge) + fix #1 (refresh-through) at the
    // file layer: two "instances" registering different plugins must NOT lose
    // either, and each must see the other's write on the next read.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tmp_registry_merge_no_lost_update_native() {
        let dir = std::env::temp_dir().join(format!("velmerge-{}", std::process::id()));
        std::env::set_var("VELOCITY_STATE_DIR", &dir);
        let _ = std::fs::remove_dir_all(&dir);

        // Instance A registers alpha.
        merge_register("alpha", "print('a')").expect("merge alpha");
        // Instance B registers beta — must NOT clobber alpha.
        let total = merge_register("beta", "print('b')").expect("merge beta");
        assert_eq!(total, 2, "merge must preserve the other instance's key");

        // Fix #1: a fresh read sees BOTH without any restart.
        let cur = load_registry();
        assert!(cur.contains_key("alpha") && cur.contains_key("beta"), "lost update: {cur:?}");

        // Removing one leaves the other intact.
        let remaining = merge_unregister("alpha").expect("merge unregister").unwrap();
        assert_eq!(remaining, 1);
        let after = load_registry();
        assert!(!after.contains_key("alpha") && after.contains_key("beta"), "bad rmw: {after:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Proves fix #3 (atomic RMW via exclusive lock + atomic rename): many threads
    // call merge_register directly (bypassing the in-process mutex) so the lock
    // file is the only serializer — no lost updates.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tmp_registry_concurrent_lock_native() {
        let dir = std::env::temp_dir().join(format!("vellock-{}", std::process::id()));
        std::env::set_var("VELOCITY_STATE_DIR", &dir);
        let _ = std::fs::remove_dir_all(&dir);

        const N: usize = 16;
        let mut handles = Vec::new();
        for i in 0..N {
            let th = std::thread::spawn(move || {
                merge_register(&format!("tool_{i:02}"), "print('x')").expect("merge");
            });
            handles.push(th);
        }
        for th in handles {
            th.join().unwrap();
        }

        let cur = load_registry();
        let keys: Vec<String> = cur.keys().cloned().collect();
        for i in 0..N {
            let k = format!("tool_{i:02}");
            assert!(cur.contains_key(&k), "lost update: {k} missing; have {keys:?}");
        }
        assert_eq!(cur.len(), N, "expected exactly {N} keys, got {:?}", cur.len());
        // The exclusive lock must be released, not left behind.
        assert!(!lock_path().exists(), "stale lock file left behind");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Proves fix #4: an external transactional store gives cross-instance safety
    // with NO shared filesystem and NO file lock. Skips unless VELOCITY_PG_DSN is set.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tmp_registry_pg_concurrent_native() {
        if pgreg::dsn().is_none() {
            eprintln!("skipping: VELOCITY_PG_DSN not set");
            return;
        }
        pgreg::ensure_table().expect("ensure_table");
        pgreg::truncate();

        const N: usize = 16;
        std::thread::scope(|s| {
            for i in 0..N {
                s.spawn(move || {
                    pgreg::register(&format!("pg_{i:02}"), "print('x')").expect("pg register")
                });
            }
        });
        let cur = pgreg::load_all();
        for i in 0..N {
            let k = format!("pg_{i:02}");
            assert!(cur.contains_key(&k), "lost update: {k} missing; have {cur:?}");
        }
        assert_eq!(cur.len(), N, "expected exactly {N} rows, got {}", cur.len());

        // Contended same-row upsert: 8 writers → still ONE row, version == 8.
        std::thread::scope(|s| {
            for i in 0..8 {
                s.spawn(move || pgreg::register("hotkey", &format!("print({i})")).unwrap());
            }
        });
        assert_eq!(pgreg::version_of("hotkey"), 8, "upserts must serialize on the row");
        assert!(pgreg::load_all().contains_key("hotkey"));

        pgreg::truncate();
    }
}
