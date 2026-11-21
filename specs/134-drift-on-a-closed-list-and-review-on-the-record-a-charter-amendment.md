---
id: spec-134-drift-on-a-closed-list-and-review-on-the-record-a-charter-amendment
title: "134: Drift on a closed list and review on the record (a charter amendment)"
author: architect
timestamp: 2026-11-20T21:30:00Z
status: approved   # by the operator, 2026-11-20, in a host session after the plan was shown: "follow your plan and go. i'll approve specs from this plan and ratify and changes". The charter text takes effect when the operator merges the pull request at a terminal (GOVERNANCE.md is a gate file).
approved_by: operator (2026-11-20)
reviewed_by: [council:t_0001]   # the review of this text and spec 133's: daemon Council of session host-de42e7270bcb4edf, 2026-11-21, record committed under missions/2026-11-20-spec-build-audit/councils/; its named changes are applied (section 9)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the practice is read from specs 123, 124 and 128 and their pull requests; nothing measures whether the new wording changes what a builder does (section 7)
source_mission: 2026-11-20-spec-build-audit
tags: [spec, governance, charter, builder, architect, drift, review]
---

# Spec 134: Drift on a closed list and review on the record

> **Status: approved by the operator, 2026-11-20** (see the frontmatter). It amends `GOVERNANCE.md` §1.3 and §1.4 (Tier 1), so its pull request is gate-changing: `delphi merge` refuses it and the operator merges it at a terminal after reading the diff. Its sibling, `specs/133` (the spec-to-build handoff contract), carries the markdown-only build contract.

---

## 1. Scope and objectives

Two rules in the charter say one thing and the record shows another.

1. **Drift (§1.4).** The charter says an agent that finds the implementation and the spec diverging "halts and requests a spec amendment". The builder's role file and `spec-driven-build` step 7 say to set `spec_drift_detected` and halt. No code reads that field (`context/invariant-enforcement.md`, the drift row: prompt-only). In practice builds proceed on "the closest safe reading" and the spec is corrected afterwards in the same pull request: spec 123 §16 (six corrections), spec 124 §18 (five), the tracker row of 128 S2 ("Spec text corrected"). Nothing says which departures may proceed and which must stop, and the session that built the code also wrote the correction.
2. **Review (§1.3).** The charter says an approved spec is "reviewed by Strategist and Red-Team". None of the 140 specs records such a review, and specs 126 and 128 were approved on the day they were drafted. The form to record one exists: [`specs/104`](104-responsive-foreign-seats-jobs-progress-concurrency-standing-consent-and-a-four-call-panel.md) §13 G5 lets a spec cite `reviewed_by: [council:<id>, panel:<id>]`, and `delphi lint` resolves each id to a record under `missions/*/councils/` and rejects an unwitnessed Panel. But nothing requires a citation, and no spec has one.

Daemon Council `t_0001` (2026-11-20, seats on Claude and Gemini) ruled on the audit's proposals R1 and R6. All three seats rejected letting the builder edit specs and rejected letting "no independent review" count as approval for software; the red-team raised both as P0. This spec writes the amended forms.

### 1.1 Non-goals

- **No code.** Nothing will read `spec_drift_detected` after this spec either, and the lint keeps checking only the reviews a spec cites, not that it cites one. The deferred check of `specs/133` (the spec-to-build handoff contract) §2.4 is where `reviewed_by` may become mechanical.
- **No change to `specs/001` or `specs/003`.** The handoff field and the orchestrator's routing of a drift halt (`specs/003` §3.1) stay as designed; the field becomes the signal of the stop tier.
- **No retroactive review.** Specs approved before this one are not reopened.

### 1.2 Simpler Alternative Considered

- **Alternative:** leave the charter as it is and only mark the drift row and the review claim as prompt-only in the invariant map.
- **Why rejected:** it makes the map honest and leaves the rule false. Every build would still break §1.4 on paper (specs 123 and 124 did), and §1.3 would still claim a review nobody records.

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | a closed list of departures that proceed, recorded in the pull request; everything else stops; review named in the frontmatter | the Deviations lists and `reviewed_by` lines of the next specs and pull requests (the tally of `specs/133` §2.4) |
| B | constraint removed: "the builder judges the departure" | the audit's tiers: proceed when the closest reading changes no criterion, interface, non-goal or authority, and correct the spec in the same pull request | whether reviewers later find departures that should have stopped |
| C | inversion: the reviewer, not the builder, classifies | every departure stops the build until the operator classifies it | count of stops per slice |

Chosen: A. B was the audit's R1; the Council's seats rejected it because it hands the judgment to the party whose blind spot is tunnel vision, and lets that party edit the spec. C stops a build for a typo. Flagged by the guard: none.

## 2. Design

### 2.1 `GOVERNANCE.md`

§1.3 keeps its sentence and gains, after "linked to the task (`Implements-Spec: specs/NNN-*.md`)":

> Above `specs/134` the review is cited: `reviewed_by` names a Council or witnessed Panel whose record is committed (`specs/104` §13 G5), or the status line quotes the operator's own waiver.

§1.4 becomes:

> **Drift Remediation:** if implementation and specification diverge, the agent stops and requests a spec amendment instead of silently working around the spec in code (`FM-1.1`). Only a typo, wording slip or stale path, pointer or file name that changes no criterion, interface, data shape, behaviour or authority proceeds, listed under **Deviations** for Architect to amend (`specs/134`). Builder never edits a spec; a silent spec stops; no test or guard is weakened to fit.

The charter text is kept short on purpose: the orchestrator's charter excerpt must stay under half the charter (`specs/106` A3, `pack_size_tests`), and a longer first draft broke it by about 220 bytes. The detail lives in 2.2 to 2.4: who routes the Deviations list (the orchestrator; in a host session, the operator), that the waiver is given in the operator's own turn and that an agent never writes one, and the stop on a silent spec.

The §2 table's builder counterweight changes from "*No Spec, No Build*; drift halt; audited against base-branch REVIEW.md" to "*No Spec, No Build*; drift halt (closed list, §1.4); audited against base-branch REVIEW.md". The status line gains a v0.9 entry.

### 2.2 `agents/builder/AGENT.md` §3

The counterweight keeps its first sentence and gains the closed list, the Deviations list, "never edit `specs/**`", "never weaken a test or a guard to fit", and a last step: the report ends with **Corrections for the spec** (each place the text was wrong or silent, for Architect). It says the field is not read by code yet, so the builder states the stop in its report.

### 2.3 `agents/architect/AGENT.md` §3

The simpler-alternative counterweight gains: cite the reviews in `reviewed_by` as `council:<id>` or `panel:<id>`; never cite a review that did not happen; never set `status: approved`.

### 2.4 `skills/spec-driven-build/SKILL.md` step 7

Step 7 becomes the closed list: stop and report (`spec_drift_detected`) when a spec changes while you work, two specs disagree, or the spec is wrong or silent; only the closed list proceeds, listed under Deviations; never edit the spec; never weaken a test or a guard.

### 2.5 `context/invariant-enforcement.md`

The drift row names §1.4's closed list and stays **Prompt-only** ❌. A new row: "An approved software spec cites its review (`reviewed_by`)", GOV §1.3, ◐: `delphi lint` checks that every cited review resolves and counts (spec 104 §13 G5); that one is cited at all is **Prompt-only**, and the deferred check of `specs/133` §2.4 is its way to ✅.

## 3. Authority

This spec narrows two roles and widens none.

- **Builder:** today it may halt; under the closed list it may also proceed on a typo or a stale pointer that changes no criterion, interface, data shape, behaviour or authority, which it already does in practice. It may never edit `specs/**`, but the manifest's write scope (`@worktree/**`) includes a worktree's `specs/`, so that limit is prompt-only: the Council's P0 is **mitigated, not closed** (section 7, item 4).
- **Approval:** an approved software spec with no cited review is no longer approved. The operator's own waiver remains, because approval is the operator's (§1.1); it is quoted in the status line, so it is visible.

## 4. Slices (one pull request, gate-changing; each slice its own commit, trailer and tracker row)

| Slice | Criteria closed | Files | Lines (est.) | Depends on | Blocked by |
| :-- | :-- | :-- | --: | :-- | :-- |
| S0 | (the spec) | this file, `compiled/`, `compiled/groups.toml`, tracker | (docs) | none | none |
| S1 | AC-1, AC-2, AC-3, AC-4, AC-5, AC-6 | `GOVERNANCE.md`, `agents/builder/AGENT.md`, `agents/architect/AGENT.md` §3, `skills/spec-driven-build/SKILL.md` step 7, `context/invariant-enforcement.md`, the builder and architect pack goldens, `MEASURED` in `crates/delphi/src/pack_size_tests.rs` (re-measured, §7 item 2) | 30-60 | S0 | none |

One slice, because the charter text and the role files that restate it must change together or contradict each other. Spec 133's pull request edits other hunks of `agents/architect/AGENT.md` and `skills/spec-driven-build/SKILL.md`; whichever merges second rebases.

## 5. Acceptance criteria

1. **AC-1.** `GOVERNANCE.md` §1.3 and §1.4 carry the text of 2.1, the §2 builder cell is changed as stated, and the status line has the v0.9 entry.
2. **AC-2.** `agents/builder/AGENT.md` §3 names the closed list, the Deviations list, "never edit `specs/**`", "never weaken a test or a guard", and the Corrections for the spec; its frontmatter is unchanged.
3. **AC-3.** `agents/architect/AGENT.md` §3 says to cite reviews in `reviewed_by` as `council:<id>` or `panel:<id>` and never to cite one that did not happen; its frontmatter is unchanged.
4. **AC-4.** `skills/spec-driven-build/SKILL.md` step 7 states the closed list and keeps `spec_drift_detected` as the stop signal.
5. **AC-5.** The invariant map's drift row names the closed list (Prompt-only), and a `reviewed_by` row exists (◐: the lint checks cited reviews; requiring one is Prompt-only).
6. **AC-6.** The pull request touches no `.github/`, rubric or `merge-gates.toml` path, and `GOVERNANCE.md` is its only gate file.

## 6. Verification Plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1 to AC-5 | reading the diff against section 2 |
| AC-2, AC-3 | `cargo test -p delphi-engine --lib full_pack` after regenerating the builder and architect goldens (`DELPHI_BLESS_PACKS=1`), and the diff of each golden shows only the changed lines, its hash and its nonce |
| AC-6 | `git diff --stat main` |
| Gates | `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`, `delphi specs compile`, `delphi check spec` on this file |

## 7. What is not verified

1. **That builders follow the closed list.** Both rules stay prompt-only. The tally of `specs/133` §2.4 reads the Deviations lists of the next pull requests; nothing reads them sooner.
2. **The charter's size.** The charter grows from 21,239 to 21,890 bytes, and every compact pack with it, because each carries the charter excerpt. The first draft's longer text broke two checks of `pack_size_tests` (the orchestrator's excerpt must stay under half the charter, `specs/106` A3; each compact pack within its measured size plus 10 percent); the charter text was shortened until the excerpt fit (10,938 of at most 10,944 bytes: a 7-byte margin, so the next sentence added to §1 will need a cut elsewhere or a decision on A3), and `MEASURED` was measured again for every role, as spec 124 S2/S3 did. Spec 106 §9's table is not amended by text.
3. **That an existing workspace takes the new charter cleanly.** As with #777, a workspace's ratified copy of `GOVERNANCE.md` will differ from the release's after an update; that path is not exercised here.
4. **Builder-edits-specs is mitigated, not closed.** Nothing stops a builder's commit under `specs/**`. Follow-up, with its trigger: a merge-time check (a gate-file change under spec 094) that flags a pull request whose builder-authored commits touch `specs/**`; it is specified when the first Deviations list shows a builder edit to a spec, or with the R4 spec of `specs/133` §2.4, whichever comes first.
5. **The operator's waiver is unchecked.** It is free text in a status line; the invariant map says so.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | Keep the operator's quoted waiver of review in §1.3, which the Council did not discuss | yes: approval is the operator's (§1.1), and a waiver quoted in the status line is visible; strike the clause to make review absolute | yes | none |
| OD-2 | Keep `spec_drift_detected` as the stop signal rather than delete it | yes: specs 001 and 003 design it, and deleting it would amend both | yes | none |

## 9. Review

Daemon Council `t_0001` of session `host-de42e7270bcb4edf` (2026-11-21, seats on Claude and Gemini) reviewed this text and spec 133's. The architect and strategist seats recommended approval with named changes; the red-team seat's Round 1 said the same, and its Round 2 could not be read, so the ruling went to the operator. Its changes for this spec are applied: §1.3 is scoped to specs numbered above 134 (so the older specs, 133 and this one are not made unapproved by it, and these two cite the review anyway); the waiver must be the operator's own words from their own turn, and the invariant map marks it unchecked; the closed list proceeds only when it changes no criterion, interface, data shape, behaviour or authority, and a silent spec stops; the orchestrator, or the operator in a host session, routes the Deviations list to Architect; and the builder-edits-specs limit is stated as mitigated, not closed, with a follow-up and its trigger (section 7, item 4).
