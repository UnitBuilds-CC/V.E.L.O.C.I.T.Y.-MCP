//! Tool Compatibility Test: verifies all WASM plugin tools produce correct output
//! through the WASM runtime, and compares JavaScript tools against Node.js native.
//!
//! Covers 16 tools across 10 languages, 20 test cases each (320 total).

use serde_json::{json, Value};
use std::process::Command;
use velocity_mcp::wasm_runtime::create_wasm_runtime_for_language;

const QUICKJS_WASM: &str = "bench_tools/quickjs_wasm/quickjs.wasm";
const MICROPYTHON_WASM: &str = "bench_tools/micropython_wasm/wasi-reactor/build/micropython.wasm";
const LUA_WASM: &str = "bench_tools/lua_wasm/lua.wasm";
const RUBY_WASM: &str = "bench_tools/mruby_wasm/ruby.wasm";
const PHP_WASM: &str = "bench_tools/php_wasm/php.wasm";
const CSHARP_WASM: &str = "bench_tools/csharp_wasm/dotnet.wasm";
const JAVA_WASM: &str = "bench_tools/java_wasm/java.wasm";
const R_WASM: &str = "bench_tools/r_wasm/r.wasm";
const JULIA_WASM: &str = "bench_tools/julia_wasm/julia.wasm";
const PERL_WASM: &str = "bench_tools/perl_wasm/perl.wasm";

struct WasmToolTest {
    tool_name: &'static str,
    language: &'static str,
    wasm_path: &'static str,
    source: &'static str,
    cases: Vec<ToolCase>,
}

struct ToolCase {
    name: String,
    input: Value,
    check: Box<dyn Fn(&Value) -> bool>,
    check_desc: String,
}

fn tc(name: &str, input: Value, check: impl Fn(&Value) -> bool + 'static, desc: &str) -> ToolCase {
    ToolCase {
        name: name.to_string(),
        input,
        check: Box::new(check),
        check_desc: desc.to_string(),
    }
}

fn main() {
    println!("================================================================");
    println!("   Tool Compatibility Test: WASM Plugin Tools");
    println!("================================================================\n");

    let tests = define_all_tests();
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for test in &tests {
        if !std::path::Path::new(test.wasm_path).exists() {
            println!("  SKIP {} ({}) — WASM file missing", test.tool_name, test.language);
            skipped += test.cases.len();
            continue;
        }

        let mut rt = match create_wasm_runtime_for_language(test.language, test.wasm_path) {
            Ok(rt) => rt,
            Err(e) => {
                println!("  SKIP {} ({}) — runtime creation failed: {}", test.tool_name, test.language, e);
                skipped += test.cases.len();
                continue;
            }
        };

        if let Err(e) = rt.init() {
            println!("  SKIP {} ({}) — init failed: {}", test.tool_name, test.language, e);
            skipped += test.cases.len();
            continue;
        }

        if let Err(e) = rt.register_tool(test.tool_name, test.source) {
            println!("  FAIL {} ({}) — register_tool failed: {}", test.tool_name, test.language, e);
            failed += test.cases.len();
            continue;
        }

        println!("  {} ({}):", test.tool_name, test.language);
        for case in &test.cases {
            let args_json = case.input.to_string();
            match rt.call_tool(test.tool_name, &args_json) {
                Ok(result_str) => {
                    match serde_json::from_str::<Value>(&result_str) {
                        Ok(result) => {
                            if (case.check)(&result) {
                                println!("    PASS  {}", case.name);
                                passed += 1;
                            } else {
                                println!("    FAIL  {} — {}", case.name, case.check_desc);
                                println!("           got: {}", result);
                                failed += 1;
                            }
                        }
                        Err(e) => {
                            println!("    FAIL  {} — invalid JSON output: {}", case.name, e);
                            println!("           raw: {}", result_str);
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    println!("    FAIL  {} — call_tool error: {}", case.name, e);
                    failed += 1;
                }
            }
        }
        println!();
    }

    println!("─── JavaScript: WASM vs Node.js native ───────────────────────\n");
    let js_native_results = test_js_native();

    println!("════════════════════════════════════════════════════════════════");
    println!("  WASM tools:  {} passed, {} failed, {} skipped", passed, failed, skipped);
    println!("  JS native:   {} passed, {} failed", js_native_results.0, js_native_results.1);
    println!("════════════════════════════════════════════════════════════════");

    if failed > 0 || js_native_results.1 > 0 {
        std::process::exit(1);
    }
}

fn json_contains_hello(v: &Value) -> bool {
    if let Some(s) = v.as_str() {
        return s.contains("Hello");
    }
    if let Some(obj) = v.as_object() {
        for val in obj.values() {
            if let Some(s) = val.as_str() {
                if s.contains("Hello") {
                    return true;
                }
            }
            if json_contains_hello(val) {
                return true;
            }
        }
    }
    false
}

fn greeting_has_name(v: &Value, name: &str) -> bool {
    let expected = format!("Hello, {}!", name);
    if let Some(s) = v.as_str() {
        return s.contains(&expected);
    }
    if let Some(obj) = v.as_object() {
        for val in obj.values() {
            if let Some(s) = val.as_str() {
                if s.contains(&expected) {
                    return true;
                }
            }
        }
    }
    false
}

// ── Test case generators (20 cases each) ─────────────────────────────

fn make_js_string_cases() -> Vec<ToolCase> {
    vec![
        tc("upper_basic", json!({"action":"upper","input":"hello world"}), |v| v["result"] == "HELLO WORLD", "HELLO WORLD"),
        tc("upper_single", json!({"action":"upper","input":"abc"}), |v| v["result"] == "ABC", "ABC"),
        tc("upper_already", json!({"action":"upper","input":"ALREADY"}), |v| v["result"] == "ALREADY", "ALREADY"),
        tc("lower_basic", json!({"action":"lower","input":"HELLO"}), |v| v["result"] == "hello", "hello"),
        tc("lower_multi", json!({"action":"lower","input":"ABC DEF"}), |v| v["result"] == "abc def", "abc def"),
        tc("lower_mixed", json!({"action":"lower","input":"MiXeD"}), |v| v["result"] == "mixed", "mixed"),
        tc("reverse_basic", json!({"action":"reverse","input":"abcde"}), |v| v["result"] == "edcba", "edcba"),
        tc("reverse_single", json!({"action":"reverse","input":"a"}), |v| v["result"] == "a", "a"),
        tc("reverse_palindrome", json!({"action":"reverse","input":"racecar"}), |v| v["result"] == "racecar", "racecar"),
        tc("reverse_phrase", json!({"action":"reverse","input":"hello"}), |v| v["result"] == "olleh", "olleh"),
        tc("slug_basic", json!({"action":"slug","input":"Hello World!"}), |v| v["result"] == "hello-world", "hello-world"),
        tc("slug_spaces", json!({"action":"slug","input":"foo bar baz"}), |v| v["result"] == "foo-bar-baz", "foo-bar-baz"),
        tc("slug_clean", json!({"action":"slug","input":"already-slug"}), |v| v["result"] == "already-slug", "already-slug"),
        tc("slug_special", json!({"action":"slug","input":"Hello @World# 2024"}), |v| v["result"] == "hello-world-2024", "hello-world-2024"),
        tc("cap_basic", json!({"action":"capitalize","input":"hello"}), |v| v["result"] == "Hello", "Hello"),
        tc("cap_single", json!({"action":"capitalize","input":"a"}), |v| v["result"] == "A", "A"),
        tc("cap_mixed", json!({"action":"capitalize","input":"mIxEd"}), |v| v["result"] == "MIxEd", "MIxEd"),
        tc("title_basic", json!({"action":"title","input":"hello world"}), |v| v["result"] == "Hello World", "Hello World"),
        tc("title_phrase", json!({"action":"title","input":"the quick brown fox"}), |v| v["result"] == "The Quick Brown Fox", "The Quick Brown Fox"),
        tc("title_single", json!({"action":"title","input":"hello"}), |v| v["result"] == "Hello", "Hello"),
    ]
}

fn make_js_calc_cases() -> Vec<ToolCase> {
    vec![
        tc("add", json!({"expression":"2 + 3"}), |v| v["result"] == 5, "5"),
        tc("mul", json!({"expression":"4 * 7"}), |v| v["result"] == 28, "28"),
        tc("sub", json!({"expression":"10 - 3"}), |v| v["result"] == 7, "7"),
        tc("div", json!({"expression":"15 / 4"}), |v| { let r = v["result"].as_f64().unwrap_or(0.0); (r - 3.75).abs() < 0.001 }, "3.75"),
        tc("nested", json!({"expression":"(10 + 5) * 2"}), |v| v["result"] == 30, "30"),
        tc("modulo", json!({"expression":"10 % 3"}), |v| v["result"] == 1, "1"),
        tc("parens", json!({"expression":"(2 + 3) * (4 + 1)"}), |v| v["result"] == 25, "25"),
        tc("large", json!({"expression":"1000 * 1000"}), |v| v["result"] == 1000000, "1000000"),
        tc("zero", json!({"expression":"0 + 0"}), |v| v["result"] == 0, "0"),
        tc("negative", json!({"expression":"-5 + 3"}), |v| v["result"] == -2, "-2"),
        tc("decimal", json!({"expression":"1.5 + 2.5"}), |v| v["result"] == 4, "4"),
        tc("power", json!({"expression":"2 ** 8"}), |v| v["result"] == 256, "256"),
        tc("complex", json!({"expression":"((2 + 3) * 4) - 1"}), |v| v["result"] == 19, "19"),
        tc("frac", json!({"expression":"1 / 3"}), |v| { let r = v["result"].as_f64().unwrap_or(0.0); (r - 0.333333).abs() < 0.001 }, "~0.333"),
        tc("dbl_div", json!({"expression":"100 / 10 / 2"}), |v| v["result"] == 5, "5"),
        tc("inv_alpha", json!({"expression":"alert(1)"}), |v| v.get("error").is_some(), "error"),
        tc("inv_fn", json!({"expression":"eval('x')"}), |v| v.get("error").is_some(), "error"),
        tc("inv_var", json!({"expression":"var x = 1"}), |v| v.get("error").is_some(), "error"),
        tc("inv_str", json!({"expression":"'hello'"}), |v| v.get("error").is_some(), "error"),
        tc("inv_proc", json!({"expression":"process.exit()"}), |v| v.get("error").is_some(), "error"),
    ]
}

fn make_js_flatten_cases() -> Vec<ToolCase> {
    vec![
        tc("nested", json!({"data":{"a":{"b":1},"c":2}}), |v| v["result"]["a.b"] == 1 && v["result"]["c"] == 2, "a.b=1, c=2"),
        tc("flat", json!({"data":{"x":10,"y":20}}), |v| v["result"]["x"] == 10 && v["result"]["y"] == 20, "x=10, y=20"),
        tc("prefix", json!({"data":{"a":1},"prefix":"pre."}), |v| v["result"]["pre.a"] == 1, "pre.a=1"),
        tc("deep", json!({"data":{"a":{"b":{"c":42}}}}), |v| v["result"]["a.b.c"] == 42, "a.b.c=42"),
        tc("wide", json!({"data":{"a":1,"b":2,"c":3,"d":4}}), |v| v["result"]["a"] == 1 && v["result"]["b"] == 2 && v["result"]["c"] == 3 && v["result"]["d"] == 4, "a=1..d=4"),
        tc("str_val", json!({"data":{"name":"alice","age":30}}), |v| v["result"]["name"] == "alice" && v["result"]["age"] == 30, "name=alice, age=30"),
        tc("bool_val", json!({"data":{"flag":true}}), |v| v["result"]["flag"] == true, "flag=true"),
        tc("arr_leaf", json!({"data":{"items":[1,2,3]}}), |v| v["result"]["items"] == json!([1,2,3]), "items=[1,2,3]"),
        tc("mix", json!({"data":{"a":{"x":1},"b":"hello","c":true}}), |v| v["result"]["a.x"] == 1 && v["result"]["b"] == "hello" && v["result"]["c"] == true, "a.x=1, b=hello, c=true"),
        tc("pfx_deep", json!({"data":{"a":{"b":1}},"prefix":"p_"}), |v| v["result"]["p_a.b"] == 1, "p_a.b=1"),
        tc("null_val", json!({"data":{"x":null}}), |v| v["result"]["x"].is_null(), "x=null"),
        tc("zero_val", json!({"data":{"x":0,"y":0}}), |v| v["result"]["x"] == 0 && v["result"]["y"] == 0, "x=0, y=0"),
        tc("neg_val", json!({"data":{"x":-5}}), |v| v["result"]["x"] == -5, "x=-5"),
        tc("single", json!({"data":{"only":99}}), |v| v["result"]["only"] == 99, "only=99"),
        tc("two_lvl", json!({"data":{"a":{"b":2},"c":{"d":4}}}), |v| v["result"]["a.b"] == 2 && v["result"]["c.d"] == 4, "a.b=2, c.d=4"),
        tc("arr_nest", json!({"data":{"outer":{"inner":[10,20]}}}), |v| v["result"]["outer.inner"] == json!([10,20]), "outer.inner=[10,20]"),
        tc("empty_pfx", json!({"data":{"a":1},"prefix":""}), |v| v["result"]["a"] == 1, "a=1"),
        tc("wide_nest", json!({"data":{"a":{"x":1,"y":2},"b":{"z":3}}}), |v| v["result"]["a.x"] == 1 && v["result"]["a.y"] == 2 && v["result"]["b.z"] == 3, "a.x=1, a.y=2, b.z=3"),
        tc("str_num", json!({"data":{"id":"abc","val":42}}), |v| v["result"]["id"] == "abc" && v["result"]["val"] == 42, "id=abc, val=42"),
        tc("deep_pfx", json!({"data":{"a":{"b":{"c":1}}},"prefix":"x."}), |v| v["result"]["x.a.b.c"] == 1, "x.a.b.c=1"),
    ]
}

fn make_ts_stats_cases() -> Vec<ToolCase> {
    vec![
        tc("two_words", json!({"text":"hello world"}), |v| v["chars"] == 11 && v["words"] == 2 && v["upper"] == "HELLO WORLD", "11/2/HELLO WORLD"),
        tc("one_word", json!({"text":"hello"}), |v| v["chars"] == 5 && v["words"] == 1 && v["upper"] == "HELLO", "5/1/HELLO"),
        tc("three_words", json!({"text":"one two three"}), |v| v["chars"] == 13 && v["words"] == 3 && v["upper"] == "ONE TWO THREE", "13/3/ONE TWO THREE"),
        tc("single_ch", json!({"text":"x"}), |v| v["chars"] == 1 && v["words"] == 1 && v["upper"] == "X", "1/1/X"),
        tc("sentence", json!({"text":"the quick brown fox"}), |v| v["chars"] == 19 && v["words"] == 4 && v["upper"] == "THE QUICK BROWN FOX", "19/4/THE QUICK BROWN FOX"),
        tc("numbers", json!({"text":"123 456"}), |v| v["chars"] == 7 && v["words"] == 2 && v["upper"] == "123 456", "7/2/123 456"),
        tc("mixed", json!({"text":"Hello World 123"}), |v| v["chars"] == 15 && v["words"] == 3 && v["upper"] == "HELLO WORLD 123", "15/3/HELLO WORLD 123"),
        tc("special", json!({"text":"hello! world?"}), |v| v["chars"] == 13 && v["words"] == 2 && v["upper"] == "HELLO! WORLD?", "13/2/HELLO! WORLD?"),
        tc("long_word", json!({"text":"supercalifragilistic"}), |v| v["chars"] == 20 && v["words"] == 1 && v["upper"] == "SUPERCALIFRAGILISTIC", "20/1/SUPERCALIFRAGILISTIC"),
        tc("five_words", json!({"text":"a b c d e"}), |v| v["chars"] == 9 && v["words"] == 5 && v["upper"] == "A B C D E", "9/5/A B C D E"),
        tc("upper_in", json!({"text":"ALREADY UPPER"}), |v| v["chars"] == 13 && v["words"] == 2 && v["upper"] == "ALREADY UPPER", "13/2/ALREADY UPPER"),
        tc("two_ch", json!({"text":"ab"}), |v| v["chars"] == 2 && v["words"] == 1 && v["upper"] == "AB", "2/1/AB"),
        tc("long_phrase", json!({"text":"one two three four five"}), |v| v["chars"] == 23 && v["words"] == 5 && v["upper"] == "ONE TWO THREE FOUR FIVE", "23/5/ONE TWO THREE FOUR FIVE"),
        tc("with_dash", json!({"text":"hello-world"}), |v| v["chars"] == 11 && v["words"] == 1 && v["upper"] == "HELLO-WORLD", "11/1/HELLO-WORLD"),
        tc("with_dot", json!({"text":"file.txt"}), |v| v["chars"] == 8 && v["words"] == 1 && v["upper"] == "FILE.TXT", "8/1/FILE.TXT"),
        tc("six_words", json!({"text":"the dog sat on the mat"}), |v| v["chars"] == 22 && v["words"] == 6 && v["upper"] == "THE DOG SAT ON THE MAT", "22/6/THE DOG SAT ON THE MAT"),
        tc("tabbed", json!({"text":"a\tb"}), |v| v["chars"] == 3 && v["upper"] == "A\tB", "3/A\\tB"),
        tc("punct", json!({"text":"hello, world!"}), |v| v["chars"] == 13 && v["words"] == 2 && v["upper"] == "HELLO, WORLD!", "13/2/HELLO, WORLD!"),
        tc("seven_ch", json!({"text":"abcdefg"}), |v| v["chars"] == 7 && v["words"] == 1 && v["upper"] == "ABCDEFG", "7/1/ABCDEFG"),
        tc("palindrome", json!({"text":"racecar"}), |v| v["chars"] == 7 && v["words"] == 1 && v["upper"] == "RACECAR", "7/1/RACECAR"),
    ]
}

fn make_py_text_cases() -> Vec<ToolCase> {
    vec![
        tc("repeat", json!({"text":"hello world hello"}), |v| v["words"] == 3 && v["chars"] == 17 && v["unique_words"] == 2, "w3 c17 u2"),
        tc("single", json!({"text":"hello"}), |v| v["words"] == 1 && v["chars"] == 5 && v["unique_words"] == 1, "w1 c5 u1"),
        tc("two", json!({"text":"hello world"}), |v| v["words"] == 2 && v["chars"] == 11 && v["unique_words"] == 2, "w2 c11 u2"),
        tc("all_same", json!({"text":"go go go"}), |v| v["words"] == 3 && v["chars"] == 8 && v["unique_words"] == 1, "w3 c8 u1"),
        tc("all_diff", json!({"text":"one two three four"}), |v| v["words"] == 4 && v["chars"] == 18 && v["unique_words"] == 4, "w4 c18 u4"),
        tc("ch", json!({"text":"x"}), |v| v["words"] == 1 && v["chars"] == 1 && v["unique_words"] == 1, "w1 c1 u1"),
        tc("sentence", json!({"text":"the cat sat on the mat"}), |v| v["words"] == 6 && v["unique_words"] == 5, "w6 u5"),
        tc("mixed_case", json!({"text":"Hello HELLO hello"}), |v| v["words"] == 3 && v["unique_words"] == 1, "w3 u1"),
        tc("nums", json!({"text":"123 456 789"}), |v| v["words"] == 3 && v["chars"] == 11 && v["unique_words"] == 3, "w3 c11 u3"),
        tc("long", json!({"text":"supercalifragilisticexpialidocious"}), |v| v["words"] == 1 && v["chars"] == 34, "w1 c34"),
        tc("five", json!({"text":"a b c d e"}), |v| v["words"] == 5 && v["chars"] == 9 && v["unique_words"] == 5, "w5 c9 u5"),
        tc("pairs", json!({"text":"aa bb aa bb"}), |v| v["words"] == 4 && v["unique_words"] == 2, "w4 u2"),
        tc("newline", json!({"text":"hello\nworld"}), |v| v["words"] == 2 && v["chars"] == 11, "w2 c11"),
        tc("special", json!({"text":"hello! world?"}), |v| v["words"] == 2 && v["chars"] == 13 && v["unique_words"] == 2, "w2 c13 u2"),
        tc("three_same", json!({"text":"go go go go"}), |v| v["words"] == 4 && v["unique_words"] == 1, "w4 u1"),
        tc("two_ch", json!({"text":"ab"}), |v| v["words"] == 1 && v["chars"] == 2, "w1 c2"),
        tc("phrase", json!({"text":"one two one two one"}), |v| v["words"] == 5 && v["unique_words"] == 2, "w5 u2"),
        tc("with_dash", json!({"text":"hello-world foo"}), |v| v["words"] == 2 && v["chars"] == 15, "w2 c15"),
        tc("long_repeat", json!({"text":"test test test test test"}), |v| v["words"] == 5 && v["unique_words"] == 1, "w5 u1"),
        tc("six", json!({"text":"a bb ccc dddd e f"}), |v| v["words"] == 6 && v["chars"] == 17 && v["unique_words"] == 6, "w6 c17 u6"),
    ]
}

fn make_py_unit_cases() -> Vec<ToolCase> {
    vec![
        tc("c_f_100", json!({"value":100,"from_unit":"celsius","to_unit":"fahrenheit"}), |v| v["result"] == 212.0, "100C=212F"),
        tc("c_f_0", json!({"value":0,"from_unit":"celsius","to_unit":"fahrenheit"}), |v| v["result"] == 32.0, "0C=32F"),
        tc("c_f_37", json!({"value":37,"from_unit":"celsius","to_unit":"fahrenheit"}), |v| v["result"] == 98.6, "37C=98.6F"),
        tc("c_k_0", json!({"value":0,"from_unit":"celsius","to_unit":"kelvin"}), |v| v["result"] == 273.15, "0C=273.15K"),
        tc("c_k_100", json!({"value":100,"from_unit":"celsius","to_unit":"kelvin"}), |v| v["result"] == 373.15, "100C=373.15K"),
        tc("f_k_32", json!({"value":32,"from_unit":"fahrenheit","to_unit":"kelvin"}), |v| v["result"] == 273.15, "32F=273.15K"),
        tc("f_k_212", json!({"value":212,"from_unit":"fahrenheit","to_unit":"kelvin"}), |v| v["result"] == 373.15, "212F=373.15K"),
        tc("k_c_273", json!({"value":273.15,"from_unit":"kelvin","to_unit":"celsius"}), |v| v["result"] == 0.0, "273.15K=0C"),
        tc("k_c_373", json!({"value":373.15,"from_unit":"kelvin","to_unit":"celsius"}), |v| v["result"] == 100.0, "373.15K=100C"),
        tc("k_f_273", json!({"value":273.15,"from_unit":"kelvin","to_unit":"fahrenheit"}), |v| v["result"] == 32.0, "273.15K=32F"),
        tc("c_c", json!({"value":50,"from_unit":"celsius","to_unit":"celsius"}), |v| v["result"] == 50.0, "50C=50C"),
        tc("f_f", json!({"value":72,"from_unit":"fahrenheit","to_unit":"fahrenheit"}), |v| v["result"] == 72.0, "72F=72F"),
        tc("m_ft", json!({"value":1,"from_unit":"meter","to_unit":"foot"}), |v| { let r = v["result"].as_f64().unwrap_or(0.0); (r - 3.28084).abs() < 0.001 }, "1m~3.28084ft"),
        tc("ft_m", json!({"value":10,"from_unit":"foot","to_unit":"meter"}), |v| { let r = v["result"].as_f64().unwrap_or(0.0); (r - 3.048).abs() < 0.001 }, "10ft~3.048m"),
        tc("in_m", json!({"value":39.37,"from_unit":"inch","to_unit":"meter"}), |v| { let r = v["result"].as_f64().unwrap_or(0.0); (r - 0.999998).abs() < 0.01 }, "39.37in~1m"),
        tc("km_mi", json!({"value":1,"from_unit":"km","to_unit":"mile"}), |v| { let r = v["result"].as_f64().unwrap_or(0.0); (r - 0.621371).abs() < 0.001 }, "1km~0.621mi"),
        tc("mi_km", json!({"value":1,"from_unit":"mile","to_unit":"km"}), |v| v["result"] == 1.609344, "1mi=1.609344km"),
        tc("zero", json!({"value":0,"from_unit":"meter","to_unit":"foot"}), |v| v["result"] == 0.0, "0m=0ft"),
        tc("incompat", json!({"value":1,"from_unit":"celsius","to_unit":"meter"}), |v| v.get("error").is_some(), "error"),
        tc("unknown", json!({"value":1,"from_unit":"gallon","to_unit":"liter"}), |v| v.get("error").is_some(), "error"),
    ]
}

fn make_py_list_cases() -> Vec<ToolCase> {
    vec![
        tc("sort3", json!({"operation":"sort","items":[3,1,2]}), |v| v["result"] == json!([1,2,3]), "[1,2,3]"),
        tc("sort_neg", json!({"operation":"sort","items":[5,-1,3,0,-2]}), |v| v["result"] == json!([-2,-1,0,3,5]), "[-2,-1,0,3,5]"),
        tc("sort_dup", json!({"operation":"sort","items":[3,1,2,1,3]}), |v| v["result"] == json!([1,1,2,3,3]), "[1,1,2,3,3]"),
        tc("sort_one", json!({"operation":"sort","items":[42]}), |v| v["result"] == json!([42]), "[42]"),
        tc("rev3", json!({"operation":"reverse","items":[1,2,3]}), |v| v["result"] == json!([3,2,1]), "[3,2,1]"),
        tc("rev5", json!({"operation":"reverse","items":[10,20,30,40,50]}), |v| v["result"] == json!([50,40,30,20,10]), "[50,40,30,20,10]"),
        tc("rev1", json!({"operation":"reverse","items":[1]}), |v| v["result"] == json!([1]), "[1]"),
        tc("rev_pal", json!({"operation":"reverse","items":[1,2,1]}), |v| v["result"] == json!([1,2,1]), "[1,2,1]"),
        tc("uniq_dup", json!({"operation":"unique","items":[1,2,2,3,3,3]}), |v| v["result"] == json!([1,2,3]), "[1,2,3]"),
        tc("uniq_none", json!({"operation":"unique","items":[1,2,3]}), |v| v["result"] == json!([1,2,3]), "[1,2,3]"),
        tc("uniq_all", json!({"operation":"unique","items":[5,5,5]}), |v| v["result"] == json!([5]), "[5]"),
        tc("uniq_one", json!({"operation":"unique","items":[42]}), |v| v["result"] == json!([42]), "[42]"),
        tc("min3", json!({"operation":"min","items":[5,2,8]}), |v| v["result"] == 2, "2"),
        tc("min_neg", json!({"operation":"min","items":[-1,-5,-3]}), |v| v["result"] == -5, "-5"),
        tc("min_first", json!({"operation":"min","items":[1,2,3]}), |v| v["result"] == 1, "1"),
        tc("min_last", json!({"operation":"min","items":[3,2,1]}), |v| v["result"] == 1, "1"),
        tc("max3", json!({"operation":"max","items":[5,2,8]}), |v| v["result"] == 8, "8"),
        tc("max_neg", json!({"operation":"max","items":[-1,-5,-3]}), |v| v["result"] == -1, "-1"),
        tc("sum4", json!({"operation":"sum","items":[1,2,3,4]}), |v| v["result"] == 10, "10"),
        tc("sum_neg", json!({"operation":"sum","items":[-1,1,-2,2]}), |v| v["result"] == 0, "0"),
    ]
}

fn make_lua_string_cases() -> Vec<ToolCase> {
    vec![
        tc("up1", json!({"action":"upper","input":"hello"}), |v| v["result"] == "HELLO", "HELLO"),
        tc("up2", json!({"action":"upper","input":"world"}), |v| v["result"] == "WORLD", "WORLD"),
        tc("up3", json!({"action":"upper","input":"abc123"}), |v| v["result"] == "ABC123", "ABC123"),
        tc("lo1", json!({"action":"lower","input":"HELLO"}), |v| v["result"] == "hello", "hello"),
        tc("lo2", json!({"action":"lower","input":"WORLD"}), |v| v["result"] == "world", "world"),
        tc("lo3", json!({"action":"lower","input":"ABC123"}), |v| v["result"] == "abc123", "abc123"),
        tc("rev1", json!({"action":"reverse","input":"abc"}), |v| v["result"] == "cba", "cba"),
        tc("rev2", json!({"action":"reverse","input":"hello"}), |v| v["result"] == "olleh", "olleh"),
        tc("rev3", json!({"action":"reverse","input":"a"}), |v| v["result"] == "a", "a"),
        tc("rep1", json!({"action":"repeat","input":"ab","count":3}), |v| v["result"] == "ababab", "ababab"),
        tc("rep2", json!({"action":"repeat","input":"x","count":5}), |v| v["result"] == "xxxxx", "xxxxx"),
        tc("rep3", json!({"action":"repeat","input":"ha","count":2}), |v| v["result"] == "haha", "haha"),
        tc("trim1", json!({"action":"trim","input":"  hello  "}), |v| v["result"] == "hello", "hello"),
        tc("trim2", json!({"action":"trim","input":"no_spaces"}), |v| v["result"] == "no_spaces", "no_spaces"),
        tc("trim3", json!({"action":"trim","input":"  leading"}), |v| v["result"] == "leading", "leading"),
        tc("wc1", json!({"action":"word_count","input":"one two three"}), |v| v["result"] == 3, "3"),
        tc("wc2", json!({"action":"word_count","input":"hello"}), |v| v["result"] == 1, "1"),
        tc("wc3", json!({"action":"word_count","input":"a b c d e"}), |v| v["result"] == 5, "5"),
        tc("up4", json!({"action":"upper","input":"mixed Case"}), |v| v["result"] == "MIXED CASE", "MIXED CASE"),
        tc("rev4", json!({"action":"reverse","input":"racecar"}), |v| v["result"] == "racecar", "racecar"),
    ]
}

fn make_lua_math_cases() -> Vec<ToolCase> {
    vec![
        tc("pow2_10", json!({"operation":"power","value":2,"exponent":10}), |v| v["result"] == 1024, "2^10=1024"),
        tc("pow3_3", json!({"operation":"power","value":3,"exponent":3}), |v| v["result"] == 27, "3^3=27"),
        tc("pow5_2", json!({"operation":"power","value":5,"exponent":2}), |v| v["result"] == 25, "5^2=25"),
        tc("pow10_0", json!({"operation":"power","value":10,"exponent":0}), |v| v["result"] == 1, "10^0=1"),
        tc("sqrt1", json!({"operation":"sqrt","value":1}), |v| v["result"] == 1, "1"),
        tc("sqrt4", json!({"operation":"sqrt","value":4}), |v| v["result"] == 2, "2"),
        tc("sqrt9", json!({"operation":"sqrt","value":9}), |v| v["result"] == 3, "3"),
        tc("sqrt100", json!({"operation":"sqrt","value":100}), |v| v["result"] == 10, "10"),
        tc("rnd1", json!({"operation":"round","value":2.5}), |v| v["result"] == 3, "3"),
        tc("rnd2", json!({"operation":"round","value":3.7}), |v| v["result"] == 4, "4"),
        tc("rnd3", json!({"operation":"round","value":3.2}), |v| v["result"] == 3, "3"),
        tc("flr1", json!({"operation":"floor","value":3.7}), |v| v["result"] == 3, "3"),
        tc("flr2", json!({"operation":"floor","value":3.99}), |v| v["result"] == 3, "3"),
        tc("flr3", json!({"operation":"floor","value":0.5}), |v| v["result"] == 0, "0"),
        tc("ceil1", json!({"operation":"ceil","value":3.2}), |v| v["result"] == 4, "4"),
        tc("ceil2", json!({"operation":"ceil","value":3.9}), |v| v["result"] == 4, "4"),
        tc("ceil3", json!({"operation":"ceil","value":0.1}), |v| v["result"] == 1, "1"),
        tc("abs1", json!({"operation":"abs","value":-42}), |v| v["result"] == 42, "42"),
        tc("abs2", json!({"operation":"abs","value":42}), |v| v["result"] == 42, "42"),
        tc("fib10", json!({"operation":"fibonacci","value":10}), |v| v["result"] == 55, "55"),
    ]
}

fn make_rb_stats_cases() -> Vec<ToolCase> {
    vec![
        tc("two", json!({"text":"hello world"}), |v| v["chars"] == 11 && v["words"] == 2 && v["upper"] == "HELLO WORLD", "11/2/HELLO WORLD"),
        tc("one", json!({"text":"hello"}), |v| v["chars"] == 5 && v["words"] == 1 && v["upper"] == "HELLO", "5/1/HELLO"),
        tc("three", json!({"text":"one two three"}), |v| v["chars"] == 13 && v["words"] == 3 && v["upper"] == "ONE TWO THREE", "13/3/ONE TWO THREE"),
        tc("single", json!({"text":"x"}), |v| v["chars"] == 1 && v["words"] == 1 && v["upper"] == "X", "1/1/X"),
        tc("four", json!({"text":"the quick brown fox"}), |v| v["chars"] == 19 && v["words"] == 4 && v["upper"] == "THE QUICK BROWN FOX", "19/4/THE QUICK BROWN FOX"),
        tc("nums", json!({"text":"123 456"}), |v| v["chars"] == 7 && v["words"] == 2 && v["upper"] == "123 456", "7/2/123 456"),
        tc("mixed", json!({"text":"Hello World 123"}), |v| v["chars"] == 15 && v["words"] == 3 && v["upper"] == "HELLO WORLD 123", "15/3/HELLO WORLD 123"),
        tc("punct", json!({"text":"hello! world?"}), |v| v["chars"] == 13 && v["words"] == 2 && v["upper"] == "HELLO! WORLD?", "13/2/HELLO! WORLD?"),
        tc("long", json!({"text":"supercalifragilistic"}), |v| v["chars"] == 20 && v["words"] == 1 && v["upper"] == "SUPERCALIFRAGILISTIC", "20/1/SUPERCALIFRAGILISTIC"),
        tc("five", json!({"text":"a b c d e"}), |v| v["chars"] == 9 && v["words"] == 5 && v["upper"] == "A B C D E", "9/5/A B C D E"),
        tc("upper", json!({"text":"ALREADY UPPER"}), |v| v["chars"] == 13 && v["words"] == 2 && v["upper"] == "ALREADY UPPER", "13/2/ALREADY UPPER"),
        tc("two_ch", json!({"text":"ab"}), |v| v["chars"] == 2 && v["words"] == 1 && v["upper"] == "AB", "2/1/AB"),
        tc("six", json!({"text":"one two three four five"}), |v| v["chars"] == 23 && v["words"] == 5 && v["upper"] == "ONE TWO THREE FOUR FIVE", "23/5/ONE TWO THREE FOUR FIVE"),
        tc("dash", json!({"text":"hello-world"}), |v| v["chars"] == 11 && v["words"] == 1 && v["upper"] == "HELLO-WORLD", "11/1/HELLO-WORLD"),
        tc("dot", json!({"text":"file.txt"}), |v| v["chars"] == 8 && v["words"] == 1 && v["upper"] == "FILE.TXT", "8/1/FILE.TXT"),
        tc("seven", json!({"text":"the dog sat on the mat"}), |v| v["chars"] == 22 && v["words"] == 6 && v["upper"] == "THE DOG SAT ON THE MAT", "22/6/THE DOG SAT ON THE MAT"),
        tc("comma", json!({"text":"hello, world!"}), |v| v["chars"] == 13 && v["words"] == 2 && v["upper"] == "HELLO, WORLD!", "13/2/HELLO, WORLD!"),
        tc("alpha", json!({"text":"abcdefg"}), |v| v["chars"] == 7 && v["words"] == 1 && v["upper"] == "ABCDEFG", "7/1/ABCDEFG"),
        tc("palin", json!({"text":"racecar"}), |v| v["chars"] == 7 && v["words"] == 1 && v["upper"] == "RACECAR", "7/1/RACECAR"),
        tc("space", json!({"text":"foo bar"}), |v| v["chars"] == 7 && v["words"] == 2 && v["upper"] == "FOO BAR", "7/2/FOO BAR"),
    ]
}

fn make_greet_cases(names: &[&'static str]) -> Vec<ToolCase> {
    names.iter().map(|name| {
        let n = *name;
        tc(
            &format!("greet_{}", n.to_lowercase()),
            json!({"name": n}),
            move |v: &Value| greeting_has_name(v, n),
            &format!("Hello, {}!", n),
        )
    }).collect()
}

fn define_all_tests() -> Vec<WasmToolTest> {
    let greet_names: Vec<&'static str> = vec![
        "World", "Alice", "Bob", "Charlie", "Dave",
        "Eve", "Frank", "Grace", "Heidi", "Ivan",
        "Judy", "Karl", "Liam", "Mallory", "Nancy",
        "Oscar", "Peggy", "Quinn", "Rupert", "Trent",
    ];

    vec![
        // ── JavaScript (3 tools) ──
        WasmToolTest {
            tool_name: "js_string_transform",
            language: "javascript",
            wasm_path: QUICKJS_WASM,
            source: r#"function js_string_transform(args) { var s = args.input || ''; switch (args.action) { case 'upper': return {result: s.toUpperCase()}; case 'lower': return {result: s.toLowerCase()}; case 'reverse': return {result: s.split('').reverse().join('')}; case 'slug': return {result: s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')}; case 'capitalize': return {result: s.charAt(0).toUpperCase() + s.slice(1)}; case 'title': return {result: s.replace(/\b\w/g, function(c) { return c.toUpperCase(); })}; default: return {error: 'Unknown action: ' + args.action}; } }"#,
            cases: make_js_string_cases(),
        },
        WasmToolTest {
            tool_name: "js_calculator",
            language: "javascript",
            wasm_path: QUICKJS_WASM,
            source: r#"function js_calculator(args) { var expr = args.expression || ''; if (!/^[0-9+\-*/().\s%^]+$/.test(expr)) return {error: 'Invalid characters in expression'}; try { var result = Function('"use strict"; return (' + expr + ')')(); return {result: result, expression: expr}; } catch(e) { return {error: e.message}; } }"#,
            cases: make_js_calc_cases(),
        },
        WasmToolTest {
            tool_name: "js_data_flatten",
            language: "javascript",
            wasm_path: QUICKJS_WASM,
            source: r#"function js_data_flatten(args) { var result = {}; function flatten(obj, pre) { for (var k in obj) { if (obj[k] && typeof obj[k] === 'object' && !Array.isArray(obj[k])) { flatten(obj[k], pre + k + '.'); } else { result[pre + k] = obj[k]; } } } flatten(args.data || {}, args.prefix || ''); return {result: result}; }"#,
            cases: make_js_flatten_cases(),
        },

        // ── TypeScript (1 tool, runs on QuickJS) ──
        WasmToolTest {
            tool_name: "ts_text_stats",
            language: "typescript",
            wasm_path: QUICKJS_WASM,
            source: r#"function ts_text_stats(args) { var text = args.text || ''; var words = text.split(' '); return { chars: text.length, words: words.length, upper: text.toUpperCase() }; }"#,
            cases: make_ts_stats_cases(),
        },

        // ── Python (3 tools) ──
        WasmToolTest {
            tool_name: "py_text_analyzer",
            language: "python",
            wasm_path: MICROPYTHON_WASM,
            source: r#"def py_text_analyzer(args):
    text = args.get('text', '')
    words = text.split()
    return {'words': len(words), 'chars': len(text), 'lines': text.count(chr(10)) + 1 if text else 0, 'unique_words': len(set(w.lower() for w in words))}"#,
            cases: make_py_text_cases(),
        },
        WasmToolTest {
            tool_name: "py_unit_converter",
            language: "python",
            wasm_path: MICROPYTHON_WASM,
            source: r#"def py_unit_converter(args):
    v = args.get('value', 0)
    f = args.get('from_unit', '')
    t = args.get('to_unit', '')
    temp = {'celsius': 'c', 'fahrenheit': 'f', 'kelvin': 'k'}
    length = {'meter': 'm', 'foot': 'ft', 'inch': 'in', 'km': 'km', 'mile': 'mi'}
    if f in temp and t in temp:
        c = v if temp[f] == 'c' else (v - 32) * 5/9 if temp[f] == 'f' else v - 273.15
        r = c if temp[t] == 'c' else c * 9/5 + 32 if temp[t] == 'f' else c + 273.15
        return {'result': round(r, 4), 'from': f, 'to': t}
    if f in length and t in length:
        to_m = {'m': 1, 'ft': 0.3048, 'in': 0.0254, 'km': 1000, 'mi': 1609.344}
        meters = v * to_m[length[f]]
        r = meters / to_m[length[t]]
        return {'result': round(r, 6), 'from': f, 'to': t}
    return {'error': 'incompatible units'}"#,
            cases: make_py_unit_cases(),
        },
        WasmToolTest {
            tool_name: "py_list_operations",
            language: "python",
            wasm_path: MICROPYTHON_WASM,
            source: r#"def py_list_operations(args):
    op = args.get('operation', '')
    items = args.get('items', [])
    if op == 'sort': return {'result': sorted(items)}
    if op == 'reverse': return {'result': list(reversed(items))}
    if op == 'unique': return {'result': sorted(set(items))}
    if op == 'min': return {'result': min(items)}
    if op == 'max': return {'result': max(items)}
    if op == 'sum': return {'result': sum(items)}
    if op == 'average': return {'result': sum(items) / len(items) if items else 0}
    return {'error': 'unknown operation'}"#,
            cases: make_py_list_cases(),
        },

        // ── Lua (2 tools) ──
        WasmToolTest {
            tool_name: "lua_string_utils",
            language: "lua",
            wasm_path: LUA_WASM,
            source: r#"function lua_string_utils(args)
  local s = args.input or ''
  local a = args.action or ''
  if a == 'upper' then return {result = string.upper(s)}
  elseif a == 'lower' then return {result = string.lower(s)}
  elseif a == 'reverse' then return {result = string.reverse(s)}
  elseif a == 'repeat' then
    local n = args.count or 2
    return {result = string.rep(s, n)}
  elseif a == 'trim' then
    local t = s:match('^%s*(.-)%s*$')
    return {result = t}
  elseif a == 'word_count' then
    local c = 0
    for _ in s:gmatch('%S+') do c = c + 1 end
    return {result = c}
  else
    return {error = 'unknown action: ' .. a}
  end
end"#,
            cases: make_lua_string_cases(),
        },
        WasmToolTest {
            tool_name: "lua_math_tools",
            language: "lua",
            wasm_path: LUA_WASM,
            source: r#"function lua_math_tools(args)
  local op = args.operation or ''
  local v = args.value or 0
  if op == 'power' then return {result = v ^ (args.exponent or 2)}
  elseif op == 'sqrt' then return {result = math.sqrt(v)}
  elseif op == 'round' then return {result = math.floor(v + 0.5)}
  elseif op == 'floor' then return {result = math.floor(v)}
  elseif op == 'ceil' then return {result = math.ceil(v)}
  elseif op == 'abs' then return {result = math.abs(v)}
  elseif op == 'fibonacci' then
    local n = math.floor(v)
    if n <= 0 then return {result = 0}
    elseif n == 1 then return {result = 1}
    end
    local a, b = 0, 1
    for _ = 2, n do a, b = b, a + b end
    return {result = b}
  else
    return {error = 'unknown operation: ' .. op}
  end
end"#,
            cases: make_lua_math_cases(),
        },

        // ── Ruby (1 tool) ──
        WasmToolTest {
            tool_name: "rb_text_stats",
            language: "ruby",
            wasm_path: RUBY_WASM,
            source: r#"def rb_text_stats(args)
  text = args['text'].to_s
  {'chars' => text.length, 'words' => text.split.length, 'upper' => text.upcase}
end"#,
            cases: make_rb_stats_cases(),
        },

        // ── Greeting tools (6 languages, 20 names each) ──
        WasmToolTest {
            tool_name: "php_greet",
            language: "php",
            wasm_path: PHP_WASM,
            source: r#"<?php set_tool_result("Hello, " . $name . "!");"#,
            cases: make_greet_cases(&greet_names),
        },
        WasmToolTest {
            tool_name: "csharp_greet",
            language: "csharp",
            wasm_path: CSHARP_WASM,
            source: r#"set_tool_result("Hello, " + name + "!");"#,
            cases: make_greet_cases(&greet_names),
        },
        WasmToolTest {
            tool_name: "java_greet",
            language: "java",
            wasm_path: JAVA_WASM,
            source: r#"set_tool_result("Hello, " + name + "!");"#,
            cases: make_greet_cases(&greet_names),
        },
        WasmToolTest {
            tool_name: "r_greet",
            language: "r",
            wasm_path: R_WASM,
            source: r#"set_tool_result("Hello, " + name + "!")"#,
            cases: make_greet_cases(&greet_names),
        },
        WasmToolTest {
            tool_name: "julia_greet",
            language: "julia",
            wasm_path: JULIA_WASM,
            source: r#"set_tool_result("Hello, " + name + "!")"#,
            cases: make_greet_cases(&greet_names),
        },
        WasmToolTest {
            tool_name: "perl_greet",
            language: "perl",
            wasm_path: PERL_WASM,
            source: r#"set_tool_result("Hello, " . $name . "!");"#,
            cases: make_greet_cases(&greet_names),
        },
    ]
}

fn test_js_native() -> (usize, usize) {
    let node_available = Command::new("node").arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !node_available {
        println!("  Node.js not available, skipping native comparison\n");
        return (0, 0);
    }

    let mut passed = 0;
    let mut failed = 0;

    let js_tests: Vec<(&str, &str, Value, fn(&Value) -> bool, &str)> = vec![
        ("js_string_transform upper",
         r#"var args = JSON.parse(require('fs').readFileSync(0, 'utf-8')); var s = args.input || ''; console.log(JSON.stringify({result: s.toUpperCase()}))"#,
         json!({"action": "upper", "input": "hello world"}),
         |v| v["result"] == "HELLO WORLD", "HELLO WORLD"),
        ("js_string_transform reverse",
         r#"var args = JSON.parse(require('fs').readFileSync(0, 'utf-8')); var s = args.input || ''; console.log(JSON.stringify({result: s.split('').reverse().join('')}))"#,
         json!({"action": "reverse", "input": "abcde"}),
         |v| v["result"] == "edcba", "edcba"),
        ("js_calculator add",
         r#"var args = JSON.parse(require('fs').readFileSync(0, 'utf-8')); var expr = args.expression || ''; try { var result = Function('"use strict"; return (' + expr + ')')(); console.log(JSON.stringify({result: result})); } catch(e) { console.log(JSON.stringify({error: e.message})); }"#,
         json!({"expression": "2 + 3"}),
         |v| v["result"] == 5, "5"),
        ("js_data_flatten nested",
         r#"var args = JSON.parse(require('fs').readFileSync(0, 'utf-8')); var result = {}; function flatten(obj, pre) { for (var k in obj) { if (obj[k] && typeof obj[k] === 'object' && !Array.isArray(obj[k])) { flatten(obj[k], pre + k + '.'); } else { result[pre + k] = obj[k]; } } } flatten(args.data || {}, args.prefix || ''); console.log(JSON.stringify({result: result}))"#,
         json!({"data": {"a": {"b": 1}, "c": 2}}),
         |v| v["result"]["a.b"] == 1 && v["result"]["c"] == 2, "a.b=1, c=2"),
    ];

    println!("  Node.js native execution:");
    for (name, script, input, check, desc) in &js_tests {
        let mut child = match Command::new("node")
            .args(["-e", script])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                println!("    FAIL  {} — spawn error: {}", name, e);
                failed += 1;
                continue;
            }
        };

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            let _ = writeln!(stdin, "{}", input);
        }

        match child.wait_with_output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                match serde_json::from_str::<Value>(stdout.trim()) {
                    Ok(result) => {
                        if check(&result) {
                            println!("    PASS  {}", name);
                            passed += 1;
                        } else {
                            println!("    FAIL  {} — expected {}", name, desc);
                            println!("           got: {}", result);
                            failed += 1;
                        }
                    }
                    Err(e) => {
                        println!("    FAIL  {} — invalid JSON: {} (raw: {})", name, e, stdout.trim());
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                println!("    FAIL  {} — wait error: {}", name, e);
                failed += 1;
            }
        }
    }
    println!();

    (passed, failed)
}
