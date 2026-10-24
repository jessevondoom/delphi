//! The Gemini pass (`specs/002` §2.2, `specs/009` §3): IPR + turn history → a `generateContent` body, and
//! the `streamGenerateContent?alt=sse` response → the same typed `ContentBlock`s the Claude pass yields.
//!
//! Where Gemini differs from Claude, and how the canonical blocks absorb it:
//! * A `thoughtSignature` rides on a *text* or *functionCall* part rather than on a thinking block. The
//!   assembler emits it as a signed `Thinking` block (usually with an empty `thought`) immediately before
//!   the part that carried it; the compiler attaches it to the next text or call part when replaying.
//! * A thought *summary* (`thought: true`) is shown to the operator but never echoed back.
//! * Gemini 3 rejects (HTTP 400) a request whose open tool loop has a function-call step without a
//!   signature on its first call. Where a real signature is not replayable (a different endpoint or model
//!   produced the turn, or the model gave none) the documented placeholder is used and reported.
//! * Function calls arrive whole, and may lack an `id`; a synthetic `dg-` id is minted and never sent back.
//! * Thought tokens are billed as output but reported apart (`thoughtsTokenCount`).
//!
//! Field names follow Google's public REST reference. Items it does not pin down (whether
//! `maxOutputTokens` includes thought tokens, `parametersJsonSchema` support per model, the placeholder
//! signature's acceptance) are unverified until probed against a live endpoint (`specs/009` §6).

use crate::blocks::{ContentBlock, ProviderMeta, Role, Turn};
use crate::claude::{Completed, CompileError, CompileOptions, Compiled, SseEvent, StreamError, StreamEvent, Target, Usage};
use crate::ipr::{escape_section_tags, Ipr};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

pub const PROVIDER: &str = "google";
/// Google's documented placeholder for a function-call part the API did not sign.
pub const PLACEHOLDER_SIGNATURE: &str = "skip_thought_signature_validator";
const SYNTHETIC_ID_PREFIX: &str = "dg-";
const NOTE_MAX_CHARS: usize = 2000;

/// Gemini-only knobs.
#[derive(Debug, Clone, Default)]
pub struct GeminiOptions {
    /// Gemini 3 thinking level (`minimal`, `low`, `medium`, `high`). Wins over a numeric budget, which
    /// the API refuses to combine with a level.
    pub thinking_level: Option<String>,
}

fn valid_tool_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 128 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn text_part(text: String) -> Value {
    json!({"text": text})
}

fn reasoning_note(ipr: &Ipr, thought: &str) -> String {
    let mut t: String = thought.chars().take(NOTE_MAX_CHARS).collect();
    if thought.chars().count() > NOTE_MAX_CHARS {
        t.push('…');
    }
    format!("<prior_reasoning_note nonce=\"{}\">\n{}\n</prior_reasoning_note>", ipr.nonce.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect::<String>(), escape_section_tags(&t))
}

fn same_origin(t: &Turn, target: &Target) -> bool {
    t.provider.as_deref() == Some(target.provider.as_str()) && t.endpoint_id.as_deref() == Some(target.endpoint_id.as_str()) && t.model_id.as_deref() == Some(target.model_id.as_str())
}

fn with_signature(mut part: Value, sig: Option<String>) -> Value {
    if let Some(s) = sig {
        part["thoughtSignature"] = json!(s);
    }
    part
}

/// An assistant turn as `model` parts. With `echo`, real signatures are replayed on the part that
/// carried them; with `needs_first_call_signature`, the first call of the step gets the placeholder if no
/// real signature is available. Returns the parts and whether the placeholder was used.
fn model_parts(ipr: &Ipr, t: &Turn, echo: bool, needs_first_call_signature: bool, downgraded: &mut usize) -> (Vec<Value>, bool) {
    let mut out: Vec<Value> = Vec::new();
    let mut pending: Option<String> = None;
    let mut first_call_done = false;
    let mut placeholder = false;
    for b in &t.blocks {
        match b {
            ContentBlock::Thinking { thought, provider_meta, .. } if echo && b.is_signed_reasoning() => {
                if let Some(prev) = pending.replace(provider_meta.signature.clone().unwrap_or_default()) {
                    out.push(with_signature(text_part(String::new()), Some(prev))); // never lose a signature
                }
                let _ = thought; // the summary text is display-only
            }
            ContentBlock::Thinking { .. } if echo => {} // an unsigned summary is not replayed
            ContentBlock::Thinking { thought, .. } => {
                *downgraded += 1;
                if !thought.trim().is_empty() {
                    out.push(text_part(reasoning_note(ipr, thought)));
                }
            }
            ContentBlock::RedactedThinking { .. } => *downgraded += 1,
            ContentBlock::Text { text } => out.push(with_signature(text_part(text.clone()), pending.take())),
            ContentBlock::ToolCall { id, name, arguments } => {
                let mut call = json!({"name": name, "args": arguments});
                if !id.starts_with(SYNTHETIC_ID_PREFIX) {
                    call["id"] = json!(id);
                }
                let mut sig = pending.take();
                if !first_call_done && sig.is_none() && needs_first_call_signature {
                    sig = Some(PLACEHOLDER_SIGNATURE.to_string());
                    placeholder = true;
                }
                first_call_done = true;
                out.push(with_signature(json!({"functionCall": call}), sig));
            }
            ContentBlock::ToolResult { .. } => {}
        }
    }
    if let Some(sig) = pending {
        out.push(with_signature(text_part(String::new()), Some(sig)));
    }
    if out.is_empty() {
        out.push(text_part("(no visible output)".into()));
    }
    (out, placeholder)
}

fn user_parts(ipr: &Ipr, t: &Turn, names: &HashMap<String, String>) -> Vec<Value> {
    let mut results = Vec::new();
    let mut texts = Vec::new();
    for b in &t.blocks {
        match b {
            ContentBlock::ToolResult { tool_call_id, content, is_error } => {
                let name = names.get(tool_call_id).cloned().unwrap_or_else(|| "unknown_tool".into());
                let response = if *is_error { json!({"error": content}) } else { json!({"output": content}) };
                let mut fr = json!({"name": name, "response": response});
                if !tool_call_id.starts_with(SYNTHETIC_ID_PREFIX) {
                    fr["id"] = json!(tool_call_id);
                }
                results.push(json!({"functionResponse": fr}));
            }
            ContentBlock::Text { text } => texts.push(text_part(text.clone())),
            _ => {}
        }
    }
    let mut out = results;
    if t.is_tool_result_turn() {
        if let Some(r) = ipr.reminder_text() {
            out.push(text_part(r));
        }
    }
    out.extend(texts);
    out
}

fn push_content(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    if let Some(last) = contents.last_mut() {
        if last["role"] == role {
            last["parts"].as_array_mut().unwrap().extend(parts);
            return;
        }
    }
    contents.push(json!({"role": role, "parts": parts}));
}

pub fn compile(ipr: &Ipr, history: &[Turn], target: &Target, opts: &CompileOptions, g: &GeminiOptions) -> Result<Compiled, CompileError> {
    for t in &opts.tools {
        if !valid_tool_name(&t.name) {
            return Err(CompileError::BadToolName(t.name.clone()));
        }
    }
    let loop_start = history.iter().rposition(|t| t.role == Role::User && !t.is_tool_result_turn()).map(|i| i + 1).unwrap_or(0);
    let mut names: HashMap<String, String> = HashMap::new();
    for t in history {
        for b in &t.blocks {
            if let ContentBlock::ToolCall { id, name, .. } = b {
                names.insert(id.clone(), name.clone());
            }
        }
    }

    let mut downgraded = 0usize;
    let mut placeholders = 0usize;
    let mut contents: Vec<Value> = Vec::new();
    push_content(&mut contents, "user", vec![text_part(ipr.first_user_text())]);
    for (i, t) in history.iter().enumerate() {
        match t.role {
            Role::Assistant => {
                let (parts, used) = model_parts(ipr, t, same_origin(t, target), i >= loop_start, &mut downgraded);
                placeholders += used as usize;
                push_content(&mut contents, "model", parts);
            }
            Role::User => push_content(&mut contents, "user", user_parts(ipr, t, &names)),
        }
    }

    let mut body = Map::new();
    body.insert("systemInstruction".into(), json!({"parts": ipr.system_sections().into_iter().map(text_part).collect::<Vec<_>>()}));
    body.insert("contents".into(), Value::Array(contents));
    if !opts.tools.is_empty() {
        let decls: Vec<Value> = opts.tools.iter().map(|t| json!({"name": t.name, "description": t.description, "parametersJsonSchema": t.input_schema})).collect();
        body.insert("tools".into(), json!([{"functionDeclarations": decls}]));
    }
    let mut cfg = Map::new();
    cfg.insert("maxOutputTokens".into(), json!(opts.max_tokens));
    if let Some(t) = opts.temperature {
        cfg.insert("temperature".into(), json!(t));
    }
    if let Some(level) = &g.thinking_level {
        cfg.insert("thinkingConfig".into(), json!({"thinkingLevel": level, "includeThoughts": true}));
    } else if let Some(budget) = opts.thinking_budget {
        cfg.insert("thinkingConfig".into(), json!({"thinkingBudget": budget, "includeThoughts": true}));
    }
    body.insert("generationConfig".into(), Value::Object(cfg));

    let note = (placeholders > 0).then(|| format!("{} function-call step(s) of the open tool loop had no replayable signature for this endpoint and model; Google's documented placeholder signature was sent", placeholders));
    Ok(Compiled { body: Value::Object(body), betas: Vec::new(), thinking_disabled_reason: note, downgraded_reasoning_blocks: downgraded })
}

// ── Streaming ───────────────────────────────────────────────────────────────

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn synthetic_id() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}{:x}-{}", SYNTHETIC_ID_PREFIX, nanos, NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

fn meta(signature: Option<String>) -> ProviderMeta {
    ProviderMeta { provider: Some(PROVIDER.into()), signature, data: None }
}

#[derive(Debug)]
enum Open {
    Text { index: usize, text: String, sig: Option<String> },
    Thought { index: usize, text: String, sig: Option<String> },
}

/// Feeds on `streamGenerateContent` SSE events, one `GenerateContentResponse` each.
#[derive(Default)]
pub struct Assembler {
    open: Option<Open>,
    /// (index, sub-order, block): a signature block sorts just before the part it signs.
    done: Vec<(usize, u8, ContentBlock)>,
    next_index: usize,
    usage: Usage,
    stop_reason: Option<String>,
    saw_call: bool,
    finished: bool,
}

impl Assembler {
    pub fn new() -> Assembler {
        Assembler::default()
    }

    fn push(&mut self, index: usize, sub: u8, block: ContentBlock, out: &mut Vec<StreamEvent>) {
        self.done.push((index, sub, block.clone()));
        out.push(StreamEvent::BlockComplete { index, block });
    }

    fn close(&mut self, out: &mut Vec<StreamEvent>) {
        match self.open.take() {
            Some(Open::Text { index, text, sig }) => {
                if let Some(s) = sig {
                    self.push(index, 0, ContentBlock::Thinking { thought: String::new(), replayable: true, signature_dropped_reason: None, provider_meta: meta(Some(s)) }, out);
                }
                if !text.is_empty() {
                    self.push(index, 1, ContentBlock::Text { text }, out);
                }
            }
            Some(Open::Thought { index, text, sig }) if !text.is_empty() || sig.is_some() => {
                self.push(index, 0, ContentBlock::Thinking { thought: text, replayable: true, signature_dropped_reason: None, provider_meta: meta(sig) }, out);
            }
            Some(Open::Thought { .. }) | None => {}
        }
    }

    fn part(&mut self, p: &Value, out: &mut Vec<StreamEvent>) {
        let sig = p["thoughtSignature"].as_str().filter(|s| !s.is_empty()).map(String::from);
        if let Some(call) = p.get("functionCall").filter(|c| c.is_object()) {
            self.close(out);
            let index = self.next_index;
            self.next_index += 1;
            if let Some(s) = sig {
                self.push(index, 0, ContentBlock::Thinking { thought: String::new(), replayable: true, signature_dropped_reason: None, provider_meta: meta(Some(s)) }, out);
            }
            let id = call["id"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(synthetic_id);
            let arguments = if call["args"].is_object() { call["args"].clone() } else { json!({}) };
            self.saw_call = true;
            self.push(index, 1, ContentBlock::ToolCall { id, name: call["name"].as_str().unwrap_or("").to_string(), arguments }, out);
            return;
        }
        let Some(text) = p["text"].as_str() else {
            // Another part kind (code execution, files): not part of the canonical block set. A signature
            // on it is kept so the next text or call part cannot lose it.
            if let Some(s) = sig {
                self.close(out);
                let index = self.next_index;
                self.next_index += 1;
                self.open = Some(Open::Text { index, text: String::new(), sig: Some(s) });
            }
            return;
        };
        let thought = p["thought"].as_bool().unwrap_or(false);
        let same_kind = matches!((&self.open, thought), (Some(Open::Thought { .. }), true) | (Some(Open::Text { .. }), false));
        if !same_kind {
            self.close(out);
            let index = self.next_index;
            self.next_index += 1;
            self.open = Some(if thought { Open::Thought { index, text: String::new(), sig: None } } else { Open::Text { index, text: String::new(), sig: None } });
        }
        match self.open.as_mut().unwrap() {
            Open::Thought { index, text: buf, sig: slot } => {
                buf.push_str(text);
                if sig.is_some() {
                    *slot = sig;
                }
                if !text.is_empty() {
                    out.push(StreamEvent::ThinkingDelta { index: *index, text: text.to_string() });
                }
            }
            Open::Text { index, text: buf, sig: slot } => {
                buf.push_str(text);
                if sig.is_some() {
                    *slot = sig;
                }
                if !text.is_empty() {
                    out.push(StreamEvent::TextDelta { index: *index, text: text.to_string() });
                }
            }
        }
    }

    pub fn handle(&mut self, ev: &SseEvent) -> Result<Vec<StreamEvent>, StreamError> {
        let v: Value = serde_json::from_str(&ev.data).map_err(|e| StreamError::Malformed(e.to_string()))?;
        if v["error"].is_object() {
            let e = &v["error"];
            let kind = e["status"].as_str().map(String::from).or_else(|| e["code"].as_u64().map(|c| c.to_string())).unwrap_or_else(|| "error".into());
            return Err(StreamError::Provider { kind, message: e["message"].as_str().unwrap_or("").to_string() });
        }
        let mut out = Vec::new();
        if let Some(reason) = v["promptFeedback"]["blockReason"].as_str() {
            return Err(StreamError::Provider { kind: "blocked".into(), message: format!("the prompt was blocked ({})", reason) });
        }
        let u = &v["usageMetadata"];
        if u.is_object() {
            let prompt = u["promptTokenCount"].as_u64().unwrap_or(0);
            let cached = u["cachedContentTokenCount"].as_u64().unwrap_or(0);
            // Cumulative on every chunk: take the latest, never add.
            self.usage.cache_read_tokens = cached;
            self.usage.input_tokens = prompt.saturating_sub(cached) + u["toolUsePromptTokenCount"].as_u64().unwrap_or(0);
            self.usage.output_tokens = u["candidatesTokenCount"].as_u64().unwrap_or(0);
            self.usage.thinking_tokens = u["thoughtsTokenCount"].as_u64().unwrap_or(0);
        }
        if let Some(cand) = v["candidates"].get(0) {
            if let Some(parts) = cand["content"]["parts"].as_array() {
                for p in parts {
                    self.part(p, &mut out);
                }
            }
            if let Some(reason) = cand["finishReason"].as_str().filter(|r| *r != "FINISH_REASON_UNSPECIFIED") {
                self.close(&mut out);
                self.finished = true;
                self.stop_reason = Some(match reason {
                    "STOP" if self.saw_call => "tool_use".into(),
                    "STOP" => "end_turn".into(),
                    "MAX_TOKENS" => "max_tokens".into(),
                    other => other.to_ascii_lowercase(),
                });
            }
        }
        Ok(out)
    }

    /// The finished turn. If the stream was cut short, the whole turn is discarded.
    pub fn finish(mut self) -> Result<Completed, StreamError> {
        let mut ignored = Vec::new();
        self.close(&mut ignored);
        if !self.finished {
            return Err(StreamError::Incomplete);
        }
        self.done.sort_by_key(|(i, s, _)| (*i, *s));
        Ok(Completed { blocks: self.done.into_iter().map(|(_, _, b)| b).collect(), usage: self.usage, stop_reason: self.stop_reason })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::tests::{ipr, opts};
    use crate::claude::{SseDecoder, ToolDef};

    fn target(endpoint: &str, model: &str) -> Target {
        Target { provider: PROVIDER.into(), endpoint_id: endpoint.into(), model_id: model.into() }
    }

    fn sse(chunks: &[Value]) -> String {
        chunks.iter().map(|d| format!("data: {}\r\n\r\n", d)).collect()
    }

    fn run(stream: &str) -> Result<Completed, StreamError> {
        let mut dec = SseDecoder::default();
        let mut asm = Assembler::new();
        for ev in dec.push(stream.as_bytes()) {
            asm.handle(&ev)?;
        }
        asm.finish()
    }

    fn usage(prompt: u64, cached: u64, out: u64, thoughts: u64) -> Value {
        json!({"promptTokenCount": prompt, "cachedContentTokenCount": cached, "candidatesTokenCount": out, "thoughtsTokenCount": thoughts, "totalTokenCount": prompt + out + thoughts})
    }

    /// A thought summary, text, then one call carrying the signature, as Gemini 3 streams it.
    fn call_stream() -> String {
        sse(&[
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "Let me ", "thought": true}]}}], "usageMetadata": usage(1000, 0, 0, 5)}),
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "look.", "thought": true}, {"text": "Reading "}]}}]}),
            json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "the file."}, {"functionCall": {"name": "read_file", "args": {"path": "a.rs"}}, "thoughtSignature": "SIG-A"}]}, "finishReason": "STOP"}], "usageMetadata": usage(1200, 800, 40, 60)}),
        ])
    }

    #[test]
    fn a_streamed_turn_becomes_blocks_with_the_signature_before_its_call() {
        let done = run(&call_stream()).unwrap();
        assert_eq!(done.stop_reason.as_deref(), Some("tool_use"));
        let kinds: Vec<&str> = done.blocks.iter().map(|b| match b {
            ContentBlock::Thinking { provider_meta, .. } if provider_meta.signature.is_some() => "sig",
            ContentBlock::Thinking { .. } => "summary",
            ContentBlock::Text { .. } => "text",
            ContentBlock::ToolCall { .. } => "call",
            _ => "other",
        }).collect();
        assert_eq!(kinds, ["summary", "text", "sig", "call"]);
        match &done.blocks[0] {
            ContentBlock::Thinking { thought, .. } => assert_eq!(thought, "Let me look."),
            b => panic!("{:?}", b),
        }
        match &done.blocks[3] {
            ContentBlock::ToolCall { id, arguments, .. } => {
                assert!(id.starts_with(SYNTHETIC_ID_PREFIX));
                assert_eq!(arguments, &json!({"path": "a.rs"}));
            }
            b => panic!("{:?}", b),
        }
    }

    #[test]
    fn usage_is_the_latest_cumulative_report_with_thoughts_apart() {
        let u = run(&call_stream()).unwrap().usage;
        assert_eq!((u.input_tokens, u.cache_read_tokens, u.output_tokens, u.thinking_tokens), (400, 800, 40, 60));
    }

    #[test]
    fn a_signature_on_a_trailing_empty_text_part_is_kept() {
        let s = sse(&[
            json!({"candidates": [{"content": {"parts": [{"text": "Done."}]}}]}),
            json!({"candidates": [{"content": {"parts": [{"text": "", "thoughtSignature": "SIG-Z"}]}, "finishReason": "STOP"}]}),
        ]);
        let done = run(&s).unwrap();
        assert_eq!(done.stop_reason.as_deref(), Some("end_turn"));
        assert!(matches!(&done.blocks[0], ContentBlock::Thinking { provider_meta, .. } if provider_meta.signature.as_deref() == Some("SIG-Z")));
        assert!(matches!(&done.blocks[1], ContentBlock::Text { text } if text == "Done."));
    }

    #[test]
    fn parallel_calls_keep_their_order_and_a_given_id() {
        let s = sse(&[json!({"candidates": [{"content": {"parts": [
            {"functionCall": {"name": "a", "args": {}, "id": "call-1"}, "thoughtSignature": "S"},
            {"functionCall": {"name": "b", "args": {"x": 1}}},
        ]}, "finishReason": "STOP"}]})]);
        let done = run(&s).unwrap();
        let calls: Vec<(&str, &str)> = done.blocks.iter().filter_map(|b| if let ContentBlock::ToolCall { id, name, .. } = b { Some((id.as_str(), name.as_str())) } else { None }).collect();
        assert_eq!(calls[0], ("call-1", "a"));
        assert!(calls[1].0.starts_with("dg-") && calls[1].1 == "b" && calls[0].0 != calls[1].0);
    }

    #[test]
    fn finish_reasons_map_and_a_cut_stream_is_discarded() {
        let max = run(&sse(&[json!({"candidates": [{"content": {"parts": [{"text": "x"}]}, "finishReason": "MAX_TOKENS"}]})])).unwrap();
        assert_eq!(max.stop_reason.as_deref(), Some("max_tokens"));
        let odd = run(&sse(&[json!({"candidates": [{"content": {"parts": []}, "finishReason": "MISSING_THOUGHT_SIGNATURE"}]})])).unwrap();
        assert_eq!(odd.stop_reason.as_deref(), Some("missing_thought_signature"));
        assert_eq!(run(&sse(&[json!({"candidates": [{"content": {"parts": [{"text": "x"}]}}]})])).unwrap_err(), StreamError::Incomplete);
    }

    #[test]
    fn errors_and_blocked_prompts_are_provider_errors() {
        let e = run(&sse(&[json!({"error": {"code": 429, "message": "slow down", "status": "RESOURCE_EXHAUSTED"}})])).unwrap_err();
        assert_eq!(e, StreamError::Provider { kind: "RESOURCE_EXHAUSTED".into(), message: "slow down".into() });
        let b = run(&sse(&[json!({"promptFeedback": {"blockReason": "SAFETY"}})])).unwrap_err();
        assert!(matches!(b, StreamError::Provider { ref kind, .. } if kind == "blocked"));
    }

    // ── Compile ──

    fn assistant_from(stream: &str, endpoint: &str, model: &str) -> Turn {
        Turn::assistant(1, PROVIDER, endpoint, model, run(stream).unwrap().blocks)
    }

    fn results_turn(id: &str) -> Turn {
        Turn::user(2, vec![ContentBlock::ToolResult { tool_call_id: id.into(), content: "fn main() {}".into(), is_error: false }])
    }

    fn call_id(t: &Turn) -> String {
        t.blocks.iter().find_map(|b| if let ContentBlock::ToolCall { id, .. } = b { Some(id.clone()) } else { None }).unwrap()
    }

    fn compile_for(history: &[Turn], t: &Target, o: &CompileOptions, g: &GeminiOptions) -> Compiled {
        compile(&ipr(), history, t, o, g).unwrap()
    }

    #[test]
    fn the_request_core_has_the_gemini_shape() {
        let c = compile_for(&[], &target("direct", "g"), &opts(Some(2048)), &GeminiOptions::default());
        let b = &c.body;
        assert_eq!(b["contents"][0]["role"], "user");
        assert!(b["systemInstruction"]["parts"].as_array().unwrap().len() >= 2);
        assert_eq!(b["generationConfig"]["maxOutputTokens"], 8000);
        assert_eq!(b["generationConfig"]["thinkingConfig"], json!({"thinkingBudget": 2048, "includeThoughts": true}));
        assert_eq!(b["tools"][0]["functionDeclarations"][0]["name"], "read_file");
        assert!(b.get("model").is_none() && b.get("stream").is_none(), "the platform adds the model to the URL");
        let l = compile_for(&[], &target("direct", "g"), &opts(Some(2048)), &GeminiOptions { thinking_level: Some("low".into()) });
        assert_eq!(l.body["generationConfig"]["thinkingConfig"], json!({"thinkingLevel": "low", "includeThoughts": true}), "a level replaces the budget");
    }

    #[test]
    fn a_replayed_open_loop_carries_the_real_signature_on_its_call() {
        let asst = assistant_from(&call_stream(), "direct", "g");
        let id = call_id(&asst);
        let c = compile_for(&[asst, results_turn(&id)], &target("direct", "g"), &opts(Some(2048)), &GeminiOptions::default());
        let contents = c.body["contents"].as_array().unwrap();
        assert_eq!(contents.iter().map(|m| m["role"].as_str().unwrap()).collect::<Vec<_>>(), ["user", "model", "user"]);
        let model = contents[1]["parts"].as_array().unwrap();
        let call = model.iter().find(|p| p.get("functionCall").is_some()).unwrap();
        assert_eq!(call["thoughtSignature"], "SIG-A");
        assert!(call["functionCall"].get("id").is_none(), "a synthetic id is never sent back");
        assert!(model.iter().all(|p| p["text"].as_str() != Some("Let me look.")), "thought summaries are not echoed");
        let fr = &contents[2]["parts"][0]["functionResponse"];
        assert_eq!((fr["name"].as_str(), fr["response"]["output"].as_str()), (Some("read_file"), Some("fn main() {}")));
        assert!(fr.get("id").is_none());
        assert!(c.thinking_disabled_reason.is_none());
    }

    #[test]
    fn a_turn_from_another_model_gets_a_note_and_the_placeholder_signature() {
        let asst = assistant_from(&call_stream(), "other-endpoint", "g-old");
        let id = call_id(&asst);
        let c = compile_for(&[asst, results_turn(&id)], &target("direct", "g"), &opts(Some(2048)), &GeminiOptions::default());
        let model = c.body["contents"][1]["parts"].as_array().unwrap();
        let call = model.iter().find(|p| p.get("functionCall").is_some()).unwrap();
        assert_eq!(call["thoughtSignature"], PLACEHOLDER_SIGNATURE);
        assert!(model.iter().any(|p| p["text"].as_str().is_some_and(|t| t.contains("prior_reasoning_note"))));
        assert!(!serde_json::to_string(&c.body).unwrap().contains("SIG-A"), "a foreign signature is never replayed");
        assert!(c.thinking_disabled_reason.unwrap().contains("placeholder"));
        assert!(c.downgraded_reasoning_blocks >= 1);
    }

    #[test]
    fn given_ids_are_echoed_and_errors_use_the_error_key() {
        let s = sse(&[json!({"candidates": [{"content": {"parts": [{"functionCall": {"name": "run", "args": {}, "id": "abc"}, "thoughtSignature": "S"}]}, "finishReason": "STOP"}]})]);
        let asst = assistant_from(&s, "direct", "g");
        let fail = Turn::user(2, vec![ContentBlock::ToolResult { tool_call_id: "abc".into(), content: "denied".into(), is_error: true }]);
        let c = compile_for(&[asst, fail], &target("direct", "g"), &opts(None), &GeminiOptions::default());
        let contents = c.body["contents"].as_array().unwrap();
        assert_eq!(contents[1]["parts"][0]["functionCall"]["id"], "abc");
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["id"], "abc");
        assert_eq!(contents[2]["parts"][0]["functionResponse"]["response"], json!({"error": "denied"}));
        assert!(c.body["generationConfig"].get("thinkingConfig").is_none());
    }

    #[test]
    fn bad_tool_names_are_refused() {
        let mut o = opts(None);
        o.tools.push(ToolDef { name: "bad name".into(), description: String::new(), input_schema: json!({}) });
        assert_eq!(compile(&ipr(), &[], &target("d", "g"), &o, &GeminiOptions::default()).unwrap_err(), CompileError::BadToolName("bad name".into()));
    }
}
