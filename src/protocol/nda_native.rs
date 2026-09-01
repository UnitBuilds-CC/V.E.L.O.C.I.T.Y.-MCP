//! NDA-native protocol handler for shared memory transport.
//!
//! Replaces JSON-in-shmem with NDA binary frames as the native transport.
//! The wire format is:
//!
//! ```text
//! [4 bytes: magic "NMCP"]
//! [32 bytes: merkle root (SHA-256 of payload)]
//! [payload: TLV-encoded message]
//! ```
//!
//! Payload layout for requests:
//! ```text
//! [1 byte:  method type]
//! [TLV:     request id]
//! [TLV:     method-specific data]
//! ```
//!
//! Payload layout for responses:
//! ```text
//! [1 byte:  status (0=ok, 1=error)]
//! [TLV:     request id (echoed)]
//! [TLV:     result data]
//! ```
//!
//! Method types:
//! - 0x01: initialize
//! - 0x02: tools/list
//! - 0x03: tools/call
//! - 0x04: ping
//! - 0x05: logging/setLevel
//! - 0x06: health/check
//! - 0x10: notifications/initialized
//! - 0x11: notifications/cancelled

use serde_json::{json, Value};
use sha2::{Sha256, Digest};
use std::error::Error;
use std::sync::{Mutex, OnceLock};

pub const METHOD_INITIALIZE: u8 = 0x01;
pub const METHOD_TOOLS_LIST: u8 = 0x02;
pub const METHOD_TOOLS_CALL: u8 = 0x03;
pub const METHOD_PING: u8 = 0x04;
pub const METHOD_LOGGING_SET_LEVEL: u8 = 0x05;
pub const METHOD_HEALTH_CHECK: u8 = 0x06;
pub const METHOD_RESOURCES_LIST: u8 = 0x07;
pub const METHOD_RESOURCES_READ: u8 = 0x08;
pub const METHOD_RESOURCE_TEMPLATES_LIST: u8 = 0x09;
pub const METHOD_PROMPTS_LIST: u8 = 0x0A;
pub const METHOD_PROMPTS_GET: u8 = 0x0B;
pub const METHOD_SAMPLING_CREATE: u8 = 0x0C;
pub const NOTIF_INITIALIZED: u8 = 0x10;
pub const NOTIF_CANCELLED: u8 = 0x11;
pub const NOTIF_PROGRESS: u8 = 0x12;

pub const STATUS_OK: u8 = 0;
pub const STATUS_ERROR: u8 = 1;

pub const NDA_MAGIC: &[u8; 4] = b"NMCP";
pub const FRAME_HEADER_SIZE: usize = 36;

pub fn is_nda_frame(data: &[u8]) -> bool {
    data.len() >= 4 && &data[0..4] == NDA_MAGIC
}

pub fn encode_json_value(value: &Value, buf: &mut Vec<u8>) {
    match value {
        Value::String(s) => {
            buf.push(0x01);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
            buf.extend_from_slice(bytes);
        }
        Value::Number(n) => {
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
        Value::Bool(b) => {
            buf.push(0x03);
            buf.push(if *b { 1 } else { 0 });
        }
        Value::Null => {
            buf.push(0x04);
        }
        Value::Array(arr) => {
            buf.push(0x05);
            buf.extend_from_slice(&(arr.len() as u32).to_be_bytes());
            for item in arr {
                encode_json_value(item, buf);
            }
        }
        Value::Object(obj) => {
            buf.push(0x06);
            buf.extend_from_slice(&(obj.len() as u32).to_be_bytes());
            for (key, val) in obj {
                let key_bytes = key.as_bytes();
                buf.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(key_bytes);
                encode_json_value(val, buf);
            }
        }
    }
}

const TLV_MAX_DEPTH: u32 = 32;
const TLV_MAX_STRING_LEN: usize = 10_000_000;
const TLV_MAX_ELEMENTS: usize = 100_000;

pub fn decode_json_value(buf: &[u8]) -> Result<(Value, usize), Box<dyn Error>> {
    decode_json_value_inner(buf, 0)
}

fn decode_json_value_inner(buf: &[u8], depth: u32) -> Result<(Value, usize), Box<dyn Error>> {
    if depth > TLV_MAX_DEPTH {
        return Err(format!("TLV nesting depth exceeds maximum {}", TLV_MAX_DEPTH).into());
    }
    if buf.is_empty() {
        return Err("Unexpected end of TLV buffer".into());
    }
    match buf[0] {
        0x01 => {
            if buf.len() < 5 { return Err("TLV string: missing length".into()); }
            let len = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if len > TLV_MAX_STRING_LEN {
                return Err(format!("TLV string length {} exceeds maximum {}", len, TLV_MAX_STRING_LEN).into());
            }
            if buf.len() < 5 + len { return Err("TLV string: truncated data".into()); }
            let s = std::str::from_utf8(&buf[5..5 + len])?.to_string();
            Ok((Value::String(s), 5 + len))
        }
        0x02 => {
            if buf.len() < 9 { return Err("TLV integer: missing data".into()); }
            let i = i64::from_be_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]);
            Ok((json!(i), 9))
        }
        0x07 => {
            if buf.len() < 9 { return Err("TLV float: missing data".into()); }
            let f = f64::from_be_bytes([buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8]]);
            Ok((json!(f), 9))
        }
        0x03 => {
            if buf.len() < 2 { return Err("TLV bool: missing data".into()); }
            Ok((Value::Bool(buf[1] != 0), 2))
        }
        0x04 => Ok((Value::Null, 1)),
        0x05 => {
            if buf.len() < 5 { return Err("TLV array: missing count".into()); }
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if count > TLV_MAX_ELEMENTS {
                return Err(format!("TLV array count {} exceeds maximum {}", count, TLV_MAX_ELEMENTS).into());
            }
            let mut offset = 5;
            let mut items = Vec::with_capacity(count.min(1024));
            for _ in 0..count {
                let (val, consumed) = decode_json_value_inner(&buf[offset..], depth + 1)?;
                items.push(val);
                offset += consumed;
            }
            Ok((Value::Array(items), offset))
        }
        0x06 => {
            if buf.len() < 5 { return Err("TLV object: missing count".into()); }
            let count = u32::from_be_bytes([buf[1], buf[2], buf[3], buf[4]]) as usize;
            if count > TLV_MAX_ELEMENTS {
                return Err(format!("TLV object count {} exceeds maximum {}", count, TLV_MAX_ELEMENTS).into());
            }
            let mut offset = 5;
            let mut map = serde_json::Map::with_capacity(count.min(1024));
            for _ in 0..count {
                if offset + 2 > buf.len() { return Err("TLV object: missing key length".into()); }
                let key_len = u16::from_be_bytes([buf[offset], buf[offset + 1]]) as usize;
                offset += 2;
                if offset + key_len > buf.len() { return Err("TLV object: truncated key".into()); }
                let key = std::str::from_utf8(&buf[offset..offset + key_len])?.to_string();
                offset += key_len;
                let (val, consumed) = decode_json_value_inner(&buf[offset..], depth + 1)?;
                map.insert(key, val);
                offset += consumed;
            }
            Ok((Value::Object(map), offset))
        }
        tag => Err(format!("Unknown TLV type tag: 0x{:02x}", tag).into()),
    }
}

pub struct NdaRequest {
    pub method: u8,
    pub request_id: Value,
    pub data: Value,
}

pub fn parse_nda_request(frame: &[u8]) -> Result<NdaRequest, Box<dyn Error>> {
    if frame.len() < FRAME_HEADER_SIZE {
        return Err("NDA frame too small for header".into());
    }
    if &frame[0..4] != NDA_MAGIC {
        return Err("Invalid NDA magic".into());
    }

    let stored_merkle = &frame[4..36];
    let payload = &frame[FRAME_HEADER_SIZE..];

    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize();
    if stored_merkle != computed.as_slice() {
        return Err("NDA frame Merkle root mismatch".into());
    }

    if payload.is_empty() {
        return Err("NDA payload is empty".into());
    }

    let method = payload[0];
    let mut offset = 1;

    let (request_id, consumed) = decode_json_value(&payload[offset..])?;
    offset += consumed;

    let data = if offset < payload.len() {
        let (d, _) = decode_json_value(&payload[offset..])?;
        d
    } else {
        Value::Null
    };

    Ok(NdaRequest { method, request_id, data })
}

// ─── Zero-alloc in-place parsing ────────────────────────────────────────────
//
// parse_nda_request decodes the whole payload into a serde_json::Value tree
// (heap allocations on every request). The hot methods (ping, tools/list,
// tools/call, health) do not need a Value tree: they can work directly on
// borrowed slices of the frame. These helpers walk the TLV without decoding.

/// Total byte length of one TLV value, walking nested containers without
/// allocating or building a Value. Mirrors decode_json_value's consumption.
pub fn skip_tlv_value(bytes: &[u8]) -> Result<usize, Box<dyn Error>> {
    if bytes.is_empty() {
        return Err("TLV skip: empty value".into());
    }
    match bytes[0] {
        0x01 => {
            if bytes.len() < 5 { return Err("TLV skip: truncated string length".into()); }
            let len = u32::from_be_bytes(bytes[1..5].try_into()?) as usize;
            if bytes.len() < 5 + len { return Err("TLV skip: truncated string body".into()); }
            Ok(5 + len)
        }
        0x02 | 0x07 => {
            if bytes.len() < 9 { return Err("TLV skip: truncated number".into()); }
            Ok(9)
        }
        0x03 => {
            if bytes.len() < 2 { return Err("TLV skip: truncated bool".into()); }
            Ok(2)
        }
        0x04 => Ok(1),
        0x05 => {
            if bytes.len() < 5 { return Err("TLV skip: truncated array count".into()); }
            let count = u32::from_be_bytes(bytes[1..5].try_into()?) as usize;
            let mut off = 5usize;
            for _ in 0..count {
                off += skip_tlv_value(&bytes[off..])?;
            }
            Ok(off)
        }
        0x06 => {
            if bytes.len() < 5 { return Err("TLV skip: truncated object count".into()); }
            let count = u32::from_be_bytes(bytes[1..5].try_into()?) as usize;
            let mut off = 5usize;
            for _ in 0..count {
                if bytes.len() < off + 2 { return Err("TLV skip: truncated key length".into()); }
                let klen = u16::from_be_bytes(bytes[off..off + 2].try_into()?) as usize;
                off += 2 + klen;
                if bytes.len() < off { return Err("TLV skip: truncated key".into()); }
                off += skip_tlv_value(&bytes[off..])?;
            }
            Ok(off)
        }
        other => Err(format!("TLV skip: unknown tag 0x{:02x}", other).into()),
    }
}

/// Borrowed view of a parsed NDA request — no Value trees, no allocations
/// beyond the frame hash. `id_tlv` is the request id TLV including its tag
/// byte, so it can be echoed verbatim into the response.
pub struct NdaRequestRef<'a> {
    pub method: u8,
    pub id_tlv: &'a [u8],
    pub data: &'a [u8],
}

/// Zero-alloc counterpart of parse_nda_request. Validates magic + Merkle +
/// a complete request-id TLV; the data subtree is validated lazily by the
/// methods that actually consume it.
pub fn parse_nda_request_inplace(frame: &[u8]) -> Result<NdaRequestRef<'_>, Box<dyn Error>> {
    if frame.len() < FRAME_HEADER_SIZE {
        return Err("NDA frame too small for header".into());
    }
    if &frame[0..4] != NDA_MAGIC {
        return Err("Invalid NDA magic".into());
    }

    let stored_merkle = &frame[4..36];
    let payload = &frame[FRAME_HEADER_SIZE..];

    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize();
    if stored_merkle != computed.as_slice() {
        return Err("NDA frame Merkle root mismatch".into());
    }

    if payload.len() < 2 {
        return Err("NDA payload too small for method + id".into());
    }

    let method = payload[0];
    let id_len = skip_tlv_value(&payload[1..])?;

    Ok(NdaRequestRef {
        method,
        id_tlv: &payload[1..1 + id_len],
        data: &payload[1 + id_len..],
    })
}

/// Response builder that takes the request id and result as pre-encoded TLV
/// slices — no Value round-trip on the hot path.
pub fn build_nda_response_raw(status: u8, id_tlv: &[u8], result_tlv: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(1 + id_tlv.len() + result_tlv.len());
    payload.push(status);
    payload.extend_from_slice(id_tlv);
    payload.extend_from_slice(result_tlv);
    build_nda_frame(&payload)
}

pub fn build_nda_error_raw(id_tlv: &[u8], error_msg: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(5 + error_msg.len());
    result.push(0x01);
    result.extend_from_slice(&(error_msg.len() as u32).to_be_bytes());
    result.extend_from_slice(error_msg.as_bytes());
    build_nda_response_raw(STATUS_ERROR, id_tlv, &result)
}

/// Walk a tools/call data object (`{"name": ..., "arguments": ...}`) in
/// place, in any key order, returning borrowed slices for the two fields.
/// Either field may be absent (None), matching serde_json indexing semantics.
pub fn extract_tools_call_fields(data: &[u8]) -> Result<(Option<&str>, Option<&[u8]>), Box<dyn Error>> {
    if data.is_empty() || data[0] != 0x06 {
        return Ok((None, None));
    }
    if data.len() < 5 { return Err("tools/call data: truncated object count".into()); }
    let count = u32::from_be_bytes(data[1..5].try_into()?) as usize;
    let mut off = 5usize;
    let mut name = None;
    let mut arguments = None;
    for _ in 0..count {
        if data.len() < off + 2 { return Err("tools/call data: truncated key length".into()); }
        let klen = u16::from_be_bytes(data[off..off + 2].try_into()?) as usize;
        off += 2;
        if data.len() < off + klen { return Err("tools/call data: truncated key".into()); }
        let key = &data[off..off + klen];
        off += klen;
        let vlen = skip_tlv_value(&data[off..])?;
        let value = &data[off..off + vlen];
        off += vlen;
        match key {
            b"name" => {
                if value.len() >= 5 && value[0] == 0x01 {
                    name = std::str::from_utf8(&value[5..]).ok();
                }
            }
            b"arguments" => arguments = Some(value),
            _ => {}
        }
    }
    Ok((name, arguments))
}

/// The TLV encoding of an empty JSON object `{}` — the result for ping,
/// notifications, and other ack-only methods.
pub const EMPTY_OBJECT_TLV: &[u8] = &[0x06, 0, 0, 0, 0];

static HEALTH_RESULT_TLV: OnceLock<Vec<u8>> = OnceLock::new();

/// Pre-encoded health/check result (`{"status":"healthy","mode":"shmem-nda",
/// "version":...}`), built once. VERSION is a compile-time constant.
pub fn health_result_tlv() -> &'static [u8] {
    HEALTH_RESULT_TLV.get_or_init(|| {
        let mut buf = Vec::new();
        encode_json_value(&json!({
            "status": "healthy",
            "mode": "shmem-nda",
            "version": crate::VERSION
        }), &mut buf);
        buf
    })
}

pub fn build_nda_frame(payload: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let merkle = hasher.finalize();

    let mut frame = Vec::with_capacity(FRAME_HEADER_SIZE + payload.len());
    frame.extend_from_slice(NDA_MAGIC);
    frame.extend_from_slice(&merkle);
    frame.extend_from_slice(payload);
    frame
}

pub fn build_nda_response(status: u8, request_id: &Value, result: &Value) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(status);
    encode_json_value(request_id, &mut payload);
    encode_json_value(result, &mut payload);
    build_nda_frame(&payload)
}

pub fn build_nda_error(request_id: &Value, error_msg: &str) -> Vec<u8> {
    build_nda_response(STATUS_ERROR, request_id, &json!(error_msg))
}

/// Encode a tools/list result (`{"tools": [...]}`) directly from registry
/// structs, skipping the intermediate serde_json::Value tree. Emits keys in
/// sorted order (description, inputSchema, name) to match serde_json's
/// BTreeMap iteration, so output is byte-identical to encode_json_value.
pub fn encode_tools_list_result(buf: &mut Vec<u8>, tools: &[crate::registry::Tool]) {
    buf.push(0x06); // outer object
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(&5u16.to_be_bytes());
    buf.extend_from_slice(b"tools");
    buf.push(0x05); // array
    buf.extend_from_slice(&(tools.len() as u32).to_be_bytes());
    for t in tools {
        buf.push(0x06); // tool object, 3 keys sorted: description, inputSchema, name
        buf.extend_from_slice(&3u32.to_be_bytes());

        buf.extend_from_slice(&11u16.to_be_bytes());
        buf.extend_from_slice(b"description");
        buf.push(0x01);
        let d = t.description.as_bytes();
        buf.extend_from_slice(&(d.len() as u32).to_be_bytes());
        buf.extend_from_slice(d);

        buf.extend_from_slice(&11u16.to_be_bytes());
        buf.extend_from_slice(b"inputSchema");
        encode_json_value(&t.input_schema, buf);

        buf.extend_from_slice(&4u16.to_be_bytes());
        buf.extend_from_slice(b"name");
        buf.push(0x01);
        let n = t.name.as_bytes();
        buf.extend_from_slice(&(n.len() as u32).to_be_bytes());
        buf.extend_from_slice(n);
    }
}

static TOOLS_LIST_CACHE: OnceLock<Mutex<Option<(u64, Vec<u8>)>>> = OnceLock::new();

/// TLV-encoded tools/list result (`{"tools": [...]}`), cached keyed by the
/// registry generation. The hot path is a lock + generation compare + clone
/// of ~8KB; re-encoding only happens when the tool set actually changes.
pub fn encoded_tools_list_result() -> Vec<u8> {
    let cell = TOOLS_LIST_CACHE.get_or_init(|| Mutex::new(None));
    let gen = crate::registry::registry_generation();
    {
        let cache = cell.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((g, bytes)) = &*cache {
            if *g == gen {
                return bytes.clone();
            }
        }
    }
    let tools = crate::registry::get_tools();
    let mut bytes = Vec::with_capacity(8 * 1024);
    encode_tools_list_result(&mut bytes, &tools);
    // Only store if the generation was stable across the build: otherwise a
    // concurrent registration would be masked by pre-mutation bytes cached
    // under the post-mutation generation.
    let gen_after = crate::registry::registry_generation();
    if gen_after == gen {
        let mut cache = cell.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Some((gen_after, bytes.clone()));
    }
    bytes
}

pub fn method_name(method: u8) -> &'static str {
    match method {
        METHOD_INITIALIZE => "initialize",
        METHOD_TOOLS_LIST => "tools/list",
        METHOD_TOOLS_CALL => "tools/call",
        METHOD_PING => "ping",
        METHOD_LOGGING_SET_LEVEL => "logging/setLevel",
        METHOD_HEALTH_CHECK => "health/check",
        NOTIF_INITIALIZED => "notifications/initialized",
        NOTIF_CANCELLED => "notifications/cancelled",
        _ => "unknown",
    }
}

pub fn method_from_str(name: &str) -> Option<u8> {
    match name {
        "initialize" => Some(METHOD_INITIALIZE),
        "tools/list" => Some(METHOD_TOOLS_LIST),
        "tools/call" => Some(METHOD_TOOLS_CALL),
        "ping" => Some(METHOD_PING),
        "logging/setLevel" => Some(METHOD_LOGGING_SET_LEVEL),
        "health/check" => Some(METHOD_HEALTH_CHECK),
        "notifications/initialized" => Some(NOTIF_INITIALIZED),
        "notifications/cancelled" => Some(NOTIF_CANCELLED),
        _ => None,
    }
}

pub fn build_nda_request(method: u8, request_id: &Value, data: &Value) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.push(method);
    encode_json_value(request_id, &mut payload);
    if !data.is_null() {
        encode_json_value(data, &mut payload);
    }
    build_nda_frame(&payload)
}

// ─── Deterministic Flat Binary Format ────────────────────────────────────────
//
// No TLV wrappers, no key names. Fields encoded in order with type tags + raw values.
//
// Flat request payload:
//   [1 byte:  method type]
//   [8 bytes: request id (u64 LE)]
//   [2 bytes: tool name length]
//   [N bytes: tool name]
//   [flat fields: type tag + value, in order]
//
// Flat response payload:
//   [1 byte:  status (0=ok, 1=error)]
//   [8 bytes: request id (u64 LE)]
//   [flat fields: type tag + value, in order]
//
// Field encoding:
//   0x01 String:  [4 bytes: len LE][N bytes: UTF-8]
//   0x02 Integer: [8 bytes: i64 LE]
//   0x03 Bool:    [1 byte: 0 or 1]
//   0x04 Null:    (no data)
//   0x05 Float:   [8 bytes: f64 LE]

pub fn encode_flat_value(value: &Value, buf: &mut Vec<u8>) {
    match value {
        Value::String(s) => {
            buf.push(0x01);
            let bytes = s.as_bytes();
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                buf.push(0x02);
                buf.extend_from_slice(&i.to_le_bytes());
            } else if let Some(f) = n.as_f64() {
                buf.push(0x05);
                buf.extend_from_slice(&f.to_le_bytes());
            } else {
                buf.push(0x02);
                buf.extend_from_slice(&0i64.to_le_bytes());
            }
        }
        Value::Bool(b) => {
            buf.push(0x03);
            buf.push(if *b { 1 } else { 0 });
        }
        Value::Null => {
            buf.push(0x04);
        }
        Value::Array(arr) => {
            for item in arr {
                encode_flat_value(item, buf);
            }
        }
        Value::Object(obj) => {
            for (_, val) in obj {
                encode_flat_value(val, buf);
            }
        }
    }
}

pub fn decode_flat_value(buf: &[u8], offset: &mut usize) -> Result<Value, Box<dyn Error>> {
    if *offset >= buf.len() {
        return Err("Flat decode: unexpected end of buffer".into());
    }
    let tag = buf[*offset];
    *offset += 1;
    match tag {
        0x01 => {
            if *offset + 4 > buf.len() { return Err("Flat string: missing length".into()); }
            let len = u32::from_le_bytes([buf[*offset], buf[*offset+1], buf[*offset+2], buf[*offset+3]]) as usize;
            *offset += 4;
            if *offset + len > buf.len() { return Err("Flat string: truncated".into()); }
            let s = std::str::from_utf8(&buf[*offset..*offset+len])?.to_string();
            *offset += len;
            Ok(Value::String(s))
        }
        0x02 => {
            if *offset + 8 > buf.len() { return Err("Flat integer: missing data".into()); }
            let i = i64::from_le_bytes([buf[*offset], buf[*offset+1], buf[*offset+2], buf[*offset+3],
                                        buf[*offset+4], buf[*offset+5], buf[*offset+6], buf[*offset+7]]);
            *offset += 8;
            Ok(json!(i))
        }
        0x05 => {
            if *offset + 8 > buf.len() { return Err("Flat float: missing data".into()); }
            let f = f64::from_le_bytes([buf[*offset], buf[*offset+1], buf[*offset+2], buf[*offset+3],
                                        buf[*offset+4], buf[*offset+5], buf[*offset+6], buf[*offset+7]]);
            *offset += 8;
            Ok(json!(f))
        }
        0x03 => {
            if *offset + 1 > buf.len() { return Err("Flat bool: missing data".into()); }
            let b = buf[*offset] != 0;
            *offset += 1;
            Ok(Value::Bool(b))
        }
        0x04 => Ok(Value::Null),
        t => Err(format!("Flat decode: unknown tag 0x{:02x}", t).into()),
    }
}

fn value_to_u64(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n.as_u64().unwrap_or(0),
        Value::String(s) => s.parse::<u64>().unwrap_or(0),
        _ => 0,
    }
}

pub fn build_flat_request(method: u8, request_id: &Value, tool_name: &str, arguments: &Value) -> Vec<u8> {
    let mut payload = Vec::with_capacity(64);
    payload.push(method);
    payload.extend_from_slice(&value_to_u64(request_id).to_le_bytes());
    payload.extend_from_slice(&(tool_name.len() as u16).to_le_bytes());
    payload.extend_from_slice(tool_name.as_bytes());
    encode_flat_value(arguments, &mut payload);
    build_nda_frame(&payload)
}

pub struct FlatRequest {
    pub method: u8,
    pub request_id: u64,
    pub tool_name: String,
    pub fields: Vec<Value>,
}

pub fn parse_flat_request(frame: &[u8]) -> Result<FlatRequest, Box<dyn Error>> {
    if frame.len() < FRAME_HEADER_SIZE {
        return Err("Flat frame too small".into());
    }
    if &frame[0..4] != NDA_MAGIC {
        return Err("Invalid NDA magic".into());
    }

    let stored_merkle = &frame[4..36];
    let payload = &frame[FRAME_HEADER_SIZE..];
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize();
    if stored_merkle != computed.as_slice() {
        return Err("Flat frame Merkle root mismatch".into());
    }

    if payload.is_empty() {
        return Err("Flat payload is empty".into());
    }

    let method = payload[0];
    let mut offset = 1;

    if offset + 8 > payload.len() { return Err("Flat: missing request id".into()); }
    let request_id = u64::from_le_bytes([payload[offset], payload[offset+1], payload[offset+2], payload[offset+3],
                                          payload[offset+4], payload[offset+5], payload[offset+6], payload[offset+7]]);
    offset += 8;

    if offset + 2 > payload.len() { return Err("Flat: missing tool name length".into()); }
    let name_len = u16::from_le_bytes([payload[offset], payload[offset+1]]) as usize;
    offset += 2;

    if offset + name_len > payload.len() { return Err("Flat: truncated tool name".into()); }
    let tool_name = std::str::from_utf8(&payload[offset..offset+name_len])?.to_string();
    offset += name_len;

    let mut fields = Vec::new();
    while offset < payload.len() {
        fields.push(decode_flat_value(payload, &mut offset)?);
    }

    Ok(FlatRequest { method, request_id, tool_name, fields })
}

pub fn build_flat_response(status: u8, request_id: u64, result: &Value) -> Vec<u8> {
    let mut payload = Vec::with_capacity(64);
    payload.push(status);
    payload.extend_from_slice(&request_id.to_le_bytes());
    encode_flat_value(result, &mut payload);
    build_nda_frame(&payload)
}

pub struct FlatResponse {
    pub status: u8,
    pub request_id: u64,
    pub fields: Vec<Value>,
}

pub fn parse_flat_response(frame: &[u8]) -> Result<FlatResponse, Box<dyn Error>> {
    if frame.len() < FRAME_HEADER_SIZE {
        return Err("Flat response frame too small".into());
    }
    if &frame[0..4] != NDA_MAGIC {
        return Err("Invalid NDA magic".into());
    }

    let stored_merkle = &frame[4..36];
    let payload = &frame[FRAME_HEADER_SIZE..];
    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize();
    if stored_merkle != computed.as_slice() {
        return Err("Flat response Merkle root mismatch".into());
    }

    if payload.is_empty() { return Err("Flat response payload empty".into()); }

    let status = payload[0];
    let mut offset = 1;

    if offset + 8 > payload.len() { return Err("Flat: missing response request id".into()); }
    let request_id = u64::from_le_bytes([payload[offset], payload[offset+1], payload[offset+2], payload[offset+3],
                                          payload[offset+4], payload[offset+5], payload[offset+6], payload[offset+7]]);
    offset += 8;

    let mut fields = Vec::new();
    while offset < payload.len() {
        fields.push(decode_flat_value(payload, &mut offset)?);
    }

    Ok(FlatResponse { status, request_id, fields })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_timer(name: &str) -> impl Drop {
        let start = std::time::Instant::now();
        struct Timer { name: String, start: std::time::Instant }
        impl Drop for Timer { fn drop(&mut self) {
            eprintln!("[TEST] {} completed in {:.3}ms", self.name, self.start.elapsed().as_secs_f64() * 1000.0);
        }}
        Timer { name: name.to_string(), start }
    }

    #[test]
    fn test_is_nda_frame_valid() {
        let _t = test_timer("test_is_nda_frame_valid");
        let mut data = Vec::new();
        data.extend_from_slice(b"NMCP");
        data.extend_from_slice(&[0u8; 32]);
        assert!(is_nda_frame(&data));
    }

    #[test]
    fn test_is_nda_frame_invalid() {
        let data = b"JSON{\"method\":\"ping\"}";
        assert!(!is_nda_frame(data));
    }

    #[test]
    fn test_is_nda_frame_too_short() {
        assert!(!is_nda_frame(b"NM"));
    }

    #[test]
    fn test_tlv_round_trip() {
        let original = json!({
            "name": "read_file",
            "args": {"path": "/tmp/test.txt", "offset": 42, "flag": true, "extra": null}
        });
        let mut encoded = Vec::new();
        encode_json_value(&original, &mut encoded);
        let (decoded, consumed) = decode_json_value(&encoded).unwrap();
        assert_eq!(consumed, encoded.len());
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_build_and_parse_request() {
        let req_id = json!(1);
        let data = json!({"name": "read_file", "arguments": {"path": "/test.txt"}});
        let frame = build_nda_request(METHOD_TOOLS_CALL, &req_id, &data);

        assert!(is_nda_frame(&frame));
        assert_eq!(&frame[0..4], b"NMCP");

        let parsed = parse_nda_request(&frame).unwrap();
        assert_eq!(parsed.method, METHOD_TOOLS_CALL);
        assert_eq!(parsed.request_id, json!(1));
        assert_eq!(parsed.data["name"], "read_file");
        assert_eq!(parsed.data["arguments"]["path"], "/test.txt");
    }

    #[test]
    fn test_build_and_parse_response() {
        let req_id = json!(42);
        let result = json!({"content": [{"type": "text", "text": "hello"}]});
        let frame = build_nda_response(STATUS_OK, &req_id, &result);

        assert!(is_nda_frame(&frame));

        let payload = &frame[FRAME_HEADER_SIZE..];
        assert_eq!(payload[0], STATUS_OK);

        let mut offset = 1;
        let (parsed_id, consumed) = decode_json_value(&payload[offset..]).unwrap();
        offset += consumed;
        assert_eq!(parsed_id, json!(42));

        let (parsed_result, _) = decode_json_value(&payload[offset..]).unwrap();
        assert_eq!(parsed_result, result);
    }

    #[test]
    fn test_merkle_integrity() {
        let frame = build_nda_request(METHOD_PING, &json!(1), &Value::Null);
        let mut tampered = frame.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        assert!(parse_nda_request(&tampered).is_err());
    }

    #[test]
    fn test_method_name_mapping() {
        assert_eq!(method_name(METHOD_INITIALIZE), "initialize");
        assert_eq!(method_name(METHOD_TOOLS_CALL), "tools/call");
        assert_eq!(method_name(METHOD_PING), "ping");
        assert_eq!(method_name(0xFF), "unknown");
    }

    #[test]
    fn test_method_from_str_mapping() {
        assert_eq!(method_from_str("initialize"), Some(METHOD_INITIALIZE));
        assert_eq!(method_from_str("tools/call"), Some(METHOD_TOOLS_CALL));
        assert_eq!(method_from_str("ping"), Some(METHOD_PING));
        assert_eq!(method_from_str("nonexistent"), None);
    }

    #[test]
    fn test_error_response() {
        let frame = build_nda_error(&json!(99), "something broke");
        let payload = &frame[FRAME_HEADER_SIZE..];
        assert_eq!(payload[0], STATUS_ERROR);

        let mut offset = 1;
        let (id, consumed) = decode_json_value(&payload[offset..]).unwrap();
        offset += consumed;
        assert_eq!(id, json!(99));

        let (msg, _) = decode_json_value(&payload[offset..]).unwrap();
        assert_eq!(msg, "something broke");
    }

    #[test]
    fn test_tlv_security_depth_limit() {
        let mut buf = Vec::new();
        for _ in 0..40 {
            buf.push(0x05);
            buf.extend_from_slice(&1u32.to_be_bytes());
        }
        buf.push(0x04);
        assert!(decode_json_value(&buf).is_err());
    }

    #[test]
    fn test_ping_request_minimal() {
        let frame = build_nda_request(METHOD_PING, &json!(1), &Value::Null);
        let parsed = parse_nda_request(&frame).unwrap();
        assert_eq!(parsed.method, METHOD_PING);
        assert_eq!(parsed.request_id, json!(1));
        assert!(parsed.data.is_null());
    }

    #[test]
    fn test_flat_request_round_trip() {
        let args = json!(["/test.txt", 42, true]);
        let frame = build_flat_request(METHOD_TOOLS_CALL, &json!(1), "read_file", &args);
        assert!(is_nda_frame(&frame));

        let parsed = parse_flat_request(&frame).unwrap();
        assert_eq!(parsed.method, METHOD_TOOLS_CALL);
        assert_eq!(parsed.request_id, 1);
        assert_eq!(parsed.tool_name, "read_file");
        assert_eq!(parsed.fields.len(), 3);
        assert_eq!(parsed.fields[0], "/test.txt");
        assert_eq!(parsed.fields[1], 42);
        assert_eq!(parsed.fields[2], true);
    }

    #[test]
    fn test_flat_response_round_trip() {
        let result = json!(["hello world", 99]);
        let frame = build_flat_response(STATUS_OK, 42, &result);
        assert!(is_nda_frame(&frame));

        let parsed = parse_flat_response(&frame).unwrap();
        assert_eq!(parsed.status, STATUS_OK);
        assert_eq!(parsed.request_id, 42);
        assert_eq!(parsed.fields.len(), 2);
        assert_eq!(parsed.fields[0], "hello world");
        assert_eq!(parsed.fields[1], 99);
    }

    #[test]
    fn test_flat_null_and_bool() {
        let args = json!([null, true, false]);
        let frame = build_flat_request(METHOD_TOOLS_CALL, &json!(5), "test", &args);
        let parsed = parse_flat_request(&frame).unwrap();
        assert_eq!(parsed.fields[0], Value::Null);
        assert_eq!(parsed.fields[1], true);
        assert_eq!(parsed.fields[2], false);
    }

    #[test]
    fn test_flat_args_smaller_than_tlv_args() {
        let args = json!(["/test.txt", 42]);
        let mut tlv_buf = Vec::new();
        encode_json_value(&args, &mut tlv_buf);

        let mut flat_buf = Vec::new();
        encode_flat_value(&args, &mut flat_buf);

        assert!(flat_buf.len() < tlv_buf.len(),
            "Flat args ({} bytes) should be smaller than TLV args ({} bytes)",
            flat_buf.len(), tlv_buf.len());
    }

    #[test]
    fn test_flat_merkle_integrity() {
        let frame = build_flat_request(METHOD_PING, &json!(1), "", &Value::Null);
        let mut tampered = frame.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        assert!(parse_flat_request(&tampered).is_err());
    }

    #[test]
    #[ignore]
    fn perf_probe_tools_list() {
        use std::time::Instant;
        let iters = 200;

        let tools = crate::registry::get_tools();
        let tools_json: Vec<Value> = tools.iter().map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema
            })
        }).collect();
        let result = json!({"tools": tools_json});

        let start = Instant::now();
        let mut payload_len = 0;
        for _ in 0..iters {
            let mut payload = Vec::new();
            payload.push(STATUS_OK);
            encode_json_value(&json!(1), &mut payload);
            encode_json_value(&result, &mut payload);
            payload_len = payload.len();
        }
        let encode_us = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let buf = vec![0xABu8; payload_len];
        let start = Instant::now();
        for _ in 0..iters {
            let mut h = Sha256::new();
            h.update(&buf);
            std::hint::black_box(h.finalize());
        }
        let hash_us = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let start = Instant::now();
        for _ in 0..iters {
            let s = serde_json::to_string(&json!({"jsonrpc":"2.0","id":1,"result": &result}));
            let _ = std::hint::black_box(s);
        }
        let serde_us = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let start = Instant::now();
        for _ in 0..iters {
            let f = build_nda_response(STATUS_OK, &json!(1), &result);
            std::hint::black_box(f);
        }
        let full_us = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        let start = Instant::now();
        for _ in 0..iters {
            let t = crate::registry::get_tools();
            std::hint::black_box(t);
        }
        let get_tools_us = start.elapsed().as_secs_f64() * 1e6 / iters as f64;

        println!("PROBE tools/list ({} tools, payload {} bytes):", tools.len(), payload_len);
        println!("  get_tools:        {:7.1} us", get_tools_us);
        println!("  TLV encode:       {:7.1} us", encode_us);
        println!("  SHA-256 only:     {:7.1} us", hash_us);
        println!("  serde to_string:  {:7.1} us", serde_us);
        println!("  full nda resp:    {:7.1} us", full_us);
    }

    #[test]
    fn tools_list_direct_encoder_byte_identical() {
        let _t = test_timer("tools_list_direct_encoder_byte_identical");
        let tools = crate::registry::get_tools();
        // Reference: the old json!-based path.
        let tools_json: Vec<Value> = tools.iter().map(|t| {
            json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema
            })
        }).collect();
        let result = json!({"tools": tools_json});
        let mut expected = Vec::new();
        encode_json_value(&result, &mut expected);

        let mut actual = Vec::new();
        encode_tools_list_result(&mut actual, &tools);
        assert_eq!(actual, expected, "direct encoder must be byte-identical to Value path");

        // Round-trip through the decoder as well.
        let (decoded, consumed) = decode_json_value(&actual).expect("decode direct-encoded tools list");
        assert_eq!(consumed, actual.len());
        assert_eq!(decoded, result);
    }

    /// Cached reads and a direct encoding, all bracketed by an unchanged
    /// generation; retry if a concurrent test thread registers a tool
    /// mid-window (other tests mutate the shared registry).
    fn stable_encoded_triple() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        loop {
            let g0 = crate::registry::registry_generation();
            let a = encoded_tools_list_result();
            let b = encoded_tools_list_result();
            let tools = crate::registry::get_tools();
            let mut direct = Vec::new();
            encode_tools_list_result(&mut direct, &tools);
            if crate::registry::registry_generation() == g0 {
                return (a, b, direct);
            }
        }
    }

    #[test]
    fn tools_list_cache_hit_and_invalidation() {
        let _t = test_timer("tools_list_cache_hit_and_invalidation");
        // Repeat calls serve identical cached bytes, matching a direct encoding.
        let t0 = std::time::Instant::now();
        let (first, second, expected) = stable_encoded_triple();
        eprintln!("[METRIC] encoded_tools_list_result (3 calls): {:.3}us", t0.elapsed().as_secs_f64() * 1e6);
        assert_eq!(first, second);
        assert_eq!(first, expected);

        // Registering a tool bumps the generation and forces a rebuild.
        let gen_before = crate::registry::registry_generation();
        crate::registry::register_tool_lazy(&crate::registry::Tool {
            name: "__nda_cache_invalidation_probe".to_string(),
            description: "probe tool for cache invalidation test".to_string(),
            input_schema: json!({"type": "object", "properties": {}, "required": []}),
        });
        assert!(crate::registry::registry_generation() > gen_before);
        let rebuilt = encoded_tools_list_result();
        assert_ne!(rebuilt, first);
        let (decoded, _) = decode_json_value(&rebuilt).expect("decode rebuilt tools list");
        let names: Vec<&str> = decoded["tools"].as_array().expect("tools array")
            .iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"__nda_cache_invalidation_probe"), "cache must rebuild after registration");
    }

    #[test]
    fn tools_list_plugin_reload_bumps_generation() {
        // Reloading plugins replaces the plugin registry, so the generation
        // must bump even when the directory holds no plugins.
        let gen_before = crate::registry::registry_generation();
        crate::registry::load_plugins(std::env::temp_dir().to_str().unwrap());
        assert!(
            crate::registry::registry_generation() > gen_before,
            "load_plugins must bump the registry generation"
        );
    }

    #[test]
    fn tools_list_cache_survives_concurrent_registration() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let stop = Arc::new(AtomicBool::new(false));
        let mut handles = Vec::new();

        // Readers: hammer the cached accessor; every result must decode and
        // always contain the built-in tools (cache must never tear). Each
        // reader guarantees a minimum number of iterations before honoring
        // the stop flag, so scheduling order can never zero out `total`.
        for _ in 0..4 {
            let stop = Arc::clone(&stop);
            handles.push(std::thread::spawn(move || {
                let mut iters = 0usize;
                loop {
                    let bytes = encoded_tools_list_result();
                    let (decoded, consumed) =
                        decode_json_value(&bytes).expect("cached bytes must decode");
                    assert_eq!(consumed, bytes.len());
                    let names: Vec<&str> = decoded["tools"].as_array().expect("tools array")
                        .iter().map(|t| t["name"].as_str().unwrap()).collect();
                    assert!(names.contains(&"read_nda"), "built-ins must survive caching");
                    iters += 1;
                    if iters >= 200 && stop.load(Ordering::Relaxed) {
                        break;
                    }
                }
                iters
            }));
        }

        // Writer: register tools while readers run.
        for i in 0..8 {
            crate::registry::register_tool_lazy(&crate::registry::Tool {
                name: format!("__hammer_probe_{:02}", i),
                description: format!("concurrency probe {}", i),
                input_schema: json!({"type": "object", "properties": {}, "required": []}),
            });
        }
        stop.store(true, Ordering::Relaxed);

        let mut total = 0usize;
        for h in handles {
            total += h.join().expect("reader thread panicked");
        }
        assert!(total > 0, "readers must have run");

        // Final state: cache reflects all registered hammer tools.
        let final_bytes = encoded_tools_list_result();
        let (decoded, _) = decode_json_value(&final_bytes).unwrap();
        let names: Vec<&str> = decoded["tools"].as_array().unwrap()
            .iter().map(|t| t["name"].as_str().unwrap()).collect();
        for i in 0..8 {
            let probe = format!("__hammer_probe_{:02}", i);
            assert!(names.contains(&probe.as_str()), "missing {} in final cache", probe);
        }
    }

    // ─── Zero-alloc in-place parse tests ─────────────────────────────────

    #[test]
    fn inplace_parse_matches_value_parser() {
        let _t = test_timer("inplace_parse_matches_value_parser");
        let cases = [
            (METHOD_PING, json!(1), Value::Null),
            (METHOD_TOOLS_LIST, json!(99), Value::Null),
            (METHOD_TOOLS_CALL, json!("req-7"), json!({"name": "bench_echo", "arguments": {"size": 64}})),
            (METHOD_HEALTH_CHECK, json!(2), json!({"deep": {"nested": [1, 2, {"k": "v"}]}})),
        ];
        for (method, id, data) in &cases {
            let frame = build_nda_request(*method, id, data);
            let old = parse_nda_request(&frame).unwrap();
            let new = parse_nda_request_inplace(&frame).unwrap();
            assert_eq!(new.method, old.method, "method mismatch");
            let (new_id, id_consumed) = decode_json_value(new.id_tlv).unwrap();
            assert_eq!(id_consumed, new.id_tlv.len());
            assert_eq!(new_id, old.request_id, "request id mismatch");
            if new.data.is_empty() {
                assert_eq!(old.data, Value::Null, "empty data must mean Null");
            } else {
                let (new_data, data_consumed) = decode_json_value(new.data).unwrap();
                assert_eq!(data_consumed, new.data.len());
                assert_eq!(new_data, old.data, "data mismatch");
            }
        }
    }

    #[test]
    fn inplace_parse_rejects_bad_frames() {
        let frame = build_nda_request(METHOD_PING, &json!(1), &Value::Null);
        assert!(parse_nda_request_inplace(&frame[..10]).is_err(), "truncated header");
        let mut bad_magic = frame.clone();
        bad_magic[0] = b'X';
        assert!(parse_nda_request_inplace(&bad_magic).is_err(), "bad magic");
        let mut tampered = frame.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xFF;
        assert!(parse_nda_request_inplace(&tampered).is_err(), "tampered payload");
        // Payload of just a method byte: no id TLV
        let no_id = build_nda_frame(&[METHOD_PING]);
        assert!(parse_nda_request_inplace(&no_id).is_err(), "missing id");
        // Truncated id TLV: string tag claiming 100 bytes
        let mut truncated = vec![METHOD_PING, 0x01, 0, 0, 0, 100];
        let trunc_frame = build_nda_frame(&mut truncated);
        assert!(parse_nda_request_inplace(&trunc_frame).is_err(), "truncated id TLV");
    }

    #[test]
    fn extract_tools_call_fields_any_key_order() {
        // Hand-built object with name FIRST (serde would sort arguments first,
        // so this proves the walker matches by key, not position).
        let mut name_first = vec![0x06]; // object
        name_first.extend_from_slice(&2u32.to_be_bytes());
        name_first.extend_from_slice(&4u16.to_be_bytes());
        name_first.extend_from_slice(b"name");
        name_first.push(0x01);
        name_first.extend_from_slice(&8u32.to_be_bytes());
        name_first.extend_from_slice(b"my_tool_");
        name_first.extend_from_slice(&9u16.to_be_bytes());
        name_first.extend_from_slice(b"arguments");
        name_first.push(0x04); // null

        let (name, args) = extract_tools_call_fields(&name_first).unwrap();
        assert_eq!(name, Some("my_tool_"));
        assert_eq!(args, Some(&[0x04u8][..]));

        // Via encode_json_value (sorted keys: arguments before name)
        let mut sorted = Vec::new();
        encode_json_value(&json!({"name": "echo", "arguments": {"size": 64}}), &mut sorted);
        let (name, args) = extract_tools_call_fields(&sorted).unwrap();
        assert_eq!(name, Some("echo"));
        let (args_val, consumed) = decode_json_value(args.unwrap()).unwrap();
        assert_eq!(consumed, args.unwrap().len());
        assert_eq!(args_val, json!({"size": 64}));

        // Missing fields and non-object data match serde indexing semantics
        let (name, args) = extract_tools_call_fields(&sorted[..0]).unwrap();
        assert!(name.is_none() && args.is_none());
        let mut only_name = vec![0x06];
        only_name.extend_from_slice(&1u32.to_be_bytes());
        only_name.extend_from_slice(&4u16.to_be_bytes());
        only_name.extend_from_slice(b"name");
        only_name.push(0x01);
        only_name.extend_from_slice(&1u32.to_be_bytes());
        only_name.extend_from_slice(b"x");
        let (name, args) = extract_tools_call_fields(&only_name).unwrap();
        assert_eq!(name, Some("x"));
        assert!(args.is_none());
        let string_data = {
            let mut b = Vec::new();
            encode_json_value(&json!("not an object"), &mut b);
            b
        };
        let (name, args) = extract_tools_call_fields(&string_data).unwrap();
        assert!(name.is_none() && args.is_none());
    }

    #[test]
    fn raw_builders_byte_identical_to_value_builders() {
        let id = json!(12345);
        let result = json!({"status": "healthy", "mode": "shmem-nda"});
        let mut id_tlv = Vec::new();
        encode_json_value(&id, &mut id_tlv);
        let mut result_tlv = Vec::new();
        encode_json_value(&result, &mut result_tlv);

        assert_eq!(
            build_nda_response(STATUS_OK, &id, &result),
            build_nda_response_raw(STATUS_OK, &id_tlv, &result_tlv)
        );
        assert_eq!(
            build_nda_error(&id, "boom"),
            build_nda_error_raw(&id_tlv, "boom")
        );
    }

    #[test]
    fn prebuilt_tlvs_decode_correctly() {
        let (empty, consumed) = decode_json_value(EMPTY_OBJECT_TLV).unwrap();
        assert_eq!(consumed, EMPTY_OBJECT_TLV.len());
        assert_eq!(empty, json!({}));

        let health = health_result_tlv();
        let (val, consumed) = decode_json_value(health).unwrap();
        assert_eq!(consumed, health.len());
        assert_eq!(val["status"], "healthy");
        assert_eq!(val["mode"], "shmem-nda");
        assert_eq!(val["version"], crate::VERSION);
        assert_eq!(health_result_tlv(), health, "must be cached, stable bytes");
    }

}
