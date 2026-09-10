use wasmer::{Engine, Module, Store};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <wasm_file>", args[0]);
        std::process::exit(1);
    }

    let wasm_path = &args[1];
    let wasm_bytes = std::fs::read(wasm_path).expect("Failed to read WASM file");

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("Failed to compile WASM module");

    println!("=== EXPORTS ===");
    for export in module.exports() {
        println!("  {} ({:?})", export.name(), export.ty());
    }

    println!("\n=== IMPORTS ===");
    for import in module.imports() {
        println!("  {}.{} ({:?})", import.module(), import.name(), import.ty());
    }
}
