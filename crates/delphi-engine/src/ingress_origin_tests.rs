//! Spec 079 S1: every ingress that returns outside content has a core-assigned origin, and the ceiling core assigns it
//! does not exceed that origin's tier in the lattice (`specs/001` §5; GOVERNANCE §4.3). Deterministic: no model, no
//! planted content, no clock.
//!
//! This test only READS the mapping. It never sets or defaults an origin or a ceiling, a tool's own tag is never an
//! input, and a missing origin or ceiling is a failure. There is no single ingress registry in the code base, so the
//! ingress points are enumerated here from the code that dispatches them, and each ceiling is read from that code.
//!
//! File ingress: by calling `delphi_core::provenance::origin_default` for every `Zone` (an exhaustive `match`, so a
//! new zone does not compile until it is classified here) and for the untrusted file trees.
//!
//! Xenia reads: by calling `AgentRead::ceiling`.
//!
//! Tool ingress (MCP host, web tools, connector results, stored payloads, library claims, project layer): by
//! reading the `observe_input(Tier::Lx)` literal at the dispatching site in the engine source. A site that no
//! longer passes a literal tier (or has lost its `observe_input`) has no readable ceiling and fails.
//!
//! Not enumerable here: see the PR description (kb_search's inline `source_type` map; a federated-peer path other than
//! Xenia, which the code does not have).

use crate::xenia_read::AgentRead;
use delphi_core::provenance::{origin_default, OriginRules, Tier, Zone};

/// An origin class and the highest tier that origin may be given in the lattice. This is the lattice side of the
/// comparison (spec 001 §5, spec 007 §3); the ceilings being checked come from core, never from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    WebTool,
    McpHost,
    ConnectorResult,
    StoredPayload,
    LibraryClaim,
    ProjectLayer,
    XeniaRead,
    XeniaRecord,
    Bundle,
    Team,
    UntrustedFileTree,
    ImportedSummary,
    TaintedMissionOutput,
}

impl Origin {
    fn max_tier(self) -> Tier {
        match self {
            Origin::WebTool | Origin::McpHost | Origin::ConnectorResult | Origin::StoredPayload | Origin::LibraryClaim | Origin::ProjectLayer => Tier::L0,
            Origin::UntrustedFileTree | Origin::ImportedSummary | Origin::TaintedMissionOutput => Tier::L0,
            // An unstamped Xenia record is not trusted at all; a read of stamped ones is federated_verified at best.
            Origin::XeniaRecord => Tier::L0,
            Origin::XeniaRead | Origin::Bundle | Origin::Team => Tier::L2,
        }
    }
}

/// One ingress and what core assigned it. `None` means core has no origin or no readable ceiling for it.
#[derive(Debug, Clone)]
struct Ingress {
    name: String,
    origin: Option<Origin>,
    ceiling: Option<Tier>,
}

/// The findings: each ingress with no known origin, no ceiling, or a ceiling above its origin's tier. Empty means pass.
fn audit(list: &[Ingress]) -> Vec<String> {
    let mut bad = Vec::new();
    for i in list {
        match (i.origin, i.ceiling) {
            (None, _) => bad.push(format!("{}: no known origin", i.name)),
            (Some(o), None) => bad.push(format!("{}: origin {:?} has no core-assigned ceiling", i.name, o)),
            (Some(o), Some(c)) if c > o.max_tier() => bad.push(format!("{}: ceiling {} is above the {} tier of origin {:?}", i.name, c, o.max_tier(), o)),
            _ => {}
        }
    }
    bad
}

/// The `Tier::Lx` literal the first `observe_input(` after `anchor` passes. `None` if the anchor or the call is gone, or
/// the argument is not a literal tier (the ceiling is then not readable).
fn site_ceiling(src: &str, anchor: &str) -> Option<Tier> {
    let after = &src[src.find(anchor)? + anchor.len()..];
    let call = after.find("observe_input(")? + "observe_input(".len();
    let arg = &after[call..];
    let lit = arg.strip_prefix("delphi_core::provenance::Tier::").or_else(|| arg.strip_prefix("Tier::"))?;
    Tier::parse(lit.get(..2)?)
}

const HARNESS: &str = include_str!("harness_run.rs");
const ENGINE: &str = include_str!("engine.rs");
const CONNECTOR: &str = include_str!("connector_run.rs");
const TOOLS: &str = include_str!("tools.rs");

/// Every zone, classified. No wildcard arm: a new `Zone` variant does not compile until it is listed.
fn zone_ingress(z: Zone) -> Vec<Ingress> {
    let rules = OriginRules::with_defaults();
    let of = |name: &str, origin: Origin, zone: Zone, path: &str| Ingress { name: name.into(), origin: Some(origin), ceiling: Some(origin_default(zone, path, &rules, true)) };
    match z {
        // Local, ratified or operator-owned content: not outside content, so no outside origin is required.
        Zone::Release | Zone::Local | Zone::Worktree => vec![],
        Zone::Project => ["vendor/x.rs", "third_party/x.rs", "node_modules/x/index.js"].iter().map(|p| of(&format!("untrusted file tree {}", p), Origin::UntrustedFileTree, Zone::Project, p)).collect(),
        Zone::Team => vec![of("team zone", Origin::Team, Zone::Team, "context/a.md")],
        Zone::Bundle => vec![of("added org bundle", Origin::Bundle, Zone::Bundle, "a.md")],
        Zone::Xenia => vec![of("xenia record without a stamp", Origin::XeniaRecord, Zone::Xenia, "r.md")],
    }
}

const ALL_ZONES: [Zone; 7] = [Zone::Release, Zone::Team, Zone::Local, Zone::Project, Zone::Worktree, Zone::Bundle, Zone::Xenia];

fn local_extras() -> Vec<Ingress> {
    let rules = OriginRules::with_defaults();
    let local = |name: &str, origin: Origin, path: &str| Ingress { name: name.into(), origin: Some(origin), ceiling: Some(origin_default(Zone::Local, path, &rules, true)) };
    vec![
        local("imported summary page", Origin::ImportedSummary, "summaries/x.md"),
        local("mission research output", Origin::TaintedMissionOutput, "missions/m/research/x.md"),
        local("mission reports output", Origin::TaintedMissionOutput, "missions/m/reports/x.md"),
        local("mission artifacts output", Origin::TaintedMissionOutput, "missions/m/artifacts/x.md"),
    ]
}

fn xenia_read() -> Ingress {
    let read = AgentRead { envelopes: vec!["<record/>".into()], ids: vec!["r".into()] };
    Ingress { name: "xenia agent read".into(), origin: Some(Origin::XeniaRead), ceiling: read.ceiling() }
}

fn tool_sites(harness: &str, engine: &str, connector: &str, tools: &str) -> Vec<Ingress> {
    let site = |name: &str, origin: Origin, src: &str, anchor: &str| Ingress { name: name.into(), origin: Some(origin), ceiling: site_ceiling(src, anchor) };
    vec![
        site("claude connector tool (mcp__*)", Origin::McpHost, harness, "n if n.starts_with(\"mcp__\") =>"),
        site("agy web tools (search_web, read_url_content)", Origin::WebTool, harness, "async fn taint_if_read_outside"),
        site("native connector result", Origin::ConnectorResult, connector, "\"connector.result\""),
        site("stored connector payload (@payload/)", Origin::StoredPayload, tools, "logical.starts_with(\"@payload/\")"),
        site("library claim pointer", Origin::LibraryClaim, engine, "starts_with(crate::library::claims::POINTER_PREFIX)"),
        site("project layer", Origin::ProjectLayer, engine, "crate::scope::prompt_layer("),
    ]
}

fn real_ingress() -> Vec<Ingress> {
    let mut v: Vec<Ingress> = ALL_ZONES.iter().flat_map(|z| zone_ingress(*z)).collect();
    v.extend(local_extras());
    v.push(xenia_read());
    v.extend(tool_sites(HARNESS, ENGINE, CONNECTOR, TOOLS));
    v
}

#[test]
fn every_current_ingress_has_an_origin_and_a_ceiling_within_its_tier() {
    let list = real_ingress();
    assert!(list.len() >= 14, "the enumeration shrank: {}", list.len());
    let bad = audit(&list);
    assert!(bad.is_empty(), "ingress points with no known origin or an over-high ceiling:\n{}", bad.join("\n"));
}

#[test]
fn a_fixture_ingress_with_no_origin_fails_and_is_named() {
    let mut list = real_ingress();
    list.push(Ingress { name: "fixture: new_ingress_tool".into(), origin: None, ceiling: Some(Tier::L0) });
    let bad = audit(&list);
    assert_eq!(bad.len(), 1, "{:?}", bad);
    assert!(bad[0].contains("fixture: new_ingress_tool") && bad[0].contains("no known origin"), "{:?}", bad);
}

#[test]
fn a_self_declared_tag_is_never_read_the_ceiling_follows_the_origin() {
    // A fixture tool declares itself L5 (user_ratified) but arrives from an untrusted origin.
    struct FixtureTool {
        name: &'static str,
        declared_tag: Tier,
        origin: Origin,
    }
    // The fixture's stand-in for core's mapping: a function of the origin only.
    let core_mapping = |o: Origin| Some(o.max_tier());
    let tool = FixtureTool { name: "fixture low-risk tool", declared_tag: Tier::L5, origin: Origin::McpHost };
    let ing = Ingress { name: tool.name.into(), origin: Some(tool.origin), ceiling: core_mapping(tool.origin) };
    assert_eq!(ing.ceiling, Some(Tier::L0), "the ceiling is the origin's, not the declared {}", tool.declared_tag);
    assert!(audit(&[ing]).is_empty());
    // Had the declared tag been used as the ceiling, the audit would have caught it.
    let wrong = Ingress { name: tool.name.into(), origin: Some(tool.origin), ceiling: Some(tool.declared_tag) };
    assert_eq!(audit(&[wrong]).len(), 1);
}

#[test]
fn removing_a_real_ingress_mapping_fails() {
    // The MCP host site and the agy web site each lose their observe_input call.
    let no_taint = HARNESS.replace("observe_input(Tier::L0)", "observe_other(Tier::L0)");
    let bad = audit(&tool_sites(&no_taint, ENGINE, CONNECTOR, TOOLS));
    assert!(bad.iter().any(|b| b.contains("claude connector tool") && b.contains("no core-assigned ceiling")), "{:?}", bad);
    assert!(bad.iter().any(|b| b.contains("agy web tools")), "{:?}", bad);
    // An anchor that has vanished (the dispatch moved or was renamed) is also a failure, never a default.
    let bad = audit(&tool_sites(HARNESS, "", CONNECTOR, TOOLS));
    assert!(bad.iter().any(|b| b.contains("library claim pointer")) && bad.iter().any(|b| b.contains("project layer")), "{:?}", bad);
}

#[test]
fn a_mapping_above_its_lattice_tier_fails() {
    let raised = HARNESS.replace("observe_input(Tier::L0)", "observe_input(Tier::L3)");
    let bad = audit(&tool_sites(&raised, ENGINE, CONNECTOR, TOOLS));
    assert!(bad.iter().any(|b| b.contains("claude connector tool") && b.contains("above")), "{:?}", bad);
    assert!(bad.iter().any(|b| b.contains("agy web tools") && b.contains("above")), "{:?}", bad);
    // A zone ceiling above its tier, by value.
    let bad = audit(&[Ingress { name: "fixture bundle".into(), origin: Some(Origin::Bundle), ceiling: Some(Tier::L3) }]);
    assert_eq!(bad.len(), 1);
}

#[test]
fn a_non_literal_ceiling_is_unreadable_and_fails() {
    let dynamic = HARNESS.replace("observe_input(Tier::L0)", "observe_input(some_tier)");
    let bad = audit(&tool_sites(&dynamic, ENGINE, CONNECTOR, TOOLS));
    assert!(bad.iter().any(|b| b.contains("claude connector tool") && b.contains("no core-assigned ceiling")), "{:?}", bad);
}
