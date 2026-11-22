//! A validated JSON-in-text answer for a carrier with no schema flag (`specs/132` §6.1, slice C5).
//!
//! The extractor is deterministic and the candidate is validated against the span's schema: the model's words cannot mark
//! themselves structured (`specs/132` §9 item 2). Two candidates or none is "not found"; nothing is guessed.

use serde_json::Value;

/// What the reply held.
#[derive(Debug, PartialEq)]
pub(crate) enum Candidate {
    /// Exactly one candidate, as text (not yet parsed).
    One(String),
    /// No candidate at all.
    None,
    /// Two or more candidates: which one was meant is not guessed.
    Several,
}

/// The candidate in `reply`: the whole reply if it parses; else the content of exactly one fenced block (```` ```json ```` or a bare
/// ```` ``` ````); else the single top-level `{...}` span when there is exactly one.
pub(crate) fn extract(reply: &str) -> Candidate {
    let whole = reply.trim();
    if !whole.is_empty() && serde_json::from_str::<Value>(whole).is_ok() {
        return Candidate::One(whole.to_string());
    }
    let fences = fenced_blocks(reply);
    if !fences.is_empty() {
        return if fences.len() == 1 { Candidate::One(fences[0].trim().to_string()) } else { Candidate::Several };
    }
    let spans = brace_spans(reply);
    match spans.len() {
        0 => Candidate::None,
        1 => Candidate::One(spans[0].to_string()),
        _ => Candidate::Several,
    }
}

/// The contents of the fenced blocks: a line that starts with ```` ``` ```` (an optional info word after it) opens one and the next
/// such line closes it. An unclosed fence counts as a block, so a cut reply is never read past its fence.
fn fenced_blocks(reply: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut open: Option<Vec<&str>> = None;
    for line in reply.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            match open.take() {
                Some(lines) => blocks.push(lines.join("\n")),
                None => open = Some(Vec::new()),
            }
        } else if let Some(lines) = open.as_mut() {
            lines.push(line);
        }
    }
    if let Some(lines) = open {
        blocks.push(lines.join("\n"));
    }
    blocks
}

/// The balanced top-level `{...}` spans of `reply`. Inside a span, braces within a JSON string do not count; outside one, quotes
/// are prose. An unbalanced `{` makes no span.
fn brace_spans(reply: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let (mut depth, mut start, mut in_str, mut esc) = (0usize, 0usize, false, false);
    for (i, c) in reply.char_indices() {
        if depth > 0 && in_str {
            match (esc, c) {
                (true, _) => esc = false,
                (false, '\\') => esc = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '{' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    spans.push(&reply[start..=i]);
                }
            }
            '"' if depth > 0 => in_str = true,
            _ => {}
        }
    }
    spans
}

/// Validate `value` against `schema` (a JSON Schema as text). The error names the first failing place and the schema keyword
/// (`at /findings/0/severity (enum)`), never the value itself, so no reply text travels in it.
pub(crate) fn validate(schema: &str, value: &Value) -> Result<(), String> {
    let schema: Value = serde_json::from_str(schema).map_err(|_| "the span's schema is not JSON".to_string())?;
    let validator = jsonschema::validator_for(&schema).map_err(|_| "the span's schema does not compile".to_string())?;
    match validator.iter_errors(value).next() {
        None => Ok(()),
        Some(e) => {
            let at = e.instance_path.to_string();
            let schema_path = e.schema_path.to_string();
            let keyword = schema_path.rsplit('/').next().unwrap_or("");
            Err(format!("at {} ({})", if at.is_empty() { "the top level" } else { at.as_str() }, keyword))
        }
    }
}

/// The structured answer in `reply`, or why there is none (no reply text in the reason).
pub(crate) fn answer(schema: &str, reply: &str) -> Result<Value, String> {
    let text = match extract(reply) {
        Candidate::One(t) => t,
        Candidate::None => return Err("no JSON object was found".into()),
        Candidate::Several => return Err("more than one candidate was found".into()),
    };
    let value: Value = serde_json::from_str(&text).map_err(|_| "the candidate is not valid JSON".to_string())?;
    validate(schema, &value)?;
    Ok(value)
}

/// The fixed message of the one retry (`specs/132` §6.1 item 4): the reason, never the reply.
pub(crate) fn retry_message(why: &str) -> String {
    format!("Your answer was not one JSON object matching the schema: {}. Reply with only that object.", why)
}

/// The prompt's part of the contract: one JSON object matching the schema, and nothing else.
pub(crate) fn contract(schema: &str) -> String {
    format!("<json_answer>Reply with one JSON object that matches this JSON Schema, and nothing else: no prose before or after it, no second object.\n{}</json_answer>", schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SCHEMA: &str = r#"{"type":"object","properties":{"word":{"type":"string","enum":["yes","no"]}},"required":["word"]}"#;

    /// `specs/132` §11 item 9: the extractor's table.
    #[test]
    fn the_extractor_accepts_one_candidate_and_nothing_else() {
        let yes = json!({"word": "yes"});
        let accepted = [
            ("bare JSON", "{\"word\":\"yes\"}", yes.clone()),
            ("a json fence (the P0 reply)", "```json\n{\"word\":\"yes\"}\n```", yes.clone()),
            ("a bare fence", "```\n{\"word\":\"yes\"}\n```", yes.clone()),
            ("prose around one object", "Here it is: {\"word\":\"yes\"} as asked.", yes.clone()),
            ("a brace inside a string", "Answer: {\"word\":\"yes\",\"note\":\"a } here\"}", json!({"word": "yes", "note": "a } here"})),
        ];
        for (name, reply, want) in accepted {
            assert_eq!(answer(SCHEMA, reply), Ok(want), "{name}");
        }
        let refused = [
            ("two fences", "```json\n{\"word\":\"yes\"}\n```\nor\n```json\n{\"word\":\"no\"}\n```", "more than one"),
            ("two objects", "{\"word\":\"yes\"} or {\"word\":\"no\"}", "more than one"),
            ("none", "I would say yes.", "no JSON object"),
            ("empty", "   ", "no JSON object"),
            ("invalid against the schema", "{\"word\":\"maybe\"}", "at /word (enum)"),
            ("a required field missing", "{\"other\":1}", "at the top level (required)"),
            ("a fence that is not JSON", "```json\n{word: yes}\n```", "not valid JSON"),
            ("the schema's own text with the answer", &format!("Schema: {SCHEMA}\nAnswer: {{\"word\":\"yes\"}}"), "more than one"),
            ("the schema's own text alone", SCHEMA, "at the top level (required)"),
        ];
        for (name, reply, why) in refused {
            let e = answer(SCHEMA, reply).expect_err(name);
            assert!(e.contains(why), "{name}: {e}");
        }
    }

    #[test]
    fn the_reason_never_carries_the_reply() {
        let e = answer(SCHEMA, "{\"word\":\"SECRET-WORD-123\"}").unwrap_err();
        assert!(!e.contains("SECRET-WORD-123"), "{e}");
        let m = retry_message(&e);
        assert!(m.starts_with("Your answer was not one JSON object matching the schema: at /word") && m.ends_with("Reply with only that object."), "{m}");
    }

    #[test]
    fn quotes_in_prose_do_not_hide_an_object() {
        assert_eq!(extract("Don't \"quote\" me: {\"word\":\"no\"}"), Candidate::One("{\"word\":\"no\"}".into()));
        assert_eq!(extract("an unclosed { brace"), Candidate::None);
    }
}
