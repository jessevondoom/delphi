//! An agent reaches Delphi's functions only through the tool registry. None of the console's Xenia methods may become a
//! tool by accident: the only Xenia tool is the Green `xenia_search` over L2 summaries.

#[test]
fn no_agent_tool_reaches_the_xenia_surfaces_or_the_signing_actions() {
    let reg = crate::tools::Registry::builtin();
    for write in [false, true] {
        let names: Vec<String> = reg.defs(write).into_iter().map(|d| d.name).collect();
        let xenia: Vec<&String> = names.iter().filter(|n| n.contains("xenia")).collect();
        // `xenia_help` is fixed catalog text with no state, store or path argument (tools.rs tests pin that).
        assert_eq!(xenia, vec!["xenia_search", "xenia_help"], "{names:?}");
    }
    for banned in [
        "xenia.status", "xenia.quarantine", "xenia.attest.prepare", "xenia.attest.decide", "xenia_status", "xenia_quarantine", "xenia_attest", "xenia_endorse", "xenia_publish", "xenia_sync", "xenia_approve",
    ] {
        assert!(reg.get(banned).is_none(), "{banned} must not be a tool");
    }
}

/// X10 gap: the install-owned trust fields (`local/bundles.lock`: the raise, the trusted-source grants) are written only by the
/// operator's commands (`delphi bundle ...`). No production code of the engine, which every agent seat, tool and the narrator's
/// data runs through, may write the lock or touch a trust field; the path guard and the tool registry deny the file by every
/// spelling (their own tests). This reads the sources so a new writer is a red build, not a surprise.
#[test]
fn no_production_code_of_the_engine_or_the_narrator_writes_the_bundle_lock_or_a_trust_field() {
    let here = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(here.join("src")).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "rs")).collect();
    for f in ["observer.rs", "hooks_cmd.rs", "status.rs"] {
        files.push(here.join("../delphi/src").join(f));
    }
    let mut checked = 0;
    for p in files {
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if name.ends_with("_tests.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let production = text.split("#[cfg(test)]").next().unwrap_or("");
        checked += 1;
        for word in ["bundles::write", "bundles::update", "bundle_trust::apply", "raise_trust", "reset_trust", "trusted_source.push", "trusted_source.retain", "trust_hash =", "trust_kid ="] {
            assert!(!production.contains(word), "{name}: production code mentions `{word}`");
        }
    }
    assert!(checked > 20, "the scan found the engine sources ({checked})");
}
