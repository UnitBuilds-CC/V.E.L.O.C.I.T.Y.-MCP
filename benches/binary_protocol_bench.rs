//! Binary Protocol Benchmark Suite
//!
//! Measures the performance of TLV binary argument protocol vs JSON serialization.
//! Compares encoding/decoding latency, memory allocation, and end-to-end tool call overhead.

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use velocity_mcp::protocol::nda_native;

/// Build TLV object from key-value pairs
fn build_tlv_object(fields: &[(&str, &str)]) -> Vec<u8> {
    let mut tlv = Vec::new();
    
    tlv.push(0x06); // Object tag
    tlv.extend_from_slice(&(fields.len() as u32).to_be_bytes());
    
    for (key, value) in fields {
        tlv.extend_from_slice(&(key.len() as u16).to_be_bytes());
        tlv.extend_from_slice(key.as_bytes());
        
        tlv.push(0x01); // String tag
        tlv.extend_from_slice(&(value.len() as u32).to_be_bytes());
        tlv.extend_from_slice(value.as_bytes());
    }
    
    tlv
}

/// Benchmark TLV encoding small object (2-3 fields)
fn bench_tlv_encode_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_encode_small");
    group.throughput(Throughput::Elements(1));
    
    let fields = vec![("name", "Alice"), ("age", "30")];
    
    group.bench_function("encode_2_fields", |b| {
        b.iter(|| {
            let tlv = build_tlv_object(&fields);
            criterion::black_box(tlv);
        });
    });
    
    group.finish();
}

/// Benchmark TLV encoding medium object (10 fields)
fn bench_tlv_encode_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_encode_medium");
    group.throughput(Throughput::Elements(1));
    
    let fields = vec![
        ("field1", "value1"),
        ("field2", "value2"),
        ("field3", "value3"),
        ("field4", "value4"),
        ("field5", "value5"),
        ("field6", "value6"),
        ("field7", "value7"),
        ("field8", "value8"),
        ("field9", "value9"),
        ("field10", "value10"),
    ];
    
    group.bench_function("encode_10_fields", |b| {
        b.iter(|| {
            let tlv = build_tlv_object(&fields);
            criterion::black_box(tlv);
        });
    });
    
    group.finish();
}

/// Benchmark TLV decoding small object
fn bench_tlv_decode_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_decode_small");
    group.throughput(Throughput::Elements(1));
    
    let fields = vec![("name", "Alice"), ("age", "30")];
    let tlv = build_tlv_object(&fields);
    
    group.bench_function("decode_2_fields", |b| {
        b.iter(|| {
            let (value, _) = nda_native::decode_json_value(&tlv).unwrap();
            criterion::black_box(value);
        });
    });
    
    group.finish();
}

/// Benchmark TLV decoding medium object
fn bench_tlv_decode_medium(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_decode_medium");
    group.throughput(Throughput::Elements(1));
    
    let fields = vec![
        ("f1", "v1"), ("f2", "v2"), ("f3", "v3"), ("f4", "v4"), ("f5", "v5"),
        ("f6", "v6"), ("f7", "v7"), ("f8", "v8"), ("f9", "v9"), ("f10", "v10"),
    ];
    let tlv = build_tlv_object(&fields);
    
    group.bench_function("decode_10_fields", |b| {
        b.iter(|| {
            let (value, _) = nda_native::decode_json_value(&tlv).unwrap();
            criterion::black_box(value);
        });
    });
    
    group.finish();
}

/// Benchmark JSON serialization for comparison
fn bench_json_serialize_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_serialize_small");
    group.throughput(Throughput::Elements(1));
    
    let data = serde_json::json!({
        "name": "Alice",
        "age": 30
    });
    
    group.bench_function("serialize_2_fields", |b| {
        b.iter(|| {
            let json = serde_json::to_string(&data).unwrap();
            criterion::black_box(json);
        });
    });
    
    group.finish();
}

/// Benchmark JSON deserialization for comparison
fn bench_json_deserialize_small(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_deserialize_small");
    group.throughput(Throughput::Elements(1));
    
    let json_str = r#"{"name":"Alice","age":30}"#;
    
    group.bench_function("deserialize_2_fields", |b| {
        b.iter(|| {
            let value: serde_json::Value = serde_json::from_str(json_str).unwrap();
            criterion::black_box(value);
        });
    });
    
    group.finish();
}

/// Benchmark round-trip: TLV encode + decode
fn bench_tlv_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_roundtrip");
    group.throughput(Throughput::Elements(1));
    
    let fields = vec![
        ("tool_name", "greet_user"),
        ("language", "php"),
        ("user_id", "12345"),
    ];
    
    group.bench_function("encode_decode_3_fields", |b| {
        b.iter(|| {
            let tlv = build_tlv_object(&fields);
            let (decoded, _) = nda_native::decode_json_value(&tlv).unwrap();
            criterion::black_box(decoded);
        });
    });
    
    group.finish();
}

/// Benchmark round-trip: JSON serialize + deserialize
fn bench_json_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_roundtrip");
    group.throughput(Throughput::Elements(1));
    
    let data = serde_json::json!({
        "tool_name": "greet_user",
        "language": "php",
        "user_id": "12345"
    });
    
    group.bench_function("serialize_deserialize_3_fields", |b| {
        b.iter(|| {
            let json = serde_json::to_string(&data).unwrap();
            let decoded: serde_json::Value = serde_json::from_str(&json).unwrap();
            criterion::black_box(decoded);
        });
    });
    
    group.finish();
}

/// Benchmark nested object TLV encoding
fn bench_tlv_nested_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_nested_encode");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("nested_2_levels", |b| {
        b.iter(|| {
            let mut tlv = Vec::new();
            
            // Outer object
            tlv.push(0x06);
            tlv.extend_from_slice(&1u32.to_be_bytes());
            tlv.extend_from_slice(&6u16.to_be_bytes());
            tlv.extend(b"config");
            
            // Inner object
            tlv.push(0x06);
            tlv.extend_from_slice(&2u32.to_be_bytes());
            
            tlv.extend_from_slice(&4u16.to_be_bytes());
            tlv.extend(b"mode");
            tlv.push(0x01);
            tlv.extend_from_slice(&5u32.to_be_bytes());
            tlv.extend(b"debug");
            
            tlv.extend_from_slice(&7u16.to_be_bytes());
            tlv.extend(b"verbose");
            tlv.push(0x03);
            tlv.push(0x01);
            
            criterion::black_box(tlv);
        });
    });
    
    group.finish();
}

/// Benchmark array TLV encoding
fn bench_tlv_array_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_array_encode");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("array_10_elements", |b| {
        b.iter(|| {
            let mut tlv = Vec::new();
            
            tlv.push(0x05); // Array
            tlv.extend_from_slice(&10u32.to_be_bytes());
            
            for i in 0..10 {
                tlv.push(0x02); // Integer
                tlv.extend_from_slice(&(i as i64).to_be_bytes());
            }
            
            criterion::black_box(tlv);
        });
    });
    
    group.finish();
}

/// Benchmark large payload TLV (simulating real tool args)
fn bench_tlv_large_payload(c: &mut Criterion) {
    let mut group = c.benchmark_group("tlv_large_payload");
    group.throughput(Throughput::Bytes(1024));
    
    // Simulate a realistic tool call with ~1KB payload
    let mut fields: Vec<(&str, &str)> = Vec::new();
    for i in 0..20 {
        fields.push((
            Box::leak(format!("param_{:02}", i).into_boxed_str()),
            Box::leak(format!("This is parameter number {} with some descriptive text to make it larger", i).into_boxed_str()),
        ));
    }
    
    group.bench_function("encode_20_params_1kb", |b| {
        b.iter(|| {
            let tlv = build_tlv_object(&fields);
            criterion::black_box(tlv.len());
        });
    });
    
    group.finish();
}

/// Compare TLV vs JSON size overhead
fn bench_size_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("size_comparison");
    
    let fields = vec![
        ("name", "benchmark_test"),
        ("count", "1000"),
        ("enabled", "true"),
        ("ratio", "3.14159"),
    ];
    
    let tlv = build_tlv_object(&fields);
    let json_obj = serde_json::json!({
        "name": "benchmark_test",
        "count": "1000",
        "enabled": "true",
        "ratio": "3.14159"
    });
    let json_str = serde_json::to_string(&json_obj).unwrap();
    
    group.bench_function("tlv_size", |b| {
        b.iter(|| {
            criterion::black_box(tlv.len());
        });
    });
    
    group.bench_function("json_size", |b| {
        b.iter(|| {
            criterion::black_box(json_str.len());
        });
    });
    
    group.finish();
}

criterion_group!(
    name = encode_benches;
    config = Criterion::default().sample_size(1000);
    targets = bench_tlv_encode_small, bench_tlv_encode_medium, bench_tlv_nested_encode, bench_tlv_array_encode, bench_tlv_large_payload
);

criterion_group!(
    name = decode_benches;
    config = Criterion::default().sample_size(1000);
    targets = bench_tlv_decode_small, bench_tlv_decode_medium
);

criterion_group!(
    name = json_comparison;
    config = Criterion::default().sample_size(1000);
    targets = bench_json_serialize_small, bench_json_deserialize_small, bench_size_comparison
);

criterion_group!(
    name = roundtrip_benches;
    config = Criterion::default().sample_size(1000);
    targets = bench_tlv_roundtrip, bench_json_roundtrip
);

criterion_main!(encode_benches, decode_benches, json_comparison, roundtrip_benches);
