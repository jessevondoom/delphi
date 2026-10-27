//! The admin approval card (spec 011 §3.2, §4.1; plan milestone X8b): what an admin reads before a roster or a
//! freshness statement is signed with their key.
//!
//! The rules are the member card's ([`crate::xenia_action::card`]): the exact bytes that will be signed, shown with
//! every control, bidi, zero-width and tag character as a `\u{..}` escape; the text of each signing domain; **every**
//! change to the set of people who can sign, with a warning for each flag; and a short approval code taken from the
//! digest that covers all of it. Nothing here is a decision: the answer comes from an [`AdminApprover`], which the
//! CLI implements with a terminal on stdin and stdout and a typed code.
//!
//! Everything printed that came from a proposal branch (handles, persons, details) goes through [`inline`], because
//! a hostile branch chose it.

use crate::xenia_action::escape;
use crate::xenia_admin::{Action, AdminPreview};

/// The most affected paths the card lists; the rest are counted.
const MAX_AFFECTED_SHOWN: usize = 60;

/// One line of text made safe: escapes as [`escape`], and the newline too.
pub fn inline(s: &str) -> String {
    escape(s.as_bytes()).replace('\n', "\\n")
}

/// The code the admin types: the first 12 hex digits of the preview's digest.
pub fn code(p: &AdminPreview) -> String {
    p.digest.trim_start_matches("blake3:").chars().take(12).collect()
}

fn title(p: &AdminPreview) -> String {
    let scope = format!("{}/{}", inline(&p.org), inline(&p.store));
    match p.action {
        Action::Genesis if p.files.len() > 1 => format!("Create the store {scope}: publish the founding roster (epoch 1) and its first freshness statement"),
        Action::Genesis => format!("Sign the founding roster of {scope} (epoch 1) as a founder"),
        Action::SignEpoch => format!("Sign roster epoch {} of {scope}", p.epoch),
        Action::Publish => format!("Publish roster epoch {} of {scope} with a new freshness statement", p.epoch),
        Action::Refresh => format!("Refresh the freshness statement of {scope} (roster epoch {})", p.epoch),
        Action::SignRewrite => format!("Sign a HISTORY REWRITE statement for {scope} (roster epoch {})", p.epoch),
        Action::PublishRewrite => format!("Publish a HISTORY REWRITE statement for {scope} (roster epoch {})", p.epoch),
    }
}

/// The card, as text. It shows everything that will be signed; there is nothing signed that it does not show.
pub fn card(p: &AdminPreview) -> String {
    let mut o = String::new();
    o.push_str(&format!("XENIA ADMIN  {}\n", title(p)));
    o.push_str(&format!("signing key: {}\n", inline(&p.signer)));
    if let Some(h) = &p.store_head {
        o.push_str(&format!("store head the statement names: {}\n", &inline(h)[..h.len().min(12)]));
    }
    if let Some(e) = &p.expires_at {
        o.push_str(&format!("statement expires: {}\n", inline(e)));
    }
    if let Some(r) = &p.rewrite {
        o.push_str("\nHISTORY REWRITE (spec 011 section 8.4)\n");
        o.push_str(&format!("  rewritten head (the host serves it; the statement names it as store_head): {}\n", inline(&r.rewritten)));
        o.push_str(&format!("  replaced head (rewrite_of; the head this install holds):                  {}\n", inline(&r.replaced)));
        o.push_str(&format!("  {} violation(s) of the store's history rules lie between the two\n", r.rule_findings));
        o.push_str(&format!("  {} signed file(s) would be deleted, reverted or reassigned for an install holding the replaced head:\n", r.affected.len()));
        for (why, path) in r.affected.iter().take(MAX_AFFECTED_SHOWN) {
            o.push_str(&format!("    {:<24} {}\n", inline(why), inline(path)));
        }
        if r.affected.len() > MAX_AFFECTED_SHOWN {
            o.push_str(&format!("    ... and {} more (every install lists all of them)\n", r.affected.len() - MAX_AFFECTED_SHOWN));
        }
        o.push_str("  Every install will list every affected path and require its own operator to type the commit and the count before it accepts anything.\n");
        o.push_str("  A majority of the admin people must sign these exact bytes; one signature, or a signature from a non-admin, publishes nothing.\n");
        o.push_str("  Signing does not accept the rewrite for anyone, and it does not clear any install's sticky state or pinned roster hashes.\n");
    }
    if !p.walked.is_empty() {
        o.push_str("history verified before this card (explicit walk, nothing unverified is signed over):\n");
        for (base, n) in &p.walked {
            o.push_str(&format!("  from {} : {n} commit(s)\n", inline(base)));
        }
    }
    if let Some(q) = &p.quorum {
        o.push_str(&format!("signatures: {} of {} needed, {}{}\n", q.have, q.need, if q.met { "quorum met" } else { "quorum NOT met" }, if q.people.is_empty() { String::new() } else { format!(" (signed: {})", q.people.iter().map(|x| inline(x)).collect::<Vec<_>>().join(", ")) }));
    }
    if p.changes.is_empty() {
        o.push_str("\nChanges to the signer set: none\n");
    } else {
        o.push_str("\nChanges to the signer set (who can sign, endorse or withdraw):\n");
        for c in &p.changes {
            o.push_str(&format!("  {}  {}  person {}  handle {}  {}\n", c.kind.slug(), inline(&c.did), inline(&c.person), inline(&c.handle), inline(&c.detail)));
        }
    }
    if !p.flags.is_empty() {
        o.push_str("\nWarnings (these do not block; signing means you read them):\n");
        for f in &p.flags {
            o.push_str(&format!("  WARNING {}: {}\n", f.slug(), f.text()));
        }
    }
    for f in &p.files {
        o.push_str(&format!("\n=== {} ({} bytes) ===\n", inline(&f.path), f.bytes.len()));
        o.push_str(&escape(&f.bytes));
        if !f.bytes.ends_with(b"\n") {
            o.push('\n');
        }
        o.push_str("=== end ===\n");
    }
    if p.signs.is_empty() {
        o.push_str("\nYour key signs nothing new here: this publishes signatures that other admins already made over the bytes above.\n");
    } else {
        o.push_str("\nYour key will sign exactly these strings (domain-separated, so the signature is good for nothing else):\n");
    }
    for s in &p.signs {
        o.push_str(&format!("  {}\n", inline(s)));
    }
    for r in &p.reminders {
        o.push_str(&format!("\nNote ({}): {}\n", r.slug(), r.text()));
    }
    o.push_str(&format!("\nApproval code: {}   (covers every byte, signer and domain above)\n", code(p)));
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_escapes_what_a_hostile_branch_could_use() {
        assert_eq!(inline("a\u{1b}[31mb"), "a\\u{1b}[31mb");
        assert_eq!(inline("x\ny"), "x\\ny");
        assert_eq!(inline("a\u{202e}b\u{200b}c"), "a\\u{202e}b\\u{200b}c");
    }

    #[test]
    fn a_card_built_from_hostile_text_has_no_raw_control_bidi_or_zero_width_character() {
        use crate::xenia_admin::{AdminPreview, PreviewFile, QuorumView};
        use delphi_xenia::admin::{Change, ChangeKind, Flag};
        let evil = "x\u{1b}[2J\u{202e}\u{200b}\u{e0041}\r\u{7}y";
        let p = AdminPreview {
            action: Action::Publish,
            org: evil.into(),
            store: "s".into(),
            epoch: 2,
            signer: evil.into(),
            files: vec![PreviewFile { path: evil.into(), bytes: format!("{{\"a\":\"{evil}\"}}").into_bytes() }],
            signs: vec![evil.into()],
            changes: vec![Change { kind: ChangeKind::Added, did: evil.into(), person: evil.into(), handle: evil.into(), detail: evil.into() }],
            flags: vec![Flag::AddsAdmin],
            reminders: vec![],
            quorum: Some(QuorumView { met: false, have: 1, need: 2, people: vec![evil.into()] }),
            store_head: Some("0123456789abcdef0123".into()),
            expires_at: Some(evil.into()),
            walked: vec![(evil.into(), 3)],
            rewrite: None,
            digest: "blake3:0123456789abcdef".into(),
        };
        let text = card(&p);
        for c in text.chars() {
            let bad = c != '\n' && (c.is_control() || ('\u{200b}'..='\u{200f}').contains(&c) || ('\u{202a}'..='\u{202e}').contains(&c) || ('\u{2066}'..='\u{2069}').contains(&c) || ('\u{e0000}'..='\u{e007f}').contains(&c));
            assert!(!bad, "raw {:?} in the card", c);
        }
        assert!(text.contains("WARNING adds-admin") && text.contains("Approval code: 0123456789ab"));
    }
}
