---
id: knowledge-and-delight-plan-2026-10-28
title: "Plan and coordination log: grow the knowledge base without weakening it, and make Delphi a delight"
author: strategist
timestamp: 2026-10-28T21:30:00Z
source_type: internal_generated
confidence: low   # a plan from a code and spec read; nothing here is measured yet
status: active
source_mission: 2026-10-23-foundational-design
tags: [plan, coordination, knowledge, validator, ux, delight, thesis]
---

# Plan and coordination log (2026-10-28)

**Assignment (operator, 2026-10-28):** Delphi is a research and strategy harness for a team (`docs/thesis.md`, open PR). Make it the most capable such harness: knowledge grows from every interaction without extra friction, correctness and validated knowledge are protected, and the console is a delight. This file is the shared plan and the **hand-off log** for every agent working on it (Claude, Copilot, others): read it first, append to §5 last.

## 1. Findings that drive the plan
1. Protection is strong (taint lattice, ratification, Xenia); growth is gated and mostly manual. The operator's ordinary work adds almost nothing to the library.
2. "Verified" means three things: web research indexes at L2 with no validation (`research.rs`), validated facts are L3 (040), bundle knowledge is L2 (024). The operator-facing labels of 040 §18.2 are decided but unbuilt.
3. The validator (040) is the bridge between growth and protection and is unbuilt; the specs that govern it are drafts.
4. The first-run path (install attestation, credentials, seats, tiers, approval modes) comes before any knowledge benefit; the README still leads with building software.
5. Colour and layout are capable (`ui/src/theme.ts`) but have no shared meaning; about 130 call sites choose colour directly.
6. Spec sprawl: two 014s and two 037s, overlapping 005x specs, and a stale tracker (bundles are built but listed as not).

## 2. Work in flight (checked 2026-10-28; keep this table current)
| Work | State | How this plan relates |
| :-- | :-- | :-- |
| #305 thesis doc | open | the product statement; 052 and 053 cite it. Merge first. |
| #300 specs 048-050 (050 and 049 approved) | open | 050 ML-1..ML-4 are the entry fields 052 builds on. 052 does not duplicate them; KS2 depends on 050 ML1/ML2. |
| #294 ratify bundle specs 022-027 | open | no overlap; bundle claims reuse the 052 ladder. |
| #307, #309 copilot harness (051); #304, #306 codex (047) | open | harness work. Untouched; the harness layer is not simplified by this plan until first-run evidence exists (§4). |
| #302 spec 041 S0 | open | no overlap. |
| decisions pane D1-D7 (044) | merged | KS3 adds a `learn` kind to its read model (`decision_model.rs`). |
| spec 040 validator | draft, merged 2026-10-28 | not built; awaits the Council pass of §19.6 and approval. |

## 3. Order of work
Each item is one pull request. A draft spec is a proposal, not permission: build slices wait for the operator's approval (AGENTS.md, No Spec, No Build).

| # | PR | Contents | Gate |
| :-- | :-- | :-- | :-- |
| 0 | this one | spec 052 (knowledge status, capture on contact, digest) and this plan | operator approval of 052 |
| 1 | next | spec 053 (visual language) | operator approval of 053 |
| 2 | tracker and spec hygiene | refresh the tracker (bundles built), renumber the duplicate 014 and 037, fold superseded 005x specs into pointers | none beyond review; no behaviour |
| 3 | spec 054, first minute | zero-config start: detect an existing `claude`, `gemini` or `ollama` login and begin; research-first front door; progressive disclosure of seats, sandbox and budgets. Draft after the first-run walk-through in §6 | operator approval |
| 4 | VL1, KS1 | semantic styles and the status table, display only (no behaviour change) | after 053 and 052 approval |
| 5 | KS2, KS3, VL2-VL4 | capture on contact, the digest, the gutter, the composer counter | after 050 ML1 lands |
| 6 | validator V1 | the `fact-validation` skill and labels, per 040 §11 | after 040 approval |

## 4. Deliberately not done yet
- **Simplifying the harness layer** (providers, failover, sandbox). It is large, but cutting it needs evidence from a first-minute walk-through (§6), not an opinion.
- **Changing tiers.** The lattice is a security mechanism. 052 changes what the operator reads, not what the daemon enforces.
- **Self-approval.** No spec in this plan is approved by the agent that wrote it.

## 5. Hand-off log (append only; newest last)
Format: `date · agent · what changed · what the next agent must know`.

- 2026-10-28 · copilot-cli · opened this plan; wrote specs 052 and 053 as drafts on `worktree-spec-052-knowledge-status` and `worktree-spec-053-visual-language`; tracker sections added. Spec links to unmerged specs (050, 052, the thesis) are plain text so `delphi lint` passes; convert them to links once those PRs merge. Do not start KS or VL slices before the operator approves the specs.
- 2026-10-28 · copilot-cli · operator approved 052 and 053 and asked to merge everything. Merged #301, #305, #300, #294, #302, #307, #304, #306, #309 (retargeted #309 from `worktree-codex-s1` to main and merged main into it; `cargo check --workspace --all-targets --locked` clean). 052 and 053 are approved; links to 050 and the thesis are real links now. Next: validator spec 040 approval and improvement, then KS1/VL1 build slices, tracker hygiene.
- 2026-10-28 · copilot-cli · first-minute walk-through, part 1 (scratch `--home`, no console): `delphi init` took 0.12 s and asked two questions (workspace name, theme); `delphi doctor` ran in 0.5 s with two warnings, no signed release and no model configured. `delphi setup --yes` already finds `claude`, `gemini`, `agy`, `ollama` and `codex` on this machine, and bare `delphi` is documented to set up whatever is missing. **Correction to §1 item 4:** the pre-console path is shorter than the specs suggest; the unmeasured part is the console's first screen and the first answer. Spec 054 must wait for a person to time that part (install to first sourced answer, prompts and concepts met) and must not remove setup steps on the strength of the spec read alone. Also note: `delphi init` prints "next: run `delphi setup`" instead of continuing into it; that one-line hand-off is the first candidate for 054.

## 6. How agents coordinate
1. **Claim before you start:** add a line to §5 naming the slice, then branch from `origin/main` in a worktree (`.claude/worktrees/<slice>`), one slice per PR.
2. **Gates before every commit** (AGENTS.md): `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`. For UI slices also the `ui/` tests and golden screens.
3. **Update the tracker** in the same PR that changes a spec's state; commit trailer `Implements-Spec: specs/NNN-*.md`.
4. **Never edit generated files** (`docs/reference/*`); regenerate them.
5. **Disagree in writing:** if code and a spec diverge, stop and amend the spec (GOVERNANCE §1.4); note it in §5.
6. **First-minute walk-through (for item 3):** from a scratch `--home <tmpdir>`, time install to first sourced answer, count prompts and concepts met, record where a new operator stalls. Put the numbers in §5; they decide what spec 054 removes.
