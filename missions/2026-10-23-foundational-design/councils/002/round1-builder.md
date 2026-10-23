---
id: council-002-round1-builder
title: "Council 002 round1 builder (verbatim)"
author: builder
timestamp: 2026-10-23T19:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-002, verbatim]
model: "Claude (this session)"
---

> Verbatim seat output. Line numbers refer to the files as they stood when this round ran (commit `6bb4cb5` plus uncommitted edits).

# Council 002, Round 1 (blind): Builder seat (Claude)
Focus: can Step 1 be built and tested from specs 001 and 006 as written?

## Findings
- **[P0]** `specs/001#L184-L191`, `specs/006#L100`: **Markdown not stamped by the daemon has undefined taint, so it can launder trust.**
  - **Failure mode:** §5.1 computes taint for spans and for non-Markdown files, but a Markdown file that arrives some other way (team sync, a raw `git pull` in `team/`, a hand edit, the init seed) is read with its frontmatter `source_type` as written. A teammate pushes `team/context/domains/x.md` containing `source_type: user_ratified` and a hidden instruction. A span that reads it stays at L5 and passes the promotion gate.
  - **Remediation:** the daemon keeps a provenance index (`path, blake3 → taint`) for every file it stamped or ratified. Frontmatter `source_type` is only a display copy. A file whose hash is not in the index takes its taint from its origin: `team/` → L2, local edits outside the daemon → L3, unknown → L0. Seeded release files take the tier recorded in the signed `core-manifest.json`.
- **[P0]** `specs/006#L77`, `#L100`: **Permission manifests can change without ratification.**
  - **Failure mode:** `ratified.lock` covers only GOVERNANCE, REVIEW and heuristics. A synced `team/agents/custom/builder/AGENT.md` overrides the built-in Builder, with `write: ["**"]` and new `execute` gates, and takes effect on the next start. The same applies to `skills/custom/**/scripts` and to operator edits in `core/agents/`.
  - **Remediation:** add every `AGENT.md` (built-in, `core/` and `custom/`), every `SKILL.md`, and everything under `skills/custom/**/scripts/**` to `ratified.lock`, staged as Red-tier proposals like the constitution. State that an override's `default_access_scope` is validated by the schema linter against REVIEW `[REV-SEC-02]` before it can be ratified.
- **[P1]** `roadmap Step 1 DoD`, `specs/001#L163-L178`: **The DoD fails on this repo today.** "This repo passes the validator" fails on 15 files: missing `source_mission`/`tags` in heuristics, the index, all 9 lessons seeds, the roadmap and the mission file, and `author` values (`mnemosyne`, `ares`, `athena`) outside the role enum. §5 also never says which fields are required (`ttl_missions`? `superseded_by` only when superseded).
  - **Remediation:** mark each field required or optional in §5 (required: id, title, author, timestamp, source_type, confidence, status, source_mission, tags; optional: ttl_missions, superseded_by). Add the `delphi-core` author for daemon-written files. Fix the 15 files before approval.
- **[P1]** `specs/001#L49-L51`, `#L234`, `skills/`: **Skills are referenced, but none exist.** All 9 manifests list `authorized_skills`, and `CATALOG.md` lists 42, but no `skills/<name>/SKILL.md` exists. Test 1 passes vacuously, or fails if the linter checks that referenced skills exist.
  - **Remediation:** state the rule: a referenced skill must exist as a package **or** be listed in `CATALOG.md` with `status: planned`, which the linter warns on rather than failing. Also specify that `authorized_agents` (SKILL.md) and `authorized_skills` (AGENT.md) must agree; a mismatch is a lint error.
- **[P1]** `specs/006#L44-L48`, `#L80-L103`: **Workspace resolution is unspecified.** Nothing says how `delphi`, run inside a project directory, finds its workspace when several workspaces register the same project, or none does. `doctor` and `init` both need this.
  - **Remediation:** resolve by walking up from the current directory to a git root, then looking for a unique workspace whose `config.toml` `projects` contains it. If there are several matches, require `--workspace`; if none, offer `delphi init`.
- **[P1]** `specs/006#L124-L134` vs `roadmap Step 1 DoD`: **"`doctor` exits `0` on a fresh init" contradicts check 7.** Provider reachability fails offline or without keys, and offline init is a stated goal. Checks 6 (sqlite-vec, deferred by D-01) and 7 (CLI harnesses, deferred by D-06) also check things that don't exist in v1.
  - **Remediation:** give `doctor` three outcomes (`ok` / `warn` / `fail`, where only `fail` exits 1). Missing keys and unreachable providers are `warn`. Drop the vector and harness checks until those features ship.
- **[P1]** `specs/006#L100`: **`ratified.lock` bootstrap is undefined.** Who ratifies the seeded constitution, theme and manifests on first `init`?
  - **Remediation:** `init` writes `ratified.lock` from the release's signed `core-manifest.json` (release ratification). Operator ratification is needed only for divergence from it.
- **[P1]** `specs/006#L110-L113`: **`delphi rollback` restores only the binary.** After a migration has run, the old binary meets a migrated `state.db` and a new `base/`.
  - **Remediation:** rollback restores `state.db.bak-<version>` and the previous `base/` snapshot, and is refused if the event log gained events of a newer schema version since then. Mark as Step 5 (packaging), not a Step 1 blocker.
- **[P2]** `specs/001#L37` vs `specs/006#L77`: the custom-agent path is `.delphi/agents/custom/` in 001 and `agents/custom/` (Zone 2, `team/`) in 006. Use `team/agents/custom/<id>/`.
- **[P2]** `specs/001#L235`: test 2 still says "valid `file://` pointers"; pointers are workspace-relative now.
- **[P2]** `specs/001#L58`: path canonicalization should NFC-normalize names (APFS is normalization-insensitive), or `memory/` and a decomposed-Unicode variant compare differently.
- **[P2]** `specs/006#L46`: init generates a federation keypair, but federation is deferred (D-06). Mark it later-phase.
- **[P2]** `specs/006#L19`: the scope list of seeded content omits `templates/`, `themes/`, seed `context/` and `memory/` (listed in §3.1).

## Checked and sound
- The logical-path model resolves before containment; zone roots are disjoint.
- Taint-gated tool table and stamping rule for spans.
- Theme pack schema, the ratify-before-load rule, and the persona slot below governance.
- Update drain-and-stop, constitution Red gate, binary signature verification.
- `.delphi/` shorthand defined once; the clean-repo invariant is checkable.

## Recommendation
- **001:** `approve-with-fixes` (both P0s, plus the frontmatter, skills and workspace P1s).
- **006:** `approve-with-fixes` (ratified.lock scope and bootstrap, the doctor outcome model, workspace resolution).

`concur: false` (two unresolved P0s).
