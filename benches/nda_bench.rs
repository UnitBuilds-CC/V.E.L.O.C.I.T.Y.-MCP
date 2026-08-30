use criterion::{black_box, criterion_group, criterion_main, Criterion};
use velocity_mcp::nda_document::{NdaCompiler, NdaDocument};

fn benchmark_nda_compile_empty(c: &mut Criterion) {
    c.bench_function("nda_compile_empty", |b| {
        b.iter(|| {
            let compiler = NdaCompiler::new();
            black_box(compiler.compile())
        })
    });
}

fn benchmark_nda_compile_with_triples(c: &mut Criterion) {
    c.bench_function("nda_compile_with_triples", |b| {
        b.iter(|| {
            let mut compiler = NdaCompiler::new();
            compiler.add_triple("subject1", "predicate1", "object1");
            compiler.add_triple("subject2", "predicate2", "object2");
            compiler.add_triple("subject3", "predicate3", "object3");
            black_box(compiler.compile())
        })
    });
}

fn benchmark_nda_read(c: &mut Criterion) {
    let compiler = NdaCompiler::new();
    let compiled = compiler.compile();
    
    c.bench_function("nda_read", |b| {
        b.iter(|| {
            black_box(NdaDocument::read(&compiled))
        })
    });
}

fn benchmark_nda_verify_merkle(c: &mut Criterion) {
    let mut compiler = NdaCompiler::new();
    compiler.add_triple("subject", "predicate", "object");
    let compiled = compiler.compile();
    let parsed = NdaDocument::read(&compiled).unwrap();
    
    c.bench_function("nda_verify_merkle", |b| {
        b.iter(|| {
            black_box(parsed.verify_merkle())
        })
    });
}

criterion_group! {
    name = nda_benches;
    config = Criterion::default().sample_size(100);
    targets = benchmark_nda_compile_empty,
              benchmark_nda_compile_with_triples,
              benchmark_nda_read,
              benchmark_nda_verify_merkle
}

criterion_main!(nda_benches);
