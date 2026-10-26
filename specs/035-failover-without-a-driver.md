---
id: spec-035-failover-without-a-driver
title: "035: Failover without a driver's seat"
author: architect
timestamp: 2026-10-26T12:00:00Z
status: draft   # asked for by the operator on 2026-10-26; D-1..D-8 are proposals the operator has not yet confirmed (the code follows them, with the defaults unchanged); §5 says what is built
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the router and the confirmation card are existing, tested code; the new classes and policy are tested on fixtures and one live Ollama run
source_mission: 2026-10-23-foundational-design
tags: [spec, routing, failover, models, ollama, gemini, draft]
---

# Spec 035: Failover without a driver's seat

## 1. What this is
The point of checking down to a lesser model is that **no work is lost**, not that the work continues quickly. Resilience over speed: a slower, more careful path that saves first is the right one.

Claude is the best default and stays the default. But nothing in Delphi should assume it is the only driver: when Claude runs out, the work should be able to continue on Gemini or on a local model, either to finish the task in hand or after asking the operator. `specs/002` §4.1 already built the machinery (a pool with per-endpoint state, exhaustion detection, promotion at a turn boundary, a confirmation card for a weaker substitute). This spec says where the code still leans on Claude, and what to change.

## 2. Findings (read from the code, 2026-10-26)
| # | Finding | Where |
| :-- | :-- | :-- |
| G-1 | **Capability is judged by tier only, so a local model passes for a frontier one.** `delphi setup` writes an Ollama endpoint with one model on all three tiers; the router counts a model as the wanted tier whenever it serves it, so when Opus is out a 9B local model is a *similar* substitute and takes over **silently**, with no card. `specs/002` §4.1 requires a card for anything less capable. | `route.rs` `rank_at`, `similar` |
| G-2 | **"Usually Claude" is a literal.** A role with no `role_family` is preferred to `claude`, even in a workspace that has only Gemini. | `config.rs` `family_for_role` |
| G-3 | **The order after the primary is accidental.** Past the preferred family the order is the endpoint's position and then pool order (tier-major), so which family stands in for Claude depends on declaration order. | `route.rs` `order` |
| G-4 | **There is one answer to "a weaker model is all that is left": ask.** An operator who would rather a half-done task finish on whatever is available, or never run on a weaker model, cannot say so. | `engine.rs` `model_turn` |
| G-6 | **An account-level failure was treated as one model's.** A billing error ("credit balance too low") or a spend cap is the account's, but the pool marks one `(endpoint, model)`, so the endpoint's *other* models stayed "available" and were offered as the stand-in, and the operator was asked to approve a model certain to fail the same way. Found by the live run of 2026-10-26. | `exhaust.rs`, `engine.rs` |
| G-7 | **The ladder skips the family.** When Opus runs out, an equal-tier model on another family (Gemini Pro) is taken silently before the next model down in the same family (Sonnet), although quotas are per model: Opus out does not mean Sonnet is out, and Gemini keeps a separate pool for its top tier and another for the Claude and GPT-OSS models it serves. | `route.rs` |
| G-8 | **Nothing is saved before the downgrade.** The card asks whether to run on a weaker model; the work so far lives only in the log, with no statement of where it stood, so an operator who declines (or a task that waits for hours) has nothing to resume from but the raw events. | `engine.rs` |
| G-5 | **Nobody can see it happening.** The narrator prints a line when a model is out of quota and nothing structured: not which models are out, until when, or which role is running on a stand-in. | `observe.rs` |

What already works and is kept: state per `(endpoint, model)`, exhaustion and re-probe, promotion at a turn boundary with history downgraded across providers, `model.promoted` and `model.degraded_confirm` events, the card (Amber) whose answer covers one target→substitute pair for one task, and a card that times out being asked again.

## 3. Decisions (proposed)
| # | Decision |
| :-- | :-- |
| D-1 | **No family is special in code.** The primary family is `[access] primary`; if unset it is `claude` when an allowed Claude endpoint exists, else the first allowed family in declaration order. `role_family` still overrides it per role. |
| D-2 | **The order after the primary is the operator's.** `[access] failover = ["gemini", "local"]` lists the families tried, in order, when the primary has nothing usable. Unlisted families follow in declaration order, `local` last. |
| D-3 | **Two kinds of stand-in.** A **peer** (at least the wanted tier, context not shorter, not reduced) takes over without asking, as a *similar* model does today. Anything else is **weaker**: a lower tier, a shorter context, or a **reduced** model. A model is reduced when its endpoint is `cost_model = "local"`, unless the catalog entry says `reduced = false`; any model can be marked `reduced = true`. A reduced model is never a peer of a model that is not reduced, whatever tier it is mapped to. |
| D-4 | **The operator chooses what "weaker" means for work in hand.** `[access] on_weaker = "ask" \| "finish" \| "never"`, default `ask`. `ask`: the card, as today. `finish`: a task already under way (its span has made a model turn, or a card has already covered it) carries on on the stand-in without a card, the switch is logged and shown; a task that has not started asks. `never`: no weaker model runs; the task waits for the primary. Both keys live in `[access]`, which is ratified, so changing them is a Red proposal. |
| D-5 | **The unit of consent is the task**, unchanged: an answer to a card covers that target→substitute pair for that task, survives a restart, and silence is not consent. |
| D-6 | **The primary takes back over by itself.** Every model turn asks the router again, so when the primary's quota returns the next turn uses it. History is rebuilt for the target provider (`specs/002` §3.1), signed reasoning is not carried across. |
| D-8 | **Account-level exhaustion takes the whole endpoint out.** A billing error, a monthly spend cap or an account spend limit (`Exhaustion::is_account_wide`) marks every model on that endpoint exhausted at once, so the next candidate is on another endpoint or family, never a lesser model on the account that is out. A model's own quota (a CLI "quota reached", a long `retry-after`) stays one model's. |
| D-9 | **Stay in the family, go down one step at a time.** The ladder is: (1) the same model on another endpoint; (2) **the same lineage, the next model down** (Opus out: Sonnet, then Haiku); (3) only then other lineages, equal-tier peers first, then lesser models; a reduced (local) model last (D-3). `[access] failover_prefers = "family"` (default) is this order; `"capability"` is the earlier one, where an equal-tier model on any family beats a lesser one in the family. Quotas are kept per `(endpoint, model)`, so a model that is out never takes its siblings with it, except for an account-level failure (D-8). |
| D-10 | **A model's lineage is not always its endpoint's family.** Gemini's subscription serves Claude and GPT-OSS models from a pool of their own. `[models.catalog."<id>"] lineage = "claude"` says a model belongs to a lineage other than its endpoint's family; the ladder's "same lineage" uses it, and the quota state stays per endpoint and model. |
| D-11 | **Save first, silently, before anyone is asked.** When the only way on is a weaker model, the weaker model first writes a **save-point** for the task: (a) where the work stood when the model ran out, (b) a summary of the chat session, (c) the pointers a resumption needs (task, span, event range, worktree and branch, pending approvals). It goes to `local/missions/<id>/checkpoints/NNN-<task>.md` and is logged as `checkpoint.saved`. The record of what happened (a) is built from the log and the span's history **without a model**, so it exists even if the weaker model fails; the model-written summaries are added when the weaker model answers within a time limit, and are left out for a session that read untrusted content. It happens once per task and substitute, survives a restart, and takes as long as it takes. |
| D-12 | **Then the operator is told, in this order:** which model ran out, that the work is saved and where, and whether to accept the weaker model and continue. Declining, or no answer, leaves the save-point and waits for the primary; the task can be resumed later from it. |
| D-13 | **What each policy does after the save-point.** `ask`: the card. `finish`: carries on if the task is under way. `never`: waits. **Optimistic mode: continues without a card, carefully:** the save-point is still written, the switch is logged and shown, and while the task runs on the weaker model the permission mode's automatic approvals (worktree edits, pinned installs) are **off for that task**, so each write asks; Red actions are never automatic in any mode. |
| D-7 | **It is visible.** The narrator's state carries which models are out (and until when) and which roles are running on a stand-in (from which model, why, whether the operator agreed or the policy did); the dashboard shows it. |

## 4. How it fits together
```
request ─► ideal = first usable of: target model, else primary family at the wanted tier
            │ usable            ─► Target
            └ not usable ─► the ladder (D-9), one rung at a time, each model's own quota:
                  1. the same model on another endpoint                       ─► Similar   (automatic)
                  2. the same lineage, the next model down (Opus → Sonnet → Haiku)   ─┐
                  3. other lineages: an equal-tier peer ─► Similar (automatic)        ├─ Weaker
                  4. other lineages: lesser models; a reduced (local) model last      ─┘
                  nothing ─► park (blocked: no_model)
            Weaker ─► SAVE-POINT first, silently, by the weaker model (D-11)
                        ├ ask      ─► the card: what ran out, where the work is saved, accept?
                        ├ finish   ─► carries on if the task is under way, else the card
                        ├ never    ─► waits for the primary (the save-point stays)
                        └ optimistic ─► carries on, carefully: this task's writes ask (D-13)
```
A reduced model that is the *only* configured one is just the ideal and runs normally. A reduced model is never called "similar". A billing error or spend cap takes every model of the endpoint out (D-8); a model's own quota takes only that model out.

## 5. Built (2026-10-26)
- **F1, the classes and the order** (`config.rs`, `route.rs`): `[access] primary`, `failover`, `on_weaker`; `reduced` in the catalog (default true for `cost_model = "local"`); a reduced model is never similar to one that is not, and is the last resort among the weaker ones; the failover rank orders the stand-ins (an explicit `[routing.pool] order` still wins). `family_for_role` falls back to the primary family, not to the word `claude`. Tests: `route::tests::a_local_model_is_never_a_peer…`, `…the_catalog_can_say…`, `…the_failover_order_decides…`, `…the_primary_family_is_claude_where…`, `…the_failover_keys_are_checked…`.
- **F2, the policy** (`engine.rs`): `ask` (unchanged), `finish` (a span that has made a model turn carries on without a card; the `model.promoted` reason says "to finish the task under way, as [access] on_weaker = \"finish\" allows"), `never` (parks like a decline, with no card). Tests: `with_on_weaker_*`.
- **F3, visible** (`observe.rs`, `dashboard.ts`, `fold.ts`): `observe.state` carries `models_out` (with reset times) and `standins` (task, role, from, to, and who agreed: `operator`, `policy` or `automatic`); the dashboard has a `models` section when either is non-empty; the chat says once "continuing on X (Y is not available)".
- **D-8** (`exhaust.rs`, `engine.rs`): `is_account_wide`, applied to every model of the endpoint. Test: `a_billing_error_takes_every_model_on_the_endpoint_out…`.
- **F5, the ladder** (`config.rs`, `route.rs`): `lineage_of` (catalog `lineage`, else the endpoint's family), `[access] failover_prefers = "family" | "capability"`; with `family` the router tries the same lineage (equal or higher tier first, then the most capable lesser model) before any other lineage. Tests: `the_ladder_stays_in_the_family…`, `a_claude_model_served_from_geminis_own_pool…`, `under_capability_ordering…`. Four older tests that depended on the old order (equal tier on another family first) now state the order they use: the three that exercise promotion across providers select `failover_prefers = "capability"`.
- **F6, the save-point** (`checkpoint.rs`, `Engine::save_point`, the Weaker arm of `model_turn`): written before the card, once per task and pair (also across a restart), to `local/missions/<id>/checkpoints/NNN-<task>.md`, logged as `checkpoint.saved`. The part built from the span's history and the log needs no model; the weaker model adds two summaries (where the work stood, the chat) within `[routing] checkpoint_wait_sec` (default 120 each) unless `[routing] checkpoint_model = false` or the task read untrusted content. The card says "Your work is saved for resumption (path)." The chat shows the path too (optimistic mode has no card). Tests: `the_weaker_model_saves_the_work_before_the_operator_is_asked…`, `…even_when_the_weaker_model_fails…`, `a_task_that_read_untrusted_content_gets_a_save_point_with_no_model_written_text`, `with_on_weaker_never_the_work_is_still_saved…`, and `checkpoint::tests`.
- **Parked too.** When nothing is usable at all the task parks and a save-point is written the same way, from the log and the span's history only (no model can write): `when_no_model_is_usable_the_task_parks_with_a_save_point_built_from_the_log`. A save-point is per task, pair and turn, so asking again on the same turn reuses it and a later exhaustion writes a new one.
- **F7, carefully** (`approval.rs`, the Weaker arm): optimistic mode (or `finish` for a task under way) continues without a card; `Broker::mark_degraded` turns the mode's automatic approvals off for the task until it ends, whoever agreed. Tests: `in_optimistic_mode_the_task_continues_after_saving…`, `while_a_task_runs_on_the_weaker_model_the_permission_mode_approves_nothing_for_it`, and the broker test.
- **Known limits.** The save-point's two model calls are not charged to the mission budget (a few hundred tokens each, on the weaker model; the event names it). A save-point note is for reading: resuming does not need it (the log and `/resume` do). The provisional-decision review of `specs/015` is not used for the downgrade.
- **F4, live** (`e2e::live_claude_runs_out_and_the_task_finishes_on_a_local_model_after_the_operator_agrees`, `--ignored`): a mock "Claude" answering 402 and a real Ollama model. The first run showed G-6 (the card offered `mock/sonnet-t`); after D-8 the card offers `ollama/gemma4`, "a reduced local model (smaller, slower, less sure with tools)".

## 6. Acceptance criteria
- **AC-1:** with Opus unavailable and only a local model left, the route is *weaker* with `a reduced local model` among what is lost, never *similar*, even though the local model is mapped to the frontier tier.
- **AC-2:** a workspace with only Gemini endpoints prefers Gemini for every role; `[access] primary` and `role_family` override.
- **AC-3:** `[access] failover = ["local", "gemini"]` makes the local model the first stand-in; the default puts `local` last.
- **AC-4:** `on_weaker = "ask"` raises the card; `"finish"` runs a task in flight without one and a task not yet started with one; `"never"` runs nothing weaker and parks the task.
- **AC-5:** a peer on another family still takes over without a card.
- **AC-6:** `observe.state` lists the models that are out with their reset times and the roles running on stand-ins; the dashboard shows both.
- **AC-7:** the keys are in the ratified `[access]` block (a local edit is a pending proposal until ratified).
- **AC-8:** a billing error on an endpoint makes none of its models a candidate, and no card offers one.
- **AC-9:** with Opus out and Sonnet up, the route is Sonnet, not a Gemini peer; with the whole family out, the equal-tier peer comes before a lesser model of another lineage; `failover_prefers = "capability"` restores the old order.
- **AC-10:** a Claude model served through the Gemini subscription (`lineage = "claude"`) counts as the same lineage.
- **AC-11:** before any card, a save-point file exists with the task, the reason, the last turns, the session and the resume pointers; `checkpoint.saved` precedes `approval.requested`; the card names the file.
- **AC-12:** if the weaker model fails to write its summaries the save-point still exists, with the part built from the log; for a tainted task no model-written text is in it.
- **AC-13:** with `never` the save-point is still written; in optimistic mode no card is raised, the task continues, and its writes ask until it finishes.

## 7. Open
- A capability score per model (a number, not a class) so that "weaker" can be judged between two cloud models of different quality; today tier and context are all there is.
- Whether a Council seat may be a reduced model (it would be a weak voice in a debate).
- `finish` for a task that has a pending write: stop at the next approval card instead of finishing on a weaker model.
