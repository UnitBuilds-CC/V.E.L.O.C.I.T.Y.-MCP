use memmap2::MmapMut;
use std::fs::OpenOptions;
use std::path::Path;
use std::error::Error;
use std::sync::atomic::{AtomicU8, Ordering};

// Shared Memory layout specs:
// Offset 0: State byte (0 = Idle, 1 = Host Request, 2 = Server Processing, 3 = Host Response Ready, 4 = Error)
// Offset 1..5: Input buffer length (u32, little endian)
// Offset 5..9: Output buffer length (u32, little endian)
// Offset 10..4096: Input request buffer
// Offset 4096..65536: Output response buffer (supports up to 61KB responses)

const STATE_OFFSET: usize = 0;
const INPUT_LEN_OFFSET: usize = 1;
const OUTPUT_LEN_OFFSET: usize = 5;
const INPUT_BUFFER_OFFSET: usize = 10;
const OUTPUT_BUFFER_OFFSET: usize = 4096;
const TOTAL_BUFFER_SIZE: usize = 65536;

/// Buffer is idle, no pending request or response.
pub const STATE_IDLE: u8 = 0;
/// Host has written a request and is waiting for the server to process it.
pub const STATE_REQ_READY: u8 = 1;
/// Server is actively processing the request.
pub const STATE_PROCESSING: u8 = 2;
/// Server has written the response and the host can read it.
pub const STATE_RES_READY: u8 = 3;
/// An error occurred during processing.
pub const STATE_ERROR: u8 = 4;

/// A 64 KB memory-mapped shared memory buffer for cross-process IPC.
///
/// Uses a state machine protocol with atomic ordering guarantees:
/// - State byte at offset 0 uses `AtomicU8` with Acquire/Release ordering.
/// - Length fields at offsets 1–4 and 5–8 use plain byte reads/writes
///   synchronized via `SeqCst` fences (see [`Self::sync_fence`]).
/// - Input buffer at offset 10 (4086 bytes max).
/// - Output buffer at offset 4096 (61440 bytes max).
pub struct SharedMemoryBuffer {
    mmap: MmapMut,
}

impl SharedMemoryBuffer {
    /// Create or open a shared memory buffer at the given file path.
    ///
    /// If the file does not exist, it is created and initialized to [`STATE_IDLE`].
    /// If it already exists, it is opened without truncation, preserving any
    /// existing state and data from a previous session.
    pub fn create_or_open<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn Error>> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false) // Preserve existing buffer contents on re-open
            .open(path)?;

        // Ensure the file is at least our desired size
        file.set_len(TOTAL_BUFFER_SIZE as u64)?;

        let mmap = unsafe { MmapMut::map_mut(&file)? };
        
        let mut buffer = SharedMemoryBuffer { mmap };
        
        // Initialize state to idle if it was just created
        if buffer.get_state() == 0 && buffer.get_input_len() == 0 {
            buffer.set_state(STATE_IDLE);
        }

        Ok(buffer)
    }

    /// Read the state byte with Acquire ordering.
    ///
    /// Acquire ensures that all subsequent reads (e.g., reading the input buffer)
    /// observe writes that happened before the Release store on the other side.
    /// This prevents the CPU from reordering data reads before the state check.
    ///
    /// # Safety
    /// Uses a pointer cast from `*const u8` (mmap base) to `*const AtomicU8`.
    /// This is safe because:
    /// - The mmap region is at least `TOTAL_BUFFER_SIZE` bytes (64 KB).
    /// - `AtomicU8` has the same size and layout as `u8` (no alignment requirement).
    /// - The state byte at offset 0 is always within the mapped region.
    pub fn get_state(&self) -> u8 {
        unsafe {
            let ptr = self.mmap.as_ptr().add(STATE_OFFSET) as *const AtomicU8;
            (*ptr).load(Ordering::Acquire)
        }
    }
    
    /// Write the state byte with Release ordering.
    ///
    /// Release ensures that all preceding writes (e.g., writing the output buffer)
    /// are visible to the other side before they observe the new state via Acquire.
    /// This prevents the CPU from reordering the state write before data writes.
    ///
    /// # Safety
    /// Same pointer cast rationale as `get_state`. The `*mut AtomicU8` cast is safe
    /// because `AtomicU8` is layout-compatible with `u8` and the offset is in-bounds.
    pub fn set_state(&mut self, state: u8) {
        unsafe {
            let ptr = self.mmap.as_mut_ptr().add(STATE_OFFSET) as *mut AtomicU8;
            (*ptr).store(state, Ordering::Release)
        }
    }

    /// Read the input buffer length field (u32, little-endian) as plain bytes.
    /// Must be called after a SeqCst fence to observe the writer's value.
    pub fn get_input_len(&self) -> u32 {
        // Length fields use plain byte reads with SeqCst fences for cross-process
        // synchronization (see set_output_len for full rationale).
        u32::from_le_bytes([
            self.mmap[INPUT_LEN_OFFSET],
            self.mmap[INPUT_LEN_OFFSET + 1],
            self.mmap[INPUT_LEN_OFFSET + 2],
            self.mmap[INPUT_LEN_OFFSET + 3],
        ])
    }

    /// Write the input buffer length field (u32, little-endian) as plain bytes.
    pub fn set_input_len(&mut self, len: u32) {
        let bytes = len.to_le_bytes();
        self.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4].copy_from_slice(&bytes);
    }

    /// Read the output buffer length field (u32, little-endian) as plain bytes.
    /// Must be called after a SeqCst fence to observe the writer's value.
    pub fn get_output_len(&self) -> u32 {
        // Length fields use plain byte reads with SeqCst fences for cross-process
        // synchronization (see set_output_len for full rationale).
        u32::from_le_bytes([
            self.mmap[OUTPUT_LEN_OFFSET],
            self.mmap[OUTPUT_LEN_OFFSET + 1],
            self.mmap[OUTPUT_LEN_OFFSET + 2],
            self.mmap[OUTPUT_LEN_OFFSET + 3],
        ])
    }

    pub fn set_output_len(&mut self, len: u32) {
        // Length fields are written as plain bytes (not AtomicU32) because the
        // mmap base address may not guarantee 4-byte alignment at the field offset,
        // which is required for AtomicU32 pointer casts.
        //
        // Cross-process synchronization is achieved via SeqCst fences:
        // - WRITER: writes data → writes length → SeqCst fence → sets state (Release)
        // - READER: reads state (Acquire) → SeqCst fence → reads length → reads data
        //
        // The Acquire/Release pair on the state byte (AtomicU8) combined with the
        // SeqCst fences ensures that:
        // 1. The length write is visible before the state transition to REQ_READY/RES_READY
        // 2. The length read observes the value written before the observed state
        //
        // On x86_64, aligned 4-byte stores are atomic at the hardware level, and
        // the store buffer is flushed by the SeqCst fence (which emits `mfence`).
        let bytes = len.to_le_bytes();
        self.mmap[OUTPUT_LEN_OFFSET..OUTPUT_LEN_OFFSET + 4].copy_from_slice(&bytes);
    }

    /// Read the input request buffer as a UTF-8 string.
    ///
    /// Issues a SeqCst fence before reading the length to ensure the writer's
    /// length value is visible. Rejects lengths exceeding the input buffer capacity.
    pub fn read_input(&self) -> Result<String, Box<dyn Error>> {
        // SeqCst fence after Acquire on state ensures we observe the
        // length written by the other side before it set REQ_READY.
        std::sync::atomic::fence(Ordering::SeqCst);
        let len = self.get_input_len() as usize;
        if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
            return Err("Input length exceeds buffer limit".into());
        }
        let bytes = &self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len];
        Ok(String::from_utf8(bytes.to_vec())?)
    }

    /// Write a request string into the input buffer region.
    /// Mirrors `write_output` but targets the input side (used by host/benchmarks).
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

        // Write data first, then length. The caller must set_state(RES_READY)
        // with its Release store AFTER this returns, which pairs with the
        // reader's Acquire load + SeqCst fence to make the length visible.
        self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);
        self.set_output_len(bytes.len() as u32);
        Ok(())
    }

    /// Flush the memory-mapped region to persistent storage.
    pub fn flush(&self) -> Result<(), Box<dyn Error>> {
        self.mmap.flush()?;
        Ok(())
    }

    /// Issue a SeqCst memory fence to synchronize length field writes/reads
    /// across processes.
    ///
    /// Writer path: write_output() → sync_fence() → set_state(RES_READY)
    /// Reader path: get_state(REQ_READY) → sync_fence() → read_input()
    ///
    /// This fence ensures that the length field writes (plain bytes) are
    /// globally visible before the state byte transitions, and that the
    /// reader observes the correct length after observing the state change.
    pub fn sync_fence() {
        std::sync::atomic::fence(Ordering::SeqCst);
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
        cleanup(&path);
    }

    #[test]
    fn test_write_and_read_input() {
        let path = temp_buffer_path("input");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        let request = r#"{"jsonrpc":"2.0","method":"tools/list","id":2}"#;
        // Manually write input for testing
        let bytes = request.as_bytes();
        buffer.set_input_len(bytes.len() as u32);
        buffer.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + bytes.len()].copy_from_slice(bytes);
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

        // Set input length beyond buffer capacity
        buffer.set_input_len(OUTPUT_BUFFER_OFFSET as u32); // 4096, but capacity is 4086
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

        let max_output = TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET; // 61440
        let oversized = "x".repeat(max_output + 1);
        let result = buffer.write_output(&oversized);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds output buffer limit"));
        cleanup(&path);
    }

    #[test]
    fn test_buffer_persists_acreate_or_open() {
        let path = temp_buffer_path("persist");
        cleanup(&path);

        // Write state in first session
        {
            let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();
            buffer.set_state(STATE_RES_READY);
            buffer.flush().unwrap();
        }

        // Re-open and verify state persisted
        {
            let buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();
            assert_eq!(buffer.get_state(), STATE_RES_READY);
        }
        cleanup(&path);
    }

    #[test]
    fn test_full_request_response_cycle() {
        let path = temp_buffer_path("cycle");
        cleanup(&path);
        let mut buffer = SharedMemoryBuffer::create_or_open(&path).unwrap();

        // Simulate host writing a request
        let request = r#"{"jsonrpc":"2.0","method":"tools/call","params":{"name":"read_nda","arguments":{"ndaPath":"test.nda"}},"id":1}"#;
        let req_bytes = request.as_bytes();
        buffer.set_input_len(req_bytes.len() as u32);
        buffer.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + req_bytes.len()].copy_from_slice(req_bytes);
        buffer.set_state(STATE_REQ_READY);
        buffer.flush().unwrap();

        // Simulate server reading request
        assert_eq!(buffer.get_state(), STATE_REQ_READY);
        let input = buffer.read_input().unwrap();
        assert_eq!(input, request);

        // Simulate server processing
        buffer.set_state(STATE_PROCESSING);
        buffer.flush().unwrap();

        // Simulate server writing response
        let response = r#"{"jsonrpc":"2.0","result":{"content":[{"type":"text","text":"OK"}]},"id":1}"#;
        buffer.write_output(response).unwrap();
        buffer.set_state(STATE_RES_READY);
        buffer.flush().unwrap();

        // Verify final state
        assert_eq!(buffer.get_state(), STATE_RES_READY);
        assert_eq!(buffer.get_output_len(), response.len() as u32);
        cleanup(&path);
    }
}
