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
#[allow(dead_code)]
const REQUEST_SEQ_OFFSET: usize = 9;
const INPUT_BUFFER_OFFSET: usize = 16;
const OUTPUT_BUFFER_OFFSET: usize = 4096;
const TOTAL_BUFFER_SIZE: usize = 65536;

const STATE_REQ_READY: u8 = 1;
const STATE_RES_READY: u8 = 3;

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

    fn wait_for_response(&self) {
        let budget = spin_budget_us();
        if budget > 0 {
            let start = std::time::Instant::now();
            let limit = std::time::Duration::from_micros(budget);
            loop {
                let state = unsafe {
                    let ptr = self.mmap.as_ptr().add(STATE_OFFSET);
                    *ptr
                };
                if state == STATE_RES_READY {
                    return;
                }
                if start.elapsed() >= limit {
                    break;
                }
                std::hint::spin_loop();
            }
        }
        unsafe {
            let rc = WaitForSingleObject(self.h_res_event, WAIT_TIMEOUT_MS);
            assert_eq!(rc, 0, "Timed out waiting for server response ({} ms)", WAIT_TIMEOUT_MS);
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
        }
        self.wait_for_response();

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
        }
        self.wait_for_response();
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
        let (resp, _, _, _) = self.send_json_phased(request);
        resp
    }

    fn send_json_phased(&mut self, request: &serde_json::Value) -> (String, f64, f64, f64) {
        let t_write = Instant::now();
        let json_str = serde_json::to_string(request).unwrap();
        let bytes = json_str.as_bytes();
        self.inner.mmap[INPUT_LEN_OFFSET..INPUT_LEN_OFFSET + 4]
            .copy_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.inner.mmap[INPUT_BUFFER_OFFSET..INPUT_BUFFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        std::sync::atomic::fence(Ordering::SeqCst);
        self.inner.mmap[STATE_OFFSET] = STATE_REQ_READY;
        let t_wait = Instant::now();

        unsafe {
            SetEvent(self.inner.h_req_event);
            let rc = WaitForSingleObject(self.inner.h_res_event, WAIT_TIMEOUT_MS);
            assert_eq!(rc, 0, "Timed out waiting for server response ({} ms)", WAIT_TIMEOUT_MS);
        }
        let t_read = Instant::now();

        std::sync::atomic::fence(Ordering::SeqCst);
        let out_len = u32::from_le_bytes([
            self.inner.mmap[OUTPUT_LEN_OFFSET],
            self.inner.mmap[OUTPUT_LEN_OFFSET + 1],
            self.inner.mmap[OUTPUT_LEN_OFFSET + 2],
            self.inner.mmap[OUTPUT_LEN_OFFSET + 3],
        ]) as usize;
        let result = String::from_utf8(
            self.inner.mmap[OUTPUT_BUFFER_OFFSET..OUTPUT_BUFFER_OFFSET + out_len].to_vec()
        ).unwrap();
        let t_done = Instant::now();

        (
            result,
            t_wait.duration_since(t_write).as_secs_f64() * 1_000_000.0,
            t_read.duration_since(t_wait).as_secs_f64() * 1_000_000.0,
            t_done.duration_since(t_read).as_secs_f64() * 1_000_000.0,
        )
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
        let (resp, _, _, _) = self.send_request_phased(request);
        resp
    }

    fn send_request_phased(&mut self, request: &serde_json::Value) -> (String, f64, f64, f64) {
        let t_encode = Instant::now();
        let json_str = serde_json::to_string(request).unwrap() + "\n";
        let encode_us = t_encode.elapsed().as_secs_f64() * 1_000_000.0;

        let t_write = Instant::now();
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(json_str.as_bytes()).unwrap();
        stdin.flush().unwrap();
        let write_us = t_write.elapsed().as_secs_f64() * 1_000_000.0;

        let t_read = Instant::now();
        let result = loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                break trimmed.to_string();
            }
        };
        let read_us = t_read.elapsed().as_secs_f64() * 1_000_000.0;

        (result, encode_us, write_us, read_us)
    }
}

impl Drop for StdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ─── NDA-over-Stdio Client (NDA binary frames over stdin/stdout pipes) ────

struct NdaStdioClient {
    child: Child,
    reader: std::io::BufReader<std::process::ChildStdout>,
}

impl NdaStdioClient {
    fn new(server_path: &str) -> Self {
        let mut child = Command::new(server_path)
            .arg("--mode").arg("stdio")
            .env("VELOCITY_RATE_LIMIT", "1000000")
            .env("VELOCITY_RATE_BURST", "1000000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start NDA stdio server");

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        NdaStdioClient { child, reader }
    }

    fn send_request(&mut self, request: &serde_json::Value) -> String {
        let (resp, _, _, _) = self.send_request_phased(request);
        resp
    }

    fn send_request_phased(&mut self, request: &serde_json::Value) -> (String, f64, f64, f64) {
        use std::io::Read;

        let t_build = Instant::now();
        let method_str = request["method"].as_str().unwrap_or("");
        let method_code = match method_str {
            "initialize" => METHOD_INITIALIZE,
            "notifications/initialized" => NOTIF_INITIALIZED,
            "ping" => METHOD_PING,
            "tools/list" => METHOD_TOOLS_LIST,
            "tools/call" => METHOD_TOOLS_CALL,
            "health/check" => METHOD_HEALTH_CHECK,
            _ => 0xFF,
        };
        let id = request["id"].as_u64().unwrap_or(0);
        let params = &request["params"];
        let frame = build_nda_request(method_code, id, params);
        let build_us = t_build.elapsed().as_secs_f64() * 1_000_000.0;

        let t_write = Instant::now();
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(&(frame.len() as u32).to_be_bytes()).unwrap();
        stdin.write_all(&frame).unwrap();
        stdin.flush().unwrap();
        let write_us = t_write.elapsed().as_secs_f64() * 1_000_000.0;

        let t_read = Instant::now();
        let mut len_buf = [0u8; 4];
        self.reader.read_exact(&mut len_buf).unwrap();
        let frame_len = u32::from_be_bytes(len_buf) as usize;
        let mut resp_frame = vec![0u8; frame_len];
        self.reader.read_exact(&mut resp_frame).unwrap();
        assert_eq!(&resp_frame[0..4], b"NMCP", "Invalid NDA response magic");
        let payload = &resp_frame[36..];
        let result = if payload.len() > 1 {
            let mut offset = 1;
            let id_consumed = skip_tlv_size(&payload[offset..]);
            offset += id_consumed;
            if offset < payload.len() {
                let (json_val, _) = decode_json_value_from_tlv(&payload[offset..]);
                serde_json::to_string(&json_val).unwrap()
            } else {
                "{}".to_string()
            }
        } else {
            "{}".to_string()
        };
        let read_us = t_read.elapsed().as_secs_f64() * 1_000_000.0;

        (result, build_us, write_us, read_us)
    }
}

// ─── HTTP Client (JSON-RPC over HTTP using ureq) ────────────────────────────

struct HttpClient {
    url: String,
    agent: ureq::Agent,
}

impl HttpClient {
    fn new(port: u16) -> Self {
        HttpClient {
            url: format!("http://127.0.0.1:{}/v1/mcp", port),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(5))
                .build(),
        }
    }

    fn send_request(&mut self, request: &serde_json::Value) -> String {
        let (resp, _, _, _) = self.send_request_phased(request);
        resp
    }

    fn send_request_phased(&mut self, request: &serde_json::Value) -> (String, f64, f64, f64) {
        let t_encode = Instant::now();
        let body = serde_json::to_string(request).unwrap();
        let encode_us = t_encode.elapsed().as_secs_f64() * 1_000_000.0;

        let t_send = Instant::now();
        let resp = self.agent
            .post(&self.url)
            .set("Content-Type", "application/json")
            .send_string(&body)
            .unwrap_or_else(|e| panic!("HTTP request failed: {}", e));
        let send_us = t_send.elapsed().as_secs_f64() * 1_000_000.0;

        let t_read = Instant::now();
        let result = resp.into_string().unwrap_or_else(|e| panic!("Failed to read HTTP response: {}", e));
        let read_us = t_read.elapsed().as_secs_f64() * 1_000_000.0;

        (result, encode_us, send_us, read_us)
    }
}

// ─── NDA/HTTP Client ─────────────────────────────────────────────────────────

struct NdaHttpClient {
    url: String,
    agent: ureq::Agent,
}

impl NdaHttpClient {
    fn new(port: u16) -> Self {
        NdaHttpClient {
            url: format!("http://127.0.0.1:{}/v1/mcp/nda", port),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(5))
                .timeout_read(Duration::from_secs(5))
                .build(),
        }
    }

    fn send_nda_request(&mut self, method_code: u8, id: u64, data: &serde_json::Value) -> Vec<u8> {
        let (resp, _, _, _) = self.send_nda_request_phased(method_code, id, data);
        resp
    }

    fn send_nda_request_phased(&mut self, method_code: u8, id: u64, data: &serde_json::Value) -> (Vec<u8>, f64, f64, f64) {
        use std::io::Read;
        // Phase 1: Build NDA frame (TLV encode + SHA-256)
        let t_build = Instant::now();
        let mut payload = Vec::new();
        payload.push(method_code);
        
        // Encode ID as TLV integer
        payload.push(0x02); // Integer type
        payload.extend_from_slice(&(id as i64).to_be_bytes());
        
        // Encode data if not null
        if !data.is_null() {
            encode_json_value_to_tlv(data, &mut payload);
        }
        
        // Build frame with magic + merkle + payload
        let mut frame = Vec::new();
        frame.extend_from_slice(b"NMCP"); // magic
        
        // Compute SHA-256 of payload
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        let merkle = hasher.finalize();
        frame.extend_from_slice(&merkle);
        
        frame.extend_from_slice(&payload);
        let frame_build_us = t_build.elapsed().as_secs_f64() * 1_000_000.0;
        
        // Phase 2: HTTP POST (send request + wait for response headers)
        let t_http = Instant::now();
        let resp = self.agent
            .post(&self.url)
            .set("Content-Type", "application/octet-stream")
            .send_bytes(&frame)
            .unwrap_or_else(|e| panic!("NDA/HTTP request failed: {}", e));
        let http_send_us = t_http.elapsed().as_secs_f64() * 1_000_000.0;
        
        // Phase 3: Read response body
        let t_read = Instant::now();
        let mut buf = Vec::new();
        resp.into_reader()
            .read_to_end(&mut buf)
            .unwrap_or_else(|e| panic!("Failed to read NDA/HTTP response: {}", e));
        let response_read_us = t_read.elapsed().as_secs_f64() * 1_000_000.0;
        
        (buf, frame_build_us, http_send_us, response_read_us)
    }
}

fn encode_json_value_to_tlv(value: &serde_json::Value, buf: &mut Vec<u8>) {
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
                encode_json_value_to_tlv(item, buf);
            }
        }
        serde_json::Value::Object(obj) => {
            buf.push(0x06);
            buf.extend_from_slice(&(obj.len() as u32).to_be_bytes());
            for (k, v) in obj {
                buf.extend_from_slice(&(k.len() as u16).to_be_bytes());
                buf.extend_from_slice(k.as_bytes());
                encode_json_value_to_tlv(v, buf);
            }
        }
    }
}

fn spawn_http_server(server_path: &str, port: u16) -> Child {
    let child = Command::new(server_path)
        .arg("--mode").arg("http")
        .arg("--addr").arg(format!("127.0.0.1:{}", port))
        .env("VELOCITY_RATE_LIMIT", "1000000")
        .env("VELOCITY_RATE_BURST", "1000000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start HTTP server");

    // Wait for server to accept connections, then give it extra time to initialize
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(50));
        if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
            std::thread::sleep(Duration::from_millis(200));
            return child;
        }
    }
    panic!("HTTP server didn't start on port {} within 5s", port);
}

// ─── Path Resolution ─────────────────────────────────────────────────────────

/// Find bench_nodejs/server.js relative to CWD or the executable location.
fn resolve_node_server() -> String {
    // Try CWD-relative first (normal case when running from workspace root)
    let cwd_path = "bench_nodejs/server.js";
    if std::path::Path::new(cwd_path).exists() {
        return cwd_path.to_string();
    }

    // Try relative to the executable (bench_nda/target/release/bench_nda.exe -> ../../bench_nodejs/server.js)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("../../bench_nodejs/server.js");
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    panic!("Cannot find bench_nodejs/server.js. Run from workspace root or ensure server.js is accessible.");
}

// ─── Node.js JSON/stdio Client ───────────────────────────────────────────────

struct NodeJsStdioClient {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}

impl NodeJsStdioClient {
    fn new(server_js_path: &str) -> Self {
        let mut child = Command::new("node")
            .arg(server_js_path)
            .env("VELOCITY_RATE_LIMIT", "1000000")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start Node.js server (is 'node' in PATH?)");

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        NodeJsStdioClient { child, reader }
    }

    fn send_request(&mut self, request: &serde_json::Value) -> String {
        let (resp, _, _, _) = self.send_request_phased(request);
        resp
    }

    fn send_request_phased(&mut self, request: &serde_json::Value) -> (String, f64, f64, f64) {
        let t_encode = Instant::now();
        let json_str = serde_json::to_string(request).unwrap() + "\n";
        let encode_us = t_encode.elapsed().as_secs_f64() * 1_000_000.0;

        let t_write = Instant::now();
        let stdin = self.child.stdin.as_mut().unwrap();
        stdin.write_all(json_str.as_bytes()).unwrap();
        stdin.flush().unwrap();
        let write_us = t_write.elapsed().as_secs_f64() * 1_000_000.0;

        let t_read = Instant::now();
        let result = loop {
            let mut line = String::new();
            self.reader.read_line(&mut line).unwrap();
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
                break trimmed.to_string();
            }
        };
        let read_us = t_read.elapsed().as_secs_f64() * 1_000_000.0;

        (result, encode_us, write_us, read_us)
    }
}

impl Drop for NodeJsStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ─── Node.js JSON/HTTP Client ────────────────────────────────────────────────

struct NodeJsHttpClient {
    child: Child,
    inner: HttpClient,
}

impl NodeJsHttpClient {
    fn new(server_js_path: &str, port: u16) -> Self {
        let mut child = Command::new("node")
            .arg(server_js_path)
            .arg("--http")
            .arg(port.to_string())
            .env("VELOCITY_RATE_LIMIT", "1000000")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Failed to start Node.js HTTP server");

        // Wait for server to accept connections
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            if std::net::TcpStream::connect(format!("127.0.0.1:{}", port)).is_ok() {
                std::thread::sleep(Duration::from_millis(200));
                return NodeJsHttpClient {
                    child,
                    inner: HttpClient::new(port),
                };
            }
        }
        let _ = child.kill();
        panic!("Node.js HTTP server didn't start on port {} within 5s", port);
    }

    fn send_request(&mut self, request: &serde_json::Value) -> String {
        let (resp, _, _, _) = self.send_request_phased(request);
        resp
    }

    fn send_request_phased(&mut self, request: &serde_json::Value) -> (String, f64, f64, f64) {
        self.inner.send_request_phased(request)
    }
}

impl Drop for NodeJsHttpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Compute the byte size of a single TLV value (type tag + length + data).
fn skip_tlv_size(buf: &[u8]) -> usize {
    if buf.is_empty() { return 0; }
    match buf[0] {
        0x01 => { // String
            if buf.len() < 5 { return buf.len(); }
            let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            5 + len
        }
        0x02 => 9, // Integer (tag + 8 bytes)
        0x03 => 2, // Bool (tag + 1 byte)
        0x04 => 1, // Null (tag only)
        0x05 => { // Array
            if buf.len() < 5 { return buf.len(); }
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let mut offset = 5;
            for _ in 0..count {
                offset += skip_tlv_size(&buf[offset..]);
            }
            offset
        }
        0x06 => { // Object
            if buf.len() < 5 { return buf.len(); }
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let mut offset = 5;
            for _ in 0..count {
                if offset + 2 > buf.len() { break; }
                let klen = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
                offset += 2 + klen;
                offset += skip_tlv_size(&buf[offset..]);
            }
            offset
        }
        0x07 => 9, // Float64
        _ => buf.len(),
    }
}

impl Drop for NdaStdioClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn decode_json_value_from_tlv(buf: &[u8]) -> (serde_json::Value, usize) {
    if buf.is_empty() {
        return (serde_json::Value::Null, 0);
    }
    
    let type_code = buf[0];
    match type_code {
        0x01 => { // String
            let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let s = String::from_utf8_lossy(&buf[5..5 + len]).to_string();
            (serde_json::Value::String(s), 5 + len)
        }
        0x02 => { // Integer
            let i = i64::from_be_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]);
            (serde_json::Value::Number(serde_json::Number::from(i)), 9)
        }
        0x03 => { // Bool
            (serde_json::Value::Bool(buf[1] != 0), 2)
        }
        0x04 => { // Null
            (serde_json::Value::Null, 1)
        }
        0x05 => { // Array
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let mut arr = Vec::new();
            let mut offset = 5;
            for _ in 0..count {
                let (val, consumed) = decode_json_value_from_tlv(&buf[offset..]);
                arr.push(val);
                offset += consumed;
            }
            (serde_json::Value::Array(arr), offset)
        }
        0x06 => { // Object
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            let mut obj = serde_json::Map::new();
            let mut offset = 5;
            for _ in 0..count {
                let key_len = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
                offset += 2;
                let key = String::from_utf8_lossy(&buf[offset..offset + key_len]).to_string();
                offset += key_len;
                let (val, consumed) = decode_json_value_from_tlv(&buf[offset..]);
                obj.insert(key, val);
                offset += consumed;
            }
            (serde_json::Value::Object(obj), offset)
        }
        _ => (serde_json::Value::Null, 0)
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

    // 4. SHA-256 throughput on an 8KB payload — the size of a 16-tool
    // tools/list frame. sha2 auto-dispatches to SHA-NI on x86_64 when the
    // CPU has the extensions; software fallback is ~0.3-0.5 GB/s, SHA-NI
    // is several GB/s, so this number identifies the active backend.
    {
        let payload = vec![0xABu8; 8 * 1024];
        let mut h = Sha256::new();
        h.update(&payload);
        let _ = h.finalize(); // warm-up
        const HASH_ITERS: u32 = 20_000;
        let start = Instant::now();
        let mut digest = [0u8; 32];
        for _ in 0..HASH_ITERS {
            let mut h = Sha256::new();
            h.update(&payload);
            digest = h.finalize().into();
        }
        std::hint::black_box(&digest);
        let ns = start.elapsed().as_nanos() as f64 / HASH_ITERS as f64;
        let gbs = (8.0 * 1024.0) / ns;
        println!("  Micro: SHA-256 of 8KB payload (Merkle size):     {:.2} us  ({:.2} GB/s)", ns / 1000.0, gbs);
    }
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
    first_call_us: Option<f64>,
    second_call_us: Option<f64>,
    warm_latencies_us: Vec<f64>,
}

impl BenchResult {
    fn avg_ms(&self) -> f64 {
        if self.warm_latencies_us.is_empty() {
            0.0
        } else {
            self.warm_latencies_us.iter().sum::<f64>() / self.warm_latencies_us.len() as f64 / 1000.0
        }
    }

    fn first_call_ms(&self) -> Option<f64> {
        self.first_call_us.map(|us| us / 1000.0)
    }

    fn second_call_ms(&self) -> Option<f64> {
        self.second_call_us.map(|us| us / 1000.0)
    }

    fn warm_avg_ms(&self) -> f64 {
        self.avg_ms()
    }

    fn percentile(&self, p: f64) -> f64 {
        let mut sorted = self.warm_latencies_us.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if sorted.is_empty() {
            0.0
        } else {
            let idx = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
            sorted[idx.min(sorted.len()) - 1] / 1000.0
        }
    }

    fn throughput(&self) -> f64 {
        let total_ms: f64 = self.warm_latencies_us.iter().sum::<f64>() / 1000.0;
        if total_ms == 0.0 {
            0.0
        } else {
            self.warm_latencies_us.len() as f64 / total_ms * 1000.0
        }
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
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut write_us, mut wait_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let start = Instant::now();
        let (resp, w, wt, r) = client.send_request_phased(&frame);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        validate_nda_response(&resp, 1, "nda_ping");
        write_us += w;
        wait_us += wt;
        read_us += r;
    }
    let n = iterations as f64;
    println!(
        "    [client phases] write={:.1}us wait={:.1}us read={:.1}us",
        write_us / n, wait_us / n, read_us / n
    );

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_nda_tools_list(client: &mut NdaShmemClient, iterations: usize) -> BenchResult {
    let frame = build_nda_request(METHOD_TOOLS_LIST, 1, &serde_json::Value::Null);
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;

    for i in 0..iterations {
        let start = Instant::now();
        let resp = client.send_request(&frame);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        validate_nda_response(&resp, 1, &format!("nda_tools_list[{}]", i));
    }

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_nda_tools_call(client: &mut NdaShmemClient, iterations: usize, tool: &str, args: &serde_json::Value) -> BenchResult {
    let data = serde_json::json!({"name": tool, "arguments": args});
    let frame = build_nda_request(METHOD_TOOLS_CALL, 1, &data);
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;

    for i in 0..iterations {
        let start = Instant::now();
        let resp = client.send_request(&frame);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        validate_nda_response(&resp, 1, &format!("nda_tools_call[{}]", i));
    }

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_nda_health(client: &mut NdaShmemClient, iterations: usize) -> BenchResult {
    let frame = build_nda_request(METHOD_HEALTH_CHECK, 1, &serde_json::Value::Null);
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;

    for i in 0..iterations {
        let start = Instant::now();
        let resp = client.send_request(&frame);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        validate_nda_response(&resp, 1, &format!("nda_health[{}]", i));
    }

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_stdio(client: &mut StdioClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut encode_us, mut write_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let (resp_str, e, w, r) = client.send_request_phased(&req);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        encode_us += e;
        write_us += w;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid JSON response at iter {}: {}", i, &resp_str[..resp_str.len().min(100)]));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "Response missing result/error at iter {}", i);
        assert!(!resp_str.contains("Rate limit exceeded"),
                "stdio iter {} hit server rate limit — throttle too tight", i);
    }

    let n = iterations as f64;
    println!("    [client phases] json_encode={:.1}us pipe_write={:.1}us pipe_read={:.1}us",
        encode_us / n, write_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_json_shmem(client: &mut JsonShmemClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut write_us, mut wait_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let (resp_str, w, wt, r) = client.send_json_phased(&req);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        write_us += w;
        wait_us += wt;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid JSON-shmem response at iter {}", i));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "JSON-shmem response missing result/error at iter {}", i);
    }

    let n = iterations as f64;
    println!("    [client phases] write={:.1}us wait={:.1}us read={:.1}us",
        write_us / n, wait_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_nda_stdio(client: &mut NdaStdioClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut build_us, mut write_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let (_resp_str, b, w, r) = client.send_request_phased(&req);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        build_us += b;
        write_us += w;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }
    }

    let n = iterations as f64;
    println!("    [client phases] frame_build={:.1}us pipe_write={:.1}us pipe_read={:.1}us",
        build_us / n, write_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_http(client: &mut HttpClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut encode_us, mut send_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let (resp_str, e, s, r) = client.send_request_phased(&req);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        encode_us += e;
        send_us += s;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid JSON response at iter {}: {}", i, &resp_str[..resp_str.len().min(100)]));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "HTTP response missing result/error at iter {}", i);
    }

    let n = iterations as f64;
    println!("    [client phases] json_encode={:.1}us http_send={:.1}us response_read={:.1}us",
        encode_us / n, send_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_node_stdio(client: &mut NodeJsStdioClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut encode_us, mut write_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let (resp_str, e, w, r) = client.send_request_phased(&req);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        encode_us += e;
        write_us += w;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid Node.js stdio response at iter {}: {}", i, &resp_str[..resp_str.len().min(100)]));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "Node.js stdio response missing result/error at iter {}", i);
    }

    let n = iterations as f64;
    println!("    [client phases] json_encode={:.1}us pipe_write={:.1}us pipe_read={:.1}us",
        encode_us / n, write_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_node_http(client: &mut NodeJsHttpClient, iterations: usize, method: &str, params: &serde_json::Value) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut encode_us, mut send_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": i
        });

        let start = Instant::now();
        let (resp_str, e, s, r) = client.send_request_phased(&req);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        encode_us += e;
        send_us += s;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        let resp: serde_json::Value = serde_json::from_str(&resp_str)
            .expect(&format!("Invalid Node.js HTTP response at iter {}: {}", i, &resp_str[..resp_str.len().min(100)]));
        assert!(resp.get("result").is_some() || resp.get("error").is_some(),
                "Node.js HTTP response missing result/error at iter {}", i);
    }

    let n = iterations as f64;
    println!("    [client phases] json_encode={:.1}us http_send={:.1}us response_read={:.1}us",
        encode_us / n, send_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_nda_http(client: &mut NdaHttpClient, iterations: usize, method_code: u8) -> BenchResult {
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut build_us, mut send_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let start = Instant::now();
        let (resp, b, s, r) = client.send_nda_request_phased(method_code, i as u64, &serde_json::Value::Null);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        build_us += b;
        send_us += s;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        assert!(resp.len() >= FRAME_HEADER_SIZE + 1, "NDA/HTTP response too small: {} bytes", resp.len());
        assert_eq!(&resp[0..4], b"NMCP", "NDA/HTTP bad magic");
        let status = resp[FRAME_HEADER_SIZE];
        assert_eq!(status, STATUS_OK, "NDA/HTTP error status={} at iter {}", status, i);
    }

    let n = iterations as f64;
    println!("    [client phases] frame_build={:.1}us http_send={:.1}us response_read={:.1}us",
        build_us / n, send_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

fn bench_nda_http_call(client: &mut NdaHttpClient, iterations: usize, tool: &str, args: &serde_json::Value) -> BenchResult {
    let data = serde_json::json!({"name": tool, "arguments": args});
    let mut warm_latencies = Vec::with_capacity(iterations);
    let mut first_call_us = None;
    let mut second_call_us = None;
    let (mut build_us, mut send_us, mut read_us) = (0.0f64, 0.0f64, 0.0f64);

    for i in 0..iterations {
        let start = Instant::now();
        let (resp, b, s, r) = client.send_nda_request_phased(METHOD_TOOLS_CALL, i as u64, &data);
        let elapsed = start.elapsed();
        let latency_us = elapsed.as_secs_f64() * 1_000_000.0;
        build_us += b;
        send_us += s;
        read_us += r;

        if i == 0 {
            first_call_us = Some(latency_us);
        } else if i == 1 {
            second_call_us = Some(latency_us);
        } else {
            warm_latencies.push(latency_us);
        }

        assert!(resp.len() >= FRAME_HEADER_SIZE + 1, "NDA/HTTP tools/call response too small");
        assert_eq!(&resp[0..4], b"NMCP", "NDA/HTTP tools/call bad magic");
        let status = resp[FRAME_HEADER_SIZE];
        assert_eq!(status, STATUS_OK, "NDA/HTTP tools/call error status={} at iter {}", status, i);
    }

    let n = iterations as f64;
    println!("    [client phases] frame_build={:.1}us http_send={:.1}us response_read={:.1}us",
        build_us / n, send_us / n, read_us / n);

    BenchResult { first_call_us, second_call_us, warm_latencies_us: warm_latencies }
}

/// Measures the SHA-256 Merkle hashing cost in isolation at various payload sizes.
/// This is the overhead baked into every NDA frame (build_nda_frame).
fn bench_merkle_hash_cost(iterations: usize) -> Vec<(String, usize, f64)> {
    let sizes: [(usize, &str); 8] = [
        (32, "32 B"),
        (64, "64 B"),
        (128, "128 B"),
        (256, "256 B"),
        (1024, "1 KB"),
        (4096, "4 KB"),
        (16384, "16 KB"),
        (65536, "64 KB"),
    ];

    let mut results = Vec::new();
    for (size, label) in &sizes {
        let payload = vec![0xABu8; *size];
        let start = Instant::now();
        for _ in 0..iterations {
            let mut hasher = Sha256::new();
            hasher.update(&payload);
            let _ = hasher.finalize();
        }
        let elapsed_us = start.elapsed().as_secs_f64() * 1_000_000.0;
        let per_op_ns = (elapsed_us * 1000.0) / iterations as f64;
        results.push((label.to_string(), *size, per_op_ns));
    }
    results
}

/// Measures the full build_nda_frame cost (TLV encode + SHA-256 + vec assembly)
/// vs frame assembly without hashing, at various payload sizes.
fn bench_merkle_frame_overhead(iterations: usize) -> Vec<(String, f64, f64, f64)> {
    let sizes: [(usize, &str); 7] = [
        (0, "ping (null)"),
        (64, "64 B args"),
        (256, "256 B args"),
        (1024, "1 KB args"),
        (4096, "4 KB args"),
        (16384, "16 KB args"),
        (65536, "64 KB args"),
    ];

    let mut results = Vec::new();
    for (size, label) in &sizes {
        let payload = vec![0xABu8; *size];

        // With Merkle (full build_nda_frame)
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = build_nda_frame(&payload);
        }
        let with_merkle_us = start.elapsed().as_secs_f64() * 1_000_000.0;

        // Without Merkle (just magic + payload, no SHA-256)
        let start = Instant::now();
        for _ in 0..iterations {
            let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
            frame.extend_from_slice(NDA_MAGIC);
            frame.extend_from_slice(&[0u8; 32]); // zeroed hash slot
            frame.extend_from_slice(&payload);
            let _ = frame;
        }
        let without_merkle_us = start.elapsed().as_secs_f64() * 1_000_000.0;

        let merkle_cost_us = with_merkle_us - without_merkle_us;
        let per_op_with = (with_merkle_us * 1000.0) / iterations as f64;
        let per_op_merkle = (merkle_cost_us * 1000.0) / iterations as f64;

        results.push((label.to_string(), per_op_with, per_op_merkle, per_op_with - per_op_merkle));
    }
    results
}

// ─── Output Formatting ────────────────────────────────────────────────────

fn print_comparison(name: &str, nda: &BenchResult, stdio: &BenchResult, shmem_json: Option<&BenchResult>, nda_stdio: Option<&BenchResult>, http: Option<&BenchResult>, node_stdio: Option<&BenchResult>, node_http: Option<&BenchResult>, nda_http: Option<&BenchResult>) {
    let nda_avg = nda.avg_ms();
    let stdio_avg = stdio.avg_ms();
    let nda_p99 = nda.percentile(99.0);
    let stdio_p99 = stdio.percentile(99.0);
    let avg_speedup = stdio_avg / nda_avg;
    let p99_speedup = stdio_p99 / nda_p99;

    println!("─── {} ──────────────────────────────────────────", name);

    let has_all_8 = shmem_json.is_some() && nda_stdio.is_some() && http.is_some()
        && node_stdio.is_some() && node_http.is_some() && nda_http.is_some();

    if has_all_8 {
        let js = shmem_json.unwrap();
        let ns = nda_stdio.unwrap();
        let ht = http.unwrap();
        let njs = node_stdio.unwrap();
        let njh = node_http.unwrap();
        let nh = nda_http.unwrap();
        println!("  {:24} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}", "", "NDA/shmem", "JSON/stdio", "JSON/shmem", "NDA/stdio", "JSON/HTTP", "Node/stdio", "Node/HTTP", "NDA/HTTP");

        // First call (cold start of measurement batch)
        let fmt_opt = |v: Option<f64>| -> String {
            match v {
                Some(ms) => format!("{:>10.3} ms", ms),
                None => "         - ".to_string(),
            }
        };
        println!("  {:24} {} {} {} {} {} {} {} {}", "1st call",
            fmt_opt(nda.first_call_ms()), fmt_opt(stdio.first_call_ms()),
            fmt_opt(js.first_call_ms()), fmt_opt(ns.first_call_ms()),
            fmt_opt(ht.first_call_ms()), fmt_opt(njs.first_call_ms()),
            fmt_opt(njh.first_call_ms()), fmt_opt(nh.first_call_ms()));
        println!("  {:24} {} {} {} {} {} {} {} {}", "2nd call",
            fmt_opt(nda.second_call_ms()), fmt_opt(stdio.second_call_ms()),
            fmt_opt(js.second_call_ms()), fmt_opt(ns.second_call_ms()),
            fmt_opt(ht.second_call_ms()), fmt_opt(njs.second_call_ms()),
            fmt_opt(njh.second_call_ms()), fmt_opt(nh.second_call_ms()));

        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "Warm avg", nda_avg, stdio_avg, js.avg_ms(), ns.avg_ms(), ht.avg_ms(), njs.avg_ms(), njh.avg_ms(), nh.avg_ms());
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "Warm p50", nda.percentile(50.0), stdio.percentile(50.0), js.percentile(50.0), ns.percentile(50.0), ht.percentile(50.0), njs.percentile(50.0), njh.percentile(50.0), nh.percentile(50.0));
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "Warm p95", nda.percentile(95.0), stdio.percentile(95.0), js.percentile(95.0), ns.percentile(95.0), ht.percentile(95.0), njs.percentile(95.0), njh.percentile(95.0), nh.percentile(95.0));
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "Warm p99", nda_p99, stdio_p99, js.percentile(99.0), ns.percentile(99.0), ht.percentile(99.0), njs.percentile(99.0), njh.percentile(99.0), nh.percentile(99.0));
        println!("  {:24} {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s", "Warm throughput", nda.throughput(), stdio.throughput(), js.throughput(), ns.throughput(), ht.throughput(), njs.throughput(), njh.throughput(), nh.throughput());
    } else if shmem_json.is_some() && nda_stdio.is_some() && http.is_some() {
        let js = shmem_json.unwrap();
        let ns = nda_stdio.unwrap();
        let ht = http.unwrap();
        println!("  {:24} {:>12} {:>12} {:>12} {:>12} {:>12}", "", "NDA/shmem", "JSON/stdio", "JSON/shmem", "NDA/stdio", "JSON/HTTP");
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "Avg latency", nda_avg, stdio_avg, js.avg_ms(), ns.avg_ms(), ht.avg_ms());
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "p50", nda.percentile(50.0), stdio.percentile(50.0), js.percentile(50.0), ns.percentile(50.0), ht.percentile(50.0));
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "p95", nda.percentile(95.0), stdio.percentile(95.0), js.percentile(95.0), ns.percentile(95.0), ht.percentile(95.0));
        println!("  {:24} {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms {:>10.3} ms", "p99", nda_p99, stdio_p99, js.percentile(99.0), ns.percentile(99.0), ht.percentile(99.0));
        println!("  {:24} {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s {:>10.0} r/s", "Throughput", nda.throughput(), stdio.throughput(), js.throughput(), ns.throughput(), ht.throughput());
    } else {
        println!("  {:24} {:>12} {:>12}", "", "NDA/shmem", "JSON/stdio");
        println!("  {:24} {:>10.3} ms {:>10.3} ms", "Avg latency", nda_avg, stdio_avg);
        println!("  {:24} {:>10.3} ms {:>10.3} ms", "p50", nda.percentile(50.0), stdio.percentile(50.0));
        println!("  {:24} {:>10.3} ms {:>10.3} ms", "p95", nda.percentile(95.0), stdio.percentile(95.0));
        println!("  {:24} {:>10.3} ms {:>10.3} ms", "p99", nda_p99, stdio_p99);
        println!("  {:24} {:>10.0} r/s {:>10.0} r/s", "Throughput", nda.throughput(), stdio.throughput());
    }

    println!();
    println!("  Avg speedup:  {:.1}x faster (NDA/shmem vs JSON/stdio)", avg_speedup);
    println!("  P99 speedup:  {:.1}x faster", p99_speedup);
    if let Some(ns) = nda_stdio {
        let ns_avg = ns.avg_ms();
        let ns_p99 = ns.percentile(99.0);
        println!("  NDA/stdio vs JSON/stdio:  {:.1}x avg, {:.1}x p99 (binary encoding over pipes)", stdio_avg / ns_avg, stdio_p99 / ns_p99);
        println!("  NDA/stdio vs NDA/shmem:   {:.1}x avg, {:.1}x p99 (stdio pipe cost for binary frames)", ns_avg / nda_avg, ns_p99 / nda_p99);
    }
    if let Some(js) = shmem_json {
        let js_avg = js.avg_ms();
        let encoding_speedup = js_avg / nda_avg;
        let transport_speedup = stdio_avg / nda_avg;
        println!("  Encoding speedup: {:.1}x (binary TLV vs JSON, same shmem transport)", encoding_speedup);
        println!("  Transport speedup: {:.1}x (shmem vs stdio pipes, same JSON encoding)", transport_speedup);
    }
    if let Some(ht) = http {
        let ht_avg = ht.avg_ms();
        let ht_p99 = ht.percentile(99.0);
        println!("  JSON/HTTP vs JSON/stdio:  {:.1}x avg, {:.1}x p99 (HTTP transport overhead on JSON)", stdio_avg / ht_avg, stdio_p99 / ht_p99);
        println!("  JSON/HTTP vs NDA/shmem:   {:.1}x avg, {:.1}x p99 (full stack cost: JSON + HTTP + Axum)", ht_avg / nda_avg, ht_p99 / nda_p99);
    }
    if let Some(njs) = node_stdio {
        let njs_avg = njs.avg_ms();
        let njs_p99 = njs.percentile(99.0);
        println!("  Node/stdio vs JSON/stdio: {:.1}x avg, {:.1}x p99 (Node.js vs Rust service layer)", stdio_avg / njs_avg, stdio_p99 / njs_p99);
        println!("  Node/stdio vs NDA/shmem:  {:.1}x avg, {:.1}x p99 (full cross-stack cost)", njs_avg / nda_avg, njs_p99 / nda_p99);
    }
    if let Some(njh) = node_http {
        let njh_avg = njh.avg_ms();
        let njh_p99 = njh.percentile(99.0);
        println!("  Node/HTTP vs JSON/HTTP:   {:.1}x avg, {:.1}x p99 (Node.js vs Rust HTTP service)", http.unwrap().avg_ms() / njh_avg, http.unwrap().percentile(99.0) / njh_p99);
        println!("  Node/HTTP vs NDA/shmem:   {:.1}x avg, {:.1}x p99 (full cross-stack HTTP cost)", njh_avg / nda_avg, njh_p99 / nda_p99);
    }
    if let Some(nh) = nda_http {
        let nh_avg = nh.avg_ms();
        let nh_p99 = nh.percentile(99.0);
        println!("  NDA/HTTP vs JSON/HTTP:   {:.1}x avg, {:.1}x p99 (binary encoding savings over HTTP)", http.unwrap().avg_ms() / nh_avg, http.unwrap().percentile(99.0) / nh_p99);
        println!("  NDA/HTTP vs NDA/shmem:   {:.1}x avg, {:.1}x p99 (HTTP transport overhead for binary)", nh_avg / nda_avg, nh_p99 / nda_p99);
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

    // Cold-start measurement: first 2 calls before any warmup
    println!("Measuring NDA/shmem cold start...");
    let cold_frame = build_nda_request(METHOD_PING, 999, &serde_json::Value::Null);
    let cold_start_1 = {
        let start = Instant::now();
        let resp = nda.send_request(&cold_frame);
        let elapsed = start.elapsed();
        validate_nda_response(&resp, 999, "nda_cold_start_1");
        elapsed.as_secs_f64() * 1000.0 // ms
    };
    let cold_start_2 = {
        let start = Instant::now();
        let resp = nda.send_request(&cold_frame);
        let elapsed = start.elapsed();
        validate_nda_response(&resp, 999, "nda_cold_start_2");
        elapsed.as_secs_f64() * 1000.0
    };
    println!("  NDA/shmem cold start: 1st={:.3}ms, 2nd={:.3}ms", cold_start_1, cold_start_2);

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
    let nda_echo_16384 = median_round(ROUNDS, || bench_nda_tools_call(&mut nda, iterations, "bench_echo", &serde_json::json!({"size": 16384})));

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

    // ─── Phase 4: NDA-over-stdio (binary TLV over stdin/stdout pipes) ─────

    println!("Starting NDA-binary stdio server...");
    let mut nda_stdio = NdaStdioClient::new(&server_path);

    println!("Warming up NDA/stdio path...");
    let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
    nda_stdio.send_request(&init);

    for i in 0..10 {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"ping","params":{},"id":i+10});
        nda_stdio.send_request(&req);
    }

    println!("Running NDA/stdio benchmarks... ({} rounds, median kept)", ROUNDS);
    let ns_ping = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "ping", &serde_json::json!({})));
    let ns_tools_list = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/list", &serde_json::json!({})));
    let ns_tools_call = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 64}})));
    let ns_health = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "health/check", &serde_json::json!({})));

    let ns_echo_256 = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 256}})));
    let ns_echo_1024 = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 1024}})));
    let ns_echo_4096 = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 4096}})));
    let ns_echo_16384 = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 16384}})));
    let ns_echo_65536 = median_round(ROUNDS, || bench_nda_stdio(&mut nda_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 65536}})));

    drop(nda_stdio);

    // ─── Phase 5: JSON-RPC over HTTP/1.1 keep-alive ──────────────────────

    let http_port = 13000 + ((ts % 10000) as u16);
    println!("Starting HTTP server on port {}...", http_port);
    let mut http_child = spawn_http_server(&server_path, http_port);
    let mut http_client = HttpClient::new(http_port);

    println!("Warming up HTTP path...");
    let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
    http_client.send_request(&init);
    let notif = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
    http_client.send_request(&notif);

    for i in 0..10 {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"ping","params":{},"id":i+10});
        http_client.send_request(&req);
    }

    println!("Running JSON/HTTP benchmarks... ({} rounds, median kept)", ROUNDS);
    let http_ping = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "ping", &serde_json::json!({})));
    let http_tools_list = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "tools/list", &serde_json::json!({})));
    let http_tools_call = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 64}})));
    let http_health = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "health/check", &serde_json::json!({})));

    let http_echo_256 = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 256}})));
    let http_echo_1024 = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 1024}})));
    let http_echo_4096 = median_round(ROUNDS, || bench_http(&mut http_client, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 4096}})));

    drop(http_client);
    let _ = http_child.kill();
    let _ = http_child.wait();

    // ─── Phase 7: Node.js JSON/stdio ─────────────────────────────────────

    let node_server_path = resolve_node_server();
    println!("Starting Node.js JSON/stdio server ({})...", node_server_path);
    let mut node_stdio = NodeJsStdioClient::new(&node_server_path);

    println!("Warming up Node.js/stdio path...");
    let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
    node_stdio.send_request(&init);
    let notif = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
    let notif_str = serde_json::to_string(&notif).unwrap() + "\n";
    let stdin = node_stdio.child.stdin.as_mut().unwrap();
    stdin.write_all(notif_str.as_bytes()).unwrap();
    stdin.flush().unwrap();

    for i in 0..10 {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"ping","params":{},"id":i+10});
        node_stdio.send_request(&req);
    }

    println!("Running Node.js/stdio benchmarks... ({} rounds, median kept)", ROUNDS);
    let node_stdio_ping = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "ping", &serde_json::json!({})));
    let node_stdio_tools_list = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "tools/list", &serde_json::json!({})));
    let node_stdio_tools_call = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 64}})));
    let node_stdio_health = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "health/check", &serde_json::json!({})));

    let node_stdio_echo_256 = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 256}})));
    let node_stdio_echo_1024 = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 1024}})));
    let node_stdio_echo_4096 = median_round(ROUNDS, || bench_node_stdio(&mut node_stdio, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 4096}})));

    drop(node_stdio);

    // ─── Phase 8: Node.js JSON/HTTP ──────────────────────────────────────

    let node_http_port = 14000 + ((ts % 10000) as u16);
    println!("Starting Node.js HTTP server on port {}...", node_http_port);
    let mut node_http = NodeJsHttpClient::new(&node_server_path, node_http_port);

    println!("Warming up Node.js/HTTP path...");
    let init = serde_json::json!({"jsonrpc":"2.0","method":"initialize","params":{},"id":0});
    node_http.send_request(&init);
    let notif = serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
    node_http.send_request(&notif);

    for i in 0..10 {
        let req = serde_json::json!({"jsonrpc":"2.0","method":"ping","params":{},"id":i+10});
        node_http.send_request(&req);
    }

    println!("Running Node.js/HTTP benchmarks... ({} rounds, median kept)", ROUNDS);
    let node_http_ping = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "ping", &serde_json::json!({})));
    let node_http_tools_list = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "tools/list", &serde_json::json!({})));
    let node_http_tools_call = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 64}})));
    let node_http_health = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "health/check", &serde_json::json!({})));

    let node_http_echo_256 = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 256}})));
    let node_http_echo_1024 = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 1024}})));
    let node_http_echo_4096 = median_round(ROUNDS, || bench_node_http(&mut node_http, iterations, "tools/call",
        &serde_json::json!({"name": "bench_echo", "arguments": {"size": 4096}})));

    drop(node_http);

    // ─── Phase 9: NDA/HTTP (binary TLV over HTTP/1.1 keep-alive) ──────────

    let nda_http_port = 15000 + ((ts % 10000) as u16);
    println!("Starting HTTP server for NDA/HTTP on port {}...", nda_http_port);
    let mut nda_http_child = spawn_http_server(&server_path, nda_http_port);
    let mut nda_http_client = NdaHttpClient::new(nda_http_port);

    println!("Warming up NDA/HTTP path...");
    {
        let resp = nda_http_client.send_nda_request(METHOD_INITIALIZE, 0, &serde_json::json!({}));
        assert!(resp.len() >= FRAME_HEADER_SIZE + 1, "NDA/HTTP init response too small");
    }

    for i in 0..10u64 {
        nda_http_client.send_nda_request(METHOD_PING, i, &serde_json::Value::Null);
    }

    println!("Running NDA/HTTP benchmarks... ({} rounds, median kept)", ROUNDS);
    let nda_http_ping = median_round(ROUNDS, || bench_nda_http(&mut nda_http_client, iterations, METHOD_PING));
    let nda_http_tools_list = median_round(ROUNDS, || bench_nda_http(&mut nda_http_client, iterations, METHOD_TOOLS_LIST));
    let nda_http_tools_call = median_round(ROUNDS, || bench_nda_http_call(&mut nda_http_client, iterations, "bench_echo", &serde_json::json!({"size": 64})));
    let nda_http_health = median_round(ROUNDS, || bench_nda_http(&mut nda_http_client, iterations, METHOD_HEALTH_CHECK));

    let nda_http_echo_256 = median_round(ROUNDS, || bench_nda_http_call(&mut nda_http_client, iterations, "bench_echo", &serde_json::json!({"size": 256})));
    let nda_http_echo_1024 = median_round(ROUNDS, || bench_nda_http_call(&mut nda_http_client, iterations, "bench_echo", &serde_json::json!({"size": 1024})));
    let nda_http_echo_4096 = median_round(ROUNDS, || bench_nda_http_call(&mut nda_http_client, iterations, "bench_echo", &serde_json::json!({"size": 4096})));
    let nda_http_echo_16384 = median_round(ROUNDS, || bench_nda_http_call(&mut nda_http_client, iterations, "bench_echo", &serde_json::json!({"size": 16384})));
    let nda_http_echo_65536 = median_round(ROUNDS, || bench_nda_http_call(&mut nda_http_client, iterations, "bench_echo", &serde_json::json!({"size": 65536})));

    drop(nda_http_client);
    let _ = nda_http_child.kill();
    let _ = nda_http_child.wait();

    // ─── Phase 10: Merkle hashing cost isolation ───────────────────────────
    //
    // Measures the SHA-256 Merkle hashing overhead that is baked into every
    // NDA frame. This is the cost of integrity verification, independent of
    // transport.

    let merkle_iters = iterations.max(10000);
    println!();
    println!("Running Merkle hashing cost benchmarks... ({} iterations)", merkle_iters);
    let merkle_hash_results = bench_merkle_hash_cost(merkle_iters);
    let merkle_frame_results = bench_merkle_frame_overhead(merkle_iters);

    // ─── Phase 11: tools/list payload scaling ─────────────────────────────
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
    println!("  Eight transport pipelines compared:");
    println!("    NDA/shmem    = NDA binary TLV over shared memory (zero-poll Win32 events)");
    println!("    JSON/stdio   = JSON-RPC over stdin/stdout pipes (thread + channel + 200ms poll)");
    println!("    JSON/shmem   = JSON-RPC over shared memory (isolates encoding cost)");
    println!("    NDA/stdio    = NDA binary TLV over stdin/stdout pipes (length-prefixed frames)");
    println!("    JSON/HTTP    = JSON-RPC over HTTP/1.1 keep-alive (Axum router + middleware)");
    println!("    Node/stdio   = JSON-RPC over stdin/stdout pipes (Node.js server)");
    println!("    Node/HTTP    = JSON-RPC over HTTP/1.1 keep-alive (Node.js http.createServer)");
    println!("    NDA/HTTP     = NDA binary TLV over HTTP/1.1 keep-alive (Axum, binary endpoint)");
    println!();

    print_comparison("Ping", &nda_ping, &stdio_ping, Some(&js_ping), Some(&ns_ping), Some(&http_ping), Some(&node_stdio_ping), Some(&node_http_ping), Some(&nda_http_ping));
    print_comparison("Tools/List", &nda_tools_list, &stdio_tools_list, Some(&js_tools_list), Some(&ns_tools_list), Some(&http_tools_list), Some(&node_stdio_tools_list), Some(&node_http_tools_list), Some(&nda_http_tools_list));
    print_comparison("Tools/Call (64B)", &nda_tools_call, &stdio_tools_call, Some(&js_tools_call), Some(&ns_tools_call), Some(&http_tools_call), Some(&node_stdio_tools_call), Some(&node_http_tools_call), Some(&nda_http_tools_call));
    print_comparison("Health/Check", &nda_health, &stdio_health, Some(&js_health), Some(&ns_health), Some(&http_health), Some(&node_stdio_health), Some(&node_http_health), Some(&nda_http_health));

    // Payload scaling
    println!("─── Payload Scaling: bench_echo ──────────────────────────────────────────");
    println!();
    println!("  {:>10}  {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}  {:>8} {:>8}", "Payload", "NDA/shmem", "JSON/stdio", "JSON/shmem", "NDA/stdio", "JSON/HTTP", "Node/stdio", "Node/HTTP", "NDA/HTTP", "Avg Δ", "P99 Δ");
    println!("  {}", "─".repeat(143));

    let scaling_data = [
        ("256 B", &nda_echo_256, &stdio_echo_256, &js_echo_256, &ns_echo_256, &http_echo_256, &node_stdio_echo_256, &node_http_echo_256, &nda_http_echo_256),
        ("1 KB", &nda_echo_1024, &stdio_echo_1024, &js_echo_1024, &ns_echo_1024, &http_echo_1024, &node_stdio_echo_1024, &node_http_echo_1024, &nda_http_echo_1024),
        ("4 KB", &nda_echo_4096, &stdio_echo_4096, &js_echo_4096, &ns_echo_4096, &http_echo_4096, &node_stdio_echo_4096, &node_http_echo_4096, &nda_http_echo_4096),
    ];

    for (label, nda_r, stdio_r, js_r, ns_r, ht_r, nstdio_r, nhttp_r, nh_r) in &scaling_data {
        let nda_avg = nda_r.avg_ms();
        let stdio_avg = stdio_r.avg_ms();
        let js_avg = js_r.avg_ms();
        let ns_avg = ns_r.avg_ms();
        let ht_avg = ht_r.avg_ms();
        let nstdio_avg = nstdio_r.avg_ms();
        let nhttp_avg = nhttp_r.avg_ms();
        let nh_avg = nh_r.avg_ms();
        let nda_p99 = nda_r.percentile(99.0);
        let stdio_p99 = stdio_r.percentile(99.0);
        let avg_speedup = stdio_avg / nda_avg;
        let p99_speedup = stdio_p99 / nda_p99;

        println!("  {:>10}  {:>8.3} ms {:>8.3} ms {:>8.3} ms {:>8.3} ms {:>8.3} ms {:>8.3} ms {:>8.3} ms {:>8.3} ms  {:>6.1}x {:>6.1}x",
                 label, nda_avg, stdio_avg, js_avg, ns_avg, ht_avg, nstdio_avg, nhttp_avg, nh_avg, avg_speedup, p99_speedup);
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

    // ─── Merkle Hashing Cost ─────────────────────────────────────────────

    println!("─── Merkle Hashing (SHA-256) Cost ─────────────────────────────────────");
    println!();
    println!("  Every NDA frame includes a SHA-256 Merkle root over the payload.");
    println!("  This measures the hashing overhead in isolation:");
    println!();
    println!("  {:>10}  {:>12}  {:>12}", "Payload", "SHA-256 (ns)", "Per frame (µs)");
    println!("  {}", "─".repeat(42));
    for (label, _size, ns) in &merkle_hash_results {
        println!("  {:>10}  {:>10.1} ns  {:>10.3} µs", label, ns, ns / 1000.0);
    }

    println!();
    println!("  Frame assembly: with Merkle vs without (client-side only):");
    println!();
    println!("  {:>14}  {:>12}  {:>12}  {:>12}  {:>8}", "Payload", "With hash", "Hash cost", "No hash", "Hash %");
    println!("  {}", "─".repeat(66));
    for (label, with_total, hash_cost, without_hash) in &merkle_frame_results {
        let pct = if *with_total > 0.0 { (hash_cost / with_total) * 100.0 } else { 0.0 };
        println!("  {:>14}  {:>9.1} ns  {:>9.1} ns  {:>9.1} ns  {:>6.1}%",
                 label, with_total, hash_cost, without_hash, pct);
    }

    println!();
    println!("  Merkle cost as fraction of total NDA pipeline round-trip:");
    println!();

    // Compare Merkle hash cost against total NDA pipeline latencies
    // across all payload sizes and all NDA-capable transports.
    let merkle_ns = |size: usize| -> f64 {
        merkle_hash_results.iter().find(|(_, s, _)| *s == size).map(|(_, _, ns)| *ns).unwrap_or(0.0)
    };
    // Each NDA round-trip hashes twice: once for request, once for response validation
    let merkle_rt = |size: usize| -> f64 { merkle_ns(size) * 2.0 };

    let pct = |total: f64, merkle: f64| -> f64 {
        if total > 0.0 { (merkle / total) * 100.0 } else { 0.0 }
    };

    println!("  (Each round-trip hashes request + validates response = 2x SHA-256)");
    println!();

    let print_pipeline = |name: &str, results: &[(&str, &BenchResult, usize)]| {
        println!("  {}:", name);
        println!("  {:>10}  {:>10}  {:>10}  {:>8}", "Payload", "Total RT", "Merkle", "Merkle %");
        println!("  {}", "─".repeat(46));
        for &(label, result, size) in results {
            let total_us = result.avg_ms() * 1000.0;
            let m_us = merkle_rt(size) / 1000.0;
            println!("  {:>10}  {:>7.3} µs  {:>7.3} µs  {:>6.1}%",
                     label, total_us, m_us, pct(total_us, m_us));
        }
        println!();
    };

    print_pipeline("NDA/shmem", &[
        ("ping (null)", &nda_ping, 0),
        ("64 B", &nda_tools_call, 64),
        ("256 B", &nda_echo_256, 256),
        ("1 KB", &nda_echo_1024, 1024),
        ("4 KB", &nda_echo_4096, 4096),
        ("16 KB", &nda_echo_16384, 16384),
    ]);
    print_pipeline("NDA/stdio", &[
        ("ping (null)", &ns_ping, 0),
        ("64 B", &ns_tools_call, 64),
        ("256 B", &ns_echo_256, 256),
        ("1 KB", &ns_echo_1024, 1024),
        ("4 KB", &ns_echo_4096, 4096),
        ("16 KB", &ns_echo_16384, 16384),
        ("64 KB", &ns_echo_65536, 65536),
    ]);
    print_pipeline("NDA/HTTP", &[
        ("ping (null)", &nda_http_ping, 0),
        ("64 B", &nda_http_tools_call, 64),
        ("256 B", &nda_http_echo_256, 256),
        ("1 KB", &nda_http_echo_1024, 1024),
        ("4 KB", &nda_http_echo_4096, 4096),
        ("16 KB", &nda_http_echo_16384, 16384),
        ("64 KB", &nda_http_echo_65536, 65536),
    ]);
    println!("  (NDA/shmem 64 KB omitted: exceeds 60 KB shmem output buffer)");

    // Overall summary
    let avg_first = |results: &[&BenchResult]| -> f64 {
        let vals: Vec<f64> = results.iter().filter_map(|r| r.first_call_ms()).collect();
        if vals.is_empty() { 0.0 } else { vals.iter().sum::<f64>() / vals.len() as f64 }
    };

    let nda_1st_all = avg_first(&[&nda_ping, &nda_tools_call, &nda_health]);
    let stdio_1st_all = avg_first(&[&stdio_ping, &stdio_tools_call, &stdio_health]);
    let js_1st_all = avg_first(&[&js_ping, &js_tools_call, &js_health]);
    let ns_1st_all = avg_first(&[&ns_ping, &ns_tools_call, &ns_health]);
    let http_1st_all = avg_first(&[&http_ping, &http_tools_call, &http_health]);
    let node_stdio_1st_all = avg_first(&[&node_stdio_ping, &node_stdio_tools_call, &node_stdio_health]);
    let node_http_1st_all = avg_first(&[&node_http_ping, &node_http_tools_call, &node_http_health]);
    let nda_http_1st_all = avg_first(&[&nda_http_ping, &nda_http_tools_call, &nda_http_health]);

    let nda_avg_all = (nda_ping.avg_ms() + nda_tools_call.avg_ms() + nda_health.avg_ms()) / 3.0;
    let stdio_avg_all = (stdio_ping.avg_ms() + stdio_tools_call.avg_ms() + stdio_health.avg_ms()) / 3.0;
    let js_avg_all = (js_ping.avg_ms() + js_tools_call.avg_ms() + js_health.avg_ms()) / 3.0;
    let ns_avg_all = (ns_ping.avg_ms() + ns_tools_call.avg_ms() + ns_health.avg_ms()) / 3.0;
    let http_avg_all = (http_ping.avg_ms() + http_tools_call.avg_ms() + http_health.avg_ms()) / 3.0;
    let node_stdio_avg_all = (node_stdio_ping.avg_ms() + node_stdio_tools_call.avg_ms() + node_stdio_health.avg_ms()) / 3.0;
    let node_http_avg_all = (node_http_ping.avg_ms() + node_http_tools_call.avg_ms() + node_http_health.avg_ms()) / 3.0;
    let nda_http_avg_all = (nda_http_ping.avg_ms() + nda_http_tools_call.avg_ms() + nda_http_health.avg_ms()) / 3.0;
    let nda_p99_all = (nda_ping.percentile(99.0) + nda_tools_call.percentile(99.0) + nda_health.percentile(99.0)) / 3.0;
    let stdio_p99_all = (stdio_ping.percentile(99.0) + stdio_tools_call.percentile(99.0) + stdio_health.percentile(99.0)) / 3.0;
    let ns_p99_all = (ns_ping.percentile(99.0) + ns_tools_call.percentile(99.0) + ns_health.percentile(99.0)) / 3.0;
    let js_p99_all = (js_ping.percentile(99.0) + js_tools_call.percentile(99.0) + js_health.percentile(99.0)) / 3.0;
    let http_p99_all = (http_ping.percentile(99.0) + http_tools_call.percentile(99.0) + http_health.percentile(99.0)) / 3.0;
    let node_stdio_p99_all = (node_stdio_ping.percentile(99.0) + node_stdio_tools_call.percentile(99.0) + node_stdio_health.percentile(99.0)) / 3.0;
    let node_http_p99_all = (node_http_ping.percentile(99.0) + node_http_tools_call.percentile(99.0) + node_http_health.percentile(99.0)) / 3.0;
    let nda_http_p99_all = (nda_http_ping.percentile(99.0) + nda_http_tools_call.percentile(99.0) + nda_http_health.percentile(99.0)) / 3.0;

    println!("========================================================================");
    println!("  SUMMARY");
    println!("========================================================================");
    println!();
    println!("  Pipeline             1st call     Warm avg     Warm P99     vs NDA/shmem");
    println!("  ─────────────────────────────────────────────────────────────────────────────────");
    println!("  NDA/shmem      {:>10.3} ms {:>10.3} ms {:>10.3} ms   (baseline)", nda_1st_all, nda_avg_all, nda_p99_all);
    println!("  NDA/stdio      {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg, {:.1}x p99", ns_1st_all, ns_avg_all, ns_p99_all, ns_avg_all / nda_avg_all, ns_p99_all / nda_p99_all);
    println!("  JSON/shmem     {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg (encoding cost only)", js_1st_all, js_avg_all, js_p99_all, js_avg_all / nda_avg_all);
    println!("  JSON/stdio     {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg, {:.1}x p99", stdio_1st_all, stdio_avg_all, stdio_p99_all, stdio_avg_all / nda_avg_all, stdio_p99_all / nda_p99_all);
    println!("  JSON/HTTP      {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg, {:.1}x p99", http_1st_all, http_avg_all, http_p99_all, http_avg_all / nda_avg_all, http_p99_all / nda_p99_all);
    println!("  Node/stdio     {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg, {:.1}x p99", node_stdio_1st_all, node_stdio_avg_all, node_stdio_p99_all, node_stdio_avg_all / nda_avg_all, node_stdio_p99_all / nda_p99_all);
    println!("  Node/HTTP      {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg, {:.1}x p99", node_http_1st_all, node_http_avg_all, node_http_p99_all, node_http_avg_all / nda_avg_all, node_http_p99_all / nda_p99_all);
    println!("  NDA/HTTP       {:>10.3} ms {:>10.3} ms {:>10.3} ms   {:.1}x avg, {:.1}x p99", nda_http_1st_all, nda_http_avg_all, nda_http_p99_all, nda_http_avg_all / nda_avg_all, nda_http_p99_all / nda_p99_all);
    println!();
    println!("  What's being measured:");
    println!("    NDA/shmem:  TLV encode → SHA-256 → mmap write → SetEvent → server processes");
    println!("                → mmap write → SetEvent → client reads. No JSON parsing anywhere.");
    println!("    NDA/stdio:  TLV encode → SHA-256 → pipe write → pipe read → server processes");
    println!("                → pipe write → pipe read → TLV decode. Same binary encoding, pipes for transport.");
    println!("    JSON/shmem: serde_json stringify → mmap write → SetEvent → server parses");
    println!("                → mmap write → SetEvent → client JSON parses. Same shmem, JSON encoding cost.");
    println!("    JSON/stdio: serde_json stringify → pipe write → thread recv → channel send →");
    println!("                serde_json parse → handle → serde_json stringify → pipe write →");
    println!("                pipe read → JSON parse. Two JSON serialize+parse cycles per request.");
    println!("    JSON/HTTP:  serde_json stringify → HTTP POST → TCP write → Axum router →");
    println!("                middleware stack → serde_json parse → handle → serde_json");
    println!("                stringify → HTTP response → TCP read → JSON parse.");
    println!("    Node/stdio: JSON.stringify → stdin write → Node.js readline → JSON.parse →");
    println!("                handleRequest → JSON.stringify → stdout write → readline parse.");
    println!("    Node/HTTP:  JSON.stringify → HTTP POST → TCP write → Node.js http server →");
    println!("                JSON.parse → handleRequest → JSON.stringify → HTTP response →");
    println!("                TCP read → JSON parse.");
    println!("    NDA/HTTP:   TLV encode → SHA-256 → HTTP POST (octet-stream) → TCP write →");
    println!("                Axum binary endpoint → TLV decode → process → TLV encode →");
    println!("                SHA-256 → HTTP response → TCP read → TLV decode.");
    println!();
    println!("  Shmem buffer limits: input ≤ {} bytes, output ≤ {} bytes",
             OUTPUT_BUFFER_OFFSET - INPUT_BUFFER_OFFSET,
             TOTAL_BUFFER_SIZE - OUTPUT_BUFFER_OFFSET);
    println!("  (4KB input / 61KB output — sufficient for typical MCP tool calls)");

    // Restore Windows timer resolution
    #[cfg(target_os = "windows")]
    unsafe { timeEndPeriod(1); }
}
