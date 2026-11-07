//! `delphi train retention`: do facts survive a compaction? (`specs/048` BM-7, "context retention".)
//!
//! A session tells the assistant N exact facts early (a hash, a path, a number, a line of code, a sentence) and then runs a long
//! stretch of routine work. A forced compaction (`specs/045` T6) or a fresh session (`/new`, `specs/030`) follows. Then a probe
//! asks for each fact by name. The score is the share answered **byte for byte**.
//!
//! The run here is **scripted**: the session, the compaction and the model are deterministic functions, so it costs nothing and
//! shows what the instrument can tell apart (`specs/048` AC-5): a model that forgets after a summary scores low; the same model
//! with a handle to the dropped turns (`specs/049` CX-3, modelled here, not built) scores high *if it follows the handle*. A
//! live run needs the compaction itself (`specs/045` H3), which is not built; the answers of a live probe session are scored by
//! the same `exact_items` check as BM-5, over the same seeded items (`train_items`).
//!
//! The scripted model is a model of retrieval, not a measurement of any real model.

use std::collections::BTreeMap;

use serde_json::json;

use crate::train_items::{self as items, Item, ItemKind, Rng};

#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub who: &'static str,
    pub text: String,
}

const TASKS: [&str; 8] = ["rename the helper", "run the formatter", "update the changelog", "check the lint output", "tidy the imports", "re-read the failing test", "bump the patch version", "draft the commit message"];
const DONE: [&str; 6] = ["Done.", "Done; nothing else changed.", "Finished, and the checks pass.", "Done, as asked.", "That is in place.", "Completed without errors."];

/// The facts first, each in a turn of its own, then `filler` turns of routine work that mention none of them.
pub fn session(its: &[Item], filler: usize, seed: u64) -> Vec<Turn> {
    let mut rng = Rng::new(seed ^ 0x00F1_11E4);
    let mut out: Vec<Turn> = Vec::new();
    for it in its {
        out.push(Turn { who: "operator", text: format!("For the record, {} is: {}", it.label, it.text) });
        out.push(Turn { who: "assistant", text: "Noted.".into() });
    }
    for _ in 0..filler {
        out.push(Turn { who: "operator", text: format!("Next, {}.", TASKS[rng.below(TASKS.len())]) });
        out.push(Turn { who: "assistant", text: DONE[rng.below(DONE.len())].into() });
    }
    out
}

fn render(turns: &[Turn]) -> String {
    turns.iter().map(|t| format!("{}: {}\n", t.who, t.text)).collect()
}

/// What happens to the session at the compaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compactor {
    /// Nothing is dropped: the upper bound.
    None,
    /// A summary replaces the old turns and states no value.
    Summary,
    /// The same summary, and the dropped turns are kept whole behind a handle the summary names (`specs/049` CX-3).
    SummaryWithHandle,
    /// A new session: only a one-line note of the old one remains.
    Reset,
}

/// What the model sees after the compaction, and what a handle can still reach.
#[derive(Debug, Clone, PartialEq)]
pub struct Context {
    pub window: String,
    pub store: BTreeMap<String, String>,
}

const HANDLE: &str = "@payload/ctx-1";

pub fn compact(turns: &[Turn], c: Compactor, keep_recent: usize) -> Context {
    let cut = turns.len().saturating_sub(keep_recent * 2);
    let (old, recent) = turns.split_at(cut);
    let summary = format!("[summary] The operator gave {} reference values and then asked for {} routine tasks, all done. Goal: carry on.\n", old.iter().filter(|t| t.text.starts_with("For the record")).count(), old.iter().filter(|t| t.text.starts_with("Next,")).count());
    match c {
        Compactor::None => Context { window: render(turns), store: BTreeMap::new() },
        Compactor::Summary => Context { window: summary + &render(recent), store: BTreeMap::new() },
        Compactor::SummaryWithHandle => Context { window: format!("{}[the earlier turns, whole: {}]\n{}", summary, HANDLE, render(recent)), store: BTreeMap::from([(HANDLE.to_string(), render(old))]) },
        Compactor::Reset => Context { window: format!("[new session] A previous session ran {} turns.\n", turns.len()), store: BTreeMap::new() },
    }
}

/// A scripted model: it answers a question about a fact only from what it can see, and follows a handle only if it is the kind
/// that does (the Orchestrator and seat instructions of `specs/049` CX-6 are what would make a real one).
#[derive(Debug, Clone, Copy)]
pub struct Model {
    pub follows_handles: bool,
}

/// The value stated after `<label> is: ` in `text`, to the end of that line.
fn stated(text: &str, label: &str) -> Option<String> {
    let needle = format!("{} is: ", label);
    let at = text.find(&needle)? + needle.len();
    Some(text[at..].lines().next().unwrap_or("").to_string())
}

impl Model {
    pub fn answer(&self, ctx: &Context, label: &str) -> Option<String> {
        if let Some(v) = stated(&ctx.window, label) {
            return Some(v);
        }
        if !self.follows_handles {
            return None;
        }
        ctx.window.split_whitespace().map(|w| w.trim_end_matches(']')).filter(|w| w.starts_with("@payload/")).find_map(|h| ctx.store.get(h).and_then(|raw| stated(raw, label)))
    }
}

/// One condition's score.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub name: &'static str,
    /// Per kind: facts, recovered exactly.
    pub kinds: BTreeMap<ItemKind, (usize, usize)>,
    pub recovered: f64,
    /// Characters the model has to read after the compaction.
    pub context_chars: usize,
}

pub const CONDITIONS: [(&str, Compactor, bool); 5] = [
    ("no compaction", Compactor::None, false),
    ("summary", Compactor::Summary, true),
    ("summary + handle, followed", Compactor::SummaryWithHandle, true),
    ("summary + handle, ignored", Compactor::SummaryWithHandle, false),
    ("new session", Compactor::Reset, true),
];

pub fn run(its: &[Item], filler: usize, keep_recent: usize, seed: u64) -> Vec<Outcome> {
    let turns = session(its, filler, seed);
    CONDITIONS.iter().map(|(name, c, follows)| {
        let ctx = compact(&turns, *c, keep_recent);
        let model = Model { follows_handles: *follows };
        let got: Vec<bool> = its.iter().map(|it| model.answer(&ctx, &it.label).as_deref() == Some(it.text.as_str())).collect();
        Outcome { name, kinds: items::by_kind(its, &got), recovered: items::share(&got).unwrap_or(0.0), context_chars: ctx.window.len() }
    }).collect()
}

/// The chart-ready table (`specs/048` §5): recovery by condition and kind.
pub fn table(seed: u64, n: usize, filler: usize, keep: usize, outcomes: &[Outcome]) -> String {
    let mut out = format!("CONTEXT RETENTION (seed {}, {} facts, {} routine tasks, last {} exchanges kept): facts answered byte for byte\n", seed, n, filler, keep);
    out.push_str(&format!("{:<30}{:>8}", "condition", "context"));
    for k in ItemKind::ALL {
        out.push_str(&format!("{:>11}", k.name()));
    }
    out.push_str(&format!("{:>8}\n", "ALL"));
    for o in outcomes {
        out.push_str(&format!("{:<30}{:>8}", o.name, format!("{}c", o.context_chars)));
        for k in ItemKind::ALL {
            let (t, r) = o.kinds.get(&k).copied().unwrap_or((0, 0));
            out.push_str(&format!("{:>11}", format!("{}/{}", r, t)));
        }
        out.push_str(&format!("{:>8}\n", format!("{:.0}%", o.recovered * 100.0)));
    }
    out.push_str("A scripted session, compaction and model. It shows what the instrument can tell apart, not what any model does; a live run waits on the compaction (specs/045 H3).\n");
    out
}

pub fn to_json(seed: u64, n: usize, outcomes: &[Outcome]) -> serde_json::Value {
    json!({"seed": seed, "facts": n, "conditions": outcomes.iter().map(|o| json!({
        "condition": o.name, "recovered": o.recovered, "context_chars": o.context_chars,
        "kinds": o.kinds.iter().map(|(k, (t, r))| (k.name().to_string(), json!({"facts": t, "recovered": r}))).collect::<serde_json::Map<_, _>>()})).collect::<Vec<_>>()})
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

const USAGE: &str = "delphi train retention run [--seed <n>] [--facts <5-200>] [--filler <n>] [--keep <n>] [--json]\n  scores a scripted session, compaction and model (no model call, no cost): the share of seeded facts answered byte for byte after no compaction, a summary, a summary with a handle to the dropped turns, and a new session.";

/// `delphi train retention run ...` (`args[0]` is `retention`).
pub fn cli(args: &[String]) -> i32 {
    if args.get(1).map(String::as_str) != Some("run") {
        eprintln!("{}", USAGE);
        return 2;
    }
    let num = |name: &str, default: u64, lo: u64, hi: u64| -> Result<u64, String> {
        let v = flag(args, name).map_or(Ok(default), |v| v.parse::<u64>().map_err(|_| format!("{} must be a whole number", name)))?;
        (lo..=hi).contains(&v).then_some(v).ok_or(format!("{} must be between {} and {}", name, lo, hi))
    };
    let parsed = (|| Ok::<_, String>((num("--seed", 1, 0, u64::MAX)?, num("--facts", 10, 5, 200)? as usize, num("--filler", 40, 0, 2000)? as usize, num("--keep", 4, 0, 100)? as usize)))();
    let (seed, n, filler, keep) = match parsed {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi train retention: {}\n  {}", e, USAGE);
            return 2;
        }
    };
    let its = items::generate(seed, n);
    let outcomes = run(&its, filler, keep, seed);
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&to_json(seed, n, &outcomes)).unwrap_or_default());
    } else {
        print!("{}", table(seed, n, filler, keep, &outcomes));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn by_name<'a>(o: &'a [Outcome], name: &str) -> &'a Outcome {
        o.iter().find(|x| x.name == name).unwrap()
    }

    #[test]
    fn a_model_that_forgets_after_a_summary_scores_low_and_with_a_handle_it_scores_high() {
        // AC-5: the same scripted model, the same session, the same compaction; only the handle differs.
        let its = items::generate(5, 10);
        let o = run(&its, 40, 4, 5);
        assert_eq!(by_name(&o, "no compaction").recovered, 1.0, "the upper bound");
        assert_eq!(by_name(&o, "summary").recovered, 0.0, "a summary that states no value loses every fact");
        assert_eq!(by_name(&o, "summary + handle, followed").recovered, 1.0);
        assert_eq!(by_name(&o, "summary + handle, ignored").recovered, 0.0, "a handle nobody follows recovers nothing");
        assert_eq!(by_name(&o, "new session").recovered, 0.0);
        for (k, (t, r)) in &by_name(&o, "summary + handle, followed").kinds {
            assert_eq!((*t, *r), (2, 2), "{:?}", k);
        }
    }

    #[test]
    fn a_handle_costs_a_few_characters_not_the_whole_history() {
        let o = run(&items::generate(5, 10), 40, 4, 5);
        let (full, handle, summary) = (by_name(&o, "no compaction").context_chars, by_name(&o, "summary + handle, followed").context_chars, by_name(&o, "summary").context_chars);
        assert!(handle > summary && handle - summary < 60 && handle * 3 < full, "full {} handle {} summary {}", full, handle, summary);
    }

    #[test]
    fn the_dropped_turns_are_kept_whole_and_the_recent_ones_stay_in_the_window() {
        let its = items::generate(2, 5);
        let turns = session(&its, 10, 2);
        assert_eq!(turns.len(), 30);
        let ctx = compact(&turns, Compactor::SummaryWithHandle, 4);
        // No loss: the stored text is exactly the first 22 turns, rendered, and the window carries the other eight.
        assert_eq!(ctx.store[HANDLE], render(&turns[..22]));
        assert!(ctx.window.ends_with(&render(&turns[22..])) && ctx.window.contains(HANDLE) && ctx.window.contains("reference values"));
        // Keeping everything recent leaves nothing to drop.
        assert_eq!(compact(&turns, Compactor::Summary, 100).window.lines().count(), 31);
        // The routine turns name none of the facts, so only the first turns can answer a probe.
        assert!(turns[10..].iter().all(|t| its.iter().all(|i| !t.text.contains(&i.text))));
    }

    #[test]
    fn a_fact_still_inside_the_recent_turns_is_answered_without_a_handle() {
        // With no routine work after the facts, nothing recent is lost: the instrument sees that too.
        let its = items::generate(8, 5);
        let o = run(&its, 0, 10, 8);
        assert_eq!(by_name(&o, "summary").recovered, 1.0);
        // And a probe for a label the session never gave is not answered.
        let ctx = compact(&session(&its, 3, 8), Compactor::None, 0);
        assert_eq!(Model { follows_handles: true }.answer(&ctx, "the ninth commit hash"), None);
    }

    #[test]
    fn the_table_and_json_carry_every_condition_and_kind_and_the_command_checks_its_arguments() {
        let its = items::generate(1, 10);
        let o = run(&its, 40, 4, 1);
        let t = table(1, 10, 40, 4, &o);
        for want in ["CONTEXT RETENTION (seed 1, 10 facts", "no compaction", "summary + handle, followed", "new session", "code line", "100%", "0%", "scripted session"] {
            assert!(t.contains(want), "{} in\n{}", want, t);
        }
        let j = to_json(1, 10, &o);
        assert_eq!((j["conditions"][2]["condition"].as_str(), j["conditions"][2]["recovered"].as_f64(), j["conditions"][1]["recovered"].as_f64()), (Some("summary + handle, followed"), Some(1.0), Some(0.0)));
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cli(&a(&["retention", "run"])), 0);
        assert_eq!(cli(&a(&["retention", "run", "--json", "--facts", "20", "--filler", "5"])), 0);
        assert_eq!(cli(&a(&["retention", "run", "--facts", "3"])), 2, "fewer than five facts cannot cover every kind");
        assert_eq!(cli(&a(&["retention", "run", "--seed", "x"])), 2);
        assert_eq!(cli(&a(&["retention"])), 2);
    }
}
