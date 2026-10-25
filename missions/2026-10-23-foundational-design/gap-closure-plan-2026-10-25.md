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

## Build status (2026-10-25, branch `worktree-gap-plan`)

| Item | State | Where |
| :-- | :-- | :-- |
| Headless `delphi run` (P1) | **Built and tested**, including `--on-card deny / policy:<file> / relay`, the JSON summary and exit codes 0 to 5 | [`specs/016`](../../specs/016-headless-run.md) |
| Relay scope, parking, the email channel (R0 to R2) | **Built and tested** against fake SMTP and IMAP servers and a real daemon. **Not yet run against a real mailbox.** TOTP step-up and chat channels are not built | [`specs/014`](../../specs/014-relay-and-away-mode.md) |
| Provisional decisions (O0, O1) | **Built and tested**: the ledger, gate 6, the review wait, `/decisions`, `/merge`, `delphi decisions`, the rule-based approver. Not built: the Strategist as a model-backed approver, the optional second seat, caps, rollback after a rejection, review by email. The charter amendments are drafted and **not applied** | [`specs/015`](../../specs/015-provisional-decisions.md) |
| Readiness matrix and the agentskills.io audit (P0) | **Done** (documents). The live conformance suite, harness contract pinning and the `PATH`-scrubbed mission are **not built** and need keys | [`provider-readiness`](provider-readiness-2026-10-25.md) |
| Skills import/export, observe hooks, remote recipe (P6) | Not started | §3 |
| Benchmark runner and pilot (P3) | Not started | [`evaluation-plan`](evaluation-plan-2026-10-25.md) |

**Decisions taken 2026-10-25 (D-30):** specs 014 to 016 and the charter amendments approved and applied; the Strategist is the approver; the second seat is optional with `park` as the default for unseconded decisions; email first, on a dedicated Gmail account; no standing PR authorisation for headless runs; evaluation thresholds frozen. A live Gmail trial is a future project task in the roadmap.

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
| The roadmap review's list of open sweep findings is mostly stale. Checked against the code on 2026-10-25: blank prices, the harness reader and `agy` states were fixed in 2e09bc9. Still open: the system prompt in argv; `install.sh` skipping attestation when `gh` is absent. | `git show 2e09bc9`, `engine.rs`, `route.rs` | Phase 2 shrinks: it is the live suite, contract pinning and the `PATH`-scrubbed mission, not bug fixing. |

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

**Channel adapters are thin.** The relay core owns tokens, payload binding, policy, rate limits and redaction. An adapter only moves bytes: `send(card message)` and `receive() -> (claimed sender, authentication evidence, text)`. Everything above is enforced once, in the core, so adding a channel can't weaken it. The first adapter is **email** (§1.2), because every operator has it. Chat adapters (Slack Socket Mode, Telegram long-poll) follow the same interface.

**Tier policy for remote answers** (`[away]` config, operator-ratified, default in bold):
- Amber: answerable remotely.
- Red: **console only**, or `step-up`. With `step-up`, the card shows a one-time 4-character code that the operator has to type back, so a blind tap can't approve a Red action. Red is never covered by "allow similar" and is never auto-approved. Both are unchanged.

**Parking, so an absent operator doesn't kill the task.** This is the one invariant to amend, and it needs a ratified decision. Today a timeout denies and the agent gets a structured denial. Proposal:
- The timeout still denies that request. The stale approval id stays dead, and a late answer still can't resurrect it.
- In away mode the *task* is then parked, not failed. It stops spending (no model calls), records `task.parked {reason: approval_timeout}`, and when the operator answers later, a **new** card is raised with a new `approval_id` and a new code.
- Parking is bounded by `[away] park_max` (default 24 h). Past it, the task fails as today.

**Notifications.** The relay posts: card raised, merge ready, task failed, budget at 80%, mission done. Messages are rate-limited and pass through the existing redactor. `detail = "summary"` is the default, so card content stays out of the third-party service unless the operator opts into `full`.

**Starting work from the phone is out of scope.** Command ingress from a chat channel is prompt injection waiting to happen. Phase 1 is approvals and notifications only. If it is ever added, it comes with the allowlist, a fixed command grammar and a confirmation card.

### 1.2 The email channel

Email is the platform-agnostic option: no app to install, no vendor API, and it works through any provider that offers SMTP and IMAP (or an equivalent). It is also the **weakest authenticated channel** here, because the `From` header is trivially forged and a compromised mailbox compromises the channel. The design assumes that and puts the secret elsewhere.

**Transport (outbound-only, no inbound port):**
- Cards go out by SMTP submission (or a configured send command) from a dedicated Delphi mailbox to the operator's address.
- Replies arrive in a **dedicated inbox the relay polls over IMAP**. This is a separate address from the operator's own mailbox, so the relay never reads the operator's mail.
- Credentials use the existing sources (`keychain`, `env`, `command`, or `oauth2` where the provider requires it). Some providers restrict password logins; that is an adapter detail and unverified here.

**Message format:**
- Plain text only. No HTML, no remote images, no tracking links.
- Subject `[delphi:<approval_id>] <one-line summary>`; the body shows the card (summary by default, full with `detail = "full"`), the payload hash prefix, and the expiry.
- Two `mailto:` links, **Approve** and **Deny**, pre-fill a reply to the relay inbox with the verb and a single-use token. One tap and send; no web endpoint is needed.

**How a reply is authenticated (all must hold):**
1. It was delivered to the relay inbox and its `In-Reply-To` or subject matches an outstanding card.
2. The sender address is on the operator allowlist **and** the receiving provider's own `Authentication-Results` header shows SPF, DKIM or DMARC passing for that sender. Only the relay inbox provider's header is trusted; headers inside the message body are ignored. If the provider adds no such header, the channel refuses Amber answers and says so in `doctor`.
3. The reply carries the **single-use token** issued with that card: 128 bits of randomness, bound to `approval_id` and the payload hash prefix, expiring with the card. The token is the secret, not the sender address.
4. The first line of the reply, before any quoted text, is exactly the verb and token. Quoted earlier messages, forwarded copies and signatures are ignored.
5. Messages with `Auto-Submitted` or `Precedence: bulk` headers (out-of-office, mail loops) are dropped.

**Mailbox providers.** The adapter speaks plain IMAP and SMTP with a `password` (app password) or `oauth2` credential, so it is not tied to any provider. These notes come from secondary sources and some were not confirmed; check each provider's current terms before relying on them. Volume is tiny (dozens of messages a day), so free-tier limits don't matter. What matters is whether the provider still offers IMAP and SMTP on a free plan, and how it authenticates.
| Option | Free? | Notes |
| :-- | :-- | :-- |
| **A dedicated free Gmail account, with an app password** | Yes | **Recommended starting point.** Personal accounts with 2-Step Verification reportedly still accept app passwords for IMAP and SMTP. Use a new account for Delphi alone, never your own mailbox. App passwords don't expire, which sidesteps the OAuth issue below. Google could tighten this, so keep the adapter generic. |
| **Google Workspace, OAuth with an Internal app** | Paid account | Workspace is reported to have ended password logins in 2025, so OAuth is required there. An *Internal* app avoids the 7-day refresh-token expiry and Google's verification review. Needs an org admin. |
| Outlook.com, iCloud Mail | Yes | Both have free accounts with IMAP and SMTP. Outlook.com is understood to require OAuth rather than passwords, and iCloud uses app-specific passwords. I did not verify either. |
| Fastmail, Proton | Not free | Fastmail is understood to be trial-only; Proton's IMAP goes through its Bridge app, and I could not confirm whether the free tier includes it. Treat as paid options. |
| Zoho Mail | Free plan lacks IMAP and SMTP | Reported in a 2026 article and a vendor community thread. Skip. |
| Transactional senders (Resend, Brevo, SMTP2GO, Mailgun) | Yes, small caps | They can send, but none gives the IMAP inbox the relay needs, and their inbound features use webhooks, which would need an open inbound port. Useful only if you want sending separated from receiving. SendGrid's free plan was retired in 2025. |
| Your own domain or a cheap host | Cheap | Any host offering a mailbox with IMAP works, and gives full control. |

**Why not reuse the Google connector?** Reusing its sign-in plumbing is fine, but reusing the Gmail connector itself is not.
- The Gmail connector's tools feed message bodies into agent context as untrusted content. The relay must parse mail *outside* any agent, which is the separation §1.1 requires.
- The connector has never run against Google. Its server URLs are marked "reported" and unconfirmed (spec 010 §7.4).
- Reading IMAP over OAuth needs a broader Gmail scope. In an OAuth client left in Testing mode, refresh tokens are reported to expire after 7 days, and Gmail's scopes are restricted, so Google verification would be needed to lift that. An app password has neither problem.
- What can be shared: `oauth2.rs` and the Google credential source, if a Workspace operator wants OAuth. That would be a separate client from the connector's, so revoking one doesn't break the other.

**Tier policy for email:**
- **Amber:** answerable by email.
- **Red:** not by email by default. With `step-up = "totp"`, a Red answer also needs a six-digit TOTP code from an authenticator the operator enrolled at setup. The code is independent of the mailbox, so a stolen mailbox alone can't approve a Red action. Merge approval and the high-risk decisions in the optimistic workflow use the same rule.

**Email-specific risks and handling:**
| Risk | Handling |
| :-- | :-- |
| Forged `From` | The token is the secret; the provider's authentication result is an extra check, not the only one. |
| Forwarded or leaked card email | Tokens are single-use, bound to the payload hash and expire; Red still needs TOTP. |
| Mailbox compromise | Amber-only by default. This is the channel's honest ceiling; `doctor` states it. |
| Content leaking to the mail provider | `detail = "summary"` by default; secrets go through the redactor; TLS required, no end-to-end encryption. Say so in the setup text. |
| Injection through inbound mail | Only the verb and token are parsed. Body text is never passed to an agent and nothing in it is treated as a command. |
| Mail flood or loops | Cap outstanding cards and messages per hour; ignore auto-generated mail. |
| Delivery delay | Email can lag minutes. An email card carries a longer timeout, and parking (§1.1) covers the rest. |

**Email tests:**
1. A forged `From` with no token is ignored. A correct token with a failing or missing authentication result is refused.
2. A wrong, reused or expired token fails; a token for another card fails.
3. A forwarded copy from a non-allowlisted address fails.
4. A reply whose quoted section contains an old token does not count; only the first line is read.
5. An out-of-office auto-reply quoting the card does nothing.
6. An HTML-only reply or one with attachments is handled safely and counts only if the first-line verb and token parse.
7. The same message delivered twice (IMAP resync) is processed once, by `Message-ID`.
8. Red by email without TOTP is refused; with a wrong TOTP code, refused.
9. Mail from a non-operator tries `/budget`-style text; nothing happens and it is logged.
10. A secret in a payload is absent from the outgoing message.

### 1.3 Tests
Written before the code, using a fake channel adapter (the email tests above run against a fake IMAP and SMTP server):
1. A forged sender, a non-allowlisted user and a shared-channel message are all ignored.
2. A changed payload hash, a replayed answer, and a double answer from two channels (first wins) are all rejected.
3. A late answer after timeout does not resurrect the request. It re-raises a new card, and only when parked.
4. A relay token calling `/budget`, `/permissions`, `ratify` or `rewind` is refused at the RPC layer.
5. A Red card under `console-only` can't be answered over the relay. Under `step-up`, a wrong or reused code fails.
6. A daemon restart with a card outstanding: after recovery the card is re-raised once, not twice.
7. An injection string inside card content, relayed to the channel, never becomes an instruction to any agent.
8. Redaction: a secret in a payload never appears in the outbound message.

### 1.4 Phases
- **R0:** spec 014 draft, with the `relay` scope, the channel-adapter interface and the parking amendment. Operator ratifies.
- **R1:** the scope, the park/re-raise logic, and a fake-channel test harness. No network yet.
- **R2:** the email adapter (Amber only), a fake IMAP and SMTP server for tests, then a week's live trial on real builds.
- **R3:** TOTP step-up for Red and for the optimistic workflow's high-risk reviews; then a chat adapter (Slack or Telegram) if email proves too slow.

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
4. **Price must be present.** *Already done in the code* (checked 2026-10-25): setup needs an explicit "treat as free", the engine refuses an unpriced model, routing skips it, and `doctor` flags it. The earlier roadmap review listed this as open and this plan repeated it; both were stale. Nothing to build; keep it as a stated criterion.
5. **Harness contract pinning.** Record stream fixtures per `claude` and `agy` CLI version and replay them in CI. `doctor` warns on an unrecognised CLI version. Most of the harness sweep findings were fixed in 2e09bc9; the one still open is the system prompt travelling in argv (low severity).
6. **Failover from subscription to metered.** Falling from a harness endpoint to an API endpoint turns cost from $0 into real dollars. Do it at task boundaries only (not mid-span), and require an Amber card that states the change in cost. `exhaust.rs` already detects exhaustion.
7. **Third family.** Promote the OpenAI-compatible driver after items 1 to 4 pass. It is a hedge against one vendor changing under us and adds Council diversity.

### 2.2 Exit criteria
- The matrix is filled in and no row says "unknown".
- The `PATH`-scrubbed end-to-end mission passes, once on Claude-only, once on Claude plus Gemini.
- The nightly live job has run green seven nights in a row at its spend cap.

## 3. Smaller gaps

### 3.1 Skills: portable, and safe to ratify
- **Compatibility check (done 2026-10-25):** I read the agentskills.io specification and audited all 43 built-in skills against its rules. Every skill passes the core rules: `name` is lowercase letters, digits and single hyphens, at most 64 characters and equal to its directory name; `description` is present and under 1,024 characters; each file is under 155 lines (the guidance is 500). The differences are two:
  1. Delphi adds four top-level fields, `authorized_agents`, `input_schema`, `output_schema` and `required_permissions`. The standard offers only `metadata` for extensions, and it is a map of string keys to string values, so these structured fields cannot move into it as they are. A strict validator may reject unknown top-level fields; I did not run the reference validator (`skills-ref`) to find out.
  2. Skills with templates keep them in `templates/`, where the standard suggests `assets/`. Cosmetic.

  So a plain standard skill loads in Delphi (the extra fields are Delphi's to require on import, see below), and **export** has to choose: drop the four fields, or encode them as JSON strings under `delphi.*` keys in `metadata`. I recommend the second, so a round trip loses nothing. The standard's experimental `allowed-tools` has no equivalent here (Delphi's tools are not Claude Code's), so import ignores it and says so.
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
| **P4b** | Optimistic workflow O0 to O2 ([exploration](optimistic-workflow-exploration-2026-10-25.md)): ledger, gate 6, `delphi decisions`, the approver | M–L | P1, P4 (relay digest) | operator ratifies the charter amendments |
| **P5** | Full study; decide which roles to cut or keep | L | P3 | |
| **P6** | Skills import/export; observe hooks; remote recipe | M | P4 | |

I'd start with P0 and P1, because P3 and P4 both depend on `delphi run`.

## 6. Decisions needed from the operator
1. **Parking amendment:** timeout denies the request but parks the task, with a new card on resume (§1.1). Yes or no.
2. **Remote Red policy:** `console-only` (recommended) or `step-up`.
3. **First relay channel:** I now recommend **email**, because it is platform-agnostic and every operator has it. It also forces the trust design to be right: if the model is safe for the weakest channel, it is safe for chat. Cost: Amber-only until TOTP step-up exists, and slower delivery. The starting recommendation is a dedicated free Gmail account with an app password; see "Mailbox providers" in §1.2.
4. **API keys and a spend cap** for the nightly live job and the pilot, and whether CI may hold the secrets.
5. **Pre-registered thresholds** in the evaluation plan (§6 there).
6. **Headless PR authorisation:** my recommendation is no standing authorisation for now.
7. **Optimistic workflow:** the "provisional decision is not an approval" framing is **accepted (2026-10-25)**. Its charter edits still need ratifying. The Strategist is proposed as approver and the second seat is optional; remaining questions are in the exploration's §8.

## 7. What this plan does not do
- It does not add computer or browser control, auto-curated user memory, or IDE integration. The comparison found them in other harnesses; the reasons for leaving them out (taint model, D-09 and D-11, terminal-first) still hold.
- It does not touch the Step 6 usage-gated items. These new items are outside that list; if you want them tracked the same way, `delphi needs add` can record them.
