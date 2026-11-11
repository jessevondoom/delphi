//! A red-team review's findings, collected from every seat's words and grouped (`specs/098` §6.2). Pure and deterministic: no model
//! decides whether two findings name the same issue. Two findings are the same issue when their titles are equal after
//! [`norm`], which is a blunt test on purpose: it never merges two findings that read differently, so a group is always safe to
//! trust and a missed merge costs one extra line.
//!
//! Disagreement is kept visible. A group lists every seat that raised the issue with its own family and where that family came
//! from, and the group's severity is the highest any seat gave it; each seat's own severity stays on its own entry.

use serde_json::{json, Value};

/// One finding as one seat worded it.
#[derive(Debug, Clone, PartialEq)]
pub struct Raw {
    /// `P0` to `P3`.
    pub severity: String,
    pub title: String,
    pub seat: String,
    /// The seat's served family from its receipt, or the host's declared family (`family_source` says which).
    pub family: Value,
    pub family_source: String,
    /// 1 for a Round 1 stance, 2 for a Round 2 critique.
    pub round: u8,
}

/// Rank of a severity: lower is worse. Anything unknown ranks last.
pub fn rank(sev: &str) -> u8 {
    match sev {
        "P0" => 0,
        "P1" => 1,
        "P2" => 2,
        "P3" => 3,
        _ => 4,
    }
}

/// A title reduced to what decides sameness: lower case, letters and digits only, a leading article dropped, single spaces.
pub fn norm(title: &str) -> String {
    let words: Vec<String> = title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(|w| w.to_lowercase()).collect();
    let skip = usize::from(words.first().is_some_and(|w| matches!(w.as_str(), "the" | "a" | "an")) && words.len() > 1);
    words[skip..].join(" ")
}

/// The per-seat list and the groups. A seat that raised the same issue in both rounds appears once, with `rounds: [1, 2]` and its
/// highest severity. Groups are ordered worst first, then in the order the issue was first raised.
pub fn collect(raws: &[Raw]) -> (Vec<Value>, Vec<Value>) {
    // per seat: merge rounds
    let mut per_seat: Vec<(String, Raw, Vec<u8>)> = Vec::new();
    for (i, r) in raws.iter().enumerate() {
        let n = norm(&r.title);
        let key = if n.is_empty() { format!("#{i}") } else { n };
        match per_seat.iter_mut().find(|(k, s, _)| *k == key && s.seat == r.seat) {
            Some((_, kept, rounds)) => {
                if rank(&r.severity) < rank(&kept.severity) {
                    kept.severity = r.severity.clone();
                }
                if !rounds.contains(&r.round) {
                    rounds.push(r.round);
                    rounds.sort_unstable();
                }
            }
            None => per_seat.push((key, r.clone(), vec![r.round])),
        }
    }
    let seats: Vec<Value> = per_seat.iter().map(|(_, r, rounds)| json!({"severity": r.severity, "title": r.title, "seat": r.seat, "family": r.family, "family_source": r.family_source, "rounds": rounds})).collect();
    // across seats: group by the same key
    let mut groups: Vec<(String, String, String, Vec<Value>)> = Vec::new(); // (key, worst severity, first title, seat entries)
    for (key, r, rounds) in &per_seat {
        let entry = json!({"seat": r.seat, "severity": r.severity, "family": r.family, "family_source": r.family_source, "rounds": rounds});
        match groups.iter_mut().find(|g| g.0 == *key) {
            Some(g) => {
                if rank(&r.severity) < rank(&g.1) {
                    g.1 = r.severity.clone();
                }
                g.3.push(entry);
            }
            None => groups.push((key.clone(), r.severity.clone(), r.title.clone(), vec![entry])),
        }
    }
    groups.sort_by_key(|g| rank(&g.1)); // stable: first raised stays first within a severity
    let groups = groups.into_iter().map(|(_, sev, title, entries)| json!({"severity": sev, "title": title, "raised_by": entries.len(), "seats": entries})).collect();
    (seats, groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(sev: &str, title: &str, seat: &str, fam: &str, src: &str, round: u8) -> Raw {
        Raw { severity: sev.into(), title: title.into(), seat: seat.into(), family: json!(fam), family_source: src.into(), round }
    }

    #[test]
    fn norm_ignores_case_punctuation_spacing_and_a_leading_article_only() {
        assert_eq!(norm("The  Replay of the barrier file!"), "replay of the barrier file");
        assert_eq!(norm("replay of the BARRIER file"), "replay of the barrier file");
        assert_eq!(norm("A"), "a", "a lone article is not dropped to nothing");
        assert_ne!(norm("replay of the barrier file"), norm("replay of the barrier files"), "it never merges two titles that read differently");
        assert_eq!(norm("***"), "");
    }

    #[test]
    fn the_same_issue_from_two_seats_is_one_group_listing_both_with_their_own_families() {
        let raws = [
            raw("P1", "Replay of the barrier file", "red-team", "gemini", "receipt", 2),
            raw("P0", "the replay of the barrier file.", "architect", "claude", "host-declared", 2),
            raw("P2", "Naming", "red-team", "gemini", "receipt", 2),
        ];
        let (seats, groups) = collect(&raws);
        assert_eq!(seats.len(), 3, "each seat keeps its own line");
        assert_eq!(groups.len(), 2);
        let g = &groups[0];
        assert_eq!((g["severity"].as_str(), g["raised_by"].as_u64()), (Some("P0"), Some(2)), "the worst severity any seat gave it: {g}");
        assert_eq!(g["title"], "Replay of the barrier file", "the first seat's wording");
        let by: Vec<(&str, &str, &str)> = g["seats"].as_array().unwrap().iter().map(|s| (s["seat"].as_str().unwrap(), s["family"].as_str().unwrap(), s["family_source"].as_str().unwrap())).collect();
        assert_eq!(by, [("red-team", "gemini", "receipt"), ("architect", "claude", "host-declared")]);
        assert_eq!(seats[0]["severity"], "P1", "a seat's own severity is not overwritten by the group's");
        assert_eq!(groups[1]["title"], "Naming");
    }

    #[test]
    fn one_seat_raising_an_issue_in_both_rounds_is_one_entry_with_both_rounds_and_its_worst_severity() {
        let raws = [raw("P2", "Idempotency keys", "red-team", "gemini", "receipt", 1), raw("P1", "idempotency keys", "red-team", "gemini", "receipt", 2)];
        let (seats, groups) = collect(&raws);
        assert_eq!(seats.len(), 1);
        assert_eq!((seats[0]["severity"].as_str(), seats[0]["rounds"].clone()), (Some("P1"), json!([1, 2])));
        assert_eq!((groups.len(), groups[0]["raised_by"].as_u64()), (1, Some(1)), "one seat raising it twice is not two seats agreeing");
    }

    #[test]
    fn groups_are_ordered_worst_first_then_by_when_they_were_first_raised() {
        let raws = [raw("P2", "b", "x", "f", "receipt", 2), raw("P1", "c", "x", "f", "receipt", 2), raw("P2", "a", "x", "f", "receipt", 2), raw("P0", "d", "x", "f", "receipt", 2)];
        let (_, groups) = collect(&raws);
        let order: Vec<&str> = groups.iter().map(|g| g["title"].as_str().unwrap()).collect();
        assert_eq!(order, ["d", "c", "b", "a"]);
    }

    #[test]
    fn a_title_with_nothing_to_compare_is_never_merged() {
        let raws = [raw("P1", "***", "a", "f", "receipt", 2), raw("P1", "!!!", "b", "f", "receipt", 2), raw("P1", "***", "a", "f", "receipt", 1)];
        let (seats, groups) = collect(&raws);
        assert_eq!((seats.len(), groups.len()), (3, 3), "an unreadable title stays alone, even a seat's own repeat");
    }

    #[test]
    fn no_findings_gives_nothing() {
        assert_eq!(collect(&[]), (vec![], vec![]));
    }
}
