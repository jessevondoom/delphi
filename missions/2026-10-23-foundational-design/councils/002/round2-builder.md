---
id: council-002-round2-builder
title: "Council 002 round2 builder (verbatim)"
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

# Council 002, Round 2: Builder seat (Claude), cross-examination

## Red-Team findings
- **F1 (`.git` file hijack): agree, P0.** Verified: in a linked worktree, `.git` is a regular file (`gitdir: …`), so neither `.git/**` nor `<repo>/.git/**` matches it, and Builder's `@worktree/**` glob covers it. The remediation should go further than a denylist. The daemon must invoke git with an explicit `--git-dir`/`--work-tree` and with hostile config neutralized: `-c core.fsmonitor=false -c core.hooksPath=/dev/null`, `GIT_CONFIG_NOSYSTEM=1`, and no `include.path` honored from writable locations. A rewritten gitfile is otherwise only one of several routes to repo-local config.
- **F2 (custom manifests unpinned): agree, P0.** Same as my Round 1 P0 #2. Red-Team's narrow-never-widen rule is the stronger fix; adopt both (narrowing only, and pinned in `ratified.lock`).
- **F3.1 (hard links): agree, P0. Red-Team is right and Strategist's "checked sound" #1 is wrong.** `O_NOFOLLOW` refuses only a symlink in the final path component; a hard link is a regular file (`S_IFREG`, `st_nlink > 1`). Agent file tools don't create links, but sandboxed code can (`ln`), and worktrees share a filesystem with `local/`. Fix in two layers: the sandbox exposes only the worktree and a temp dir (so protected targets can't even be named), and file tools reject `st_nlink > 1` and protected `(st_dev, st_ino)` pairs.
- **F3.2 (`read: ["*"]` reaches peer lessons and `state.db`): agree, P1.** This contradicts GOVERNANCE §4.1 isolation.
- **F3.3 (proposals path split): agree, P1.** Proposals must live in a daemon-only `local/proposals/`. Otherwise Orchestrator can edit a proposal after Red-Team has reviewed it.
- **F4 (taint can never rise; sandbox writes skip the taint map): agree, P0.** A real contradiction in text I wrote. Merge it with my Round 1 P0 #1 (Markdown provenance) into one **provenance model**:
  - an external ceiling that only goes down;
  - local output stamped `min(L3, ceiling)`, or `min(L4, ceiling)` after a passing check;
  - a persisted `(path, blake3) → taint` index covering every file, Markdown and otherwise;
  - files changed during an execution step attributed to that step's span, by diffing against the pre-step checkpoint tree;
  - origin defaults for files the daemon didn't write.
- **F5 (Step 1 DoD fails on this repo): agree.** Same as my P1 and Strategist P0-2.
- **F6 (update swaps the binary before the constitution gate; single-workspace drain; downgrade): agree, P1.** Correct, but it's Step 5 work. Fix the ordering text now because it's cheap, and mark §4 as re-reviewed before Step 5.
- **F7 (theme XML breakout, `@name` collisions): agree, P1.** Broaden it: *every* string the adapter interpolates into tagged prompt sections must be escaped, not just theme fields.

## Strategist findings
- **P0-1 (offline init and doctor): agree.** Same as my doctor P1. The `--non-interactive` flag and `ok/warn/fail` outcomes together fix it.
- **P0-2 (repo fails validator): agree.** The fenced-code-block exclusion for check 5 and the `CATALOG.md` frontmatter are additions I missed.
- **P1-3 (no phase table): agree, and it's the most important structural fix.** It's what lets F6, rollback, `theme new` and runtime taint be approved as design without blocking Step 1.
- **P1-4 (incomplete zone mapping, `base/` vs binary, missing tree entries, `.py` names vs D-02): agree.**
- **P1-5 (no Step 1 tests, no source-repo lint mode): agree, P1.** A `delphi lint --repo` mode is required, because this repo uses the flat source layout rather than a workspace home.
- **P2-6 (doctor check 4 omits the 350-char cap): agree.**
- **Checked-sound #1 (hard links): disagree.** See F3.1.

## Missed by all three
1. **P1: the clean-repo check would flag Delphi's own repo.** Doctor check 8 says a registered project must "contain no Delphi files", but under D-03 this repo is registered as a project and *contains* `GOVERNANCE.md`, `agents/` and so on. Define it by provenance, not content: no `.delphi/` directory, and no file created or modified by Delphi outside a merged worktree.
2. *(Restating from my Round 1, not raised by Gemini):* `ratified.lock` bootstrap at first init, and workspace resolution when several workspaces register the same project. Both are needed for Step 1's `init` and `doctor`.

## Must-fix before approval
- **001:** F1, F2, F3.1–F3.3, the merged provenance model (F4 + Builder P0 #1), F5/S-P0-2 (schema optionality, catalog-backed skills rule, test 2 wording, repo frontmatter and name fixes), F7 (broadened to all interpolation), S-P1-3 phase table, S-P1-4 mapping and D-02 names, custom-agent path.
- **006:** S-P0-1, S-P1-3 phase table (federation keygen deferred), S-P1-4 tree entries, S-P1-5 Step 1 tests and lint mode, `ratified.lock` bootstrap, workspace resolution, clean-repo definition, F6 ordering text, S-P2-6.
- **Deferred via the phase table:** rollback state restore (Step 5), `delphi theme new` (post-v1), diff3 (Step 6).

`concur_after_fixes: true`
