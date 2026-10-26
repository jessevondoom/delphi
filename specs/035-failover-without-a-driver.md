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
Claude is the best default and stays the default. But nothing in Delphi should assume it is the only driver: when Claude runs out, the work should be able to continue on Gemini or on a local model, either to finish the task in hand or after asking the operator. `specs/002` §4.1 already built the machinery (a pool with per-endpoint state, exhaustion detection, promotion at a turn boundary, a confirmation card for a weaker substitute). This spec says where the code still leans on Claude, and what to change.

## 2. Findings (read from the code, 2026-10-26)
| # | Finding | Where |
| :-- | :-- | :-- |
| G-1 | **Capability is judged by tier only, so a local model passes for a frontier one.** `delphi setup` writes an Ollama endpoint with one model on all three tiers; the router counts a model as the wanted tier whenever it serves it, so when Opus is out a 9B local model is a *similar* substitute and takes over **silently**, with no card. `specs/002` §4.1 requires a card for anything less capable. | `route.rs` `rank_at`, `similar` |
| G-2 | **"Usually Claude" is a literal.** A role with no `role_family` is preferred to `claude`, even in a workspace that has only Gemini. | `config.rs` `family_for_role` |
| G-3 | **The order after the primary is accidental.** Past the preferred family the order is the endpoint's position and then pool order (tier-major), so which family stands in for Claude depends on declaration order. | `route.rs` `order` |
| G-4 | **There is one answer to "a weaker model is all that is left": ask.** An operator who would rather a half-done task finish on whatever is available, or never run on a weaker model, cannot say so. | `engine.rs` `model_turn` |
| G-6 | **An account-level failure was treated as one model's.** A billing error ("credit balance too low") or a spend cap is the account's, but the pool marks one `(endpoint, model)`, so the endpoint's *other* models stayed "available" and were offered as the stand-in, and the operator was asked to approve a model certain to fail the same way. Found by the live run of 2026-10-26. | `exhaust.rs`, `engine.rs` |
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
| D-7 | **It is visible.** The narrator's state carries which models are out (and until when) and which roles are running on a stand-in (from which model, why, whether the operator agreed or the policy did); the dashboard shows it. |

## 4. How it fits together
```
request ─► ideal = first usable of: target model, else primary family at the wanted tier
            │ usable            ─► Target
            └ not usable ─► candidates, in [access] failover order
                  peer available ──────────────► Similar   (automatic, model.promoted)
                  only weaker/reduced ─► policy
                        ask    ─► card (model.degraded_confirm) ─► run if confirmed
                        finish ─► in flight? run, model.promoted{agreed:"policy"}   else card
                        never  ─► wait for the primary
                  nothing ────────────────────► park (blocked: no_model)
```
A reduced model that is the *only* configured one is just the ideal and runs normally. A reduced model is never called "similar".

## 5. Built (2026-10-26)
- **F1, the classes and the order** (`config.rs`, `route.rs`): `[access] primary`, `failover`, `on_weaker`; `reduced` in the catalog (default true for `cost_model = "local"`); a reduced model is never similar to one that is not, and is the last resort among the weaker ones; the failover rank orders the stand-ins (an explicit `[routing.pool] order` still wins). `family_for_role` falls back to the primary family, not to the word `claude`. Tests: `route::tests::a_local_model_is_never_a_peer…`, `…the_catalog_can_say…`, `…the_failover_order_decides…`, `…the_primary_family_is_claude_where…`, `…the_failover_keys_are_checked…`.
- **F2, the policy** (`engine.rs`): `ask` (unchanged), `finish` (a span that has made a model turn carries on without a card; the `model.promoted` reason says "to finish the task under way, as [access] on_weaker = \"finish\" allows"), `never` (parks like a decline, with no card). Tests: `with_on_weaker_*`.
- **F3, visible** (`observe.rs`, `dashboard.ts`, `fold.ts`): `observe.state` carries `models_out` (with reset times) and `standins` (task, role, from, to, and who agreed: `operator`, `policy` or `automatic`); the dashboard has a `models` section when either is non-empty; the chat says once "continuing on X (Y is not available)".
- **D-8** (`exhaust.rs`, `engine.rs`): `is_account_wide`, applied to every model of the endpoint. Test: `a_billing_error_takes_every_model_on_the_endpoint_out…`.
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

## 7. Open
- A capability score per model (a number, not a class) so that "weaker" can be judged between two cloud models of different quality; today tier and context are all there is.
- Whether a Council seat may be a reduced model (it would be a weak voice in a debate).
- `finish` for a task that has a pending write: stop at the next approval card instead of finishing on a weaker model.
