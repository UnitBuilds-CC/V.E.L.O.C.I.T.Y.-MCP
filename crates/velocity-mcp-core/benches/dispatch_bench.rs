use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use serde_json::json;
use velocity_mcp_core::{handle_mcp_request, McpRequest};

fn bench_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("core_dispatch");

    let initialize = json!({
        "jsonrpc": "2.0",
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "bench", "version": "1.0.0"}
        },
        "id": 1
    });

    let ping = json!({
        "jsonrpc": "2.0",
        "method": "ping",
        "id": 2
    });

    let tools_list = json!({
        "jsonrpc": "2.0",
        "method": "tools/list",
        "params": {},
        "id": 3
    });

    let tools_call = json!({
        "jsonrpc": "2.0",
        "method": "tools/call",
        "params": {
            "name": "echo",
            "arguments": {"message": "benchmark payload"}
        },
        "id": 4
    });

    let resources_read = json!({
        "jsonrpc": "2.0",
        "method": "resources/read",
        "params": {"uri": "file:///test.txt"},
        "id": 5
    });

    let completion = json!({
        "jsonrpc": "2.0",
        "method": "completion/complete",
        "params": {
            "ref": {"type": "ref/prompt", "name": "test"},
            "argument": {"name": "arg", "value": "val"}
        },
        "id": 6
    });

    let requests = [
        ("initialize", initialize),
        ("ping", ping),
        ("tools_list", tools_list),
        ("tools_call_stub", tools_call),
        ("resources_read", resources_read),
        ("completion_complete", completion),
    ];

    for (name, request_json) in &requests {
        let request_str = serde_json::to_string(request_json).unwrap();
        let parsed: McpRequest = serde_json::from_value(request_json.clone()).unwrap();
        group.throughput(Throughput::Bytes(request_str.len() as u64));
        group.bench_with_input(BenchmarkId::new("dispatch", name), &parsed, |b, req| {
            b.iter(|| handle_mcp_request(black_box(req)))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_dispatch);
criterion_main!(benches);
