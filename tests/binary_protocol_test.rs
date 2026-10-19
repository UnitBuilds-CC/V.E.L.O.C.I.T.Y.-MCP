//! Binary Protocol (TLV) Tests
//!
//! Comprehensive tests for the NDA TLV binary argument protocol across all WASM runtimes.
//! Tests cover: encoding, decoding, round-trip correctness, error handling, and performance.

use velocity_mcp::protocol::nda_native;

/// Helper to build a simple TLV object with string fields
fn build_tlv_object(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut tlv = Vec::new();

    // Object tag + count
    tlv.push(0x06); // Object tag
    tlv.extend_from_slice(&(fields.len() as u32).to_be_bytes());

    for (key, value) in fields {
        // Key length (u16 BE) + key bytes
        tlv.extend_from_slice(&(key.len() as u16).to_be_bytes());
        tlv.extend_from_slice(key.as_bytes());

        // Value: String tag + length + bytes
        tlv.push(0x01); // String tag
        tlv.extend_from_slice(&(value.len() as u32).to_be_bytes());
        tlv.extend_from_slice(value.as_bytes());
    }

    tlv
}

#[test]
fn test_tlv_encode_decode_simple_object() {
    let fields = vec![("name", "Alice"), ("city", "Henties Bay")];
    let tlv = build_tlv_object(&fields);

    // Decode back
    let (value, _) = nda_native::decode_json_value(&tlv).expect("TLV decode failed");

    assert!(value.is_object(), "Expected object, got {:?}", value);
    let obj = value.as_object().unwrap();
    assert_eq!(obj["name"], "Alice");
    assert_eq!(obj["city"], "Henties Bay");
}

#[test]
fn test_tlv_roundtrip_preserves_data() {
    let original_fields = vec![
        ("tool_name", "greet_user"),
        ("language", "php"),
        ("version", "8.2"),
    ];

    let tlv = build_tlv_object(&original_fields);
    let (decoded, _) = nda_native::decode_json_value(&tlv).expect("decode failed");

    // Re-encode to JSON for comparison
    let json_str = serde_json::to_string(&decoded).expect("serialize failed");
    let re_parsed: serde_json::Value = serde_json::from_str(&json_str).expect("re-parse failed");

    assert_eq!(re_parsed["tool_name"], "greet_user");
    assert_eq!(re_parsed["language"], "php");
    assert_eq!(re_parsed["version"], "8.2");
}

#[test]
fn test_tlv_with_nested_objects() {
    let mut tlv = Vec::new();

    // Outer object with 2 fields
    tlv.push(0x06); // Object
    tlv.extend_from_slice(&2u32.to_be_bytes());

    // Field 1: "config" -> nested object
    tlv.extend_from_slice(&6u16.to_be_bytes()); // "config"
    tlv.extend(b"config");

    // Nested object with 1 field
    tlv.push(0x06); // Object
    tlv.extend_from_slice(&1u32.to_be_bytes());
    tlv.extend_from_slice(&4u16.to_be_bytes()); // "mode"
    tlv.extend(b"mode");
    tlv.push(0x01); // String
    tlv.extend_from_slice(&5u32.to_be_bytes()); // "debug"
    tlv.extend(b"debug");

    // Field 2: "enabled" -> bool true
    tlv.extend_from_slice(&7u16.to_be_bytes()); // "enabled"
    tlv.extend(b"enabled");
    tlv.push(0x03); // Bool
    tlv.push(0x01); // true

    let (value, _) = nda_native::decode_json_value(&tlv).expect("nested TLV decode failed");

    assert!(value.is_object());
    let config = &value["config"];
    assert!(config.is_object());
    assert_eq!(config["mode"], "debug");
    assert_eq!(value["enabled"], true);
}

#[test]
fn test_tlv_with_arrays() {
    let mut tlv = Vec::new();

    // Array with 3 elements
    tlv.push(0x05); // Array tag
    tlv.extend_from_slice(&3u32.to_be_bytes());

    // Element 1: string
    tlv.push(0x01);
    tlv.extend_from_slice(&3u32.to_be_bytes());
    tlv.extend(b"foo");

    // Element 2: integer
    tlv.push(0x02);
    tlv.extend_from_slice(&42i64.to_be_bytes());

    // Element 3: null
    tlv.push(0x04);

    let (value, _) = nda_native::decode_json_value(&tlv).expect("array TLV decode failed");

    assert!(value.is_array());
    let arr = value.as_array().unwrap();
    assert_eq!(arr.len(), 3);
    assert_eq!(arr[0], "foo");
    assert_eq!(arr[1], 42);
    assert!(arr[2].is_null());
}

#[test]
fn test_tlv_with_floats() {
    let mut tlv = Vec::new();

    // Object with float field
    tlv.push(0x06); // Object
    tlv.extend_from_slice(&1u32.to_be_bytes());

    tlv.extend_from_slice(&2u16.to_be_bytes()); // "pi"
    tlv.extend(b"pi");

    tlv.push(0x07); // Float tag
    let pi: f64 = std::f64::consts::PI;
    tlv.extend_from_slice(&pi.to_be_bytes());

    let (value, _) = nda_native::decode_json_value(&tlv).expect("float TLV decode failed");

    let decoded_pi = value["pi"].as_f64().expect("expected float");
    assert!((decoded_pi - pi).abs() < 1e-10, "Float precision lost");
}

#[test]
fn test_tlv_empty_object() {
    let mut tlv = Vec::new();
    tlv.push(0x06); // Object
    tlv.extend_from_slice(&0u32.to_be_bytes());

    let (value, _) = nda_native::decode_json_value(&tlv).expect("empty object decode failed");

    assert!(value.is_object());
    assert_eq!(value.as_object().unwrap().len(), 0);
}

#[test]
fn test_tlv_empty_array() {
    let mut tlv = Vec::new();
    tlv.push(0x05); // Array
    tlv.extend_from_slice(&0u32.to_be_bytes());

    let (value, _) = nda_native::decode_json_value(&tlv).expect("empty array decode failed");

    assert!(value.is_array());
    assert_eq!(value.as_array().unwrap().len(), 0);
}

#[test]
fn test_tlv_depth_limit_rejection() {
    // Build deeply nested structure (>32 levels should fail)
    let mut tlv = Vec::new();

    for _ in 0..35 {
        tlv.push(0x06); // Object
        tlv.extend_from_slice(&1u32.to_be_bytes());
        tlv.extend_from_slice(&1u16.to_be_bytes()); // "a"
        tlv.extend(b"a");
    }

    // Leaf value
    tlv.push(0x01);
    tlv.extend_from_slice(&4u32.to_be_bytes());
    tlv.extend(b"deep");

    // Should fail due to depth limit
    let result = nda_native::decode_json_value(&tlv);
    assert!(result.is_err(), "Should reject deeply nested TLV");
}

#[test]
fn test_tlv_large_string_limit() {
    let mut tlv = Vec::new();
    tlv.push(0x01); // String
                    // Claim 11MB string (exceeds 10MB limit)
    tlv.extend_from_slice(&(11 * 1024 * 1024u32).to_be_bytes());
    tlv.extend(b"x"); // Just one byte, but length says 11MB

    let result = nda_native::decode_json_value(&tlv);
    assert!(result.is_err(), "Should reject oversized string claim");
}

#[test]
fn test_call_tool_binary_trait_exists() {
    use velocity_mcp::wasm_runtime::WasmRuntime;

    // Verify the trait method exists and has default impl
    // This is a compile-time check - if it compiles, the method exists
    // The alias would hide the very signature this probe exists to pin.
    #[allow(clippy::type_complexity)]
    fn _assert_trait_method<T: WasmRuntime>() {
        // The call_tool_binary method must exist on the trait
        let _: fn(&mut T, &str, &[u8]) -> Result<String, Box<dyn std::error::Error>> =
            |rt, name, args| rt.call_tool_binary(name, args);
    }
}

#[test]
fn test_registry_dispatches_binary_correctly() {
    use velocity_mcp::registry;

    // Test that registry::call_tool_binary properly routes to WASM tools
    // For unit testing, we verify the function signature compiles
    // The alias would hide the very signature this probe exists to pin.
    #[allow(clippy::type_complexity)]
    let _sig: fn(&str, &[u8]) -> Result<String, Box<dyn std::error::Error>> =
        registry::call_tool_binary;
}

/// The six tree-walk interpreter guests used to answer every NDA call with
/// "Unknown tool" because their binary entry point looked the tool name up in
/// the scope it had just reset, instead of running the source registered through
/// `interp_register_tool`. The fix lives in the guest, so the gate has to be the
/// payload: the same arguments through both encodings must return the same text.
#[test]
fn test_interpreter_guests_binary_matches_json() {
    let _guest = lock_guest_slot();
    use velocity_mcp::registry;

    let plugins_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins");
    registry::load_plugins(&plugins_dir.to_string_lossy());

    for name in [
        "perl_greet",
        "r_greet",
        "php_greet",
        "java_greet",
        "julia_greet",
        "csharp_greet",
    ] {
        let args = serde_json::json!({ "name": "Audit" });
        let json_out = registry::call_tool(name, &args)
            .unwrap_or_else(|e| panic!("{}: JSON path failed: {}", name, e));
        let tlv = build_tlv_object(&[("name", "Audit")]);
        let binary_out = registry::call_tool_binary(name, &tlv)
            .unwrap_or_else(|e| panic!("{}: binary path failed: {}", name, e));
        assert_eq!(
            json_out, binary_out,
            "{}: NDA payload != JSON payload",
            name
        );
    }

    // Payload equality alone is not proof: a guest whose binary entry point
    // fails before executing anything is retried through decoded JSON and
    // returns the same text. The capability report is the positive gate - a
    // recorded fallback flips the tool to the decoded-JSON group - so this
    // asserts the TLV path actually served all six.
    let capabilities = registry::nda_binary_capabilities();
    for name in [
        "perl_greet",
        "r_greet",
        "php_greet",
        "java_greet",
        "julia_greet",
        "csharp_greet",
    ] {
        let entry = capabilities
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("{} missing from the NDA capability report", name));
        assert_eq!(
            entry["binaryExecutor"], true,
            "{} fell back to decoded JSON instead of executing TLV in the guest",
            name
        );
    }
}
/// The binary (NDA) dispatcher resolves a tool name against the plugin registries
/// *before* the built-in match arm, so a plugin that registers a name belonging to a
/// guarded built-in answers that call in-guest and the `shell_exec` blocklist is never
/// consulted on that transport. This test asserts today's behaviour on purpose: it is a
/// pin, so a repair has to flip it deliberately rather than discover it silently.
/// Nothing destructive runs on either path -- both the JSON and binary transports
/// refuse dangerous commands via the built-in tool validation before spawning.
#[test]
fn test_shell_exec_blocklist_is_reached_on_both_transports() {
    let _guest = lock_guest_slot();
    use velocity_mcp::{plugins, registry};

    plugins::register_wasm_tool_source("shell_exec", "r", "set_tool_result(\"shadowed\")")
        .expect("a WASM tool named shell_exec should register in the R runtime");

    let command = "rm -rf /";
    let json_err = registry::call_tool("shell_exec", &serde_json::json!({ "command": command }))
        .expect_err("the JSON path must refuse a destructive command");
    assert!(
        json_err.to_string().contains("dangerous pattern"),
        "the JSON path refused for a different reason: {}",
        json_err
    );

    // #555 FIXED: binary transport now routes built-in tools through native validation
    let tlv = build_tlv_object(&[("command", command)]);
    let binary_err = registry::call_tool_binary("shell_exec", &tlv)
        .expect_err("the binary path must also refuse a destructive command");
    assert!(
        binary_err.to_string().contains("dangerous pattern"),
        "the binary path refused for a different reason: {}",
        binary_err
    );
}

/// interp_core guests hold exactly one program per language: the shared
/// `tool_source` buffer in bench_tools/interp_core/interp.c is overwritten by
/// every registration and the name arguments are voided, and the JSON path
/// re-registers as a side effect of each call. Two tests that touch the same
/// language therefore stomp each other's program mid-run, which is why the
/// full-binary run was flaky in ~3 of 10 attempts while both tests passed
/// alone and in either order under --test-threads=1. This guard reproduces that
/// serialisation inside the binary.
static GUEST_PROGRAM_SLOT: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_guest_slot() -> std::sync::MutexGuard<'static, ()> {
    GUEST_PROGRAM_SLOT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
