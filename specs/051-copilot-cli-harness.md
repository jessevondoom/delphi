---
id: spec-051-copilot-cli-harness
title: "051: GitHub Copilot CLI as a Fourth Harness, Sized for a Free Tier"
author: architect
timestamp: 2026-10-28T21:30:00Z
status: draft   # awaiting the operator; nothing here is permission to build (AGENTS.md: No Spec, No Build)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # §2 is from the installed `copilot` 1.0.105 (`--help`, `help <topic>`) and ONE live `-p --output-format json` call on the operator's free-tier login; the free tier's quota is not known (§4)
source_mission: 2026-10-23-foundational-design
tags: [spec, harness, copilot, github, coverage, free-tier]
---

# Spec 051: GitHub Copilot CLI as a Fourth Harness

Extends [`specs/046`](046-harness-and-openai-coverage.md) (the disposition method and the manifest format of its §6), [`047`](047-codex-harness-and-coverage.md) (the closest sibling: a CLI harness with a JSONL stream and an approval question), [`020`](020-harness-tool-bridge.md) and [`021`](021-harness-cli-features.md). Delphi drives `claude` and `agy` as subscription "harness" seats and plans `codex` as a third; this adds the GitHub Copilot CLI (`copilot`) as a fourth, on the operator's GitHub login.

> The operator's request, 2026-10-28: add or extend a spec for Copilot support. The Copilot CLI is installed; the operator's OpenAI and Copilot accounts are both on the free tier.

**Nothing here is built.** §2 is what the installed CLI says and one live call showed. The two facts that shape the design are in §1.

## 1. What is different about this harness

1. **One login, several model vendors.** The probe's `assistant.message` carried `model: "claude-sonnet-5.5"`, `providerId: "copilot"`; the CLI routes to Anthropic, OpenAI and Google models behind one GitHub login (`--model <id>`, or `auto`). So a Copilot seat's *family* is a property of the model it was asked for, not of the CLI. That is useful (a cross-family reviewer on one login, spec 031's loop) and a hazard (a seat that says "family = gpt" and is served by `auto` would break the cross-family rule). The seat must name its model and the span must record the model the stream reports, never the one asked for.
2. **A free tier is a count, not a rate.** The one-token probe (`Reply with exactly: ok`) cost **1 premium request** and 23,049 prompt tokens (21 tools, 10,523 tokens of tool schemas, 4,046 of tool instructions) for a 4-token answer. If requests are the unit, a span costs the same whether it is a one-line verdict or a long review, so Copilot should get **few, large, read-mostly spans**, not many small ones. Delphi counts requests itself, because the CLI only caps one session (`--max-ai-credits`, minimum 30; §2.4) and not a month. The quota itself is unknown (OD-2).

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
| `-p/--prompt`, `--output-format json` | stream parse | **S1** (`parse_copilot_line`, beside the others). Whether the prompt can come on stdin instead of in `-p` is an S0 probe: spec 020 keeps prompts out of `ps`, and `-p <text>` puts them there |
| `-s/--silent`, `--stream`, `--plain-diff`, `--screen-reader`, `--banner`, `--mouse`, `--no-color`, `--bash-env` | | **n/a** (display) |
| `--model <id>`, `--auto-tier`, `COPILOT_MODEL`, `COPILOT_AUTO_TIER` | model | `--model` **S1**, always explicit (§1.1); `auto` and the Auto tiers **refused** for a seat that must be a known family; `COPILOT_*` env **stripped** |
| `--reasoning-effort none…max`, `--context default\|long_context` | `effort` | **S1** (`effort`); `long_context` **S1**, off by default (may cost more requests; S0 measures) |
| `--available-tools`, `--excluded-tools` | `tools_allow` / `tools_deny` | **S1**: the narrowing gate. Role with no tools: `--available-tools` listing nothing. **Probe finding:** a bare `--available-tools` with no values did **not** remove any tool (21 were still offered), so S0 must find the form that does (a list, an `=` form, or `--excluded-tools` of every name) before a "no tools" seat is believed |
| `--allow-tool`, `--deny-tool` (`shell(git:*)`, `write(path)`, `url(domain)`, `<mcp>(tool)`) | `tools_allow` / `tools_deny` | **S1** for deny (deny beats allow, even over `--allow-all-tools`); allow only for a ratified Builder in its worktree |
| `--allow-all-tools` ("required for non-interactive mode") | approvals | **S1 only paired with `--available-tools` narrowed to read tools** (so the flag approves nothing that was not already allowed); **refused** on its own. This is the §1 spec-047 tension: a headless `-p` seat has no one to ask, so *what it can see* is the gate, not what it is asked about. Hence read-only until S2 |
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
| `COPILOT_HOME` | | **S1**: the span may point it at a scratch directory **only** in token mode (a scratch home has no login). Otherwise the operator's home is used read-mostly, which is the reason for `--log-dir` and the `--no-*` flags |

### 2.3 Other commands

| Command | Disposition |
| :-- | :-- |
| `version`, `help`, `completion`, `instruction`, `lsp`, `sandbox` (inspection) | **read-only / n/a**; `version` **S0**: the manifest records the version |
| `login` | **operator-only** (browser or device flow). There is no `login status`: the doctor check is S2 (a single minimal call is **not** free on a request quota, so prefer reading `~/.copilot/config.json` for the account name only; S0 decides) |
| `config`, `sessions`, `plugin`, `mcp`, `skill`, `init`, `workflow`, `update` | **operator-only** where they write (`copilot config set`, `plugin install`, `mcp add`, `skill add`, `init`, `update` change global state); read forms (`mcp list`, `sessions list`, `plugin list`, `config list`) **read-only, S2** |
| `memories` | **refused** (§2.2) |

### 2.4 Cost and limits

`copilot help billing` / `help limits`: usage is in AI credits, or **premium requests** on the legacy billing platform; the probe's accounting shows both (`totalNanoAiu` 5,766,050,000, `totalPremiumRequests` 1, a request `cost` of 1 for Sonnet 5.5). Models carry a cost multiplier shown in `/model`. Session limits are soft: "a response can exceed or exhaust the limit before the CLI can observe that it has done so". Delphi therefore:

- records `premiumRequests` and `nanoAiu` per span from the stream, and keeps a per-seat monthly request ledger;
- refuses a span whose model's multiplier would take the ledger past the operator's ceiling (OD-2);
- treats "quota exhausted" as a failover trigger (spec 035) once S0 has captured the **real** exhausted output, as for Claude and agy (operator memory: capture the real exhaustion text, never invent one).

## 3. Plan

| Slice | Delivers | Needs |
| :-- | :-- | :-- |
| **S0** | `crates/delphi-engine/coverage/copilot.cov` from the real `--help` plus its snapshot (spec 046 §6); probes: (a) the form of `--available-tools` that removes every tool; (b) stdin for the prompt; (c) a tool-using turn's events and permission events; (d) whether `-p` with `--allow-all-tools` and a narrowed tool list can write when told to; (e) the model catalog (there is no `models` command: try `/model` via `--acp`, or the error text of an unknown `--model`); (f) the cost multiplier of each model on the free tier; (g) the exhausted-quota output; (h) whether `--log-dir` and `COPILOT_HOME` keep the span out of `~/.copilot`; (i) `--acp`'s approval messages | the operator's login (done) |
| **S1** | `Cli::Copilot` in `harness.rs`; `-p --output-format json` driver, read-only roles only; explicit model; request ledger and usage meter; token mode; tests against a stand-in CLI | S0 |
| **S2** | read-only introspection (`version`, `mcp list`, `sessions list`, account name); the approval bridge over `--acp` or `--additional-mcp-config` if S0 says it is stable; only then may a Copilot seat run a Builder | S0 |
| **S3** | `--plan` lane, vision | S1 |

Each slice is one pull request with the usual trailer and tracker row, and tests against a stand-in CLI. **Live proof is separate**: every Copilot seat is labelled *unproven* until `delphi endpoints test --probe` has run it on the installed version, and each probe spends requests from a free quota, so S0 lists its probes and runs the cheapest first.

## 4. Open decisions (the operator's)

| # | Question | Recommendation |
| :-- | :-- | :-- |
| OD-1 | Is a Copilot seat a Builder only after the approval bridge is proven (§2.2, `--allow-all-tools`)? | Yes. Until then it is a reader, like an unprobed agy seat. |
| OD-2 | What is the free tier's monthly allowance, and how much of it may Delphi spend? | Please say what the Copilot account page shows (premium requests or AI credits per month). Until known, a ceiling of 20 % of it for Delphi, as a count in `[harness.copilot]`. |
| OD-3 | Where does Copilot sit in the failover order (spec 035) and role routing? | Last among subscription seats, and first choice only for the reviewer role on a different family from the builder, where one big read-only span is cheap in requests. |
| OD-4 | May a seat use `auto` model routing? | No: the family is not known (§1.1). Explicit model only. |
| OD-5 | Is token mode (`COPILOT_GITHUB_TOKEN`) wanted, or only the stored login? | Stored login first; token mode built but off. |
| OD-6 | OpenAI free tier: is Codex (spec 047) on a ChatGPT Free login or an API key, and does the free allowance cover `codex exec`? | Answer before spec 047's S1; it is not this spec's to decide, but both free quotas share the "count requests, few large spans" design above. |

## 5. What this spec does not claim

- That any flag behaves as `--help` says beyond what the one probe showed (§2.1).
- What the free tier allows. The probe spent one premium request; the allowance, the multiplier of other models, and the exhausted-quota behaviour are unknown.
- That the same model behaves the same through Copilot as through its own vendor's CLI (a different system prompt, 21 tools, and its own routing). That is a question for the training experiments (spec 037).
- That `--acp` or `--additional-mcp-config` can carry Delphi's approvals. Only S0 and S2 can say.
