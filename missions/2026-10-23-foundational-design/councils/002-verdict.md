---
id: council-002-verdict
title: "Council 002: Stage 1 Review of Specs 001 and 006"
author: orchestrator
timestamp: 2026-10-23T19:20:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-002, verdict]
---

# Council 002: Stage 1 Review of Specs 001 and 006

**Proposition:** can `specs/001-governance-and-manifests.md` and `specs/006-installer-and-updater.md` be approved so roadmap Step 1 can begin?

**Seats (heterogeneous, `[H-001]` satisfied):**
- `builder`: Claude, this session.
- `red-team`: Gemini 4 Argon High, via `agy`.
- `strategist`: Gemini 4 Argon Medium, via `agy`.

Both critics sat on a different family from the specs' author (`[H-007]`). Round 1 was blind: the Gemini seats started before the Builder seat wrote anything. All six transcripts are kept verbatim in [`002/`](002/).

## Verdict
**Approvable once the agreed fixes land. They have now landed in the working tree.**
- **Round 1:** all three seats said `concur: false`, with 9 P0 findings between them, several overlapping.
- **Round 2:** all three said `concur_after_fixes: true`.
- **Synthesis:** found no unresolved P0. The deterministic escalation rule (`specs/003` §4.1) therefore doesn't trigger, and the decision passes to the operator as a normal approval.

## Findings & dispositions
| Finding (seat) | Sev | Disposition |
| :-- | :-- | :-- |
| Worktree `.git` is a *file*; rewriting it redirects the daemon's host git calls (RT F1) | P0 | **Fixed:** `.git` denied at any depth, file or dir (001 §2.2, `REV-SEC-02`, 008 §4); hardened git invocation (explicit `--git-dir`, config neutralized) |
| Custom agent and skill overrides unpinned; can widen permissions (RT F2, B P0-2) | P0 | **Fixed:** 001 §2.3 precedence, all manifests, skills, scripts and theme in `ratified.lock`, narrow-only, lint-enforced; the daemon loads only ratified copies (006 §3.1) |
| `O_NOFOLLOW` doesn't stop hard links; `read: ["*"]` exposes peer memory and `state.db`; proposals path split (RT F3) | P0 | **Fixed:** `st_nlink` and protected-inode check, sandbox visibility, read denylist, daemon-only `local/proposals/` via the `propose` tool. Strategist's "checked sound" claim on hard links was retracted in Round 2. |
| Taint could never rise (deadlock); sandbox writes skipped the taint map (RT F4); unstamped Markdown trusted its frontmatter (B P0-1) | P0 | **Fixed:** 001 §5.1 provenance model (external ceiling, persisted index, origin defaults, execution attribution); frontmatter is a display copy |
| Step 1 DoD failed on this repo: frontmatter, names, missing skills, `file://` test text (RT F5, S P0-2, B P1) | P0 | **Fixed:** required/optional fields, a 15-file backfill, name leaks removed, catalog-backed skill rule, test wording. Verified with a repo-wide check: 0 frontmatter failures, 0 leaks, 0 broken links. |
| Offline/CI `init` and `doctor` couldn't exit 0 (S P0-1, B P1) | P0 | **Fixed:** `init --non-interactive` (no network); `doctor` ok/warn/fail/skipped; check 6 skips until `state.db` exists and checks vectors only for the hybrid engine; check 7 warns |
| Specs didn't say what Step 1 builds (S P1-3) | P1 | **Fixed:** phase-mapping tables in both specs |
| Zone mapping incomplete; `base/` vs binary; missing tree entries; `.py` names contradicted D-02 (S P1-4) | P1 | **Fixed** |
| No Step 1 tests; no source-repo lint mode (S P1-5) | P1 | **Fixed:** 006 tests 6–10; `delphi lint --repo` |
| Update swapped the binary before the constitution gate; single-workspace drain; downgrade (RT F6) | P1 | **Fixed:** verify → drain all → ratify → merge → swap; downgrade refused; rollback restores state (Step 5) |
| Theme XML breakout and `@name` collisions (RT F7) | P1 | **Fixed:** field sanitizer, unique role-disjoint names, `@<role>` precedence, plus escaping of all interpolated prompt text (002 §2, broadened by the Builder seat) |
| Workspace resolution; `ratified.lock` bootstrap; clean-repo rule would flag Delphi's own repo (B) | P1 | **Fixed** (006 §2.2, §3.1) |
| `doctor` check 5 vs unseeded and cross-zone links; check 3 had no tier to read for REVIEW and skills; the absolute-path regex matched its own rule (found in Round 2 by RT/S) | P0/P1 | **Fixed:** logical overlay, design-record links skipped, check 3 via `ratified.lock` and the provenance index, username-segment regex |
| Rule-grammar check, NFC normalization, scope list, federation keygen in Step 1 (S, B) | P2/P1 | **Fixed;** keygen deferred to Step 7 |

## Accepted Risks & Minority Report (Red-Team / Dissent)
Unanimous: no unresolved P0/P1 (red-team concurs after fixes). Accepted risks recorded by the seats:
- **Theme linter strength:** the deterministic keyword and pattern linter can be paraphrased around. It's accepted because the real controls are elsewhere: operator ratification of every pack, the persona slot ranking below governance, prompt escaping, and taint-gated tools.
- **macOS sandbox:** `sandbox-exec` is deprecated (carried over from Council 001). Without a sandbox, execution falls back to Amber, and harness mode becomes unavailable for code-writing tasks.
- **Out of scope here:** spec 009 (model access, D-19) was drafted during this council and has **not** been reviewed. Its harness mode in particular delegates controls to third-party CLIs. → **Council 003.**
