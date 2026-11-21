---
id: spec-133-the-spec-to-build-handoff-contract-criteria-mapped-to-tests-a-fuller-skeleton-and-spikes-before-unknowns
title: "133: The spec-to-build handoff contract: criteria mapped to tests, a fuller skeleton, and spikes before unknowns"
author: architect
timestamp: 2026-11-20T21:00:00Z
status: approved   # by the operator, 2026-11-20, in a host session after the plan was shown: "follow your plan and go. i'll approve specs from this plan and ratify and changes". It takes effect when the pull request that carries it merges.
approved_by: operator (2026-11-20)
reviewed_by: [council:t_0001]   # the review of this text and spec 134's: daemon Council of session host-de42e7270bcb4edf, 2026-11-21, record committed under missions/2026-11-20-spec-build-audit/councils/; its named changes are applied (section 9)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the gaps are read from the role files, the skills and specs 110-132 at main 76da4a79; whether the new text changes what a model does is not measured (section 7)
source_mission: 2026-11-20-spec-build-audit
tags: [spec, builder, architect, skills, spec-authoring, spec-driven-build, process]
---

# Spec 133: The spec-to-build handoff contract

> **Status: approved by the operator, 2026-11-20** (see the frontmatter). Markdown only: no code, no gate file, no change to any role's authority. Its sibling, `specs/134` (drift on a closed list and review on the record), carries the two charter changes (drift and recorded review) in their own pull request.

---

## 1. Scope and objectives

An audit of the architect, the builder and the spec skills (host session, 2026-11-20, a Fable subagent reading main at 76da4a79, then a daemon Council `t_0001` with seats on two families) found that the specs Delphi writes are better than the contract that carries them into a build:

1. **`spec-driven-build` covers reading a spec, not building one.** It says nothing on slice boundaries, mapping criteria to tests, or what "done" means. Spec 123's criteria are cited in tests about ten times; spec 128's ten criteria are cited zero times in `crates/`. Pull request #766 gives suite totals only, so `pr-alignment-review` rebuilds the mapping each time.
2. **The skeleton lags practice.** `skills/spec-authoring/templates/spec-skeleton.md` has no slices table, no "What is not verified", no authority section and no decision table. Of the 22 specs numbered 110 and up, 22 have slices and 18 have open decisions, but only 6 have "What is not verified".
3. **No sizing guidance.** The median recent spec is about 21 KB; spec 124 grew from 12 to 15 slices through two amendments.
4. **Unknowns are built through.** Spec 126 was approved at `confidence: low` with "the first task of S1 is to check its premise". Specs 126 and 128 let every open decision take its recommendation unless the operator spoke, whether or not it was reversible.
5. **A skill is not reliably loaded** (spec 124 §17: the architect wrote the three option kinds 0 of 3 times unless told to load the skill). The template is followed more reliably than skill prose, so this spec puts the structure in the template.

This spec changes four markdown files and one role file so that a slice has a written contract before code and a checkable record after it.

### 1.1 Non-goals

- **No code and no check.** Mechanical enforcement (recommendation R4 of the audit: `delphi check spec` tests numbered criteria, criterion-to-slice mapping and blocking decisions, run from `delphi lint`) is deferred to a later spec with a written trigger (section 2.4).
- **No charter change.** The drift rule and the recorded review are `specs/134` (drift on a closed list and review on the record). This spec leaves step 7 of `spec-driven-build` and the builder's role file to it.
- **No new skill for any role.** The audit proposed authorizing `scope-creep-detection` for the builder. That widens a manifest; its one rule ("build only this slice; new ideas go to a follow-up list") is folded into the build contract instead.
- **No retroactive edits** to existing specs.

### 1.2 Simpler Alternative Considered

- **Alternative:** add one paragraph to `spec-driven-build` ("map criteria to tests, list them in the pull request") and nothing else.
- **Why rejected:** it fixes gap 1 only, and it is the kind of skill prose the evidence (gap 5) says is skipped. The skeleton is what an architect copies; leaving it behind leaves gaps 2 to 4 open.

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | text in the two skills plus a fuller skeleton; enforcement later, on evidence | the tally of section 2.4 over the next three specs |
| B | constraint removed: "enforcement needs a template that has been used" | ship the `check spec` extension now, warn-only | lint warnings on every new spec from day one |
| C | inversion: make the pull request the contract, not the spec | a pull-request template with the criterion table and no skeleton change | whether reviewers stop rebuilding the mapping |

Chosen: A. B was the Council's red-team position; the architect and strategist seats and the operator chose to defer it, because a check written before the template is used would encode a shape nobody has tried (the operator's decision, 2026-11-20). C is folded into A: the pull-request table is part of the build contract (section 2.1). Flagged by the guard: none.

## 2. Design

### 2.1 The build contract (`skills/spec-driven-build/SKILL.md`, `skills/git-pr-packaging/SKILL.md`)

`spec-driven-build` gains a section **Build contract (when you build a slice)**, after its reading steps:

1. **Before code, write the slice's contract:** the criteria this slice closes (by number) and, for each, the test or check that will show it. A criterion with no possible test is a spec defect: report it, do not invent a test that proves something else.
2. **Name the criterion in the test** (`specs/NNN AC-n` in the test's name, doc comment or assertion message), so a reader can find the test from the spec.
3. **Build only this slice.** A good idea outside it goes to a follow-up list in the pull request, not into the diff.
4. **Done means every criterion has a row.** The pull request carries the table of 2.1.1. A slice is not done while one of its criteria has no row, and a row's result is never "pass" for a test you did not run.

`git-pr-packaging` step 5 gains the table and two lists, and a new step for the tracker:

#### 2.1.1 The table and lists in the pull request

```
| Criterion | Test or check | Result |
| AC-1 | `crate::module::test_name` | pass / fail / not run / not run live / not verified |
Deviations: none, or each one (specs/134 / GOVERNANCE §1.4)
Follow-ups: ideas outside this slice
```

#### 2.1.2 The tracker

The tracker (`missions/2026-10-23-foundational-design/spec-tracker.md`) is over 1,200 lines. Find the spec's rows with a search and add or change only those lines; never read the whole file into context.

The skill's `description` changes to say it covers building a slice (at most 200 characters), and `docs/reference/catalog.md` is regenerated.

### 2.2 The skeleton (`skills/spec-authoring/templates/spec-skeleton.md`)

The skeleton keeps every section `delphi check spec` looks for (Scope, Simpler Alternative Considered, Verification Plan) and gains:

- **`reviewed_by: []` in the frontmatter**, in the form `delphi lint` already checks ([`specs/104`](104-responsive-foreign-seats-jobs-progress-concurrency-standing-consent-and-a-four-call-panel.md) §13 G5): `[council:<id>, panel:<id>]`, each resolving to a record committed under `missions/*/councils/`. `GOVERNANCE.md` §1.3 already requires the reviews; this is where they are cited.
- **Authority** (only when the spec touches authority, trust, a gate or a sandbox): what the change may and may not do, and the test that proves each limit.
- **Slices**: a table with columns `Slice | Criteria closed | Files | Lines (est.) | Depends on | Blocked by`. S0 is the spec itself, or a spike (2.3).
- **What is not verified**: the claims the spec makes that no one has observed.
- **Decisions for the operator**: a table with columns `OD | Question | Recommendation | Reversible? (operator decides) | Blocks`. It replaces "Open Questions".
- The Verification Plan maps each criterion number to its test.

### 2.3 Authoring rules (`skills/spec-authoring/SKILL.md`)

New steps and rules, in place of nothing (no existing step is weakened):

1. **Size (guidance, not a rule).** A slice closes named criteria, is observable by a test, and is about 300 changed lines or fewer. A spec past about 8 slices, or one that needs a second amendment, is split into a new spec. Exceeding either is allowed when the spec says why.
2. **Tiny change.** A spec for a change of about 100 lines or fewer with 3 criteria or fewer may leave out the Slices table, and the Decisions table only when no open item touches authority, an interface or something irreversible; it says so in one line.
3. **Unknowns first.** A premise nobody has observed gets slice S0: a spike, with a kill criterion written into the spec before the spike runs. No other slice starts until the spike's result is written into the spec.
4. **Decisions.** The architect proposes whether a decision is reversible; the operator decides. Only a decision the operator marked reversible may take its recommendation by default, and the default is recorded in the tracker. A decision that changes an acceptance criterion, an interface, a data shape or authority blocks the slices it names until the operator answers.
5. **Review before approval.** Ask for a Council or a witnessed Panel with Strategist and Red-Team seats, cite it in `reviewed_by` as `council:<id>` or `panel:<id>`, and commit its record under `missions/<mission>/councils/` with the spec so the lint can resolve it. Council ids restart in every session (`t_0001`), and the lint maps an id to one record: before citing, search `missions/*/councils/` for the same `council_id`; if one is there, the citation is ambiguous, so stop and ask instead of citing. A spec with an Authority section also gets `spec-premortem`. Never cite a review that did not happen.
6. **Start from the skeleton** (unchanged: `status: draft`; an agent never marks a spec `approved`).

### 2.4 The deferred check (recommendation R4) and its trigger

R4 is not dropped; it waits for evidence. **Trigger:** the first of (a) three specs numbered above 134 written from the new skeleton, or (b) 2026-12-18. **Owner:** the operator's first session on or after the trigger; this spec's tracker row R4 carries the date, and the session that merges the third such spec says in its report that the trigger fired. At the trigger, the owner writes a tally into that row:

- for each of those specs: which of the new sections it has (Slices with the criteria column, the Decisions table with reversibility, What is not verified, Authority where it applies, a Verification Plan that names every criterion), and whether `reviewed_by` names real reviews;
- for each pull request that built one of their slices: whether it carries the table of 2.1.1, and whether its tests name criteria.

The R4 spec then decides each check from the tally: a section present in at least two of three specs becomes a check (a warning first); a section skipped in at least two of three is evidence that prose does not hold, so R4 proposes it as a failing check from the start, or proposes removing the section if the specs were better without it. R4 also takes the Council-id collision of section 7, item 3.

## 3. Authority

None changes. No manifest's `authorized_skills`, `default_access_scope` or tier moves. The text added to a skill is an instruction to the role that already holds that skill. Section 2.3 rule 4 moves a judgment (is a decision reversible) from the architect to the operator, which narrows the architect.

## 4. Slices (one pull request; same-spec slices, each its own commit, own trailer and own tracker row)

| Slice | Criteria closed | Files | Lines (est.) | Depends on | Blocked by |
| :-- | :-- | :-- | --: | :-- | :-- |
| S0 | (the spec) | this file, `compiled/`, `compiled/groups.toml`, tracker | (docs) | none | none |
| S1 | AC-1, AC-2, AC-3 | `skills/spec-driven-build/SKILL.md` (not step 7), `skills/git-pr-packaging/SKILL.md`, `docs/reference/catalog.md` | 40-70 | S0 | none |
| S2 | AC-4, AC-5, AC-6, AC-7 | `skills/spec-authoring/templates/spec-skeleton.md`, `skills/spec-authoring/SKILL.md`, `agents/architect/AGENT.md` §2 item 2 | 60-110 | S0 | none |

The pull request touches no `.github/`, charter or rubric path. Spec 134 edits step 7 of `spec-driven-build` and §3 of `agents/architect/AGENT.md`; the hunks are separate, and whichever merges second rebases.

## 5. Acceptance criteria

1. **AC-1.** `skills/spec-driven-build/SKILL.md` has a "Build contract" section with the four steps of 2.1, and its step 7 is byte-identical to main's.
2. **AC-2.** `skills/git-pr-packaging/SKILL.md` step 5 names the table and two lists of 2.1.1, and a step says to change tracker rows by search, not by reading the file.
3. **AC-3.** Both skill packages pass `delphi check skill`; the new description is at most 200 characters; `docs/reference/catalog.md` matches `delphi docs catalog`.
4. **AC-4.** The skeleton has `reviewed_by: []` with a comment giving the spec 104 form, Authority, Slices (with the six columns), What is not verified and the Decisions table (with the five columns), and still passes `delphi check spec` (the test `shipped_templates_pass_their_checks`).
5. **AC-5.** `skills/spec-authoring/SKILL.md` carries rules 1 to 5 of 2.3, and step 8's "an agent never marks a spec `approved`" is unchanged.
6. **AC-6.** `agents/architect/AGENT.md` §2 item 2 asks for a traversal map only when a design spans more than one crate or process; its frontmatter is unchanged.
7. **AC-7.** No manifest's `authorized_skills` or `default_access_scope` changes (a diff of every `agents/*/AGENT.md` and `skills/*/SKILL.md` frontmatter shows only the one `description` of AC-3).

## 6. Verification Plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1, AC-2, AC-5, AC-6 | reading the diff; `git diff main -- skills/spec-driven-build/SKILL.md` shows no change inside step 7 |
| AC-3 | `delphi check skill skills/spec-driven-build`, `delphi check skill skills/git-pr-packaging`; `cargo test -p delphi --test docs` |
| AC-4 | `delphi check spec skills/spec-authoring/templates/spec-skeleton.md`; `cargo test -p delphi --bin delphi -- shipped_templates_pass_their_checks` |
| AC-7 | `git diff main -- agents skills` read for frontmatter lines |
| Gates | `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`, `delphi specs compile`, `delphi check spec` on this file |

## 7. What is not verified

1. **That the text changes behaviour.** Gap 5 says skill prose is often skipped. The skeleton is the stronger lever, but whether architects fill the new sections, and builders the table, is what the tally of 2.4 measures. Nothing here measures it in advance.
2. **The sizing numbers.** About 300 lines a slice and about 8 slices a spec are guesses from specs 110-132, not measured against outcomes. They are guidance for that reason.
3. **That a cited Council id is unique.** The lint maps a `council_id` to one record; ids such as `t_0001` restart in every session, so two committed records with the same id would shadow each other. Until R4 fixes it in the lint, rule 5 of 2.3 makes an author check for a duplicate before citing.
4. **The audit's counts** (criteria cited in tests, sections per spec) are regular-expression matches over specs 110-132 and `crates/`, not a manual read.

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | Defer R4 until the trigger of 2.4 | yes (decided by the operator, 2026-11-20) | yes | none |
| OD-2 | Fold `scope-creep-detection`'s rule into the build contract instead of authorizing the skill for the builder | yes; no manifest widens | yes | none |

## 9. Review

Daemon Council `t_0001` of session `host-de42e7270bcb4edf` (2026-11-21, seats on Claude and Gemini) reviewed this text and spec 134's. The architect and strategist seats recommended approval with named changes; the red-team seat's Round 1 said the same, and its Round 2 could not be read, so the ruling went to the operator. The changes it named for this spec are applied: the tiny-change exemption keeps the Decisions table when an item touches authority, an interface or something irreversible (2.3 rule 2); a duplicate Council id stops a citation (2.3 rule 5); the R4 trigger has an owner, a dated row and a rule without the contradiction it had (2.4). The design it rests on was ruled by an earlier Council of the same id in session `host-390eef8435513f56` (2026-11-20), whose record is not committed, so it is not cited.
