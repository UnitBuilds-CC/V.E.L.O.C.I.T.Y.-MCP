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

    let clock_res_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _clock_id: i32, resolution_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let resolution: u64 = 1000;
            mem.view(&env).write(resolution_ptr as u64, &resolution.to_le_bytes()).unwrap();
            0
        });

    let fd_advise_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _len: i64, _advice: i32| -> i32 { 0 });
    let fd_allocate_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _len: i64| -> i32 { 8 });
    let fd_datasync_fn = Function::new_typed(store, |_fd: i32| -> i32 { 8 });
    let fd_fdstat_set_flags_fn = Function::new_typed(store, |_fd: i32, _flags: i32| -> i32 { 8 });
    let fd_fdstat_set_rights_fn = Function::new_typed(store, |_fd: i32, _rights_base: i64, _rights_inheriting: i64| -> i32 { 8 });
    let fd_filestat_get_fn = Function::new_typed(store, |_fd: i32, _stat_ptr: i32| -> i32 { 8 });
    let fd_filestat_set_size_fn = Function::new_typed(store, |_fd: i32, _size: i64| -> i32 { 8 });
    let fd_filestat_set_times_fn = Function::new_typed(store, |_fd: i32, _atime: i64, _mtime: i64, _fst_flags: i32| -> i32 { 8 });
    let fd_pread_fn = Function::new_typed(store, |_fd: i32, _iovs_ptr: i32, _iovs_len: i32, _offset: i64, _nread_ptr: i32| -> i32 { 8 });
    let fd_prestat_dir_name_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32| -> i32 { 8 });
    let fd_prestat_get_fn = Function::new_typed(store, |_fd: i32, _buf_ptr: i32| -> i32 { 8 });
    let fd_pwrite_fn = Function::new_typed(store, |_fd: i32, _iovs_ptr: i32, _iovs_len: i32, _offset: i64, _nwritten_ptr: i32| -> i32 { 8 });
    let fd_read_fn = Function::new_typed(store, |_fd: i32, _iovs_ptr: i32, _iovs_len: i32, _nread_ptr: i32| -> i32 { 8 });
    let fd_readdir_fn = Function::new_typed(store, |_fd: i32, _buf_ptr: i32, _buf_len: i32, _cookie: i64, _bufused_ptr: i32| -> i32 { 8 });
    let fd_renumber_fn = Function::new_typed(store, |_fd: i32, _to: i32| -> i32 { 8 });
    let fd_sync_fn = Function::new_typed(store, |_fd: i32| -> i32 { 8 });
    let fd_tell_fn = Function::new_typed(store, |_fd: i32, _offset_ptr: i32| -> i32 { 8 });
    let path_create_directory_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32| -> i32 { 28 });
    let path_filestat_get_fn = Function::new_typed(store, |_fd: i32, _flags: i32, _path_ptr: i32, _path_len: i32, _stat_ptr: i32| -> i32 { 28 });
    let path_filestat_set_times_fn = Function::new_typed(store, |_fd: i32, _flags: i32, _path_ptr: i32, _path_len: i32, _atime: i64, _mtime: i64, _fst_flags: i32| -> i32 { 28 });
    let path_link_fn = Function::new_typed(store, |_old_fd: i32, _old_flags: i32, _old_path_ptr: i32, _old_path_len: i32, _new_fd: i32, _new_path_ptr: i32, _new_path_len: i32| -> i32 { 28 });
    let path_open_fn = Function::new_typed(store, |_dirfd: i32, _dirflags: i32, _path_ptr: i32, _path_len: i32, _o_flags: i32, _fs_rights_base: i64, _fs_rights_inheriting: i64, _fd_flags: i32, _fd_ptr: i32| -> i32 { 28 });
    let path_readlink_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32, _buf_ptr: i32, _buf_len: i32, _bufused_ptr: i32| -> i32 { 28 });
    let path_remove_directory_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32| -> i32 { 28 });
    let path_rename_fn = Function::new_typed(store, |_fd: i32, _old_ptr: i32, _old_len: i32, _new_fd: i32, _new_ptr: i32, _new_len: i32| -> i32 { 28 });
    let path_symlink_fn = Function::new_typed(store, |_old_ptr: i32, _old_len: i32, _fd: i32, _new_ptr: i32, _new_len: i32| -> i32 { 28 });
    let path_unlink_file_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32| -> i32 { 28 });
    let poll_oneoff_fn = Function::new_typed(store, |_in_ptr: i32, _out_ptr: i32, _nsubscriptions: i32, _nevents_ptr: i32| -> i32 { 28 });
    let sched_yield_fn = Function::new_typed(store, || -> i32 { 0 });
    let sock_accept_fn = Function::new_typed(store, |_fd: i32, _flags: i32, _result_fd_ptr: i32| -> i32 { 28 });
    let sock_recv_fn = Function::new_typed(store, |_fd: i32, _ri_data_ptr: i32, _ri_data_len: i32, _ri_flags: i32, _ro_datalen_ptr: i32, _ro_flags_ptr: i32| -> i32 { 28 });
    let sock_send_fn = Function::new_typed(store, |_fd: i32, _si_data_ptr: i32, _si_data_len: i32, _si_flags: i32, _so_datalen_ptr: i32| -> i32 { 28 });
    let sock_shutdown_fn = Function::new_typed(store, |_fd: i32, _how: i32| -> i32 { 28 });

    imports! {
        "wasi_snapshot_preview1" => {
            "clock_time_get" => clock_fn,
            "clock_res_get" => clock_res_fn,
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
            "fd_advise" => fd_advise_fn,
            "fd_allocate" => fd_allocate_fn,
            "fd_datasync" => fd_datasync_fn,
            "fd_fdstat_set_flags" => fd_fdstat_set_flags_fn,
            "fd_fdstat_set_rights" => fd_fdstat_set_rights_fn,
            "fd_filestat_get" => fd_filestat_get_fn,
            "fd_filestat_set_size" => fd_filestat_set_size_fn,
            "fd_filestat_set_times" => fd_filestat_set_times_fn,
            "fd_pread" => fd_pread_fn,
            "fd_prestat_dir_name" => fd_prestat_dir_name_fn,
            "fd_prestat_get" => fd_prestat_get_fn,
            "fd_pwrite" => fd_pwrite_fn,
            "fd_read" => fd_read_fn,
            "fd_readdir" => fd_readdir_fn,
            "fd_renumber" => fd_renumber_fn,
            "fd_sync" => fd_sync_fn,
            "fd_tell" => fd_tell_fn,
            "path_create_directory" => path_create_directory_fn,
            "path_filestat_get" => path_filestat_get_fn,
            "path_filestat_set_times" => path_filestat_set_times_fn,
            "path_link" => path_link_fn,
            "path_open" => path_open_fn,
            "path_readlink" => path_readlink_fn,
            "path_remove_directory" => path_remove_directory_fn,
            "path_rename" => path_rename_fn,
            "path_symlink" => path_symlink_fn,
            "path_unlink_file" => path_unlink_file_fn,
            "poll_oneoff" => poll_oneoff_fn,
            "sched_yield" => sched_yield_fn,
            "sock_accept" => sock_accept_fn,
            "sock_recv" => sock_recv_fn,
            "sock_send" => sock_send_fn,
            "sock_shutdown" => sock_shutdown_fn,
        },
    }
}

// ─── Ruby/WASM Component Model imports ───────────────────────────────────────

use std::sync::{Arc, Mutex};

pub type RubySlab = Arc<Mutex<Vec<Option<i32>>>>;

pub fn new_ruby_slab() -> RubySlab {
    Arc::new(Mutex::new(Vec::new()))
}

pub fn slab_insert(slab: &RubySlab, val: i32) -> i32 {
    let mut s = slab.lock().unwrap();
    for (i, slot) in s.iter_mut().enumerate() {
        if slot.is_none() {
            *slot = Some(val);
            return i as i32;
        }
    }
    s.push(Some(val));
    (s.len() - 1) as i32
}

pub fn slab_get(slab: &RubySlab, handle: i32) -> i32 {
    let s = slab.lock().unwrap();
    s.get(handle as usize).copied().flatten().unwrap_or(0)
}

fn slab_remove(slab: &RubySlab, handle: i32) {
    let mut s = slab.lock().unwrap();
    if (handle as usize) < s.len() {
        s[handle as usize] = None;
    }
}

pub fn build_ruby_imports(
    store: &mut Store,
    env: &FunctionEnv<WasiEnv>,
    ruby_slab: RubySlab,
    js_slab: RubySlab,
) -> Imports {
    // ── WASI preview 1 ──

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
                let iov_base = (iovs_ptr as u64) + i * 8;
                let mut bp = [0u8; 4];
                let mut bl = [0u8; 4];
                view.read(iov_base, &mut bp).unwrap();
                view.read(iov_base + 4, &mut bl).unwrap();
                let ptr = u32::from_le_bytes(bp) as u64;
                let len = u32::from_le_bytes(bl) as usize;
                if len > 0 {
                    let mut data = vec![0u8; len];
                    view.read(ptr, &mut data).unwrap();
                    if fd == 2 { eprint!("{}", String::from_utf8_lossy(&data)); }
                    total_written += len as u32;
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
            } else { 8 }
        });

    let fd_seek_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _whence: i32, _rp: i32| -> i32 { 52 });

    let random_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, buf_ptr: i32, buf_len: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = vec![0u8; buf_len as usize];
            use rand::Rng;
            rand::thread_rng().fill(&mut buf[..]);
            mem.view(&env).write(buf_ptr as u64, &buf).unwrap();
            0
        });

    let proc_exit_fn = Function::new_typed(store, |_code: i32| {});
    let args_sizes_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, argc_ptr: i32, argv_buf_size_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            mem.view(&env).write(argc_ptr as u64, &0u32.to_le_bytes()).unwrap();
            mem.view(&env).write(argv_buf_size_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });
    let args_get_fn = Function::new_typed(store, |_a: i32, _b: i32| -> i32 { 0 });
    let environ_sizes_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, count_ptr: i32, buf_size_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            mem.view(&env).write(count_ptr as u64, &0u32.to_le_bytes()).unwrap();
            mem.view(&env).write(buf_size_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });
    let environ_get_fn = Function::new_typed(store, |_a: i32, _b: i32| -> i32 { 0 });
    let clock_res_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _clock_id: i32, resolution_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let resolution: u64 = 1000;
            mem.view(&env).write(resolution_ptr as u64, &resolution.to_le_bytes()).unwrap();
            0
        });

    let fd_advise_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _len: i64, _advice: i32| -> i32 { 0 });
    let fd_allocate_fn = Function::new_typed(store, |_fd: i32, _offset: i64, _len: i64| -> i32 { 8 });
    let fd_datasync_fn = Function::new_typed(store, |_fd: i32| -> i32 { 8 });
    let fd_fdstat_set_flags_fn = Function::new_typed(store, |_fd: i32, _flags: i32| -> i32 { 8 });
    let fd_fdstat_set_rights_fn = Function::new_typed(store, |_fd: i32, _rb: i64, _ri: i64| -> i32 { 8 });
    let fd_filestat_get_fn = Function::new_typed(store, |_fd: i32, _sp: i32| -> i32 { 8 });
    let fd_filestat_set_size_fn = Function::new_typed(store, |_fd: i32, _size: i64| -> i32 { 8 });
    let fd_filestat_set_times_fn = Function::new_typed(store, |_fd: i32, _at: i64, _mt: i64, _fl: i32| -> i32 { 8 });
    let fd_pread_fn = Function::new_typed(store, |_fd: i32, _ip: i32, _il: i32, _off: i64, _np: i32| -> i32 { 8 });
    let fd_prestat_dir_name_fn = Function::new_typed(store, |_fd: i32, _pp: i32, _pl: i32| -> i32 { 8 });
    let fd_prestat_get_fn = Function::new_typed(store, |_fd: i32, _bp: i32| -> i32 { 8 });
    let fd_pwrite_fn = Function::new_typed(store, |_fd: i32, _ip: i32, _il: i32, _off: i64, _np: i32| -> i32 { 8 });
    let fd_read_fn = Function::new_typed(store, |_fd: i32, _ip: i32, _il: i32, _np: i32| -> i32 { 8 });
    let fd_readdir_fn = Function::new_typed(store, |_fd: i32, _bp: i32, _bl: i32, _c: i64, _bp2: i32| -> i32 { 8 });
    let fd_renumber_fn = Function::new_typed(store, |_fd: i32, _to: i32| -> i32 { 8 });
    let fd_sync_fn = Function::new_typed(store, |_fd: i32| -> i32 { 8 });
    let fd_tell_fn = Function::new_typed(store, |_fd: i32, _op: i32| -> i32 { 8 });
    let path_create_directory_fn = Function::new_typed(store, |_fd: i32, _pp: i32, _pl: i32| -> i32 { 28 });
    let path_filestat_get_fn = Function::new_typed(store, |_fd: i32, _fl: i32, _pp: i32, _pl: i32, _sp: i32| -> i32 { 28 });
    let path_filestat_set_times_fn = Function::new_typed(store, |_fd: i32, _fl: i32, _pp: i32, _pl: i32, _at: i64, _mt: i64, _fs: i32| -> i32 { 28 });
    let path_link_fn = Function::new_typed(store, |_ofd: i32, _ofl: i32, _opp: i32, _opl: i32, _nfd: i32, _npp: i32, _npl: i32| -> i32 { 28 });
    let path_open_fn = Function::new_typed(store, |_d: i32, _df: i32, _pp: i32, _pl: i32, _of: i32, _rb: i64, _ri: i64, _ff: i32, _fp: i32| -> i32 { 28 });
    let path_readlink_fn = Function::new_typed(store, |_fd: i32, _pp: i32, _pl: i32, _bp: i32, _bl: i32, _bu: i32| -> i32 { 28 });
    let path_remove_directory_fn = Function::new_typed(store, |_fd: i32, _pp: i32, _pl: i32| -> i32 { 28 });
    let path_rename_fn = Function::new_typed(store, |_fd: i32, _op: i32, _ol: i32, _nfd: i32, _np: i32, _nl: i32| -> i32 { 28 });
    let path_symlink_fn = Function::new_typed(store, |_op: i32, _ol: i32, _fd: i32, _np: i32, _nl: i32| -> i32 { 28 });
    let path_unlink_file_fn = Function::new_typed(store, |_fd: i32, _pp: i32, _pl: i32| -> i32 { 28 });
    let poll_oneoff_fn = Function::new_typed(store, |_ip: i32, _op: i32, _ns: i32, _ne: i32| -> i32 { 28 });
    let sched_yield_fn = Function::new_typed(store, || -> i32 { 0 });
    let sock_accept_fn = Function::new_typed(store, |_fd: i32, _fl: i32, _rfp: i32| -> i32 { 28 });
    let sock_recv_fn = Function::new_typed(store, |_fd: i32, _ridp: i32, _ridl: i32, _rif: i32, _rod: i32, _rof: i32| -> i32 { 28 });
    let sock_send_fn = Function::new_typed(store, |_fd: i32, _sidp: i32, _sil: i32, _sif: i32, _sod: i32| -> i32 { 28 });
    let sock_shutdown_fn = Function::new_typed(store, |_fd: i32, _how: i32| -> i32 { 28 });

    // ── canonical_abi: resource management for rb-abi-value handles ──

    let rs_new = ruby_slab.clone();
    let resource_new_ruby = Function::new_typed(store,
        move |raw_val: i32| -> i32 { slab_insert(&rs_new, raw_val) });

    let rs_get = ruby_slab.clone();
    let resource_get_ruby = Function::new_typed(store,
        move |handle: i32| -> i32 { slab_get(&rs_get, handle) });

    let rs_drop = ruby_slab.clone();
    let resource_drop_ruby = Function::new_typed(store,
        move |handle: i32| { slab_remove(&rs_drop, handle); });

    let js_drop = js_slab.clone();
    let resource_drop_js = Function::new_typed(store,
        move |handle: i32| { slab_remove(&js_drop, handle); });

    // ── rb-js-abi-host: JS interop stubs ──

    let eval_js_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _code_ptr: i32, _code_len: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = [0u8; 8];
            buf[0] = 1; // failure discriminant
            buf[4] = 0;
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let is_js_fn = Function::new_typed(store, |_handle: i32| -> i32 { 1 });
    let instance_of_fn = Function::new_typed(store, |_value: i32, _klass: i32| -> i32 { 0 });

    let js_global = js_slab.clone();
    let global_this_fn = Function::new_typed(store,
        move || -> i32 { slab_insert(&js_global, 0) });

    let js_int = js_slab.clone();
    let int_to_js_fn = Function::new_typed(store,
        move |val: i32| -> i32 { slab_insert(&js_int, val) });

    let js_float = js_slab.clone();
    let float_to_js_fn = Function::new_typed(store,
        move |_val: f64| -> i32 { slab_insert(&js_float, 0) });

    let js_str = js_slab.clone();
    let string_to_js_fn = Function::new_typed(store,
        move |_ptr: i32, _len: i32| -> i32 { slab_insert(&js_str, 0) });

    let js_bool = js_slab.clone();
    let bool_to_js_fn = Function::new_typed(store,
        move |val: i32| -> i32 { slab_insert(&js_bool, val) });

    let js_proc = js_slab.clone();
    let proc_to_js_fn = Function::new_typed(store,
        move |_id: i32| -> i32 { slab_insert(&js_proc, 0) });

    let js_obj = js_slab.clone();
    let rb_obj_to_js_fn = Function::new_typed(store,
        move |raw_val: i32| -> i32 { slab_insert(&js_obj, raw_val) });

    let js_val_to_string_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _handle: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let buf = [0u8; 8];
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let js_val_to_integer_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _handle: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = [0u8; 16];
            buf[0] = 0; // as-float discriminant
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let js_val_typeof_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _handle: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let buf = [0u8; 8];
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let js_val_equal_fn = Function::new_typed(store, |_lhs: i32, _rhs: i32| -> i32 { 0 });
    let js_val_strictly_equal_fn = Function::new_typed(store, |_lhs: i32, _rhs: i32| -> i32 { 0 });

    let reflect_apply_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _target: i32, _this: i32, _args_ptr: i32, _args_len: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = [0u8; 8];
            buf[0] = 1;
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let reflect_get_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _target: i32, _key_ptr: i32, _key_len: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = [0u8; 8];
            buf[0] = 1;
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let reflect_set_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _target: i32, _key_ptr: i32, _key_len: i32, _value: i32, ret_ptr: i32| {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let mut buf = [0u8; 8];
            buf[0] = 1;
            mem.view(&env).write(ret_ptr as u64, &buf).unwrap();
        });

    let export_js_fn = Function::new_typed(store, |_handle: i32| {});

    let js_import = js_slab.clone();
    let import_js_fn = Function::new_typed(store,
        move || -> i32 { slab_insert(&js_import, 0) });

    let throw_rewind_fn = Function::new_typed(store, |_ptr: i32, _len: i32| {});

    imports! {
        "wasi_snapshot_preview1" => {
            "clock_time_get" => clock_fn,
            "clock_res_get" => clock_res_fn,
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
            "fd_advise" => fd_advise_fn,
            "fd_allocate" => fd_allocate_fn,
            "fd_datasync" => fd_datasync_fn,
            "fd_fdstat_set_flags" => fd_fdstat_set_flags_fn,
            "fd_fdstat_set_rights" => fd_fdstat_set_rights_fn,
            "fd_filestat_get" => fd_filestat_get_fn,
            "fd_filestat_set_size" => fd_filestat_set_size_fn,
            "fd_filestat_set_times" => fd_filestat_set_times_fn,
            "fd_pread" => fd_pread_fn,
            "fd_prestat_dir_name" => fd_prestat_dir_name_fn,
            "fd_prestat_get" => fd_prestat_get_fn,
            "fd_pwrite" => fd_pwrite_fn,
            "fd_read" => fd_read_fn,
            "fd_readdir" => fd_readdir_fn,
            "fd_renumber" => fd_renumber_fn,
            "fd_sync" => fd_sync_fn,
            "fd_tell" => fd_tell_fn,
            "path_create_directory" => path_create_directory_fn,
            "path_filestat_get" => path_filestat_get_fn,
            "path_filestat_set_times" => path_filestat_set_times_fn,
            "path_link" => path_link_fn,
            "path_open" => path_open_fn,
            "path_readlink" => path_readlink_fn,
            "path_remove_directory" => path_remove_directory_fn,
            "path_rename" => path_rename_fn,
            "path_symlink" => path_symlink_fn,
            "path_unlink_file" => path_unlink_file_fn,
            "poll_oneoff" => poll_oneoff_fn,
            "sched_yield" => sched_yield_fn,
            "sock_accept" => sock_accept_fn,
            "sock_recv" => sock_recv_fn,
            "sock_send" => sock_send_fn,
            "sock_shutdown" => sock_shutdown_fn,
        },
        "canonical_abi" => {
            "resource_new_rb-abi-value" => resource_new_ruby,
            "resource_get_rb-abi-value" => resource_get_ruby,
            "resource_drop_rb-abi-value" => resource_drop_ruby,
            "resource_drop_js-abi-value" => resource_drop_js,
        },
        "rb-js-abi-host" => {
            "eval-js" => eval_js_fn,
            "is-js" => is_js_fn,
            "instance-of" => instance_of_fn,
            "global-this" => global_this_fn,
            "int-to-js-number" => int_to_js_fn,
            "float-to-js-number" => float_to_js_fn,
            "string-to-js-string" => string_to_js_fn,
            "bool-to-js-bool" => bool_to_js_fn,
            "proc-to-js-function" => proc_to_js_fn,
            "rb-object-to-js-rb-value" => rb_obj_to_js_fn,
            "js-value-to-string" => js_val_to_string_fn,
            "js-value-to-integer" => js_val_to_integer_fn,
            "js-value-typeof" => js_val_typeof_fn,
            "js-value-equal" => js_val_equal_fn,
            "js-value-strictly-equal" => js_val_strictly_equal_fn,
            "reflect-apply" => reflect_apply_fn,
            "reflect-get" => reflect_get_fn,
            "reflect-set" => reflect_set_fn,
            "export-js-value-to-host" => export_js_fn,
            "import-js-value-from-host" => import_js_fn,
            "rb_wasm_throw_prohibit_rewind_exception" => throw_rewind_fn,
        },
    }
}
