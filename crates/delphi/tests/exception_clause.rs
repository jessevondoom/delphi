//! Spec 092 S2d2b: the one exception to "Red actions always ask" is written the same way everywhere it is stated. The sentences below are the
//! wording the operator approved ("as-is", 2026-11-08); a change to any of them in one place and not the others fails here. This test checks
//! that the files agree. It does not, and cannot, show that the operator ratified them: the ratification is recorded in the `amendments:` of
//! `specs/092`, by the person who merges.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The sentence of AGENTS.md and CONTRIBUTING.md.
const RULE: &str = "Red actions always ask the operator, live or, in a headless run only, through an authorization the operator ratified for that run (`specs/092`: one sandboxed `/bin/bash <pinned script>` kind only; an agent may request one, never grant one).";

/// The clause spec 092 AC-5 quotes: it must be the first clause of `RULE`, apart from the verb ("asked of" against "always ask").
const CLAUSE_TAIL: &str = "live or, in a headless run only, through an authorization the operator ratified for that run";

/// The text of invariant 1 of spec 016 section 3.
const INVARIANT_1: &str = "**Red is never allowed by a policy, and in a headless run only by an operator authorization.** A card of the Red tier is denied whatever `policy:<file>` or `relay` says. The one exception is `--on-card authorization:<id> --auth-run <run>` (`specs/092`): a `run_command` card that exactly matches an entry in a record the operator created, with step-up, for this run, home and machine (unexpired, unrevoked, use-counted), and that runs as `/bin/bash <pinned script>` inside the verified OS sandbox, network off, over a read-only snapshot, where such a sandbox exists (macOS today; elsewhere the run refuses). This exception covers that one action kind only; any other kind needs a new amendment. Merges, pull requests, credentials, network, dispatch, and writes to `.git`, hooks, configuration or the authorization store are never covered. An agent may request an authorization, never create or approve one, and a request is never effective in the run that wrote it. A timeout or a missing answer is never an approval. Every use is logged as `authorization.used`.";

#[test]
fn agents_md_and_contributing_md_state_the_exception_in_the_same_words() {
    for f in ["AGENTS.md", "CONTRIBUTING.md"] {
        let t = read(f);
        assert!(t.contains(RULE), "{f} does not carry the approved sentence");
        assert_eq!(t.matches("Red actions always ask").count(), 1, "{f}: the rule is stated once, with its exception");
        assert!(!t.contains("Red actions always ask."), "{f}: an unqualified `Red actions always ask.` is left");
    }
}

#[test]
fn spec_016_section_3_carries_invariant_1_as_approved_and_its_three_companion_edits() {
    let t = read("specs/016-headless-run.md");
    assert!(t.contains(&format!("1. {INVARIANT_1}")), "invariant 1 differs from the approved text");
    assert!(t.contains("or approve a Red action except under a `specs/092` authorization (§3, invariant 1)."), "the section 1.1 non-goal");
    assert!(t.contains("**Red and merge cards are never allowed under any policy**; a Red card is allowed only through a matching authorization;"), "the tests line");
    assert!(t.contains("| `authorization:<id>` + `--auth-run <run>` | Only a matching Red `run_command` is approved (invariant 1); every other card is denied. |"), "the section 3 table row");
}

#[test]
fn spec_092_ac_5_quotes_the_clause_that_agents_md_states() {
    let ac5 = read("specs/092-operator-authorized-tests.md");
    let line = ac5.lines().find(|l| l.starts_with("- **AC-5**")).expect("AC-5");
    assert!(line.contains(&format!("Red actions are asked of the operator, {CLAUSE_TAIL}.")), "AC-5 no longer quotes the approved clause");
    assert!(RULE.contains(CLAUSE_TAIL) && read("AGENTS.md").contains(CLAUSE_TAIL) && read("CONTRIBUTING.md").contains(CLAUSE_TAIL));
}

#[test]
fn the_four_places_name_the_same_kind_the_same_runner_and_the_same_limits() {
    let agents = read("AGENTS.md");
    let contributing = read("CONTRIBUTING.md");
    let s016 = read("specs/016-headless-run.md");
    // the one runner and the one kind
    for (f, t) in [("AGENTS.md", &agents), ("CONTRIBUTING.md", &contributing), ("specs/016", &s016)] {
        assert!(t.contains("`/bin/bash <pinned script>`"), "{f} does not name the runner");
        assert!(t.contains("`specs/092`"), "{f} does not name the spec");
    }
    assert!(agents.contains("an agent may request one, never grant one") && contributing.contains("an agent may request one, never grant one"));
    assert!(s016.contains("An agent may request an authorization, never create or approve one"));
    // the operator's words, kept where the operator is asked to read them
    assert!(read("docs/guides/headless-and-ci.md").contains("Only a Red `run_command` that matches the record is approved, as a pinned `/bin/bash` script in the sandbox; every other card, a merge included, is denied."));
    assert!(read("docs/features.md").contains("(Red only through a spec 092 authorization, never a merge)"));
    assert!(read("docs/reference/console.md").contains("authorization:<id> --auth-run <run>"));
    // the `--on-card` help (and the generated reference built from it)
    let help = "`--on-card authorization:<id> --auth-run <run>` (spec 092): approves only a Red `run_command` that exactly matches the record's entry, as `/bin/bash <pinned script>` in the sandbox, network off (macOS); every other card is denied.";
    // the help wraps the sentence over lines for the terminal; the words are the approved ones
    let flat = |s: &str| s.replace("\\n", " ").split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(flat(&read("crates/delphi/src/main.rs")).contains(help) && flat(&read("docs/reference/cli.md")).contains(help));
    let cli = read("docs/reference/cli.md");
    assert!(!cli.contains("launch gate only") && !cli.contains("best-effort in S2a"));
}

#[test]
fn the_denial_text_agents_see_is_byte_identical() {
    // The approval is the daemon's; every card that is not approved is denied with the words it has always had.
    assert!(read("crates/delphi/src/run.rs").contains("\"headless run: a Red action is never approved without the operator\""));
    assert!(read("crates/delphi-engine/src/tools.rs").contains("a Red action is never approved without the operator") || read("crates/delphi-engine/src/engine.rs").contains("a Red action is never approved without the operator") || read("crates/delphi/src/run.rs").contains("a Red action is never approved without the operator"));
    assert!(read(&format!("crates/delphi-engine/src/{}.rs", concat!("authorization", "_grant"))).contains("The operator denied this action: authorized approval is not enabled in this build (spec 092 S2c). Do not retry it."));
}
