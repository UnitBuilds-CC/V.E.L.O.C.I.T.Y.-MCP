#![allow(dead_code)]
//! Comprehensive audit of tree-walk interpreter capabilities.
//! Tests edge cases, advanced features, and failure modes.

use velocity_mcp::wasm_runtime::{WasmRuntime, csharp::CSharpRuntime, java::JavaRuntime,
    julia::JuliaRuntime, perl::PerlRuntime, php::PhpRuntime, r::RRuntime};

fn load_wasm(lang: &str) -> Vec<u8> {
    let path = format!("bench_tools/{}_wasm/{}.wasm",
        match lang {
            "php" => "php", "csharp" => "csharp", "java" => "java",
            "r" => "r", "julia" => "julia", "perl" => "perl",
            _ => panic!("unknown lang"),
        },
        match lang {
            "php" => "php", "csharp" => "dotnet", "java" => "java",
            "r" => "r", "julia" => "julia", "perl" => "perl",
            _ => panic!("unknown lang"),
        }
    );
    std::fs::read(&path).expect(&format!("WASM not found: {}", path))
}

// Test 1: Closures with mutable captured state
fn test_closure_mutable_state(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
function make_counter() {
    $count = 0;
    function increment() {
        $count = $count + 1;
        return $count;
    }
    return increment;
}
$counter = make_counter();
$a = $counter();
$b = $counter();
$c = $counter();
set_tool_result($a . "," . $b . "," . $c);
"#,
        "perl" => r#"
sub make_counter {
    my $count = 0;
    sub increment {
        $count = $count + 1;
        return $count;
    }
    return increment;
}
$counter = make_counter();
$a = $counter();
$b = $counter();
$c = $counter();
set_tool_result($a . "," . $b . "," . $c);
"#,
        _ => r#"
function make_counter() {
    count = 0;
    function increment() {
        count = count + 1;
        return count;
    }
    return increment;
}
counter = make_counter();
a = counter();
b = counter();
c = counter();
set_tool_result(to_string(a) + "," + to_string(b) + "," + to_string(c));
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 2: Higher-order functions (functions as arguments)
fn test_higher_order_functions(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
function apply($fn, $x) {
    return $fn($x);
}
function double($n) {
    return $n * 2;
}
$result = apply(double, 21);
set_tool_result($result);
"#,
        "perl" => r#"
sub apply {
    my $fn = $_[0];
    my $x = $_[1];
    return $fn->($x);
}
sub double {
    return $_[0] * 2;
}
$result = apply(\&double, 21);
set_tool_result($result);
"#,
        _ => r#"
function apply(fn, x) {
    return fn(x);
}
function double(n) {
    return n * 2;
}
result = apply(double, 21);
set_tool_result(result);
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 3: String escape sequences
fn test_string_escapes(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
$s = "line1\nline2\ttab\\backslash\"quote";
set_tool_result(len($s));
"#,
        "perl" => r#"
$s = "line1\nline2\ttab\\backslash\"quote";
set_tool_result(len($s));
"#,
        _ => r#"
s = "line1\nline2\ttab\\backslash\"quote";
set_tool_result(len(s));
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 4: Nested data structure manipulation
fn test_nested_data_structures(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
$data = [
    ["name" => "Alice", "scores" => [95, 87, 92]],
    ["name" => "Bob", "scores" => [88, 91, 85]]
];
$total = 0;
$i = 0;
while ($i < len($data)) {
    $scores = $data[$i]["scores"];
    $j = 0;
    while ($j < len($scores)) {
        $total = $total + $scores[$j];
        $j = $j + 1;
    }
    $i = $i + 1;
}
set_tool_result($total);
"#,
        "perl" => r#"
$data = [
    {"name" => "Alice", "scores" => [95, 87, 92]},
    {"name" => "Bob", "scores" => [88, 91, 85]}
];
$total = 0;
$i = 0;
while ($i < len($data)) {
    $scores = $data[$i]["scores"];
    $j = 0;
    while ($j < len($scores)) {
        $total = $total + $scores[$j];
        $j = $j + 1;
    }
    $i = $i + 1;
}
set_tool_result($total);
"#,
        _ => r#"
data = [
    {"name": "Alice", "scores": [95, 87, 92]},
    {"name": "Bob", "scores": [88, 91, 85]}
];
total = 0;
i = 0;
while (i < len(data)) {
    scores = data[i]["scores"];
    j = 0;
    while (j < len(scores)) {
        total = total + scores[j];
        j = j + 1;
    }
    i = i + 1;
}
set_tool_result(total);
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 5: Function references in data structures
fn test_function_refs_in_structures(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
function add($a, $b) { return $a + $b; }
function mul($a, $b) { return $a * $b; }
$ops = ["+" => add, "*" => mul];
$result = $ops["+"](10, 5) + $ops["*"](3, 4);
set_tool_result($result);
"#,
        "perl" => r#"
sub add($a, $b) { return $a + $b; }
sub mul($a, $b) { return $a * $b; }
$ops = {"+" => add, "*" => mul};
$result = $ops["+"](10, 5) + $ops["*"](3, 4);
set_tool_result($result);
"#,
        _ => r#"
function add(a, b) { return a + b; }
function mul(a, b) { return a * b; }
ops = {"+": add, "*": mul};
result = ops["+"](10, 5) + ops["*"](3, 4);
set_tool_result(result);
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 6: Deep recursion (fibonacci)
fn test_deep_recursion(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
function fib($n) {
    if ($n <= 1) return $n;
    return fib($n - 1) + fib($n - 2);
}
set_tool_result(fib(20));
"#,
        "perl" => r#"
sub fib {
    my $n = $_[0];
    if ($n <= 1) { return $n; }
    return fib($n - 1) + fib($n - 2);
}
set_tool_result(fib(20));
"#,
        _ => r#"
function fib(n) {
    if (n <= 1) { return n; }
    return fib(n - 1) + fib(n - 2);
}
set_tool_result(fib(20));
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 7: Error handling - division by zero
fn test_division_by_zero(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
$x = 10 / 0;
set_tool_result($x);
"#,
        "perl" => r#"
$x = 10 / 0;
set_tool_result($x);
"#,
        _ => r#"
x = 10 / 0;
set_tool_result(x);
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 8: Error handling - undefined variable
fn test_undefined_variable(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
set_tool_result($undefined_var);
"#,
        "perl" => r#"
set_tool_result($undefined_var);
"#,
        _ => r#"
set_tool_result(undefined_var);
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 9: Complex string operations
fn test_complex_string_ops(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
$s = "Hello, World!";
$parts = split($s, ", ");
$joined = join($parts, " - ");
$upper = upper($joined);
set_tool_result($upper);
"#,
        "perl" => r#"
$s = "Hello, World!";
$parts = split($s, ", ");
$joined = join($parts, " - ");
$upper = upper($joined);
set_tool_result($upper);
"#,
        _ => r#"
s = "Hello, World!";
parts = split(s, ", ");
joined = join(parts, " - ");
upper_str = upper(joined);
set_tool_result(upper_str);
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

// Test 10: Array/map mutation
fn test_mutation(rt: &mut dyn WasmRuntime, lang: &str) -> Result<String, String> {
    let source = match lang {
        "php" => r#"<?php
$arr = [1, 2, 3];
push($arr, 4);
push($arr, 5);
$map = {"a" => 1};
$map["b"] = 2;
$map["c"] = 3;
set_tool_result(len($arr) + len(keys($map)));
"#,
        "perl" => r#"
$arr = [1, 2, 3];
push($arr, 4);
push($arr, 5);
$map = {"a" => 1};
$map["b"] = 2;
$map["c"] = 3;
set_tool_result(len($arr) + len(keys($map)));
"#,
        _ => r#"
arr = [1, 2, 3];
push(arr, 4);
push(arr, 5);
map = {"a": 1};
map["b"] = 2;
map["c"] = 3;
set_tool_result(len(arr) + len(keys(map)));
"#,
    };

    rt.register_tool("test", source).map_err(|e| format!("register failed: {}", e))?;
    let rc = rt.call_tool("test", "{}").map_err(|e| format!("call failed: {}", e))?;
    Ok(rc)
}

fn run_test(name: &str, test_fn: fn(&mut dyn WasmRuntime, &str) -> Result<String, String>) {
    println!("\n=== {} ===", name);
    let langs = ["php", "csharp", "java", "r", "julia", "perl"];

    for lang in &langs {
        let wasm = load_wasm(lang);
        let mut rt: Box<dyn WasmRuntime> = match *lang {
            "php" => Box::new(PhpRuntime::cold_start(&wasm).unwrap()),
            "csharp" => Box::new(CSharpRuntime::cold_start(&wasm).unwrap()),
            "java" => Box::new(JavaRuntime::cold_start(&wasm).unwrap()),
            "r" => Box::new(RRuntime::cold_start(&wasm).unwrap()),
            "julia" => Box::new(JuliaRuntime::cold_start(&wasm).unwrap()),
            "perl" => Box::new(PerlRuntime::cold_start(&wasm).unwrap()),
            _ => unreachable!(),
        };

        match test_fn(rt.as_mut(), lang) {
            Ok(result) => {
                let parsed: serde_json::Value = serde_json::from_str(&result).unwrap_or(serde_json::Value::Null);
                let msg = parsed.get("message").and_then(|v| v.as_str()).unwrap_or(&result);
                println!("  {}: OK - {}", lang, msg);
            }
            Err(e) => {
                println!("  {}: FAIL - {}", lang, e);
            }
        }

        let _ = rt.destroy();
    }
}

#[test]
fn audit_closures() {
    run_test("Closures with mutable state", test_closure_mutable_state);
}

#[test]
fn audit_higher_order() {
    run_test("Higher-order functions", test_higher_order_functions);
}

#[test]
fn audit_string_escapes() {
    run_test("String escape sequences", test_string_escapes);
}

#[test]
fn audit_nested_structures() {
    run_test("Nested data structures", test_nested_data_structures);
}

#[test]
fn audit_function_refs() {
    run_test("Function refs in structures", test_function_refs_in_structures);
}

#[test]
fn audit_recursion() {
    run_test("Deep recursion (fib(20))", test_deep_recursion);
}

#[test]
fn audit_div_zero() {
    run_test("Division by zero (error handling)", test_division_by_zero);
}

#[test]
fn audit_undefined() {
    run_test("Undefined variable (error handling)", test_undefined_variable);
}

#[test]
fn audit_string_ops() {
    run_test("Complex string operations", test_complex_string_ops);
}

#[test]
fn audit_mutation() {
    run_test("Array/map mutation", test_mutation);
}
