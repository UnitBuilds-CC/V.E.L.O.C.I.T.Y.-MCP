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
        |env: FunctionEnvMut<WasiEnv>, fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            let mut total_written: u32 = 0;

            for i in 0..iovs_len as u64 {
                let iov_base_offset = (iovs_ptr as u64) + i * 8;
                let iov_len_offset = iov_base_offset + 4;

                let mut buf_ptr_bytes = [0u8; 4];
                let mut buf_len_bytes = [0u8; 4];
                view.read(iov_base_offset, &mut buf_ptr_bytes).unwrap();
                view.read(iov_len_offset, &mut buf_len_bytes).unwrap();

                let buf_ptr = u32::from_le_bytes(buf_ptr_bytes) as u64;
                let buf_len = u32::from_le_bytes(buf_len_bytes) as usize;

                if buf_len > 0 {
                    let mut data = vec![0u8; buf_len];
                    view.read(buf_ptr, &mut data).unwrap();
                    if fd == 2 {
                        eprint!("{}", String::from_utf8_lossy(&data));
                    }
                    total_written += buf_len as u32;
                }
            }

            view.write(nwritten_ptr as u64, &total_written.to_le_bytes()).unwrap();
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

    let proc_exit_fn = Function::new_typed(store, |_code: i32| {
        // no-op: interpreter WASM should not exit the host process
    });

    let args_sizes_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, argc_ptr: i32, argv_buf_size_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            mem.view(&env).write(argc_ptr as u64, &0u32.to_le_bytes()).unwrap();
            mem.view(&env).write(argv_buf_size_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });

    let args_get_fn = Function::new_typed(store, |_argv_ptr: i32, _argv_buf_ptr: i32| -> i32 { 0 });

    let environ_sizes_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, count_ptr: i32, buf_size_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            mem.view(&env).write(count_ptr as u64, &0u32.to_le_bytes()).unwrap();
            mem.view(&env).write(buf_size_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });

    let environ_get_fn = Function::new_typed(store, |_environ_ptr: i32, _environ_buf_ptr: i32| -> i32 { 0 });

    imports! {
        "wasi_snapshot_preview1" => {
            "clock_time_get" => clock_fn,
            "fd_write" => fd_write_fn,
            "fd_close" => fd_close_fn,
            "fd_fdstat_get" => fd_fdstat_fn,
            "fd_seek" => fd_seek_fn,
            "random_get" => random_fn,
            "proc_exit" => proc_exit_fn,
            "args_sizes_get" => args_sizes_fn,
            "args_get" => args_get_fn,
            "environ_sizes_get" => environ_sizes_fn,
            "environ_get" => environ_get_fn,
        },
    }
}
