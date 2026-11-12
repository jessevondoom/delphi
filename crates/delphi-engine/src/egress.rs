//! The one place a private entry is refused (`specs/098` §12 item 16, `specs/055` §10.1). Every outbound path calls [`check`]; the
//! test at the bottom lists the registered paths and fails when a channel has none, when a wired path does not call it, or when
//! any other file calls the library's own `may_leave` directly.
//!
//! Deny by default: metadata that is not an object, or that cannot be read, is treated as private.

use serde_json::Value;

use crate::library::{is_private, may_leave, Channel};

/// May the body of this entry go out through `channel`? Only a public, readable record may.
pub fn check(meta: &Value, channel: Channel) -> Result<(), String> {
    if !meta.is_object() {
        return Err(format!("an entry whose record cannot be read is treated as private ({:?})", channel));
    }
    may_leave(meta, channel)
}

/// May this entry be placed in a seat's prompt? A private entry never goes to a foreign-family seat, whatever
/// `[library] private_to_models` says. On a seat of the host's own family the setting decides (`allow` or the default `deny`).
pub fn check_for_seat(meta: &Value, foreign_family: bool, private_to_models_allow: bool) -> Result<(), String> {
    if !meta.is_object() {
        return Err("an entry whose record cannot be read is treated as private".into());
    }
    if is_private(meta) && (foreign_family || !private_to_models_allow) {
        return Err(if foreign_family { "a private entry is never placed in the prompt of a foreign-family seat".into() } else { "a private entry is not placed in a model prompt ([library] private_to_models is not `allow`)".into() });
    }
    Ok(())
}

/// Whether a path calls [`check`] today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wired {
    /// The file calls `egress::check` with this channel.
    Yes,
    /// The channel carries no library or knowledge-base entry text, and a named test at the sender proves it (the file, relative to the
    /// `crates/` directory, and the test's name). The enumeration test below checks that the named test exists; the named test is what
    /// fails if the sender starts to carry entry text. A path added later that does carry it must be registered as `Yes`.
    NoEntryText { proof_file: &'static str, proof_test: &'static str },
    /// The path has not been audited. Said plainly: nothing below proves it carries no entry text, and a path added later must be
    /// registered as `Yes`.
    Unaudited,
}

/// Every channel of `specs/055` §10.1 and the file that sends through it.
pub const PATHS: [(Channel, &str, Wired); 13] = [
    (Channel::Bundle, "library_export.rs", Wired::Yes),
    (Channel::WebSearch, "library_acquire.rs", Wired::Yes),
    (Channel::Xenia, "", Wired::Unaudited),
    (Channel::Export, "", Wired::Unaudited),
    (Channel::DebugBundle, "", Wired::NoEntryText { proof_file: "delphi/src/logs_cmd_tests.rs", proof_test: "a_bundle_never_carries_a_noted_entrys_text" }),
    (Channel::Log, "", Wired::NoEntryText { proof_file: "delphi-core/src/elog_tests.rs", proof_test: "context_keys_never_name_a_field_that_carries_entry_text" }),
    (Channel::Relay, "", Wired::Unaudited),
    (Channel::Connector, "", Wired::Unaudited),
    (Channel::Mail, "", Wired::Unaudited),
    (Channel::Outbound, "", Wired::Unaudited),
    (Channel::Telemetry, "", Wired::Unaudited),
    (Channel::ArchiveLookup, "library_archive.rs", Wired::Yes),
    (Channel::CitationLookup, "research_host.rs", Wired::Yes),
];

/// A path that is deliberately NOT guarded by [`check`] (`specs/098` §12 item 16, §7.5 item 7a): it carries no text from any entry, so there
/// is nothing for the check to read, and its guard is structural. Listing it here, with its reason and the tests that keep the reason true,
/// makes the exemption a recorded decision and not an oversight. Each named test must exist (the enumeration test checks). If a change could
/// put entry text into the request, the exemption lapses and the path must be registered in [`PATHS`] and call [`check`].
pub struct Exempt {
    pub name: &'static str,
    /// The file that sends, relative to this crate's `src`.
    pub sender: &'static str,
    pub reason: &'static str,
    /// `(file relative to the crates directory, test name)`.
    pub proofs: &'static [(&'static str, &'static str)],
}

pub const EXEMPT: [Exempt; 1] = [Exempt {
    name: "the fetch of an observed URL (slice S1d, 7a)",
    sender: "research_host.rs",
    reason: "it sends one URL that the daemon's own provider search returned, exactly as returned, so no entry text can be in it; its guard is membership in the per-run observed set, which only the daemon's own search fills",
    proofs: &[
        ("delphi-engine/src/egress.rs", "only_the_daemons_own_search_adds_to_the_observed_set"),
        ("delphi-engine/src/research_host.rs", "a_fetched_url_is_exactly_the_one_the_provider_returned"),
        ("delphi-engine/src/research_host.rs", "only_a_url_a_search_returned_is_reachable_and_it_is_fetched_as_given"),
        ("delphi-engine/src/research_observed.rs", "no_file_of_the_job_pipeline_follows_a_link_found_in_a_page"),
        ("delphi-engine/src/research_run.rs", "a_url_a_model_wrote_never_reaches_the_fetcher"),
        ("delphi-engine/src/search_log.rs", "a_host_declared_search_carries_no_url_and_never_touches_the_observed_set"),
        ("delphi-engine/src/service.rs", "a_claim_whose_url_the_sessions_search_returned_is_refetched_public_and_raises_a_source_card"),
    ],
}];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The non-test text of a source file: everything before its last `mod tests`. (Cutting at the first `#[cfg(test)]` would hide the rest of
    /// a file that has an early `#[cfg(test)] mod x_tests;` line, and an attribute indented inside a module is not a test module.)
    fn production(src: &str) -> &str {
        let cut = src.rfind("\n#[cfg(test)]\nmod tests").or_else(|| src.rfind("\nmod tests")).unwrap_or(src.len());
        &src[..cut]
    }

    fn crate_sources() -> Vec<(String, String)> {
        fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    if !matches!(p.file_name().and_then(|n| n.to_str()), Some("target" | "tests" | "node_modules" | "fixtures")) {
                        walk(&p, out);
                    }
                } else if p.extension().is_some_and(|x| x == "rs") {
                    out.push(p);
                }
            }
        }
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").canonicalize().unwrap();
        let mut files = Vec::new();
        walk(&crates, &mut files);
        files.into_iter().map(|f| (f.strip_prefix(&crates).unwrap().to_string_lossy().replace('\\', "/"), std::fs::read_to_string(&f).unwrap())).collect()
    }

    #[test]
    fn deny_by_default_and_private_never_leaves() {
        for c in Channel::ALL {
            assert!(check(&json!({"private": true}), c).is_err());
            assert!(check(&json!("unreadable"), c).is_err());
            assert!(check(&Value::Null, c).is_err());
            assert!(check(&json!({"private": false}), c).is_ok());
        }
    }

    #[test]
    fn a_private_entry_never_reaches_a_foreign_seat() {
        let private = json!({"private": true});
        assert!(check_for_seat(&private, true, true).is_err(), "allow does not reach across families");
        assert!(check_for_seat(&private, false, false).is_err());
        assert!(check_for_seat(&private, false, true).is_ok());
        assert!(check_for_seat(&json!({"private": false}), true, false).is_ok());
    }

    /// §12 item 16: the registered paths are enumerated, and a wired one must call the check.
    #[test]
    fn every_registered_egress_path_calls_the_one_check() {
        for c in Channel::ALL {
            assert!(PATHS.iter().any(|(p, _, _)| *p == c), "channel {:?} has no registered path", c);
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for (c, file, wired) in PATHS {
            match wired {
                Wired::Yes => {
                    let text = std::fs::read_to_string(dir.join(file)).unwrap_or_else(|e| panic!("{file}: {e}"));
                    let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
                    assert!(prod.contains("egress::check(") && prod.contains(&format!("Channel::{:?}", c)), "{file} does not call egress::check for {:?}", c);
                }
                Wired::NoEntryText { proof_file, proof_test } => {
                    let text = std::fs::read_to_string(crates.join(proof_file)).unwrap_or_else(|e| panic!("{proof_file}: {e}"));
                    assert!(text.contains(&format!("fn {proof_test}(")), "{:?}: the proof test `{proof_test}` is missing from {proof_file}", c);
                }
                Wired::Unaudited => {}
            }
        }
    }

    #[test]
    fn the_unaudited_channels_are_listed_so_the_gap_is_visible() {
        let un: Vec<String> = PATHS.iter().filter(|(_, _, w)| *w == Wired::Unaudited).map(|(c, _, _)| format!("{c:?}")).collect();
        assert_eq!(un, ["Xenia", "Export", "Relay", "Connector", "Mail", "Outbound", "Telemetry"], "audit a channel, then move it out of this list");
    }

    #[test]
    fn nothing_calls_the_library_gate_directly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for f in std::fs::read_dir(&dir).unwrap().flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            if !name.ends_with(".rs") || matches!(name.as_str(), "egress.rs" | "library.rs" | "library_tests.rs") {
                continue;
            }
            let text = std::fs::read_to_string(f.path()).unwrap();
            let prod = text.split("#[cfg(test)]\nmod ").next().unwrap_or("");
            assert!(!prod.contains("may_leave("), "{name} calls may_leave directly: go through egress::check");
        }
    }

    /// An exemption is only as good as the tests that keep its reason true: every one named in [`EXEMPT`] must exist.
    #[test]
    fn every_exempt_path_names_proof_tests_that_exist() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        for e in EXEMPT.iter() {
            assert!(!e.reason.is_empty() && !e.proofs.is_empty(), "{}: an exempt path needs a reason and proofs", e.name);
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(e.sender).is_file(), "{}: the sender file {} is missing", e.name, e.sender);
            for (file, test) in e.proofs {
                let text = std::fs::read_to_string(crates.join(file)).unwrap_or_else(|er| panic!("{file}: {er}"));
                assert!(text.contains(&format!("fn {test}(")), "{}: the proof test `{test}` is missing from {file}", e.name);
            }
        }
    }

    /// `specs/098` §12 item 16, test (a): the observed set is filled only by the daemon's own provider search (`LiveWeb::remember`, called
    /// right after the provider returned its rows) and restored only from the `research.observed` events the daemon's own research job
    /// wrote. A host, a model, a claim's `source_url`, a declared search or a page's links have no way in. A new caller fails here, and a
    /// reviewer has to ask whether it can put entry text into a fetched URL.
    #[test]
    fn only_the_daemons_own_search_adds_to_the_observed_set() {
        let re = |p: &str| regex::Regex::new(p).unwrap();
        let (observe, restore, new_set, remember) = (re(r"observed\s*\.\s*observe\("), re(r"observed\s*\.\s*restore\("), re(r"Observed::new\("), re(r"\bself\s*\.\s*remember\("));
        let mut seen: std::collections::BTreeMap<String, (usize, usize, usize, usize)> = Default::default();
        for (rel, src) in crate_sources() {
            if rel.ends_with("research_observed.rs") {
                continue; // the module that defines the set
            }
            let code: String = production(&src).lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            let counts = (observe.find_iter(&code).count(), restore.find_iter(&code).count(), new_set.find_iter(&code).count(), remember.find_iter(&code).count());
            if counts != (0, 0, 0, 0) {
                seen.insert(rel, counts);
            }
        }
        let want: std::collections::BTreeMap<String, (usize, usize, usize, usize)> = [
            // the provider search: one `observe` in `remember`, which is called once, in `search`, after the provider answered; one set per `LiveWeb`
            ("delphi-engine/src/research_host.rs".to_string(), (1, 0, 1, 1)),
            // the daemon restores the set for a host's claim from the `research.observed` events its own research job wrote
            ("delphi-engine/src/service.rs".to_string(), (0, 1, 0, 0)),
        ]
        .into_iter()
        .collect();
        assert_eq!(seen, want, "something new fills the observed set: it must come only from the daemon's own provider search");
        // and the one `remember` call sits in `search`, right after the provider's answer
        let host = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/research_host.rs")).unwrap();
        let at = host.find("self.remember(n, &found);").expect("the one remember call");
        assert!(host[..at].rsplit("fn search(&self").next().is_some_and(|b| b.contains("search(self.transport")), "`remember` must follow the provider search in `search`");
    }
}
