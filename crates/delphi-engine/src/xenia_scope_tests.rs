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
