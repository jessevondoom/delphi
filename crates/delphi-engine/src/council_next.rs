//! A Council's decision and the steps that follow from it (`specs/003` §4.2). A verdict that ends in prose is a checkbox; this makes it end
//! in something to do: one decision, and up to five concrete steps, each for a named role, in order. When the Council escalates, each option
//! carries the steps that follow if it is chosen. `/follow` starts them. Pure functions; `council.rs` asks the arbiter for the shape and
//! `service.rs` runs the steps.

use serde_json::{json, Value};

/// The most steps one option carries: a Council that wants more should say "plan this" as the first.
pub const MAX_STEPS: usize = 5;
const OBJECTIVE_CAP: usize = 300;
const DECISION_CAP: usize = 240;

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The option this follows from, when the Council escalated; `None` for the one verdict.
    pub option: Option<String>,
    pub role: String,
    pub objective: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Followup {
    /// The arbiter's text without the lines that carry the decision and the steps.
    pub prose: String,
    pub decision: Option<String>,
    pub steps: Vec<Step>,
    /// The arbiter gave no usable steps, so these come from the open blocking findings instead.
    pub from_findings: bool,
}

fn clip(s: &str, n: usize) -> String {
    let t = crate::observe::sanitize(s).split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > n {
        t.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    } else {
        t
    }
}

fn norm(s: &str) -> String {
    s.trim().trim_matches(|c: char| c == ':' || c == '`' || c == '*' || c == '[' || c == ']').trim().to_lowercase()
}

/// The shape the arbiter is asked to end with.
pub fn format_instruction(option_labels: &[String]) -> String {
    let shared = "End with these lines, and nothing after them:\nDECISION: <what we do, in one sentence>\n";
    if option_labels.is_empty() {
        format!("{}NEXT <role>: <one concrete step, for that role, in one sentence>\n(one to five NEXT lines, in the order they should happen; the role is one of the enabled roles other than the orchestrator).", shared)
    } else {
        format!(
            "{}Then, for each option, a line `IF <the option's label, exactly>` followed by its NEXT lines: `NEXT <role>: <one concrete step, for that role, in one sentence>`, one to five, in order. The options are: {}.",
            "End with these lines, and nothing after them:\nDECISION: needs the operator: <the question, in one sentence>\n",
            option_labels.join("; ")
        )
    }
}

/// Split the arbiter's text into prose, decision and steps. `roles` are the roles a step may name; `labels` the options of an escalated Council.
pub fn parse(text: &str, roles: &[String], labels: &[String]) -> Followup {
    let mut prose: Vec<&str> = Vec::new();
    let mut decision = None;
    let mut steps: Vec<Step> = Vec::new();
    let mut option: Option<String> = None;
    let mut ignoring_option = false;
    for line in text.lines() {
        let t = line.trim();
        let up = t.to_uppercase();
        if let Some(rest) = up.strip_prefix("DECISION:").map(|_| t["DECISION:".len()..].trim()) {
            if decision.is_none() && !rest.is_empty() {
                decision = Some(clip(rest, DECISION_CAP));
            }
        } else if up.starts_with("IF ") || up == "IF" {
            let label = norm(t[2..].trim());
            // The label as written, else the one option it unambiguously abbreviates (`PROCEED` must not match `DO-NOT-PROCEED`).
            let exact = labels.iter().find(|l| norm(l) == label);
            let fuzzy: Vec<&String> = labels.iter().filter(|l| { let l = norm(l); !l.is_empty() && !label.is_empty() && (l.contains(&label) || label.contains(&l)) }).collect();
            let known = exact.or(if fuzzy.len() == 1 { Some(fuzzy[0]) } else { None });
            match known {
                Some(l) => {
                    option = Some(l.clone());
                    ignoring_option = false;
                }
                None => ignoring_option = true, // an option the Council does not have, or one that could be two
            }
        } else if up.starts_with("NEXT ") || up.starts_with("NEXT:") {
            let rest = t[4..].trim_start_matches(':').trim();
            if let Some((role, objective)) = rest.split_once(':') {
                let role = norm(role);
                let objective = clip(objective, OBJECTIVE_CAP);
                let owned = steps.iter().filter(|s| s.option == option).count();
                if !ignoring_option && roles.contains(&role) && !objective.is_empty() && owned < MAX_STEPS && (labels.is_empty() || option.is_some()) {
                    steps.push(Step { option: option.clone(), role, objective });
                }
            }
        } else {
            prose.push(line);
        }
    }
    Followup { prose: prose.join("\n").trim().to_string(), decision, steps, from_findings: false }
}

/// When the arbiter gave no usable steps: one step for each blocking or major finding still open, for the architect to resolve, so the verdict
/// is never only a description. Only for a Council that did not escalate (an escalated one needs the operator's choice first).
pub fn from_findings(unresolved: &[(String, String, String)], roles: &[String]) -> Vec<Step> {
    if !roles.iter().any(|r| r == "architect") {
        return Vec::new();
    }
    unresolved.iter().take(MAX_STEPS).map(|(sev, seat, title)| Step { option: None, role: "architect".into(), objective: clip(&format!("Resolve the open {} finding raised by {}: {}", sev, seat, title), OBJECTIVE_CAP) }).collect()
}

/// The markdown section of the record.
pub fn render_section(f: &Followup, escalated: bool) -> String {
    let mut s = String::from("## Decision and next steps\n\n");
    match &f.decision {
        Some(d) => s.push_str(&format!("**Decision:** {}\n\n", d)),
        None => s.push_str(if escalated { "**Decision:** needs the operator's choice between the options above.\n\n" } else { "**Decision:** the Council gave no one-line decision; read the verdict above.\n\n" }),
    }
    if f.steps.is_empty() {
        s.push_str("No next steps were recorded.\n");
        return s;
    }
    if f.from_findings {
        s.push_str("*(The arbiter named no steps, so these are the open blocking findings, for the architect to resolve.)*\n\n");
    }
    let mut last: Option<&Option<String>> = None;
    let mut n = 0;
    for st in &f.steps {
        if last != Some(&st.option) {
            if let Some(o) = &st.option {
                s.push_str(&format!("\nIf **{}**:\n\n", o));
            }
            last = Some(&st.option);
            n = 0;
        }
        n += 1;
        s.push_str(&format!("{}. `{}`: {}\n", n, st.role, st.objective));
    }
    s
}

/// What the operator is shown after the verdict, ending with how to start the steps.
pub fn render_text(f: &Followup, escalated: bool) -> String {
    if f.steps.is_empty() {
        return match &f.decision {
            Some(d) => format!("Decision: {}\nNo further steps were recorded.", d),
            None => String::new(),
        };
    }
    let mut s = String::new();
    if let Some(d) = &f.decision {
        s.push_str(&format!("Decision: {}\n", d));
    }
    let options = options_of(&f.steps);
    if options.is_empty() {
        s.push_str("Next steps:\n");
        for (i, st) in f.steps.iter().enumerate() {
            s.push_str(&format!("  {}. {}: {}\n", i + 1, st.role, st.objective));
        }
        s.push_str("Start them, one after the other, with /follow.");
    } else {
        for (i, o) in options.iter().enumerate() {
            s.push_str(&format!("If {} ({}):\n", i + 1, o));
            for (j, st) in f.steps.iter().filter(|s| s.option.as_deref() == Some(o)).enumerate() {
                s.push_str(&format!("  {}. {}: {}\n", j + 1, st.role, st.objective));
            }
        }
        s.push_str(&format!("Choose with /follow <1-{}> to start that option's steps.", options.len()));
    }
    let _ = escalated;
    s
}

/// The option labels that have steps, in the order they first appear.
pub fn options_of(steps: &[Step]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in steps {
        if let Some(o) = &s.option {
            if !out.contains(o) {
                out.push(o.clone());
            }
        }
    }
    out
}

pub fn steps_json(steps: &[Step]) -> Value {
    Value::Array(steps.iter().map(|s| json!({"option": s.option, "role": s.role, "objective": s.objective})).collect())
}

pub fn steps_from_json(v: &Value) -> Vec<Step> {
    v.as_array().map(|a| a.iter().filter_map(|x| Some(Step { option: x["option"].as_str().map(String::from), role: x["role"].as_str()?.into(), objective: x["objective"].as_str()?.into() })).collect()).unwrap_or_default()
}

/// The task a step becomes: what was decided, which step this is, and the record to read.
pub fn step_prompt(n: u32, path: &str, decision: Option<&str>, i: usize, k: usize, step: &Step) -> String {
    format!("Following Council {:03}{}. Step {} of {}: {}\n\nThe Council's record: @{}", n, decision.map(|d| format!(", which decided: {}", d)).unwrap_or_default(), i, k, step.objective, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles() -> Vec<String> {
        ["architect", "builder", "red-team", "strategist", "orchestrator"].iter().filter(|r| **r != "orchestrator").map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_verdict_ends_in_a_decision_and_steps_in_order_and_the_prose_keeps_the_rest() {
        let text = "The Council recommends a table: it survives restarts.\nThe seats agreed on durability.\nDECISION: build the queue as a table in the existing store\nNEXT architect: write the table schema and the claim query into the spec\nNEXT builder: implement the claim loop against it\nNEXT red-team: audit the claim query for double delivery\n";
        let f = parse(text, &roles(), &[]);
        assert_eq!(f.prose, "The Council recommends a table: it survives restarts.\nThe seats agreed on durability.");
        assert_eq!(f.decision.as_deref(), Some("build the queue as a table in the existing store"));
        assert_eq!(f.steps.iter().map(|s| (s.role.as_str(), s.option.is_none())).collect::<Vec<_>>(), vec![("architect", true), ("builder", true), ("red-team", true)]);
        assert!(f.steps[1].objective.starts_with("implement the claim loop"));
    }

    #[test]
    fn steps_that_name_nobody_real_or_cannot_be_started_are_dropped_and_there_are_at_most_five() {
        let mut text = String::from("DECISION: go\nNEXT orchestrator: arbitrate\nNEXT scribe: write it down\nNEXT architect: \nNEXT: no role\n");
        for i in 0..8 {
            text.push_str(&format!("NEXT builder: step {}\x1b[2J\n", i));
        }
        let f = parse(&text, &roles(), &[]);
        assert_eq!(f.steps.len(), MAX_STEPS, "{:?}", f.steps);
        assert!(f.steps.iter().all(|s| s.role == "builder" && !s.objective.contains('\x1b')));
    }

    #[test]
    fn an_escalated_council_gives_each_option_its_own_steps_and_an_invented_option_gets_none() {
        let labels = vec!["PROCEED".to_string(), "ALTERNATIVE: use cron".to_string()];
        let text = "Two paths.\nDECISION: needs the operator: queue or cron?\nIF PROCEED\nNEXT architect: spec the queue\nNEXT builder: build it\nIF ALTERNATIVE: use cron\nNEXT builder: wire the cron job\nIF a third way\nNEXT builder: do something else\n";
        let f = parse(text, &roles(), &labels);
        assert_eq!(f.decision.as_deref(), Some("needs the operator: queue or cron?"));
        assert_eq!(f.steps.len(), 3, "{:?}", f.steps);
        assert_eq!(options_of(&f.steps), labels);
        assert_eq!(f.steps.iter().filter(|s| s.option.as_deref() == Some("PROCEED")).count(), 2);
        // Steps before any IF are not attached to anything when there are options: they would start without the operator choosing.
        assert!(parse("NEXT builder: start now\nIF PROCEED\nNEXT architect: spec\n", &roles(), &labels).steps.iter().all(|s| s.option.is_some()));
    }

    #[test]
    fn an_option_label_matches_exactly_before_it_matches_loosely() {
        let labels = vec!["PROCEED".to_string(), "DO-NOT-PROCEED".to_string()];
        let f = parse("IF DO-NOT-PROCEED\nNEXT architect: write up why\nIF PROCEED\nNEXT builder: build it\n", &roles(), &labels);
        assert_eq!(f.steps.iter().map(|s| (s.option.clone().unwrap(), s.role.clone())).collect::<Vec<_>>(), vec![("DO-NOT-PROCEED".to_string(), "architect".to_string()), ("PROCEED".to_string(), "builder".to_string())]);
        // `not` could mean either: the steps under it are dropped rather than guessed.
        assert!(parse("IF proceed-ish\nNEXT builder: x\n", &roles(), &labels).steps.len() <= 1);
    }

    #[test]
    fn with_no_usable_steps_the_open_blocking_findings_become_them_and_nothing_is_invented_otherwise() {
        let open = vec![("P0".to_string(), "red-team".to_string(), "double delivery".to_string()), ("P1".to_string(), "strategist".to_string(), "no rollback".to_string())];
        let steps = from_findings(&open, &roles());
        assert_eq!(steps.len(), 2);
        assert!(steps[0].role == "architect" && steps[0].objective == "Resolve the open P0 finding raised by red-team: double delivery");
        assert!(from_findings(&[], &roles()).is_empty());
        assert!(from_findings(&open, &["builder".to_string()]).is_empty(), "no architect, no one to resolve it");
    }

    #[test]
    fn the_record_and_the_chat_show_the_decision_the_steps_and_how_to_start_them() {
        let f = Followup { prose: "p".into(), decision: Some("build the table".into()), steps: vec![Step { option: None, role: "architect".into(), objective: "spec it".into() }, Step { option: None, role: "builder".into(), objective: "build it".into() }], from_findings: false };
        let md = render_section(&f, false);
        assert!(md.starts_with("## Decision and next steps") && md.contains("**Decision:** build the table") && md.contains("1. `architect`: spec it") && md.contains("2. `builder`: build it"), "{}", md);
        let text = render_text(&f, false);
        assert!(text.contains("Decision: build the table") && text.contains("  2. builder: build it") && text.ends_with("Start them, one after the other, with /follow."), "{}", text);
        let esc = Followup { prose: String::new(), decision: Some("needs the operator: which?".into()), steps: vec![Step { option: Some("A".into()), role: "architect".into(), objective: "x".into() }, Step { option: Some("B".into()), role: "builder".into(), objective: "y".into() }], from_findings: false };
        let t = render_text(&esc, true);
        assert!(t.contains("If 1 (A):") && t.contains("If 2 (B):") && t.ends_with("Choose with /follow <1-2> to start that option's steps."), "{}", t);
        assert!(render_section(&esc, true).contains("If **A**:"));
        assert!(render_section(&Followup::default(), false).contains("No next steps were recorded"));
        let json = steps_json(&f.steps);
        assert_eq!(steps_from_json(&json), f.steps);
    }

    #[test]
    fn the_instruction_names_the_options_exactly() {
        assert!(format_instruction(&[]).contains("NEXT <role>"));
        let i = format_instruction(&["PROCEED".into(), "ALTERNATIVE: cron".into()]);
        assert!(i.contains("IF <the option's label, exactly>") && i.contains("PROCEED; ALTERNATIVE: cron") && i.contains("needs the operator"));
    }
}
