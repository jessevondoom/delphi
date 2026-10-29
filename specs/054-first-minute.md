---
id: spec-054-first-minute
title: "054: The first minute: init continues into setup, and the first answer is research, not a tour"
author: architect
timestamp: 2026-10-28T23:00:00Z
status: approved   # by the operator, 2026-10-28 ("merge and approve all, follow the rest of your recommendations"); FR1 is small and buildable; FR2 waits for a human timing
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # from a timed walk-through of init, doctor and setup in a scratch home; the console's first answer is untimed
source_mission: 2026-10-23-foundational-design
tags: [spec, onboarding, delight, console]
---

# Spec 054: The first minute

## 1. Why
A walk-through from a scratch `--home` (plan doc §5, 2026-10-28): `delphi init` takes 0.12 s and asks two questions; `delphi doctor` takes 0.5 s; `delphi setup --yes` already finds `claude`, `gemini`, `agy`, `ollama` and `codex`. The front door is already quick. What is left is a **seam**: `init` ends with "next: run `delphi setup` to connect a model, then `delphi`", so a new operator is handed three commands where one flow would do. The thesis ([`docs/thesis.md`](../docs/thesis.md)) wants the first interaction to leave something behind; the first minute should end with an answer and a first noted item in the library, not with homework.

### 1.1 Non-Goals
- No new authority, no network call during `init`, nothing written into projects.
- No change to `--non-interactive` (scripts see the exact output they see now) or to `delphi setup` itself.
- No tour, no animation (spec 053 and `stillMotion` apply to anything later).

### 1.2 Simpler Alternative Considered
Leave the printed hint. Rejected: it is the one place a person must leave the flow and remember a second command.

## 2. Design
**FR1, init continues into setup (interactive only).** After a successful interactive `init`, ask once, default yes: `Connect a model now? [Y/n]`. Yes runs the same code as `delphi setup` for that home; no prints the existing hint. `--non-interactive` and any `--yes`-less pipe (stdin not a terminal) skip the question and print the hint unchanged. A failure in setup is reported as `setup` already reports it and does not undo the workspace.

**FR2, the first answer (waits for measurement).** When setup finds a model, the console's first prompt suggests one research question about the operator's own project and, once answered, the capture of spec 052 notes it (status "noted") so the library counter reads "+1 noted". Not built until a person has timed the console's first minute and decided what the suggested question is; builds with 052 KS2.

## 3. Slices
| Slice | Content | Depends on |
| :-- | :-- | :-- |
| FR1 | interactive `init` offers setup; tests for the three paths (yes, no, non-interactive) | none |
| FR2 | first-question suggestion and first noted item | 052 KS2; a human timing |

## 4. Verification
FR1: unit tests for the decision function (interactive yes, interactive no, non-interactive, non-terminal), plus the existing init tests unchanged. A scratch-home run proves the non-interactive output is byte-identical to before.

## 5. Open decisions
- OD-1: default of the question is yes (provisional answer; standing under the operator's blanket approval).
- OD-2: FR2's suggested question (needs the operator).
