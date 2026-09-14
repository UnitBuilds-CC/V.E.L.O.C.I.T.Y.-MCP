//! TypeScript/WASM runtime — TypeScript tools transpiled to JavaScript and executed via QuickJS.
//!
//! TypeScript source is transpiled to JavaScript at registration time, then executed
//! using the existing QuickJS runtime. This allows TypeScript tools to run in the
//! same sandboxed environment as JavaScript tools.

use super::quickjs::QuickJsRuntime;
use super::WasmRuntime;
use std::error::Error;

pub struct TypeScriptRuntime {
    inner: QuickJsRuntime,
}

impl TypeScriptRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let inner = QuickJsRuntime::new(wasm_bytes)?;
        Ok(Self { inner })
    }

    /// Simple TypeScript to JavaScript transpiler.
    /// Uses regex-based replacement to strip type annotations safely.
    fn transpile_ts_to_js(ts_source: &str) -> String {
        let mut js = String::new();

        for line in ts_source.lines() {
            let trimmed = line.trim();

            // Skip interface and type declarations entirely
            if trimmed.starts_with("interface ") || trimmed.starts_with("type ") {
                continue;
            }

            let mut result = line.to_string();

            // Remove parameter type annotations using regex-like pattern matching
            // Pattern: identifier: Type followed by comma, paren, or equals
            // We need to handle nested generics like Array<string>
            result = Self::remove_type_annotations(&result);

            // Remove return type annotations: ): Type { -> ) {
            result = Self::remove_return_types(&result);

            // Remove "as" type assertions: x as Type -> x
            result = result.replace(" as ", " ");

            js.push_str(&result);
            js.push('\n');
        }

        js
    }

    /// Remove type annotations from a single line of TypeScript code.
    /// Handles patterns like: param: string, arr: number[], obj: { key: value }
    fn remove_type_annotations(line: &str) -> String {
        let chars: Vec<char> = line.chars().collect();
        let len = chars.len();
        let mut result = String::with_capacity(len);
        let mut last_output_end = 0;

        for i in 0..len {
            // Look for ": " pattern (colon followed by space)
            if i + 1 < len && chars[i] == ':' && chars[i + 1] == ' ' {
                // Check if preceded by identifier (potential type annotation)
                if i > 0
                    && (chars[i - 1].is_alphanumeric()
                        || chars[i - 1] == '_'
                        || chars[i - 1] == '?')
                {
                    // This looks like a type annotation candidate
                    let after_space = i + 2;

                    // Peek ahead to see what follows the colon+space
                    // Type annotations start with type keywords or symbols, object values are expressions
                    if after_space < len {
                        let next_word_start = after_space;

                        // Extract the next word/token after ": "
                        let mut next_token = String::new();
                        for j in next_word_start..len.min(next_word_start + 20) {
                            if chars[j].is_alphanumeric() || chars[j] == '_' {
                                next_token.push(chars[j]);
                            } else {
                                break;
                            }
                        }

                        // Common TypeScript type keywords
                        let type_keywords = [
                            "string",
                            "number",
                            "boolean",
                            "any",
                            "void",
                            "never",
                            "unknown",
                            "undefined",
                            "null",
                            "object",
                            "bigint",
                            "symbol",
                        ];

                        // If next token is a type keyword, it's definitely a type annotation
                        let is_type_keyword =
                            type_keywords.contains(&next_token.to_lowercase().as_str());

                        // If next token starts with uppercase (like Array, Map, custom types), likely a type
                        let starts_uppercase = !next_token.is_empty()
                            && next_token.chars().next().unwrap().is_uppercase();

                        // If next char after space is a symbol (<, {, [, (), it's a type
                        let next_char_is_type_symbol = after_space < len
                            && (chars[after_space] == '<'
                                || chars[after_space] == '{'
                                || chars[after_space] == '['
                                || chars[after_space] == '(');

                        if is_type_keyword || starts_uppercase || next_char_is_type_symbol {
                            // This is a type annotation - find where it ends
                            let mut depth = 0i32;
                            let mut type_end = None;

                            for j in after_space..len {
                                match chars[j] {
                                    '<' | '{' | '[' | '(' => depth += 1,
                                    '>' | '}' | ']' | ')' => {
                                        if depth > 0 {
                                            depth -= 1;
                                        } else {
                                            type_end = Some(j);
                                            break;
                                        }
                                    }
                                    ',' | '=' if depth == 0 => {
                                        type_end = Some(j);
                                        break;
                                    }
                                    ';' if depth == 0 => {
                                        type_end = Some(j);
                                        break;
                                    }
                                    _ => {}
                                }
                            }

                            if let Some(end) = type_end {
                                // Check if this might be === operator
                                if chars[end] == '=' && end + 1 < len && chars[end + 1] == '=' {
                                    continue;
                                }

                                // Output everything up to the colon
                                for k in last_output_end..i {
                                    result.push(chars[k]);
                                }

                                // Skip the type annotation
                                last_output_end = end;
                            }
                        }
                        // Otherwise, it's likely an object property - leave it alone
                    }
                }
            }
        }

        // Output remaining characters
        for k in last_output_end..len {
            result.push(chars[k]);
        }

        result
    }

    /// Remove return type annotations from function declarations
    fn remove_return_types(line: &str) -> String {
        // Pattern: ): ReturnType {
        // Replace with: ) {
        if let Some(pos) = line.find("): ") {
            if let Some(brace_pos) = line[pos..].find('{') {
                let abs_brace = pos + brace_pos;
                let mut result = String::with_capacity(line.len());
                result.push_str(&line[..pos + 1]); // Keep up to ")"
                result.push_str(&line[abs_brace..]); // Keep from "{" onwards
                return result;
            }
        }
        line.to_string()
    }
}

impl WasmRuntime for TypeScriptRuntime {
    fn init(&mut self) -> Result<(), Box<dyn Error>> {
        self.inner.init()
    }

    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>> {
        // Transpile TypeScript to JavaScript
        let js_source = Self::transpile_ts_to_js(source);

        // Register using the inner QuickJS runtime
        self.inner.register_tool(name, &js_source)
    }

    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        self.inner.call_tool(name, args_json)
    }

    fn destroy(&mut self) -> Result<(), Box<dyn Error>> {
        self.inner.destroy()
    }

    fn reset_instruction_budget(&mut self) {
        self.inner.reset_instruction_budget();
    }

    fn language(&self) -> &str {
        "typescript"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wasm_path() -> std::path::PathBuf {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.push("bench_tools/quickjs_wasm/quickjs.wasm");
        p
    }

    #[test]
    fn test_typescript_transpilation() {
        let ts_source = r#"
interface Args {
    text: string;
}

function analyze(args: Args): { count: number } {
    const text: string = args.text;
    return { count: text.length };
}
"#;

        let js = TypeScriptRuntime::transpile_ts_to_js(ts_source);
        assert!(!js.contains("interface"));
        assert!(!js.contains(": string"));
        assert!(!js.contains(": number"));
        assert!(js.contains("function analyze"));
    }

    #[test]
    #[ignore] // requires QuickJS WASM binary
    fn test_typescript_tool_execution() {
        let wasm = std::fs::read(wasm_path()).expect("QuickJS WASM not found");
        let mut rt = TypeScriptRuntime::new(&wasm).expect("failed to create runtime");
        rt.init().expect("init failed");

        let ts_source = r#"
function text_analyze(args) {
    const text = args.text || '';
    const words = text.split(/\s+/).filter(w => w.length > 0);
    return {
        word_count: words.length,
        char_count: text.length
    };
}
"#;

        rt.register_tool("text_analyze", ts_source)
            .expect("register failed");
        let result = rt
            .call_tool("text_analyze", r#"{"text": "hello world"}"#)
            .expect("call failed");
        assert!(result.contains("2")); // word count
        assert!(result.contains("11")); // char count

        rt.destroy().unwrap();
    }
}
