---
id: step-3c-review-seat-b
title: "Step 3c independent review: Seat B (correctness, state and concurrency), verbatim"
author: red-team
timestamp: 2026-10-25T00:30:00Z
source_type: external_untrusted
confidence: low
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, step-3c, review]
---

Model: `gemini-3.8-flash-high` through `agy --mode plan` (read-only tools only; a first run was refused a shell command and produced nothing, so it was re-run with the instruction to use only the file-reading tools). The output below is verbatim and **untrusted**: every finding is checked against the code in [`report.md`](report.md) before anything is done about it.

````text
FINDING P1 Ineligible target model causes resolve_route to return NoModel instead of fallback candidates
File: crates/delphi-engine/src/route.rs:188-197
Quote:
```rust
    let ideal: Option<PoolKey> = match req.target.as_deref().filter(|t| !t.is_empty()) {
        Some(t) => by_preference.iter().map(|(_, k)| k).find(|k| k.model_id == t).cloned(),
        None => {
            // The first pair, in preference order, at the nearest tier (`min_by_key` keeps the first minimum).
            let want = rank(&req.tier);
            by_preference.iter().map(|(_, k)| k).min_by_key(|k| ((rank_at(cfg, k, want) - want).abs(), (rank_at(cfg, k, want) < want) as i32)).cloned()
        }
    };
    let Some(ideal) = ideal else {
        return Route::NoModel { target: None, blocked: vec![], ineligible: rejected, back_at: None };
    };
```
Scenario: A role configures a target model in `[routing.targets]` (e.g. `builder = "gem-agy"`), but that target model fails a hard constraint (e.g. `gem-agy` cannot mutate for a Builder, lacks prices, or exceeds context limits). `by_preference` contains only eligible models and thus excludes the target, leaving `ideal` as `None`. Line 196 immediately returns `Route::NoModel`, ignoring other healthy, available models in the pool.
Why it matters: Spec 002 §4.1 mandates that Delphi try to satisfy the target and otherwise substitute similar or weaker models. An ineligible target model completely blocks the role instead of falling back to eligible models.

FINDING P1 Mid-span API-to-harness fallback launches harness from initial prompt and discards prior turns
File: crates/delphi-engine/src/engine.rs:545-551
Quote:
```rust
                        if esc.0.starts_with(EXHAUSTED) && !harness_tried {
                            if let Some(hep) = self.route_harness(&sc) {
                                m.append(sc.ev("harness.fallback", json!({"from": "api", "to": hep.id, "reason": esc.0}))).await?;
                                break match self.run_harness(&mut sc, &ipr, &hep).await? {
                                    Ok(text) => Ok(text),
                                    Err(f) => Err(Escalate(format!("{}; then the {} harness: {}", esc.0, hep.id, f.reason))),
                                };
                            }
                        }
```
Scenario: A span executes turn 1 on an API endpoint, successfully completing tool calls and writing modifications to the worktree. On turn 2, the API endpoint is exhausted. Line 545 sees `!harness_tried` and launches `run_harness`, which runs the harness CLI with `user_prompt: ipr.first_user_text()` without passing earlier turns or conversation history, and breaks out of the span loop with the harness result.
Why it matters: Harness spans cannot be resumed or promoted mid-span. Re-executing from `first_user_text` causes previously executed tool actions to be repeated or lost while abandoning prior turns recorded in the log.

FINDING P1 Parked task ignores cancellation and continues polling until model availability
File: crates/delphi-engine/src/engine.rs:940-950
Quote:
```rust
        loop {
            tokio::time::sleep(self.settings.park_poll).await;
            if self.settings.park_max.is_some_and(|max| started.elapsed() >= max) {
                sc.m.append(sc.ev("task.unblocked", json!({"resumed": false, "reason": "waited as long as allowed"}))).await?;
                return Ok(false);
            }
            self.reprobe_due(&sc.m).await?;
            if self.ready_for_turn(sc) {
                sc.m.append(sc.ev("task.unblocked", json!({"resumed": true, "waited_secs": started.elapsed().as_secs()}))).await?;
                return Ok(true);
            }
        }
```
Scenario: A task blocks in `park()` waiting for an exhausted model. The operator cancels the task via `cancel_task(task_id)` (`engine.rs:594`), appending `task.cancelled` to the log. The `park()` loop never inspects task cancellation or abort status; it continues sleeping and polling until a model recovers, whereupon `ready_for_turn` returns true, the task unblocks, and the cancelled task resumes execution.
Why it matters: Operator cancellations do not interrupt parked tasks, allowing cancelled tasks to wake up and perform unrequested work and tool mutations.

FINDING P1 Park wait elapsed duration is lost across daemon restarts because recovery ignores task.blocked
File: crates/delphi-engine/src/engine.rs:938
Quote:
```rust
        let started = *sc.blocked_since.lock().unwrap().get_or_insert_with(std::time::Instant::now);
```
Scenario: A task is parked with `park_max = Some(10m)` and has been waiting for 9 minutes when the daemon restarts. `recovery.rs:117-220` does not parse or track `task.blocked` or `task.unblocked`, treating the span as a standard running span. When `resume_task` restarts `drive()`, `blocked_since` is reinitialized as `None`, causing `started` to reset to `Instant::now()` and forcing the task to wait an additional 10 minutes.
Why it matters: Spec 002 §4.1 requires that `max_park_secs` bound the total wait from when the task first blocked across restarts; resetting the timer allows parked tasks to remain blocked indefinitely over repeated restarts.

FINDING P1 Retry ladder attempt 3 mislabels homogeneous retry as heterogeneous when failover occurs
File: crates/delphi-engine/src/build.rs:149-163
Quote:
```rust
                    match self.other_family(&base_family, true) {
                        Some(f) => {
                            family = f.clone();
                            h.routing_directive.model_family = Some(f);
                        }
                        None => homogeneous = true,
                    }
                }
            }
            m.append({
                let mut e = EventEnvelope::new(&m.id, "build.attempt", json!({"attempt": attempt, "tier": h.routing_directive.model_tier, "family": family, "thinking_budget": h.routing_directive.thinking_budget_tokens, "homogeneous_retry": homogeneous}));
                e.task_id = Some(task_id.to_string());
                e
            })
            .await?;
```
Scenario: On attempt 3 of `build_ladder`, `other_family` initially finds an alternate family `f` and emits `build.attempt` with `homogeneous_retry: false`. When `run_span` executes, family `f` throttles or exhausts, and `model_turn` fails over to an available endpoint on `base_family`. The attempt runs and completes on `base_family`, but `build.attempt` already recorded that the retry was heterogeneous.
Why it matters: The mission log mislabels a homogeneous retry as heterogeneous, violating rule [H-008] and obscuring that the author's own model family verified the build.

FINDING P1 Harness fallback to API fails when API only has weaker models
File: crates/delphi-engine/src/engine.rs:502-505
Quote:
```rust
                    Err(f) if f.retryable && self.has_api_route(&sc) => {
                        m.append(sc.ev("harness.fallback", json!({"from": hep.id, "to": "api", "reason": self.secrets.redact(&f.reason)}))).await?;
                    }
                    Err(f) => harness_result = Some(Err(Escalate(f.reason))),
```
Scenario: A task begins on a subscription harness (`claude-sub`). The harness fails with a retryable startup error before using tools. The API has only weaker models available (e.g. workhorse instead of frontier). `has_api_route` (`engine.rs:1010-1013`) only matches `Route::Target` and `Route::Similar`, returning `false`. Line 502 fails to match, and line 505 immediately escalates and halts the task.
Why it matters: Spec 002 §4.1 specifies that Delphi should not stop when only weaker models are available, but ask the operator for confirmation. The harness fallback check blocks fallback to weaker API models entirely.

FINDING P1 In-span subscription rate limit rejection is immediately cleared by record_success
File: crates/delphi-engine/src/harness_run.rs:363-366 and 399-401
Quote:
```rust
        if let Some(x) = &exhaustion {
            batch.push(sc.ev("model.exhausted", json!({"endpoint_id": ep.id, "model_id": launch.model, "reason": x.reason, "reset_at": x.reset_at, "raw": self.secrets.redact(&x.raw)})));
            self.pool.record_exhausted(&ep.id, &launch.model, x);
        }
```
and
```rust
        if self.pool.record_success(&ep.id, &launch.model) {
            sc.m.append(sc.ev("model.recovered", json!({"endpoint_id": ep.id, "model_id": launch.model}))).await?;
        }
```
Scenario: During a CLI session, Claude CLI emits a `rate_limit_event` with `status: "rejected"`, but returns a completed final turn (`is_error: false`). Line 365 records the endpoint as `Exhausted` in the pool. Thirty lines later, line 399 unconditionally calls `record_success(&ep.id, &launch.model)`, which immediately reverts the endpoint state from `Exhausted` back to `Available`.
Why it matters: The explicit quota exhaustion signal from the subscription harness is undone within the same span, causing subsequent dispatches to continue routing to a rejected harness.

FINDING P1 Duration parsing panics on large duration values exceeding u64::MAX
File: crates/delphi-adapter/src/exhaust.rs:55-65
Quote:
```rust
    let mut secs = 0.0f64;
    for c in re(&PART, r"(\d+(?:\.\d+)?)([hms])").captures_iter(s) {
        let n: f64 = c[1].parse().ok()?;
        secs += n * match &c[2] {
            "h" => 3600.0,
            "m" => 60.0,
            _ => 1.0,
        };
    }
    Some(Duration::from_secs_f64(secs))
```
Scenario: A provider reply contains a duration string with an astronomical value (e.g. `Resets in 99999999999999999999999999999999h`). `c[1].parse::<f64>()` parses the number as a large finite float. `Duration::from_secs_f64(secs)` panics because `secs` exceeds `u64::MAX as f64`.
Why it matters: A panic during error classification crashes the thread or async task handling the model turn.

FINDING P1 Zero-second duration in CLI error classifies as exhaustion and locks out model for 15 minutes
File: crates/delphi-adapter/src/exhaust.rs:116-119 and crates/delphi-engine/src/pool.rs:260-268
Quote:
```rust
    let says_quota = ["quota reached", "usage limit reached", "reached your usage limit", "hit your limit", "hit your session limit"].iter().any(|p| lower.contains(p));
    if says_quota || (lower.contains("resource_exhausted") && reset.is_some()) {
        return exhausted("quota reached", reset, text);
    }
```
and in `pool.rs`:
```rust
            let known = x.reset_at.filter(|r| *r > now);
            e.state = State::Exhausted;
            e.since = now;
            e.until = known;
...
            e.next_probe_at = if known.is_some() { None } else { Some(now + t.reprobe_first_secs) };
```
Scenario: A CLI error outputs `RESOURCE_EXHAUSTED: rate limit exceeded. Resets in 0s.` `parse_reset` returns `Some(now)`. In `classify_cli_error`, `reset.is_some()` causes the error to be classified as `Exhausted`. In `pool.rs:260`, `x.reset_at.filter(|r| *r > now)` filters out `now` (`now > now` is false), setting `e.until = None` and `e.next_probe_at = Some(now + 900)`.
Why it matters: A transient 0-second rate limit is misclassified as exhaustion and placed on a 15-minute re-probe timer rather than cooling down immediately.

FINDING P2 Sliding window cap of 32 in pool.rs prevents Rule 3 exhaustion during sustained throttling
File: crates/delphi-engine/src/pool.rs:230-235
Quote:
```rust
            if e.throttles.len() > KEEP_THROTTLES {
                e.throttles.remove(0);
            }
            let n = t.throttle_attempts as usize;
            // A run of at least `n` consecutive throttles that has lasted `throttle_window_secs`.
            if e.throttles.len() >= n && now - e.throttles[0] >= t.throttle_window_secs {
```
Scenario: An endpoint is throttled repeatedly under load (e.g. every 10 seconds). After 32 attempts (320 seconds), `e.throttles.remove(0)` shifts the earliest tracked timestamp forward on every subsequent throttle. `now - e.throttles[0]` never exceeds 320 seconds and can never reach `throttle_window_secs` (600 seconds).
Why it matters: High-frequency throttling never transitions to `Exhausted` under Rule 3 because the fixed 32-element buffer continuously truncates the window history.

FINDING P2 Spec 002 §4.1 Test 7 and retry tests in build.rs silently skip assertions without an OS sandbox
File: crates/delphi-engine/src/build.rs:353-356
Quote:
```rust
        if f.engine.exec.sandbox.is_none() {
            return;
        }
```
Scenario: In `a_model_running_out_mid_build_is_promoted_without_using_a_retry_attempt` (Spec 002 §4.1 Test 7) and related build ladder tests, if `f.engine.exec.sandbox` is `None` (the default when sandbox-exec or bubblewrap is unavailable), the test executes an early `return` without evaluating any assertions.
Why it matters: Key regression tests for exhaustion promotion and retry ladders silently pass without testing anything on non-sandboxed environments.

FINDING P2 In-stream SSE and AWS EventStream errors bypass exhaust.rs classification
File: crates/delphi-adapter/src/http.rs:184-188 and crates/delphi-engine/src/model.rs:90
Quote:
```rust
            for se in asm.handle(&ev).map_err(|e| match e {
                StreamError::Provider { kind, message } => CallError::Stream(StreamError::Provider { kind, message: redact(&message) }),
                other => CallError::Stream(other),
            })? {
                on_event(se);
            }
```
and in `model.rs`:
```rust
                Err(CallError::Stream(StreamError::Provider { kind, message })) if matches!(kind.as_str(), "overloaded_error" | "rate_limit_error" | "throttlingException" | "ThrottlingException" | "serviceUnavailableException" | "modelNotReadyException" | "RESOURCE_EXHAUSTED" | "UNAVAILABLE" | "rate_limit_exceeded") => Err(ModelError::Throttled(message)),
```
Scenario: An AWS Bedrock or SSE stream returns an HTTP 200 header followed by an in-stream exception signaling quota exhaustion (e.g. `RESOURCE_EXHAUSTED` with reset details). `http.rs` returns `CallError::Stream(StreamError::Provider)`, bypassing `classify_http`. In `model.rs:90`, it is unconditionally converted to `ModelError::Throttled`, ignoring the exhaustion signal.
Why it matters: Quota exhaustion delivered inside stream frames is treated as a transient rate limit, causing unnecessary backoff loops rather than immediate model promotion.

FINDING P2 Council available_families checks whether any model in family is usable rather than frontier tier
File: crates/delphi-engine/src/council.rs:608
Quote:
```rust
        f.retain(|fam| self.family_usable(fam, false));
```
Scenario: A secondary family (e.g. Gemini) has its frontier model exhausted, but its fast model is available. `available_families` calls `self.family_usable("gemini", false)`, which returns `true`. Council assignment seats `red-team` on Gemini. During the council run, `red-team` requests `frontier`, finds only the `fast` model, and stalls waiting for degraded confirmation instead of running the Council as `homogeneous` upfront.
Why it matters: Councils block and critic seats abstain instead of degrading smoothly to homogeneous mode as required by spec 002 §4.1.

FINDING P2 Model promotion event is emitted for intermediate candidate that immediately throttles
File: crates/delphi-engine/src/engine.rs:781-785 and 853
Quote:
```rust
            if let Some((target, why)) = passed_over {
                if sc.last_key.lock().unwrap().as_ref() != Some(&this_key) {
                    m.append(sc.ev("model.promoted", json!({"from": {"endpoint_id": target.endpoint_id, "model_id": target.model_id}, "to": {"endpoint_id": key.endpoint_id, "model_id": key.model_id}, "reason": self.secrets.redact(&why)}))).await?;
                }
            }
```
and line 853:
```rust
                        *sc.last_key.lock().unwrap() = Some(this_key);
```
Scenario: When target model A exhausts, `resolve_route` selects substitute B. Line 783 emits `model.promoted { from: A, to: B }`. Model B immediately throttles on `self.model.call` (`ModelError::Throttled`). Line 853 is never reached, leaving `last_key` as `None`. The loop continues, selects substitute C, and emits `model.promoted { from: A, to: C }`.
Why it matters: The event log reports that model B was promoted into service even though it failed without ever executing a turn.

FINDING P2 resume_task omits re-probing due models and clearing degraded decisions
File: crates/delphi-engine/src/engine.rs:685-702
Quote:
```rust
    pub async fn resume_task(self: &Arc<Self>, m: &Arc<MissionState>, rec: &Arc<Recovered>, task_id: &str) -> Result<TaskOutcome, EngineError> {
```
Scenario: Unlike `start_task` (`engine.rs:343, 346`), `resume_task` never calls `self.reprobe_due(m)` nor cleans up `self.degraded.remove(task_id)` upon completion. If an exhausted model's re-probe timer elapsed while the task was interrupted or daemon was stopped, resuming the task routes without probing the primary model.
Why it matters: Resumed tasks fail to detect recovered primary models and continue running on degraded substitutes.

REVIEW COMPLETE
````
