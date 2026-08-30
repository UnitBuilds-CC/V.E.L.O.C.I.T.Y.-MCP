use criterion::{black_box, criterion_group, criterion_main, Criterion};
use serde_json::json;
use velocity_mcp::protocol::json_rpc::handle_request;

fn benchmark_json_rpc_initialize(c: &mut Criterion) {
    let request = json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "benchmark-client",
                "version": "1.0.0"
            }
        },
        "id": 1
    });

    c.bench_function("json_rpc_initialize", |b| {
        b.iter(|| {
            handle_request(black_box(&request))
        })
    });
}

fn benchmark_json_rpc_tools_list(c: &mut Criterion) {
    let request = json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "params": {},
        "id": 1
    });

    c.bench_function("json_rpc_tools_list", |b| {
        b.iter(|| {
            handle_request(black_box(&request))
        })
    });
}

fn benchmark_json_rpc_ping(c: &mut Criterion) {
    let request = json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "id": 1
    });

    c.bench_function("json_rpc_ping", |b| {
        b.iter(|| {
            handle_request(black_box(&request))
        })
    });
}

fn benchmark_json_rpc_health_check(c: &mut Criterion) {
    let request = json!({
        "jsonrpc": "2.0",
        "method": "health/check",
        "id": 1
    });

    c.bench_function("json_rpc_health_check", |b| {
        b.iter(|| {
            handle_request(black_box(&request))
        })
    });
}

criterion_group! {
    name = protocol_benches;
    config = Criterion::default().sample_size(100);
    targets = benchmark_json_rpc_initialize,
              benchmark_json_rpc_tools_list,
              benchmark_json_rpc_ping,
              benchmark_json_rpc_health_check
}

criterion_main!(protocol_benches);
