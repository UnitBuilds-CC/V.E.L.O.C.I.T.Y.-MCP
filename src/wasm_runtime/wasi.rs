//! Shared WASI import builder for WASM runtimes.
//!
//! Provides a minimal `wasi_snapshot_preview1` implementation sufficient for
//! interpreter WASM modules (QuickJS, MicroPython, Lua, etc.). Also provides
//! QuickJS-specific `env` imports.

use wasmer::{Function, FunctionEnv, FunctionEnvMut, Imports, Memory, Store};
use wasmer::imports;

/// Environment shared between host WASI functions and the WASM guest.
/// Holds a reference to the guest's linear memory, set after instantiation.
#[derive(Clone)]
pub struct WasiEnv {
    pub memory: Option<Memory>,
}

/// Build WASI + env imports for QuickJS-based runtimes.
///
/// Provides:
/// - `wasi_snapshot_preview1`: clock_time_get, fd_write, fd_close, fd_fdstat_get, fd_seek, random_get
/// - `env`: host_get_timezone_offset, host_interrupt, host_promise_rejection,
///          host_module_normalize, host_module_load, host_call
pub fn build_quickjs_imports(store: &mut Store, env: &FunctionEnv<WasiEnv>) -> Imports {
    let clock_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _clock_id: i32, _precision: i64, result_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            mem.view(&env).write(result_ptr as u64, &ns.to_le_bytes()).unwrap();
            0
        });

    let fd_write_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _fd: i32, _iovs_ptr: i32, _iovs_len: i32, nwritten_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            mem.view(&env).write(nwritten_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });

    let fd_close_fn = Function::new_typed(store, |_fd: i32| -> i32 { 52 });

    let fd_fdstat_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, stat_ptr: i32| -> i32 {
            if fd == 1 || fd == 2 {
                let mem = env.data().memory.as_ref().unwrap().clone();
                let mut stat = [0u8; 24];
                stat[0] = 2; // CHARACTER_DEVICE
                mem.view(&env).write(stat_ptr as u64, &stat).unwrap();
                0
            } else {
                8
            }
        });

    let fd_seek_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _whence: i32, _result_ptr: i32| -> i32 { 52 });

    let random_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, buf_ptr: i32, buf_len: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = vec![0u8; buf_len as usize];
            use rand::Rng;
            rand::thread_rng().fill(&mut buf[..]);
            mem.view(&env).write(buf_ptr as u64, &buf).unwrap();
            0
        });

    let tz_fn = Function::new_typed(store, |_hi: i32, _lo: i32| -> i32 { 0 });
    let interrupt_fn = Function::new_typed(store, || -> i32 { 0 });
    let promise_fn = Function::new_typed(store, |_promise_ptr: i32, _reason_ptr: i32, _is_handled: i32| {});
    let mod_norm_fn = Function::new_typed(store, |_name_ptr: i32, _name_len: i32| -> i32 { 0 });
    let mod_load_fn = Function::new_typed(store, |_name_ptr: i32, _name_len: i32| -> i32 { 0 });
    let host_call_fn = Function::new_typed(store, |_name_ptr: i32, _name_len: i32, _this_ptr: i32, _argc: i32, _argv_ptr: i32| -> i32 { 0 });

    imports! {
        "wasi_snapshot_preview1" => {
            "clock_time_get" => clock_fn,
            "fd_write" => fd_write_fn,
            "fd_close" => fd_close_fn,
            "fd_fdstat_get" => fd_fdstat_fn,
            "fd_seek" => fd_seek_fn,
            "random_get" => random_fn,
        },
        "env" => {
            "host_get_timezone_offset" => tz_fn,
            "host_interrupt" => interrupt_fn,
            "host_promise_rejection" => promise_fn,
            "host_module_normalize" => mod_norm_fn,
            "host_module_load" => mod_load_fn,
            "host_call" => host_call_fn,
        },
    }
}

/// Build minimal WASI imports (no QuickJS-specific `env` functions).
/// For use with MicroPython, Lua, and other interpreters that only need WASI.
pub fn build_wasi_imports(store: &mut Store, env: &FunctionEnv<WasiEnv>) -> Imports {
    let clock_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _clock_id: i32, _precision: i64, result_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let ns = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64;
            mem.view(&env).write(result_ptr as u64, &ns.to_le_bytes()).unwrap();
            0
        });

    let fd_write_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _fd: i32, _iovs_ptr: i32, _iovs_len: i32, nwritten_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            mem.view(&env).write(nwritten_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });

    let fd_close_fn = Function::new_typed(store, |_fd: i32| -> i32 { 52 });

    let fd_fdstat_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, stat_ptr: i32| -> i32 {
            if fd == 1 || fd == 2 {
                let mem = env.data().memory.as_ref().unwrap().clone();
                let mut stat = [0u8; 24];
                stat[0] = 2;
                mem.view(&env).write(stat_ptr as u64, &stat).unwrap();
                0
            } else {
                8
            }
        });

    let fd_seek_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _whence: i32, _result_ptr: i32| -> i32 { 52 });

    let random_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, buf_ptr: i32, buf_len: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = vec![0u8; buf_len as usize];
            use rand::Rng;
            rand::thread_rng().fill(&mut buf[..]);
            mem.view(&env).write(buf_ptr as u64, &buf).unwrap();
            0
        });

    imports! {
        "wasi_snapshot_preview1" => {
            "clock_time_get" => clock_fn,
            "fd_write" => fd_write_fn,
            "fd_close" => fd_close_fn,
            "fd_fdstat_get" => fd_fdstat_fn,
            "fd_seek" => fd_seek_fn,
            "random_get" => random_fn,
        },
    }
}
