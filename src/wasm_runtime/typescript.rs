//! TypeScript/WASM runtime — TypeScript tools transpiled to JavaScript and executed via QuickJS.
//!
//! TypeScript source is transpiled to JavaScript at registration time, then executed
//! using the existing QuickJS runtime. This allows TypeScript tools to run in the
//! same sandboxed environment as JavaScript tools.

use std::error::Error;
use super::quickjs::QuickJsRuntime;
use super::WasmRuntime;

pub struct TypeScriptRuntime {
    inner: QuickJsRuntime,
}

impl TypeScriptRuntime {
    pub fn new(wasm_bytes: &[u8]) -> Result<Self, Box<dyn Error>> {
        let inner = QuickJsRuntime::new(wasm_bytes)?;
        Ok(Self { inner })
    }

    /// Simple TypeScript to JavaScript transpiler.
    /// This is a minimal implementation that strips type annotations.
    /// For production use, integrate a proper TypeScript compiler.
    fn transpile_ts_to_js(ts_source: &str) -> String {
        let mut js = String::new();
        let mut in_type_annotation = false;
        let mut in_interface = false;
        let mut brace_depth: i32 = 0;

        for line in ts_source.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("interface ") || trimmed.starts_with("type ") {
                in_interface = true;
            }

            if in_interface {
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;
                if brace_depth == 0 && line.contains('}') {
                    in_interface = false;
                }
                continue;
            }

            let mut result = String::new();
            let mut chars = line.chars().peekable();

            while let Some(ch) = chars.next() {
                if ch == '{' {
                    brace_depth += 1;
                    result.push(ch);
                    continue;
                }
                if ch == '}' {
                    brace_depth -= 1;
                    result.push(ch);
                    continue;
                }

                if ch == ':' && !in_type_annotation && brace_depth == 0 {
                    let next: String = chars.clone().take(10).collect();
                    if next.trim_start().starts_with(|c: char| c.is_alphabetic() || c == '{' || c == '[' || c == '(') {
                        in_type_annotation = true;
                        continue;
                    }
                }

                if in_type_annotation {
                    if ch == ',' || ch == ')' || ch == '=' {
                        in_type_annotation = false;
                        result.push(ch);
                    }
                    continue;
                }

                result.push(ch);
            }

            let result = result.replace(" as ", " ");

            js.push_str(&result);
            js.push('\n');
        }

        js
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

        rt.register_tool("text_analyze", ts_source).expect("register failed");
        let result = rt.call_tool("text_analyze", r#"{"text": "hello world"}"#).expect("call failed");
        assert!(result.contains("2")); // word count
        assert!(result.contains("11")); // char count

        rt.destroy().unwrap();
    }
}
