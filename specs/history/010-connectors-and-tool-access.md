---
id: spec-010-history
title: "010: history of amendments and Council 006 (non-normative)"
author: architect
timestamp: 2026-10-30T00:00:00Z
status: active   # history only; the lint requires provenance frontmatter under specs/
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, history, connectors]
---

# Spec 010 history (non-normative)

This file keeps amendment text and Council 006 deliberation removed from `specs/010-connectors-and-tool-access.md` when it was folded on 2026-10-30; it binds nothing.

## Removed from §2 item 6 (010-A1 history)

*History:* Council 006 chose a minimal in-tree client so redirects, timeouts and size limits stayed under Delphi's control; the strategist seat argued for a library. The slice-1 spike ran both on the same contract cases and the library matched them except the accepted differences listed in the `mcp_rmcp.rs` header (no cap on a plain JSON response, `Retry-After` dropped, strict parsing of server replies).

## Removed from §3 item 2

(Council 006 split on this; the stricter position was kept.)

## Original §12 (010-A2 as amended text; normative content now in §6 item 4)

A draft is not harmless: a tainted span could otherwise create unlimited drafts, holds or meeting titles carrying data it read, covered by one earlier approval. So (1) **every draft call raises its own card** with the full payload; the "allow similar" cache and the auto-approving modes never cover a draft (stricter than hashing the payload into the pattern), and a draft card refuses `[a]`, records no grant and hides the option in the console; (2) a draft from a span whose ceiling is **L1 or below is Red**, with `Action::ConnectorWrite` semantics (§5.2); (3) a draft that names recipients or invitees is Red. This is detected conservatively: keys are normalised (case, `_`, `-`) and matched by stem (`to`/`cc`/`bcc`, recipient, attendee, invitee, guest, participant, email, address, collaborator, share-with); string values holding an email address or a `To:`/`Cc:`/`Bcc:` header count; and an opaque body (`raw`, `mime`, `rfc822`, or a long base64-looking string) counts, because it can hide recipients. A catalog tool that sends should be classed `write` regardless; (4) Amber remains only for private unsent artifacts from untainted spans. Other tool classes are unchanged. Implemented in `connector_run.rs`.

## Original §13

## 13. Council 006, Round 1
Both seats (Gemini 3.8 Flash, red-team and strategist) returned `reject` on the first draft. This revision addresses every finding that held up: the taint-gate contradiction (§5.2, amendment 001-A3), the missing `[connectors.*]` pin (done in `lock::access_blocks`), tool shadowing (§2 item 2, enforced in the registry), loopback listener hardening and fixed ports (§4 item 11), account pinning (§4 item 10), argument exfiltration (§6.2), PII in the event log (§8), the dependency on Step 5 retrieval (§8 item 1), unresolved remote writes (§10 test 7) and phasing (§10). The five open questions were answered: (1) yes, read-mostly with Red writes; (2) separate payload store (§8); (3) loopback with a manual fallback and a fixed-port option, never a hosted page (§4); (4) the seats disagreed; the description stays pinned with a cheap diff card (§3); (5) a minimal in-tree client behind a trait (§2).

Round 2 (both seats still `reject`) turned on one point: it said the revision cited amendment 001-A3, `Action::ConnectorWrite`, the `[connectors.*]` pin and `Registry::register_external` as existing when they did not. They were on a different branch from the one the seats read; all four are now on `main` (PR #4) with tests. The new points that held up are folded in above: tainted-span query reads need an Amber card and a 128-character cap (§6), the `@payload/<hash>` scheme and quota (§8), `Sec-Fetch-Site`/`Origin` checks on the callback (§4.11), a promotion card that highlights injection-shaped text (§5.3), `first_use` to cut approval fatigue (§6) and a phasing that keeps slices 2 and 3 off Step 4's critical path (§10). Positions that stayed split are recorded in §2 item 6 (build vs adopt, resolved by a spike) and §3 item 2 (description pinning kept).

## Original frontmatter status comment and §2 item 6 lead-in

`status: approved   # by the operator, 2026-10-24, after Council 006 rounds 1 and 2 (D-23); §1 item 6 amended by 010-A1 (D-27, operator 2026-10-24); §5.2, §6 amended by 010-A2 (council 013 D23)`

§2 item 6: "The MCP client is the official Rust SDK, `rmcp` (amendment 010-A1, D-27, operator 2026-10-24)". §5.2: "with one addition (amendment 001-A3)".
