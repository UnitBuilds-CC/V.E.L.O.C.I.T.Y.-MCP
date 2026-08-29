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

pub const METHOD_INITIALIZE: u8 = 0x01;
pub const METHOD_TOOLS_LIST: u8 = 0x02;
pub const METHOD_TOOLS_CALL: u8 = 0x03;
pub const METHOD_PING: u8 = 0x04;
pub const METHOD_LOGGING_SET_LEVEL: u8 = 0x05;
pub const METHOD_HEALTH_CHECK: u8 = 0x06;
pub const NOTIF_INITIALIZED: u8 = 0x10;
pub const NOTIF_CANCELLED: u8 = 0x11;

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

    #[test]
    fn test_is_nda_frame_valid() {
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
}
