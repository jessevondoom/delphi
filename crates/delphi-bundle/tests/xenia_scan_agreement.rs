//! The Xenia publish gate's scan (`delphi-core::xenia_scan`) and the bundle `validate` step 8 scan
//! (`delphi-bundle::scan`) must agree on what a secret, a token, a key block and a local path are: one
//! `[REV-SEC-05]` list, two callers. `delphi-bundle` depends on `delphi-core`, so the agreement test lives here.

use delphi_bundle::scan::{scan_text, Context};
use delphi_core::xenia_scan::{self, ScanContext};

/// `(text, the class both scanners must report)`. Classes the bundle scan reports as failures and the gate blocks.
const CANARIES: [(&str, &str); 9] = [
    ("the key is sk-ant-abcdefghijklmnopqrstuvwxyz0123456789", "secret"),
    ("use ghp_abcdefghijklmnopqrstuvwxyz0123456789 for CI", "token"),
    ("AKIAABCDEFGHIJKLMNOP is the access key", "secret"),
    ("xoxb-1234567890-abcdefghij is a slack token", "token"),
    ("-----BEGIN OPENSSH PRIVATE KEY-----", "private-key"),
    ("see /Users/jane/work/notes.md", "local-path"),
    ("see /home/deploy/app/config", "local-path"),
    (r"see C:\Users\jane\notes", "local-path"),
    ("export STRIPE_API_KEY=whatever123", "secret"),
];

#[test]
fn both_scanners_find_every_canary_and_neither_finds_prose() {
    for (text, class) in CANARIES {
        let bundle = scan_text(text, &Context::default());
        let gate = xenia_scan::scan_text("summary", text, &ScanContext::default());
        assert!(bundle.iter().any(|h| h.fail), "the bundle scan missed {text:?}");
        assert!(!gate.passed(), "the publish scan missed {text:?}");
        // Same family: a secret-like finding from one is secret-like from the other.
        let family = |c: &str| match c {
            "secret" | "token" | "private-key" => "credential",
            other => if other == "local-path" { "local-path" } else { "other" },
        };
        assert!(bundle.iter().any(|h| family(h.class) == family(class)), "bundle: {text:?} {bundle:?}");
        assert!(gate.blocks.iter().any(|b| family(b.class) == family(class)), "gate: {text:?} {:?}", gate.blocks);
    }
    for prose in ["Deliveries are retried with exponential backoff for up to three days.", "Use the rate limiter in crates/delphi-core/src/budget.rs", "Set the retry count to 5 and log the attempt number."] {
        assert!(scan_text(prose, &Context::default()).iter().all(|h| !h.fail), "bundle flagged {prose:?}");
        assert!(xenia_scan::scan_text("summary", prose, &ScanContext::default()).passed(), "gate blocked {prose:?}");
    }
}
