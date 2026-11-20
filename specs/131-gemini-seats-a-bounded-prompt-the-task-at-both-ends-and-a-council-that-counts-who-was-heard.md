---
id: spec-131-gemini-seats-a-bounded-prompt-the-task-at-both-ends-and-a-council-that-counts-who-was-heard
title: "131: Gemini seats: a bounded prompt, the task at both ends, and a Council that counts who was heard"
author: architect
timestamp: 2026-11-20T00:00:00Z
status: draft   # a proposal written 2026-11-20 for the operator to read; it is not permission to build. Nothing here is approved.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the code anchors were read on 2026-11-20 at main 56fe81ac; the cut size comes from ONE operator test (three one-seat calls) and the limit itself is unmeasured (section 8). No model was run and no Council was convened to write this.
source_mission: 2026-11-20-gemini-council-seats
tags: [spec, council, agy, gemini, prompt-budget, truncation, require-families, tools, codex, copilot]
---

# Spec 131: Gemini seats: a bounded prompt, the task at both ends, and a Council that counts who was heard

> **Status: draft, 2026-11-20.** A proposal. A draft spec is not permission to build; no code, schema or config changes with it.
> **Extends, if approved:** `specs/128` (`128-round-2-without-tools-one-retry-for-an-empty-finish-and-a-guard-that-knows-delphis-own-writes.md`, in draft pull request #792 when this was written and not yet on `main`, so not linked): it takes 128's one retry and its `harness.no_answer` event as given and adds to them. One sentence of 128 is amended (section 3.5, OD-3): its non-goal "a Round 1 seat keeps its tools". It also builds on [`specs/123`](123-the-council-and-panel-from-a-host-who-sat-what-it-cost-a-tail-and-the-loop.md) (receipts say who sat; `require_families`), [`specs/099`](099-round-2-readability-keep-a-valid-critique-ask-for-the-object-only-and-say-why.md) (what a readable Round 2 is) and [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) section 6.2 (`ask_family`).
> **Number:** 131. 129 is the teaching-pipeline pull request (#793); 130 is on branch `worktree-app-onboarding-127` (the desktop app). Verify the number again before the pull request merges.

---

## 1. Scope and objectives

Over three Councils (`t_0002`, `t_0003`, `t_0004`, 2026-11-19 and 2026-11-20) every seat on the Gemini family failed, and each Council still printed "two families met". This spec says how Delphi stops a Gemini-carried seat from being sent more than its carrier delivers, keeps the task readable when something is cut anyway, counts a family only when a seat was heard, and keeps a seat from acting on the project.

**What the operator saw (as reported to the architect; read as data, not re-checked here).** The install has two endpoints: `claude-sub` (family `claude`, harness `claude`) and `agy-sub` (family `gemini`, harness `agy`, model `gemini-3.8-flash-medium`), `role_family = { red-team = "gemini" }`. `t_0002` red-team was discarded ("the agy run changed a project checkout"); in `t_0003` and `t_0004` the red-team ended "without the schema-validated answer that was asked for" and the strategist gave an empty answer or abstained. The red-team's last words said its prompt was truncated by 76,525 bytes and every tool call was denied with "the gate could not be reached". The Councils were given context pointers of about 255 KB (seven specs and GOVERNANCE.md).

**The controlled test (operator's consent, 2026-11-20, three one-seat `ask_family` calls to Gemini Flash).** A bare question answered. One 18 KB file answered correctly. Seven large files (about 255 KB) were answered with a complaint that the task, objective and output contract were missing: the carrier had cut **69,110 bytes from the tail**, mid-sentence in the fifth file. About 186 KB of file text arrived. **That figure comes from one test on one prompt.** The limit is not measured, it may count bytes or tokens, and it may differ by agy version or model (section 8).

### 1.1 Observations tied to code

All anchors are `file:line` in the tree at `56fe81ac`. "None found" means searched for and absent.

| # | Observation | Anchor |
| :-- | :-- | :-- |
| O1 | **No pre-start size check exists for any carrier.** No limit, count or warning on prompt bytes in `harness.rs`, `harness_run.rs`, `harness_opts.rs`, `seat.rs` or `council.rs` (searched for `truncat`, `max_input`, `MAX_PROMPT`, `prompt_bytes`: none found). The receipt keeps only a hash of the prompt. | `seat.rs:195-208`, `harness_run.rs:1093` |
| O2 | **The numbers in `carrier_plan.rs` are not limits.** `estimate(kind)` returns (low, high) *typical input tokens per seat*, "the measured medians", source "measured 2026-11-07, small samples": `agy` (26,000, 283,000), `claude-cli` (31,000, 48,000), `copilot-acp` (10,000, 17,000), anything else (12,000, 24,000). They are summed into the Panel plan's `plan_budget` for the operator to read. The `agy` high figure is a spend seen when a seat loops over tools. Nothing enforces or cross-checks them. | `carrier_plan.rs:58-66`, `136-138`, `22` |
| O3 | **Pointers are inlined, each capped by lines, with no total.** A Council question lists at most ten pointers (`- path: reason`); the engine turns each into a context slice through `tools::read_pointer`, which shows at most 400 lines and reads at most 256 KB of the file. Ten pointers of 400 long lines can run to hundreds of KB. | `handoff.rs:209-238`, `host_rpc.rs:449-460`, `engine.rs:1053-1078`, `tools.rs:15`, `1458-1491` |
| O4 | **Round 2 sends the pointers again, plus the stances.** The Round 2 handoff, the repair handoff and the synthesis handoff each get `pointers.clone()`; Round 2 adds every seat's Round 1 text (6,000 characters each) and earlier rulings. A Round 2 prompt is therefore at least as big as Round 1's. | `council.rs:1680-1685`, `1716`, `1749-1754` |
| O5 | **The task is last.** The system sections (governance, role, skills, persona, guidance, heuristics, lessons) come first; the user turn is the context slices and then the contract, whose first line is `Objective:`. For `agy` all of it goes in ONE JSON user event on stdin: system text, a "do not call tools" sentence if there is no bridge, then the user text. Codex and Copilot get the same order as plain text. This is heuristic `[H-006]` as designed, and it is exactly what a tail cut destroys. There is no copy of the task at the top. | `ipr.rs:75-92`, `95-114`, `handoff.rs:134-135`, `engine.rs:1185`, `harness_run.rs:773`, `harness.rs:916-920`, `909`, `912-914` |
| O6 | **A cut prompt is invisible to Delphi, and a complaint about it counts as an answer.** A seat that returns any non-empty text is `Stance::Said`. A seat that returns words but no schema object is not `AnswerNotFound` (that needs no words), so the success path hands the complaint on as the answer. Only a seat with a schema asked for and no words at all is failed with the one line "the CLI finished without the schema-validated answer that was asked for". | `council.rs:1340-1345`, `harness_run.rs:130-141`, `986-1006` (the line is at 1003) |
| O7 | **Round 1 counts a seat by its text, not by whether it answered.** `answered` is every seat whose stance has text; two are enough to deliberate. `refusal::declines` and `round1_parsed` are computed but are "diagnostics, not a score": a seat that declines, or says it cannot see the task, still counts. | `council.rs:1664-1674`, `1669`, `refusal.rs:16-25`, `27` |
| O8 | **`require_families` is judged on who was started, not who was heard.** At convening it is judged on the plan's families. In the ruling it is judged on the served families of receipts whose outcome is `ok`, once per seat; a receipt is `ok` when the CLI exited without a failure, whatever the seat said. | `council.rs:1619-1625`, `host_rpc.rs:880-930`, `seat.rs:342-358`, `harness_run.rs:1097` |
| O9 | **A Council seat is not toolless in Round 1, and Round 1 has write tools.** `span_tool_defs` clears the list only when the span is `toolless`; the daemon Council does not set it (spec 128 S1 would, for Round 2 only). The Round 1 constraint "you must not write files" is prose. The red-team manifest has a write scope, so the offer includes the `write_*` tools and `request_research`, `record_finding`, `raise_decision`, `propose_admin_action` and `request_test_authorization`. `request_research` queues a job whose writer puts files in the project checkout. | `engine.rs:2081-2146` (clear at 2142-2144), `tools.rs:273-276`, `1811-1812`, `council.rs:1301`, `agents/red-team/AGENT.md` |
| O10 | **`t_0002` was Delphi's own write, not the seat's process.** Spec 128 section 1 reads the discarded run as a `request_research` job writing `research/topics/...` into the checkout. The guard compares `git status` of the registered projects, so a scratch working directory for the seat would not have prevented it. | spec 128 section 1, `harness_run.rs:1129-1148` |
| O11 | **"The gate could not be reached" is a hook-side denial that Delphi never sees.** `delphi __agy-hook` prints it when it cannot connect to, or write to, the span's socket. The daemon logs `harness.gate` decisions it makes, but a hook that never connected leaves no daemon event. Truncation does not by itself explain this text. | `bridge.rs:528-559` (542, 549) |
| O12 | **`ask_family` has the same blind spot.** The seat is toolless, but the pointers are inlined with no total; its budget check counts seats and an output-based token ceiling (`2 * max_tokens + 24,000`), never input; any non-empty text is `answered`. | `service.rs:1457-1533` (1479, 1499, 1505-1510), `seat.rs:461-469`, `council.rs:1432-1434` |
| O13 | **Codex and Copilot have no input limit in Delphi either.** Codex gets the prompt on stdin (`exec`) or as one text input (app-server); Copilot as one ACP `session/prompt` text block. No length is checked. Whether their CLIs or providers cut a long prompt silently is unknown. | `harness.rs:909`, `497`, `639-642`, `1231` |

### 1.2 Objectives

1. A seat on a carrier with a known or assumed input limit is never sent more than a budget, counted on the whole prompt before the CLI starts, and the receipt and the ruling say what was cut.
2. If a cut happens anyway, the task and the output contract are still in what arrives.
3. A seat that says the task was cut off, or that declines, is not counted as having sat. `require_families` is judged on seats heard.
4. A Council seat cannot act: it is offered reading tools only, and cannot queue a research job or write into the project.
5. The failure says why in words, and the budget is learned or set, not guessed.

### 1.3 Non-goals

- No change to which seats sit, to the critique schema or to the Round 2 reader of spec 099.
- No change to spec 128's three slices; this spec adds to them and says where they meet (section 6).
- No automatic summarising of files by a model (option C in section 10).
- No change to `config.toml`. The install's endpoints are ratified; adding Codex or Copilot endpoints is a Red-tier proposal for the operator (section 7).
- No claim about any provider's real limit. Section 8 says what is not known.

### 1.4 Simpler alternative considered

Do nothing in the engine and tell hosts to pass less. That is the interim rule (section 4), and it is all this spec asks until S2 is built. It cannot be the whole answer: Delphi's own fixed text, Round 2's repeated pointers and the stances all add bytes the host never sees, a host cannot see the carrier, and a rule that rests on each caller remembering is how three Councils ran with 255 KB.

## 2. Interim rule (in force from the operator's approval of this spec, until S2 is built)

**A Council or `ask_family` call that includes a Gemini seat (or any seat on the `agy` carrier) is given under about 150 KB of context files in total.** To stay under it: pass at most three whole spec-sized files; for the rest pass an excerpt (`path#Lstart-Lend`, which `read_pointer` already honours, `tools.rs:1471`) or read a spec section with `delphi specs show <spec> --section <n>` and quote it in the question. Remember that Delphi adds its own governance, role and skills text and, in Round 2, the Round 1 stances and the pointers again, so 150 KB of files is not 150 KB of prompt. If the Council is also asked `require_families`, check the ruling's families against who produced a readable Round 1 and Round 2, not against `require_families_met`. The orchestrator and the architect follow this rule when they frame a Council; the operator can waive it by saying so.

## 3. The changes

### 3.1 S1: count the prompt (diagnostic only)

At the single point where a harness launch is assembled (`harness_run.rs:773`, where `system_prompt` and `user_prompt` are known and the endpoint is known), count `system_prompt.len()` and `user_prompt.len()` in bytes, and the bytes of the context slices and of the contract inside the user text. Record them: `prompt_bytes` (total), `fixed_bytes` (system plus the contract), `slice_bytes` and `slices` (count) on `harness.started`, and `prompt_bytes` on the seat receipt (`seat.rs:195-208`, `schemas/seat-receipt.schema.json`). Nothing is cut and nothing refused in this slice. It exists to answer, with real Councils, what fixed text costs (section 8, item 3) before any number is chosen.

### 3.2 S2: a per-carrier byte budget

1. **A budget per carrier kind**, `max_prompt_bytes`, default **150,000 for `agy`**; for `codex-cli` and `copilot-acp` the same default until measured (OD-2); none for `claude-cli`. It is a key of `[harness.<cli>]` (so an operator can narrow or widen it in their ratified config; the default lives in code, not in `config.toml`). Update `crates/delphi/config.example.toml`, `crates/delphi-adapter/src/config.rs`, `crates/delphi-engine/src/boot.rs`, `schemas/config.schema.json` and `docs/reference/config.md` (AGENTS.md table).
2. **Enforcement at assembly**, before the CLI process starts, in the same place as S1. If `prompt_bytes <= max_prompt_bytes` nothing changes. If not, each slice that came from a pointer (never the system sections, never the contract, never a peer stance) is cut to its fair share of what is left, at a line boundary, from the END of the slice, and ends with a fixed line: `[cut by Delphi: the carrier takes at most N bytes of prompt; lines 1 to K of M shown; ask for the rest with a pointer such as path#L<start>-L<end>]`. Fair share: `(budget - fixed_bytes - stances_bytes) / slices`, with a share a small slice does not use passed to the larger ones. Peer stances (Round 2) are already capped at 6,000 characters each (`council.rs:1680`) and are not cut here. If the fixed text and stances alone exceed the budget, the seat does not start: it abstains with the reason "the prompt cannot fit the carrier's budget (fixed text N bytes, budget B)", before any tokens are spent.
3. **It says so.** The receipt gains `context_cut`: `{budget, prompt_bytes_before, prompt_bytes_after, slices_cut: [{label, kept_lines, total_lines}]}` (labels are pointers, never contents; spec 039 section 3). `harness.started` carries the same. The ruling card names a seat whose context was cut. A cut is NOT a failure; the seat sat on less than it was given, and the card says what.
4. **`ask_family` and a Panel foreign seat use the same function**; `ask_family`'s answer carries `context_cut` so the host can tell the operator.
5. The function is pure (strings in, strings out) and deterministic, so it is tested without a model.

### 3.3 S3: the task at the top and at the bottom

For a carrier that has a `max_prompt_bytes` (so: `agy`, `codex-cli`, `copilot-acp` by default), the first lines of the prompt are a `task` section holding the objective and the output contract (the Round's shape, the same text the contract ends with) and one fixed sentence saying the task is repeated at the end. The contract at the end is unchanged. Claude seats are unchanged.

- The section is built by the same `section()` function as the others, so a caller's text cannot close it or forge another (`ipr.rs:70-72`, `escape_section_tags`). `task` is added to `SECTION_TAGS` (`ipr.rs:19`) and the test that pins that list (`ipr.rs:216`) is updated in the same commit; the test that says exactly one `<contract` appears (`ipr.rs:184`) still holds because the head copy is a different tag.
- The head copy is limited to the objective and its constraints, not the context. It is never larger than the contract.
- For `agy` the head goes at the start of the single user event (`harness.rs:916-920`); for Codex and Copilot at the start of their text (`harness.rs:909`, `912-914`).
- Whether a model attends better with it is not measured. What it buys is certain only in one case: a cut tail still leaves the task.

### 3.4 S4: count who was heard

1. **A cut complaint is not a stance.** A fixed matcher, beside `refusal::declines` (`refusal.rs:16-25`), reads the first 1,200 characters of an answer (and, like `declines`, skips an answer that is long or full of code) for the shapes seen: a `<truncated N bytes>` marker (the operator reported this form; the exact string a carrier prints is not verified here and the matcher must accept a few spellings), "was truncated", "cut off", "missing" next to "task", "objective" or "output contract". It is a diagnostic that names a cause; the rule below does not depend on it alone.
2. **Heard** means: Round 1, non-empty text, not a decline, not a cut complaint; Round 2, a readable critique (`critique_read(..).critique.is_some()`, already the `readable` field of `council.critique`, `council.rs:1741`). `council.stance` and `council.critique` gain `heard: bool` and, when false, `why: "declined" | "cut_complaint" | "unreadable" | "empty" | "abstained"`.
3. **The Round 1 threshold** (`council.rs:1671`) counts heard seats. Fewer than two heard: the Council escalates with a sentence that names the seats and the cause ("the red-team's Round 1 says its prompt was cut").
4. **`require_families` and `homogeneous`** in the ruling are computed from the served families of seats heard in Round 1 AND Round 2 (a seat the barrier released only in Round 1 counts as heard in Round 1 and the ruling says "round 1 only"). A seat started but not heard is listed under `seats_run_by_delphi` with `heard: false` and its `why`, as `sat` is today (`seat.rs:394-409`), and does not count toward `families` or `require_families_met`. `served_for_task` (`seat.rs:342-358`) keeps counting tokens for every receipt.
5. **`ask_family`**: a cut complaint or a decline is `unanswered` with the reason, not `answered` (`service.rs:1506-1529`).
6. **The failure message says why.** The line at `harness_run.rs:1003` becomes: "the CLI finished without the answer that was asked for (prompt N bytes; budget B; K tool calls; M words)", using only counts. Spec 128's `harness.no_answer` event gets `prompt_bytes`, `budget` and `gate_decisions` (how many `harness.gate` decisions the daemon logged) beside its `tool_calls`: if the CLI reports calls the daemon never decided, the hook did not reach it (O11), which is the only way to tell that case from a cut tail without reading stderr.
7. **Retry.** Spec 128 S2 allows one retry for an empty finish. This spec adds one condition to that single retry: if the first run's answer was a cut complaint, the retry runs with HALF the carrier's budget. It is the same retry, not a second one. A second failure abstains.

### 3.5 S5: a Council seat is offered reading tools only

For a seat in a Council or a Panel (the spans with injected slices, `engine.rs:1085-1092`), `span_tool_defs` keeps only these tools: `read_file`, `read_doc`, `list_dir`, `search`, `kb_search`, `recall_memory`, `xenia_search`, `check`. It drops every `write_*` tool, `request_research`, `record_finding`, `raise_decision`, `record_change`, `note`, `suggest_rule`, `name_thread`, `propose_admin_action`, `request_test_authorization`, `inspect_system`, `flag_wrong` and `external_available`, and dispatch, timers and connectors, as the other offers already do (`engine.rs:2092-2132`). This is a fourth line of the existing allowlist (`engine.rs:2139-2144`) and is asserted by an offer test like the existing one. Round 2 and repair seats are made toolless by spec 128 S1; this spec adds nothing to them.

- Why: spec 128 keeps Round 1's whole bridge and fixes the false positive with a ledger of Delphi's own writes. The ledger makes `t_0002`'s discard stop, but a seat can still ask Delphi to run research into the project, which no Council seat needs. Removing the trigger is the cheaper half. Both stay: if S5 is built, 128's S3 is still right as a guard for any other Delphi write during a run.
- It narrows authority only (invariant 1). It amends spec 128's non-goal "a Round 1 seat keeps its tools" to "a Round 1 seat keeps its reading tools" (OD-3).
- The `agy` native web tools (`search_web`, `read_url_content`) are not changed here (OD-4): a seat that reads the web is already lowered to L0 (`harness_run.rs:508-514`).
- A scratch working directory for the seat is NOT added. It would not have prevented `t_0002` (O10).

## 4. Authority invariants (what the tests prove)

1. The budget and the cut only remove or shorten context text, and only context from pointers; they never add text a caller wrote, never change a system section, the contract or a peer stance, and never raise a trust tier. The cut line and the task head are fixed text or escaped section text.
2. S5 only narrows the offered tools. No tool is added, no gate is loosened, and the call-time allowlist still denies a tool the offer forgot.
3. `heard` is derived from the seat's own words by deterministic matchers and the existing reader. It can lower a count; it never approves anything, never admits a seat, and a seat's words cannot make themselves count.
4. The new fields (`prompt_bytes`, `context_cut`, `heard`, `why`, `gate_decisions`) are counts, labels and fixed words, never contents.
5. No new path acts outside the sandbox or approves its own action. The probe in S6 spends the operator's tokens only with the operator's yes for that call (spec 104 section 5), and writes a record, never config.
6. A config key `max_prompt_bytes` is read from the ratified `[harness.<cli>]` block; Delphi never writes it.

## 5. Slices

One pull request may carry several slices of this one spec under the same-spec trial in `AGENTS.md` (each its own commit and trailer, own tracker row, no gate-changing paths, about 3 slices and 800 lines, merge commit). Hours are estimates for a person who knows the code.

| Slice | Content | Files (expected) | Hours | Depends on | Exit |
| :-- | :-- | :-- | --: | :-- | :-- |
| S0 | this spec, its compile, tracker rows | `specs/`, `compiled/`, tracker | 0 (done) | none | operator reads it and answers section 9 |
| S1 | count the prompt; `prompt_bytes` in `harness.started` and the receipt; schema | `harness_run.rs`, `seat.rs`, `schemas/seat-receipt.schema.json` | 2 | none | a fake-CLI test asserts the counts; the receipt schema test passes |
| S2 | the per-carrier byte budget, the cut function, `context_cut`, config key, `ask_family` and Panel | new `prompt_budget.rs`, `harness_run.rs`, `service.rs`, `boot.rs`, `config.rs`, example config, schema, docs | 6 | S1 | AC-1 to AC-4 |
| S3 | the `task` head for budgeted carriers | `ipr.rs`, `harness.rs` | 3 | S1 | AC-5, AC-6 |
| S4 | `heard`, the cut matcher, thresholds, `require_families` on heard seats, the failure words, the retry condition | `refusal.rs`, `council.rs`, `host_rpc.rs`, `seat.rs`, `service.rs`, `harness_run.rs` | 6 | S1; 128 S2 | AC-7 to AC-11 |
| S5 | reading tools only for Council and Panel seats | `engine.rs`, `tools.rs` (names only) | 4 | 128 merged (same files) | AC-12, AC-13 |
| S6 | the input-ceiling probe and its record (operator-run; section 8.2) | `capabilities.rs`, `endpoints` test command, `doctor` | 5 | S2 | AC-14; the operator runs it |

Together about 26 hours. Suggested pull requests: (1) S1 to S3, (2) S4, (3) S5 after 128 merges, (4) S6. S2 and S4 each touch `service.rs` and `council.rs`; where 128 has not merged, S4 and S5 wait for it, so that nothing is built on a text that may change.

## 6. Where this meets spec 128

| Concern | Spec 128 | This spec | Rule |
| :-- | :-- | :-- | :-- |
| Round 2 tools | toolless (S1) | none | 128 stands as written |
| Round 1 tools | all kept (non-goal) | reading tools only (S5) | this amends the non-goal, OD-3 |
| Empty finish | one retry; `harness.no_answer` (S2) | adds `prompt_bytes`, `budget`, `gate_decisions`; halves the budget if the first answer was a cut complaint | one retry, shared |
| Checkout guard | ledger of Delphi's own writes (S3) | none | independent; S5 removes one trigger, S3 stays |
| Logging at the default level | `harness_failed` at its own level (S2) | relies on it | none |

## 7. Why Codex and Copilot are not used in Councils (read-only; no config was changed)

Read from the code. The operator's `config.toml` has only `claude-sub` and `agy-sub`, and `[access] allowed_endpoints` lists those two. The code then explains three further reasons that would still hold once endpoints exist:

1. **Only configured, allowed families are planned.** `configured_families` lists the families of allowed endpoints (`council.rs:1284-1290`); `family_usable` needs a pool entry with a model (`engine.rs:1916-1932`). Codex and Copilot have no entry, so neither exists to the planner.
2. **A Council seat takes the family its role is assigned.** `assign` gives each role `role_family`'s family and moves a seat only to span two families, or when `require_families` asks (`council.rs:56-127`, `128-155`). An `openai` endpoint with no role mapped to it, and no `require_families = 3`, would sit idle even when configured. A Panel's foreign seat is different: `carrier_plan` ranks `copilot-acp` first for a seat with no tools (`carrier_plan.rs:78-87`) and Codex after the others, so a Panel would reach for Copilot once it is configured.
3. **What each seat would be.** A Codex seat and a Copilot reader seat are read-only, get no Delphi tools (`harness_run.rs:701` excludes Codex from the bridge; Copilot is started with `--available-tools` narrowed) and start at L0 (`harness_run.rs:578`, `591`); that is acceptable for advice. Copilot's endpoint must name its model and set `family` to that vendor, and an answer from a different model is a `model_mismatch` receipt, not a seat that sat (`config.example.toml:518-527`, `harness_run.rs:1082`, `1097`).

Nothing in the code stops either carrier being used once the operator proposes the endpoints, adds them to `allowed_endpoints`, gives them a model in the family table, and sets `role_family` or `require_families`. Both would hit the same unmeasured limit as `agy` (O13), which is why S2's budget defaults to 150 KB for them until measured (OD-2). Whether a Codex or Copilot seat obeys the critique schema is covered by the existing tests with a fake CLI, not by a live call; no Councils have run on them.

## 8. What is not verified, and how the limit is learned

1. **The 186 KB figure is one observation.** It is the file text delivered in one test with prompts that also held Delphi's fixed text; the red-team's own words in `t_0003` named a different cut (76,525 bytes) on a different prompt whose total size is not known. Two cuts do not show a constant ceiling. The limit may be bytes, tokens, a per-event size, or depend on the model and the agy version. `agy` reported 74,574 input tokens for the `t_0003` Round 2 seat (spec 128), so a pure token count of 50,000 is not what cut it either.
2. **Learning it.** (a) **S1 measures** the bytes every seat is sent, so after a few Councils the receipts hold prompt size against what the seat reported receiving. (b) **S6 measures the ceiling directly**: an operator-run extension of `delphi endpoints test --probe` that sends no-tool prompts of 100, 150, 200 and 250 KB made of filler with a one-line code word at the very end, asks the seat to reply with that code word, and records the largest size answered correctly and the smallest that was not. It costs about four calls and roughly 0.6 million input tokens on Gemini Flash, which is the operator's tokens, so it runs only on the operator's yes (or the ratified standing consent, spec 104). The result is a record keyed to the endpoint and the `agy --version`, with a time limit like the gate record (`capabilities.rs:59-87`); a new version has not been probed and falls back to the default. The effective budget is the smaller of the config key and 80 percent of the largest size answered. `delphi doctor` prints the budget and where it came from ("150,000, default, unmeasured"). (c) **No self-lowering**: a seat's complaint never changes the budget by itself; a model's words are not a measurement. It only makes S4's `heard` false.
3. **Fixed text.** How many bytes governance, role, skills and lessons add for each role is not measured here. It is probably tens of KB, which is why the budget is counted on the whole prompt and S1 comes first.
4. **The hook's "could not be reached" is unexplained.** Truncation explains the missing task. It does not obviously explain every tool call being denied with the hook's connect-failure text. The daemon's own log showed `harness.gate` policy denials at least once, so the socket was reachable then. This spec does not claim hypothesis 2 is false; S4's `gate_decisions` count is how the next occurrence is told apart.
5. **The cut marker's exact spelling and whether any carrier prints one.** Taken from the operator's report, not captured.
6. **That `request_research` can run at all inside a Council seat headless.** It asks for an operator card (`tools.rs:273`); who answers it in a host-raised Council was not checked. `t_0002` shows it ran.
7. **Nothing here was tried on a model.** The sandwich at both ends, the budget and the heard rule are tested with fake CLIs. Whether Gemini Flash then answers a 150 KB Council is the closing live check, and it spends the operator's tokens.
8. **Flash marks nearly every finding "verified"** (earlier notes on this carrier): a seat being heard does not make it right. This spec counts who was heard, not who was correct.

## 9. Acceptance criteria

1. **Counts (S1).** A fake `agy` that echoes its stdin length shows `harness.started.prompt_bytes` equal to the bytes written, and the receipt carries the same; a Claude seat also carries it.
2. **Under budget changes nothing (S2).** A prompt below the budget is byte-identical to today's.
3. **Over budget is cut, said, and never in the fixed text (S2).** Ten fake pointers of 40 KB for an `agy` seat produce a prompt at or under the budget; every slice ends at a line boundary with the fixed cut line; the system sections, the contract and any peer stance are byte-identical to the uncut prompt; the receipt's `context_cut` lists each cut label with kept and total lines and no contents.
4. **A prompt that cannot fit never starts (S2).** With fixed text over the budget the CLI process is not started, the seat abstains with the stated reason, and no tokens are recorded.
5. **Both ends (S3).** For `agy`, `codex-cli` and `copilot-acp` the user text starts with the `task` section and ends with the contract; with the last 70 KB removed from the prompt the objective and output contract are still present; a Claude seat's prompt is unchanged byte for byte; a caller's `</task>` or `<contract` text is escaped.
6. **`SECTION_TAGS` pin (S3).** The pinned list test lists `task` and still passes the "exactly one `<contract`" test.
7. **Heard (S4).** A Round 1 answer that is a decline, a cut complaint (with a `<truncated 76525 bytes>` marker, a "was truncated" sentence and a "task is missing" sentence) or empty is `heard: false` with the right `why`; a normal stance is `heard: true`.
8. **Fewer than two heard escalates (S4).** Three seats of which two decline or complain end in the escalation sentence naming them; two heard deliberate.
9. **`require_families` on heard seats (S4).** A two-family Council in which the second family's seat gave an unreadable Round 2 reports `require_families_met = false` and lists that seat with `heard: false`; the same Council with a readable critique reports true. The tokens of the unheard seat still count in the spend.
10. **`ask_family` (S4).** A cut complaint returns `unanswered` with the reason; an ordinary answer is unchanged; a cut prompt carries `context_cut`.
11. **One retry, half the budget (S4).** With spec 128's retry present: a first answer that is a cut complaint is retried once with half the budget; a second one abstains; a first answer that is not a complaint is not retried by this rule; the failure line carries the counts and no text.
12. **Reading tools only (S5).** A Council Round 1 seat's `tools_offered` is exactly the set in section 3.5 for Claude and `agy` seats; `request_research` and every `write_*` tool are absent; a call to one is denied at call time; a non-Council span's offer is unchanged.
13. **128 still holds (S5).** Round 2 seats stay toolless; the checkout guard tests of spec 128 pass unchanged.
14. **The probe (S6).** With a fake CLI that cuts at a known size, the probe records that size within one step and `doctor` prints the budget and its source; a record for another agy version is ignored.

## 10. Verification Plan

| Item | How it is verified |
| :-- | :-- |
| S1 | Engine tests with a fake CLI that records its stdin: bytes counted, receipt and event fields, schema test for the receipt. |
| S2 | Unit tests on the pure cut function (fair share, line boundary, fixed text untouched, passing unused share, cannot-fit case, no contents in `context_cut`); an engine test with ten large fake pointers for an `agy` seat. Config parse, schema and docs tests (`cargo test -p delphi --test docs`). |
| S3 | `ipr.rs` tests: both ends, escaping, the pinned list, Claude unchanged; a fake-CLI test that drops the tail and checks the task survives. |
| S4 | Matcher table test (the three shapes, a normal stance, a long answer that mentions truncation as a topic, a code-heavy answer); council tests over fake seats for the thresholds, `require_families`, `ask_family` and the retry condition. Ruling-card text tests. |
| S5 | An offer test beside the existing tool-offer tests (`tools.rs:1811`), with a call-time denial test and a non-Council control. |
| S6 | A fake CLI with a known cut; `doctor` line test. The real probe is run by the operator. |
| Gates | `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo run -q --locked -p delphi -- lint --repo .`, `delphi specs compile`, `delphi check spec` on this file. For this S0 draft only the last three were run (it changes markdown); CI runs the rest. |

No slice is verified by a live Council. Section 8, items 1, 4 and 7 stay open after every test above passes.

## 11. Open decisions for the operator

- **OD-1 (what the budget counts and its default).** *Recommendation:* count the whole assembled prompt (system text plus user text), default **150,000 bytes** for `agy`. The observed 186 KB was file text only, so the carrier's limit on a whole prompt is probably higher; 150 KB leaves room for Delphi's own text and is a cautious start. S1 and S6 replace the guess.
- **OD-2 (Codex and Copilot defaults).** *Recommendation:* the same 150,000 for `codex-cli` and `copilot-acp` until S6 or live use shows otherwise; none for `claude-cli`. *Alternative:* none for all but `agy`, which is simpler but trusts two carriers never measured.
- **OD-3 (amend 128's Round 1 non-goal).** *Recommendation:* yes, reading tools only (S5). *Alternative:* leave Round 1 as 128 has it and rely on 128's ledger alone. *Not taken:* a scratch checkout (section 3.5).
- **OD-4 (agy web tools in a Council seat).** *Recommendation:* leave them; the seat is already lowered to L0 when it uses them. *Alternative:* turn them off for Council seats, which makes the seat purely a reader of what it is given.
- **OD-5 (a decline counts as not heard).** *Recommendation:* yes in Round 1 and Round 2, and `ask_family` says unanswered. A refusal is not a stance, and the card already shows `declined`. This makes a Council escalate sooner when one seat refuses (for example on safety-filter wording).
- **OD-6 (the probe).** *Recommendation:* the operator runs S6 once per agy version on a "go", about four calls. *Alternative:* skip it and live with the 150 KB default.
- **OD-7 (retry with half the budget).** *Recommendation:* yes, as part of 128's one retry. *Alternative:* no retry for a cut complaint; the seat abstains and the Council reports it.
- **OD-8 (the interim rule).** *Recommendation:* in force from approval, applied by the orchestrator and architect when they frame a Council, until S2 is built.

## 12. Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | Count the prompt in bytes where it is assembled, hold a per-carrier budget, shorten the pointer slices to a fair share with a fixed cut line, and say what was cut in the receipt (S1, S2). | Pure unit tests on the cut function; an engine test with ten fake 40 KB pointers asserting the prompt size, untouched fixed text and the `context_cut` record. |
| B | constraint removed: pointers are no longer inlined | Send only the pointer lines and let a seat read what it needs with its own reading tool, so the prompt is always small. | A fake CLI given a pointer-only prompt must call `read_file`; run for a Round 2 seat, where spec 128 gives no tools, and show it cannot read anything. Rejected: it fails exactly where the failure was seen (toolless Round 2, `ask_family`) and moves the work onto the gate that reported "could not be reached". Kept only for Round 1 reading, as today. |
| C | map-reduce | A cheaper model first writes a 2 KB summary of each file and the Gemini seat reads the summaries. | Compare the Council's findings on a known-defect spec with whole files and with summaries. Rejected: it spends more tokens, puts a model's paraphrase between the seat and the source, and the repo already has a deterministic way to take a section (`delphi specs show --section`). |
| D | sandwich prompting [H-006], applied at both ends | Put the objective and the output contract at the top as well as the bottom for carriers with a budget (S3). | Drop the last 70 KB of a fake prompt and assert the task and contract remain; assert escaping and the pinned section list. Chosen with A, since the cost is small and it is the only part that helps when a cut happens anyway. |
| E | sentinel canary | A code word at the end of every seat's prompt which the seat must echo, so a cut tail is detected on every run. | Fake CLI that cuts the tail: assert the echo is missing. Rejected for every seat (it taxes every answer and a seat can fake the echo); used once, in S6, where the answer is the measurement. |
| F | interim rule only | The 150 KB advice of section 2 and nothing built. | Replay the three Councils with context under 150 KB and see whether the Gemini seats answer. Kept as the stopgap, not as the fix. |

Chosen: A and D together as S1 to S3, with S4 (counting who was heard) and S5 (reading tools only) as the parts that make the label "two families met" true, and S6 to measure rather than guess. Flagged by the guard: none.

## 13. Known limits

- A byte budget is a stand-in for whatever the carrier really limits; if the limit is tokens, a prompt of dense text or code can still be cut under the byte budget. S6's probe uses filler, so it measures bytes on plain text only.
- A cut at a line boundary can remove the part of a file that mattered. The fixed cut line tells the seat, and a host that cares passes the lines it needs with `#L`.
- The cut matcher reads English phrases; a seat that complains in another language is `unreadable` or `empty` by the other rules, not by the matcher.
- `heard` lowers a count; it cannot raise a seat's trustworthiness. A heard Gemini seat can still be wrong.
- Nothing here makes a Council on Gemini cheaper; S2 makes it smaller.
