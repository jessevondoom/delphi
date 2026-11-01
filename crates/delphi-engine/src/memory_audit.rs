//! The memory write-path audit (`specs/050` ML-6).
//!
//! Every code path that writes memory (a staged note, the agent `note` tool, a proposed rule, a ratified rule, a
//! checkpoint summary) is listed in [`SITES`] with the test that proves it stamps the source's taint, keeps a note from a
//! tainted span below `internal_empirical`, never lands in an always-loaded store without ratification, and is inert as
//! an instruction in a later prompt. [`audit`] scans the source for such writes: one that is not listed, or whose proof
//! test is gone, is a finding, and `delphi lint --repo` fails on it. It reads source text only and writes nothing.
//!
//! The context flush of `specs/045` T6 is not built; when it is, it gets a row here or the lint fails.

/// The `learn.*` event kinds that carry memory. (`learn.dropped` removes a note and writes nothing.)
const MEMORY_EVENTS: [&str; 4] = ["learn.staged", "learn.note", "learn.proposed", "learn.proposal_resolved"];

/// One audited write path. `files` are repo-relative paths allowed to write `events`; `proofs` are `(file, test name)`
/// pairs that must exist.
pub struct Site {
    pub name: &'static str,
    pub files: &'static [&'static str],
    pub events: &'static [&'static str],
    pub proofs: &'static [(&'static str, &'static str)],
}

pub const SITES: &[Site] = &[
    Site {
        name: "staged-note capture (the daemon's triggers) and its projection into the `staged` zone",
        files: &["crates/delphi-engine/src/learn.rs"],
        events: &["learn.staged"],
        proofs: &[
            ("crates/delphi-engine/src/learn.rs", "a_note_from_a_task_that_read_untrusted_content_stays_below_empirical"),
            ("crates/delphi-engine/src/learn.rs", "a_verdict_from_a_tainted_span_is_staged_below_empirical"),
            ("crates/delphi-engine/src/kb.rs", "staged_notes_are_a_zone_that_only_an_explicit_search_sees_and_taint_still_applies"),
            ("crates/delphi-engine/src/learn.rs", "a_staged_note_of_a_project_mission_lands_in_that_projects_layer_and_never_in_home_or_another_project"),
            ("crates/delphi-engine/src/learn.rs", "a_mission_scoped_to_a_spoofed_or_unregistered_project_is_staged_nowhere"),
        ],
    },
    Site {
        name: "a host agent's proposed note or session summary (`specs/062` §8: tier L0, labelled, staged only, role-keyed, evidence required)",
        files: &["crates/delphi-engine/src/host_rpc.rs"],
        events: &["learn.staged"],
        proofs: &[
            ("crates/delphi-engine/src/service.rs", "a_host_stages_untrusted_notes_in_its_own_session_and_the_switch_holds"),
        ],
    },
    Site {
        name: "`delphi customize note` (the operator's own note, tier L5, staged and never loaded)",
        files: &["crates/delphi-engine/src/customize.rs"],
        events: &["learn.staged"],
        proofs: &[
            ("crates/delphi-engine/src/customize.rs", "a_note_is_tier_l5_staged_and_named_by_its_sequence_number"),
            ("crates/delphi/src/customize_cmd.rs", "a_script_cannot_write_a_rule_a_skill_or_a_profile_line"),
        ],
    },
    Site {
        name: "the agent `note` tool",
        files: &["crates/delphi-engine/src/tools.rs"],
        events: &["learn.note"],
        proofs: &[
            ("crates/delphi-engine/src/tools.rs", "any_role_stages_a_note_that_inherits_the_spans_taint"),
            ("crates/delphi-engine/src/learn.rs", "an_agent_note_is_staged_below_trusted_tiers_and_three_per_task_are_kept"),
        ],
    },
    Site {
        name: "the orchestrator's `suggest_rule` proposal",
        files: &["crates/delphi-engine/src/tools.rs"],
        events: &["learn.proposed"],
        proofs: &[("crates/delphi-engine/src/tools.rs", "only_the_orchestrator_suggests_a_rule_and_the_proposal_inherits_the_spans_taint")],
    },
    Site {
        name: "`/retro` promotion (the operator's ratification into `heuristics.md`)",
        files: &["crates/delphi-engine/src/service.rs", "crates/delphi/src/retro_cmd.rs", "crates/delphi-engine/src/retro.rs"],
        events: &["learn.proposal_resolved"],
        proofs: &[
            ("crates/delphi-engine/src/retro.rs", "accepting_writes_the_live_file_the_copy_and_the_pin_and_the_next_snapshot_carries_it"),
            ("crates/delphi-engine/src/service.rs", "retro_over_rpc_is_console_only_ratifies_a_rule_stamps_it_and_resolves_the_proposal"),
            ("crates/delphi/src/retro_cmd.rs", "a_refused_proposal_changes_nothing_and_stays_listed"),
        ],
    },
    Site {
        name: "the digest's `promote` (the operator's act: a candidate becomes a proposal for `/retro`; nothing loads)",
        files: &["crates/delphi-engine/src/digest.rs"],
        events: &["learn.proposed"],
        proofs: &[("crates/delphi-engine/src/digest.rs", "promote_only_proposes_for_the_retro_review_and_never_a_card_or_a_rule_and_a_low_tier_cannot_promote")],
    },
    Site {
        name: "project-to-home promotion (`specs/058` §7): a data-only proposal, the operator's acceptance as a tainted home entry, a withdrawal that retires it",
        files: &["crates/delphi-engine/src/promote.rs"],
        events: &["learn.proposed", "learn.proposal_resolved"],
        proofs: &[
            ("crates/delphi-engine/src/promote.rs", "preflight_refuses_each_ineligible_record_with_a_stable_code"),
            ("crates/delphi-engine/src/promote.rs", "a_proposal_is_data_in_the_retro_flow_and_leaves_rules_and_home_untouched"),
            ("crates/delphi-engine/src/promote.rs", "accepting_writes_a_home_entry_with_provenance_that_stays_tainted_and_unvalidated"),
            ("crates/delphi-engine/src/promote.rs", "a_record_that_turned_private_revoked_or_secret_after_the_proposal_is_refused_at_accept"),
            ("crates/delphi-engine/src/promote.rs", "two_concurrent_accepts_of_one_proposal_land_once"),
            ("crates/delphi-engine/src/promote.rs", "rejecting_records_the_outcome_and_the_reason_and_copies_no_proposal_text"),
            ("crates/delphi-engine/src/promote.rs", "withdrawing_retires_the_entry_with_an_audit_reference_and_deletes_nothing"),
            ("crates/delphi-engine/src/service.rs", "a_promotion_over_rpc_is_console_only_cas_protected_and_withdrawable"),
        ],
    },
    Site {
        name: "the checkpoint summary (`specs/030` §4.3): no model text for a task that read content that is not validated",
        files: &["crates/delphi-engine/src/checkpoint.rs"],
        events: &[],
        proofs: &[("crates/delphi-engine/src/checkpoint.rs", "the_models_two_summaries_go_in_when_there_are_some_and_a_tainted_task_gets_none")],
    },
];

fn production(text: &str) -> &str {
    text.find("#[cfg(test)]").map_or(text, |i| &text[..i])
}

/// A write of the `staged` zone's files: a path join naming `local/staged` (other than a read by the indexer) in a file that also writes files.
fn writes_staged_zone(text: &str) -> bool {
    let writes = text.contains("fs::write(") || text.contains("File::create(");
    writes && production(text).lines().any(|l| !l.trim_start().starts_with("//") && l.contains(".join(\"local/staged\")") && !l.contains("md_files("))
}

/// Findings for `files` (repo-relative path, text) of the workspace's Rust sources. Empty means every write path is
/// listed and still has its proof.
pub fn audit(files: &[(String, String)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (path, text) in files {
        if path.ends_with("_tests.rs") || path.contains("/tests/") || path.ends_with("memory_audit.rs") {
            continue;
        }
        let code = production(text);
        for line in code.lines().filter(|l| !l.trim_start().starts_with("//")) {
            if !(line.contains("EventEnvelope::new") || line.contains("events.push")) {
                continue;
            }
            for kind in MEMORY_EVENTS.iter().filter(|k| line.contains(&format!("\"{}\"", k))) {
                if !SITES.iter().any(|s| s.events.contains(kind) && s.files.contains(&path.as_str())) {
                    out.push((path.clone(), format!("writes `{}` but is not an audited memory write path; list it in `memory_audit::SITES` with a proof that it stamps taint, stays below `internal_empirical` when tainted, needs ratification to be always-loaded and is fenced (specs/050 ML-6)", kind)));
                }
            }
        }
        if writes_staged_zone(text) && !SITES.iter().any(|s| s.events.contains(&"learn.staged") && s.files.contains(&path.as_str())) {
            out.push((path.clone(), "writes the `staged` zone but is not an audited memory write path (specs/050 ML-6)".into()));
        }
    }
    for s in SITES {
        for (file, test) in s.proofs {
            match files.iter().find(|(p, _)| p == file) {
                Some((_, text)) if text.contains(&format!("fn {}(", test)) => {}
                _ => out.push((file.to_string(), format!("the proof `{}` for the memory write path `{}` is missing", test, s.name))),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_sources() -> Vec<(String, String)> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        walkdir::WalkDir::new(root.join("crates"))
            .into_iter()
            .flatten()
            .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "rs") && !e.path().components().any(|c| c.as_os_str() == "target"))
            .filter_map(|e| Some((e.path().strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/"), std::fs::read_to_string(e.path()).ok()?)))
            .collect()
    }

    #[test]
    fn every_existing_memory_write_path_is_listed_and_has_its_proof() {
        let found = audit(&repo_sources());
        assert!(found.is_empty(), "{:#?}", found);
    }

    #[test]
    fn a_new_write_path_without_the_proof_fails_and_a_removed_proof_fails() {
        let mut files = repo_sources();
        files.push(("crates/delphi-engine/src/sneaky.rs".into(), "fn f() { let e = EventEnvelope::new(m, \"learn.staged\", json!({})); }\n".into()));
        files.push(("crates/delphi-engine/src/sneakier.rs".into(), "fn f() { std::fs::write(ws.join(\"local/staged\"), \"x\"); }\n".into()));
        let found = audit(&files);
        assert!(found.iter().any(|(p, m)| p.ends_with("sneaky.rs") && m.contains("learn.staged")));
        assert!(found.iter().any(|(p, _)| p.ends_with("sneakier.rs")));
        let mut gutted = repo_sources();
        gutted.iter_mut().find(|(p, _)| p.ends_with("engine/src/tools.rs")).unwrap().1 = "fn nothing() {}".into();
        assert!(audit(&gutted).iter().any(|(_, m)| m.contains("any_role_stages_a_note_that_inherits_the_spans_taint")));
    }

    #[test]
    fn a_write_inside_test_code_is_not_a_write_path() {
        let files = vec![("crates/x/src/a.rs".to_string(), "#[cfg(test)]\nmod t { fn f() { EventEnvelope::new(m, \"learn.staged\", j); } }\n".to_string())];
        assert!(audit(&files).iter().all(|(p, _)| p != "crates/x/src/a.rs"));
    }
}
