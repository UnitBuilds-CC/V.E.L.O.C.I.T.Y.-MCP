//! Client-side shared memory buffer for VELOCITY-MCP IPC.
//!
//! Opens a file-backed mmap and Win32 auto-reset events to communicate
//! with a VELOCITY-MCP server over shared memory.

use crate::error::{Error, Result};
use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::sync::atomic::{AtomicU8, Ordering};

const STATE_OFFSET: usize = 0;
const INPUT_LEN_OFFSET: usize = 1;
const OUTPUT_LEN_OFFSET: usize = 5;
const REQUEST_SEQ_OFFSET: usize = 9;
const INPUT_BUFFER_OFFSET: usize = 16;
pub(crate) const OUTPUT_BUFFER_OFFSET: usize = 4096;
pub(crate) const TOTAL_BUFFER_SIZE: usize = 65536;

const STATE_IDLE: u8 = 0;
const STATE_REQ_READY: u8 = 1;
#[allow(dead_code)]
const STATE_PROCESSING: u8 = 2;
const STATE_RES_READY: u8 = 3;
const STATE_ERROR: u8 = 4;

const WAIT_TIMEOUT_MS: u32 = 10_000;
const WAIT_TIMEOUT: u32 = 0x00000102;
#[allow(dead_code)]
const WAIT_FAILED: u32 = 0xFFFFFFFF;

#[cfg(target_os = "windows")]
extern "system" {
    fn CreateEventW(
        lpEventAttributes: *mut std::ffi::c_void,
        bManualReset: i32,
        bInitialState: i32,
        lpName: *const u16,
    ) -> *mut std::ffi::c_void;
    fn SetEvent(hEvent: *mut std::ffi::c_void) -> i32;
    fn WaitForSingleObject(hHandle: *mut std::ffi::c_void, dwMilliseconds: u32) -> u32;
    fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    fn GetLastError() -> u32;
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

#[cfg(target_os = "windows")]
fn spin_budget_us() -> u64 {
    std::env::var("VELOCITY_SPIN_US")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

#[cfg(target_os = "windows")]
fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(target_os = "windows")]
fn enable_high_resolution_timer() {
    unsafe { timeBeginPeriod(1); }
}

#[cfg(target_os = "windows")]
fn disable_high_resolution_timer() {
    unsafe { timeEndPeriod(1); }
}

pub(crate) struct ShmemBuffer {
    mmap: MmapMut,
    next_seq: u32,
    #[cfg(target_os = "windows")]
    h_req_event: *mut std::ffi::c_void,
    #[cfg(target_os = "windows")]
    h_res_event: *mut std::ffi::c_void,
}

impl ShmemBuffer {
    pub(crate) fn open(buffer_path: &str) -> Result<Self> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = buffer_path;
            return Err(Error::PlatformUnsupported(
                "Shared memory transport is only supported on Windows".into(),
            ));
        }

        #[cfg(target_os = "windows")]
        {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(buffer_path)
                .map_err(|e| Error::SharedMemory(format!("Failed to open buffer '{}': {}", buffer_path, e)))?;

            let file_len = file.metadata()
                .map_err(|e| Error::SharedMemory(format!("Failed to stat buffer '{}': {}", buffer_path, e)))?
                .len();
            if (file_len as usize) < TOTAL_BUFFER_SIZE {
                return Err(Error::SharedMemory(format!(
                    "Buffer file '{}' is {} bytes, expected at least {}",
                    buffer_path, file_len, TOTAL_BUFFER_SIZE
                )));
            }

            let mmap = unsafe {
                MmapMut::map_mut(&file)
                    .map_err(|e| Error::SharedMemory(format!("Failed to mmap buffer: {}", e)))?
            };

            let stem = std::path::Path::new(buffer_path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| Error::SharedMemory("Invalid buffer path".into()))?;

            let req_name = format!("Global\\VELOCITY_NMCP_REQ_{}", stem);
            let res_name = format!("Global\\VELOCITY_NMCP_RES_{}", stem);

            let w_req = to_wstring(&req_name);
            let w_res = to_wstring(&res_name);

            let h_req = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, w_req.as_ptr()) };
            if h_req.is_null() {
                let err = unsafe { GetLastError() };
                return Err(Error::SharedMemory(format!(
                    "Failed to create req event '{}' (error {})", req_name, err
                )));
            }

            let h_res = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, w_res.as_ptr()) };
            if h_res.is_null() {
                let err = unsafe { GetLastError() };
                unsafe { CloseHandle(h_req); }
                return Err(Error::SharedMemory(format!(
                    "Failed to create res event '{}' (error {})", res_name, err
                )));
            }

            enable_high_resolution_timer();

            Ok(ShmemBuffer {
                mmap,
                next_seq: 1,
                h_req_event: h_req,
                h_res_event: h_res,
            })
        }
    }

    pub(crate) fn send_raw(&mut self, data: &[u8]) -> Result<Vec<u8>> {
        #[cfg(not(target_os = "windows"))]
        {
            let _ = data;
            return Err(Error::PlatformUnsupported("shmem requires Windows".into()));
        }

        #[cfg(target_os = "windows")]
        {
            let max_input = OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET;
            if data.len() > max_input {
                return Err(Error::SharedMemory(format!(
                    "Request {} bytes exceeds input buffer limit {}",
                    data.len(),
                    max_input
                )));
            }

            self.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4]
                .copy_from_slice(&(data.len() as u32).to_le_bytes());
            self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + data.len()]
                .copy_from_slice(data);

            let sent_seq = self.next_seq;
            self.next_seq = self.next_seq.wrapping_add(1);
            self.mmap[REQUEST_SEQ_OFFSET..REQUEST_SEQ_OFFSET + 4]
                .copy_from_slice(&sent_seq.to_le_bytes());

            let _ = self.mmap.flush_async();
            std::sync::atomic::fence(Ordering::SeqCst);

            unsafe {
                let ptr = self.mmap.as_mut_ptr().add(STATE_OFFSET) as *mut AtomicU8;
                (*ptr).store(STATE_REQ_READY, Ordering::Release);
            }

            unsafe {
                SetEvent(self.h_req_event);
            }

            match self.wait_for_response() {
                Ok(()) => {}
                Err(e) => {
                    self.cleanup_after_timeout();
                    return Err(e);
                }
            }

            let state = unsafe {
                let ptr = self.mmap.as_ptr().add(STATE_OFFSET) as *const AtomicU8;
                (*ptr).load(Ordering::Acquire)
            };

            if state == STATE_ERROR {
                self.reset_to_idle();
                let out_len = self.read_output_len();
                if out_len > 0 && out_len <= TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET {
                    let err_bytes = &self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + out_len];
                    if let Ok(err_str) = std::str::from_utf8(err_bytes) {
                        return Err(Error::SharedMemory(format!("Server error: {}", err_str)));
                    }
                }
                return Err(Error::SharedMemory("Server returned STATE_ERROR".into()));
            }

            unsafe {
                WaitForSingleObject(self.h_res_event, 0);
            }

            self.reset_to_idle();

            std::sync::atomic::fence(Ordering::SeqCst);
            let out_len = self.read_output_len();

            if out_len > TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET {
                return Err(Error::SharedMemory(format!(
                    "Response length {} exceeds output buffer",
                    out_len
                )));
            }

            if out_len == 0 {
                return Err(Error::SharedMemory(
                    "Server returned empty response (out_len=0)".into(),
                ));
            }

            let echoed_seq = u32::from_le_bytes([
                self.mmap[REQUEST_SEQ_OFFSET],
                self.mmap[REQUEST_SEQ_OFFSET + 1],
                self.mmap[REQUEST_SEQ_OFFSET + 2],
                self.mmap[REQUEST_SEQ_OFFSET + 3],
            ]);
            if echoed_seq != sent_seq {
                return Err(Error::StaleResponse(format!(
                    "Expected seq {}, got {}", sent_seq, echoed_seq
                )));
            }

            Ok(self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + out_len].to_vec())
        }
    }

    #[cfg(target_os = "windows")]
    fn read_output_len(&self) -> usize {
        u32::from_le_bytes([
            self.mmap[OUTPUT_LEN_OFFSET],
            self.mmap[OUTPUT_LEN_OFFSET + 1],
            self.mmap[OUTPUT_LEN_OFFSET + 2],
            self.mmap[OUTPUT_LEN_OFFSET + 3],
        ]) as usize
    }

    #[cfg(target_os = "windows")]
    fn reset_to_idle(&mut self) {
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(STATE_OFFSET) as *mut AtomicU8;
            (*ptr).store(STATE_IDLE, Ordering::Release);
        }
    }

    #[cfg(target_os = "windows")]
    fn cleanup_after_timeout(&mut self) {
        unsafe {
            WaitForSingleObject(self.h_res_event, 0);
        }
        self.reset_to_idle();
    }

    #[cfg(target_os = "windows")]
    fn wait_for_response(&self) -> Result<()> {
        let budget = spin_budget_us();
        if budget > 0 {
            let start = std::time::Instant::now();
            let limit = std::time::Duration::from_micros(budget);
            loop {
                let state = unsafe {
                    let ptr = self.mmap.as_ptr().add(STATE_OFFSET) as *const AtomicU8;
                    (*ptr).load(Ordering::Acquire)
                };
                if state == STATE_RES_READY || state == STATE_ERROR {
                    return Ok(());
                }
                if start.elapsed() >= limit {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        unsafe {
            let rc = WaitForSingleObject(self.h_res_event, WAIT_TIMEOUT_MS);
            if rc == WAIT_TIMEOUT {
                return Err(Error::Timeout);
            }
            if rc != 0 {
                let err = GetLastError();
                return Err(Error::SharedMemory(format!(
                    "WaitForSingleObject failed (rc={}, error={})", rc, err
                )));
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
unsafe impl Send for ShmemBuffer {}

#[cfg(target_os = "windows")]
impl Drop for ShmemBuffer {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.h_req_event);
            CloseHandle(self.h_res_event);
            disable_high_resolution_timer();
        }
    }
}
