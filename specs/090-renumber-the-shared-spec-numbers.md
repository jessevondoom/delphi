---
id: spec-090-renumber-the-shared-spec-numbers
title: "090: A one-time renumbering of the three spec numbers that two specs share"
author: architect
timestamp: 2026-11-07T22:00:00Z
status: approved   # by the operator, 2026-11-07 ("approve both 088 and 090"), after the recommendations for OD-1 to OD-6 were shown; the work waits for a quiet window the operator picks
approved_by: operator (2026-11-07)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # counts taken by search on main on 2026-11-07; the hours are estimates and no trial rewrite was run
source_mission: 2026-10-23-foundational-design
tags: [spec, numbering, citations, lint, cleanup]
---

# Spec 090: Renumber the three shared spec numbers, once

> **Status: approved by the operator, 2026-11-07.** Written at the operator's request after a review of spec 088 (PR #517) found that three spec numbers are used by two specs each. Decisions OD-1 to OD-6 (§8) take the recommendations, as the operator approved them after seeing them. Nothing is built; the work waits for a quiet window the operator picks (OD-4).
> **Extends:** the folding rule in [`CONTRIBUTING.md`](../CONTRIBUTING.md) and the reference gate `crates/delphi/tests/spec_refs.rs`. It changes no spec's meaning, no engine behaviour and no approval rule. It is independent of spec 088, which does not need it (that spec identifies a spec by its file name) but is easier to extend with it.
> **Number:** on 2026-11-07 the latest spec on main is 094; 088 is held by its pull request. The three renumbered specs take the next three free numbers when the work starts: 091 to 094 are taken, so 095 to 097 as of today (042 stays reserved).

---

## 1. Scope and objectives

Three numbers are used by two spec files each (first commit dates from `git log`):

| Number | Spec | First committed |
| :-- | :-- | :-- |
| 014 | `014-documents-and-dataviz.md` | 2026-10-25 |
| 014 | `014-relay-and-away-mode.md` | 2026-10-25 |
| 037 | `037-console-deck-change-log.md` | 2026-10-26 (renumbered to 037 that day) |
| 037 | `037-training-experiments-quality-and-efficiency.md` | 2026-10-26 |
| 061 | `061-shell-tool.md` | 2026-10-31 |
| 061 | `061-encyclopedia-search-pages.md` | 2026-11-01 |

Each clash came from two sessions taking the next number at once. The cost is that a bare citation such as "spec 014" or "014 §3" names either spec, and nothing can tell which was meant:

- `spec_refs` accepts a citation if *any* file with that number has the section (`crates/delphi/tests/spec_refs.rs`, the `any` checks), so a wrong citation passes silently whenever both specs have a section with that number.
- A lint rule against duplicate numbers cannot be switched on without an allowlist, and an allowlist is a promise that the clash stays.
- Requirement IDs of the form `014-R4`, proposed in spec 088 for a later stage, would name two specs.

This spec renumbers **one spec of each pair, once**, inside `specs/` (no file changes directory), and turns the lint on afterwards so the clash cannot return. It does this in one pull request that can be reviewed commit by commit and reverted as one.

1. **Choose which spec of each pair moves** by a stated rule (§2), not by taste.
2. **Rename and edit** the three files (§3.1).
3. **Rewrite every path pointer** to the old file names (§3.2).
4. **Decide every bare citation** of 014, 037 and 061 and record each decision in a map a person can sample (§3.3).
5. **Check what the existing gate cannot** with a one-time strict check (§3.4).
6. **Add the permanent guard:** `delphi lint` fails when two files in `specs/` share a number (§3.5).
7. **Leave a mapping for history**, which cannot be rewritten (§3.6).

### 1.1 Non-goals

- **No move to a subfolder.** The engine treats `specs/` as a governed path by prefix (spec 088 lists the sites), so this change keeps every file directly under `specs/` and touches no engine code.
- **No change of meaning.** The only edits to a spec's text are its own identifiers and one note line (§3.1). No requirement is reworded.
- **No constitutional edit under the recommended choice.** GOVERNANCE.md (line 87) and REVIEW.md (line 27) cite the relay spec as 014. The recommendation (OD-1) leaves the relay spec at 014 so neither file changes. If the operator chooses the other member of that pair, those two Red-tier files need their own ratified step (§2.2).
- **No rewriting of history.** Commit trailers, merged pull-request text and git history keep the old numbers. Mission notes, councils and past changelog entries are historical records and are rewritten only where a gate fails on them (OD-2).
- **No other cleanup.** The 005a to 005g letter specs, the reserved 042, the gaps in numbering and every other irregularity stay.
- **No new numbering scheme.** Numbers stay three digits with an optional letter.

### 1.2 Simpler alternatives considered

- **Do nothing and add a lint with an allowlist for the three clashes.** About an hour. The clash and the silent mis-citation stay for good, and the allowlist is the evidence. Rejected as the end state, but it is the fallback if the operator decides the cost below is not worth paying.
- **Keys or suffixes** (`014a`, `014b`, or a `key:` line in frontmatter). Rejected by the review of spec 088: a frontmatter line edits ratified content, and a suffix still leaves "014" in prose ambiguous.
- **Rename only, with no citation work.** Cheapest, and unsafe: every citation of the moved spec keeps its old number and silently points at the other spec.

---

## 2. Which spec of each pair moves

### 2.1 The rule

Move the member of the pair whose citations can be told apart most reliably, and whose move does not require a constitutional edit. Where the two are equal, move the newer. The evidence, counted by search on 2026-11-07:

| Pair | Candidate to move | Path references to its file name | Code files citing the number that clearly mean it | Why |
| :-- | :-- | :-- | :-- | :-- |
| 014 | documents and dataviz | 4 files, 4 occurrences | about 45, all in `crates/delphi-docs` | the citations sit in one crate, so one directory-wide rule decides them |
| 014 | relay and away mode | 7 files, 9 occurrences | about 15 spread over `relay/`, `delphi-core` and the engine, plus GOVERNANCE.md and REVIEW.md | mixed with the documents citations in the engine and core, and it would reach the constitution |
| 037 | training experiments | 5 files, 5 occurrences | 9, all `train_*.rs` | the file-name prefix decides them |
| 037 | console deck change log | 3 files, 3 occurrences | `ui/src/fold.ts` and a few notes | few, but `fold.ts` is also cited for other work |
| 061 | encyclopedia search pages | 4 files, 4 occurrences | about 10, in `library_*`, `page_run`, `kbserve` | mixed with shell citations in `tools.rs`, `engine.rs`, `service.rs` and three console files |
| 061 | shell tool | 2 files, 3 occurrences | about 10, in `exec`, `approval`, `shellsafe`, `shell.ts` | mixed in the same files |

Total files citing the three numbers by a crude word search: 90 for 014, 72 for 037 and 29 for 061 (including false positives), and about 165 by a stricter citation pattern. Only the citations that mean a moved spec change.

### 2.2 Recommendation

Move **documents and dataviz** (014), **training experiments** (037) and **encyclopedia search pages** (061, the newer of two equal cases). The relay spec keeps 014, so GOVERNANCE.md and REVIEW.md are untouched and no Red-tier edit is needed. The operator may choose otherwise (OD-1). If the relay spec moves, its path and number appear in GOVERNANCE.md and REVIEW.md, those edits are constitutional even though they change only a number (GOVERNANCE.md, "Constitutional Changes Are Red-Tier"), and they become a separate step applied after the operator ratifies exactly those lines.

### 2.3 The new numbers

The next three free numbers **at the moment the work starts**, in the order of the pairs (014, 037, 061), expected 095, 096 and 097 (091 to 094 were taken while this spec was being drafted, which is the problem it describes). The approval pull request (S0) names them in the tracker, but other sessions keep taking numbers, so S1 re-checks `specs/` and the open pull requests when it starts and uses the next three free ones. File names keep their slugs: `095-documents-and-dataviz.md`, `096-training-experiments-quality-and-efficiency.md`, `097-encyclopedia-search-pages.md`.

---

## 3. The operation

### 3.1 Rename and edit the three files

`git mv` each file (history follows the rename). In each file: the frontmatter `id:` and `title:` take the new number; every self-citation of the old number in the body ("this spec", "spec 014" inside the file) takes the new one; and one note line is added under the status block: "Renumbered from 014 on 2026-11-DD; it shared that number with the relay spec." The file's content and path both change, so any ratification of these three specs is refreshed (§3.7).

### 3.2 Path pointers

An exact-string rewrite of the old file name (for example `specs/014-documents-and-dataviz.md`, and the same without `specs/` in relative links) to the new one, in every file that is not a git object. These are unambiguous: counted today, 4 files for the documents spec, 5 for training and 4 for encyclopedia. They are checked by `spec_refs`, `delphi lint`'s link check and the docs-drift test, and by a final search that finds no occurrence of the three old file names outside the mapping note (§3.6).

### 3.3 Bare citations: the map

A bare citation is `014 §N`, `spec 014`, `specs/014` and similar, naming a number and not a file. For every such citation of 014, 037 or 061 in a **live** file (specs, code and its comments, docs, schemas, agents, skills, themes, the console, `AGENTS.md`, `CONTRIBUTING.md`, `README.md`, `llms.txt`), the operation decides which spec was meant and records it. A decision uses the first rule that applies:

1. **Location.** A citation inside `crates/delphi-docs/**` means the documents spec; inside `crates/delphi/src/train_*.rs` means training; a citation next to a path under `library_*` or `page_run` means encyclopedia pages. The rules are listed in the map's header so they can be read and challenged.
2. **Content.** A citation with a section number whose heading exists in only one of the pair is that one's.
3. **Reading.** Anything else is read in context and decided by hand, with the reason in one line.

The map is a table committed with the pull request, one row per citation: file and line, the old text, the spec meant (by file name), the new text or "stays", and the rule used. It lives in a plain mission note (`missions/2026-10-23-foundational-design/`), is not a spec, and is the thing the operator samples (§5, test 6). Historical records (mission notes, council records, history files, past changelog entries) are not part of the map unless a gate fails on one (OD-2); the mapping paragraph (§3.6) covers them.

### 3.4 A one-time strict check

`spec_refs` cannot see a wrong decision when both specs have the cited section. So the pull request carries a script, run once and not kept as a test, that lists **every** remaining citation of 014, 037 and 061 in a live file and fails if any is not a row in the map marked "stays (means <file name>)" or "changed to <new>". It also checks the converse: no row of the map has a citation text that still appears unchanged at its place when the row says "changed". The script's output is attached to the pull request. The permanent guard is §3.5; a permanent strict mode in `spec_refs` was considered and not adopted, because after the renumber the numbers are unique and the ambiguity is in historical text only.

### 3.5 The permanent guard

A `delphi lint` rule: two files directly under `specs/` may not begin with the same number-and-letter (`005a` and `005b` differ; two `014` files do not pass). It runs in CI, so a second session that takes a taken number fails its own pull request and renumbers its **new, unmerged** spec, which is free to do. The rule has no allowlist. It lands in this pull request, after the renames, so main is never red. Spec 088 lists the same rule in its first slice; whichever merges first carries it and the other drops it.

### 3.6 History

Commit trailers, merged pull requests, past changelog entries and the git log keep the old numbers and cannot be changed. `CONTRIBUTING.md` gains one paragraph under "Folding spec amendments": the three renumberings (old number and file name, new number and file name, date) and the sentence "a citation of 014, 037 or 061 dated before 2026-11-DD may mean either spec of that pair; the files named above tell them apart." The generated index of spec 088, if built, repeats it.

### 3.7 Ratification

The renamed files have a new path and new content. Where a ratification lock applies to this repository (it is keyed by the path, `crates/delphi-engine/src/ratify.rs`, and checked at `finalize.rs`'s `spec_link`), the three entries are refreshed by the operator in one step after the pull request merges; the operator's rule that committed work counts as ratified is recorded in memory and this spec does not assume it covers the lock. Whether the repository uses a lock for its own work is confirmed in S1 before the pull request opens, and the answer is written in its description.

---

## 4. What does not change

- No engine, adapter or console behaviour. The only `.rs` and `.ts` edits are comments and string constants that cite a spec; none changes what a program does, and the test suite confirms it.
- No spec's meaning, no section number, no acceptance criterion.
- GOVERNANCE.md and REVIEW.md, under the recommended choice.
- The 005a to 005g specs, the reserved 042, the history files' contents.
- Git history, trailers and merged text.

---

## 5. Verification plan

1. **Reference gate.** `cargo test -p delphi --test spec_refs` passes with its allowlist unchanged or shorter. No entry is added for a new break.
2. **Lint rule.** A fixture directory with two files sharing a number fails the new rule and one with `005a` and `005b` passes; the real `specs/` passes. `cargo run -q -p delphi -- lint --repo .` passes.
3. **Old names gone.** A search for each of the three old file names finds nothing outside the mapping paragraph and the historical records the operator chose to leave (OD-2).
4. **Map complete (§3.4).** The one-time script exits clean: every citation of the three numbers in a live file is a row of the map, and no row marked "changed" still shows its old text.
5. **Behaviour unchanged.** The three gate commands in `AGENTS.md` pass: the workspace tests, clippy and `delphi lint`; and the docs-drift test (`cargo test -p delphi --test docs`) passes, with `docs/reference/` regenerated if any generated page cites a moved spec.
6. **Sample.** The operator reads a sample of the map: every row decided by rule 3 (reading) and at least thirty others, with **zero wrong decisions tolerated** in the sample. One wrong row sends the whole rule that produced it back for re-checking.
7. **Reviewability.** The pull request is a sequence of commits that can be read one at a time: (a) the three renames and their own edits; (b) path pointers; (c) citations decided by location; (d) by content; (e) by reading; (f) the lint rule; (g) `CONTRIBUTING.md`, the tracker and the map. Reverting the pull request restores the old state exactly.

---

## 6. Risks

- **A wrong citation decision that nothing catches.** The main risk. Mitigated by the location rules, the map, the one-time strict check and the sample, not removed. The residual risk is a wrong "stays" or "changed" in a file where both specs share a section number and the surrounding text does not tell them apart.
- **Parallel sessions.** Open pull requests that cite the three numbers, or that add a spec, collide with the renames. The work starts in a quiet window the operator picks; open pull requests touching the cited files are rebased onto it; the new numbers are claimed in the tracker first.
- **The new numbers are taken** before the work starts. The next free numbers are used, and the file names and this spec's examples change; nothing else depends on 095 to 097. (This already happened once while the spec was drafted: 091 to 094 were taken.)
- **Large diff.** Many files change, almost all by one number in a comment. The commit sequence in §5 test 7 and the map keep it reviewable.
- **Historical text stays ambiguous.** Accepted and recorded (§3.6).
- **The operator chooses the other member of 014.** Then two constitutional files change; §2.2 makes that a separate ratified step and this spec's S1 does not start without it.

---

## 7. Slices (one pull request each)

| Slice | Content | Depends on | Hours (low / likely / high) |
| :-- | :-- | :-- | --: |
| S0 | spec approval; tracker entry; the three numbers claimed | operator | 1 |
| S1 | §3 in one pull request: renames, path pointers, citation map and decisions, the one-time check, the lint rule, `CONTRIBUTING.md`, the tracker; confirm in its description whether a ratification lock applies | S0, a quiet window | 8 / 12 / 18 |

Operator time: about one to two hours (the sample, the quiet window, and the refresh of three ratification entries if a lock applies). The hours are estimates from the counts in §2.1; no trial rewrite has been run, and the citation work (§3.3) is the part most likely to move the estimate.

---

## 8. Decisions for the operator

**All six were taken as recommended by the operator on 2026-11-07** ("approve both 088 and 090"), after the recommendations were shown. Each recommendation below is therefore the decision.

- **OD-1. Which spec of each pair moves** (§2). *Recommendation:* documents and dataviz (014), training experiments (037), encyclopedia search pages (061). It keeps the relay spec at 014 and so keeps GOVERNANCE.md and REVIEW.md untouched. The alternative for 014 (move relay) is possible with a separate ratified constitutional step.
- **OD-2. Historical records** (mission notes, council records, history files, past changelog entries). *Recommendation:* leave them as they are, explained by the mapping paragraph (§3.6), and rewrite one only where a gate fails on it. Rewriting history for tidiness is large and adds risk for no gate's benefit.
- **OD-3. The new numbers.** *Recommendation:* the next three free at the start, expected 095 to 097 as of 2026-11-07, named in the tracker by the approval pull request and re-checked when S1 starts.
- **OD-4. When.** *Recommendation:* in a quiet window with no open pull request that touches the cited files, and not while spec 088's first slice is mid-flight. The operator picks the day.
- **OD-5. The sample** (§5, test 6): thirty rows plus every hand-decided row, zero errors tolerated. *Recommendation:* yes; raise the number if the first sample finds anything.
- **OD-6. The note line inside each renumbered spec** (§3.1). *Recommendation:* yes, one line, so a reader of the file sees why the number changed. It is the only text added to the three specs.
