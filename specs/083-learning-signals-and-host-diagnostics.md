---
id: spec-083-learning-signals-and-host-diagnostics
title: '083: Learning signals and host diagnostics: four small slices from the 2026-11-05 Council ranking (draft)'
author: architect
timestamp: 2026-11-05T00:00:00Z
status: draft
governed_by: GOVERNANCE.md
review_rubric: REVIEW.md
source_type: internal_generated
confidence: medium   # code anchors checked on main 2026-11-05; nothing built, nothing measured
source_mission: 2026-11-05-session
tags:
- spec
- learning
- doctor
- host
- draft
---
# Spec 083: Learning signals and host diagnostics

> **Status: draft. A draft is a proposal, not permission to build.** Approval by the operator makes slices S1-S4 buildable, one pull request each.
> **Origin:** the 2026-11-05 ranking of unbuilt work (strategist, red-team, builder, advocate and orchestrator seats, one model family; no cross-family round). These are the four phase-1 items that no approved spec covers. Spec 082 slices, the E1 experiment and the 037 pilot are covered elsewhere.
> **Extends:** [`031`](031-layered-learning-and-training-mode.md) §3 (staging), [`062`](062-delphi-as-host-agent-mcp-server.md) §8 (host notes), [`074`](074-stable-launch-path-and-delphi-connect.md) / [`071`](071-host-plugin-marketplace-and-durable-mcp-enablement.md) (`--check`, doctor check 15).

## 1. Scope

| Slice | Delivers | Hours (builder-corrected) |
| :-- | :-- | --: |
| S1 | `delphi doctor` host-chain section: first failing link and the exact fix, printed, never applied | 7 |
| S2 | One quiet learning-health line in the console footer: staged count, awaiting-review count, age of the oldest | 7 |
| S3 | Learning-signal slice: recurrence count on staged notes, dismissed-claim memory for non-contact triggers, seat-keyed Council claims; may split into S3a (dismissal, 4 h) and S3b (recurrence and seat, 10 h) | 14 |
| S4 | `host.propose_note` returns `duplicate_of` for the same host session's own earlier notes | 5 |

Hours are after a code review of this draft (original estimates 5, 5, 10, 4 were low).

### 1.1 Non-goals

- No score, rank or "health percentage" anywhere (S2, S3 show counts and ages only).
- No outcome, `done_when_met` or other self-reported field from outside hosts (that is a separate, later spec; the red-team classed it P0 unless typed `untrusted/self_reported`).
- No auto-apply, auto-withdraw, auto-approve. Every change still reaches the operator through `/retro` or a card.
- Nothing here changes the sandbox, taint rules, gates, or what a host may do.
- N2 (a reverted decision stages a note) already exists: `learn.rs`, trigger `rejected_decision`. Not rebuilt.

## 2. What exists today (verified on main, 2026-11-05)

1. `crates/delphi/src/doctor.rs` (around lines 228-245) runs check 15 `host-plugin` over `mcp_hosts.rs` / `mcp_check::check_host` (claude-code, copilot, codex). It emits one line per host with a level of Ok, Warn or Skipped and a `Fix:` line. It reads only host files: not the daemon host socket and not `[mcp] enabled`. `host_rpc::gate` (`host_rpc.rs`, near line 82) is what reads `[mcp] enabled`. "Plugin installed" and "`--check` result" are the same `check_host` call.
2. `learn.rs` has 12 `TRIGGERS`, `is_contact`, `subject_key` (trigger plus summary text) and a `contact_seen` set (cap `CONTACT_CAP`) rebuilt by replay. `learn.dropped` removes an id from `live` but does not clear `contact_seen`, so a dismissed **contact** claim is already never re-staged. Non-contact triggers and host notes have no subject dedupe, no count of recurrences exists, and no source hash exists (`StagedNote` has `event_seqs`).
3. `service.rs` `host_stage` handles `host.propose_note` and `host.record_session_summary`; `host_rpc.rs` builds the `learn.staged` event, whose summary is `[label] text (evidence: ...)`, so the plain note text is not stored separately. Host notes live in per-session missions (`host-<session>`); `Caller::parse` accepts any 8-32 character session id the host supplies. The path counts `already` staged notes under the `host_gate` lock but returns nothing about them.
4. `ui/src/screen.ts` footer already carries `libraryCounter` (`+3 noted · 1 to review`), where `m.review` counts decision-pane candidates, not `learn.proposed` items. The model has no staged count or oldest-age field; `digest.rs` has age logic that may be reused. `m.now` exists in the model for time.

## 3. S1: host-chain diagnostic

`delphi doctor` gains, inside check 15 (not a new command), an ordered list of links: (1) the daemon is reachable, (2) `[mcp] enabled`, read through `host_rpc::gate` so there is one reader of the flag, (3) the host config entry is present and the plugin is installed (one `check_host` call). Output names **the first failing link** and prints the exact fix as text. This is a new ordered model, not an extension of the one-line-per-host output.

- **Never applied.** The fix is a printed command or config line. Doctor does not enable MCP, edit a host config, or start a daemon.
- **Daemon link.** S1 must name the owner file and path of the host socket (`<hash>.host.sock`, spec 062 S5) and probe by connect-and-close with a short timeout, sending no method call, so it cannot spend tokens or stage anything. If that probe module proves larger than its 2 hours, fall back to the existing daemon-status read and say so in the pull request.
- Printed fixes use a fixed table and never show an operator path beyond what doctor already prints.
- Unknown state is reported as unknown, not as healthy.
- Docs: regenerate `docs/reference/` if help text changes; the tests in `tests/docs.rs` must pass.

## 4. S2: learning-health line

A derived value from existing events (`learn.staged`, `learn.proposed`, `learn.proposal_resolved`, `learn.dropped`): `learn: 3 staged · 1 to review · oldest 6d`. It needs a new field and a daemon feed (the model has none today; reuse the age logic in `digest.rs`), distinct from `m.review`. Rules:

- The age comes from the daemon's own event timestamps; it is hidden when the timestamp is absent, and host-supplied content never sets it.

- Static text. Obeys reduce motion by having nothing that changes with time except the number of days (computed on render, no private clock; take the time from the same source the rest of `screen.ts` uses).
- Hidden when there is nothing staged and nothing to review.
- **Age and backlog only.** Zero staged is not shown as "healthy" and no colour implies good or bad (red-team: a vanity metric discourages staging). Not alert-coloured, as in 053 §5.
- Shares the footer's narrow-window give-way order: it yields before the tokens and mode do.
- Golden screens under `ui/test/snapshots*` are regenerated in the same pull request.

## 5. S3: learning-signal slice

One pass in `learn.rs`, three behaviours:

1. **Recurrence count (S3b).** For staged notes with the same `subject_key`, show a count of how many **daemon-assigned** missions produced them. Host session ids are chosen by the host (`Caller::parse` accepts any 8-32 characters), so one adversary can mint unlimited "sessions"; a host note's count is therefore labelled `reported by N host sessions` and is never used for ordering above a note the operator's own work produced. `/retro` and `delphi retro list` show it. It is a display, not a threshold that creates rules. Counts are computed by a read-only scan of mission logs at `/retro` time; there is no new store.
2. **Dismissed-claim memory for the remaining triggers (S3a).** Contact claims already stay dismissed (§2.2). For the other triggers and host notes, a `learn.dropped` item is not re-staged while its normalized subject is unchanged. This needs a new `source_hash` property on the `learn.staged` payload (and in `schemas/`); a changed source set offers the claim again. A dismissal expires after 90 days and may re-stage at most once per window.
3. **Seat-keyed Council claims (S3b).** `council_claim` notes already carry their seat as a role ID, so recurrence is shown per role. `council_minority` has no seat today (it is built from unresolved finding titles and severities), so it is out of scope unless the verdict event is extended in a separate change. Staging only; it never writes a rule.

Constraints: the contact cap and the existing taint stamping are unchanged; nothing here relaxes a rule or raises a tier.

## 6. S4: `duplicate_of` on `host.propose_note`

`host.propose_note` already stages one untrusted note. Add an optional response field `duplicate_of: <note id>` when **only the same host session's own earlier notes** include one with the same normalized `text`. The key is a new `norm_text` property stored in the `learn.staged` payload, because the summary is `[label] text (evidence: ...)` and a different evidence string would defeat a key over the summary. The lookup runs inside the existing `host_gate` lock, over that session's own `host-<session>` mission only. Staging behaviour and the cap rules are unchanged (today the path counts and appends); the host just learns it is repeating itself.

- **No existence leak.** A note in another host session's mission, or any operator note, is never returned. A test stages the same text from two sessions and asserts the second receives no id.
- The tool schema and the `learn.staged` payload change, so the host tool snapshots, `docs/reference/`, the coverage manifests (046/047) and the JSON schemas under `schemas/` are regenerated in the same pull request.
- A host cannot set trust, status or origin through this path; that stays as in 062 §8.

## 7. Simpler Alternative Considered

- **S1:** add the socket and enabled checks to existing output with no ordering. Rejected as the sole design because "which link failed first" is the whole point of the section, but it is the fallback if the ordered form grows.
- **S2:** fold the line into a session-start recap only. Rejected: the recap does not stay visible, and the staged queue ages unnoticed. Kept as the fallback if footer room is a problem.
- **S3:** only the dismissed-claim memory (3 hours). This is the smallest part and ships first if the recurrence count proves hard to compute over the log.
- **S4:** silent server-side dedupe with no response field. Cheaper (no schema change), but the host keeps retrying blind; kept as the fallback if the snapshot churn is unwelcome.
- **Doing nothing** was rejected: outside-host failures are invisible, and the staged queue has no age signal.

## 8. Risks and mitigations (red-team seat)

| Risk | Class | Mitigation |
| :-- | :-- | :-- |
| S1 prints a wrong or unsafe fix (for example "enable MCP") | P1 | fixes are text only, taken from a fixed table, reviewed in the PR; doctor never applies one |
| S2 reads as a score and discourages staging | P1 | counts and age only, no colour, hidden at zero |
| S3 recurrence is manufactured by a host minting session ids | P0 | count daemon-assigned missions; label host counts `reported by N host sessions`; never rank them above operator-origin notes |
| S3 expiry re-stages a poisoned claim every 90 days | P1 | at most once per window; test it |
| S3 suppresses a claim after its evidence improved | P2 | key on `source_hash`, expire after 90 days |
| S2 age taken from host-supplied content | P1 | daemon timestamps only; hidden when absent |
| S3 prunes rare, high-severity lessons | P2 | nothing is pruned; count only orders the list |
| S4 leaks that another session's or the operator's note exists | P0 | lookup scoped to the caller's own `host-<session>` mission; cross-session test |
| S4 widens a host's reach | P0 if violated | no new input that sets trust or status; test asserts it |

## 9. Verification

- S1: unit tests per failing link with a fixture; each prints the right fix and nothing is written to disk or config; unknown reads as unknown.
- S2: golden screens for zero, one, many and narrow windows; the reduce-motion test (`ui/test/reduce-motion.test.ts`) stays green; no raw clock.
- S3: a fixture with the same note from one session five times counts as 1; from three sessions as 3; a dropped item is not re-staged until its source hash changes; a 91-day-old dismissal is offered again; a role-keyed dissent shows per role.
- S4: second identical `propose_note` from the same session returns `duplicate_of`; from a different session returns none; the operator's own note is never revealed; the schema, docs and snapshot tests pass.
- Gates for every slice: the three AGENTS.md commands. Commit trailer `Implements-Spec: specs/083-learning-signals-and-host-diagnostics.md`. The tracker row is updated in the same pull request.

## 10. Slices (one pull request each)

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| S0 | operator approves this spec; tracker entry | operator |
| S1 | §3 | S0 |
| S2 | §4 | S0 |
| S3 | §5 (ship the dismissed-claim part first if split) | S0 |
| S4 | §6 | S0; best after #479 so the host schema churns once |
