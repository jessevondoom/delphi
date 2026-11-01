//! The MCP wire shared by Delphi's stdio servers (`delphi bundle serve`, `specs/025` §4.4, and `delphi mcp`, `specs/062` §3.1):
//! newline-delimited JSON-RPC, a bounded request line, the request envelope checks and the reply shapes.
//!
//! It holds no state and no policy: no file, no socket, no process. What a method does is the server's business.

use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

/// Longest accepted request line. Every request these servers understand is far smaller.
pub const MAX_LINE: usize = 64 * 1024;
/// The protocol versions spoken, newest first; a client that asks for another is answered with the first.
pub const PROTOCOLS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub fn err(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub fn tool_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

pub fn tool_err(msg: &str) -> Value {
    tool_result(msg.to_string(), true)
}

pub fn cut_at_char(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut n = max;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

/// A request that passed the envelope checks.
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Map<String, Value>,
}

/// The envelope checks, in order: a single object (no batch), an id that is a string or a number, a string method, `"jsonrpc": "2.0"`,
/// a request rather than a notification, object params. `Err` carries the reply to write, or `None` when nothing is to be said
/// (a notification, or a reply to a request the server never made).
pub fn parse_request(v: Value) -> Result<Request, Option<Value>> {
    let Value::Object(obj) = v else { return Err(Some(err(Value::Null, -32600, "invalid request: a single JSON object is expected (batches are not supported)"))) };
    let id = match obj.get("id") {
        None => None,
        Some(i @ (Value::String(_) | Value::Number(_))) => Some(i.clone()),
        Some(_) => return Err(Some(err(Value::Null, -32600, "invalid request: id must be a string or a number"))),
    };
    let method = match obj.get("method") {
        Some(Value::String(m)) => m.clone(),
        // A reply to a request this server never made, or junk: nothing to say to either.
        _ if obj.contains_key("result") || obj.contains_key("error") => return Err(None),
        _ => return Err(id.map(|i| err(i, -32600, "invalid request: method must be a string")).or_else(|| Some(err(Value::Null, -32600, "invalid request: method must be a string")))),
    };
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(Some(err(id.unwrap_or(Value::Null), -32600, "invalid request: jsonrpc must be \"2.0\"")));
    }
    let Some(id) = id else { return Err(None) }; // a notification is never answered
    let params = match obj.get("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(p)) => p.clone(),
        Some(_) => return Err(Some(err(id, -32602, "invalid params: an object is expected"))),
    };
    Ok(Request { id, method, params })
}

/// One input line parsed as JSON and handed to `handle`; a line that is not UTF-8 or not JSON is answered with a parse error.
pub fn handle_bytes(line: &[u8], handle: &mut dyn FnMut(Value) -> Option<Value>) -> Option<Value> {
    let Ok(text) = std::str::from_utf8(line) else { return Some(err(Value::Null, -32700, "parse error: the request is not UTF-8")) };
    match serde_json::from_str::<Value>(text) {
        Ok(v) => handle(v),
        Err(_) => Some(err(Value::Null, -32700, "parse error: the request is not valid JSON")),
    }
}

pub enum Line {
    Eof,
    Ok(Vec<u8>),
    TooLong,
}

/// One newline-terminated line of at most `MAX_LINE` bytes. A longer line is consumed to its end without being kept.
pub fn read_line<R: BufRead>(r: &mut R) -> std::io::Result<Line> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let chunk = match r.fill_buf() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        if chunk.is_empty() {
            return Ok(if too_long {
                Line::TooLong
            } else if buf.is_empty() {
                Line::Eof
            } else {
                Line::Ok(buf)
            });
        }
        let (take, found) = match chunk.iter().position(|b| *b == b'\n') {
            Some(i) => (i + 1, true),
            None => (chunk.len(), false),
        };
        if !too_long {
            if buf.len() + take > MAX_LINE + 1 {
                too_long = true;
                buf = Vec::new();
            } else {
                buf.extend_from_slice(&chunk[..take]);
            }
        }
        r.consume(take);
        if found {
            if too_long {
                return Ok(Line::TooLong);
            }
            while matches!(buf.last(), Some(b'\n' | b'\r')) {
                buf.pop();
            }
            return Ok(Line::Ok(buf));
        }
    }
}

pub fn write_line<W: Write>(out: &mut W, v: &Value) -> std::io::Result<()> {
    let mut s = serde_json::to_vec(v).unwrap_or_default();
    s.push(b'\n');
    out.write_all(&s)?;
    out.flush()
}

/// Serve until the input closes: `handle` gets each line's bytes and returns the reply to write, if any. Returns the exit code.
pub fn serve_lines<R: BufRead, W: Write>(mut input: R, mut output: W, handle: &mut dyn FnMut(&[u8]) -> Option<Value>) -> i32 {
    loop {
        let reply = match read_line(&mut input) {
            Ok(Line::Eof) => return 0,
            Ok(Line::Ok(b)) if b.iter().all(u8::is_ascii_whitespace) => continue,
            Ok(Line::Ok(b)) => handle(&b),
            Ok(Line::TooLong) => Some(err(Value::Null, -32600, "invalid request: the request is over the 64 KiB limit")),
            Err(_) => return 1,
        };
        if let Some(r) = reply {
            if write_line(&mut output, &r).is_err() {
                return 1; // the client went away
            }
        }
    }
}
