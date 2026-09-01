//! NDA binary TLV codec for the client SDK.
//!
//! Encodes/decodes the NDA binary frame format used by VELOCITY-MCP's
//! shared memory transport. This is a client-side extraction of the
//! protocol defined in `src/protocol/nda_native.rs`.

use crate::error::{Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const NDA_MAGIC: &[u8; 4] = b"NMCP";
pub const FRAME_HEADER_SIZE: usize = 36;

#[allow(dead_code)]
pub const STATUS_OK: u8 = 0;
pub const STATUS_ERROR: u8 = 1;

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

pub fn method_to_code(method: &str) -> Option<u8> {
    match method {
        "initialize" => Some(METHOD_INITIALIZE),
        "tools/list" => Some(METHOD_TOOLS_LIST),
        "tools/call" => Some(METHOD_TOOLS_CALL),
        "ping" => Some(METHOD_PING),
        "logging/setLevel" => Some(METHOD_LOGGING_SET_LEVEL),
        "health/check" => Some(METHOD_HEALTH_CHECK),
        "resources/list" => Some(METHOD_RESOURCES_LIST),
        "resources/read" => Some(METHOD_RESOURCES_READ),
        "resources/templates/list" => Some(METHOD_RESOURCE_TEMPLATES_LIST),
        "prompts/list" => Some(METHOD_PROMPTS_LIST),
        "prompts/get" => Some(METHOD_PROMPTS_GET),
        "sampling/createMessage" => Some(METHOD_SAMPLING_CREATE),
        "notifications/initialized" => Some(NOTIF_INITIALIZED),
        "notifications/cancelled" => Some(NOTIF_CANCELLED),
        _ => None,
    }
}

pub fn encode_tlv_value(value: &Value, buf: &mut Vec<u8>) -> Result<()> {
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
                encode_tlv_value(item, buf)?;
            }
        }
        Value::Object(obj) => {
            buf.push(0x06);
            buf.extend_from_slice(&(obj.len() as u32).to_be_bytes());
            for (key, val) in obj {
                let key_bytes = key.as_bytes();
                if key_bytes.len() > u16::MAX as usize {
                    return Err(Error::NdaProtocol(format!(
                        "TLV object key length {} exceeds u16 max", key_bytes.len()
                    )));
                }
                buf.extend_from_slice(&(key_bytes.len() as u16).to_be_bytes());
                buf.extend_from_slice(key_bytes);
                encode_tlv_value(val, buf)?;
            }
        }
    }
    Ok(())
}

pub fn decode_tlv_value(bytes: &[u8]) -> Result<(Value, usize)> {
    if bytes.is_empty() {
        return Err(Error::NdaProtocol("TLV: empty value".into()));
    }
    match bytes[0] {
        0x01 => {
            if bytes.len() < 5 {
                return Err(Error::NdaProtocol("TLV string: truncated length".into()));
            }
            let len = u32::from_be_bytes(bytes[1..5].try_into().map_err(|_| Error::NdaProtocol("TLV string: bad length".into()))?) as usize;
            if bytes.len() < 5 + len {
                return Err(Error::NdaProtocol("TLV string: truncated body".into()));
            }
            let s = std::str::from_utf8(&bytes[5..5 + len])
                .map_err(|e| Error::NdaProtocol(format!("TLV string: invalid UTF-8: {}", e)))?;
            Ok((json!(s), 5 + len))
        }
        0x02 => {
            if bytes.len() < 9 {
                return Err(Error::NdaProtocol("TLV i64: truncated".into()));
            }
            let v = i64::from_be_bytes(bytes[1..9].try_into().map_err(|_| Error::NdaProtocol("TLV i64: bad length".into()))?);
            Ok((json!(v), 9))
        }
        0x03 => {
            if bytes.len() < 2 {
                return Err(Error::NdaProtocol("TLV bool: truncated".into()));
            }
            Ok((json!(bytes[1] != 0), 2))
        }
        0x04 => Ok((Value::Null, 1)),
        0x05 => {
            if bytes.len() < 5 {
                return Err(Error::NdaProtocol("TLV array: truncated count".into()));
            }
            let count = u32::from_be_bytes(bytes[1..5].try_into().map_err(|_| Error::NdaProtocol("TLV array: bad count".into()))?) as usize;
            let mut arr = Vec::with_capacity(count.min(1024));
            let mut off = 5usize;
            for _ in 0..count {
                let (v, n) = decode_tlv_value(&bytes[off..])?;
                arr.push(v);
                off += n;
            }
            Ok((json!(arr), off))
        }
        0x06 => {
            if bytes.len() < 5 {
                return Err(Error::NdaProtocol("TLV object: truncated count".into()));
            }
            let count = u32::from_be_bytes(bytes[1..5].try_into().map_err(|_| Error::NdaProtocol("TLV object: bad count".into()))?) as usize;
            let mut obj = serde_json::Map::with_capacity(count.min(1024));
            let mut off = 5usize;
            for _ in 0..count {
                if bytes.len() < off + 2 {
                    return Err(Error::NdaProtocol("TLV object: truncated key length".into()));
                }
                let klen = u16::from_be_bytes(bytes[off..off + 2].try_into().map_err(|_| Error::NdaProtocol("TLV object: bad key length".into()))?) as usize;
                off += 2;
                if bytes.len() < off + klen {
                    return Err(Error::NdaProtocol("TLV object: truncated key".into()));
                }
                let key = std::str::from_utf8(&bytes[off..off + klen])
                    .map_err(|e| Error::NdaProtocol(format!("TLV object key: invalid UTF-8: {}", e)))?
                    .to_string();
                off += klen;
                let (v, n) = decode_tlv_value(&bytes[off..])?;
                obj.insert(key, v);
                off += n;
            }
            Ok((Value::Object(obj), off))
        }
        0x07 => {
            if bytes.len() < 9 {
                return Err(Error::NdaProtocol("TLV f64: truncated".into()));
            }
            let v = f64::from_be_bytes(bytes[1..9].try_into().map_err(|_| Error::NdaProtocol("TLV f64: bad length".into()))?);
            Ok((json!(v), 9))
        }
        other => Err(Error::NdaProtocol(format!("TLV: unknown tag 0x{:02x}", other))),
    }
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

pub fn build_nda_request(method: u8, request_id: &Value, data: &Value) -> Result<Vec<u8>> {
    let mut payload = Vec::new();
    payload.push(method);
    encode_tlv_value(request_id, &mut payload)?;
    if !data.is_null() {
        encode_tlv_value(data, &mut payload)?;
    }
    Ok(build_nda_frame(&payload))
}

#[derive(Debug)]
pub struct NdaResponse {
    pub status: u8,
    pub request_id: Value,
    pub result: Value,
}

pub fn parse_nda_response(frame: &[u8]) -> Result<NdaResponse> {
    if frame.len() < FRAME_HEADER_SIZE + 1 {
        return Err(Error::NdaProtocol(format!(
            "NDA response too small ({} bytes)",
            frame.len()
        )));
    }
    if &frame[0..4] != NDA_MAGIC {
        return Err(Error::NdaProtocol("bad NDA magic".into()));
    }

    let payload = &frame[FRAME_HEADER_SIZE..];

    let mut hasher = Sha256::new();
    hasher.update(payload);
    let computed = hasher.finalize();
    if &frame[4..36] != computed.as_slice() {
        return Err(Error::NdaProtocol("NDA Merkle root mismatch".into()));
    }

    let status = payload[0];
    let mut offset = 1;

    let (request_id, consumed) = decode_tlv_value(&payload[offset..])?;
    offset += consumed;

    let result = if offset < payload.len() {
        let (val, _) = decode_tlv_value(&payload[offset..])?;
        val
    } else {
        json!({})
    };

    Ok(NdaResponse {
        status,
        request_id,
        result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_empty_input() {
        assert!(decode_tlv_value(&[]).is_err());
    }

    #[test]
    fn test_decode_unknown_tag() {
        assert!(decode_tlv_value(&[0xFF]).is_err());
    }

    #[test]
    fn test_decode_truncated_string() {
        let data = [0x01, 0x00, 0x00, 0x00, 0x05, b'h', b'i'];
        let result = decode_tlv_value(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("truncated"));
    }

    #[test]
    fn test_decode_truncated_i64() {
        let data = [0x02, 0x00, 0x00];
        assert!(decode_tlv_value(&data).is_err());
    }

    #[test]
    fn test_decode_truncated_bool() {
        assert!(decode_tlv_value(&[0x03]).is_err());
    }

    #[test]
    fn test_decode_truncated_f64() {
        let data = [0x07, 0x00, 0x00, 0x00];
        assert!(decode_tlv_value(&data).is_err());
    }

    #[test]
    fn test_roundtrip_string() {
        let val = json!("hello world");
        let mut buf = Vec::new();
        encode_tlv_value(&val, &mut buf).unwrap();
        let (decoded, consumed) = decode_tlv_value(&buf).unwrap();
        assert_eq!(decoded, val);
        assert_eq!(consumed, buf.len());
    }

    #[test]
    fn test_roundtrip_object() {
        let val = json!({"key": "value", "num": 42, "flag": true, "nil": null});
        let mut buf = Vec::new();
        encode_tlv_value(&val, &mut buf).unwrap();
        let (decoded, _) = decode_tlv_value(&buf).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn test_roundtrip_array() {
        let val = json!([1, "two", false, null, [3, 4]]);
        let mut buf = Vec::new();
        encode_tlv_value(&val, &mut buf).unwrap();
        let (decoded, _) = decode_tlv_value(&buf).unwrap();
        assert_eq!(decoded, val);
    }

    #[test]
    fn test_parse_response_too_small() {
        let data = [0x00; 10];
        assert!(parse_nda_response(&data).is_err());
    }

    #[test]
    fn test_parse_response_bad_magic() {
        let mut data = vec![0u8; FRAME_HEADER_SIZE + 10];
        data[0..4].copy_from_slice(b"BAAD");
        assert!(parse_nda_response(&data).is_err());
        assert!(parse_nda_response(&data).unwrap_err().to_string().contains("magic"));
    }

    #[test]
    fn test_parse_response_bad_merkle() {
        let payload = [0x00, 0x04, 0x00, 0x00, 0x00, 0x01];
        let mut frame = Vec::new();
        frame.extend_from_slice(NDA_MAGIC);
        frame.extend_from_slice(&[0xFF; 32]);
        frame.extend_from_slice(&payload);
        let result = parse_nda_response(&frame);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Merkle"));
    }

    #[test]
    fn test_build_and_parse_roundtrip() {
        let id = json!(42);
        let data = json!({"result": "ok"});
        let frame = build_nda_request(METHOD_PING, &id, &data).unwrap();

        assert_eq!(&frame[0..4], NDA_MAGIC);
        assert!(frame.len() > FRAME_HEADER_SIZE);

        let payload = &frame[FRAME_HEADER_SIZE..];
        assert_eq!(payload[0], METHOD_PING);
    }

    #[test]
    fn test_method_to_code_unknown() {
        assert!(method_to_code("nonexistent/method").is_none());
    }

    #[test]
    fn test_method_to_code_all_known() {
        assert_eq!(method_to_code("initialize"), Some(METHOD_INITIALIZE));
        assert_eq!(method_to_code("tools/list"), Some(METHOD_TOOLS_LIST));
        assert_eq!(method_to_code("tools/call"), Some(METHOD_TOOLS_CALL));
        assert_eq!(method_to_code("ping"), Some(METHOD_PING));
        assert_eq!(method_to_code("notifications/initialized"), Some(NOTIF_INITIALIZED));
    }

    #[test]
    fn test_key_length_overflow_rejected() {
        let long_key = "k".repeat(u16::MAX as usize + 1);
        let val = serde_json::Map::from_iter([(long_key, json!("v"))]);
        let mut buf = Vec::new();
        let result = encode_tlv_value(&Value::Object(val), &mut buf);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds u16 max"));
    }
}
