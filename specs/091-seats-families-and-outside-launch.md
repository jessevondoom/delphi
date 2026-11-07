---
id: spec-091-seats-families-and-outside-launch
title: "091: Seats, model families and launching agents on any carrier: receipts, a family table, an opt-in diversity requirement, and an ask-another-family tool"
author: architect
timestamp: 2026-11-07T00:00:00Z
status: draft   # a proposal, not permission to build (No Spec, No Build). The operator decided two points on 2026-11-07 (§9); everything else is the architect's recommendation after Council t_0002.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the design follows one Council and one set of live runs on a single machine; the carriers' metadata (§2) was read from three CLIs, not tested for spoofing
source_mission: 2026-10-23-foundational-design
tags: [spec, seats, families, harness, panel, red-team, arms, copilot, agy, codex, draft]
---

# Spec 091: Seats, model families and launching agents on any carrier

> **Status: draft.** Extends [`specs/086`](086-outside-council-panel-and-host-subagents.md) (the Panel and `foreign_seat`), [`specs/062`](062-delphi-as-host-agent-mcp-server.md) (Delphi for host agents), [`specs/051`](051-copilot-cli-harness.md) and [`specs/047`](047-codex-harness-and-coverage.md) (Copilot and Codex as harnesses), [`specs/056`](056-discovered-model-assignment.md) (model classes and quota), [`specs/037`](037-training-experiments-quality-and-efficiency.md) and [`specs/048`](048-harness-benchmarking-and-targeted-evals.md) (arms). Uses `GOVERNANCE.md` H-001 (a family is a training lineage, whoever serves it) and `context/heuristics.md` H-007, H-008. **Widens no authority.** The separate question of letting a Red action run without a live operator is `specs/092`.

## 1. Why

The operator's goal (2026-11-07): Delphi agents running on the outside, in any host CLI or over any API connection, using several model families, so that arms testing, Panels and Councils, and red-team sweeps all get the quality that family diversity buys. The Council that wrote `specs/003` found that the author's family caught 0 of 9 P0 defects (H-008); a Council on this design (`t_0002`, 2026-11-07) ruled for the smallest first step.

**What exists, so this is not rebuilt.** An endpoint is a carrier: `claude`, `agy`, `codex` and `copilot` are harnesses with launch rules, an environment allowlist, a scratch home, L0 taint and, for Copilot, a refusal when the served model differs from the one asked for (051 §7). The Panel's `foreign_seat` already has Delphi run one seat on another family (086). `request_help` lets the orchestrator pick who answers (062). Councils spread seats across families when several are configured and label a one-family verdict `homogeneous` (H-001, H-008, `council.rs`). `056` assigns models by class and quota. `037` and `048` run arms in throwaway homes.

**What is missing, from live evidence on 2026-11-07:**

1. **Nothing records which model really answered, on which carrier.** An arm is a model *and* a carrier. The same orchestrator task was allowed to dispatch an investigator on a `claude` seat and refused on every `agy` seat in a headless run; that is a carrier policy, not a model trait, and a cross-family comparison confounds the two.
2. **One endpoint, one family does not fit an aggregator.** Copilot serves Anthropic, OpenAI and Google models behind one login (23 models from five vendors, 051 §6.4); `agy` lists Gemini, Claude and `gpt-oss`.
3. **Prompt completeness swamps a family effect.** A refusal that did not name the project cost Haiku 0 of 4 runs and 3 of 3 with the name (`specs/020` §4). A family comparison with different prompts is noise.
4. **There is no way for a host to ask another family one question** without opening a whole Panel.

## 2. Principles

1. **The family is a property of the served model**, not of the carrier or the endpoint (H-001). The same model through two clouds is one family.
2. **The daemon writes the receipt.** A model's own claim about what it is is never used. What a carrier reports about the served model is recorded as *carrier-claimed*, not verified: a router or alias may serve something else, and metadata can be missing.
3. **Label, do not block** (H-008), unless the caller asked otherwise (§3 SR-3).
4. **Foreign output is advice.** It is observed at L0 (it carries the taint of its lowest-trust input), gets no mutating or outbound tool, cannot dispatch, cannot promote itself into memory (H-004), and cannot widen the caller's grants.
5. **Tokens and counts, never dollars.**
6. **Nothing here launches a process that a harness does not already launch.**
7. **A seat that needs a Red action in a headless run is not enabled here.** That depends on `specs/092`; until it is approved, such a seat runs live or with Amber actions only, and a refused Red card is the recorded outcome (as for the `agy` dispatch refusal of 2026-11-07).

## 3. The additions

| # | Addition | What it is |
| :-- | :-- | :-- |
| SR-1 | **Seat receipt** | An event `seat.receipt` the daemon writes when a seat ends: `seat_id`; `requested` {endpoint, model}; `served` {model as the carrier reported it, marked `carrier_claimed`; family; `family_source`: `table` or `unknown`}; `carrier` {harness kind, CLI version, tools offered, whether dispatch was allowed}; `tool_grant`; `prompt_hash` (blake3 of the prompt text as sent); tokens in and out; outcome. No text, no arguments. |
| SR-2 | **Family table** | A ratified file `model-families.toml`: model-id patterns to a family name, with an `owner` and an `expires` date. A served model the table does not map has family `unknown`. An expired entry warns in `delphi doctor` and still applies. |
| SR-3 | **`require_families = N`** | Opt-in, set by the caller (a Panel, a Council, an arm, a release-gate sweep). The receipt, not the request, decides whether it was met: `unknown` never counts, and a fallback to another family never counts. If unmet, the seat is refused with a fixed sentence that says what was missing and how to retry. **Unset (the default) is H-008:** the work proceeds and the result is labelled `homogeneous`. *Operator decision, 2026-11-07: a release-gate sweep also labels and does not fail closed.* |
| SR-4 | **Count budget** | Per task and per mission: foreign seats, input tokens and output tokens. Over the budget, a foreign seat is refused with a fixed sentence. A reserve for the operator's own subscription window is deferred (§10). |
| SR-5 | **`ask_family`** (host tool, later slice) | `ask_family {question, family?, model?, context_pointers?}` on the host socket: one read-only foreign seat answers one question; advice only; it returns the answer *with its receipt*. It cannot dispatch, has no tools beyond reading the pointers it was given, counts against SR-4, and is refused when the daemon is not running (it never starts one). It is a constrained `request_help`, not a new launch path. |
| SR-6 | **Arms record receipts** | `delphi train run` stores the receipt of each seat in its row. The report shows the served model and carrier per arm, and labels a family comparison **confounded** when its arms differ in prompt hash or in a carrier policy that changed what the seat could do (such as a refused dispatch). |

## 4. Where each lives

| Addition | Code and files |
| :-- | :-- |
| SR-1, SR-2 | a new `crates/delphi-engine/src/seat.rs` (the receipt type, the table, the resolver); the event in `event.rs` and its schema under `schemas/`; written at the end of a span in `harness_run.rs` and the API path in `engine.rs` |
| SR-3, SR-4 | `council_panel.rs` and `host_rpc.rs` (the Panel's `foreign_seat`), the engine's seat routing |
| SR-5 | `mcp_host.rs`, `host_rpc.rs`; the tool list golden test; `docs/reference` |
| SR-6 | `train_run.rs`, `train_report.rs` |

## 5. Invariants and tests

- **Daemon-written:** a seat that claims to be another model in its text leaves the receipt unchanged; a receipt exists for every foreign seat and no event of that kind comes from a host.
- **Unknown is not diversity:** a served model not in the table, or not reported, resolves to `unknown`; `require_families = 2` over one real family and one `unknown` is unmet.
- **Default degrades:** with `require_families` unset and one family, the Panel proceeds and the verdict is `homogeneous`.
- **Copilot:** a request for model A that the carrier reports as model B is an error result (051 §7) *and* writes a receipt showing both.
- **No authority:** a foreign seat is offered no mutating or outbound tool; a dispatch from one is refused; its output is L0 and cannot be written to memory or a rule.
- **Budget:** the first foreign seat past the budget is refused; the sentence names the budget and does not echo the question.
- **Arms:** two arms with different prompt hashes are reported as confounded.

## 6. Acceptance criteria

- **AC-1** A Panel run on two real families writes two receipts, each naming the served model and its family from the table.
- **AC-2** The same run with one family available labels the verdict `homogeneous` and completes.
- **AC-3** The same run with `require_families = 2` and one family available is refused, cheaply, before any seat runs.
- **AC-4** A `train run` of the same case on two carriers reports both receipts and, if the carriers' policies differed, says so.

## 7. Slices (one per pull request)

| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| S0 | this spec: approval | |
| S1 | SR-1 and SR-2: `seat.rs`, the event, the schema, the table and `delphi doctor` expiry warning | S0 |
| S2 | SR-3 and SR-4 on the Panel's `foreign_seat` | S1 |
| S3 | SR-6: receipts in arm rows; the confounded label | S1 |
| S4 | SR-5: `ask_family` | S2 |
| S5 | the served model read from Copilot and `agy` metadata, so SR-1 is true for them (051, 020) | S1 |

**Promotion trigger.** Slices S1 to S3 keep the receipt and the table small and standalone. When the arms runner (S3) and the Panel (S2) both read receipts, the shared pieces are promoted into one seat layer (the Council's option B); until then no other consumer gets one.

## 8. Cut for now

A reserve for the operator's subscription window; a generalised seat layer; any family quality score (`078` rejects seat accuracy scoring; a family is chosen from seeded-defect recall and unique-catch counts, never ranked by an accuracy number); automatic choice of the best family; tools for Copilot seats until receipts exist.

## 9. Decisions

**Decided by the operator, 2026-11-07:** (1) a release-gate sweep labels `homogeneous` and does not fail closed; (2) `require_families` is opt-in. **Open (the architect's recommendation in brackets):**

- **OD-1** Who owns `model-families.toml` and how long an entry lives. [The operator; 90 days.]
- **OD-2** The default count budget. [Three foreign seats and 200K input tokens per task, 20 foreign seats per mission.]
- **OD-3** Whether `ask_family` may name a model or only a family. [Family only at first.]
- **OD-4** Whether a red-team sweep should prefer two families by default. [Yes, labelled when it could not (Council t_0002: the strategist and architect); the red-team seat wanted it required whenever configured.]

## 10. Known limits

A carrier's served-model field is a claim. A router or alias can serve something else; the receipt says what was reported. The table drifts as vendors ship models. A tool-less seat cannot check a claim against the repo, so its findings are labelled `unverified-against-repo` and count toward a unique-catch figure only once verified. Subscription windows are shared with the operator, and a foreign seat spends them; the count budget is a stop, not a guarantee.
