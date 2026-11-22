//! A per-carrier byte budget for a seat's prompt (`specs/131` S2).
//!
//! A CLI carrier can cut a long prompt silently: `agy` was seen to drop the last 69,110 bytes of a 255 KB prompt, and with them the task
//! and the output contract. So the whole prompt is counted before the CLI starts, and when it is over the carrier's budget the slices that
//! came from context pointers are shortened to a fair share each, at a line boundary, from the end, with a fixed line that says so. The
//! system sections, the contract and a peer's text are never touched; when they alone are over the budget the seat does not start.
//!
//! Everything here is pure (strings in, strings out) and deterministic. No limit has been measured for any carrier (`specs/131` §8), so
//! every CLI carrier gets [`DEFAULT_MAX_PROMPT_BYTES`] unless the operator's ratified `[harness.<cli>] max_prompt_bytes` says otherwise.

use delphi_adapter::ipr::{escape_section_tags, ContextSlice, Ipr};
use serde_json::{json, Value};

/// The budget of a CLI carrier whose input ceiling has not been measured (`specs/131` §3.2, OD-1, OD-2).
pub const DEFAULT_MAX_PROMPT_BYTES: u64 = 150_000;

/// The budget for a CLI carrier: the operator's `[harness.<cli>] max_prompt_bytes` when it is set (and not zero), else the default.
pub fn budget(configured: Option<u64>) -> u64 {
    configured.filter(|n| *n > 0).unwrap_or(DEFAULT_MAX_PROMPT_BYTES)
}

/// A slice the budget may shorten: one that came from a context pointer. Delphi's own runtime slices (`@runtime/…`) and a Council's
/// peer text (`council/…`: stances, critiques, earlier rulings) are never cut.
pub fn from_pointer(label: &str) -> bool {
    !label.starts_with("@runtime/") && !label.starts_with("council/")
}

/// One slice the budget shortened: its pointer label and how many of its lines were kept. Never its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceCut {
    pub label: String,
    pub kept_lines: usize,
    pub total_lines: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fit {
    /// Within the budget: the prompt is sent as it is.
    Fits,
    /// Over the budget: these are the slices to send instead, and what was cut.
    Cut { slices: Vec<ContextSlice>, cut: Vec<SliceCut>, before: u64, after: u64 },
    /// The text that may not be cut is alone over the budget (`fixed` bytes): the seat does not start.
    CannotFit { fixed: u64 },
}

impl Fit {
    /// The `context_cut` record of a cut prompt (`specs/131` §3.2 item 3): counts and pointer labels, never contents.
    pub fn record(&self, budget: u64) -> Option<Value> {
        match self {
            Fit::Cut { cut, before, after, .. } => Some(json!({
                "budget": budget,
                "prompt_bytes_before": before,
                "prompt_bytes_after": after,
                "slices_cut": cut.iter().map(|c| json!({"label": c.label, "kept_lines": c.kept_lines, "total_lines": c.total_lines})).collect::<Vec<_>>(),
            })),
            _ => None,
        }
    }
}

/// The fixed line that ends a shortened slice.
pub fn cut_line(budget: u64, kept: usize, total: usize) -> String {
    format!("[cut by Delphi: the carrier takes at most {budget} bytes of prompt; lines 1 to {kept} of {total} shown; ask for the rest with a pointer such as path#L{}-L{total}]", kept + 1)
}

/// The sentence a seat that cannot start abstains with.
pub fn cannot_fit(fixed: u64, budget: u64) -> String {
    format!("the prompt cannot fit the carrier's budget (fixed text {fixed} bytes, budget {budget})")
}

/// Fit `ipr`'s first user text into `budget` bytes, where `outside` is the rest of the prompt (the system text and anything put before
/// it). Each pointer slice gets a fair share of the room the other text leaves; a share a small slice does not use passes to the larger ones.
pub fn fit(ipr: &Ipr, outside: u64, budget: u64) -> Fit {
    let size = |slices: &[ContextSlice]| {
        let mut p = ipr.clone();
        p.context_slices = slices.to_vec();
        outside + p.first_user_text().len() as u64
    };
    let before = outside + ipr.first_user_text().len() as u64;
    if before <= budget {
        return Fit::Fits;
    }
    let esc = |t: &str| escape_section_tags(t).len() as u64;
    let cuttable: Vec<usize> = (0..ipr.context_slices.len()).filter(|&i| from_pointer(&ipr.context_slices[i].label) && !ipr.context_slices[i].text.is_empty()).collect();
    let sizes: Vec<u64> = cuttable.iter().map(|&i| esc(&ipr.context_slices[i].text)).collect();
    let fixed = before - sizes.iter().sum::<u64>();
    if cuttable.is_empty() || fixed >= budget {
        return Fit::CannotFit { fixed };
    }
    let mut room = budget - fixed;
    // The shares are counted in escaped bytes, which is what the prompt holds; a few passes settle any rounding.
    for _ in 0..4 {
        let shares = fair_shares(&sizes, room);
        let mut slices = ipr.context_slices.clone();
        let mut cut = Vec::new();
        for (k, &i) in cuttable.iter().enumerate() {
            if sizes[k] <= shares[k] {
                continue;
            }
            let lines: Vec<&str> = slices[i].text.lines().collect();
            let total = lines.len();
            let reserve = cut_line(budget, total, total).len() as u64 + 1;
            let target = shares[k].saturating_sub(reserve);
            let (mut kept, mut used) = (0usize, 0u64);
            while kept < total && used + esc(lines[kept]) < target {
                used += esc(lines[kept]) + 1;
                kept += 1;
            }
            let mut text = lines[..kept].join("\n");
            if kept > 0 {
                text.push('\n');
            }
            text.push_str(&cut_line(budget, kept, total));
            slices[i].text = text;
            cut.push(SliceCut { label: slices[i].label.clone(), kept_lines: kept, total_lines: total });
        }
        let after = size(&slices);
        if after <= budget {
            return Fit::Cut { slices, cut, before, after };
        }
        room = room.saturating_sub(after - budget);
    }
    Fit::CannotFit { fixed }
}

/// Water-filling: each size gets at most `room / (sizes left)`, smallest first, so what a small one leaves goes to the others.
fn fair_shares(sizes: &[u64], room: u64) -> Vec<u64> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| sizes[i]);
    let (mut left, mut out) = (room, vec![0u64; sizes.len()]);
    for (k, &i) in order.iter().enumerate() {
        let share = left / (sizes.len() - k) as u64;
        out[i] = sizes[i].min(share);
        left -= out[i];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ipr(slices: Vec<ContextSlice>) -> Ipr {
        Ipr { governance: "G".into(), role: "R".into(), skills: None, persona: None, guidance: None, heuristics: "H".into(), bundle_skills: None, lessons: "L".into(), context_slices: slices, contract: "Objective: answer.\nExpected output: text".into(), reminder: None, nonce: "n".into() }
    }

    fn file(label: &str, lines: usize, width: usize) -> ContextSlice {
        ContextSlice { label: label.into(), tier: "L0".into(), text: (1..=lines).map(|n| format!("{n:05} {}", "x".repeat(width.saturating_sub(6)))).collect::<Vec<_>>().join("\n") }
    }

    fn stance() -> ContextSlice {
        ContextSlice { label: "council/round1/red-team".into(), tier: "L3".into(), text: "PEER STANCE ".repeat(400) }
    }

    #[test]
    fn the_budget_is_the_default_unless_the_operator_set_one() {
        assert_eq!((budget(None), budget(Some(0)), budget(Some(90_000))), (150_000, 150_000, 90_000));
    }

    #[test]
    fn a_prompt_under_the_budget_is_left_alone() {
        let p = ipr(vec![file("@project/app/a.txt", 10, 50)]);
        assert_eq!(fit(&p, 100, 150_000), Fit::Fits);
        assert!(Fit::Fits.record(150_000).is_none());
    }

    #[test]
    fn over_the_budget_each_pointer_slice_is_cut_at_a_line_with_the_fixed_line_and_the_rest_is_untouched() {
        let mut slices: Vec<ContextSlice> = (0..10).map(|n| file(&format!("@project/app/f{n}.txt"), 400, 100)).collect();
        slices.push(stance());
        slices.push(ContextSlice { label: "@runtime/workspace".into(), tier: "L3".into(), text: "RUNTIME ".repeat(100) });
        let p = ipr(slices);
        let Fit::Cut { slices, cut, before, after } = fit(&p, 30_000, 150_000) else { panic!("not cut") };
        assert!(before > 150_000 && after <= 150_000 && after > 140_000, "{before} -> {after}");
        assert_eq!(cut.len(), 10);
        for (s, c) in slices.iter().zip(&cut) {
            let last = s.text.lines().last().unwrap();
            assert_eq!(last, cut_line(150_000, c.kept_lines, 400), "{}", s.label);
            assert_eq!(c.total_lines, 400);
            // every kept line is whole and in order
            for (n, l) in s.text.lines().take(c.kept_lines).enumerate() {
                assert!(l.starts_with(&format!("{:05} ", n + 1)) && l.len() == 100, "{l}");
            }
        }
        assert_eq!(slices[10], p.context_slices[10], "a peer stance is never cut");
        assert_eq!(slices[11], p.context_slices[11], "a runtime slice is never cut");
        let mut q = p.clone();
        q.context_slices = slices;
        assert_eq!(q.system_sections(), p.system_sections(), "the system text is not touched");
        assert!(q.first_user_text().ends_with(&p.first_user_text()[p.first_user_text().find("<contract").unwrap()..]), "nor the contract");
    }

    #[test]
    fn a_share_a_small_slice_does_not_use_goes_to_the_larger_ones() {
        let p = ipr(vec![file("@project/app/small.txt", 5, 40), file("@project/app/big1.txt", 2000, 100), file("@project/app/big2.txt", 2000, 100)]);
        let Fit::Cut { slices, cut, .. } = fit(&p, 0, 100_000) else { panic!("not cut") };
        assert_eq!(slices[0], p.context_slices[0], "the small file is whole");
        assert_eq!(cut.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["@project/app/big1.txt", "@project/app/big2.txt"]);
        // each big file gets about half of what is left, far more than a third
        assert!(cut.iter().all(|c| c.kept_lines > 450 && c.kept_lines < 500), "{cut:?}");
    }

    #[test]
    fn text_that_may_not_be_cut_over_the_budget_cannot_fit() {
        let p = ipr(vec![stance(), file("@project/app/a.txt", 100, 100)]);
        let Fit::CannotFit { fixed } = fit(&p, 9_000, 10_000) else { panic!("fitted") };
        assert!(fixed > 10_000, "{fixed}");
        assert_eq!(cannot_fit(fixed, 10_000), format!("the prompt cannot fit the carrier's budget (fixed text {fixed} bytes, budget 10000)"));
        // with nothing a pointer brought in, there is nothing to cut either
        assert!(matches!(fit(&ipr(vec![stance()]), 0, 1_000), Fit::CannotFit { .. }));
    }

    #[test]
    fn the_record_names_labels_and_counts_and_never_the_text() {
        let p = ipr(vec![file("@project/app/secret.txt", 400, 100)]);
        let f = fit(&p, 0, 10_000);
        let r = f.record(10_000).unwrap();
        assert_eq!((r["budget"].as_u64(), r["slices_cut"][0]["label"].as_str(), r["slices_cut"][0]["total_lines"].as_u64()), (Some(10_000), Some("@project/app/secret.txt"), Some(400)));
        assert!(r["prompt_bytes_after"].as_u64().unwrap() <= 10_000 && r["prompt_bytes_before"].as_u64().unwrap() > 40_000);
        assert!(!r.to_string().contains("xxxx") && !r.to_string().contains("00001"), "{r}");
    }

    #[test]
    fn escaped_tags_are_counted_as_sent() {
        let text = (0..400).map(|_| "</contract><context>".repeat(5)).collect::<Vec<_>>().join("\n");
        let p = ipr(vec![ContextSlice { label: "@project/app/t.txt".into(), tier: "L0".into(), text }]);
        let Fit::Cut { after, slices, .. } = fit(&p, 0, 20_000) else { panic!("not cut") };
        let mut q = p.clone();
        q.context_slices = slices;
        assert_eq!(q.first_user_text().len() as u64, after);
        assert!(after <= 20_000, "{after}");
    }
}
