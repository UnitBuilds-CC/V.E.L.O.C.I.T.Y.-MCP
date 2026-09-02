use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::path::Path;
use std::error::Error;
use std::sync::atomic::{AtomicU8, Ordering};

const STATE_OFFSET: usize = 0;
const INPUT_LEN_OFFSET: usize = 1;
const OUTPUT_LEN_OFFSET: usize = 5;
const REQUEST_SEQ_OFFSET: usize = 9;
const INPUT_BUFFER_OFFSET: usize = 16;
const OUTPUT_BUFFER_OFFSET: usize = 4096;
const TOTAL_BUFFER_SIZE: usize = 65536;

pub const STATE_IDLE: u8 = 0;
pub const STATE_REQ_READY: u8 = 1;
pub const STATE_PROCESSING: u8 = 2;
pub const STATE_RES_READY: u8 = 3;
pub const STATE_ERROR: u8 = 4;

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
}

#[cfg(target_os = "windows")]
#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(uPeriod: u32) -> u32;
    fn timeEndPeriod(uPeriod: u32) -> u32;
}

/// Improve Windows timer resolution from 15.6ms to 1ms for low-latency event waits.
#[cfg(target_os = "windows")]
pub fn enable_high_resolution_timer() {
    unsafe { timeBeginPeriod(1); }
}

#[cfg(not(target_os = "windows"))]
pub fn enable_high_resolution_timer() {}

/// Restore default Windows timer resolution.
#[cfg(target_os = "windows")]
pub fn disable_high_resolution_timer() {
    unsafe { timeEndPeriod(1); }
}

#[cfg(not(target_os = "windows"))]
pub fn disable_high_resolution_timer() {}

#[cfg(target_os = "windows")]
const INFINITE: u32 = 0xFFFFFFFF;

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
pub struct SharedMemoryBuffer {
    mmap: MmapMut,
    h_req_event: *mut std::ffi::c_void,
    h_res_event: *mut std::ffi::c_void,
}

#[cfg(not(target_os = "windows"))]
pub struct SharedMemoryBuffer {
    mmap: MmapMut,
}

impl SharedMemoryBuffer {
    #[cfg(target_os = "windows")]
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let file_name = path.as_ref().file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default");

        let req_event_name = format!("Global\\VELOCITY_NMCP_REQ_{}", file_name);
        let res_event_name = format!("Global\\VELOCITY_NMCP_RES_{}", file_name);

        let w_req = to_wstring(&req_event_name);
        let w_res = to_wstring(&res_event_name);

        // SAFETY: CreateEventW with null security attrs, manual reset, null-terminated names.
        let h_req_event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, w_req.as_ptr()) };
        if h_req_event.is_null() {
            return Err("Failed to create req event".into());
        }

        let h_res_event = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, w_res.as_ptr()) };
        if h_res_event.is_null() {
            unsafe { CloseHandle(h_req_event); }
            return Err("Failed to create res event".into());
        }

        let mut buffer = SharedMemoryBuffer { mmap, h_req_event, h_res_event };

        // Always reset to clean state on startup. If the previous server
        // crashed mid-request, the buffer may be left in REQ_READY,
        // PROCESSING, RES_READY, or ERROR state. Any in-flight request
        // is lost, so we must start fresh.
        buffer.set_input_len(0);
        buffer.set_output_len(0);
        buffer.set_state(STATE_IDLE);

        Ok(buffer)
    }

    #[cfg(not(target_os = "windows"))]
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;

        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };

        let mut buffer = SharedMemoryBuffer { mmap };

        // Always reset to clean state on startup (see Windows path for rationale).
        buffer.set_input_len(0);
        buffer.set_output_len(0);
        buffer.set_state(STATE_IDLE);

        Ok(buffer)
    }

    /// Block until a request is available. Hybrid spin-wait on Windows:
    /// spin-polls the state byte for STATE_REQ_READY (avoids ~2.7µs event
    /// wake cost), falls back to WaitForSingleObject when budget expires.
    /// Set VELOCITY_SPIN_US=0 to disable spin and use events only.
    #[cfg(target_os = "windows")]
    pub fn wait_for_request(&self) {
        let budget = spin_budget_us();
        if budget > 0 {
            let start = std::time::Instant::now();
            let limit = std::time::Duration::from_micros(budget);
            loop {
                if self.get_state() == STATE_REQ_READY {
                    return;
                }
                if start.elapsed() >= limit {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        // SAFETY: h_req_event is a valid event handle from CreateEventW.
        unsafe { WaitForSingleObject(self.h_req_event, INFINITE); }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn wait_for_request(&self) {
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    /// Signal that a response is ready for the host to read.
    #[cfg(target_os = "windows")]
    pub fn signal_response(&self) {
        // SAFETY: h_res_event is a valid event handle from CreateEventW.
        unsafe { SetEvent(self.h_res_event); }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn signal_response(&self) {}

    /// Signal that a request is ready for the server to read (used by host).
    #[cfg(target_os = "windows")]
    pub fn signal_request(&self) {
        // SAFETY: h_req_event is a valid event handle from CreateEventW.
        unsafe { SetEvent(self.h_req_event); }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn signal_request(&self) {}

    /// Block until a response is available (used by host). Hybrid spin-wait:
    /// polls state byte for STATE_RES_READY before falling back to event.
    #[cfg(target_os = "windows")]
    pub fn wait_for_response(&self) {
        let budget = spin_budget_us();
        if budget > 0 {
            let start = std::time::Instant::now();
            let limit = std::time::Duration::from_micros(budget);
            loop {
                if self.get_state() == STATE_RES_READY {
                    return;
                }
                if start.elapsed() >= limit {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        // SAFETY: h_res_event is a valid event handle from CreateEventW.
        unsafe { WaitForSingleObject(self.h_res_event, INFINITE); }
    }

    #[cfg(not(target_os = "windows"))]
    pub fn wait_for_response(&self) {
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    pub fn get_state(&self) -> u8 {
        unsafe {
            let ptr = self.mmap.as_ptr().add(STATE_OFFSET) as *const AtomicU8;
            (*ptr).load(Ordering::Acquire)
        }
    }

    pub fn set_state(&mut self, state: u8) {
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(STATE_OFFSET) as *mut AtomicU8;
            (*ptr).store(state, Ordering::Release)
        }
    }

    pub fn get_input_len(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[INPUT_LEN_OFFSET],
            self.mmap[INPUT_LEN_OFFSET + 1],
            self.mmap[INPUT_LEN_OFFSET + 2],
            self.mmap[INPUT_LEN_OFFSET + 3],
        ])
    }

    pub fn set_input_len(&mut self, len: u32) {
        let bytes = len.to_le_bytes();
        self.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4].copy_from_slice(&bytes);
    }

    pub fn get_output_len(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[OUTPUT_LEN_OFFSET],
            self.mmap[OUTPUT_LEN_OFFSET + 1],
            self.mmap[OUTPUT_LEN_OFFSET + 2],
            self.mmap[OUTPUT_LEN_OFFSET + 3],
        ])
    }

    pub fn set_output_len(&mut self, len: u32) {
        let bytes = len.to_le_bytes();
        self.mmap[OUTPUT_LEN_OFFSET..OUTPUT_LEN_OFFSET + 4].copy_from_slice(&bytes);
    }

    pub fn get_request_seq(&self) -> u32 {
        u32::from_le_bytes([
            self.mmap[REQUEST_SEQ_OFFSET],
            self.mmap[REQUEST_SEQ_OFFSET + 1],
            self.mmap[REQUEST_SEQ_OFFSET + 2],
            self.mmap[REQUEST_SEQ_OFFSET + 3],
        ])
    }

    pub fn set_request_seq(&mut self, seq: u32) {
        let bytes = seq.to_le_bytes();
        self.mmap[REQUEST_SEQ_OFFSET..REQUEST_SEQ_OFFSET + 4].copy_from_slice(&bytes);
    }

    pub fn read_input(&self) -> Result<String, Box<dyn Error>> {
        std::sync::atomic::fence(Ordering::SeqCst);
        let len = self.get_input_len() as usize;
        if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Input length exceeds buffer limit".into());
        }
        let bytes = &self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len];
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    pub fn read_input_raw(&self) -> Result<Vec<u8>, Box<dyn Error>> {
        std::sync::atomic::fence(Ordering::SeqCst);
        let len = self.get_input_len() as usize;
        if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Input length exceeds buffer limit".into());
        }
        Ok(self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len].to_vec())
    }

    pub fn write_output_raw(&mut self, data: &[u8]) -> Result<(), Box<dyn Error>> {
        if data.len() > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Response length exceeds output buffer limit".into());
        }
        self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + data.len()].copy_from_slice(data);
        self.set_output_len(data.len() as u32);
        Ok(())
    }

    pub fn read_output(&self) -> Result<String, Box<dyn Error>> {
        std::sync::atomic::fence(Ordering::SeqCst);
        let len = self.get_output_len() as usize;
        if len > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Output length exceeds buffer limit".into());
        }
        let bytes = &self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + len];
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    pub fn write_input(&mut self, request: &str) -> Result<(), Box<dyn Error>> {
        let bytes = request.as_bytes();
        if bytes.len() > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Request length exceeds input buffer limit".into());
        }
        self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);
        self.set_input_len(bytes.len() as u32);
        Ok(())
    }

    pub fn write_output(&mut self, response: &str) -> Result<(), Box<dyn Error>> {
        let bytes = response.as_bytes();
        if bytes.len() > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
            return Err("Response length exceeds output buffer limit".into());
        }
        self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);
        self.set_output_len(bytes.len() as u32);
        Ok(())
    }

    pub fn flush(&self) -> Result<(), Box<dyn Error>> {
        self.mmap.flush()?;
        Ok(())
    }

    /// Async flush - marks pages dirty for cross-process visibility without blocking on disk I/O.
    pub fn flush_async(&self) -> Result<(), Box<dyn Error>> {
        self.mmap.flush_async()?;
        Ok(())
    }

    pub fn sync_fence() {
        std::sync::atomic::fence(Ordering::SeqCst);
    }
}

#[cfg(target_os = "windows")]
impl Drop for SharedMemoryBuffer {
    fn drop(&mut self) {
        // SAFETY: handles created by CreateEventW, checked for null at creation.
        unsafe {
            if !self.h_req_event.is_null() {
                CloseHandle(self.h_req_event);
            }
            if !self.h_res_event.is_null() {
                CloseHandle(self.h_res_event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_buffer_path(name: &str) -> String {
        format!("test_shmem_{}.bin", name)
    }

    fn cleanup(path: &str) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_create_or_open_initializes_to_idle() {
        let path = temp_buffer_path("init");
        cleanup(&path);
        let buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();
        assert_eq!(buffer.get_state(), STATE_IDLE);
        cleanup(&path);
    }

    #[test]
    fn test_state_transitions_with_atomic_ordering() {
        let path = temp_buffer_path("states");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        for state in [STATE_IDLE, STATE_REQ_READY, STATE_PROCESSING, STATE_RES_READY, STATE_ERROR] {
            buffer.set_state(state);
            buffer.flush().unwrap();
            assert_eq!(buffer.get_state(), state);
        }
        cleanup(&path);
    }

    #[test]
    fn test_write_and_read_output() {
        let path = temp_buffer_path("output");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let response = r#"{"jsonrpc":"2.0","result":{"tools":[]},"id":1}"#;
        buffer.write_output(response).unwrap();
        buffer.flush().unwrap();

        assert_eq!(buffer.get_output_len(), response.len() as u32);
        let read_back = buffer.read_output().unwrap();
        assert_eq!(read_back, response);
        cleanup(&path);
    }

    #[test]
    fn test_write_and_read_input() {
        let path = temp_buffer_path("input");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let request = r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#;
        buffer.write_input(request).unwrap();
        buffer.flush().unwrap();

        let read_back = buffer.read_input().unwrap();
        assert_eq!(read_back, request);
        cleanup(&path);
    }

    #[test]
    fn test_input_length_overflow_rejected() {
        let path = temp_buffer_path("overflow");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        buffer.set_input_len(OUTPUT_BUFFER_OFFSET as u32);
        let result = buffer.read_input();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds buffer limit"));
        cleanup(&path);
    }

    #[test]
    fn test_output_length_overflow_rejected() {
        let path = temp_buffer_path("out_overflow");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let max_output = TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET;
        let oversized = "x".repeat(max_output + 1);
        let result = buffer.write_output(&oversized);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds output buffer limit"));
        cleanup(&path);
    }

    #[test]
    fn test_create_or_open_resets_stale_state() {
        let path = temp_buffer_path("persist");
        cleanup(&path);

        {
            let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();
            buffer.set_state(STATE_RES_READY);
            buffer.flush().unwrap();
        }

        {
            let buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();
            assert_eq!(buffer.get_state(), STATE_IDLE);
        }
        cleanup(&path);
    }

    #[test]
    fn test_full_request_response_cycle() {
        let path = temp_buffer_path("cycle");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"test.nda"}},"id":1}"#;
        buffer.write_input(request).unwrap();
        buffer.set_state(STATE_REQ_READY);
        buffer.flush().unwrap();

        assert_eq!(buffer.get_state(), STATE_REQ_READY);
        let input = buffer.read_input().unwrap();
        assert_eq!(input, request);

        buffer.set_state(STATE_PROCESSING);
        buffer.flush().unwrap();

        let response = r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"OK"}]},"id":1}"#;
        buffer.write_output(response).unwrap();
        buffer.set_state(STATE_RES_READY);
        buffer.flush().unwrap();

        assert_eq!(buffer.get_state(), STATE_RES_READY);
        assert_eq!(buffer.get_output_len(), response.len() as u32);
        let read_back = buffer.read_output().unwrap();
        assert_eq!(read_back, response);
        cleanup(&path);
    }

    #[test]
    fn test_read_input_raw_returns_bytes() {
        let path = temp_buffer_path("raw_input");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let request = r#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
        buffer.write_input(request).unwrap();

        let raw = buffer.read_input_raw().unwrap();
        assert_eq!(raw, request.as_bytes());
        cleanup(&path);
    }

    #[test]
    fn test_read_input_raw_overflow_rejected() {
        let path = temp_buffer_path("raw_overflow");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        buffer.set_input_len(OUTPUT_BUFFER_OFFSET as u32);
        let result = buffer.read_input_raw();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds buffer limit"));
        cleanup(&path);
    }

    #[test]
    fn test_write_and_read_output_raw() {
        let path = temp_buffer_path("raw_output");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let data = b"binary data test 123";
        buffer.write_output_raw(data).unwrap();

        assert_eq!(buffer.get_output_len(), data.len() as u32);
        let read_back = buffer.read_output().unwrap();
        assert_eq!(read_back.as_bytes(), data);
        cleanup(&path);
    }

    #[test]
    fn test_write_output_raw_overflow_rejected() {
        let path = temp_buffer_path("raw_out_overflow");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let max_output = TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET;
        let oversized = vec![0u8; max_output + 1];
        let result = buffer.write_output_raw(&oversized);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds output buffer limit"));
        cleanup(&path);
    }

    #[test]
    fn test_write_input_overflow_rejected() {
        let path = temp_buffer_path("input_overflow");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let max_input = OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET;
        let oversized = "x".repeat(max_input + 1);
        let result = buffer.write_input(&oversized);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds input buffer limit"));
        cleanup(&path);
    }

    #[test]
    fn test_request_seq_get_set() {
        let path = temp_buffer_path("seq");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        assert_eq!(buffer.get_request_seq(), 0);

        buffer.set_request_seq(42);
        assert_eq!(buffer.get_request_seq(), 42);

        buffer.set_request_seq(u32::MAX);
        assert_eq!(buffer.get_request_seq(), u32::MAX);
        cleanup(&path);
    }

    #[test]
    fn test_flush_async_succeeds() {
        let path = temp_buffer_path("flush_async");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        buffer.write_output("test").unwrap();
        buffer.flush_async().unwrap();
        cleanup(&path);
    }

    #[test]
    fn test_sync_fence_does_not_panic() {
        SharedMemoryBuffer::sync_fence();
    }
}
