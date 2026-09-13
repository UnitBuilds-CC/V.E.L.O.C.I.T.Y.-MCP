// Test real tool sources with control flow, functions, data structures
// Run with: cargo test --test real_tool_test -- --ignored

use std::error::Error;
use velocity_mcp::wasm_runtime::WasmRuntime;

struct RealToolTest {
    language: &'static str,
    source: &'static str,
    args: &'static str,
    expected_contains: Vec<&'static str>,
}

fn get_test_cases() -> Vec<RealToolTest> {
    vec![
        // PHP: text statistics with functions, loops, arrays
        RealToolTest {
            language: "php",
            source: r#"<?php
function count_words($text) {
    $words = split($text, " ");
    return len($words);
}

function find_longest($text) {
    $words = split($text, " ");
    $longest = "";
    $i = 0;
    while ($i < len($words)) {
        $word = $words[$i];
        if (len($word) > len($longest)) {
            $longest = $word;
        }
        $i = $i + 1;
    }
    return $longest;
}

$word_count = count_words($text);
$char_count = len($text);
$longest = find_longest($text);
$result = "Words: " . to_string($word_count) . ", Chars: " . to_string($char_count) . ", Longest: " . $longest;
set_tool_result($result);
"#,
            args: r#"{"text": "the quick brown fox jumps"}"#,
            expected_contains: vec!["Words: 5", "Chars: 25", "Longest: quick"],
        },
        // C#: text statistics with functions, loops, arrays
        RealToolTest {
            language: "csharp",
            source: r#"
function count_words(text) {
    words = split(text, " ");
    return len(words);
}

function find_longest(text) {
    words = split(text, " ");
    longest = "";
    i = 0;
    while (i < len(words)) {
        word = words[i];
        if (len(word) > len(longest)) {
            longest = word;
        }
        i = i + 1;
    }
    return longest;
}

word_count = count_words(text);
char_count = len(text);
longest = find_longest(text);
result = "Words: " + to_string(word_count) + ", Chars: " + to_string(char_count) + ", Longest: " + longest;
set_tool_result(result);
"#,
            args: r#"{"text": "the quick brown fox jumps"}"#,
            expected_contains: vec!["Words: 5", "Chars: 25", "Longest: quick"],
        },
        // Java: same as C#
        RealToolTest {
            language: "java",
            source: r#"
function count_words(text) {
    words = split(text, " ");
    return len(words);
}

function find_longest(text) {
    words = split(text, " ");
    longest = "";
    i = 0;
    while (i < len(words)) {
        word = words[i];
        if (len(word) > len(longest)) {
            longest = word;
        }
        i = i + 1;
    }
    return longest;
}

word_count = count_words(text);
char_count = len(text);
longest = find_longest(text);
result = "Words: " + to_string(word_count) + ", Chars: " + to_string(char_count) + ", Longest: " + longest;
set_tool_result(result);
"#,
            args: r#"{"text": "the quick brown fox jumps"}"#,
            expected_contains: vec!["Words: 5", "Chars: 25", "Longest: quick"],
        },
        // R: text statistics (no semicolons, newline-terminated)
        RealToolTest {
            language: "r",
            source: r#"
function count_words(text) {
    words = split(text, " ")
    return len(words)
}

function find_longest(text) {
    words = split(text, " ")
    longest = ""
    i = 0
    while (i < len(words)) {
        word = words[i]
        if (len(word) > len(longest)) {
            longest = word
        }
        i = i + 1
    }
    return longest
}

word_count = count_words(text)
char_count = len(text)
longest = find_longest(text)
result = "Words: " + to_string(word_count) + ", Chars: " + to_string(char_count) + ", Longest: " + longest
set_tool_result(result)
"#,
            args: r#"{"text": "the quick brown fox jumps"}"#,
            expected_contains: vec!["Words: 5", "Chars: 25", "Longest: quick"],
        },
        // Julia: same as R (no semicolons)
        RealToolTest {
            language: "julia",
            source: r#"
function count_words(text) {
    words = split(text, " ")
    return len(words)
}

function find_longest(text) {
    words = split(text, " ")
    longest = ""
    i = 0
    while (i < len(words)) {
        word = words[i]
        if (len(word) > len(longest)) {
            longest = word
        }
        i = i + 1
    }
    return longest
}

word_count = count_words(text)
char_count = len(text)
longest = find_longest(text)
result = "Words: " + to_string(word_count) + ", Chars: " + to_string(char_count) + ", Longest: " + longest
set_tool_result(result)
"#,
            args: r#"{"text": "the quick brown fox jumps"}"#,
            expected_contains: vec!["Words: 5", "Chars: 25", "Longest: quick"],
        },
        // Perl: text statistics with $ vars, . concat, semicolons
        RealToolTest {
            language: "perl",
            source: r#"
sub count_words {
    $words = split($text, " ");
    return len($words);
}

sub find_longest {
    $words = split($text, " ");
    $longest = "";
    $i = 0;
    while ($i < len($words)) {
        $word = $words[$i];
        if (len($word) > len($longest)) {
            $longest = $word;
        }
        $i = $i + 1;
    }
    return $longest;
}

$word_count = count_words($text);
$char_count = len($text);
$longest = find_longest($text);
$result = "Words: " . to_string($word_count) . ", Chars: " . to_string($char_count) . ", Longest: " . $longest;
set_tool_result($result);
"#,
            args: r#"{"text": "the quick brown fox jumps"}"#,
            expected_contains: vec!["Words: 5", "Chars: 25", "Longest: quick"],
        },
    ]
}

#[test]
#[ignore]
fn test_real_tools_php() {
    test_real_tool("php");
}

#[test]
#[ignore]
fn test_real_tools_csharp() {
    test_real_tool("csharp");
}

#[test]
#[ignore]
fn test_real_tools_java() {
    test_real_tool("java");
}

#[test]
#[ignore]
fn test_real_tools_r() {
    test_real_tool("r");
}

#[test]
#[ignore]
fn test_real_tools_julia() {
    test_real_tool("julia");
}

#[test]
#[ignore]
fn test_real_tools_perl() {
    test_real_tool("perl");
}

fn test_real_tool(language: &str) {
    let test_cases = get_test_cases();
    let test_case = test_cases
        .iter()
        .find(|t| t.language == language)
        .expect("no test case");

    println!("\n=== Testing {} with real tool source ===", language);
    println!("Source:\n{}", test_case.source);
    println!("Args: {}", test_case.args);

    let result = match language {
        "php" => test_php_tool(test_case),
        "csharp" => test_csharp_tool(test_case),
        "java" => test_java_tool(test_case),
        "r" => test_r_tool(test_case),
        "julia" => test_julia_tool(test_case),
        "perl" => test_perl_tool(test_case),
        _ => panic!("unknown language: {}", language),
    };

    match result {
        Ok(output) => {
            println!("Output: {}", output);
            for expected in &test_case.expected_contains {
                assert!(
                    output.contains(expected),
                    "expected '{}' in output '{}'",
                    expected,
                    output
                );
            }
            println!("✓ {} passed all assertions", language);
        }
        Err(e) => {
            panic!("{} tool execution failed: {}", language, e);
        }
    }
}

fn test_php_tool(test: &RealToolTest) -> Result<String, Box<dyn Error>> {
    use velocity_mcp::wasm_runtime::php::PhpRuntime;

    let wasm = std::fs::read("bench_tools/php_wasm/php.wasm")?;
    let mut rt = PhpRuntime::cold_start(&wasm)?;
    rt.register_tool("text_stats", test.source)?;
    let result = rt.call_tool("text_stats", test.args)?;
    rt.destroy()?;
    Ok(result)
}

fn test_csharp_tool(test: &RealToolTest) -> Result<String, Box<dyn Error>> {
    use velocity_mcp::wasm_runtime::csharp::CSharpRuntime;

    let wasm = std::fs::read("bench_tools/csharp_wasm/dotnet.wasm")?;
    let mut rt = CSharpRuntime::cold_start(&wasm)?;
    rt.register_tool("text_stats", test.source)?;
    let result = rt.call_tool("text_stats", test.args)?;
    rt.destroy()?;
    Ok(result)
}

fn test_java_tool(test: &RealToolTest) -> Result<String, Box<dyn Error>> {
    use velocity_mcp::wasm_runtime::java::JavaRuntime;

    let wasm = std::fs::read("bench_tools/java_wasm/java.wasm")?;
    let mut rt = JavaRuntime::cold_start(&wasm)?;
    rt.register_tool("text_stats", test.source)?;
    let result = rt.call_tool("text_stats", test.args)?;
    rt.destroy()?;
    Ok(result)
}

fn test_r_tool(test: &RealToolTest) -> Result<String, Box<dyn Error>> {
    use velocity_mcp::wasm_runtime::r::RRuntime;

    let wasm = std::fs::read("bench_tools/r_wasm/r.wasm")?;
    let mut rt = RRuntime::cold_start(&wasm)?;
    rt.register_tool("text_stats", test.source)?;
    let result = rt.call_tool("text_stats", test.args)?;
    rt.destroy()?;
    Ok(result)
}

fn test_julia_tool(test: &RealToolTest) -> Result<String, Box<dyn Error>> {
    use velocity_mcp::wasm_runtime::julia::JuliaRuntime;

    let wasm = std::fs::read("bench_tools/julia_wasm/julia.wasm")?;
    let mut rt = JuliaRuntime::cold_start(&wasm)?;
    rt.register_tool("text_stats", test.source)?;
    let result = rt.call_tool("text_stats", test.args)?;
    rt.destroy()?;
    Ok(result)
}

fn test_perl_tool(test: &RealToolTest) -> Result<String, Box<dyn Error>> {
    use velocity_mcp::wasm_runtime::perl::PerlRuntime;

    let wasm = std::fs::read("bench_tools/perl_wasm/perl.wasm")?;
    let mut rt = PerlRuntime::cold_start(&wasm)?;
    rt.register_tool("text_stats", test.source)?;
    let result = rt.call_tool("text_stats", test.args)?;
    rt.destroy()?;
    Ok(result)
}
