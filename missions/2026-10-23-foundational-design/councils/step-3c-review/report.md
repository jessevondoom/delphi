---
id: step-3c-review-report
title: "Step 3c independent review: what the seats found and what was true"
author: orchestrator
timestamp: 2026-10-25T00:40:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, step-3c, review, report]
---

# Step 3c independent review (2026-10-25)

The model pool (D-26) was written by a Claude model. For an independent look it was given to reviewers from other families, as `[H-008]` asks: two Gemini 3.8 Flash seats with different focuses, read-only through `agy`. The code reviewed is `main` at `a80a8d1` (all of Step 3c, M1 to M8). Brief and focus questions: [`brief.md`](brief.md). Raw outputs, untrusted: [`seat-a.md`](seat-a.md), [`seat-b.md`](seat-b.md).

**Seats.** A: security, consent and policy. B: correctness, state and concurrency. A third seat (GPT-OSS 120B, a broad second opinion) could not run: its quota was exhausted (resets in about 141 hours), which is the situation the feature handles.

**Method.** Seats are not trusted. Each finding was checked against the code before anything was changed; some were wrong or overstated. Both seats first returned nothing because they tried to run a shell command, which headless `agy` refuses and which was not granted; they were re-run with an instruction to use only the file-reading tools.

## Result

19 findings. After checking: **13 were real and are fixed**, 1 was real but overstated and is partly fixed, 2 were wrong, 2 are real and deferred, 1 is true and by design.

| # | Seat · severity claimed | Finding | Verdict | What was done |
| :-- | :-- | :-- | :-- | :-- |
| A1 | A · P0 | A write-capable role can be routed to the `agy` harness | **Overstated; the real point is capability loss.** A harness span for a non-Builder runs in an empty directory with `Tools::None`; `agy` output is untrusted L0. Nothing executes Delphi's write tools. What is true: since M4 a tool-using role (the Orchestrator needs dispatch, the Architect writes specs) can fall back to a harness and silently becomes text-only. | `harness.fallback` now says so ("a harness answers in text only: this role's Delphi tools are not available there"). The fallback is also now only allowed before a span's first turn (see B2). **Decided by the operator (2026-10-25): no extra confirmation.** Listing the harness as allowed is the consent; the fallback event says the role is text-only there. |
| A2 | A · P0 | Unredacted text in the approval card summary | **Quote accurate, premise not.** Line 741 did not redact `why`; but `why` is built from our own fixed phrases (`exhaust.rs` reasons), not provider free text, so no secret reaches it today. Severity P2 hardening. | Fixed: the summary is redacted. Test registers a secret in a pool reason and checks the card and the log. |
| A3 | A · P1 | Confirmation keyed on the substitute only: consent to sonnet-for-opus covers sonnet-for-a-1M-window-model | **Valid.** | Fixed: decisions are keyed by (target, substitute); events now carry the target; an answer logged without a target is never trusted. |
| A4 | A · P2 | Timestamp arithmetic on `pool-state.json` can overflow | **Valid (debug builds panic; release wraps).** | Fixed with saturating arithmetic; test with `i64::MIN` timestamps. |
| B1 | B · P1 | A target that is served but ineligible (a read-only harness model as a Builder's target) gives `NoModel` while other models are usable | **Valid.** | Fixed: a target nothing eligible can meet falls back to the ordinary choice. Test. The earlier test that pinned the dead end was changed. |
| B2 | B · P1 | API-to-harness fallback after turns have run starts the harness from the first prompt, repeating tool calls | **Valid** (the fallback predates M4, but M4 widened it across families). | Fixed: only before a span's first turn; the first-turn shortcut also no longer fires after a harness was already tried. Test: a span with a completed tool turn is not handed to the harness. |
| B3 | B · P1 | A parked task ignores cancellation | **Wrong.** Cancelling aborts the task's future (`service.rs` `task_cancel`), which drops `park` at its `sleep`. Covered by the M8 test that cancels a parked task. | None. |
| B4 | B · P1 | The park timer restarts after a daemon restart | **Not a defect as specified.** Spec 002 §4.1 counts `max_park_secs` "across repeated asks", not restarts; a restarted task is offered to the operator by recovery, who is in the loop. | None; documented. |
| B5 | B · P1 | The ladder logs `homogeneous_retry: false` at planning, then the attempt can run on the author's family | **Valid, minor.** The label is plan-time; the truth is the span's `model.homogeneous` event, which is emitted. | **Deferred.** |
| B6 | B · P1 | A failed harness cannot fall back to an API that has only weaker models | **Valid.** | Fixed: a weaker API model counts, so the operator is asked. Test: harness exits, API is weaker, the card appears, Allow runs it. (Building the test found that the same fix was incomplete: the first-turn shortcut still escalated after a harness had failed. Also fixed.) |
| B7 | B · P1 | A `rejected` rate-limit event marks the pair exhausted, then `record_success` clears it | **Valid in effect** (adjusted: a run that finished is not exhausted). | Fixed: exhaustion is recorded only if the run failed. Test. |
| B8 | B · P1 | `Duration::from_secs_f64` panics on an absurd number in provider text | **Valid.** A provider-controlled string could panic the classifier. | Fixed (`try_from_secs_f64`, resets beyond 400 days ignored). Tests with 400-digit and past-`u64` values. |
| B9 | B · P2 | "Resets in 0s" with `RESOURCE_EXHAUSTED` classifies as exhaustion | **Valid.** | Fixed: a reset already due is not a reset time. Test. |
| B10 | B · P2 | The 32-entry throttle list can hide a long run of throttles | **Valid in principle**; unreachable through the engine (cooldowns space throttles out). | Fixed anyway: the first entry (when the run began) is kept. Test. |
| B11 | B · P2 | Ladder tests return early without an OS sandbox | **True and by design** (the existing pattern); the Linux bubblewrap CI job runs them. | None. |
| B12 | B · P2 | A quota error delivered inside a 200 stream is always a throttle | **Valid.** | Fixed: the stream's error text goes through the classifier (`map_call_error`). Tests. |
| B13 | B · P2 | The Council counts a family as available if any model in it is usable, not one at the seat's tier | **Valid, minor** (the seat then asks or abstains instead of degrading upfront). | **Deferred.** |
| B14 | B · P2 | `model.promoted` is emitted for a substitute that then throttles at once | **Valid.** | Fixed: reported once the substitute has served a turn. Test with a three-endpoint scenario. |
| B15 | B · P2 | `resume_task` neither re-probes due models nor drops its decisions | **Valid.** | Fixed. |

The seats also reported four checks that hold (A: the endpoint pin, approval integrity, taint, denial of service). I did not re-verify those beyond the existing tests.

## What the review taught

- The two most valuable findings were not the loudest. A "P0" was mostly a misreading, while a P1 (B2) and a P2 (B12) were real gaps.
- Writing the test for B6 exposed that my first fix was incomplete, which the seat did not say. Independent reviewers find what they were pointed at; the test finds what the fix missed.
- Both seats tried to run a shell. Reviewer prompts must say "file-reading tools only" up front.

## Left open

1. ~~Whether a harness for a tool-using role should need confirmation (A1).~~ Decided 2026-10-25: it should not.
2. B5 and B13, minor, deferred.
3. A third family for the next review (GPT-OSS, when its quota returns).
