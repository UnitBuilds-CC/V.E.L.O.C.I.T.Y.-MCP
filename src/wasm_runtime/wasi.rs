//! Shared WASI import builder for WASM runtimes.
//!
//! Provides a minimal `wasi_snapshot_preview1` implementation sufficient for
//! interpreter WASM modules (QuickJS, MicroPython, Lua, etc.). Also provides
//! QuickJS-specific `env` imports.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{Read as IoRead, Seek};
use std::path::PathBuf;
use wasmer::{Function, FunctionEnv, FunctionEnvMut, Imports, Memory, Store};
use wasmer::imports;

const ERRNO_BADF: i32 = 8;
const ERRNO_AGAIN: i32 = 6;
const ERRNO_NOENT: i32 = 44;
const ERRNO_PERM: i32 = 63;
const ERRNO_INVAL: i32 = 28;
const ERRNO_IO: i32 = 29;
const ERRNO_NOTSUP: i32 = 58;

/// Environment shared between host WASI functions and the WASM guest.
/// Holds a reference to the guest's linear memory, set after instantiation.
/// When `wasm-networking` is enabled, also holds TCP socket state.
/// Stdin input buffer allows fd_read(fd=0) to read from host-provided data.
/// Filesystem support via preopened directories and file descriptors.
#[derive(Clone)]
pub struct WasiEnv {
    pub memory: Option<Memory>,
    pub stdin_buffer: std::sync::Arc<std::sync::Mutex<VecDeque<u8>>>,
    pub fs_state: std::sync::Arc<std::sync::Mutex<WasiFsState>>,
    #[cfg(feature = "wasm-networking")]
    pub socket_state: std::sync::Arc<std::sync::Mutex<super::wasi_net::WasiSocketState>>,
}

/// Minimal WASI filesystem state.
/// Tracks open files and preopened directories.
pub struct WasiFsState {
    pub files: std::collections::HashMap<i32, File>,
    pub preopens: Vec<PathBuf>,
    next_fd: i32,
}

impl WasiFsState {
    pub fn new() -> Self {
        Self {
            files: std::collections::HashMap::new(),
            preopens: Vec::new(),
            next_fd: 3, // Start after stdin(0), stdout(1), stderr(2)
        }
    }

    pub fn with_preopen(mut self, path: PathBuf) -> Self {
        self.preopens.push(path);
        self
    }

    fn alloc_fd(&mut self) -> i32 {
        let fd = self.next_fd;
        self.next_fd += 1;
        fd
    }
}

impl WasiEnv {
    pub fn new() -> Self {
        Self {
            memory: None,
            stdin_buffer: std::sync::Arc::new(std::sync::Mutex::new(VecDeque::new())),
            fs_state: std::sync::Arc::new(std::sync::Mutex::new(WasiFsState::new())),
            #[cfg(feature = "wasm-networking")]
            socket_state: std::sync::Arc::new(std::sync::Mutex::new(
                super::wasi_net::WasiSocketState::new(),
            )),
        }
    }

    /// Add a preopened directory for filesystem access.
    pub fn with_preopen(self, path: PathBuf) -> Self {
        {
            let mut guard = self.fs_state.lock().unwrap();
            guard.preopens.push(path);
        }
        self
    }

    /// Push bytes into the stdin buffer for fd_read(fd=0) to consume.
    pub fn push_stdin(&self, data: &[u8]) {
        let mut buf = self.stdin_buffer.lock().unwrap();
        buf.extend(data);
    }
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

    let fd_close_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32| -> i32 {
            // File descriptors (fd >= 3)
            if fd >= 3 {
                let fs_state = env.data().fs_state.clone();
                if fs_state.lock().unwrap().files.remove(&fd).is_some() {
                    return 0;
                }
                return ERRNO_BADF;
            }
            // stdin/stdout/stderr - can't close
            ERRNO_BADF
        });

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

    let fd_seek_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, offset: i64, whence: i32, result_ptr: i32| -> i32 {
            if fd < 3 {
                return ERRNO_BADF; // Can't seek on stdin/stdout/stderr
            }
            
            let fs_state = env.data().fs_state.clone();
            let mut guard = fs_state.lock().unwrap();
            
            let file = match guard.files.get_mut(&fd) {
                Some(f) => f,
                None => return ERRNO_BADF,
            };
            
            let seek_from = match whence {
                0 => std::io::SeekFrom::Start(offset as u64), // SEEK_SET
                1 => std::io::SeekFrom::Current(offset),      // SEEK_CUR
                2 => std::io::SeekFrom::End(offset),          // SEEK_END
                _ => return ERRNO_INVAL,
            };
            
            match file.seek(seek_from) {
                Ok(pos) => {
                    let mem = env.data().memory.as_ref().unwrap().clone();
                    mem.view(&env).write(result_ptr as u64, &pos.to_le_bytes()).unwrap();
                    0
                }
                Err(_) => ERRNO_IO,
            }
        });

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

    #[cfg(not(feature = "wasm-networking"))]
    let fd_write_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);

            // File descriptors (fd >= 3) - write to files
            if fd >= 3 {
                use std::io::Write;
                let fs_state = env.data().fs_state.clone();
                let mut guard = fs_state.lock().unwrap();
                
                let file = match guard.files.get_mut(&fd) {
                    Some(f) => f,
                    None => return ERRNO_BADF,
                };

                let mut total_written: u32 = 0;
                for i in 0..iovs_len as u64 {
                    let base = (iovs_ptr as u64) + i * 8;
                    let mut ptr_bytes = [0u8; 4];
                    let mut len_bytes = [0u8; 4];
                    view.read(base, &mut ptr_bytes).unwrap();
                    view.read(base + 4, &mut len_bytes).unwrap();
                    let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                    let buf_len = u32::from_le_bytes(len_bytes) as usize;

                    if buf_len > 0 {
                        let mut data = vec![0u8; buf_len];
                        view.read(buf_ptr, &mut data).unwrap();
                        match file.write(&data) {
                            Ok(n) => total_written += n as u32,
                            Err(_) => break,
                        }
                    }
                }
                view.write(nwritten_ptr as u64, &total_written.to_le_bytes()).unwrap();
                return 0;
            }

            // stdout/stderr
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

    #[cfg(feature = "wasm-networking")]
    let fd_write_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, iovs_ptr: i32, iovs_len: i32, nwritten_ptr: i32| -> i32 {
            use super::wasi_net::SOCKET_FD_BASE;
            use std::io::Write;
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);

            // Socket fds (fd >= SOCKET_FD_BASE)
            if fd >= SOCKET_FD_BASE {
                let state = env.data().socket_state.clone();
                let mut guard = state.lock().unwrap();
                let mut total_sent: u32 = 0;
                for i in 0..iovs_len as u64 {
                    let base = (iovs_ptr as u64) + i * 8;
                    let mut ptr_bytes = [0u8; 4];
                    let mut len_bytes = [0u8; 4];
                    view.read(base, &mut ptr_bytes).unwrap();
                    view.read(base + 4, &mut len_bytes).unwrap();
                    let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                    let buf_len = u32::from_le_bytes(len_bytes) as usize;
                    if buf_len > 0 {
                        let mut data = vec![0u8; buf_len];
                        view.read(buf_ptr, &mut data).unwrap();
                        if let Some(stream) = guard.sockets.get_mut(&fd) {
                            match stream.write(&data) {
                                Ok(n) => total_sent += n as u32,
                                Err(_) => break,
                            }
                        }
                    }
                }
                view.write(nwritten_ptr as u64, &total_sent.to_le_bytes()).unwrap();
                return 0;
            }

            // File descriptors (fd >= 3 and < SOCKET_FD_BASE)
            if fd >= 3 && fd < SOCKET_FD_BASE {
                let fs_state = env.data().fs_state.clone();
                let mut guard = fs_state.lock().unwrap();
                
                let file = match guard.files.get_mut(&fd) {
                    Some(f) => f,
                    None => return ERRNO_BADF,
                };

                let mut total_written: u32 = 0;
                for i in 0..iovs_len as u64 {
                    let base = (iovs_ptr as u64) + i * 8;
                    let mut ptr_bytes = [0u8; 4];
                    let mut len_bytes = [0u8; 4];
                    view.read(base, &mut ptr_bytes).unwrap();
                    view.read(base + 4, &mut len_bytes).unwrap();
                    let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                    let buf_len = u32::from_le_bytes(len_bytes) as usize;

                    if buf_len > 0 {
                        let mut data = vec![0u8; buf_len];
                        view.read(buf_ptr, &mut data).unwrap();
                        match file.write(&data) {
                            Ok(n) => total_written += n as u32,
                            Err(_) => break,
                        }
                    }
                }
                view.write(nwritten_ptr as u64, &total_written.to_le_bytes()).unwrap();
                return 0;
            }

            // stdout/stderr
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

    #[cfg(not(feature = "wasm-networking"))]
    let fd_close_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32| -> i32 {
            if fd >= 3 {
                let fs_state = env.data().fs_state.clone();
                if fs_state.lock().unwrap().files.remove(&fd).is_some() {
                    return 0;
                }
                return ERRNO_BADF;
            }
            ERRNO_BADF
        });

    #[cfg(not(feature = "wasm-networking"))]
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

    #[cfg(feature = "wasm-networking")]
    let fd_close_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32| -> i32 {
            use super::wasi_net::SOCKET_FD_BASE;
            
            // Socket fds
            if fd >= SOCKET_FD_BASE {
                let state = env.data().socket_state.clone();
                if state.lock().unwrap().close(fd) { 0 } else { ERRNO_BADF }
            // File fds
            } else if fd >= 3 {
                let fs_state = env.data().fs_state.clone();
                if fs_state.lock().unwrap().files.remove(&fd).is_some() {
                    return 0;
                }
                ERRNO_BADF
            } else {
                ERRNO_BADF
            }
        });

    #[cfg(feature = "wasm-networking")]
    let fd_fdstat_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, stat_ptr: i32| -> i32 {
            use super::wasi_net::SOCKET_FD_BASE;
            if fd == 1 || fd == 2 {
                let mem = env.data().memory.as_ref().unwrap().clone();
                let mut stat = [0u8; 24];
                stat[0] = 2;
                mem.view(&env).write(stat_ptr as u64, &stat).unwrap();
                0
            } else if fd >= SOCKET_FD_BASE {
                let mem = env.data().memory.as_ref().unwrap().clone();
                let guard = env.data().socket_state.lock().unwrap();
                let file_type: u8 = if guard.is_socket_fd(fd) || guard.is_listener_fd(fd) {
                    6 // SOCKET
                } else { return 8; };
                let mut stat = [0u8; 24];
                stat[0] = file_type;
                mem.view(&env).write(stat_ptr as u64, &stat).unwrap();
                0
            } else { 8 }
        });

    let fd_seek_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, offset: i64, whence: i32, result_ptr: i32| -> i32 {
            if fd < 3 {
                return ERRNO_BADF; // Can't seek on stdin/stdout/stderr
            }
            
            let fs_state = env.data().fs_state.clone();
            let mut guard = fs_state.lock().unwrap();
            
            let file = match guard.files.get_mut(&fd) {
                Some(f) => f,
                None => return ERRNO_BADF,
            };
            
            let seek_from = match whence {
                0 => std::io::SeekFrom::Start(offset as u64), // SEEK_SET
                1 => std::io::SeekFrom::Current(offset),      // SEEK_CUR
                2 => std::io::SeekFrom::End(offset),          // SEEK_END
                _ => return ERRNO_INVAL,
            };
            
            match file.seek(seek_from) {
                Ok(pos) => {
                    let mem = env.data().memory.as_ref().unwrap().clone();
                    mem.view(&env).write(result_ptr as u64, &pos.to_le_bytes()).unwrap();
                    0
                }
                Err(_) => ERRNO_IO,
            }
        });

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
    let fd_read_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, iovs_ptr: i32, iovs_len: i32, nread_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);

            // stdin (fd=0) - read from buffer
            if fd == 0 {
                let stdin_buf = env.data().stdin_buffer.clone();
                let mut buf = stdin_buf.lock().unwrap();

                if buf.is_empty() {
                    return ERRNO_AGAIN;
                }

                let mut total_read: u32 = 0;
                for i in 0..iovs_len as u64 {
                    let base = (iovs_ptr as u64) + i * 8;
                    let mut ptr_bytes = [0u8; 4];
                    let mut len_bytes = [0u8; 4];
                    view.read(base, &mut ptr_bytes).unwrap();
                    view.read(base + 4, &mut len_bytes).unwrap();
                    let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                    let buf_len = u32::from_le_bytes(len_bytes) as usize;

                    if buf_len > 0 && !buf.is_empty() {
                        let to_read = buf_len.min(buf.len());
                        let data: Vec<u8> = buf.drain(..to_read).collect();
                        view.write(buf_ptr, &data).unwrap();
                        total_read += to_read as u32;
                    }
                }
                view.write(nread_ptr as u64, &total_read.to_le_bytes()).unwrap();
                return 0;
            }

            // File descriptors (fd >= 3)
            if fd >= 3 {
                let fs_state = env.data().fs_state.clone();
                let mut guard = fs_state.lock().unwrap();
                
                let file = match guard.files.get_mut(&fd) {
                    Some(f) => f,
                    None => return ERRNO_BADF,
                };

                let mut total_read: u32 = 0;
                for i in 0..iovs_len as u64 {
                    let base = (iovs_ptr as u64) + i * 8;
                    let mut ptr_bytes = [0u8; 4];
                    let mut len_bytes = [0u8; 4];
                    view.read(base, &mut ptr_bytes).unwrap();
                    view.read(base + 4, &mut len_bytes).unwrap();
                    let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                    let buf_len = u32::from_le_bytes(len_bytes) as usize;

                    if buf_len > 0 {
                        let mut buffer = vec![0u8; buf_len];
                        match file.read(&mut buffer) {
                            Ok(n) => {
                                view.write(buf_ptr, &buffer[..n]).unwrap();
                                total_read += n as u32;
                                if n < buf_len {
                                    break; // EOF
                                }
                            }
                            Err(_) => break,
                        }
                    }
                }
                view.write(nread_ptr as u64, &total_read.to_le_bytes()).unwrap();
                return 0;
            }

            ERRNO_BADF
        });
    let fd_readdir_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, _buf_ptr: i32, _buf_len: i32, _cookie: i64, _bufused_ptr: i32| -> i32 {
            if fd < 3 {
                return ERRNO_BADF;
            }
            
            let fs_state = env.data().fs_state.clone();
            let mut guard = fs_state.lock().unwrap();
            
            let _file = match guard.files.get_mut(&fd) {
                Some(f) => f,
                None => return ERRNO_BADF,
            };
            
            // Note: This is a simplified implementation. Real WASI readdir
            // would use the file's directory handle and cookie for pagination.
            // For now, we'll just return ENOTSUP since we're treating files as regular files.
            ERRNO_NOTSUP
        });
    let fd_renumber_fn = Function::new_typed(store, |_fd: i32, _to: i32| -> i32 { 8 });
    let fd_sync_fn = Function::new_typed(store, |_fd: i32| -> i32 { 8 });
    let fd_tell_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, offset_ptr: i32| -> i32 {
            if fd >= 3 {
                let fs_state = env.data().fs_state.clone();
                let mut guard = fs_state.lock().unwrap();
                
                let file = match guard.files.get_mut(&fd) {
                    Some(f) => f,
                    None => return ERRNO_BADF,
                };

                match file.stream_position() {
                    Ok(pos) => {
                        let mem = env.data().memory.as_ref().unwrap().clone();
                        mem.view(&env).write(offset_ptr as u64, &(pos as u64).to_le_bytes()).unwrap();
                        0
                    }
                    Err(_) => ERRNO_INVAL,
                }
            } else {
                ERRNO_BADF
            }
        });
    let path_create_directory_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, path_ptr: i32, path_len: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            
            // Read path
            let mut path_bytes = vec![0u8; path_len as usize];
            view.read(path_ptr as u64, &mut path_bytes).unwrap();
            let path_str = match std::str::from_utf8(&path_bytes) {
                Ok(s) => s.trim_end_matches('\0'),
                Err(_) => return ERRNO_INVAL,
            };
            
            // Resolve through preopens
            let fs_state = env.data().fs_state.clone();
            let guard = fs_state.lock().unwrap();
            
            let full_path = if fd == 3 {
                if let Some(preopen) = guard.preopens.first() {
                    preopen.join(path_str)
                } else {
                    return ERRNO_PERM;
                }
            } else {
                PathBuf::from(path_str)
            };
            
            // Security check
            let allowed = guard.preopens.iter().any(|p| full_path.starts_with(p));
            if !allowed {
                return ERRNO_PERM;
            }
            
            // Create directory
            match std::fs::create_dir_all(&full_path) {
                Ok(()) => 0,
                Err(e) => match e.kind() {
                    std::io::ErrorKind::PermissionDenied => ERRNO_PERM,
                    _ => ERRNO_IO,
                }
            }
        });
    let path_filestat_get_fn = Function::new_typed(store, |_fd: i32, _flags: i32, _path_ptr: i32, _path_len: i32, _stat_ptr: i32| -> i32 { 28 });
    let path_filestat_set_times_fn = Function::new_typed(store, |_fd: i32, _flags: i32, _path_ptr: i32, _path_len: i32, _atime: i64, _mtime: i64, _fst_flags: i32| -> i32 { 28 });
    let path_link_fn = Function::new_typed(store, |_old_fd: i32, _old_flags: i32, _old_path_ptr: i32, _old_path_len: i32, _new_fd: i32, _new_path_ptr: i32, _new_path_len: i32| -> i32 { 28 });
    let path_open_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, dirfd: i32, _dirflags: i32, path_ptr: i32, path_len: i32, o_flags: i32, _fs_rights_base: i64, _fs_rights_inheriting: i64, _fd_flags: i32, fd_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            
            // Read path from WASM memory
            let mut path_bytes = vec![0u8; path_len as usize];
            view.read(path_ptr as u64, &mut path_bytes).unwrap();
            let path_str = match std::str::from_utf8(&path_bytes) {
                Ok(s) => s.trim_end_matches('\0'),
                Err(_) => return ERRNO_INVAL,
            };

            // Check preopens - only allow access through preopened directories
            let fs_state = env.data().fs_state.clone();
            let mut guard = fs_state.lock().unwrap();
            
            let full_path = if dirfd == 3 {
                // Use first preopen as root
                if let Some(preopen) = guard.preopens.first() {
                    preopen.join(path_str)
                } else {
                    return ERRNO_PERM;
                }
            } else {
                PathBuf::from(path_str)
            };

            // Security: ensure path is within a preopened directory
            let allowed = guard.preopens.iter().any(|preopen| {
                full_path.starts_with(preopen)
            });
            if !allowed {
                return ERRNO_PERM;
            }

            // Open the file
            let mut options = OpenOptions::new();
            
            // Parse o_flags (WASI O flags)
            let read = (o_flags & 1) != 0; // __WASI_OFLAGS_RDONLY
            let write = (o_flags & 2) != 0; // __WASI_OFLAGS_WRONLY
            let create = (o_flags & 4) != 0; // __WASI_OFLAGS_CREAT
            
            if read && !write {
                options.read(true);
            } else if write && !read {
                options.write(true).create(create).truncate(!create);
            } else {
                options.read(true).write(true).create(create).truncate(!create);
            }

            match options.open(&full_path) {
                Ok(file) => {
                    let fd = guard.alloc_fd();
                    guard.files.insert(fd, file);
                    view.write(fd_ptr as u64, &(fd as u32).to_le_bytes()).unwrap();
                    0
                }
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => ERRNO_NOENT,
                    std::io::ErrorKind::PermissionDenied => ERRNO_PERM,
                    _ => ERRNO_INVAL,
                }
            }
        });
    let path_readlink_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32, _buf_ptr: i32, _buf_len: i32, _bufused_ptr: i32| -> i32 { 28 });
    let path_remove_directory_fn = Function::new_typed(store, |_fd: i32, _path_ptr: i32, _path_len: i32| -> i32 { 28 });
    let path_rename_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, old_path_ptr: i32, old_path_len: i32, new_fd: i32, new_path_ptr: i32, new_path_len: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            
            // Read old path
            let mut old_bytes = vec![0u8; old_path_len as usize];
            view.read(old_path_ptr as u64, &mut old_bytes).unwrap();
            let old_path_str = match std::str::from_utf8(&old_bytes) {
                Ok(s) => s.trim_end_matches('\0'),
                Err(_) => return ERRNO_INVAL,
            };
            
            // Read new path
            let mut new_bytes = vec![0u8; new_path_len as usize];
            view.read(new_path_ptr as u64, &mut new_bytes).unwrap();
            let new_path_str = match std::str::from_utf8(&new_bytes) {
                Ok(s) => s.trim_end_matches('\0'),
                Err(_) => return ERRNO_INVAL,
            };
            
            // Resolve paths through preopens
            let fs_state = env.data().fs_state.clone();
            let guard = fs_state.lock().unwrap();
            
            let old_full = if fd == 3 {
                if let Some(preopen) = guard.preopens.first() {
                    preopen.join(old_path_str)
                } else {
                    return ERRNO_PERM;
                }
            } else {
                PathBuf::from(old_path_str)
            };
            
            let new_full = if new_fd == 3 {
                if let Some(preopen) = guard.preopens.first() {
                    preopen.join(new_path_str)
                } else {
                    return ERRNO_PERM;
                }
            } else {
                PathBuf::from(new_path_str)
            };
            
            // Security check
            let allowed_old = guard.preopens.iter().any(|p| old_full.starts_with(p));
            let allowed_new = guard.preopens.iter().any(|p| new_full.starts_with(p));
            if !allowed_old || !allowed_new {
                return ERRNO_PERM;
            }
            
            // Perform rename
            match std::fs::rename(&old_full, &new_full) {
                Ok(()) => 0,
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => ERRNO_NOENT,
                    std::io::ErrorKind::PermissionDenied => ERRNO_PERM,
                    _ => ERRNO_IO,
                }
            }
        });
    let path_symlink_fn = Function::new_typed(store, |_old_ptr: i32, _old_len: i32, _fd: i32, _new_ptr: i32, _new_len: i32| -> i32 { 28 });
    let path_unlink_file_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, path_ptr: i32, path_len: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            
            // Read path
            let mut path_bytes = vec![0u8; path_len as usize];
            view.read(path_ptr as u64, &mut path_bytes).unwrap();
            let path_str = match std::str::from_utf8(&path_bytes) {
                Ok(s) => s.trim_end_matches('\0'),
                Err(_) => return ERRNO_INVAL,
            };
            
            // Resolve through preopens
            let fs_state = env.data().fs_state.clone();
            let guard = fs_state.lock().unwrap();
            
            let full_path = if fd == 3 {
                if let Some(preopen) = guard.preopens.first() {
                    preopen.join(path_str)
                } else {
                    return ERRNO_PERM;
                }
            } else {
                PathBuf::from(path_str)
            };
            
            // Security check
            let allowed = guard.preopens.iter().any(|p| full_path.starts_with(p));
            if !allowed {
                return ERRNO_PERM;
            }
            
            // Delete file
            match std::fs::remove_file(&full_path) {
                Ok(()) => 0,
                Err(e) => match e.kind() {
                    std::io::ErrorKind::NotFound => ERRNO_NOENT,
                    std::io::ErrorKind::PermissionDenied => ERRNO_PERM,
                    _ => ERRNO_IO,
                }
            }
        });
    #[cfg(not(feature = "wasm-networking"))]
    let poll_oneoff_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _in_ptr: i32, _out_ptr: i32, _nsubscriptions: i32, _nevents_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            
            let in_ptr = _in_ptr;
            let out_ptr = _out_ptr;
            let nsubscriptions = _nsubscriptions;
            let nevents_ptr = _nevents_ptr;
            
            let sub_size: u64 = 48;
            let event_size: u64 = 32;
            let mut event_count: u32 = 0;
            
            for i in 0..nsubscriptions as u64 {
                let sub_base = (in_ptr as u64) + i * sub_size;
                let mut userdata_bytes = [0u8; 8];
                view.read(sub_base, &mut userdata_bytes).unwrap();
                let userdata = u64::from_le_bytes(userdata_bytes);
                let mut tag_bytes = [0u8; 1];
                view.read(sub_base + 8, &mut tag_bytes).unwrap();
                let tag = tag_bytes[0];
                let event_base = (out_ptr as u64) + (event_count as u64) * event_size;
                
                match tag {
                    0 => { // CLOCK subscription
                        let mut timeout_bytes = [0u8; 8];
                        view.read(sub_base + 24, &mut timeout_bytes).unwrap();
                        let timeout_ns = u64::from_le_bytes(timeout_bytes);
                        let mut flags_bytes = [0u8; 2];
                        view.read(sub_base + 40, &mut flags_bytes).unwrap();
                        let flags = u16::from_le_bytes(flags_bytes);
                        let is_relative = (flags & 1) != 0;
                        
                        if is_relative && timeout_ns > 0 {
                            std::thread::sleep(std::time::Duration::from_nanos(timeout_ns));
                        }
                        
                        view.write(event_base, &userdata.to_le_bytes()).unwrap();
                        view.write(event_base + 8, &0u16.to_le_bytes()).unwrap(); // error
                        view.write(event_base + 10, &0u8.to_le_bytes()).unwrap(); // type = clock
                        view.write(event_base + 16, &0u64.to_le_bytes()).unwrap(); // nbytes
                        view.write(event_base + 24, &0u16.to_le_bytes()).unwrap(); // flags
                        event_count += 1;
                    }
                    1 | 2 => { // FD_READ or FD_WRITE subscription
                        let mut fd_bytes = [0u8; 4];
                        view.read(sub_base + 16, &mut fd_bytes).unwrap();
                        let fd = i32::from_le_bytes(fd_bytes);
                        
                        // For file fds, check if they exist
                        let is_ready = if fd >= 3 {
                            let fs_state = env.data().fs_state.clone();
                            let guard = fs_state.lock().unwrap();
                            guard.files.contains_key(&fd)
                        } else {
                            fd == 0 || fd == 1 || fd == 2 // stdin/stdout/stderr always ready
                        };
                        
                        view.write(event_base, &userdata.to_le_bytes()).unwrap();
                        view.write(event_base + 8, &0u16.to_le_bytes()).unwrap(); // error
                        view.write(event_base + 10, &tag.to_le_bytes()).unwrap(); // type
                        let nbytes: u64 = if is_ready { 1 } else { 0 };
                        view.write(event_base + 16, &nbytes.to_le_bytes()).unwrap();
                        view.write(event_base + 24, &0u16.to_le_bytes()).unwrap(); // flags
                        event_count += 1;
                    }
                    _ => {}
                }
            }
            
            view.write(nevents_ptr as u64, &event_count.to_le_bytes()).unwrap();
            0
        });
    let sched_yield_fn = Function::new_typed(store, || -> i32 { 0 });
    #[cfg(not(feature = "wasm-networking"))]
    let sock_accept_fn = Function::new_typed(store, |_fd: i32, _flags: i32, _result_fd_ptr: i32| -> i32 { 28 });
    #[cfg(not(feature = "wasm-networking"))]
    let sock_recv_fn = Function::new_typed(store, |_fd: i32, _ri_data_ptr: i32, _ri_data_len: i32, _ri_flags: i32, _ro_datalen_ptr: i32, _ro_flags_ptr: i32| -> i32 { 28 });
    #[cfg(not(feature = "wasm-networking"))]
    let sock_send_fn = Function::new_typed(store, |_fd: i32, _si_data_ptr: i32, _si_data_len: i32, _si_flags: i32, _so_datalen_ptr: i32| -> i32 { 28 });
    #[cfg(not(feature = "wasm-networking"))]
    let sock_shutdown_fn = Function::new_typed(store, |_fd: i32, _how: i32| -> i32 { 28 });

    #[cfg(feature = "wasm-networking")]
    let poll_oneoff_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, _in_ptr: i32, _out_ptr: i32, _nsubscriptions: i32, _nevents_ptr: i32| -> i32 {
            use super::wasi_net::SOCKET_FD_BASE;
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            let state = env.data().socket_state.clone();
            let guard = state.lock().unwrap();

            let in_ptr = _in_ptr;
            let out_ptr = _out_ptr;
            let nsubscriptions = _nsubscriptions;
            let nevents_ptr = _nevents_ptr;

            let sub_size: u64 = 48;
            let event_size: u64 = 32;
            let mut event_count: u32 = 0;

            for i in 0..nsubscriptions as u64 {
                let sub_base = (in_ptr as u64) + i * sub_size;
                let mut userdata_bytes = [0u8; 8];
                view.read(sub_base, &mut userdata_bytes).unwrap();
                let userdata = u64::from_le_bytes(userdata_bytes);
                let mut tag_bytes = [0u8; 1];
                view.read(sub_base + 8, &mut tag_bytes).unwrap();
                let tag = tag_bytes[0];
                let event_base = (out_ptr as u64) + (event_count as u64) * event_size;

                match tag {
                    1 | 2 => {
                        let mut fd_bytes = [0u8; 4];
                        view.read(sub_base + 16, &mut fd_bytes).unwrap();
                        let fd = i32::from_le_bytes(fd_bytes);
                        let is_ready = fd < SOCKET_FD_BASE
                            || guard.is_socket_fd(fd)
                            || guard.is_listener_fd(fd);
                        view.write(event_base, &userdata.to_le_bytes()).unwrap();
                        view.write(event_base + 8, &0u16.to_le_bytes()).unwrap();
                        view.write(event_base + 10, &tag.to_le_bytes()).unwrap();
                        let nbytes: u64 = if is_ready { 1 } else { 0 };
                        view.write(event_base + 16, &nbytes.to_le_bytes()).unwrap();
                        view.write(event_base + 24, &0u16.to_le_bytes()).unwrap();
                        event_count += 1;
                    }
                    0 => {
                        let mut timeout_bytes = [0u8; 8];
                        view.read(sub_base + 24, &mut timeout_bytes).unwrap();
                        let timeout_ns = u64::from_le_bytes(timeout_bytes);
                        let mut flags_bytes = [0u8; 2];
                        view.read(sub_base + 40, &mut flags_bytes).unwrap();
                        let flags = u16::from_le_bytes(flags_bytes);
                        let is_relative = (flags & 1) != 0;
                        if is_relative && timeout_ns > 0 {
                            std::thread::sleep(std::time::Duration::from_nanos(timeout_ns));
                        }
                        view.write(event_base, &userdata.to_le_bytes()).unwrap();
                        view.write(event_base + 8, &0u16.to_le_bytes()).unwrap();
                        view.write(event_base + 10, &0u8.to_le_bytes()).unwrap();
                        view.write(event_base + 16, &0u64.to_le_bytes()).unwrap();
                        view.write(event_base + 24, &0u16.to_le_bytes()).unwrap();
                        event_count += 1;
                    }
                    _ => {}
                }
            }
            view.write(nevents_ptr as u64, &event_count.to_le_bytes()).unwrap();
            0
        });

    #[cfg(feature = "wasm-networking")]
    let sock_accept_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, _flags: i32, result_fd_ptr: i32| -> i32 {
            let mem = env.data().memory.as_ref().unwrap().clone();
            let state = env.data().socket_state.clone();
            let mut guard = state.lock().unwrap();
            let result = guard.accept_tcp(fd);
            drop(guard);
            match result {
                Ok(new_fd) => {
                    mem.view(&env).write(result_fd_ptr as u64, &new_fd.to_le_bytes()).unwrap();
                    0
                }
                Err(errno) => errno,
            }
        });

    #[cfg(feature = "wasm-networking")]
    let sock_recv_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, ri_data_ptr: i32, ri_data_len: i32, _ri_flags: i32, ro_datalen_ptr: i32, ro_flags_ptr: i32| -> i32 {
            use std::io::Read;
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            let state = env.data().socket_state.clone();
            let mut guard = state.lock().unwrap();
            let stream = match guard.sockets.get_mut(&fd) {
                Some(s) => s,
                None => return 8,
            };
            let mut total_read: u32 = 0;
            for i in 0..ri_data_len as u64 {
                let base = (ri_data_ptr as u64) + i * 8;
                let mut ptr_bytes = [0u8; 4];
                let mut len_bytes = [0u8; 4];
                view.read(base, &mut ptr_bytes).unwrap();
                view.read(base + 4, &mut len_bytes).unwrap();
                let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                let buf_len = u32::from_le_bytes(len_bytes) as usize;
                if buf_len > 0 {
                    let mut buf = vec![0u8; buf_len];
                    match stream.read(&mut buf) {
                        Ok(n) => {
                            if n > 0 { view.write(buf_ptr, &buf[..n]).unwrap(); }
                            total_read += n as u32;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => return 62,
                    }
                }
            }
            view.write(ro_datalen_ptr as u64, &total_read.to_le_bytes()).unwrap();
            view.write(ro_flags_ptr as u64, &0u32.to_le_bytes()).unwrap();
            0
        });

    #[cfg(feature = "wasm-networking")]
    let sock_send_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, si_data_ptr: i32, si_data_len: i32, _si_flags: i32, so_datalen_ptr: i32| -> i32 {
            use std::io::Write;
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            let state = env.data().socket_state.clone();
            let mut guard = state.lock().unwrap();
            let stream = match guard.sockets.get_mut(&fd) {
                Some(s) => s,
                None => return 8,
            };
            let mut total_sent: u32 = 0;
            for i in 0..si_data_len as u64 {
                let base = (si_data_ptr as u64) + i * 8;
                let mut ptr_bytes = [0u8; 4];
                let mut len_bytes = [0u8; 4];
                view.read(base, &mut ptr_bytes).unwrap();
                view.read(base + 4, &mut len_bytes).unwrap();
                let buf_ptr = u32::from_le_bytes(ptr_bytes) as u64;
                let buf_len = u32::from_le_bytes(len_bytes) as usize;
                if buf_len > 0 {
                    let mut data = vec![0u8; buf_len];
                    view.read(buf_ptr, &mut data).unwrap();
                    match stream.write(&data) {
                        Ok(n) => total_sent += n as u32,
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(_) => return 62,
                    }
                }
            }
            view.write(so_datalen_ptr as u64, &total_sent.to_le_bytes()).unwrap();
            0
        });

    #[cfg(feature = "wasm-networking")]
    let sock_shutdown_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, fd: i32, how: i32| -> i32 {
            let state = env.data().socket_state.clone();
            let mut guard = state.lock().unwrap();
            let stream = match guard.sockets.get_mut(&fd) {
                Some(s) => s,
                None => return 8,
            };
            use std::net::Shutdown;
            let shutdown_how = match how {
                0 => Shutdown::Read,
                1 => Shutdown::Write,
                2 => Shutdown::Both,
                _ => return 8,
            };
            match stream.shutdown(shutdown_how) {
                Ok(()) => 0,
                Err(_) => 62,
            }
        });

    #[cfg(feature = "wasm-networking")]
    let tcp_connect_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, host_ptr: i32, host_len: i32, port: i32| -> i32 {
            use super::wasi_net::ERRNO_BADF;
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            let mut host_bytes = vec![0u8; host_len as usize];
            view.read(host_ptr as u64, &mut host_bytes).unwrap();
            let host = match String::from_utf8(host_bytes) {
                Ok(s) => s,
                Err(_) => return -(ERRNO_BADF as i32),
            };
            let state = env.data().socket_state.clone();
            let mut guard = state.lock().unwrap();
            let result = guard.connect_tcp(&host, port as u16);
            drop(guard);
            match result {
                Ok(fd) => fd,
                Err(errno) => -(errno as i32),
            }
        });

    #[cfg(feature = "wasm-networking")]
    let tcp_listen_fn = Function::new_typed_with_env(store, env,
        |env: FunctionEnvMut<WasiEnv>, host_ptr: i32, host_len: i32, port: i32| -> i32 {
            use super::wasi_net::ERRNO_BADF;
            let mem = env.data().memory.as_ref().unwrap().clone();
            let view = mem.view(&env);
            let mut host_bytes = vec![0u8; host_len as usize];
            view.read(host_ptr as u64, &mut host_bytes).unwrap();
            let host = match String::from_utf8(host_bytes) {
                Ok(s) => s,
                Err(_) => return -(ERRNO_BADF as i32),
            };
            let state = env.data().socket_state.clone();
            let mut guard = state.lock().unwrap();
            let result = guard.listen_tcp(&host, port as u16);
            drop(guard);
            match result {
                Ok(fd) => fd,
                Err(errno) => -(errno as i32),
            }
        });

    #[allow(unused_mut)]
    let mut imports = imports! {
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
    };

    #[cfg(feature = "wasm-networking")]
    {
        imports.define("velocity_net", "tcp_connect", tcp_connect_fn);
        imports.define("velocity_net", "tcp_listen", tcp_listen_fn);
    }

    imports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stdin_buffer_push_and_read() {
        let env = WasiEnv::new();
        
        // Push data into stdin buffer
        env.push_stdin(b"hello world");
        
        // Verify data is in buffer
        let buf = env.stdin_buffer.lock().unwrap();
        assert_eq!(buf.len(), 11);
        assert_eq!(&buf.iter().copied().collect::<Vec<_>>()[..], b"hello world");
    }

    #[test]
    fn test_stdin_buffer_empty() {
        let env = WasiEnv::new();
        
        // Buffer should be empty initially
        let buf = env.stdin_buffer.lock().unwrap();
        assert_eq!(buf.len(), 0);
    }

    #[test]
    fn test_stdin_buffer_multiple_pushes() {
        let env = WasiEnv::new();
        
        env.push_stdin(b"first");
        env.push_stdin(b" second");
        env.push_stdin(b" third");
        
        let buf = env.stdin_buffer.lock().unwrap();
        assert_eq!(buf.len(), 18); // 5 + 7 + 6
    }
}
