# Memory-Mapped Ring Buffer

<cite>
**Referenced Files in This Document**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
</cite>

## Overview

The `SharedMemoryBuffer` in `src/ipc/shmem.rs` implements a memory-mapped file buffer for IPC between the NMCP server and its host process. It uses the `memmap2` crate to map a 64KB file into the virtual address space, providing direct memory access without kernel-mediated data transfer.

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

Reads `input_len` bytes from the input region, validates the length, and converts to UTF-8 String. Note: `to_vec()` allocates — this is a necessary copy since the String must outlive the mmap borrow.

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

Writes the response bytes directly into the output region of the mmap. Length is validated against the available space (~61KB).

### Flush

```rust
pub fn flush(&self) -> Result<(), Box<dyn Error>> {
    self.mmap.flush()?;
    Ok(())
}
```

Calls `MmapMut::flush()` which invokes `msync()` (or `FlushViewOfFile` on Windows) to sync the memory-mapped pages to the backing file. This is essential for cross-process visibility.

## Dead Code Allowances

Several methods are marked `#[allow(dead_code)]`:
- `set_input_len()` — Used by the host process, not the server
- `get_output_len()` — Used by the host process to read response length
- `NmcpBinaryFrame` — Future binary driver reference implementation

These are part of the shared API surface that the host process uses.

## Key Design Decisions

1. **Manual byte serialization**: Length fields are serialized byte-by-byte instead of using `unsafe` pointer casts. This is safer and the performance difference is negligible (4 bytes).
2. **`MmapMut` not `Mmap`**: The buffer needs both read and write access, so `MmapMut` (mutable mapping) is required.
3. **File-backed not anonymous**: File-backed mapping provides persistence and a named rendezvous point. Anonymous mappings cannot be shared across process launches.
4. **`to_vec()` on read**: The input is copied into a `Vec<u8>` before converting to `String`. This is necessary because the `String` must be owned data that outlives the borrow on `self.mmap`.

**Section sources**
- [src/ipc/shmem.rs](file://src/ipc/shmem.rs)
