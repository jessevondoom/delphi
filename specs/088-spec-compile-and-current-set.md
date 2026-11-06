---
id: spec-088-spec-compile-and-current-set
title: "088: A spec compile step: per-spec machine files and grouped human views of the current set"
author: architect
timestamp: 2026-11-06T18:00:00Z
status: draft   # a proposal, not permission to build (AGENTS.md); awaits the operator
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # repository facts counted on main on 2026-11-06; the prior-art survey (§9) is partly unverified and says which parts
source_mission: 2026-10-23-foundational-design
tags: [spec, compile, requirements, supersession, agents, review, drift]
---

# Spec 088: A spec compile step

> **Status: draft, 2026-11-06.** Written from a conversation with the operator the same day and a prior-art survey (§9). Decisions OD-1 to OD-7 (§11) are open; each carries a recommendation. Nothing is built.
> **Extends:** the folding rule in [`CONTRIBUTING.md`](../CONTRIBUTING.md) ("Folding spec amendments") and the reference gate `crates/delphi/tests/spec_refs.rs`. It changes no spec's meaning and no approval rule.
> **Number:** 087 is the latest merged spec; 042 stays reserved (spec 041 §7); no open pull request claims 088.

---

## 1. Scope and objectives

Development is fast and specs are corrected as it goes. An agent or a validator that asks "what is currently required?" has to read the specs themselves, and they do not fit in a context: on 2026-11-06 `specs/` held 97 spec files and about 325,000 words, and 68 of them mention superseding or amending something. Today the current state is kept readable by **folding by hand**: when a spec's amendments are folded into its body, the removed text moves verbatim to `specs/history/` and every section number survives as a stub. That works, but it is manual, it rewrites approved files, it is done spec by spec, and nothing checks across specs.

This spec adds a **compile step** that *derives* the current set from the source specs, so currency stops depending on someone remembering to fold:

1. **Extract, never rewrite** (§2). The compiler copies text out of the source specs byte for byte. No model runs in it, and nothing it writes can say more than a source says.
2. **One JSON file per spec, for agents** (§4). Each carries the spec's metadata, its requirements with a state (active, superseded, withdrawn), its amendments, the amendments the compiler could not apply, a hash of its source, and the field `authoritative: false`.
3. **A few grouped markdown views, for review** (§5), rendered from those JSON files and never from the sources a second time. A spec names its group in its frontmatter, so the grouping is deterministic. An index table sits on top.
4. **Stable requirement IDs with revisions and tombstones** (§6), so "what changed and what must be re-tested" is a computation.
5. **Declared amendments and supersession, validated in both directions** (§7). Contradictions nobody declared are out of reach of a compiler and stay with the `contradiction-pruning` skill (§1.1).
6. **Skills** (§10) that teach authors to write extractable specs and builders to read the compiled set without trusting it past the source.

### 1.1 Non-goals

- **No authority moves.** The source spec stays the authority and approval stays the operator's. A compiled file is a view: it says so in a field, carries the source hash, and is never cited as the spec. "No Spec, No Build" is unchanged; a builder still opens the source section it implements (§10.2).
- **No model in the compiler, and no summarizing.** A spec's "summary" is a verbatim slice (§4.2) or absent. Deterministic work is done in code.
- **No semantic contradiction detection.** The compiler applies declared supersession and amendments. Two specs that disagree without declaring it are found by the `contradiction-pruning` skill and `delphi check kb`, as today.
- **No change to the folding rule.** Folding stays allowed and its history files stay. Compile makes folding optional for currency; it may still be done for the source's readability.
- **No requirement-ID retrofit of all 97 specs.** Every spec gets metadata-only compilation (tier 0, §3); IDs go on the specs still being built or validated (§9.1, S5).
- **No test-coverage gate yet.** Coverage of requirements by tests is a report first (S6) and a gate only if the operator later says so (OD-6).

### 1.2 Simpler alternative considered

Generate only the index table (what PEP 0 does for Python: a script reads each header and writes an index). That is S2 plus the table in S3 and needs no IDs, no registry and no move. It answers "which specs exist, in what state, superseded by what" and nothing about requirements. It is the right first measurement: S2 and S3 ship first, and S4 onward waits until agents are seen to use the output (§8). If they do not, nothing past S3 is built.

---

## 2. Invariants

1. **Extraction is byte-exact.** For every requirement, summary and amendment text in a compiled file, the text equals a contiguous slice of the source file at the recorded line span. A test re-slices every source and compares.
2. **The human view is rendered from the JSON.** `delphi specs compile` reads sources and writes JSON; the same command then renders markdown from the JSON, not from the sources. Reviewing the markdown therefore reviews what agents read. Two tests: JSON equals a fresh compile of the sources; markdown equals a fresh render of the JSON.
3. **One parser.** The compiler, `delphi lint`'s spec checks and `spec_refs` call the same parsing code (a module in `delphi-core`). Two parsers that disagree about what a heading or an ID is was the failure behind OpenSpec's silent-deletion bugs (§9).
4. **Compiled output is never edited and never the authority.** Every file starts with a generated banner (markdown) or `authoritative: false` and `generated_by` (JSON).
5. **The compiler does not guess.** An amendment or supersession it cannot apply mechanically is listed under `unapplied` in the JSON and in the view, and `delphi lint` reports it. A current view that silently lags the truth is worse than one that says what it has not applied.
6. **Status comes from the source.** The compiler copies `status`, `approved_by` and the dates from the frontmatter. It never sets or changes them. A draft spec appears, labelled draft, and its requirements are not shown as active authority.

---

## 3. Two tiers

| Tier | Which specs | What compiles |
| :-- | :-- | :-- |
| **0 (metadata)** | every spec | frontmatter (number, title, status, group, approval, `supersedes`, `superseded_by`), heading outline, a verbatim summary if the spec has the standard scope section, the source hash |
| **1 (requirements)** | specs whose source has requirement tags (§6) | tier 0 plus requirements, revisions, states, amendments, `unapplied` |

A spec reaches tier 1 by being tagged, which is a source edit that goes through the same review as any spec edit. The view shows the tier. Tier 0 alone answers "what exists, in which state, in which group, superseded by what" for all specs on day one.

---

## 4. The machine files

### 4.1 Layout

```
specs/
  README.md                  how this works, in one page
  source/                    the canonical specs (moved by S1; see OD-1)
    NNN-*.md   history/   assets/
  current/                   generated; never edit
    index.md                 the table of every spec (human)
    <group>.md               one view per group (human)
    groups.toml              the closed list of groups: id, title, order (hand-written)
    data/
      NNN.json               one per spec (machine)
```

There is deliberately **no aggregate JSON**. Parallel pull requests that touch different specs then touch different files. An agent finds a spec by number, and the CLI and MCP surface (§10.3) list them. (A tool named `specs_status` already exists on the MCP server; S3 decides whether to extend it or add a sibling.)

### 4.2 Contents of `data/NNN.json`

A JSON Schema, `schemas/spec-current.schema.json`, is added with the others (and the schemas guide and the docs-drift test are updated). Fields:

- `schema_version`, `authoritative: false`, `generated_by` (compiler version), `as_of` (the source file's last-commit date, not the wall clock, so recompiling changes nothing).
- `source`: `path`, `sha256` of the file, `lines`.
- `spec`: `number`, `title`, `status`, `group`, `approved_by`, `approved_on`, `supersedes`, `superseded_by`, `tier`.
- `summary`: the first paragraph under the spec's "Scope" heading, verbatim with its span, or `null`.
- `outline`: the heading numbers and titles with line numbers.
- tier 1 only:
  - `requirements[]`: `id`, `rev`, `state` (`active`, `superseded`, `withdrawn`), `text` (verbatim), `section`, `lines`, `replaces`, `superseded_by`, `withdrawn_reason`.
  - `amendments[]`: the amendment records (§7.1) in source order.
  - `unapplied[]`: each amendment or supersession the compiler could not apply, with the reason.
  - `ids_issued`: every ID the spec has ever issued with its latest `rev` and `state`. This is the registry (§6.3); it lives here, not in a shared file.

### 4.3 What the compiler refuses

It fails (not warns) when a source has a duplicate ID, an ID with no text and no tombstone, a `replaces` or `superseded_by` that points at no ID, a supersession declared on one side only, or a tier 1 spec whose frontmatter lacks `group`. These are the checks that, in the tools surveyed, were convention only (§9).

---

## 5. The human views

`delphi specs compile` writes `current/index.md` and one `current/<group>.md` per group listed in `groups.toml`.

- **Index.** One table: number, title, status, tier, group, supersedes / superseded by, `unapplied` count, link to the group view and the source. It is the page a person opens first.
- **Group view.** For each spec in the group, in number order: title, status line, the verbatim summary, and (tier 1) the **active** requirements as a list with their IDs and revisions. Superseded and withdrawn requirements are not listed; the spec's line shows how many exist and links to its JSON. Every entry links to its source file and section, where the rationale lives.
- **Banner.** The top of every generated file: "Generated by `delphi specs compile` from `specs/source/`. Not the authority: the source spec governs. As of <date>. Do not edit."
- **Grouping.** A spec's frontmatter has `group: <id>`; `groups.toml` is the closed list, so a typo fails the compile. The starter list is in OD-4.

The markdown is committed, so a pull request that changes a requirement shows the change to the current set in its diff. OD-3 weighs the cost.

---

## 6. Requirement IDs

### 6.1 Form

In the source, a requirement is one paragraph or list item that begins with its tag in bold square brackets and then the statement:

```
**[R4]** The relay never raises the trust level of content it forwards.
```

The global form is `NNN-R4` (spec number, then the local ID), and a revision is written `NNN-R4@2`. IDs are issued by taking the next unused number in the spec, are never reused, and are never renumbered. A statement is normative if it is tagged; untagged prose is informative. Rationale, history and examples stay untagged in the source.

### 6.2 Revisions

`@rev` starts at 1 and increases when a requirement's meaning changes. A wording fix that does not change meaning keeps the revision (the author says which in the amendment record, §7.1). Tests and code may cite `NNN-R4@2`; when the active revision becomes 3, anything pinned to 2 is *outdated* by definition, and the coverage report (S6) lists it. This is OpenFastTrace's revision-in-the-ID idea, kept because it turns "what must be re-tested" into a query.

### 6.3 Registry and tombstones

A requirement is never deleted from the source; it is withdrawn in place:

```
**[R4]** (withdrawn 2026-11-09, 088-A1: the relay no longer forwards content)
```

The compiled `ids_issued` records every ID the spec has issued. A CI check compares each pull request's `data/NNN.json` with the one on the base branch and fails when an ID disappears, when a `rev` goes down, or when a requirement's text changed with no revision bump and no amendment record. The registry is therefore the committed compiled file plus git, not a new shared file that every pull request would edit. For a spec that is new in the pull request there is nothing to compare, so the check passes.

### 6.4 Seeding

Many specs already number their acceptance criteria and verification tests so that "tests and PRs can cite them" (the `spec-authoring` skill, step 4). The retrofit tool (S5) proposes tags for those numbered items as a patch for the Architect to review; it adds tags and changes no words. Statements in design sections are tagged by hand.

---

## 7. Amendments and supersession

### 7.1 Amendment records

The repository already uses amendment IDs `NNN-An` (for example `004-A1`, kept in `specs/history/`). Tier 1 specs keep one section, "Amendments", whose entries are a list of records:

```
**[A2]** 2026-11-09. R4 rev 1 to 2 (meaning changed); R7 added; R9 withdrawn. Reason: <one line>. Approved: <who, when>.
```

The reason is required. The compiler parses the record, checks that every ID it names exists and that each revision change in the source matches one record, and copies the records into `amendments[]`. Prose in the amendment, if any, is untouched and is not extracted.

### 7.2 Supersession

- *Within a spec:* a tagged requirement may say `(replaces R3)`. The old one is marked `superseded-by` in place, a status-only edit as `adr-tools` does, and the compiler requires both sides.
- *Across specs:* the replacing requirement says `(replaces 085-R3)`; the old requirement in 085's source carries `(superseded by 088-R2)`. The compiler does not write into another spec's file or JSON. If either side is missing the compile fails and says which (§4.3). This keeps a pull request's compiled changes inside its own specs.
- *Whole specs:* frontmatter `supersedes: [NNN]` / `superseded_by: NNN`, same two-sided rule, same lint.

---

## 8. Verification plan

1. **Byte-exact (§2.1).** For every compiled file in the repository, each extracted text equals the slice of its source at its recorded span.
2. **Derivation (§2.2).** JSON equals a fresh compile; markdown equals a fresh render of the JSON. Both fail with a message naming the command to run, the way `tests/docs.rs` does.
3. **Parser parity (§2.3).** `spec_refs` still passes, with its section and amendment resolution moved onto the shared parser, and the allowlist unchanged.
4. **Refusals (§4.3).** One test per refusal, each with a minimal fixture spec.
5. **Registry (§6.3).** A fixture that drops an ID, lowers a `rev`, or changes text without a revision, against a base copy, each fails; a legal amendment passes.
6. **Supersession (§7.2).** One-sided declarations fail in both directions, within a spec and across specs.
7. **Draft safety (§2.6).** A draft spec's requirements are labelled draft in JSON and view and are not counted as active authority.
8. **Idempotence.** Compiling twice changes nothing. `as_of` comes from git, not the clock.
9. **Whole corpus.** `delphi specs compile --check` passes on all 97 specs at tier 0 before S2 merges, with every failure either fixed in the source or listed in an allowlist that only shrinks, as `spec_refs` does.
10. **Measurement before S4.** Before the requirement slices are built, S3 is used for a stated period (OD-5) and the tracker records whether agents opened the compiled files, from the session tool-call records (S3 first confirms which records name a file read; if none does, S3 adds a counter), and what they were used for. The result decides whether S4 onward proceeds.

`delphi lint` and the gate commands in `AGENTS.md` pass in every slice. Slices that add a command regenerate `docs/reference/` (`docs cli`, `docs man`) and, for a new skill, `docs catalog`.

---

## 9. Prior art (surveyed 2026-11-06)

Primary sources were fetched where possible; the fetcher summarizes pages, some pages did not load, and items it could not confirm are marked unverified here and should not carry weight.

**What the survey supports.**

- Sidecar metadata beside prose, with `replaces` / `superseded-by` kept consistent: Kubernetes KEPs (`kep.yaml` template: status values including `replaced`; `replaces`) and PEP 1 (`Superseded-By` on the old, `Replaces` on the new).
- An index generated from headers by a script: PEP 0 (`pep_zero_generator/writer.py`). It carries no "generated" banner and the survey could not tell whether PEP 0 is committed or built; this spec adds the banner.
- A superseding record plus a status-only edit to the old one: `adr-tools`. A second source argues for date-stamped additions to mutable ADRs; this spec's tombstones are the same idea.
- A machine ID separate from the human one, surviving renames: StrictDoc's MID (opt-in there).
- Revision in the ID and "outdated" link detection, `Needs:` coverage declarations, duplicate IDs reported and not merged: OpenFastTrace (`type~name~revision`).
- Consolidated text that is dated, labelled as having no legal effect, tagged by source and with a list of known-unapplied changes: EUR-Lex consolidated texts; legislation.gov.uk "Latest available (Revised)", with a warning that it may not be fully up to date. This is the model for `unapplied` and for the banner.

**What it contradicts or does not support.**

- *No surveyed spec-driven tool extracts from approved prose.* OpenSpec, Spec Kit and Kiro author specs with models. There is no template for the compile step, only parts.
- *OpenSpec is not a model for this.* Its archive step applies deltas into the main spec, which rewrites the authority; it identifies requirements by header text, and its tracker (PRs #1490, #843, issue #498, read as search summaries) shows silent deletions and a validator that passed what the archive rejected. This spec uses explicit IDs, a single parser and no application of deltas to sources.
- *Stable IDs are convention only* in OpenFastTrace, StrictDoc and the others checked. The registry in §6.3 is this spec's answer, and the survey found no tool that has one.
- *Generated aggregates conflict.* Merge drivers are local configuration, and "keep ours" hides drift. Hence per-spec JSON, no aggregate JSON, and OD-3.

**Not verified:** `kepctl` and any KEP CI validation; Sphinx-Needs ID uniqueness behaviour; doorstop suspect links; how the RFC index is built; any Spec Kit or Kiro ID scheme; the ordinary regenerate-and-diff CI pattern (standard practice, no source found).

### 9.1 What the repository already has

- The folding practice and `specs/history/` (CONTRIBUTING); five specs are folded (004, 006, 007, 010, 043).
- The `spec_refs` gate with its parser for `NNN §X.Y` and `NNN-An`.
- Frontmatter `status: approved   # comment`: a YAML comment, so `status` already parses as one of `draft`, `approved`, `active`; the approval provenance in the comment is free text. S2 adds `approved_on` and `approved_by` as fields and leaves existing comments alone.
- The `spec-authoring`, `contradiction-pruning` and `knowledge-compaction` skills, and `delphi check kb`, which already flags live documents linking to superseded ones (S2 first checks whether it covers `specs/`).
- Numbered acceptance criteria in many specs (§6.4).

---

## 10. Skills and surfaces

### 10.1 Authors (Architect)

Extend `spec-authoring`: write atomic tagged requirements; keep rationale untagged; amend with a record and a reason instead of editing in place; withdraw with a tombstone; declare supersession on both sides; set `group`. Ships with S4, because the skill must not describe tags the compiler does not yet read.

### 10.2 Consumers (Builder, Validator, Red-Team)

A short new skill, `spec-driven-build`, authorized for those three roles (the `authorized_skills` lists in `agents/` are governed files and change in the same slice). It teaches:

1. read `current/data/NNN.json` first, for what is active and what was replaced;
2. **open the cited source section before building**; the JSON is an index to it, never a replacement. Compare `source.sha256` with the file; if they differ, the JSON is stale, say so and recompile;
3. cite IDs in tests (`covers 088-R4@2`) and in the pull request, alongside `Implements-Spec`;
4. if reality contradicts a requirement, set `spec_drift_detected` against that ID, as the Builder already does against the spec (`agents/builder/AGENT.md`), and halt. The compiled file makes drift easier to name, not easier to ignore.

### 10.3 Surfaces

`delphi specs compile [--check]`, `delphi specs show NNN [--json]`, `delphi specs list [--group G] [--status S]`, and the MCP counterpart (S3). `AGENTS.md` and `llms.txt` point agents at `specs/current/`.

### 10.4 Commit trailer

`Implements-Spec: specs/NNN-*.md` stays valid. After S1 the lint resolves a trailer by **spec number**, so the old path form (in history and in open pull requests) and the new path both pass and the folder name stops mattering.

---

## 11. Slices (one pull request each) and decisions

| Slice | Content | Depends on | Hours (low / likely / high) |
| :-- | :-- | :-- | --: |
| S0 | spec approval; tracker entry | operator | 1 |
| S1 | move the canonical specs to `specs/source/`; rewrite links; lint resolves trailers by number; no content change | S0, OD-1 | 4 / 6 / 10 |
| S2 | shared parser in `delphi-core`; `spec_refs` moved onto it; tier 0 compile; `schemas/spec-current.schema.json`; frontmatter `group`, `approved_by`, `approved_on`, `supersedes`; `groups.toml`; the corpus passes `--check` | S1 | 10 / 16 / 24 |
| S3 | index and group views, banner, drift tests, `specs show/list`, MCP surface, CONTRIBUTING, `specs/README.md`; start of the measurement (§8.10) | S2 | 8 / 12 / 18 |
| S4 | requirement tags, revisions, tombstones, amendment records, registry check against the base branch, two-sided supersession; `spec-authoring` extended | S3 and the measurement | 10 / 16 / 26 |
| S5 | `spec-driven-build` skill; builder, validator and red-team authorized; `AGENTS.md`, `llms.txt`; the retrofit tool; tag the specs still being built | S4 | 12 / 20 / 34 |
| S6 | coverage report: active requirements with no citing test, and outdated pins; warn-only | S5 | 6 / 10 / 16 |

Council: this changes how every agent reads specs, so a Council review of this draft is recommended before approval, as spec 086 did for its own draft.

### Decisions for the operator

- **OD-1. Move the sources to `specs/source/`** (S1). *Recommendation:* yes, as its own mechanical pull request, with number-based trailer resolution first. It touches links in specs, docs, code comments, the changelog and `AGENTS.md`, and `spec_refs` will show any it misses. The cheaper alternative is to leave the sources and put only the outputs in `specs/current/`; it loses the clean split and nothing else.
- **OD-2. Syntax** (§6.1): `**[R4]**` in the source, `NNN-R4@2` globally. *Recommendation:* yes; it reads as plain markdown and is hard to write by accident. The exact form is the S4 reviewer's to adjust, the meaning is fixed here.
- **OD-3. Commit the markdown views?** *Recommendation:* yes, so a pull request's diff shows what changed in the current set, matching `docs/reference/`. The cost is that two pull requests changing specs in the same group can conflict in `current/<group>.md`; the fix is to rerun `delphi specs compile`, and the drift test is the backstop. The per-spec JSON does not have this problem. The alternative (build views on demand, commit only JSON) removes the conflicts and the diff.
- **OD-4. Groups.** *Recommendation:* a closed list in `groups.toml`, decided in S2 by the operator from this starting set: `governance`, `runtime` (adapter, engine, harness), `console`, `knowledge-and-memory`, `council-and-learning`, `trust-and-xenia`, `bundles-and-external`, `docs-and-tooling`.
- **OD-5. Measurement before S4:** how long, and what counts as use. *Recommendation:* two weeks of normal work after S3, counting agent reads of `current/data/` against reads of `specs/source/`, and reading a sample of the sessions. If agents never open it, stop at S3.
- **OD-6. Coverage as a gate.** *Recommendation:* report only (S6). A gate on requirements without tests would fail most of the corpus on day one.
- **OD-7. Strictness on `unapplied`.** *Recommendation:* the compile lists them and `delphi lint` warns; it fails only on the refusals in §4.3. An unapplied amendment is information about the sources, not an error in the compiler.
