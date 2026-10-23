//! AWS `vnd.amazon.eventstream` decoding, for Bedrock `InvokeModelWithResponseStream`.
//!
//! Frame: `total_len(4) headers_len(4) prelude_crc(4) headers payload message_crc(4)`, big-endian, CRC-32.
//! Every frame is verified; a bad CRC or length aborts the stream. For Bedrock, a `chunk` event's
//! payload is `{"bytes": "<base64 of one Anthropic stream event>"}`.

use crate::claude::{SseEvent, StreamError};
use base64::Engine;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub headers: BTreeMap<String, String>,
    pub payload: Vec<u8>,
}

#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

fn parse_headers(mut h: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    while !h.is_empty() {
        let n = *h.first().ok_or("truncated header")? as usize;
        if h.len() < 1 + n + 1 {
            return Err("truncated header name".into());
        }
        let name = String::from_utf8_lossy(&h[1..1 + n]).into_owned();
        let ty = h[1 + n];
        h = &h[2 + n..];
        let take = |h: &mut &[u8], len: usize| -> Result<Vec<u8>, String> {
            if h.len() < len {
                return Err("truncated header value".into());
            }
            let v = h[..len].to_vec();
            *h = &h[len..];
            Ok(v)
        };
        let value = match ty {
            0 => "true".to_string(),
            1 => "false".to_string(),
            2 => take(&mut h, 1)?[0].to_string(),
            3 => i16::from_be_bytes(take(&mut h, 2)?.try_into().unwrap()).to_string(),
            4 => i32::from_be_bytes(take(&mut h, 4)?.try_into().unwrap()).to_string(),
            5 | 8 => i64::from_be_bytes(take(&mut h, 8)?.try_into().unwrap()).to_string(),
            6 | 7 => {
                let len = u16::from_be_bytes(take(&mut h, 2)?.try_into().unwrap()) as usize;
                String::from_utf8_lossy(&take(&mut h, len)?).into_owned()
            }
            9 => take(&mut h, 16)?.iter().map(|b| format!("{:02x}", b)).collect(),
            other => return Err(format!("unknown header type {}", other)),
        };
        out.insert(name, value);
    }
    Ok(out)
}

impl Decoder {
    /// Feed bytes; returns every complete, verified message.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Message>, String> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            if self.buf.len() < 12 {
                return Ok(out);
            }
            let total = u32_at(&self.buf, 0) as usize;
            let hdr_len = u32_at(&self.buf, 4) as usize;
            if crc32fast::hash(&self.buf[..8]) != u32_at(&self.buf, 8) {
                return Err("event-stream prelude CRC mismatch".into());
            }
            if total < 16 + hdr_len || total > 16 * 1024 * 1024 {
                return Err(format!("implausible event-stream frame length {}", total));
            }
            if self.buf.len() < total {
                return Ok(out);
            }
            let frame: Vec<u8> = self.buf.drain(..total).collect();
            if crc32fast::hash(&frame[..total - 4]) != u32_at(&frame, total - 4) {
                return Err("event-stream message CRC mismatch".into());
            }
            let headers = parse_headers(&frame[12..12 + hdr_len])?;
            out.push(Message { headers, payload: frame[12 + hdr_len..total - 4].to_vec() });
        }
    }
}

/// Turn a Bedrock message into the Anthropic stream event it carries (`None` for non-event frames).
pub fn to_sse(m: &Message) -> Result<Option<SseEvent>, StreamError> {
    match m.headers.get(":message-type").map(|s| s.as_str()) {
        Some("event") => {
            let v: serde_json::Value = serde_json::from_slice(&m.payload).map_err(|e| StreamError::Malformed(format!("chunk payload: {}", e)))?;
            let b64 = v["bytes"].as_str().ok_or_else(|| StreamError::Malformed("chunk has no `bytes`".into()))?;
            let raw = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|e| StreamError::Malformed(format!("chunk base64: {}", e)))?;
            let data = String::from_utf8(raw).map_err(|e| StreamError::Malformed(format!("chunk utf8: {}", e)))?;
            Ok(Some(SseEvent { event: None, data }))
        }
        Some("exception") | Some("error") => {
            let kind = m.headers.get(":exception-type").or_else(|| m.headers.get(":error-code")).cloned().unwrap_or_else(|| "exception".into());
            let message = serde_json::from_slice::<serde_json::Value>(&m.payload).ok().and_then(|v| v["message"].as_str().map(String::from)).unwrap_or_default();
            Err(StreamError::Provider { kind, message })
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn encode(headers: &[(&str, &str)], payload: &[u8]) -> Vec<u8> {
        let mut h = Vec::new();
        for (k, v) in headers {
            h.push(k.len() as u8);
            h.extend_from_slice(k.as_bytes());
            h.push(7);
            h.extend_from_slice(&(v.len() as u16).to_be_bytes());
            h.extend_from_slice(v.as_bytes());
        }
        let total = (16 + h.len() + payload.len()) as u32;
        let mut m = Vec::new();
        m.extend_from_slice(&total.to_be_bytes());
        m.extend_from_slice(&(h.len() as u32).to_be_bytes());
        m.extend_from_slice(&crc32fast::hash(&m).to_be_bytes());
        m.extend_from_slice(&h);
        m.extend_from_slice(payload);
        m.extend_from_slice(&crc32fast::hash(&m).to_be_bytes());
        m
    }

    /// A Bedrock `chunk` frame carrying one Anthropic stream event.
    pub(crate) fn chunk(event_json: &str) -> Vec<u8> {
        let payload = serde_json::json!({"bytes": base64::engine::general_purpose::STANDARD.encode(event_json)}).to_string();
        encode(&[(":event-type", "chunk"), (":content-type", "application/json"), (":message-type", "event")], payload.as_bytes())
    }

    #[test]
    fn frames_decode_across_arbitrary_chunk_boundaries() {
        let mut wire = chunk(r#"{"type":"ping"}"#);
        wire.extend(chunk(r#"{"type":"message_stop"}"#));
        for size in [1usize, 3, 11, 12, 13, 50, 1000] {
            let mut d = Decoder::default();
            let mut msgs = Vec::new();
            for part in wire.chunks(size) {
                msgs.extend(d.push(part).unwrap());
            }
            assert_eq!(msgs.len(), 2, "chunk size {}", size);
            assert_eq!(msgs[0].headers[":event-type"], "chunk");
            assert_eq!(to_sse(&msgs[1]).unwrap().unwrap().data, r#"{"type":"message_stop"}"#);
        }
    }

    #[test]
    fn corruption_is_detected() {
        let mut wire = chunk(r#"{"type":"ping"}"#);
        let n = wire.len();
        wire[n - 6] ^= 0xff; // payload
        assert!(Decoder::default().push(&wire).unwrap_err().contains("message CRC"));
        let mut wire = chunk(r#"{"type":"ping"}"#);
        wire[2] ^= 0x01; // prelude
        assert!(Decoder::default().push(&wire).unwrap_err().contains("prelude CRC"));
    }

    #[test]
    fn exceptions_become_provider_errors() {
        let wire = encode(&[(":message-type", "exception"), (":exception-type", "throttlingException")], br#"{"message":"Too many requests"}"#);
        let m = Decoder::default().push(&wire).unwrap().remove(0);
        assert_eq!(to_sse(&m).unwrap_err(), StreamError::Provider { kind: "throttlingException".into(), message: "Too many requests".into() });
    }
}
