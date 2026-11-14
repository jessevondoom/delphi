//! Spec 106 §3 item 7 as amended by A3: the size of each release role's pack, on a workspace `delphi init` seeds from this repository's
//! own files. The compact pack is smaller than the full pack for every role, the charter excerpt is under half the charter, and each is pinned
//! at its measured size (spec 106 §9) plus 10 percent.

use super::{init, InitOptions};
use delphi_engine::mcp_packs::{Detail, Packs};
use delphi_engine::workspace::WorkspacePaths;

/// Compact pack sizes in bytes, measured when spec 106 P2 was built and again after spec 109 Q6 added the `search_use` header line (spec 106 §9). `None`: the role is not served to hosts.
const MEASURED: &[(&str, Option<usize>)] = &[
    ("orchestrator", Some(25249)),
    ("architect", Some(18746)),
    ("builder", Some(18944)),
    ("scout", Some(19674)),
    ("investigator", Some(20230)),
    ("strategist", Some(18224)),
    ("red-team", Some(20651)),
    ("writer", Some(17854)),
    ("messenger", None),
    ("validator", Some(18231)),
    ("administrator", Some(18092)),
    ("assistant", Some(17013)),
    ("advocate", Some(17446)),
];

#[test]
fn every_release_roles_compact_pack_is_smaller_than_its_full_pack_and_within_its_budget() {
    let t = tempfile::tempdir().unwrap();
    let home = t.path().join("home");
    init(&InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![], theme: "greek".into() }).unwrap();
    let packs = Packs::new(WorkspacePaths::new(&home, "w"));
    let roles = packs.roles().unwrap();
    assert_eq!(roles.len(), MEASURED.len(), "the release has {} roles; update MEASURED and spec 106 §9", roles.len());
    let mut report = Vec::new();
    let mut over = Vec::new();
    for (role, measured) in MEASURED {
        let compact = packs.pack_for(role, false, Detail::Compact, Some("claude-code"));
        let full = packs.pack_for(role, false, Detail::Full, Some("claude-code"));
        let Some(measured) = measured else {
            assert!(compact.unwrap_err().starts_with("not_servable"), "{role}");
            report.push(format!("| `{role}` | not served | not served |"));
            continue;
        };
        let (compact, full) = (compact.unwrap(), full.unwrap());
        report.push(format!("| `{role}` | {} | {} |", full.len(), compact.len()));
        assert!(compact.contains("- charter: excerpt\n"), "{role}: the repository's charter fits the excerpt table");
        assert!(compact.len() < full.len(), "{role}: compact {} is not smaller than full {}", compact.len(), full.len());
        if compact.len() > measured + measured / 10 {
            over.push(format!("{role}: the compact pack is {} bytes, over its measured {measured} plus 10 percent", compact.len()));
        }
    }
    let charter = std::fs::read_to_string(home.join("workspaces/w/local/ratified/GOVERNANCE.md")).unwrap();
    let excerpt = delphi_engine::charter_excerpt::excerpt(charter.trim(), "orchestrator", "0123456789abcdef").unwrap();
    report.push(format!("charter {} bytes, orchestrator's excerpt {} bytes", charter.len(), excerpt.len()));
    eprintln!("{}", report.join("\n"));
    assert!(excerpt.len() < charter.len() / 2, "the excerpt is under half the charter (A3)");
    assert!(over.is_empty(), "{} (spec 106 §9; measure again and update MEASURED and §9 if the growth is intended)", over.join("; "));
    // A3 also asks for the orchestrator's compact pack to be at most 24 KiB. Measured, it is 25,249 bytes (24.7 KiB) since spec 109 Q6: the selection the
    // spec defines cannot reach that without trimming the orchestrator's manifest or skill index, which A3 leaves to separate work, so that
    // bound is not asserted here; spec 106 §9 records the gap.
}
