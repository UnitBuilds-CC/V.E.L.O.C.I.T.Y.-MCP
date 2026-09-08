#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

// ─── Bump allocator (resets per call for benchmark reuse) ────────────────────

struct BumpAlloc;

static OFFSET: AtomicUsize = AtomicUsize::new(0);
static mut BUF: [u8; 1 << 20] = [0u8; 1 << 20];

unsafe impl GlobalAlloc for BumpAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        let current = BUF.as_ptr() as usize + OFFSET.load(Ordering::Relaxed);
        let aligned = (current + align - 1) & !(align - 1);
        let new_offset = aligned - BUF.as_ptr() as usize + size;
        if new_offset > BUF.len() {
            return core::ptr::null_mut();
        }
        OFFSET.store(new_offset, Ordering::Relaxed);
        aligned as *mut u8
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOC: BumpAlloc = BumpAlloc;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[alloc_error_handler]
fn oom(_layout: Layout) -> ! {
    loop {}
}

// ─── Tool logic: text analysis ───────────────────────────────────────────────

fn find_key<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let pattern = alloc::format!("\"{}\":\"", key);
    let start = json.find(&pattern)? + pattern.len();
    let rest = &json[start..];
    let bytes = rest.as_bytes();
    let mut end = 0;
    while end < bytes.len() {
        if bytes[end] == b'"' {
            let mut backslashes = 0;
            let mut i = end;
            while i > 0 && bytes[i - 1] == b'\\' {
                backslashes += 1;
                i -= 1;
            }
            if backslashes % 2 == 0 {
                return Some(&rest[..end]);
            }
        }
        end += 1;
    }
    None
}

fn push_usize(buf: &mut alloc::vec::Vec<u8>, val: usize) {
    if val == 0 {
        buf.push(b'0');
        return;
    }
    let mut tmp = [0u8; 20];
    let mut i = 20;
    let mut v = val;
    while v > 0 {
        i -= 1;
        tmp[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    buf.extend_from_slice(&tmp[i..]);
}

fn build_json(word_count: usize, char_count: usize, line_count: usize) -> alloc::vec::Vec<u8> {
    let mut buf = alloc::vec::Vec::with_capacity(128);
    buf.extend_from_slice(b"{\"word_count\":");
    push_usize(&mut buf, word_count);
    buf.extend_from_slice(b",\"char_count\":");
    push_usize(&mut buf, char_count);
    buf.extend_from_slice(b",\"line_count\":");
    push_usize(&mut buf, line_count);
    buf.push(b'}');
    buf
}

// ─── WASM exports ────────────────────────────────────────────────────────────

/// Reset allocator — call before each benchmark iteration to reuse memory.
#[no_mangle]
pub extern "C" fn prepare_call() {
    OFFSET.store(0, Ordering::Relaxed);
}

/// Entry point: reads JSON from WASM memory, returns JSON in WASM memory.
/// Return value encodes (result_ptr << 32) | result_len.
#[no_mangle]
pub extern "C" fn tool_execute(ptr: i32, len: i32) -> i64 {
    let input_bytes = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let input = match core::str::from_utf8(input_bytes) {
        Ok(s) => s,
        Err(_) => {
            let err = b"{\"error\":\"invalid utf8\"}";
            let out = alloc::vec::Vec::from(&err[..]);
            let p = out.as_ptr();
            let l = out.len();
            core::mem::forget(out);
            return ((p as i64) << 32) | (l as i64);
        }
    };

    let text = find_key(input, "text").unwrap_or("");
    let word_count = text.split_whitespace().count();
    let char_count = text.len();
    let line_count = if text.is_empty() { 0 } else { text.split('\n').count() };

    let result = build_json(word_count, char_count, line_count);
    let p = result.as_ptr();
    let l = result.len();
    core::mem::forget(result);
    ((p as i64) << 32) | (l as i64)
}
