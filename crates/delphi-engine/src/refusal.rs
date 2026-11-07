//! Does a short answer begin by declining? One deterministic tagger, shared by the training harness and the Council record.
//!
//! DIAGNOSTIC ONLY (`specs/078` Goodhart rule): the result is shown beside a seat in a record or a report. It is never a seat
//! score and never feeds ranking, routing, seat choice or any decision; code that did so would make refusing look like a loss
//! or a win to be optimised.

/// Does a short answer begin by declining ("I can't reproduce ...")? A refusal fails every content check the same way a wrong
/// answer does; the 2026-11-07 Copilot runs showed one model declining to list hashes and paths (`specs/091` §1). Anchored at the
/// start and limited to short answers, so a long answer that merely contains "I can't" is not tagged.
pub fn declines(deliverable: &str) -> Option<String> {
    let t = deliverable.trim_start();
    if t.chars().count() > 1200 {
        return None;
    }
    let re = regex::Regex::new(r"(?i)^\W*(i(\s+(can.?t|cannot|won.?t|will\s+not|am\s+(unable|not\s+able))|.?m\s+(unable|not\s+able))|sorry|unable\s+to)\b").ok()?;
    re.find(t)?;
    let start: String = t.chars().take(50).collect();
    Some(format!("the answer declines (it begins \"{}...\"): a refusal, not a lossy answer", start.split_whitespace().collect::<Vec<_>>().join(" ")))
}

/// Did a Round 1 reply carry the `OPTION:` line the Council asks for? A diagnostic like [`declines`]: not a score.
pub fn round1_parsed(text: &str) -> bool {
    text.lines().any(|l| l.trim_start().strip_prefix("OPTION:").is_some_and(|r| !r.trim().is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_answer_that_begins_by_declining_is_tagged_and_other_answers_are_not() {
        for yes in [
            "I can\u{2019}t reproduce a bulk list of exact repository identifiers, paths, limits, and security rotation details.",
            "I can't reproduce the report's exact rollback hashes.",
            "I cannot do that.",
            "  I'm unable to list them.",
            "Sorry, I won't list those.",
            "Unable to comply.",
        ] {
            let n = declines(yes).unwrap_or_else(|| panic!("not tagged: {}", yes));
            assert!(n.contains("a refusal, not a lossy answer"), "{}", n);
        }
        for no in [
            "- Finding 1: `0f171a65`\n- Finding 2: `src/a.rs`",
            "Here is the list. I can't be sure about line 7.",
            "The investigator said it can't be done, but here are the items: ...",
            "",
        ] {
            assert!(declines(no).is_none(), "wrongly tagged: {}", no);
        }
        // A long answer is never tagged, even if it starts that way.
        assert!(declines(&format!("I can't be exact. {}", "x".repeat(1300))).is_none());
    }

    #[test]
    fn round1_needs_an_option_line() {
        assert!(round1_parsed("OPTION: queue\nreasons"));
        assert!(round1_parsed("preamble\n  OPTION: cron"));
        assert!(!round1_parsed("OPTION:   \nreasons"));
        assert!(!round1_parsed("I can't reproduce that."));
    }
}
