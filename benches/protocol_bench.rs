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

fn benchmark_json_rpc_parse_only(c: &mut Criterion) {
    let request_str = r#"{"jsonrpc":"2.0","method":"ping","id":1}"#;

    c.bench_function("json_rpc_parse_only", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(request_str)).unwrap();
        })
    });
}

fn benchmark_json_rpc_dispatch_only(c: &mut Criterion) {
    let request = json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "id": 1
    });

    c.bench_function("json_rpc_dispatch_only", |b| {
        b.iter(|| {
            handle_request(black_box(&request))
        })
    });
}

fn benchmark_registry_dispatch(c: &mut Criterion) {
    use velocity_mcp::registry;

    c.bench_function("registry_dispatch", |b| {
        b.iter(|| {
            let tools = registry::get_tools();
            black_box(tools)
        })
    });
}

fn benchmark_audit_record_throughput(c: &mut Criterion) {
    use velocity_mcp::audit::{global_audit, AuditOutcome};

    let audit = global_audit();

    c.bench_function("audit_record_throughput", |b| {
        b.iter(|| {
            audit.record(black_box("benchmark"), std::time::Instant::now(), AuditOutcome::Success);
        })
    });
}

criterion_group! {
    name = protocol_benches;
    config = Criterion::default().sample_size(100);
    targets = benchmark_json_rpc_initialize,
              benchmark_json_rpc_tools_list,
              benchmark_json_rpc_ping,
              benchmark_json_rpc_health_check,
              benchmark_json_rpc_parse_only,
              benchmark_json_rpc_dispatch_only,
              benchmark_registry_dispatch,
              benchmark_audit_record_throughput
}

criterion_main!(protocol_benches);
