//! Step 8 of `specs/023` §10: the `[REV-SEC-05]` secret and path scan over every text file, plus the identifiers a
//! bundle should not carry.

use crate::manifest::Manifest;
use crate::tree::{in_vectors, Tree};
use crate::{Host, Report};
use delphi_core::redact::Redactor;
use regex::Regex;

pub fn check(tree: &Tree, m: Option<&Manifest>, host: &dyn Host, r: &mut Report) {
    let redactor = Redactor::new();
    let home = Regex::new(r"(/Users/[A-Za-z0-9._-]+/|/home/[A-Za-z0-9._-]+/|[A-Za-z]:\\Users\\[A-Za-z0-9._-]+)").unwrap();
    let email = Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+").unwrap();
    let contact = m.and_then(|m| m.publisher_contact.as_deref());
    let identifiers = host.identifiers();
    let handles = host.roster_handles();
    for e in tree.entries.iter().filter(|e| !in_vectors(&e.path) && !e.path.starts_with(".delphi-bundle/")) {
        let Some(text) = tree.text(&e.path) else { continue };
        if redactor.redact_str(&text).1 {
            r.fail(8, &e.path, "contains a secret (an API key, token or private key)");
        }
        if let Some(mat) = home.find(&text) {
            r.fail(8, &e.path, format!("contains a local user path (`{}…`)", mat.as_str()));
        }
        for mat in email.find_iter(&text) {
            if Some(mat.as_str()) != contact {
                r.fail(8, &e.path, "contains an e-mail address other than publisher.contact");
                break;
            }
        }
        for id in identifiers.iter().filter(|i| !i.is_empty()) {
            if text.contains(id.as_str()) {
                r.warn(8, &e.path, format!("contains the workspace identifier `{}`", id));
            }
        }
        for h in handles.iter().filter(|h| !h.is_empty()) {
            if text.contains(h.as_str()) {
                r.warn(8, &e.path, format!("names the roster handle `{}`", h));
            }
        }
    }
}
