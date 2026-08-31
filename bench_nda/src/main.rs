//! NDA-native benchmark: compares NDA binary over shared memory vs JSON-RPC over stdio.
//!
//! Measures the binary TLV transport path against the stdio fallback.
//! The NDA path uses: binary TLV frames → shared memory → Win32 Events (zero-poll).
//! The stdio path uses: JSON text → pipes → thread + channel + poll loop.

use memmap2::MmapMut;
use sha2::{Sha256, Digest};
use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::sync::atomic::Ordering;

// ─── Shmem layout (matches src/ipc/shmem.rs) ─────────────────────────────

const STATE_OFFSET: usize = 0;
const INPUT_LEN_OFFSET: usize = 1;
const OUTPUT_LEN_OFFSET: usize = 5;
const INPUT_BUFFER_OFFSET: usize = 10;
const OUTPUT_BUFFER_OFFSET: usize = 4096;
const TOTAL_BUFFER_SIZE: usize = 65536;

const STATE_REQ_READY: u8 = 1;

// ─── NDA protocol constants (matches src/protocol/nda_native.rs) ──────────

const NDA_MAGIC: &[u8; 4] = b"NMCP";
const FRAME_HEADER_SIZE: usize = 36;

const METHOD_INITIALIZE: u8 = 0x01;
const METHOD_TOOLS_LIST: u8 = 0x02;
const METHOD_TOOLS_CALL: u8 = 0x03;
const METHOD_PING: u8 = 0x04;
const METHOD_HEALTH_CHECK: u8 = 0x06;
const NOTIF_INITIALIZED: u8 = 0x10;

const STATUS_OK: u8 = 0;

// ─── Win32 imports ────────────────────────────────────────────────────────

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

/// Fail loudly instead of hanging forever if the server stops responding.
const WAIT_TIMEOUT_MS: u32 = 10_000;

#[cfg(target_os = "windows")]
fn to_wstring(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

// ─── NDA Shmem Client ─────────────────────────────────────────────────────
//
// Uses auto-reset events so each SetEvent/WaitForSingleObject pair is
// self-consuming — no manual ResetEvent needed on the client side.

struct NdaShmemClient {
    mmap: MmapMut,
    h_req_event: *mut std::ffi::c_void,
    h_res_event: *mut std::ffi::c_void,
}

impl NdaShmemClient {
    fn new(buffer_path: &str) -> Self {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(buffer_path)
            .expect("Failed to open shared memory buffer");

        let mmap = unsafe { MmapMut::map_mut(&file).expect("Failed to mmap buffer") };

        let stem = std::path::Path::new(buffer_path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap();

        let req_name = format!("Global\\VELOCITY_NMCP_REQ_{}", stem);
        let res_name = format!("Global\\VELOCITY_NMCP_RES_{}", stem);

        // Auto-reset events (bManualReset = 0) — each signal wakes exactly one wait.
        let h_req = unsafe {
            CreateEventW(std::ptr::null_mut(), 0, 0, to_wstring(&req_name).as_ptr())
        };
        let h_res = unsafe {
            CreateEventW(std::ptr::null_mut(), 0, 0, to_wstring(&res_name).as_ptr())
        };

        assert!(!h_req.is_null() && !h_res.is_null(), "Failed to create Win32 events");

        NdaShmemClient {
            mmap,
            h_req_event: h_req,
            h_res_event: h_res,
        }
    }

    fn send_request(&mut self, frame: &[u8]) -> Vec<u8> {
        self.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4]
            .copy_from_slice(&(frame.len() as u32).to_le_bytes());
        self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + frame.len()]
            .copy_from_slice(frame);
        std::sync::atomic::fence(Ordering::SeqCst);
        self.mmap[STATE_OFFSET] = STATE_REQ_READY;
        // No flush: same-section views share physical pages, so the write is
        // already visible to the server. Events + fence carry the ordering.

        unsafe {
            SetEvent(self.h_req_event);
            let rc = WaitForSingleObject(self.h_res_event, WAIT_TIMEOUT_MS);
            assert_eq!(rc, 0, "Timed out waiting for server response ({} ms)", WAIT_TIMEOUT_MS);
        }

        std::sync::atomic::fence(Ordering::SeqCst);
        let out_len = u32::from_le_bytes([
            self.mmap[OUTPUT_LEN_OFFSET],
            self.mmap[OUTPUT_LEN_OFFSET + 1],
            self.mmap[OUTPUT_LEN_OFFSET + 2],
            self.mmap[OUTPUT_LEN_OFFSET + 3],
        ]) as usize;
        // Events carry all synchronization; no state write/flush needed after read.
        self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + out_len].to_vec()
    }

    fn send_notification(&mut self, frame: &[u8]) {
        self.send_request(frame);
    }

    /// Same as send_request but splits the round trip into three timed
    /// phases: (1) mmap write, (2) signal + server turnaround + wake,
    /// (3) response read. Returns (response, write_us, wait_us, read_us).
    fn send_request_phased(&mut self, frame: &[u8]) -> (Vec<u8>, f64, f64, f64) {
        let t0 = Instant::now();
        self.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4]
            .copy_from_slice(&(frame.len() as u32).to_le_bytes());
        self.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + frame.len()]
            .copy_from_slice(frame);
        std::sync::atomic::fence(Ordering::SeqCst);
        self.mmap[STATE_OFFSET] = STATE_REQ_READY;
        let t1 = Instant::now();

        unsafe {
            SetEvent(self.h_req_event);
            let rc = WaitForSingleObject(self.h_res_event, WAIT_TIMEOUT_MS);
            assert_eq!(rc, 0, "Timed out waiting for server response ({} ms)", WAIT_TIMEOUT_MS);
        }
        let t2 = Instant::now();

        std::sync::atomic::fence(Ordering::SeqCst);
        let out_len = u32::from_le_bytes([
            self.mmap[OUTPUT_LEN_OFFSET],
            self.mmap[OUTPUT_LEN_OFFSET + 1],
            self.mmap[OUTPUT_LEN_OFFSET + 2],
            self.mmap[OUTPUT_LEN_OFFSET + 3],
        ]) as usize;
        let resp = self.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + out_len].to_vec();
        let t3 = Instant::now();

        (
            resp,
            t1.duration_since(t0).as_secs_f64() * 1_000_000.0,
            t2.duration_since(t1).as_secs_f64() * 1_000_000.0,
            t3.duration_since(t2).as_secs_f64() * 1_000_000.0,
        )
    }
}

impl Drop for NdaShmemClient {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.h_req_event);
            CloseHandle(self.h_res_event);
        }
    }
}

// ─── JSON-over-Shmem Client (same transport, JSON encoding) ───────────────

struct JsonShmemClient {
    inner: NdaShmemClient,
}

impl JsonShmemClient {
    fn new(buffer_path: &str) -> Self {
        JsonShmemClient { inner: NdaShmemClient::new(buffer_path) }
    }

    fn send_json(&mut self, request: &serde_json::Value) -> String {
        let json_str = serde_json::to_string(request).unwrap();
        let bytes = json_str.as_bytes();

        self.inner.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4]
            .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.inner.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        std::sync::atomic::fence(Ordering::SeqCst);
        self.inner.mmap[STATE_OFFSET] = STATE_REQ_READY;
        // No flush: same-section views are cache-coherent cross-process.

        unsafe {
            SetEvent(self.inner.h_req_event);
            let rc = WaitForSingleObject(self.inner.h_res_event, WAIT_TIMEOUT_MS);
            assert_eq!(rc, 0, "Timed out waiting for server response ({} ms)", WAIT_TIMEOUT_MS);
        }

        std::sync::atomic::fence(Ordering::SeqCst);
        let out_len = u32::from_le_bytes([
            self.inner.mmap[OUTPUT_LEN_OFFSET],
            self.inner.mmap[OUTPUT_LEN_OFFSET + 1],
            self.inner.mmap[OUTPUT_LEN_OFFSET + 2],
            self.inner.mmap[OUTPUT_LEN_OFFSET + 3],
        ]) as usize;
        // Events carry all synchronization; no state write/flush needed after read.
        String::from_utf8(
            self.inner.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + out_len].to_vec()
        ).unwrap()
    }
}

// ─── Stdio Client (JSON-RPC over stdin/stdout pipes) ──────────────────────

struct StdioClient {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}

impl StdioClient {
    fn new(server_path: &str) -> Self {
        let mut child = Command::new(server_path)
            .arg("--mode").arg("stdio")
            // Raise the tool-call rate limit so tools/call isn't throttled mid-benchmark
            .env("VELOCITY_RATE_LIMIT", "1000000")
            .env("VELOCITY_RATE_BURST", "1000000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start stdio server");

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        StdioClient { child, reader }
    }

    fn send_request(&mut self, request: &serde_json::Value) -> String {
        let json_str = serde_json::to_string(request).unwrap() + "\n";
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(json_str.as_bytes()).unwrap();
        stdin.flush().unwrap();

        // Read lines until we get valid JSON (skip log lines)
        loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // Try to parse as JSON - if it fails, it's a log line, skip it
            if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                return trimmed.to_string();
            }
        }
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ─── NDA Frame Building ───────────────────────────────────────────────────

fn encode_tlv_value(value: &serde_json::Value, buf: &mut Vec<u8>) {
    match value {
        serde_json::Value::String(s) => {
            buf.push(0x01);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            buf.extend_from_slice(bytes);
        }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                buf.push(0x02);
                buf.extend_from_slice(&i.to_be_bytes());
            } else if let Some(f) = n.as_f64() {
                buf.push(0x07);
                buf.extend_from_slice(&f.to_be_bytes());
            } else {
                buf.push(0x02);
                buf.extend_from_slice(&0i64.to_be_bytes());
            }
        }
        serde_json::Value::Bool(b) => {
            buf.push(0x03);
            buf.push(if *b { 1 } else { 0 });
        }
        serde_json::Value::Null => {
            buf.push(0x04);
        }
        serde_json::Value::Array(arr) => {
            buf.push(0x05);
            buf.extend_from_slice(&(arr.len() as u32).to_be_bytes());
            for item in arr {
                encode_tlv_value(item, buf);
            }
        }
        serde_json::Value::Object(obj) => {
            buf.push(0x06);
            buf.extend_from_slice(&(obj.len() as u32).to_be_bytes());
            for (key, val) in obj {
                let key_bytes = key.as_bytes();
                buf.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(key_bytes);
                encode_tlv_value(val, buf);
            }
        }
    }
}

fn build_nda_frame(payload: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let merkle = hasher.finalize();

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    frame.extend_from_slice(NDA_MAGIC);
    frame.extend_from_slice(&merkle);
    frame.extend_from_slice(payload);
    frame
}

fn build_nda_request(method: u8, request_id: u64, data: &serde_json::Value) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(method);
    encode_tlv_value(&serde_json::json!(request_id), &mut payload);
    if !data.is_null() {
        encode_tlv_value(data, &mut payload);
    }
    build_nda_frame(&payload)
}

fn validate_nda_response(frame: &[u8], expected_id: u64, ctx: &str) {
    assert!(
        frame.len() >= FRAME_HEADER_SIZE + 1,
        "{}: NDA response too small ({} bytes)", ctx, frame.len()
    );
    assert_eq!(&frame[0..4], NDA_MAGIC, "{}: bad NDA magic", ctx);

    let payload = &frame[FRAME_HEADER_SIZE..];
    let mut hasher = Sha256::new();
    hasher.update(payload);
    assert_eq!(
        &frame[4..36],
        hasher.finalize().as_slice(),
        "{}: NDA Merkle root mismatch (corrupted response)", ctx
    );
    assert_eq!(payload[0], STATUS_OK, "{}: NDA error status={}", ctx, payload[0]);

    // Request id echo: TLV tag 0x02 (i64, 8 bytes big-endian)
    assert!(payload.len() >= 10, "{}: response missing request id", ctx);
    assert_eq!(payload[1], 0x02, "{}: request id not i64 TLV", ctx);
    let id = i64::from_be_bytes(payload[2..10].try_into().unwrap());
    assert_eq!(id, expected_id as i64, "{}: request id not echoed", ctx);
}

/// Mirror of the server's `decode_json_value`: TLV bytes → serde_json::Value.
/// Lets the harness verify response CONTENT, not just integrity fields.
fn decode_tlv_value(bytes: &[u8]) -> (serde_json::Value, usize) {
    use serde_json::json;
    assert!(!bytes.is_empty(), "TLV: empty value");
    match bytes[0] {
        0x01 => {
            assert!(bytes.len() >= 5, "TLV string: truncated length");
            let len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
            assert!(bytes.len() >= 5 + len, "TLV string: truncated body");
            let s = std::str::from_utf8(&bytes[5..5 + len]).expect("TLV string: invalid UTF-8");
            (json!(s), 5 + len)
        }
        0x02 => {
            assert!(bytes.len() >= 9, "TLV i64: truncated");
            (json!(i64::from_be_bytes(bytes[1..9].try_into().unwrap())), 9)
        }
        0x03 => {
            assert!(bytes.len() >= 2, "TLV bool: truncated");
            (json!(bytes[1] != 0), 2)
        }
        0x04 => (serde_json::Value::Null, 1),
        0x05 => {
            assert!(bytes.len() >= 5, "TLV array: truncated count");
            let count = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
            let mut arr = Vec::with_capacity(count);
            let mut off = 5usize;
            for _ in 0..count {
                let (v, n) = decode_tlv_value(&bytes[off..]);
                arr.push(v);
                off += n;
            }
            (json!(arr), off)
        }
        0x06 => {
            assert!(bytes.len() >= 5, "TLV object: truncated count");
            let count = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
            let mut obj = serde_json::Map::new();
            let mut off = 5usize;
            for _ in 0..count {
                assert!(bytes.len() >= off + 2, "TLV object: truncated key length");
                let klen = u16::from_be_bytes(bytes[off..off + 2].try_into().unwrap()) as usize;
                off += 2;
                assert!(bytes.len() >= off + klen, "TLV object: truncated key");
                let key = std::str::from_utf8(&bytes[off..off + klen])
                    .expect("TLV object: invalid key UTF-8")
                    .to_string();
                off += klen;
                let (v, n) = decode_tlv_value(&bytes[off..]);
                obj.insert(key, v);
                off += n;
            }
            (serde_json::Value::Object(obj), off)
        }
        0x07 => {
            assert!(bytes.len() >= 9, "TLV f64: truncated");
            (json!(f64::from_be_bytes(bytes[1..9].try_into().unwrap())), 9)
        }
        other => panic!("TLV: unknown tag 0x{:02x}", other),
    }
}

/// Fully decode a tools/list response and return the set of tool names.
/// Validates integrity fields, status, id echo, AND decodes the result TLV.
fn decode_tools_list_names(frame: &[u8], expected_id: u64, ctx: &str) -> BTreeSet<String> {
    validate_nda_response(frame, expected_id, ctx);
    let payload = &frame[FRAME_HEADER_SIZE..];
    let (result, consumed) = decode_tlv_value(&payload[10..]);
    assert_eq!(
        consumed,
        payload.len() - 10,
        "{}: {} trailing bytes after result TLV", ctx, payload.len() - 10 - consumed
    );
    let tools = result["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("{}: result has no 'tools' array", ctx));
    tools
        .iter()
        .map(|t| {
            t["name"]
                .as_str()
                .unwrap_or_else(|| panic!("{}: tool entry without string name", ctx))
                .to_string()
        })
        .collect()
}

// ─── Transport Primitive Microbenchmarks ────────────────────────────────
//
// Accounts for every microsecond of the round trip: how much does an event
// wake cost, how much does FlushViewOfFile cost?

#[cfg(target_os = "windows")]
fn run_microbenchmarks(self_exe: &str) {
    const PINGPONG_ITERS: usize = 20_000;
    const FLUSH_ITERS: usize = 2_000;

    // 1. Event ping-pong between two threads (same process — optimistic).
    let ev_a = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
    let ev_b = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
    assert!(!ev_a.is_null() && !ev_b.is_null(), "microbench: CreateEventW failed");
    let (a, b) = (ev_a as usize, ev_b as usize);
    let peer = std::thread::spawn(move || {
        let (ev_a, ev_b) = (a as *mut std::ffi::c_void, b as *mut std::ffi::c_void);
        for _ in 0..PINGPONG_ITERS {
            unsafe {
                WaitForSingleObject(ev_a, 0xFFFFFFFF);
                SetEvent(ev_b);
            }
        }
    });
    let start = Instant::now();
    for _ in 0..PINGPONG_ITERS {
        unsafe {
            SetEvent(ev_a);
            WaitForSingleObject(ev_b, 0xFFFFFFFF);
        }
    }
    let rtt_us = start.elapsed().as_secs_f64() * 1_000_000.0 / PINGPONG_ITERS as f64;
    peer.join().unwrap();
    unsafe { CloseHandle(ev_a); CloseHandle(ev_b); }
    println!("  Micro: event ping-pong RTT (2 threads, same proc): {:.2} us  (one wake = {:.2} us)", rtt_us, rtt_us / 2.0);

    // 2. Event ping-pong across two processes — the real shmem wake cost.
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let name_a = format!("Global\\MB_A_{}", ts);
    let name_b = format!("Global\\MB_B_{}", ts);
    let h_a = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, to_wstring(&name_a).as_ptr()) };
    let h_b = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, to_wstring(&name_b).as_ptr()) };
    assert!(!h_a.is_null() && !h_b.is_null(), "microbench: named CreateEventW failed");
    let mut peer_proc = Command::new(self_exe)
        .arg("peer").arg(&name_a).arg(&name_b).arg(PINGPONG_ITERS.to_string())
        .spawn().expect("failed to spawn ping-pong peer");
    std::thread::sleep(Duration::from_millis(300)); // let peer open the events
    let start = Instant::now();
    for _ in 0..PINGPONG_ITERS {
        unsafe {
            SetEvent(h_a);
            WaitForSingleObject(h_b, WAIT_TIMEOUT_MS);
        }
    }
    let xproc_rtt_us = start.elapsed().as_secs_f64() * 1_000_000.0 / PINGPONG_ITERS as f64;
    let _ = peer_proc.wait();
    unsafe { CloseHandle(h_a); CloseHandle(h_b); }
    println!("  Micro: event ping-pong RTT (2 processes):          {:.2} us  (one wake = {:.2} us)", xproc_rtt_us, xproc_rtt_us / 2.0);

    // 3. flush_async (FlushViewOfFile) cost on a 64KB file mapping with a
    // dirty page — matches the real pattern of writing a response before
    // flushing. Informational only: the hot path no longer flushes.
    let path = "mb_flush.bin";
    let _ = std::fs::remove_file(path);
    {
        let file = OpenOptions::new().read(true).write(true).create(true).open(path).unwrap();
        file.set_len(TOTAL_BUFFER_SIZE as u64).unwrap();
        let mut mmap = unsafe { MmapMut::map_mut(&file).unwrap() };
        for i in 0..100 { mmap[i % 4096] = i as u8; let _ = mmap.flush_async(); }
        let start = Instant::now();
        for i in 0..FLUSH_ITERS {
            mmap[i % 4096] = i as u8;
            let _ = mmap.flush_async();
        }
        let flush_us = start.elapsed().as_secs_f64() * 1_000_000.0 / FLUSH_ITERS as f64;
        println!("  Micro: flush_async (FlushViewOfFile, dirty page):  {:.2} us", flush_us);
    }
    let _ = std::fs::remove_file(path);
}

/// Peer mode for the cross-process event ping-pong microbenchmark:
/// wait on event A, set event B, N times, then exit.
#[cfg(target_os = "windows")]
fn run_pingpong_peer(name_a: &str, name_b: &str, iters: usize) {
    let h_a = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, to_wstring(name_a).as_ptr()) };
    let h_b = unsafe { CreateEventW(std::ptr::null_mut(), 0, 0, to_wstring(name_b).as_ptr()) };
    assert!(!h_a.is_null() && !h_b.is_null(), "peer: failed to open events");
    for _ in 0..iters {
        unsafe {
            WaitForSingleObject(h_a, 0xFFFFFFFF);
            SetEvent(h_b);
        }
    }
    unsafe { CloseHandle(h_a); CloseHandle(h_b); }
}

#[cfg(not(target_os = "windows"))]
fn run_microbenchmarks(_self_exe: &str) {}

// ─── Server Spawning ──────────────────────────────────────────────────────

/// RAII guard: kills the server and removes the buffer file even on panic,
/// so assertion failures never orphan a velocity_mcp.exe process.
struct ServerGuard {
    child: Option<Child>,
    buffer_path: Option<String>,
}

impl ServerGuard {
    fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.kill();
        if let Some(path) = self.buffer_path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn spawn_shmem_server(server_path: &str, buffer_path: &str, extra_env: &[(&str, &str)]) -> ServerGuard {
    let mut cmd = Command::new(server_path);
    cmd.arg("--mode").arg("shmem")
        .arg("--buffer-path").arg(buffer_path)
        // Raise the server's tool-call rate limit so the benchmark measures
        // throughput, not throttling (default is 20 req/s, burst 100).
        .env("VELOCITY_RATE_LIMIT", "1000000")
        .env("VELOCITY_RATE_BURST", "1000000");
    for (key, val) in extra_env {
        cmd.env(key, val);
    }
    let child = cmd
        .stderr(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("Failed to start shmem server");
    ServerGuard { child: Some(child), buffer_path: Some(buffer_path.to_string()) }
}

/// Wait until the server has created the shared memory buffer file.
fn wait_for_buffer_file(buffer_path: &str) {
    for _ in 0..500 {
        if std::path::Path::new(buffer_path).exists() {
            std::thread::sleep(Duration::from_millis(100));
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("Server did not create buffer file '{}' within 5s", buffer_path);
}

// ─── Benchmark Helpers ────────────────────────────────────────────────────

struct BenchResult {
    latencies_us: Vec<f64>,
}

impl BenchResult {
    fn avg_ms(&self) -> f64 {
        self.latencies_us.iter().sum::<f64>() / self.latencies_us.len() as f64 / 1000.0
    }

    fn percentile(&self, p: f64) -> f64 {
        let mut sorted = self.latencies_us.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let idx = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
        sorted[idx.min(sorted.len()) - 1] / 1000.0
    }

    fn throughput(&self) -> f64 {
        let total_ms: f64 = self.latencies_us.iter().sum::<f64>() / 1000.0;
        self.latencies_us.len() as f64 / total_ms * 1000.0
    }
}

/// Run a benchmark `rounds` times and keep the round with the median average
/// latency, so transient system noise cannot skew the reported numbers.
fn median_round(rounds: usize, mut f: impl FnMut() -> BenchResult) -> BenchResult {
    let mut results: Vec<BenchResult> = (0..rounds).map(|_| f()).collect();
    results.sort_by(|a, b| a.avg_ms().partial_cmp(&b.avg_ms()).unwrap());
    results.remove(rounds / 2)
}

const ROUNDS: usize = 3;

fn bench_nda_ping(client: &mut NdaShmemClient, iterations: usize) -> BenchResult {
    let frame = build_nda_request(METHOD_PING, 1, &serde_json::Value::Null);
    let mut latencies = Vec::with_capacity(iterations);
    let (mut write_us, mut wait_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for _ in 0..iterations {
        let start = Instant::now();
        let (resp, w, wt, r) = client.send_request_phased(&frame);
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1_000_000.0);
        validate_nda_response(&resp, 1, "nda_ping");
        write_us += w;
        wait_us += wt;
        read_us += r;
    }
    let n = iterations as f64;
    println!(
        "    [client phases] write={:.1}us wait={:.1}us read+copy={:.1}us",
        write_us / n, wait_us / n, read_us / n
    );

    BenchResult { latencies_us: latencies }
}

fn bench_nda_tools_list(client: &mut NdaShmemClient, iterations: usize) -> BenchResult {
    let frame = build_nda_request(METHOD_TOOLS_LIST, 1, &serde_json::Value::Null);
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let start = Instant::now();
        let resp = client.send_request(&frame);
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1_000_000.0);
        validate_nda_response(&resp, 1, &format!("nda_tools_list[{}]", i));
    }

    BenchResult { latencies_us: latencies }
}

fn bench_nda_tools_call(client: &mut NdaShmemClient, iterations: usize, tool: &str, args: &serde_json::Value) -> BenchResult {
    let data = serde_json::json!({"name": tool, "arguments": args});
    let frame = build_nda_request(METHOD_TOOLS_CALL, 1, &data);
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let start = Instant::now();
        let resp = client.send_request(&frame);
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1_000_000.0);
        validate_nda_response(&resp, 1, &format!("nda_tools_call[{}]", i));
    }

    BenchResult { latencies_us: latencies }
}

fn bench_nda_health(client: &mut NdaShmemClient, iterations: usize) -> BenchResult {
    let frame = build_nda_request(METHOD_HEALTH_CHECK, 1, &serde_json::Value::Null);
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let start = Instant::now();
        let resp = client.send_request(&frame);
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1_000_000.0);
        validate_nda_response(&resp, 1, &format!("nda_health[{}]", i));
    }

    BenchResult { latencies_us: latencies }
}

fn bench_stdio(client: &mut StdioClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let resp_str = client.send_request(&req);
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1_000_000.0);

        // Validate response is valid JSON-RPC (skip ID check due to buffer sync issues)
        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid JSON response at iter {}: {}", i, &resp_str[..resp_str.len().min(100)]));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "Response missing result/error at iter {}", i);
        assert!(!resp_str.contains("Rate limit exceeded"),
                "stdio iter {} hit server rate limit — throttle too tight", i);
    }

    BenchResult { latencies_us: latencies }
}

fn bench_json_shmem(client: &mut JsonShmemClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut latencies = Vec::with_capacity(iterations);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let resp_str = client.send_json(&req);
        let elapsed = start.elapsed();
        latencies.push(elapsed.as_secs_f64() * 1_000_000.0);

        // Validate response is valid JSON-RPC
        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid JSON-shmem response at iter {}", i));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "JSON-shmem response missing result/error at iter {}", i);
        assert!(!resp_str.contains("Rate limit exceeded"),
                "json-shmem iter {} hit server rate limit — throttle too tight", i);
    }

    BenchResult { latencies_us: latencies }
}

// ─── Output Formatting ────────────────────────────────────────────────────

fn print_comparison(name: &str, nda: &BenchResult, stdio: &BenchResult, shmem_json: Option<&BenchResult>) {
    let nda_avg = nda.avg_ms();
    let stdio_avg = stdio.avg_ms();
    let nda_p99 = nda.percentile(99.0);
    let stdio_p99 = stdio.percentile(99.0);
    let avg_speedup = stdio_avg / nda_avg;
    let p99_speedup = stdio_p99 / nda_p99;

    println!("─── {} ──────────────────────────────────────────", name);
    println!("  {:24} {:>14} {:>14} {:>14}", "", "NDA/shmem", "JSON/stdio", "JSON/shmem");
    if let Some(js) = shmem_json {
        println!("  {:24} {:>12.3} ms {:>12.3} ms {:>12.3} ms", "Avg latency", nda_avg, stdio_avg, js.avg_ms());
        println!("  {:24} {:>12.3} ms {:>12.3} ms {:>12.3} ms", "p50", nda.percentile(50.0), stdio.percentile(50.0), js.percentile(50.0));
        println!("  {:24} {:>12.3} ms {:>12.3} ms {:>12.3} ms", "p95", nda.percentile(95.0), stdio.percentile(95.0), js.percentile(95.0));
        println!("  {:24} {:>12.3} ms {:>12.3} ms {:>12.3} ms", "p99", nda_p99, stdio_p99, js.percentile(99.0));
        println!("  {:24} {:>12.0} r/s {:>12.0} r/s {:>12.0} r/s", "Throughput", nda.throughput(), stdio.throughput(), js.throughput());
    } else {
        println!("  {:24} {:>12.3} ms {:>12.3} ms", "Avg latency", nda_avg, stdio_avg);
        println!("  {:24} {:>12.3} ms {:>12.3} ms", "p50", nda.percentile(50.0), stdio.percentile(50.0));
        println!("  {:24} {:>12.3} ms {:>12.3} ms", "p95", nda.percentile(95.0), stdio.percentile(95.0));
        println!("  {:24} {:>12.3} ms {:>12.3} ms", "p99", nda_p99, stdio_p99);
        println!("  {:24} {:>12.0} r/s {:>12.0} r/s", "Throughput", nda.throughput(), stdio.throughput());
    }
    println!();
    println!("  Avg speedup:  {:.1}x faster (NDA vs stdio JSON-RPC)", avg_speedup);
    println!("  P99 speedup:  {:.1}x faster", p99_speedup);
    if let Some(js) = shmem_json {
        let js_avg = js.avg_ms();
        let encoding_speedup = js_avg / nda_avg;
        let transport_speedup = stdio_avg / nda_avg;
        println!("  Encoding speedup: {:.1}x (binary TLV vs JSON, same shmem transport)", encoding_speedup);
        println!("  Transport speedup: {:.1}x (shmem vs stdio pipes, same JSON encoding)", transport_speedup);
    }
    println!();
}

// ─── Main ─────────────────────────────────────────────────────────────────

fn main() {
    // Improve Windows timer resolution from 15.6ms to 1ms for accurate measurements
    #[cfg(target_os = "windows")]
    unsafe { timeBeginPeriod(1); }

    let args: Vec<String> = std::env::args().collect();

    // Peer mode for the cross-process event ping-pong microbenchmark.
    #[cfg(target_os = "windows")]
    if args.get(1).map(|s| s.as_str()) == Some("peer") {
        run_pingpong_peer(&args[2], &args[3], args[4].parse().expect("peer: bad iter count"));
        unsafe { timeEndPeriod(1); }
        return;
    }

    let iterations: usize = args.get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(500);

    let server_path = if let Some(p) = args.get(2) {
        p.clone()
    } else {
        let default = "target/release/velocity_mcp.exe";
        if !std::path::Path::new(default).exists() {
            eprintln!("Error: server binary not found at {}", default);
            eprintln!("Build it first: cargo build --release");
            std::process::exit(1);
        }
        default.to_string()
    };

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let buffer_path = format!("nda_bench_{}.bin", ts);

    println!("========================================================================");
    println!("  NDA-Native Benchmark: Binary-over-Shmem vs JSON-RPC-over-Stdio");
    println!("  VELOCITY-MCP v3.0.0 — {} iterations × {} rounds (median) per method", iterations, ROUNDS);
    println!("========================================================================");
    println!();
    println!("  Server: {}", server_path);
    println!("  Buffer: {}", buffer_path);
    println!("  Shmem input limit: {} bytes, output limit: {} bytes",
             OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET,
             TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET);
    println!();

    let _ = std::fs::remove_file(&buffer_path);

    println!("  Transport primitive costs (this machine):");
    run_microbenchmarks(&std::env::current_exe().expect("cannot locate own exe").to_string_lossy());
    println!();

    // ─── Phase 1: NDA-native over shared memory ──────────────────────────

    println!("Starting NDA shmem server...");
    let nda_server = spawn_shmem_server(&server_path, &buffer_path, &[]);
    wait_for_buffer_file(&buffer_path);

    let mut nda = NdaShmemClient::new(&buffer_path);

    println!("Warming up NDA path...");
    let init_frame = build_nda_request(METHOD_INITIALIZE, 0, &serde_json::json!({}));
    nda.send_request(&init_frame);
    let notif_frame = build_nda_request(NOTIF_INITIALIZED, 1, &serde_json::Value::Null);
    nda.send_notification(&notif_frame);

    for _ in 0..10 {
        let f = build_nda_request(METHOD_PING, 99, &serde_json::Value::Null);
        nda.send_request(&f);
    }

    println!("Running NDA-native benchmarks... ({} rounds, median kept)", ROUNDS);
    let nda_ping = median_round(ROUNDS, || bench_nda_ping(&mut nda, iterations));
    let nda_tools_list = median_round(ROUNDS, || bench_nda_tools_list(&mut nda, iterations));
    let nda_tools_call = median_round(ROUNDS, || bench_nda_tools_call(&mut nda, iterations, "bench_echo", &serde_json::json!({"size": 64})));
    let nda_health = median_round(ROUNDS, || bench_nda_health(&mut nda, iterations));

    // Semantic verification: decode the tools/list result TLV and check its
    // CONTENT (tool names), not just the integrity fields.
    let nda_tool_names: BTreeSet<String> = {
        let probe = build_nda_request(METHOD_TOOLS_LIST, 7, &serde_json::Value::Null);
        let resp = nda.send_request(&probe);
        let names = decode_tools_list_names(&resp, 7, "nda_tools_list_semantic");
        assert!(names.contains("read_nda"), "NDA tools/list missing built-in read_nda");
        assert!(names.contains("bench_echo"), "NDA tools/list missing built-in bench_echo");
        names
    };
    println!(
        "  Semantic check: NDA tools/list decoded to {} tools (incl. read_nda, bench_echo) — OK",
        nda_tool_names.len()
    );

    let nda_echo_256 = median_round(ROUNDS, || bench_nda_tools_call(&mut nda, iterations, "bench_echo", &serde_json::json!({"size": 256})));
    let nda_echo_1024 = median_round(ROUNDS, || bench_nda_tools_call(&mut nda, iterations, "bench_echo", &serde_json::json!({"size": 1024})));
    let nda_echo_4096 = median_round(ROUNDS, || bench_nda_tools_call(&mut nda, iterations, "bench_echo", &serde_json::json!({"size": 4096})));

    drop(nda);
    drop(nda_server);

    // ─── Phase 2: JSON-RPC over stdio ────────────────────────────────────

    println!("Starting JSON-RPC stdio server...");
    let mut stdio = StdioClient::new(&server_path);

    println!("Warming up stdio path...");
    let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
    stdio.send_request(&init);
    // notifications/initialized is a notification - server sends no response
    let notif = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
    let notif_str = serde_json::to_string(&notif).unwrap() + "\n";
    let stdin = stdio.child.stdin.as_mut().unwrap();
    stdin.write_all(notif_str.as_bytes()).unwrap();
    stdin.flush().unwrap();

    for i in 0..10 {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"ping","params":{},"id":i+10});
        stdio.send_request(&req);
    }

    println!("Running stdio benchmarks... ({} rounds, median kept)", ROUNDS);
    let stdio_ping = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "ping", &serde_json::json!({})));
    let stdio_tools_list = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "tools/list", &serde_json::json!({})));
    let stdio_tools_call = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 64}})));
    let stdio_health = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "health/check", &serde_json::json!({})));

    let stdio_echo_256 = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 256}})));
    let stdio_echo_1024 = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 1024}})));
    let stdio_echo_4096 = median_round(ROUNDS, || bench_stdio(&mut stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 4096}})));

    drop(stdio);

    // ─── Phase 3: JSON-over-shmem (isolates encoding vs transport) ───────

    println!("Starting shmem server for JSON-over-shmem test...");
    let json_server = spawn_shmem_server(&server_path, &buffer_path, &[]);
    wait_for_buffer_file(&buffer_path);

    let mut json_shmem = JsonShmemClient::new(&buffer_path);

    let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
    json_shmem.send_json(&init);
    let notif = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{},"id":1});
    json_shmem.send_json(&notif);

    for i in 0..10 {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"ping","params":{},"id":i+10});
        json_shmem.send_json(&req);
    }

    println!("Running JSON-over-shmem benchmarks... ({} rounds, median kept)", ROUNDS);
    let js_ping = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "ping", &serde_json::json!({})));
    let js_tools_list = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "tools/list", &serde_json::json!({})));

    // Cross-transport semantic check: JSON/shmem tools/list must name exactly
    // the same tools the NDA/shmem path decoded in Phase 1.
    {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"tools/list","params":{},"id":9999});
        let resp_str = json_shmem.send_json(&req);
        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect("JSON/shmem tools/list semantic probe returned invalid JSON");
        let tools = resp["result"]["tools"]
            .as_array()
            .expect("JSON/shmem tools/list result.tools missing");
        let js_names: BTreeSet<String> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("JSON tool without name").to_string())
            .collect();
        assert_eq!(
            js_names, nda_tool_names,
            "NDA and JSON/shmem tools/list disagree on tool set"
        );
        println!(
            "  Semantic check: JSON/shmem tools/list matches NDA tool set ({} tools) — OK",
            js_names.len()
        );
    }
    let js_tools_call = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 64}})));
    let js_health = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "health/check", &serde_json::json!({})));

    let js_echo_256 = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 256}})));
    let js_echo_1024 = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 1024}})));
    let js_echo_4096 = median_round(ROUNDS, || bench_json_shmem(&mut json_shmem, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 4096}})));

    drop(json_shmem);
    drop(json_server);

    // ─── Phase 4: tools/list payload scaling ─────────────────────────────
    //
    // Inflates the registry via VELOCITY_BENCH_EXTRA_TOOLS and measures how
    // tools/list latency scales with payload size, NDA/shmem vs JSON/shmem.
    // JSON/stdio is excluded: its tools/list paginates at 100 tools, so the
    // payloads would not be comparable.

    struct ScalingRow {
        extra: usize,
        tool_count: usize,
        nda_bytes: usize,
        json_bytes: usize,
        nda: BenchResult,
        js: BenchResult,
    }

    const SCALING_SIZES: [usize; 4] = [0, 32, 64, 128];
    let scaling_iters = iterations.min(200);
    let mut scaling_rows: Vec<ScalingRow> = Vec::new();
    let mut base_tool_count: Option<usize> = None;

    println!();
    println!("Running tools/list payload-scaling benchmark...");
    for &extra in &SCALING_SIZES {
        let env_val = extra.to_string();
        let extra_env = [("VELOCITY_BENCH_EXTRA_TOOLS", env_val.as_str())];

        // NDA/shmem at this registry size
        let (nda_names, nda_bytes, nda_res) = {
            let guard = spawn_shmem_server(&server_path, &buffer_path, &extra_env);
            wait_for_buffer_file(&buffer_path);
            let mut client = NdaShmemClient::new(&buffer_path);

            let init_frame = build_nda_request(METHOD_INITIALIZE, 0, &serde_json::json!({}));
            client.send_request(&init_frame);
            let frame = build_nda_request(METHOD_TOOLS_LIST, 1, &serde_json::Value::Null);
            for _ in 0..10 {
                client.send_request(&frame); // warm the generation-keyed cache
            }

            let resp = client.send_request(&frame);
            let names = decode_tools_list_names(&resp, 1, &format!("scale_nda[{}]", extra));
            let count = names.len();
            match base_tool_count {
                None => base_tool_count = Some(count),
                Some(base) => assert_eq!(
                    count, base + extra,
                    "NDA registry size wrong: expected {} tools, got {}", base + extra, count
                ),
            }
            let nbytes = resp.len();
            let res = median_round(ROUNDS, || bench_nda_tools_list(&mut client, scaling_iters));
            drop(client);
            drop(guard);
            (names, nbytes, res)
        };

        // JSON/shmem at the same registry size
        let (json_bytes, js_res) = {
            let guard = spawn_shmem_server(&server_path, &buffer_path, &extra_env);
            wait_for_buffer_file(&buffer_path);
            let mut client = JsonShmemClient::new(&buffer_path);

            let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
            client.send_json(&init);
            let req = serde_json::json!({"jsonrpc":"2.0","method":"tools/list","params":{},"id":1});
            for _ in 0..10 {
                client.send_json(&req);
            }

            let resp_str = client.send_json(&req);
            let resp: serde_json::Value = serde_json::from_str(&resp_str)
                .expect("JSON/shmem scaling probe returned invalid JSON");
            let tools = resp["result"]["tools"]
                .as_array()
                .expect("JSON/shmem scaling probe: result.tools missing");
            let js_names: BTreeSet<String> = tools
                .iter()
                .map(|t| t["name"].as_str().expect("JSON tool without name").to_string())
                .collect();
            assert_eq!(
                js_names, nda_names,
                "tools/list name sets disagree at {} extra tools", extra
            );
            let nbytes = resp_str.len();
            let res = median_round(ROUNDS, || bench_json_shmem(&mut client, scaling_iters, "tools/list", &serde_json::json!({})));
            drop(client);
            drop(guard);
            (nbytes, res)
        };

        println!(
            "  +{} tools: {} tools total — NDA {} B vs JSON {} B, sets match ({} names)",
            extra, nda_names.len(), nda_bytes, json_bytes, nda_names.len()
        );
        scaling_rows.push(ScalingRow {
            extra,
            tool_count: nda_names.len(),
            nda_bytes,
            json_bytes,
            nda: nda_res,
            js: js_res,
        });
    }

    // ─── Results ─────────────────────────────────────────────────────────

    println!();
    println!("========================================================================");
    println!("  RESULTS");
    println!("========================================================================");
    println!();
    println!("  Three transport modes compared:");
    println!("    NDA/shmem    = NDA binary TLV over shared memory (zero-poll Win32 events)");
    println!("    JSON/stdio   = JSON-RPC over stdin/stdout pipes (thread + channel + 200ms poll)");
    println!("    JSON/shmem   = JSON-RPC over shared memory (isolates encoding cost)");
    println!();

    print_comparison("Ping", &nda_ping, &stdio_ping, Some(&js_ping));
    print_comparison("Tools/List", &nda_tools_list, &stdio_tools_list, Some(&js_tools_list));
    print_comparison("Tools/Call (64B)", &nda_tools_call, &stdio_tools_call, Some(&js_tools_call));
    print_comparison("Health/Check", &nda_health, &stdio_health, Some(&js_health));

    // Payload scaling
    println!("─── Payload Scaling: bench_echo ──────────────────────────────────────────");
    println!();
    println!("  {:>10}  {:>12} {:>12} {:>12}  {:>10} {:>10}", "Payload", "NDA/shmem", "JSON/stdio", "JSON/shmem", "Avg Δ", "P99 Δ");
    println!("  {}", "─".repeat(76));

    let scaling_data = [
        ("256 B", &nda_echo_256, &stdio_echo_256, &js_echo_256),
        ("1 KB", &nda_echo_1024, &stdio_echo_1024, &js_echo_1024),
        ("4 KB", &nda_echo_4096, &stdio_echo_4096, &js_echo_4096),
    ];

    for (label, nda_r, stdio_r, js_r) in &scaling_data {
        let nda_avg = nda_r.avg_ms();
        let stdio_avg = stdio_r.avg_ms();
        let js_avg = js_r.avg_ms();
        let nda_p99 = nda_r.percentile(99.0);
        let stdio_p99 = stdio_r.percentile(99.0);
        let avg_speedup = stdio_avg / nda_avg;
        let p99_speedup = stdio_p99 / nda_p99;

        println!("  {:>10}  {:>10.3} ms {:>10.3} ms {:>10.3} ms  {:>8.1}x {:>8.1}x",
                 label, nda_avg, stdio_avg, js_avg, avg_speedup, p99_speedup);
    }

    println!();
    println!("─── tools/list Payload Scaling: registry size (NDA/shmem vs JSON/shmem) ──");
    println!();
    println!("  (JSON/stdio excluded: its tools/list paginates at 100 tools,");
    println!("   so payloads would not be comparable at larger registries.)");
    println!();
    println!("  {:>10} {:>6} {:>10} {:>10}  {:>12} {:>12} {:>8}",
             "+tools", "tools", "NDA bytes", "JSON bytes", "NDA/shmem", "JSON/shmem", "Speedup");
    println!("  {}", "─".repeat(78));
    for row in &scaling_rows {
        println!("  {:>10} {:>6} {:>10} {:>10}  {:>10.3} ms {:>10.3} ms {:>7.1}x",
                 row.extra, row.tool_count, row.nda_bytes, row.json_bytes,
                 row.nda.avg_ms(), row.js.avg_ms(),
                 row.js.avg_ms() / row.nda.avg_ms());
    }

    println!();

    // Overall summary
    let nda_avg_all = (nda_ping.avg_ms() + nda_tools_call.avg_ms() + nda_health.avg_ms()) / 3.0;
    let stdio_avg_all = (stdio_ping.avg_ms() + stdio_tools_call.avg_ms() + stdio_health.avg_ms()) / 3.0;
    let nda_p99_all = (nda_ping.percentile(99.0) + nda_tools_call.percentile(99.0) + nda_health.percentile(99.0)) / 3.0;
    let stdio_p99_all = (stdio_ping.percentile(99.0) + stdio_tools_call.percentile(99.0) + stdio_health.percentile(99.0)) / 3.0;

    println!("========================================================================");
    println!("  SUMMARY");
    println!("========================================================================");
    println!();
    println!("  NDA-binary/shmem avg: {:.3} ms   (binary TLV frame parse, Win32 events)", nda_avg_all);
    println!("  JSON-RPC/stdio avg:   {:.3} ms   (serde_json + pipe + thread channel)", stdio_avg_all);
    println!("  Overall avg speedup:  {:.1}x faster (NDA vs stdio JSON-RPC)", stdio_avg_all / nda_avg_all);
    println!("  Overall p99 speedup:  {:.1}x faster", stdio_p99_all / nda_p99_all);
    println!();
    println!("  What's being measured:");
    println!("    NDA path:   TLV encode → SHA-256 → mmap write → SetEvent → server processes");
    println!("                → mmap write → SetEvent → client reads. No JSON parsing anywhere.");
    println!("    Stdio path: JSON stringify → pipe write → thread recv → channel send →");
    println!("                serde_json parse → handle → serde_json stringify → pipe write →");
    println!("                pipe read → JSON parse. Two JSON serialize+parse cycles per request.");
    println!();
    println!("  Shmem buffer limits: input ≤ {} bytes, output ≤ {} bytes",
             OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET,
             TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET);
    println!("  (4KB input / 61KB output — sufficient for typical MCP tool calls)");

    // Restore Windows timer resolution
    #[cfg(target_os = "windows")]
    unsafe { timeEndPeriod(1); }
}
