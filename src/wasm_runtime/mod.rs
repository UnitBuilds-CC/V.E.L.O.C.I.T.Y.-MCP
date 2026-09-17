//! WASM runtime abstraction for cross-language tool execution.
//!
//! Each language interpreter (QuickJS, MicroPython, Lua, etc.) is compiled to WASM
//! and runs in-process via Wasmer. The `WasmRuntime` trait provides a uniform interface
//! for registering and calling tools written in any supported language.

pub mod csharp;
pub mod go;
pub mod java;
pub mod julia;
pub mod lua;
pub mod micropython;
pub mod perl;
pub mod php;
pub mod quickjs;
pub mod r;
pub mod ruby;
pub mod rust;
pub mod typescript;
pub mod wasi;
#[cfg(feature = "wasm-networking")]
pub mod wasi_net;

use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, LazyLock, Mutex};
use wasmer::{CompilerConfig, FunctionEnv, Instance, Memory, Module, Store};

use crate::wasm_runtime::wasi::WasiEnv;

/// Shared memory layout constants for WASM runtimes.
/// Most runtimes use this standard layout; QuickJS has its own extended layout.
pub mod memory_layout {
    pub const EXEC_SLOT: u64 = 512 * 1024; // 512KB - for source code / args
    pub const ARGS_SLOT: u64 = 4 * 1024; // 4KB - for tool args JSON
    pub const NAME_SLOT: u64 = 8 * 1024; // 8KB - for tool name
}

/// Global WASM module compilation cache.
/// Maps WASM bytecode hash to cached Module for 20x faster cold starts.
static MODULE_CACHE: LazyLock<Mutex<HashMap<u64, Arc<Vec<u8>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Global WASM instruction limit (0 = metering disabled). Mirrors
/// `WasmRuntimesConfig.instruction_limit`; set once at startup from config.
static WASM_INSTRUCTION_LIMIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10_000_000);

/// Set the global instruction limit applied to every WASM runtime engine.
/// `None` disables metering entirely.
pub fn set_instruction_limit(limit: Option<u64>) {
    let value = limit.unwrap_or(0);
    WASM_INSTRUCTION_LIMIT.store(value, std::sync::atomic::Ordering::Relaxed);
}

/// Current global instruction limit, if metering is enabled.
pub fn instruction_limit() -> Option<u64> {
    match WASM_INSTRUCTION_LIMIT.load(std::sync::atomic::Ordering::Relaxed) {
        0 => None,
        n => Some(n),
    }
}

/// Build a Wasmer engine with optional metering middleware.
///
/// Metering must be applied at compile time; the limit is baked into the
/// compiled module, so it must be set before any runtime is created.
pub fn build_metered_engine(limit: Option<u64>) -> wasmer::Engine {
    let mut cranelift = wasmer::Cranelift::default();
    if let Some(limit) = limit {
        let metering = wasmer_middlewares::metering::Metering::new(limit, |_operator| 1);
        cranelift.push_middleware(std::sync::Arc::new(metering));
    }
    wasmer::Engine::from(cranelift)
}

/// Reset the metering budget on a store/instance pair to the global limit.
///
/// Wasmer metering budgets deplete cumulatively across calls on the same
/// instance; without a reset, a long-lived server would eventually exhaust
/// the budget for its cached runtimes. Call before each user-code execution.
pub(crate) fn reset_instruction_budget(store: &mut Store, instance: &Instance) {
    if let Some(limit) = instruction_limit() {
        wasmer_middlewares::metering::set_remaining_points(store, instance, limit);
    }
}

/// Configuration for building a WASM runtime instance.
pub struct WasmRuntimeConfig<'a> {
    /// WASM module bytes
    pub wasm_bytes: &'a [u8],
    /// Function to build WASI imports (varies by runtime)
    pub import_builder: Box<dyn FnOnce(&mut Store, &FunctionEnv<WasiEnv>) -> wasmer::Imports>,
    /// Additional memory pages needed beyond EXEC_SLOT (default: 64KB)
    pub extra_memory_pages: u32,
    /// Optional instruction limit for metering (None = unlimited)
    pub instruction_limit: Option<u64>,
}

/// Helper function to create a new WASM runtime instance with common bootstrap logic.
/// Eliminates duplication across all 12 runtime implementations.
/// Cache key for MODULE_CACHE. Must include the instruction limit: metering
/// is baked into the compiled module, so a cached module from a different
/// limit would silently disable (or misapply) metering on reuse.
fn module_cache_key(wasm_bytes: &[u8], instruction_limit: Option<u64>) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    wasm_bytes.hash(&mut hasher);
    instruction_limit.hash(&mut hasher);
    hasher.finish()
}

pub fn create_wasm_instance(
    config: WasmRuntimeConfig,
) -> Result<(Store, Instance, Memory, FunctionEnv<WasiEnv>), Box<dyn Error>> {
    let engine = build_metered_engine(config.instruction_limit);

    // Check module cache first for faster cold starts
    let wasm_hash = module_cache_key(config.wasm_bytes, config.instruction_limit);

    let module = {
        let cache = MODULE_CACHE
            .lock()
            .map_err(|e| format!("Cache lock poisoned: {}", e))?;
        if let Some(cached_bytes) = cache.get(&wasm_hash) {
            // Deserialize cached module from bytes (faster than recompilation)
            unsafe { Module::deserialize(&engine, cached_bytes.as_slice())? }
        } else {
            drop(cache);
            // Compile and cache the module
            let module = Module::new(&engine, config.wasm_bytes)?;
            let serialized = module.serialize()?;
            MODULE_CACHE
                .lock()
                .map_err(|e| format!("Cache lock poisoned: {}", e))?
                .insert(wasm_hash, Arc::new(serialized.to_vec()));
            module
        }
    };

    let mut store = Store::new(engine);

    let env = FunctionEnv::new(&mut store, WasiEnv::new());
    let imports = (config.import_builder)(&mut store, &env);
    let instance = Instance::new(&mut store, &module, &imports)?;

    let memory = instance.exports.get_memory("memory")?.clone();
    env.as_mut(&mut store).memory = Some(memory.clone());

    // Calculate and grow memory if needed
    let current_pages = memory.view(&store).size();
    let exec_slot = memory_layout::EXEC_SLOT;
    let extra_bytes = config.extra_memory_pages as u64 * 65536;
    let needed_pages = ((exec_slot + extra_bytes) / 65536 + 1) as u32;
    if current_pages.0 < needed_pages {
        memory.grow(&mut store, wasmer::Pages(needed_pages - current_pages.0))?;
    }

    Ok((store, instance, memory, env))
}

/// Compile a WASM module with cache lookup. Used by runtimes that manage
/// their own Store/Engine (Rust, Go) rather than going through create_wasm_instance.
pub fn compile_module_cached(
    engine: &wasmer::Engine,
    wasm_bytes: &[u8],
    instruction_limit: Option<u64>,
) -> Result<Module, Box<dyn Error>> {
    let wasm_hash = module_cache_key(wasm_bytes, instruction_limit);

    let cache = MODULE_CACHE
        .lock()
        .map_err(|e| format!("Cache lock poisoned: {}", e))?;
    if let Some(cached_bytes) = cache.get(&wasm_hash) {
        return unsafe { Ok(Module::deserialize(engine, cached_bytes.as_slice())?) };
    }
    drop(cache);

    let module = Module::new(engine, wasm_bytes)?;
    let serialized = module.serialize()?;
    MODULE_CACHE
        .lock()
        .map_err(|e| format!("Cache lock poisoned: {}", e))?
        .insert(wasm_hash, Arc::new(serialized.to_vec()));
    Ok(module)
}

/// Helper for cold start benchmarking - eliminates duplication across runtimes.
pub fn bench_cold_start_generic(
    wasm_bytes: &[u8],
    mut import_builder: impl FnMut(&mut Store, &FunctionEnv<WasiEnv>) -> wasmer::Imports,
    init_fn_name: &str,
    init_args: Option<(i32, i32)>, // For runtimes like MicroPython that need (pystack, heap)
) -> f64 {
    let cold_iters = 20;
    let start = std::time::Instant::now();
    for _ in 0..cold_iters {
        let engine = wasmer::Engine::from(wasmer::Cranelift::default());
        let module = Module::new(&engine, wasm_bytes).unwrap();
        let mut store = Store::new(engine);
        let env = FunctionEnv::new(&mut store, WasiEnv::new());
        let imports = import_builder(&mut store, &env);
        let instance = Instance::new(&mut store, &module, &imports).unwrap();
        let memory = instance.exports.get_memory("memory").unwrap().clone();
        env.as_mut(&mut store).memory = Some(memory);

        let init_fn = instance.exports.get_function(init_fn_name).unwrap();

        if let Some((arg1, arg2)) = init_args {
            let typed = init_fn.typed::<(i32, i32), i32>(&store).unwrap();
            let _ = typed.call(&mut store, arg1, arg2).unwrap();
        } else {
            let typed = init_fn.typed::<(), i32>(&store).unwrap();
            let _ = typed.call(&mut store).unwrap();
        }
    }
    start.elapsed().as_nanos() as f64 / cold_iters as f64 / 1_000_000.0
}

/// Helper for executing source code and reading output from WASM memory.
/// Eliminates duplication of exec()/get_output() pairs across runtimes.
pub struct WasmExecHelper<'a> {
    store: &'a mut Store,
    instance: &'a Instance,
    memory: &'a Memory,
}

impl<'a> WasmExecHelper<'a> {
    pub fn new(store: &'a mut Store, instance: &'a Instance, memory: &'a Memory) -> Self {
        Self {
            store,
            instance,
            memory,
        }
    }

    /// Execute source code at EXEC_SLOT using the named export function.
    pub fn exec(&mut self, src: &str, exec_fn_name: &str) -> Result<i32, Box<dyn Error>> {
        use memory_layout::EXEC_SLOT;
        let data = src.as_bytes();
        self.memory.view(self.store).write(EXEC_SLOT, data)?;
        let exec_fn = self.instance.exports.get_function(exec_fn_name)?;
        let result = exec_fn.call(
            self.store,
            &[
                wasmer::Value::I32(EXEC_SLOT as i32),
                wasmer::Value::I32(data.len() as i32),
            ],
        )?;
        Ok(result[0].unwrap_i32())
    }

    /// Read output from WASM memory using get_output and get_output_len functions.
    pub fn get_output(
        &mut self,
        get_output_fn_name: &str,
        get_len_fn_name: &str,
    ) -> Result<String, Box<dyn Error>> {
        let get_output_fn = self.instance.exports.get_function(get_output_fn_name)?;
        let get_len_fn = self.instance.exports.get_function(get_len_fn_name)?;

        let out_ptr = get_output_fn.call(self.store, &[])?[0].unwrap_i32();
        let out_len = get_len_fn.call(self.store, &[])?[0].unwrap_i32();

        if out_ptr == 0 || out_len == 0 {
            return Ok(String::new());
        }

        let mut buf = vec![0u8; out_len as usize];
        self.memory
            .view(self.store)
            .read(out_ptr as u64, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    /// Convenience: execute and read output in one call.
    pub fn exec_and_get_output(
        &mut self,
        src: &str,
        exec_fn_name: &str,
        get_output_fn_name: &str,
        get_len_fn_name: &str,
    ) -> Result<String, Box<dyn Error>> {
        let rc = self.exec(src, exec_fn_name)?;
        let output = self.get_output(get_output_fn_name, get_len_fn_name)?;
        if rc != 0 {
            return Err(format!("WASM execution error (rc={}): {}", rc, output.trim()).into());
        }
        Ok(output)
    }
}

/// Uniform interface for language runtimes compiled to WASM.
///
/// Each implementation wraps a specific interpreter (QuickJS, MicroPython, etc.)
/// compiled to a WASM module and running inside Wasmer.
pub trait WasmRuntime: Send {
    /// Initialize the interpreter runtime. Called once after WASM instantiation.
    fn init(&mut self) -> Result<(), Box<dyn Error>>;

    /// Register a tool with the given name and source code.
    /// The source is evaluated/compiled in the interpreter context.
    fn register_tool(&mut self, name: &str, source: &str) -> Result<(), Box<dyn Error>>;

    /// Call a registered tool with JSON arguments. Returns JSON result string.
    fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>>;

    /// Call a registered tool with binary TLV arguments (NDA format).
    /// Default implementation falls back to JSON serialization for backwards compatibility.
    /// Override in specific runtimes for zero-allocation TLV decoding.
    fn call_tool_binary(&mut self, name: &str, args_tlv: &[u8]) -> Result<String, Box<dyn Error>> {
        // Fallback: decode TLV to Value, serialize to JSON, call regular path
        use crate::protocol::nda_native::decode_json_value;
        let (value, _) = decode_json_value(args_tlv)?;
        let json_str = serde_json::to_string(&value)?;
        self.call_tool(name, &json_str)
    }

    /// Destroy the runtime, freeing interpreter resources.
    fn destroy(&mut self) -> Result<(), Box<dyn Error>>;

    /// Reset the metering instruction budget before a tool invocation.
    /// Default no-op; implementations with persistent instances override this
    /// so the global instruction limit applies per call, not per process.
    fn reset_instruction_budget(&mut self) {}

    /// Language identifier (e.g., "javascript", "python", "lua").
    fn language(&self) -> &str;
}

/// Metadata about a registered tool.
#[derive(Debug, Clone)]
pub struct WasmToolMeta {
    /// Tool name
    pub name: String,
    /// Language runtime that hosts this tool
    pub language: String,
    /// JSON Schema for tool input (provided at registration)
    pub input_schema: serde_json::Value,
    /// Tool description
    pub description: String,
}

/// Registry of WASM-based language runtimes and their tools.
///
/// Manages the lifecycle of interpreter instances and routes tool calls
/// to the appropriate language runtime.
pub struct WasmRuntimeRegistry {
    runtimes: HashMap<String, RuntimeEntry>,
    tools: HashMap<String, WasmToolMeta>,
}

struct RuntimeEntry {
    runtime: Box<dyn WasmRuntime>,
    tool_names: Vec<String>,
}

impl WasmRuntimeRegistry {
    pub fn new() -> Self {
        Self {
            runtimes: HashMap::new(),
            tools: HashMap::new(),
        }
    }

    /// Register a language runtime.
    pub fn register_runtime(
        &mut self,
        mut runtime: Box<dyn WasmRuntime>,
    ) -> Result<(), Box<dyn Error>> {
        let lang = runtime.language().to_string();
        runtime.init()?;
        self.runtimes.insert(
            lang.clone(),
            RuntimeEntry {
                runtime,
                tool_names: Vec::new(),
            },
        );
        Ok(())
    }

    /// Register a tool in the appropriate language runtime.
    pub fn register_tool(
        &mut self,
        language: &str,
        name: &str,
        source: &str,
        description: &str,
        input_schema: serde_json::Value,
    ) -> Result<(), Box<dyn Error>> {
        let entry = self
            .runtimes
            .get_mut(language)
            .ok_or_else(|| format!("No runtime registered for language: {}", language))?;
        entry.runtime.register_tool(name, source)?;
        entry.tool_names.push(name.to_string());
        self.tools.insert(
            name.to_string(),
            WasmToolMeta {
                name: name.to_string(),
                language: language.to_string(),
                input_schema,
                description: description.to_string(),
            },
        );
        Ok(())
    }

    /// Call a tool by name. Routes to the correct language runtime.
    pub fn call_tool(&mut self, name: &str, args_json: &str) -> Result<String, Box<dyn Error>> {
        let meta = self
            .tools
            .get(name)
            .ok_or_else(|| format!("Unknown WASM tool: {}", name))?;
        let lang = meta.language.clone();
        let entry = self
            .runtimes
            .get_mut(&lang)
            .ok_or_else(|| format!("Runtime missing for language: {}", lang))?;
        entry.runtime.reset_instruction_budget();
        entry.runtime.call_tool(name, args_json)
    }

    /// Get all registered WASM tools.
    pub fn get_tools(&self) -> Vec<&WasmToolMeta> {
        self.tools.values().collect()
    }

    /// Check if a tool name is registered as a WASM tool.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Destroy a specific runtime.
    pub fn destroy_runtime(&mut self, language: &str) -> Result<(), Box<dyn Error>> {
        if let Some(mut entry) = self.runtimes.remove(language) {
            for name in &entry.tool_names {
                self.tools.remove(name);
            }
            entry.runtime.destroy()?;
        }
        Ok(())
    }
}

impl Default for WasmRuntimeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Factory function to create a WASM runtime for a given language.
///
/// Reads the WASM module from disk and initializes the appropriate interpreter runtime.
/// Supports all 12 registered language runtimes.
pub fn create_wasm_runtime_for_language(
    language: &str,
    wasm_path: &str,
) -> Result<Box<dyn WasmRuntime>, Box<dyn Error>> {
    let wasm_bytes = std::fs::read(wasm_path)
        .map_err(|e| format!("Failed to read WASM module '{}': {}", wasm_path, e))?;

    let mut runtime: Box<dyn WasmRuntime> = match language {
        "javascript" => Box::new(
            crate::wasm_runtime::quickjs::QuickJsRuntime::new(&wasm_bytes)
                .map_err(|e| format!("QuickJS init failed: {}", e))?,
        ),
        "typescript" => Box::new(
            crate::wasm_runtime::typescript::TypeScriptRuntime::new(&wasm_bytes)
                .map_err(|e| format!("TypeScript init failed: {}", e))?,
        ),
        "python" => Box::new(
            crate::wasm_runtime::micropython::MicroPythonRuntime::new(&wasm_bytes)
                .map_err(|e| format!("MicroPython init failed: {}", e))?,
        ),
        "lua" => Box::new(
            crate::wasm_runtime::lua::LuaRuntime::new(&wasm_bytes)
                .map_err(|e| format!("Lua init failed: {}", e))?,
        ),
        "ruby" => Box::new(
            crate::wasm_runtime::ruby::RubyRuntime::new(&wasm_bytes)
                .map_err(|e| format!("Ruby init failed: {}", e))?,
        ),
        "rust" => Box::new(
            crate::wasm_runtime::rust::RustRuntime::new(&wasm_bytes)
                .map_err(|e| format!("Rust init failed: {}", e))?,
        ),
        "php" => Box::new(
            crate::wasm_runtime::php::PhpRuntime::new(&wasm_bytes)
                .map_err(|e| format!("PHP init failed: {}", e))?,
        ),
        "csharp" => Box::new(
            crate::wasm_runtime::csharp::CSharpRuntime::new(&wasm_bytes)
                .map_err(|e| format!("C# init failed: {}", e))?,
        ),
        "java" => Box::new(
            crate::wasm_runtime::java::JavaRuntime::new(&wasm_bytes)
                .map_err(|e| format!("Java init failed: {}", e))?,
        ),
        "r" => Box::new(
            crate::wasm_runtime::r::RRuntime::new(&wasm_bytes)
                .map_err(|e| format!("R init failed: {}", e))?,
        ),
        "julia" => Box::new(
            crate::wasm_runtime::julia::JuliaRuntime::new(&wasm_bytes)
                .map_err(|e| format!("Julia init failed: {}", e))?,
        ),
        "perl" => Box::new(
            crate::wasm_runtime::perl::PerlRuntime::new(&wasm_bytes)
                .map_err(|e| format!("Perl init failed: {}", e))?,
        ),
        "go" => Box::new(crate::wasm_runtime::go::GoWasmRuntime::new(wasm_path)),
        _ => return Err(format!("Unsupported WASM language: {}", language).into()),
    };

    runtime.init()?;
    Ok(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmer::{imports, Value};
    use wasmer_middlewares::metering::{get_remaining_points, MeteringPoints};

    /// Metering must trap infinite loops instead of hanging the host. The
    /// trap text matters: plugins/mod.rs classifies limit errors by message,
    /// and Wasmer 5 surfaces exhaustion as a plain "unreachable" trap.
    #[test]
    fn test_metering_traps_on_budget_exhaustion() {
        let engine = build_metered_engine(Some(1_000));
        let module = Module::new(
            &engine,
            r#"(module (func (export "spin") (loop (br 0))))"#,
        )
        .expect("WAT module should compile");
        let mut store = Store::new(engine);
        let instance = Instance::new(&mut store, &module, &imports! {}).expect("instantiate");
        let spin = instance.exports.get_function("spin").expect("spin export");

        let err = spin.call(&mut store, &[]).expect_err("infinite loop must trap under metering");
        let msg = err.to_string();
        assert!(
            msg.contains("unreachable"),
            "expected metering exhaustion to surface as 'unreachable', got: {}",
            msg
        );
    }

    /// Budgets deplete cumulatively across calls on one instance, so a cached
    /// long-lived runtime would eventually brick without the per-call reset.
    #[test]
    fn test_metering_budget_resets_between_calls() {
        // ~15 metered operators per loop iteration; n=50 ≈ 750+ ops per call.
        let engine = build_metered_engine(Some(1_000));
        let module = Module::new(
            &engine,
            r#"(module
              (func (export "work") (param $n i32) (result i32)
                (local $acc i32)
                (loop $l
                  local.get $n
                  i32.eqz
                  if
                    local.get $acc
                    return
                  end
                  local.get $acc
                  local.get $n
                  i32.add
                  local.set $acc
                  local.get $n
                  i32.const 1
                  i32.sub
                  local.set $n
                  br $l
                )
                local.get $acc))"#,
        )
        .expect("WAT module should compile");
        let mut store = Store::new(engine);
        let instance = Instance::new(&mut store, &module, &imports! {}).expect("instantiate");
        let work = instance.exports.get_function("work").expect("work export");

        let mut ok_calls = 0u32;
        let trap = loop {
            match work.call(&mut store, &[Value::I32(50)]) {
                Ok(_) => {
                    ok_calls += 1;
                    assert!(
                        ok_calls < 10,
                        "metering budget should deplete cumulatively across calls"
                    );
                }
                Err(trap) => break trap,
            }
        };
        assert!(ok_calls >= 1, "a call within the budget must succeed");
        assert!(
            trap.to_string().contains("unreachable"),
            "unexpected trap message: {}",
            trap
        );
        assert!(
            matches!(
                get_remaining_points(&mut store, &instance),
                MeteringPoints::Exhausted
            ),
            "instance should report an exhausted budget after the trap"
        );

        // The production reset path (global limit is the 10M default in tests,
        // well above consumption) must restore execution on this instance.
        reset_instruction_budget(&mut store, &instance);
        assert!(matches!(
            get_remaining_points(&mut store, &instance),
            MeteringPoints::Remaining(_)
        ));
        let result = work
            .call(&mut store, &[Value::I32(50)])
            .expect("call must succeed after budget reset");
        assert_eq!(result[0].unwrap_i32(), 1275); // sum(1..=50)
    }

    /// The cache key must distinguish modules compiled under different metering
    /// limits — metering is baked in at compile time, so reusing a cached module
    /// across limits would silently misapply (or drop) metering.
    #[test]
    fn test_module_cache_key_separates_instruction_limits() {
        let bytes_a = b"wasm module bytes a";
        let bytes_b = b"wasm module bytes b";

        assert_ne!(
            module_cache_key(bytes_a, None),
            module_cache_key(bytes_a, Some(1_000_000)),
            "None (metering off) and a finite limit must not share a cache entry"
        );
        assert_ne!(
            module_cache_key(bytes_a, Some(1_000_000)),
            module_cache_key(bytes_a, Some(10_000_000)),
            "different limits must not share a cache entry"
        );
        assert_eq!(
            module_cache_key(bytes_a, Some(1_000_000)),
            module_cache_key(bytes_a, Some(1_000_000)),
            "same inputs must yield a stable key"
        );
        assert_ne!(
            module_cache_key(bytes_a, None),
            module_cache_key(bytes_b, None),
            "different bytecode must not share a cache entry"
        );
    }
}
