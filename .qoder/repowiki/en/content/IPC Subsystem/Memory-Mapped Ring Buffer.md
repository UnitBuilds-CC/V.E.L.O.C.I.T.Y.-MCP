# Memory-Mapped Ring Buffer

<cite>
**Referenced Files in This Document**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
</cite>

## Overview

The `SharedMemoryBuffer` in `src/ipc/shmem.rs` implements a memory-mapped file buffer for IPC between the MCP server and its host process. It uses the `memmap2` crate to map a 64KB file into the virtual address space, providing direct memory access without kernel-mediated data transfer. Cross-platform support works on Windows, Linux, and macOS.

## Implementation Details

### File Management

```rust
let file = OpenOptions::new()
    .read(true)
    .write(true)
    .create(true)
    .open(path)?;
file.set_len(TOTAL_BUFFER_SIZE as u64)?;  // Ensure 64KB
let mmap = unsafe { MmapMut::map_mut(&file)? };
```

The file is opened with read+write+create permissions. If it doesn't exist, it's created. The file length is explicitly set to 64KB to ensure the mmap has sufficient backing store.

### Memory Map

`MmapMut` provides a mutable memory-mapped view. Both the server and host process map the same file, giving them access to the same physical pages. Writes by one process are visible to the other after `flush()`.

### Buffer Layout Constants

```rust
const STATE_OFFSET: usize = 0;           // 1 byte: state machine
const INPUT_LEN_OFFSET: usize = 1;       // 4 bytes: input length (u32 LE)
const OUTPUT_LEN_OFFSET: usize = 5;      // 4 bytes: output length (u32 LE)
const INPUT_BUFFER_OFFSET: usize = 10;   // Input data starts here
const OUTPUT_BUFFER_OFFSET: usize = 4096; // Output data starts here
const TOTAL_BUFFER_SIZE: usize = 65536;  // 64KB total
```

### Length Encoding

Lengths are stored as little-endian u32 values, manually serialized byte-by-byte:

```rust
// Read
u32::from_le_bytes([mmap[1], mmap[2], mmap[3], mmap[4]])

// Write
mmap[offset..offset+4].copy_from_slice(&len.to_le_bytes());
```

This avoids alignment issues that could arise from direct pointer casts on the mmap region.

### Atomic State Management

In v3.0, the state byte uses `AtomicU8` with proper memory ordering:

```rust
// Writer: Release ordering ensures data is visible before state change
self.state.store(STATE_RES_READY, Ordering::Release);

// Reader: Acquire ordering ensures state is read before data
let state = self.state.load(Ordering::Acquire);
```

Combined with `SeqCst` fences on length fields, this ensures correct cross-process synchronization.

### Win32 Event Integration

On Windows, the buffer creates named events for zero-poll signaling:

```rust
#[cfg(target_os = "windows")]
fn create_events(&self) -> (HANDLE, HANDLE) {
    let req_event = CreateEventW(..., &format!("Global\\VELOCITY_NMCP_REQ_{}", self.buffer_name));
    let res_event = CreateEventW(..., &format!("Global\\VELOCITY_NMCP_RES_{}", self.buffer_name));
    (req_event, res_event)
}
```

### Input Reading

```rust
pub fn read_input(&self) -> Result<String, Box<dyn Error>> {
    let len = self.get_input_len() as usize;
    if len > (OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET) {
        return Err("Input length exceeds buffer limit".into());
    }
    let bytes = &self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + len];
    Ok(String::from_utf8(bytes.to_vec())?)
}
```

Reads `input_len` bytes from the input region, validates the length, and converts to UTF-8 String.

### Output Writing

```rust
pub fn write_output(&mut self, response: &str) -> Result<(), Box<dyn Error>> {
    let bytes = response.as_bytes();
    if bytes.len() > (TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET) {
        return Err("Response length exceeds output buffer limit".into());
    }
    self.set_output_len(bytes.len() as u32);
    self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    Ok(())
}
```

### Flush

```rust
pub fn flush(&self) -> Result<(), Box<dyn Error>> {
    self.mmap.flush()?;
    Ok(())
}
```

Calls `MmapMut::flush()` which invokes `msync()` (or `FlushViewOfFile` on Windows) to sync the memory-mapped pages to the backing file.

## Key Design Decisions

1. **Manual byte serialization**: Length fields are serialized byte-by-byte instead of using `unsafe` pointer casts. This is safer and the performance difference is negligible (4 bytes).
2. **`MmapMut` not `Mmap`**: The buffer needs both read and write access, so `MmapMut` (mutable mapping) is required.
3. **File-backed not anonymous**: File-backed mapping provides persistence and a named rendezvous point. Anonymous mappings cannot be shared across process launches.
4. **Atomic state with Acquire/Release**: Proper memory ordering ensures cross-process correctness without locks.
5. **Win32 Events on Windows**: Named events provide zero-poll blocking waits, eliminating CPU waste from polling.

**Section sources**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
