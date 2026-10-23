---
id: review-step-1-red-team-r4
title: "Step 1 Merge-Gate-4 Audit, Round 4 (single-family fallback, verbatim)"
author: red-team
timestamp: 2026-10-24T03:30:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [review, step-1, merge-gate-4, verbatim]
model: "Claude Sonnet 5.5 (subagent; the code's author seat was Claude Opus 5.5)"
audited_commit: 4539fb1
---

> Gemini quota was exhausted (RESOURCE_EXHAUSTED, resets in about 6.6 days), so per `[H-008]` in a single-family setup this round used a different model of the Claude family as the critic.

### Red-Team Implementation Audit — Merge Gate 4, Round 4 (single-family fallback)

Read the current `crates/delphi/src/*.rs`, ran `cargo test` (48 passed), and probed a throwaway copy of the crate with direct `Guard::check` calls, `delphi doctor` on a fresh home, and `delphi lint --repo` on synthetic repos.

## 1. Round-3 findings
- **R3-1 (frontmatter governance scope): RESOLVED.** `GOVERNED` no longer includes `themes/` or the broad `operator/` prefix, and covers `heuristics.md`. An invalid `local/heuristics.md` fails; a valid operator skill and override raise no frontmatter failures.
- **R3-2 (`skills/custom/**/scripts/**` zero-segment bypass): RESOLVED.** `is_custom_skill_script` requires at least 5 segments, a `scripts` segment strictly between the skill and the filename, and a name other than `skill.md`. With Builder's real glob, `skills/custom/scripts/SKILL.md`, a file named `scripts`, `…/scripts/SKILL.md` and `…/SCRIPTS/SKILL.MD` are denied; `skills/custom/s/scripts/run.sh` is allowed. `write_glob_error` rejects `skills/custom/scripts/**`, `…/scripts/SKILL.md` and `…/x/scripts`.
- **NEW-3 (pathguard zone mapping): RESOLVED.** Protected kinds are served from `zones.ratified` on read; `spec.md` maps to base, `memory/agents/` to local, `skills/custom/` and `agents/custom/` to team. Remaining mismatch: N-6.
- **NEW-4 (wildcard glob handling): RESOLVED** for the cases round 3 listed (`st?te.db`, `missions/**/events.json?`, `missions/**/.barr*` rejected; non-string list elements fail closed). Residual gap: N-5.
- **R3-3 (overlay shadowing, symlinks, deleted live files): RESOLVED.** `team/agents/scout/AGENT.md` gives `[fail] 3 zones`; a symlink in `team/context/` fails; deleting `team/REVIEW.md` warns that the live file is missing. Case-sensitivity gaps: N-2.

## 2. New defects (no P0 or P1; all P2, verified unless marked)
- **N-1 [P2]** Repo-mode lint skips `target`, `node_modules`, `.git` and `.delphi` at any depth: `context/target/a.md` is never linted.
- **N-2 [P2]** `is_protected` and the doctor zone check are case-sensitive while the guard and macOS volumes are not: a Builder-written `skills/custom/s/SCRIPTS/evil.sh` is not flagged as unratified, and `team/Agents/scout/AGENT.md` produces no `3 zones` failure.
- **N-3 [P2]** Doctor and lint swallow walk errors (`.into_iter().flatten()`): a `chmod 000` directory drops its files from every check and `doctor` exits 0. A dangling symlink at a zone root is dropped by `exists()` (uncertain; not probed).
- **N-4 [P2]** A faithful copy of a built-in manifest in `operator/core/agents/` fails `doctor` on its relative `../../GOVERNANCE.md` link, because links resolve from the operator path rather than the logical counterpart.
- **N-5 [P2]** Read-glob lint misses prefix-style denials: `l*/**`, `o*/**`, `prop*/x`, `memory/op*/x`, `r?n/x` and `**/local/**` pass. The runtime guard still denies them, so this is defense in depth.
- **N-6 [P2]** Guard reads ignore `core/` customization (`Zones` has no `core` root), and `check_overrides` covers `core/agents` only, not `core/skills`. No impact today because no built-in skills ship.

Checked with no defect: NFC normalization, case folding and special case-mapping characters (Kelvin sign, dotted İ, long-s); `.GIT`, `.BARRIER`, `Events.JSONL`, `State.db`, `Local/`, `Config.toml` and `./` or `//` segments; protected kinds served from `ratified/` on read only; unreadable or malformed locks, planted files in `ratified/` and missing required lock entries all fail; only `fail` gives a non-zero exit code.

### Verdict
approve-with-fixes

---

## Disposition (builder)
N-1 to N-5 fixed in the commit that follows this record, each with a test. N-6 is deferred to Step 2, when the daemon gains a `core/` root and built-in skills ship; it is tracked in `context/invariant-enforcement.md` terms as a known gap and has no effect while `core/skills` is empty.
