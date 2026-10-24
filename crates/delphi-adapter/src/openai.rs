//! The compat pass (`specs/002` §2.2): IPR + turn history → an OpenAI **chat-completions** body, and the
//! `chat.completion.chunk` stream → the same typed `ContentBlock`s the other passes yield. It serves
//! `gateway-compat` endpoints for any family, and later `codex`.
//!
//! This path is **degraded** by design and labelled so in the agents pane:
//! * Reasoning blocks are dropped when replaying; no signature ever crosses it. Reasoning text a gateway
//!   streams (`reasoning_content`) is shown live and kept as an unsigned `Thinking` block.
//! * No caching is assumed; cache-read tokens are recorded only when the gateway reports them.
//! * No thinking-budget control. `strict` tool schemas are not forced (backends differ on accepting them).
//! * `max_tokens` is used rather than `max_completion_tokens`, which older gateways reject.

use crate::blocks::{ContentBlock, ProviderMeta, Role, Turn};
use crate::claude::{Completed, CompileError, CompileOptions, Compiled, SseEvent, StreamError, StreamEvent, Usage};
use crate::ipr::Ipr;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const PROVIDER: &str = "openai-compat";

fn valid_tool_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn assistant_message(t: &Turn, downgraded: &mut usize) -> Value {
    let mut text = String::new();
    let mut calls = Vec::new();
    for b in &t.blocks {
        match b {
            ContentBlock::Text { text: s } => {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(s);
            }
            ContentBlock::ToolCall { id, name, arguments } => calls.push(json!({"id": id, "type": "function", "function": {"name": name, "arguments": arguments.to_string()}})),
            ContentBlock::Thinking { .. } | ContentBlock::RedactedThinking { .. } => *downgraded += 1,
            ContentBlock::ToolResult { .. } => {}
        }
    }
    let mut m = Map::new();
    m.insert("role".into(), json!("assistant"));
    if calls.is_empty() {
        m.insert("content".into(), json!(if text.is_empty() { "(no visible output)".to_string() } else { text }));
    } else {
        m.insert("content".into(), if text.is_empty() { Value::Null } else { json!(text) });
        m.insert("tool_calls".into(), Value::Array(calls));
    }
    Value::Object(m)
}

pub fn compile(ipr: &Ipr, history: &[Turn], opts: &CompileOptions) -> Result<Compiled, CompileError> {
    for t in &opts.tools {
        if !valid_tool_name(&t.name) {
            return Err(CompileError::BadToolName(t.name.clone()));
        }
    }
    let mut downgraded = 0usize;
    let mut messages: Vec<Value> = vec![json!({"role": "system", "content": ipr.system_sections().join("\n\n")}), json!({"role": "user", "content": ipr.first_user_text()})];
    for t in history {
        match t.role {
            Role::Assistant => messages.push(assistant_message(t, &mut downgraded)),
            Role::User => {
                let mut texts = Vec::new();
                for b in &t.blocks {
                    match b {
                        ContentBlock::ToolResult { tool_call_id, content, is_error } => {
                            let content = if *is_error { format!("ERROR: {}", content) } else { content.clone() };
                            messages.push(json!({"role": "tool", "tool_call_id": tool_call_id, "content": content}));
                        }
                        ContentBlock::Text { text } => texts.push(text.clone()),
                        _ => {}
                    }
                }
                // The constraint reminder follows the tool results, as on the other passes.
                let mut user = Vec::new();
                if t.is_tool_result_turn() {
                    user.extend(ipr.reminder_text());
                }
                user.extend(texts);
                if !user.is_empty() {
                    messages.push(json!({"role": "user", "content": user.join("\n\n")}));
                }
            }
        }
    }
    let mut body = Map::new();
    body.insert("messages".into(), Value::Array(messages));
    body.insert("max_tokens".into(), json!(opts.max_tokens));
    if let Some(t) = opts.temperature {
        body.insert("temperature".into(), json!(t));
    }
    if !opts.tools.is_empty() {
        body.insert("tools".into(), Value::Array(opts.tools.iter().map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.input_schema}})).collect()));
    }
    Ok(Compiled { body: Value::Object(body), betas: Vec::new(), thinking_disabled_reason: (opts.thinking_budget.is_some()).then(|| "the compat pass has no thinking control; the request goes without it".to_string()), downgraded_reasoning_blocks: downgraded })
}

// ── Streaming ───────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
struct PartialCall {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
pub struct Assembler {
    text: String,
    reasoning: String,
    calls: BTreeMap<u64, PartialCall>,
    usage: Usage,
    stop_reason: Option<String>,
    finished: bool,
    done_marker: bool,
}

impl Assembler {
    pub fn new() -> Assembler {
        Assembler::default()
    }

    pub fn handle(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, StreamError> {
        let data = ev.data.trim();
        if data == "[DONE]" {
            self.done_marker = true;
            return Ok(Vec::new());
        }
        let v: Value = serde_json::from_str(data).map_err(|e| StreamError::Malformed(e.to_string()))?;
        if v["error"].is_object() {
            let e = &v["error"];
            let kind = e["type"].as_str().or_else(|| e["code"].as_str()).map(String::from).or_else(|| e["code"].as_u64().map(|c| c.to_string())).unwrap_or_else(|| "error".into());
            return Err(StreamError::Provider { kind, message: e["message"].as_str().unwrap_or("").to_string() });
        }
        let mut out = Vec::new();
        let u = &v["usage"];
        if u.is_object() {
            let prompt = u["prompt_tokens"].as_u64().unwrap_or(0);
            let cached = u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
            let completion = u["completion_tokens"].as_u64().unwrap_or(0);
            let reasoning = u["completion_tokens_details"]["reasoning_tokens"].as_u64().unwrap_or(0);
            self.usage = Usage { input_tokens: prompt.saturating_sub(cached), cache_read_tokens: cached, cache_write_tokens: 0, output_tokens: completion.saturating_sub(reasoning), thinking_tokens: reasoning };
        }
        if let Some(choice) = v["choices"].get(0) {
            let d = &choice["delta"];
            if let Some(r) = d["reasoning_content"].as_str().or_else(|| d["reasoning"].as_str()).filter(|s| !s.is_empty()) {
                self.reasoning.push_str(r);
                out.push(StreamEvent::ThinkingDelta { index: 0, text: r.to_string() });
            }
            if let Some(c) = d["content"].as_str().filter(|s| !s.is_empty()) {
                self.text.push_str(c);
                out.push(StreamEvent::TextDelta { index: 1, text: c.to_string() });
            }
            for tc in d["tool_calls"].as_array().into_iter().flatten() {
                let i = tc["index"].as_u64().unwrap_or(0);
                let p = self.calls.entry(i).or_default();
                if let Some(id) = tc["id"].as_str().filter(|s| !s.is_empty()) {
                    p.id = id.to_string();
                }
                if let Some(n) = tc["function"]["name"].as_str().filter(|s| !s.is_empty()) {
                    p.name = n.to_string();
                }
                if let Some(a) = tc["function"]["arguments"].as_str() {
                    p.args.push_str(a);
                }
            }
            if let Some(reason) = choice["finish_reason"].as_str() {
                self.finished = true;
                self.stop_reason = Some(match reason {
                    "stop" if !self.calls.is_empty() => "tool_use".into(),
                    "stop" => "end_turn".into(),
                    "tool_calls" | "function_call" => "tool_use".into(),
                    "length" => "max_tokens".into(),
                    other => other.to_string(),
                });
            }
        }
        Ok(out)
    }

    /// The finished turn. If the stream was cut short, the whole turn is discarded.
    pub fn finish(self) -> Result<Completed, StreamError> {
        if !self.finished {
            return Err(StreamError::Incomplete);
        }
        let mut blocks = Vec::new();
        if !self.reasoning.is_empty() {
            blocks.push(ContentBlock::Thinking { thought: self.reasoning, replayable: false, signature_dropped_reason: Some("compat gateway".into()), provider_meta: ProviderMeta { provider: Some(PROVIDER.into()), signature: None, data: None } });
        }
        if !self.text.is_empty() {
            blocks.push(ContentBlock::Text { text: self.text });
        }
        for (_, c) in self.calls {
            let arguments = if c.args.trim().is_empty() { json!({}) } else { serde_json::from_str(&c.args).map_err(|e| StreamError::Malformed(format!("tool arguments are not JSON: {}", e)))? };
            let id = if c.id.is_empty() { format!("call-{}", blocks.len()) } else { c.id };
            blocks.push(ContentBlock::ToolCall { id, name: c.name, arguments });
        }
        let _ = self.done_marker;
        Ok(Completed { blocks, usage: self.usage, stop_reason: self.stop_reason })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::tests::{ipr, opts};
    use crate::claude::SseDecoder;

    fn sse(chunks: &[Value]) -> String {
        let mut s: String = chunks.iter().map(|d| format!("data: {}\n\n", d)).collect();
        s.push_str("data: [DONE]\n\n");
        s
    }

    fn run(stream: &str) -> Result<Completed, StreamError> {
        let mut dec = SseDecoder::default();
        let mut asm = Assembler::new();
        for ev in dec.push(stream.as_bytes()) {
            asm.handle(&ev)?;
        }
        asm.finish()
    }

    #[test]
    fn a_streamed_turn_with_fragmented_tool_calls_and_trailing_usage() {
        let s = sse(&[
            json!({"choices": [{"index": 0, "delta": {"role": "assistant", "content": "Let me "}}]}),
            json!({"choices": [{"index": 0, "delta": {"content": "check."}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_1", "type": "function", "function": {"name": "read_file", "arguments": ""}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "{\"path\":"}}, {"index": 1, "id": "call_2", "function": {"name": "grep", "arguments": "{}"}}]}}]}),
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": " \"a.rs\"}"}}]}, "finish_reason": "tool_calls"}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 1000, "completion_tokens": 90, "prompt_tokens_details": {"cached_tokens": 600}, "completion_tokens_details": {"reasoning_tokens": 30}}}),
        ]);
        let done = run(&s).unwrap();
        assert_eq!(done.stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(done.blocks[0], ContentBlock::Text { text: "Let me check.".into() });
        assert_eq!(done.blocks[1], ContentBlock::ToolCall { id: "call_1".into(), name: "read_file".into(), arguments: json!({"path": "a.rs"}) });
        assert_eq!(done.blocks[2], ContentBlock::ToolCall { id: "call_2".into(), name: "grep".into(), arguments: json!({}) });
        assert_eq!((done.usage.input_tokens, done.usage.cache_read_tokens, done.usage.output_tokens, done.usage.thinking_tokens), (400, 600, 60, 30));
    }

    #[test]
    fn gateway_reasoning_is_shown_but_never_replayable() {
        let s = sse(&[json!({"choices": [{"delta": {"reasoning_content": "hmm"}}]}), json!({"choices": [{"delta": {"content": "ok"}, "finish_reason": "stop"}]})]);
        let done = run(&s).unwrap();
        assert!(matches!(&done.blocks[0], ContentBlock::Thinking { thought, replayable: false, .. } if thought == "hmm"));
        assert!(!done.blocks[0].is_signed_reasoning());
        assert_eq!(done.stop_reason.as_deref(), Some("end_turn"));
    }

    #[test]
    fn finish_reasons_errors_and_cut_streams() {
        let len = run(&sse(&[json!({"choices": [{"delta": {"content": "x"}, "finish_reason": "length"}]})])).unwrap();
        assert_eq!(len.stop_reason.as_deref(), Some("max_tokens"));
        let cut = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n";
        assert_eq!(run(cut).unwrap_err(), StreamError::Incomplete);
        let e = run(&sse(&[json!({"error": {"message": "busy", "type": "overloaded_error"}})])).unwrap_err();
        assert_eq!(e, StreamError::Provider { kind: "overloaded_error".into(), message: "busy".into() });
        assert!(matches!(run("data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c\",\"function\":{\"name\":\"f\",\"arguments\":\"{bad\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n"), Err(StreamError::Malformed(_))));
    }

    #[test]
    fn history_maps_to_tool_messages_and_drops_reasoning() {
        let asst = Turn::assistant(
            1,
            PROVIDER,
            "gw",
            "m",
            vec![
                ContentBlock::Thinking { thought: "secret plan".into(), replayable: true, signature_dropped_reason: None, provider_meta: ProviderMeta { provider: Some(PROVIDER.into()), signature: Some("SIG".into()), data: None } },
                ContentBlock::ToolCall { id: "call_1".into(), name: "read_file".into(), arguments: json!({"path": "a.rs"}) },
            ],
        );
        let results = Turn::user(2, vec![ContentBlock::ToolResult { tool_call_id: "call_1".into(), content: "boom".into(), is_error: true }]);
        let c = compile(&ipr(), &[asst, results], &opts(Some(2048))).unwrap();
        let m = c.body["messages"].as_array().unwrap();
        assert_eq!(m.iter().map(|x| x["role"].as_str().unwrap()).collect::<Vec<_>>(), ["system", "user", "assistant", "tool", "user"]);
        assert!(m[2]["content"].is_null());
        assert_eq!(m[2]["tool_calls"][0]["function"]["arguments"], "{\"path\":\"a.rs\"}");
        assert_eq!((m[3]["tool_call_id"].as_str(), m[3]["content"].as_str()), (Some("call_1"), Some("ERROR: boom")));
        assert!(m[4]["content"].as_str().unwrap().contains("Stay in scope."), "the reminder follows the tool results");
        let all = c.body.to_string();
        assert!(!all.contains("secret plan") && !all.contains("SIG"), "reasoning and signatures never cross a compat gateway");
        assert_eq!(c.downgraded_reasoning_blocks, 1);
        assert!(c.thinking_disabled_reason.is_some());
        assert_eq!(c.body["tools"][0]["function"]["name"], "read_file");
        assert!(m[0]["content"].as_str().unwrap().contains("governance"), "blocks 1-4 are one system message");
        assert!(!m[0]["content"].as_str().unwrap().contains("fn f<T>()"), "context slices stay in user content");
    }
}
