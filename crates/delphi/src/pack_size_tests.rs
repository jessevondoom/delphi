//! Spec 106 §3 item 7 as amended by A3: the size of each release role's pack, on a workspace `delphi init` seeds from this repository's
//! own files. The compact pack is smaller than the full pack for every role, the charter excerpt is under half the charter, and each is pinned
//! at its measured size (spec 106 §9) plus 10 percent.

use super::{init, InitOptions};
use delphi_engine::mcp_packs::{Detail, Packs};
use delphi_engine::workspace::WorkspacePaths;

/// Compact pack sizes in bytes, measured when spec 106 P2 was built, after spec 109 Q6 added the `search_use` header line, and after G1's markers and names-only skill index (spec 106 §9). `None`: the role is not served to hosts.
const MEASURED: &[(&str, Option<usize>)] = &[
    ("orchestrator", Some(21035)),
    ("architect", Some(17524)),
    ("builder", Some(17562)),
    ("scout", Some(18898)),
    ("investigator", Some(19291)),
    ("strategist", Some(20116)), // measured again for `specs/124` S3: it gained the source ladder
    ("red-team", Some(19676)),
    ("writer", Some(16744)),
    ("messenger", None),
    ("validator", Some(17923)),
    // Measured again for `specs/124` S2: the three roles of `specs/067` gained the source ladder (about 2.4 KB each). Spec 106 §9's
    // table still shows the sizes before it; spec 124 amends no spec by text.
    ("administrator", Some(19680)),
    ("assistant", Some(19304)),
    ("advocate", Some(19763)),
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
    // A3 as amended by spec 106 §11 G1: the orchestrator's compact pack is at most 24 KiB.
    let orchestrator = packs.pack_for("orchestrator", false, Detail::Compact, Some("claude-code")).unwrap();
    // The marked blocks are gone from the compact pack and the host-facing "Outside runs" guidance stays; the full pack keeps all of it.
    let full = packs.pack_for("orchestrator", false, Detail::Full, Some("claude-code")).unwrap();
    for gone in ["name_thread", "inspect_system", "`timers`", "`rationale` says what happened", "<!-- inside-only", "<!-- /inside-only"] {
        assert!(!orchestrator.contains(gone), "the compact pack still has `{gone}`");
    }
    assert!(!full.contains("<!-- inside-only") && !full.contains("<!-- /inside-only"), "the full pack carries no marker");
    assert!(orchestrator.contains("**Outside runs**") && orchestrator.contains("`suggest_rule` tool, one call per rule. The operator ratifies each"));
    assert!(full.contains("name_thread") && full.contains("`timers`") && full.contains("`rationale` says what happened"));
    // Names only in a compact skill index, with the line that says where the descriptions are; the full pack's index is unchanged.
    assert!(orchestrator.contains("- task-decomposition\n") && !orchestrator.contains("- task-decomposition:") && orchestrator.contains("(`list_skills` gives each skill's description.)"));
    assert!(full.contains("- task-decomposition: ") && !full.contains("`list_skills` gives each"));
    assert!(orchestrator.len() <= 24_576, "the orchestrator's compact pack is {} bytes, over 24,576 (spec 106 §11 G1)", orchestrator.len());
}
