---
id: gap-closure-plan-2026-10-25
title: "Gap closure plan: away mode, headless, API readiness, and the smaller gaps"
author: strategist
timestamp: 2026-10-25T12:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-10-23-foundational-design
tags: [plan, gaps, away-mode, headless, api-readiness, skills, hooks]
---

# Gap closure plan (draft, 2026-10-25)

This follows a comparison of Delphi with Claude Code, `agy`, `hermes-agent` and Vellum. It plans four of the gaps found: **1** (approvals only work at the terminal), **3** (dependence on the provider CLIs), the **smaller gaps** (skills, hooks, remote execution), and the headless work that gap **4** needs. The test design for gap 4 is in [`evaluation-plan-2026-10-25.md`](evaluation-plan-2026-10-25.md).

Nothing here is ratified or committed work. Competitor details came from secondary sources and need checking against official docs before anything depends on them. Where this plan says "unverified", I did not confirm it in the code or the docs.

## 0. Ground truth in the code today

| Fact | Where | Why it matters here |
| :-- | :-- | :-- |
| Two client scopes: `Console` (privileged, includes `approval.resolve`) and `Narrator` (read-only). | `crates/delphi-core/src/proto.rs` | A remote approver needs a third, narrower scope. Handing a phone the `Console` scope would give it `/budget`, ratification and `/rewind`. |
| Answers are bound to the blake3 of the exact payload shown; first answer wins; a timed-out request can't be resurrected. | `crates/delphi-engine/src/approval.rs` | These are the properties a remote channel must keep. Most of the work is not breaking them. |
| Timeout (default 600 s) always denies, console attached or not; Red is never auto-approved by any mode. | same | Fine at a desk. For an away operator, the task dies at the first card nobody answered. |
| `delphi init --non-interactive` exists, but there is no non-interactive way to run a task. `/build` and `/council` exist only as console input. | `crates/delphi/src/main.rs`, `chat.rs` | Gap 4 and CI use both need a headless entry point. |
| Direct drivers exist for Claude (also Bedrock and Vertex), Gemini and OpenAI-compatible chat. Spec 009 calls API keys "the least common configuration". | `crates/delphi-adapter/src/{claude,gemini,openai}.rs`, spec 009 | "API readiness" is mostly about what is proven live, not what is written. I did not check which paths have run live. |
| Harness mode has no failover to an API endpoint inside a span, and harness spans are not resumable. | spec 009 §7 "As built" | A subscription running dry mid-task is a hard stop today. |
| Skills are `SKILL.md` with `name` and `description` plus Delphi fields (`authorized_agents`, schemas, `required_permissions`). | `skills/*/SKILL.md` | `name` and `description` match what the competitors' skills use. Whether the rest is compatible with the agentskills.io standard is unverified. |
| Several red-team sweep findings are still open: blank price silently disables budget reservation, harness reader hangs on a non-UTF-8 line, `agy` tool states, system prompt in argv. | roadmap review §2 | These are API-readiness and provider-dependence bugs. They are folded into phase 2 below. |

## 1. Gap 1: away mode

**Goal:** a build can run while the operator is away, cards reach the operator on a device they carry, and the answers keep every property listed in §0.

### 1.1 Design

**A third scope, `relay`.** It can read approval and mission-summary events and call `approval.resolve`, nothing else. Tokens come from `issue_token` on a `Console` session, one per channel, revocable. It cannot touch `/budget`, `/permissions`, ratification, rewind or the config.

**The relay is its own process, not part of the daemon.** `delphi relay <channel>` connects like the Narrator does. The daemon gets no network code, and the sandbox and egress story stays unchanged. A relay crash can't stall a mission: cards still time out as today.

**The relay is an operator instrument, not an agent connector.** Spec 010's Slack connector ingests untrusted content and taints what reads it. The relay's channel is the control plane. Mixing them is the main risk in this design. Rules:
- Outbound-only transports (Slack Socket Mode, Telegram long-poll), so no inbound port is opened.
- Answers are accepted only from an allowlist of platform user IDs, in a direct message, never a shared channel.
- Message text is never parsed as a command. Only a structured button callback or a short reply code is accepted.
- Each answer carries `approval_id` plus the payload hash prefix shown on the card, so a stale or forged tap approves nothing.
- Any other inbound text is dropped and logged. It never reaches an agent.

**Tier policy for remote answers** (`[away]` config, operator-ratified, default in bold):
- Amber: answerable remotely.
- Red: **console only**, or `step-up`. With `step-up`, the card shows a one-time 4-character code that the operator has to type back, so a blind tap can't approve a Red action. Red is never covered by "allow similar" and is never auto-approved. Both are unchanged.

**Parking, so an absent operator doesn't kill the task.** This is the one invariant to amend, and it needs a ratified decision. Today a timeout denies and the agent gets a structured denial. Proposal:
- The timeout still denies that request. The stale approval id stays dead, and a late answer still can't resurrect it.
- In away mode the *task* is then parked, not failed. It stops spending (no model calls), records `task.parked {reason: approval_timeout}`, and when the operator answers later, a **new** card is raised with a new `approval_id` and a new code.
- Parking is bounded by `[away] park_max` (default 24 h). Past it, the task fails as today.

**Notifications.** The relay posts: card raised, merge ready, task failed, budget at 80%, mission done. Messages are rate-limited and pass through the existing redactor. `detail = "summary"` is the default, so card content stays out of the third-party service unless the operator opts into `full`.

**Starting work from the phone is out of scope.** Command ingress from a chat channel is prompt injection waiting to happen. Phase 1 is approvals and notifications only. If it is ever added, it comes with the allowlist, a fixed command grammar and a confirmation card.

### 1.2 Tests
Written before the code, using a fake channel adapter:
1. A forged sender, a non-allowlisted user and a shared-channel message are all ignored.
2. A changed payload hash, a replayed answer, and a double answer from two channels (first wins) are all rejected.
3. A late answer after timeout does not resurrect the request. It re-raises a new card, and only when parked.
4. A relay token calling `/budget`, `/permissions`, `ratify` or `rewind` is refused at the RPC layer.
5. A Red card under `console-only` can't be answered over the relay. Under `step-up`, a wrong or reused code fails.
6. A daemon restart with a card outstanding: after recovery the card is re-raised once, not twice.
7. An injection string inside card content, relayed to the channel, never becomes an instruction to any agent.
8. Redaction: a secret in a payload never appears in the outbound message.

### 1.3 Phases
- **R0:** spec 014 draft, with the `relay` scope and the parking amendment. Operator ratifies.
- **R1:** the scope, the park/re-raise logic, and a fake-channel test harness. No network yet.
- **R2:** one real channel, then live trial for a week of real builds.
- **R3:** the second channel, and `step-up` for Red if R2 shows a need.

## 2. Gap 3: API-key readiness

**Definition of ready:** a full mission (Builder, tests, cross-family audit, a Council, merge card) runs end to end with the `claude` and `agy` binaries absent from `PATH`, on API endpoints only, with the budget enforcing real dollars. Until that has passed once, the harness paths are a single point of failure.

### 2.1 Work items
1. **Fill the readiness matrix first.** For each (family, path) combination, record built, unit-tested or live-proven, with the date. Candidates: Claude on direct API, Bedrock and Vertex; Gemini on direct API (with the OAuth-client caveat in `first-real-run.md`) and Vertex; OpenAI-compatible gateway. I haven't verified the live status of any of them.
2. **Live conformance suite.** `#[ignore]` tests gated on environment keys, plus a nightly `live.yml` workflow with a hard spend cap. Per driver:
   - a tool loop that keeps reasoning signatures across turns,
   - cancellation mid-stream,
   - 429 backoff and failover,
   - usage accounting checked against the provider's reported usage,
   - the key never appearing in events.
3. **`delphi doctor --providers`.** One tiny live call per endpoint, reporting tool use, thinking signatures, caching, streaming and usage reporting. `probe.rs` is the starting point.
4. **Price must be present.** A blank or missing price on an API endpoint becomes a hard setup error. Today it silently disables budget reservation, which would make API mode unsafe to run unattended.
5. **Harness contract pinning.** Record stream fixtures per `claude` and `agy` CLI version and replay them in CI. `doctor` warns on an unrecognised CLI version. Fix the open harness findings (non-UTF-8 line hang, `agy` states, system prompt in argv).
6. **Failover from subscription to metered.** Falling from a harness endpoint to an API endpoint turns cost from $0 into real dollars. Do it at task boundaries only (not mid-span), and require an Amber card that states the change in cost. `exhaust.rs` already detects exhaustion.
7. **Third family.** Promote the OpenAI-compatible driver after items 1 to 4 pass. It is a hedge against one vendor changing under us and adds Council diversity.

### 2.2 Exit criteria
- The matrix is filled in and no row says "unknown".
- The `PATH`-scrubbed end-to-end mission passes, once on Claude-only, once on Claude plus Gemini.
- The nightly live job has run green seven nights in a row at its spend cap.

## 3. Smaller gaps

### 3.1 Skills: portable, and safe to ratify
- **Compatibility check (do first):** fetch the agentskills.io specification and diff it against our frontmatter. I haven't read it. Delphi's extra fields should be additive, so a plain standard skill loads in Delphi and a Delphi skill degrades cleanly elsewhere.
- **`delphi skills import <path|url>`:** the skill lands in `skills/custom/`, marked `external_untrusted`, with `authorized_agents` empty until the operator assigns it. A skill that ships scripts is Red, because scripts are host-executed. Ratification is required before any agent can load it.
- **`delphi skills export`:** strips the Delphi extensions.
- **Counter ratification fatigue.** Agent-proposed skills and lessons arrive through `/retro` as one batch. Mitigations: cap batch size, sort by risk, show the evidence (test-verified or not) and what changes if the operator says no, and record dwell time and accept rate. If someone accepts everything in seconds, warn once and say so.

### 3.2 Hooks: operator-owned, and unable to loosen enforcement
Claude Code and `agy` hooks can allow or deny. In Delphi, enforcement lives in the daemon, and a script that could return "allow" would be a bypass.
- **Observe hooks:** an event matches, a command runs, and the output is ignored. This covers notifications, metrics and logging. They run out of process, in operator-owned Zone-3 config.
- **Tighten-only gate hooks:** a hook may *deny* an action, never allow one. Deny-only gates can't weaken anything.
- **The relay is the first observe hook.** Building it this way means one mechanism serves both. If the relay lands first, extract the hook runner from it.

### 3.3 Remote and cloud execution
Don't add execution backends to the sandbox crate. The cheaper route gets most of the value:
- Run the whole of Delphi on the remote machine, under tmux or `ssh`, with the relay (phase R2) for approvals and headless `delphi run` (phase H1) for starting.
- Write that recipe up, then test it once on a real remote box.
- Revisit a true backend abstraction only if `delphi needs` records a need.

## 4. Headless mode, the prerequisite for gap 4 and for CI

**`delphi run`** is a non-interactive entry point that talks to the daemon as any client does:

```
delphi run --task "<text>" [--workspace W] [--project P]
           [--permissions default|accept-edits|mission-auto]
           [--on-card deny|park|relay|policy:<file>]
           [--budget <usd>] [--timeout <dur>] [--json|--jsonl]
```

- **Output:** `--jsonl` streams the already-redacted event stream. A final summary object carries outcome, commit sha, cost and tokens per model, wall time, approval counts and retries.
- **Exit codes:** 0 completed, 1 gates failed, 2 stopped for a card, 3 budget exceeded, 4 infrastructure.
- **Cards in headless mode:** `deny` is the default, which suits CI. `policy:<file>` points at pre-ratified allow rules for Amber, which is what the benchmark runner needs. Red is never auto-approved, so a headless run ends at "ready to merge" on a branch. Opening a PR is an outbound Red action, so standing authorisation for it would be a decision to ratify. I recommend not granting it yet.
- **Protocol stability:** `PROTOCOL_MAJOR` already exists. Add JSON Schemas for events and golden-file tests, and document the JSON-RPC surface. A separate SDK crate waits for a second consumer.

## 5. Phasing and order

| Phase | Work | Size | Depends on | Gate |
| :-- | :-- | :-- | :-- | :-- |
| **P0** | Readiness matrix (§2.1 item 1); agentskills.io diff (§3.1); freeze the evaluation plan | S | none | |
| **P1** | `delphi run`, event schemas, exit codes, the `policy:` card mode (§4) | M | P0 | |
| **P2** | API readiness items 2 to 5 (§2.1); the open harness findings | M | P0 | |
| **P3** | Benchmark runner and the pilot (evaluation plan) | M | P1, P2 for metered cost | operator supplies API keys and a spend cap |
| **P4** | Relay R0 to R2, in parallel with P3 | M | P1 | operator ratifies spec 014 and the parking amendment |
| **P5** | Full study; decide which roles to cut or keep | L | P3 | |
| **P6** | Skills import/export; observe hooks; remote recipe | M | P4 | |

I'd start with P0 and P1, because P3 and P4 both depend on `delphi run`.

## 6. Decisions needed from the operator
1. **Parking amendment:** timeout denies the request but parks the task, with a new card on resume (§1.1). Yes or no.
2. **Remote Red policy:** `console-only` (recommended) or `step-up`.
3. **First relay channel:** Slack Socket Mode or Telegram long-poll. Both are outbound-only. Telegram has a simpler trust surface; Slack matches where the work happens.
4. **API keys and a spend cap** for the nightly live job and the pilot, and whether CI may hold the secrets.
5. **Pre-registered thresholds** in the evaluation plan (§6 there).
6. **Headless PR authorisation:** my recommendation is no standing authorisation for now.

## 7. What this plan does not do
- It does not add computer or browser control, auto-curated user memory, or IDE integration. The comparison found them in other harnesses; the reasons for leaving them out (taint model, D-09 and D-11, terminal-first) still hold.
- It does not touch the Step 6 usage-gated items. These new items are outside that list; if you want them tracked the same way, `delphi needs add` can record them.
