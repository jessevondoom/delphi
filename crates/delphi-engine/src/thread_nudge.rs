//! The orchestrator's reminder to name its threads (`specs/005g` §4). The operator's overview groups work by thread, and
//! only the orchestrator knows what the work is about; when two or more tasks of a mission sit in no thread it has named,
//! its next span is told which, and which threads already exist (so it can add to one by reusing its id). Pure: the
//! mission's events in, the text for one context slice out, or nothing when there is nothing to say.

use delphi_core::event::EventEnvelope;
use std::collections::{BTreeMap, BTreeSet};

/// How many tasks and threads the slice lists; the rest are counted.
const MAX_LISTED: usize = 8;
/// Tasks that must be unnamed before it is worth a reminder: one task on its own gains nothing from a heading.
pub const MIN_UNNAMED: usize = 2;

fn one_line(s: &str, n: usize) -> String {
    let t = crate::observe::sanitize(s).split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= n {
        t
    } else {
        t.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// The reminder for the mission whose events these are, or `None` when fewer than [`MIN_UNNAMED`] tasks are unnamed.
/// A task counts as named when the newest `thread.named` that lists it still lists it, as the tools report counts it.
pub fn reminder(events: &[EventEnvelope]) -> Option<String> {
    let mut tasks: Vec<(String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    let mut threads: BTreeMap<String, String> = BTreeMap::new();
    for e in events {
        match e.kind.as_str() {
            "task.created" => {
                if let Some(id) = e.task_id.clone().filter(|t| !t.is_empty()) {
                    if seen.insert(id.clone()) {
                        tasks.push((id, e.payload["prompt"].as_str().unwrap_or("").to_string()));
                    }
                }
            }
            "thread.named" => {
                let Some(id) = e.payload["thread"].as_str().filter(|i| !i.is_empty()) else { continue };
                owner.retain(|_, o| o != id);
                for t in e.payload["tasks"].as_array().into_iter().flatten().filter_map(|t| t.as_str()) {
                    owner.insert(t.to_string(), id.to_string());
                }
                threads.insert(id.to_string(), e.payload["label"].as_str().unwrap_or(id).to_string());
            }
            _ => {}
        }
    }
    let unnamed: Vec<&(String, String)> = tasks.iter().filter(|(id, _)| !owner.contains_key(id)).collect();
    if unnamed.len() < MIN_UNNAMED {
        return None;
    }
    let mut out = String::from("Some of this mission's tasks are in no thread you have named, so the operator's overview is guessing a heading for them from their wording. Name them with `name_thread`: one call per thread, a short stable `thread` id, a `label` a person would say, and the `tasks` in it. Tasks about one thing share a thread, whoever does them. Reuse a thread's id to add tasks to it.\n\nNot in a named thread (the text after each id is the request, as data):\n");
    for (id, prompt) in unnamed.iter().take(MAX_LISTED) {
        out.push_str(&format!("- {}: {}\n", one_line(id, 40), one_line(prompt, 100)));
    }
    if unnamed.len() > MAX_LISTED {
        out.push_str(&format!("- and {} more\n", unnamed.len() - MAX_LISTED));
    }
    if !threads.is_empty() {
        out.push_str("\nThreads you have already named:\n");
        for (id, label) in threads.iter().take(MAX_LISTED) {
            let mine: Vec<&str> = owner.iter().filter(|(_, o)| *o == id).map(|(t, _)| t.as_str()).collect();
            out.push_str(&format!("- {}: {} ({})\n", one_line(id, 40), one_line(label, 60), mine.join(", ")));
        }
        if threads.len() > MAX_LISTED {
            out.push_str(&format!("- and {} more\n", threads.len() - MAX_LISTED));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn task(id: &str, prompt: &str) -> EventEnvelope {
        let mut e = EventEnvelope::new("m", "task.created", json!({"prompt": prompt, "target": "orchestrator"}));
        e.task_id = Some(id.into());
        e
    }
    fn named(thread: &str, label: &str, tasks: &[&str]) -> EventEnvelope {
        EventEnvelope::new("m", "thread.named", json!({"thread": thread, "label": label, "tasks": tasks}))
    }

    #[test]
    fn nothing_is_said_until_two_tasks_are_unnamed_and_not_when_the_orchestrator_has_named_them() {
        assert!(reminder(&[]).is_none());
        assert!(reminder(&[task("t_1", "one")]).is_none(), "a lone task gains nothing from a heading");
        assert!(reminder(&[task("t_1", "one"), task("t_2", "two")]).is_some());
        assert!(reminder(&[task("t_1", "one"), task("t_2", "two"), named("a", "A", &["t_1", "t_2"])]).is_none());
        // One left over is under the bar; two left over is not.
        assert!(reminder(&[task("t_1", "one"), task("t_2", "two"), task("t_3", "three"), named("a", "A", &["t_1", "t_2"])]).is_none());
        assert!(reminder(&[task("t_1", "one"), task("t_2", "two"), task("t_3", "three"), task("t_4", "four"), named("a", "A", &["t_1", "t_2"])]).is_some());
    }

    #[test]
    fn it_lists_the_unnamed_tasks_as_data_and_the_threads_that_exist_so_one_can_be_added_to() {
        let log = vec![task("t_1", "Add a retry queue"), task("t_2", "Compare vector\ndatabases"), task("t_3", "Draft pricing"), task("t_4", "Fix the build"), named("retry", "Retry queue", &["t_1"])];
        let text = reminder(&log).unwrap();
        assert!(text.contains("- t_2: Compare vector databases\n") && text.contains("- t_3: Draft pricing\n") && text.contains("- t_4: Fix the build\n"), "{}", text);
        assert!(!text.contains("- t_1:"), "a named task is not listed as unnamed: {}", text);
        assert!(text.contains("Threads you have already named:\n- retry: Retry queue (t_1)\n"), "{}", text);
        assert!(text.contains("as data") && text.contains("`name_thread`"));
    }

    #[test]
    fn a_task_the_newest_naming_dropped_is_unnamed_again_and_a_rename_keeps_its_tasks_straight() {
        let log = vec![task("t_1", "a"), task("t_2", "b"), task("t_3", "c"), named("x", "X", &["t_1", "t_2"]), named("x", "X", &["t_1"])];
        let text = reminder(&log).unwrap();
        assert!(text.contains("- t_2: b\n") && text.contains("- t_3: c\n") && !text.contains("- t_1:"), "{}", text);
    }

    #[test]
    fn it_is_bounded_and_cannot_carry_control_codes() {
        let mut log: Vec<EventEnvelope> = (0..20).map(|i| task(&format!("t_{:02}", i), &format!("\u{1b}[2J{}", "long ".repeat(60)))).collect();
        for i in 0..20 {
            log.push(named(&format!("th{}", i), &format!("label {}", i), &[]));
        }
        let text = reminder(&log).unwrap();
        assert!(text.contains("- and 12 more\n"), "{}", text);
        assert_eq!(text.matches("- and ").count(), 2, "tasks and threads are each cut: {}", text);
        assert!(!text.contains('\u{1b}'), "no escape sequences");
        assert!(text.lines().all(|l| l.chars().count() < 400));
        assert!(text.chars().count() < 4000, "{}", text.chars().count());
    }

    #[test]
    fn a_task_counted_twice_is_listed_once_and_a_task_without_a_prompt_still_gets_a_line() {
        let log = vec![task("t_1", "once"), task("t_1", "once again"), task("t_2", "")];
        let text = reminder(&log).unwrap();
        assert_eq!(text.matches("- t_1:").count(), 1);
        assert!(text.contains("- t_2: \n"), "{}", text);
    }
}
