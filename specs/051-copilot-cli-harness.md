---
id: spec-051-copilot-cli-harness
title: "051: GitHub Copilot CLI as a Fourth Harness, Sized for a Free Tier"
author: architect
timestamp: 2026-10-28T21:30:00Z
status: approved   # by the operator, 2026-10-28 ("this is approved. start on S0"); OD-3 answered the same day (§4)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # §2 is from the installed `copilot` 1.0.105 and one live `-p --output-format json` call; §6 is the S0 probe of both CLIs' quota on this machine
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, copilot, github, coverage, free-tier]
---

# Spec 051: GitHub Copilot CLI as a Fourth Harness

Extends [`specs/046`](046-harness-and-openai-coverage.md) (the disposition method and the manifest format of its §6), [`047`](047-codex-harness-and-coverage.md) (the closest sibling: a CLI harness with a JSONL stream and an approval question), [`020`](020-harness-tool-bridge.md) and [`021`](021-harness-cli-features.md). Delphi drives `claude` and `agy` as subscription "harness" seats and plans `codex` as a third; this adds the GitHub Copilot CLI (`copilot`) as a fourth, on the operator's GitHub login.

> The operator's request, 2026-10-28: add or extend a spec for Copilot support. The operator believed both the OpenAI and Copilot accounts to be on a free tier; §6 shows the Copilot login is an organization-granted Business seat and the Codex login a Free plan. Quota is not transparent to the end user on either, and other operators will be on other tiers, so the design discovers it (§1.2) and assumes no tier.

**S0 is built (§6): the manifest `crates/delphi-engine/coverage/copilot.cov`, its help snapshot, and the quota probes. S1 onward is not.** §2 is what the installed CLI says; §6 corrects it. The two facts that shape the design are in §1.

## 1. What is different about this harness

1. **One login, several model vendors.** The probe's `assistant.message` carried `model: "claude-sonnet-5.5"`, `providerId: "copilot"`; the CLI routes to Anthropic, OpenAI and Google models behind one GitHub login (`--model <id>`, or `auto`). So a Copilot seat's *family* is a property of the model it was asked for, not of the CLI. That is useful (a cross-family reviewer on one login, spec 031's loop) and a hazard (a seat that says "family = gpt" and is served by `auto` would break the cross-family rule). The seat must name its model and the span must record the model the stream reports, never the one asked for.
2. **Quota is a number each vendor reports differently, so Delphi reads it instead of assuming a tier** (§6 shows the units differ by vendor and plan). The first probe, kept for scale: The one-token probe (`Reply with exactly: ok`) cost **1 premium request** and 23,049 prompt tokens (21 tools, 10,523 tokens of tool schemas, 4,046 of tool instructions) for a 4-token answer. If requests are the unit, a span costs the same whether it is a one-line verdict or a long review, so Copilot should get **few, large, read-mostly spans**, not many small ones. Delphi counts what it spends itself, because the CLI only caps one session (`--max-ai-credits`, minimum 30; §2.4) and not a month, and reads each vendor's own meter where one exists (§6.1). A seat's routing weight comes from the headroom the meter reports now, so an operator who upgrades or downgrades a subscription changes behavior with no edit to Delphi.

Principles of spec 046 §1 and spec 047 §1 hold unchanged: a disposition for every item; narrow or read, never widen; no Delphi path changes the operator's global CLI state; drift is detected; Delphi's approvals stay Delphi's; the vendor's own cloud is not a seat. Plus:

3. **No trust by environment.** `COPILOT_ALLOW_ALL=true` "additionally trusts the working directory … which loads that directory's skills, plugins, MCP servers, and hooks, including hooks that run shell commands" (`copilot help environment`). Delphi never sets it, and strips it, `COPILOT_*` and `GH_TOKEN`/`GITHUB_TOKEN` from the child's environment before setting only what the span needs (§2.2). Those token variables "take precedence over previously stored credentials", so an inherited one would silently switch the seat to another account.

## 2. The surface (from `copilot` 1.0.105)

### 2.1 The way to drive it

`copilot -p <text> --output-format json [options]` runs one non-interactive prompt and exits. Verified live (2026-10-28, free-tier login, scratch directory, no trust prompt): the stream is JSONL, one object per line, each with `type`, `data`, `id`, `timestamp`, `parentId`, and `ephemeral: true` on transient ones. The events a parser needs:

| `type` | Carries | Use |
| :-- | :-- | :-- |
| `session.tools_updated` | `data.model` | the model actually in use |
| `assistant.message_delta` | `deltaContent` (ephemeral) | streaming text |
| `assistant.message` | `content`, `toolRequests[]`, `model`, `providerId` | final text of a turn and the tool calls it asked for |
| `session.usage_checkpoint` | `totalPremiumRequests`, `totalNanoAiu`, `accountingSnapshot.tokenDetails` (`input`, `cache_read`, `cache_write`, `output`), per-model `modelMetrics` | the usage meter |
| `model.call_final_result` | `result` (`success`), `isByok` | per-call outcome (S0 captures the failure values) |
| `result` (last line) | `sessionId`, `exitCode`, `usage.premiumRequests`, `codeChanges` | end of span |

Not probed: a tool-using turn's `toolRequests` and result events, permission events, the error and exhausted-quota shapes. `--usage-output-file <file>` writes the final usage as JSON and is a cross-check of the stream, as `-o` is for Codex.

### 2.2 Options and their dispositions

| Flag / feature | Neutral option (spec 046 §2) | Disposition |
| :-- | :-- | :-- |
| `-p/--prompt`, `--output-format json` | stream parse | **S1** (`parse_copilot_line`, beside the others). **The prompt goes on stdin with no `-p`** (verified §6.4): spec 020 keeps prompts out of `ps`; `-p -` sends a literal "-" |
| `-s/--silent`, `--stream`, `--plain-diff`, `--screen-reader`, `--banner`, `--mouse`, `--no-color`, `--bash-env` | | **n/a** (display) |
| `--model <id>`, `--auto-tier`, `COPILOT_MODEL`, `COPILOT_AUTO_TIER` | model | `--model` **S1**, always explicit (§1.1); `auto` and the Auto tiers **refused** for a seat that must be a known family; `COPILOT_*` env **stripped** |
| `--reasoning-effort none…max`, `--context default\|long_context` | `effort` | **S1** (`effort`); `long_context` **S1**, off by default (may cost more requests; S0 measures) |
| `--available-tools`, `--excluded-tools` | `tools_allow` / `tools_deny` | **S1**: the narrowing gate. Role with no tools: `--available-tools` listing nothing. **Probe finding:** a bare `--available-tools` with no values did **not** remove any tool (21 were still offered), so S0 must find the form that does (a list, an `=` form, or `--excluded-tools` of every name) before a "no tools" seat is believed |
| `--allow-tool`, `--deny-tool` (`shell(git:*)`, `write(path)`, `url(domain)`, `<mcp>(tool)`) | `tools_allow` / `tools_deny` | **S1** for deny (deny beats allow, even over `--allow-all-tools`); allow only for a ratified Builder in its worktree |
| `--allow-all-tools` (help: "required for non-interactive mode") | approvals | **Not needed, not passed.** §6.4: without it a headless run still reads inside its working directory and **denies** everything else with a clean error (it does not hang), so the default is already fail-closed. **Refused** |
| `--allow-all`, `--yolo`, `--allow-all-paths`, `--allow-all-urls`, `COPILOT_ALLOW_ALL` | | **refused**, always |
| `--allow-url`, `--deny-url`, `--add-dir`, `--disallow-temp-dir` | `add_dirs` | `--deny-url`/`--disallow-temp-dir` **S1** (always); `--add-dir` and `--allow-url` **S1** only in a ratified block (as spec 021) |
| `--assisted-approval` | | **refused**: a probabilistic judge answering approvals (spec 047 §5's `--approve-for-me`, again); it states it "is not a security boundary" |
| `--no-custom-instructions` | | **S1**: always. Delphi supplies the governance text; the checkout's `AGENTS.md` must not be a second voice |
| `--disable-builtin-mcps`, `--disable-mcp-server`, `--additional-mcp-config`, `--plugin-dir`, `--add-github-mcp-*`, `--enable-all-github-mcp-tools`, `--mcp-github-auth` | | `--disable-builtin-mcps` **S1**: always (the built-in `github-mcp-server` reaches GitHub's cloud). `--additional-mcp-config` **S2**: the candidate transport for Delphi's tool bridge (spec 020). The rest **refused** or **n/a** |
| `--no-auto-update`, `--disable-client-telemetry`, `--no-ask-user`, `--no-experimental` | | **S1**: always (a span must not change the binary, phone home on Delphi's behalf, or stall on a question nobody can answer) |
| `--secret-env-vars` | | **S1**: names of any variable Delphi puts in the child's environment |
| `--log-dir <dir>`, `-C <dir>` | | **S1**: the span's scratch directory (nothing under `~/.copilot/logs`); `-C` is the span's working directory |
| `--session-id <uuid>`, `--resume <id>`, `-n/--name` | `session` | **S1**: Delphi chooses the UUID; resume by id only. `--continue`, bare `--resume` (picker) and resume by name are **refused** (directory- or operator-dependent) |
| `--max-ai-credits <n>` (min 30; a soft cap checked after each response) | `max_budget` | **S1** as a backstop only; the real ceiling is Delphi's own request ledger (§1.2). Not a dollar figure, in line with "limits as token shares or counts" |
| `--usage-output-file <file>` | usage | **S1** as a cross-check of the stream |
| `--attachment` | vision | **S3**, off by default |
| `--agent <agent>`, `--mode plan\|autopilot`, `--plan`, `--autopilot`, `--max-autopilot-continues`, `--fleet` | | `--plan` **S3** (a read-only planning lane, if S0 shows it cannot write); `--autopilot` and `--fleet` **refused** (the CLI orchestrating its own sub-agents is spec 041's job, and uses requests invisibly) |
| `--enable-memory`, `--sync-session`, `--share`, `--share-gist`, `memories` | | **refused**: shared memory is Xenia's (spec 022); `--share-gist` publishes a transcript to GitHub |
| `--trust-folder` | | **S1** only if S0 finds a span in a scratch directory needs it (the probe did not) |
| `--connect`, `--host-environment`, `--host-compute-id`, `--host-ahp`, `app` | | **refused** (§1: the vendor's cloud and a remote session are not a seat) |
| `--acp` (Agent Client Protocol server) | approvals | **S2 probe**: the candidate for Delphi to answer approvals, the role `app-server` plays for Codex |
| `--experimental`, `--extension-sdk-path`, `--dynamic-retrieval`, `--prefer-version`, `--mouse` | | **refused** / **n/a** |
| `COPILOT_PROVIDER_*` (BYOK: OpenAI-, Azure- or Anthropic-compatible endpoint, or Ollama) | | **refused** as a seat route: it bypasses the account and the ledger, and Delphi already has `family = "openai"` / `"local"` for endpoints |
| `COPILOT_GITHUB_TOKEN` (fine-grained PAT with "Copilot Requests"; classic `ghp_` unsupported) | token mode | **S1**: the API-key analogue of spec 021 §4; in the child's environment only, redacted, no fallback to the stored login, never printed. Off unless configured |
| `COPILOT_HOME` | | **S1**: always a per-span scratch directory. §6.4: the login lives in the OS keychain, so a scratch home still authenticates, and session state (prompts, tool output) then stays out of `~/.copilot`. Where a machine has no keychain and the token sits in `~/.copilot/config.json`, a scratch home is signed out (unprobed): S2's doctor check reports it |

### 2.3 Other commands

| Command | Disposition |
| :-- | :-- |
| `version`, `help`, `completion`, `instruction`, `lsp`, `sandbox` (inspection) | **read-only / n/a**; `version` **S0**: the manifest records the version |
| `login` | **operator-only** (browser or device flow). There is no `login status`: the doctor check is S2 (a single minimal call is **not** free on a request quota, so prefer reading `~/.copilot/config.json` for the account name only; S0 decides) |
| `config`, `sessions`, `plugin`, `mcp`, `skill`, `init`, `workflow`, `update` | **operator-only** where they write (`copilot config set`, `plugin install`, `mcp add`, `skill add`, `init`, `update` change global state); read forms (`mcp list`, `sessions list`, `plugin list`, `config list`) **read-only, S2** |
| `memories` | **refused** (§2.2) |

### 2.4 Cost and limits

`copilot help billing` / `help limits`: usage is in AI credits, or **premium requests** on the legacy billing platform; the probe's accounting shows both (`totalNanoAiu` 5,766,050,000, `totalPremiumRequests` 1, a request `cost` of 1 for Sonnet 5.5). Models carry a cost multiplier shown in `/model`. Session limits are soft: "a response can exceed or exhaust the limit before the CLI can observe that it has done so". Delphi therefore:

- records `premiumRequests` and `nanoAiu` per span from the stream, and keeps a per-seat ledger for the vendor's window;
- refuses a span whose model's cost would take the ledger past the ceiling of OD-2;
- treats "quota exhausted" as a failover trigger (spec 035) once S0 has captured the **real** exhausted output, as for Claude and agy (operator memory: capture the real exhaustion text, never invent one).

## 3. Plan

| Slice | Delivers | Needs |
| :-- | :-- | :-- |
| **S0** (built; see §6) | `crates/delphi-engine/coverage/copilot.cov` from the real `--help` plus its snapshot (spec 046 §6); probes: (a) the form of `--available-tools` that removes every tool; (b) stdin for the prompt; (c) a tool-using turn's events and permission events; (d) whether `-p` with `--allow-all-tools` and a narrowed tool list can write when told to; (e) the model catalog (there is no `models` command: try `/model` via `--acp`, or the error text of an unknown `--model`); (f) the cost multiplier of each model on the free tier; (g) the exhausted-quota output; (h) whether `--log-dir` and `COPILOT_HOME` keep the span out of `~/.copilot`; (i) `--acp`'s approval messages | the operator's login (done) |
| **S1** | `Cli::Copilot` in `harness.rs`; `-p --output-format json` driver, read-only roles only; explicit model; request ledger and usage meter; token mode; tests against a stand-in CLI | S0 |
| **S2** | read-only introspection (`version`, `mcp list`, `sessions list`, account name); the approval bridge over `--acp` or `--additional-mcp-config` if S0 says it is stable; only then may a Copilot seat run a Builder | S0 |
| **S3** | `--plan` lane, vision | S1 |

Each slice is one pull request with the usual trailer and tracker row, and tests against a stand-in CLI. **Live proof is separate**: every Copilot seat is labelled *unproven* until `delphi endpoints test --probe` has run it on the installed version, and each probe spends quota, so S0 lists its probes and runs the cheapest first.

## 4. Open decisions (the operator's)

| # | Question | Recommendation |
| :-- | :-- | :-- |
| OD-1 | Is a Copilot seat a Builder only after the approval bridge is proven (§2.2, `--allow-all-tools`)? | Yes. Until then it is a reader, like an unprobed agy seat. |
| OD-2 | What may Delphi spend of each seat's quota? | **Answered by discovery, not by a figure**: a ceiling as a share of the headroom the vendor's meter reports (default 20 % of the current window), or a count where the vendor reports counts (§6). A Business seat's pool belongs to an organization: see OD-7. |
| OD-3 | Where does Copilot sit in the failover order (spec 035)? | **Decided (operator, 2026-10-28): nowhere fixed.** Other operators are on other tiers and the same operator can change plans at any time, so no provider is ranked last by design. Position comes from the quota probe (§6.2): remaining headroom, reset time and plan type, with a seat that reports nothing treated as unknown, not empty. |
| OD-4 | May a seat use `auto` model routing? | No: the family is not known (§1.1). Explicit model only. |
| OD-5 | Is token mode (`COPILOT_GITHUB_TOKEN`) wanted, or only the stored login? | Stored login first; token mode built but off. |
| OD-6 | Which login backs Codex (spec 047)? | **Answered by S0 (§6.1): a ChatGPT Free plan, and `codex exec` ran on it.** |
| OD-7 | The Copilot login is an employer-granted Business seat (organization-managed; the account reports enterprise usage telemetry). Is it acceptable to send Delphi's work through it? | **Answered (operator, 2026-10-28): acceptable to run, and noted.** Delphi still shows the plan type in `delphi doctor`, and another operator's organization policy is theirs to decide, so a seat on an organization plan is labelled as such. |

## 5. What this spec does not claim

- That any flag behaves as `--help` says beyond what the one probe showed (§2.1).
- What any plan allows. Codex's meter is a percentage of an undisclosed total; Copilot's absolute pool lags (§6.1). The multiplier of other models and the exhausted-quota behaviour of Copilot are unknown.
- That the same model behaves the same through Copilot as through its own vendor's CLI (a different system prompt, 21 tools, and its own routing). That is a question for the training experiments (spec 037).
- That `--acp` or `--additional-mcp-config` can carry Delphi's approvals. Only S0 and S2 can say.

## 6. S0 results (2026-10-28, `copilot` 1.0.105 and `codex-cli 0.165.0`, both logged in on this machine)

### 6.1 Quota: what each CLI will tell you without a model call

| | Codex (ChatGPT login) | Copilot (GitHub login) |
| :-- | :-- | :-- |
| **Plan seen** | `planType: "free"` (`account/read`) | `copilot_plan: "business"`, a seat granted by an organization (`/copilot_internal/user`): not free |
| **Read how** | `codex app-server --stdio`, JSON-RPC `initialize`, `initialized`, then `account/rateLimits/read`, `account/read`, `account/usage/read`. No model call. It also pushes `account/rateLimits/updated` as the meter moves | `gh api /copilot_internal/user`: an **undocumented** endpoint (the CLI's own backend), reached with the operator's `gh` token, a credential Delphi does not hold. No `copilot` command prints quota, and `/usage` works only interactively: in `-p` it was sent to the model as text, spent a request, and ran a docs-fetch tool |
| **Shape** | `primary: {usedPercent, windowDurationMins: 43200, resetsAt}` (a 30-day window, here to 2026-11-27), `secondary: null`, `credits: {hasCredits, unlimited, balance}`, `individualLimit`, `spendControlReached`, `rateLimitResetCredits` | `quota_snapshots.premium_interactions: {entitlement: 100000, remaining, credits_used, overage_permitted: true, percent_remaining}`; `chat` and `completions` are `unlimited: true`; `quota_reset_date: 2026-11-01` |
| **Resolution** | Integer percent of an unknown total: a 13,235-token `exec` span left it at 0 %. The absolute allowance is **not** disclosed | An absolute count, but see the lag below |
| **Exhaustion** | `rateLimitReachedType` names the cause: `rate_limit_reached`, `workspace_owner_credits_depleted`, `workspace_member_credits_depleted`, `workspace_owner_usage_limit_reached`, `workspace_member_usage_limit_reached`. S1 can map these to failover (spec 035) without captured text | not probed. Here `overage_permitted: true`, so exhaustion may bill the organization instead of stopping; another plan will differ |
| **Per span** | `turn.completed.usage`: tokens only | `session.usage_checkpoint`: `totalPremiumRequests`, `totalNanoAiu`, token details; `result.usage.premiumRequests` |

Findings that bear on the design:

1. **The two meters are different kinds.** Codex gives a *percentage of a window* with its length and reset time; Copilot gives an *absolute pool*. A neutral `QuotaProbe` returns `{used_fraction?, remaining?, unit?, resets_at?, plan, reached?}`, every field optional, and routing treats a missing field as unknown.
2. **Copilot's server counter lagged.** After two real calls (one a wasted `/usage` turn) `credits_used` was still 0 with a fresh `timestamp_utc`, while the client's stream said `totalPremiumRequests: 1` for each. Either the meter updates in batches or a Business seat is metered another way. Delphi's own ledger from the stream is therefore the live number, and the server read is a periodic reconciliation, never a gate on its own.
3. **Cost per call (an inference S1 must confirm against a known balance).** The one-word probe reported `totalNanoAiu` 5,766,050,000. If a credit is 10^9 nano-units, a trivial call cost about 5.8 credits, which fits `--max-ai-credits` having a minimum of 30 (five calls). Against a 100,000-credit pool that is negligible; against a small allowance the 23,049-token fixed prompt (21 tools) dominates, so the narrowing gate of §2.2 is also a cost control.
4. **There is no Copilot model-catalog command.** An unknown `--model` fails with `Model "…" from --model flag is not available.` Probe (e) stays open.
5. **Codex takes its prompt on stdin** (`-`): `echo … | codex exec --json --ephemeral --sandbox read-only --skip-git-repo-check --ignore-user-config -` returned the four events of spec 047 §5. Copilot's stdin form is open (probe b).

### 6.2 Quota design

- **A `QuotaProbe` per harness**, run by `delphi doctor` and the router, never inside a span. Codex: the app-server read above over stdio, not the operator's shared daemon. Copilot: Delphi's own stream ledger, with the `gh api` snapshot as an **optional**, operator-enabled reconciliation (S2, read-only, off by default, because it uses a second credential and an undocumented endpoint that may change).
- **Never a tier name.** Plan strings (`free`, `business`, …) are displayed, never branched on; behavior follows the numbers the meter returns. A plan the CLI does not report is `unknown`.
- **Failover and routing (OD-3)** read the probe: headroom, reset time, `reached`. A seat with no probe result stays eligible, ranked by the operator's configured order, not demoted.
- **Copilot's exhausted text is still uncaptured** (probe g). Until it is, exhaustion is detected by a non-zero `result.exitCode` plus the probe, and labelled unproven.

### 6.3 Manifest and probes

- `copilot.cov`: every flag and command of 1.0.105 has a disposition; `cargo test -p delphi-engine --lib coverage` passes and `delphi docs coverage` regenerates `docs/reference/harness-coverage.md`. New beyond §2.2: `--bash-env` refused (it sources the operator's `BASH_ENV` into the span's shell); `--sync-session` always passed `=off`; `--dynamic-retrieval` and `--extension-sdk-path` refused.
- Probes **done**: the quota reads (§6.1) and probes a-f, h, i (§6.4). A prompt that starts with `/` goes to the model as text, so a span's text must never begin with one.
- Probe **open**: (g) the exhausted-quota output. A 30-credit session cap was not reached by a 12-credit Opus run or a six-turn 7.5-credit one, and forcing it would burn the organization's pool for one error string, so it waits for a natural exhaustion (or a seat whose pool is small).

### 6.4 Probe results, round 2 (all on `claude-sonnet-5.5` unless named; one prompt each)

| Probe | Result |
| :-- | :-- |
| **(a) no-tools form** | `--available-tools view grep` offers exactly those two (prompt 5,010 tokens against 23,050 for the full set of 21). `--available-tools=` and `--available-tools ""` change nothing (21 tools): **an empty value is "no narrowing", the unsafe default**. `--available-tools none` (a name that matches nothing) yields **0 tools**, 3,609 prompt tokens, and a `session.info` event `Unknown tool name in the tool allowlist: "none"`. S1 passes an allowlist that is never empty, and asserts the stream's `tool_count` equals what it asked for |
| **(b) stdin** | The prompt on stdin with **no `-p`**, piped, runs headless: `{"content":"stdin-ok"}`. `-p -` sends a literal `-` |
| **(c) tool events** | A tool turn adds `assistant.message` with `toolRequests[{name, arguments}]`, then `tool.execution_start {toolName, arguments}` and `tool.execution_complete {success, result.content \| error.message, fileEdits[]}`. A multi-turn run (3 turns, or 6) reports one `result` and one `premiumRequests` |
| **(d) writes** | With no allow flags, `view` of a file in the working directory runs; `create` is **denied** (`Permission denied because no interactive user response was available…`), the model then reports it could not write and does not route around it; `view /etc/hosts` is **denied** the same way (path verification, no flag needed). With `--allow-tool 'write(<absolute path>)'`, that one file was created and a second `create` in the same turn was denied. So a Builder's authority can be expressed as an exact pre-approved set, everything else failing closed |
| **(e) catalog and (f) cost** | `--acp`'s `session/new` returns `models.availableModels[]` with `modelId`, `name`, and `_meta {copilotUsage: "1x", copilotEnablement, copilotPriceCategory}`. 23 models here from five vendors: Claude (sonnet 5.5/5, fable 5.1/5, opus 5.5/5/4.8, opus-4.8-fast 30x, haiku 5.5, haiku 4.5 0.33x), GPT (6.1/6/5.6 sol, luna, terra, astra, 5.3-codex), Gemini (4-argon 1x, 3.8-flash 14x), kimi-k3, mai-code-1.1-flash; most are 1x, the Opus/Fable family 15x. `--model <id>` switches vendor (`gpt-6-luna` answered, `model: gpt-6-luna`). The catalog is per account, so S2 reads it per run and a seat names a `modelId` that must be in it |
| **accounting** | Two units. `premiumRequests` is the multiplier times prompts, fractional (haiku-4.5: 0.33; opus-5.5: 15 for one prompt). `totalNanoAiu` is token-priced: one trivial span on gpt-6-luna 3.7e7, haiku-4.5 2.3e8, sonnet-5.5 about 1e9 (narrow tools) to 5.8e9 (21 tools, cold cache), opus-5.5 7.5e9 to 1.2e10. The account is `token_based_billing: true`, so **the ledger keeps nano-AIU and treats premium requests as legacy bookkeeping**. Tool narrowing cut sonnet's cost about 5x |
| **(h) isolation** | `--log-dir` moves the log files. It does **not** move session state: 10 spans added 10 directories (prompts, tool output) under `~/.copilot/session-state`. `COPILOT_HOME=<scratch>` does: the scratch home authenticated (the login is in the keychain), wrote its own `session-state`, `session-store.db`, `config.json`, `logs`, and left `~/.copilot` unchanged |
| **(i) approvals over `--acp`** | `copilot --acp` speaks Agent Client Protocol on stdio: `initialize` (agent capabilities, `authMethods`), `session/new {cwd, mcpServers}` (returns models, modes, `configOptions`), `session/prompt {sessionId, prompt[]}`. A write the policy would ask about arrives as a request **to the client**, `session/request_permission {toolCall{title, kind: "edit", rawInput incl. a diff, locations}, options[allow_once, allow_always, reject…]}`; answering `reject` produced `tool_call_update {status: "failed", rawOutput: {code: "rejected"}}` and a normal `stopReason: "end_turn"`. **This is the approval bridge spec 047 §1.1 asks for**, and it is a documented open protocol. The final response carries `usage {inputTokens, outputTokens, cachedReadTokens, cachedWriteTokens}`; `usage_update {used, size}` streams the context fill. Not yet probed: `allow_once` end to end, `allow_always`'s persistence (it must not persist: refuse that option), mid-turn cancel, MCP servers passed in `session/new` for the tool bridge |

Consequences for the plan (§3): S1 can be a stdin, no-`-p` JSONL driver for read-only roles, with `COPILOT_HOME` scratch, `--available-tools` allowlist, and no allow flags. **A Builder is no longer blocked on an unknown**: S2's driver is an ACP client that answers `session/request_permission` from Delphi's approval card (never `allow_always`), with `--allow-tool 'write(<path>)'` available as a pre-approved floor. Both stay *unproven* until `delphi endpoints test --probe` runs them.
