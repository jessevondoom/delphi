//! `delphi bundle validate --report` (`specs/027` §2.4): the step 8 findings as the redaction report of
//! `skills/org-bundle-authoring/templates/redaction-report.md`, for the author to account for and the human to approve.
//!
//! The report is made from the same scan as step 8 and lists every hit with file, line and class. It names what was
//! found only in masked form (a length, a domain, a path prefix), so the report itself can be shared with a reviewer.
//! The "Action" and "Reason" cells are the author's to fill in; the approval line is the human's.

use crate::scan::{self, FileHit};
use crate::{manifest, tree, Host, Options, Report};
use std::path::Path;

/// The findings of the tree at `dir` and how many text files were scanned.
pub fn findings(dir: &Path, host: &dyn Host) -> (Option<crate::Manifest>, usize, Vec<FileHit>) {
    let mut scratch = Report::default();
    let t = tree::scan(dir, &mut scratch);
    let m = manifest::check(&t, &Options::default(), &mut scratch);
    let (files, hits) = scan::report(&t, m.as_ref(), host);
    (m, files, hits)
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// The report as Markdown.
pub fn render(dir: &Path, host: &dyn Host) -> String {
    let (m, files, hits) = findings(dir, host);
    let title = m.as_ref().map(|m| format!("{}/{} {}", m.org, m.name, m.version)).unwrap_or_else(|| dir.display().to_string());
    let failing = hits.iter().filter(|h| h.hit.fail).count();
    let mut out = format!("# Redaction report: {}\n\nWritten by `delphi bundle validate --report` and completed by the author. The human approves this report before anything is packaged (`skills/org-bundle-authoring`, step 9).\n\n", title);
    out.push_str(&format!("## Summary\n- Files scanned: {}  ·  Findings: {}  ·  Resolved: 0  ·  Left for the human: {}\n- Failing findings (the bundle cannot be packaged until they are gone): {}\n- Sources distilled from: <list of allow-listed paths>\n- Sources refused (outside the allow-list): <list or \"none\">\n\n", files, hits.len(), hits.len(), failing));
    out.push_str("## Findings\n| # | File:line | Class | Found | Action | Reason |\n| :-: | :-- | :-- | :-- | :-- | :-- |\n");
    if hits.is_empty() {
        out.push_str("| – | none | – | – | – | – |\n");
    }
    for (i, h) in hits.iter().enumerate() {
        out.push_str(&format!("| {} | {}:{} | {}{} | {} |  |  |\n", i + 1, cell(&h.path), h.hit.line, h.hit.class, if h.hit.fail { "" } else { " (warning)" }, cell(&h.hit.found)));
    }
    out.push_str("\nClasses: `secret`, `token`, `private-key`, `local-path`, `e-mail`, `person-name`, `identifier` (from `[privacy] identifiers`), `customer-data`, `transcript`. The last two cannot be found by a scan; the author looks for them by hand.\n\n");
    out.push_str("## Pointers that replace removed detail\n| Removed from | Pointer added | Access hint |\n| :-- | :-- | :-- |\n|  |  |  |\n\n");
    out.push_str("## Author's attestation\n- [ ] Every finding above has an action or a stated reason to keep.\n- [ ] No file under `knowledge/` contains instructions aimed at an agent (validator warnings addressed).\n- [ ] No secret was found in a previous published version; or, the secret has been rotated (date: <date>).\n\n");
    out.push_str("## Human approval\nApproved by: <handle>  ·  Date: <date>  ·  Package may proceed: yes / no\n");
    out
}
