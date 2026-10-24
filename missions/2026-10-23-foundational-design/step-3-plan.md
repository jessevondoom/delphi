---
id: step-3-plan
title: "Step 3 plan: builder, remaining access paths, and the connector prerequisites"
author: strategist
timestamp: 2026-10-24T17:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-3, plan, milestones]
---

# Step 3 plan

Scope from the roadmap: Gemini (direct API, Vertex), compat gateways, `oauth2` login, endpoint probing; then, on the new sandbox, subscription CLIs (egress proxy, approval bridge) and the Builder in sandboxed worktrees (`.git` write-denied, host-exec badge), the unified retry ladder and the five merge gates. Spec 010 (connectors) depends on three pieces of this step and is built right after it (Step 3b).

Ordering principle: everything that needs no OS sandbox and can be tested against mocks comes first, so it can ship and be verified without real credentials. The sandbox and worktree work, which is the riskiest, follows with a full red-team sweep before the harness paths use it.

**Reordered 2026-10-24: the operator has subscription accounts (Claude, Gemini) and no API keys.** The only path to real testing without keys is harness mode (`specs/009` §7): Delphi launches the operator's own signed-in `claude` or `gemini` CLI. Delphi's `oauth2` login does not help here: it is for corporate identity providers and connectors, and subscription logins stay with the official CLIs (`specs/009` §7.1, §4.2). Both CLIs are installed on the development machine. Harness mode requires the sandbox, so the sandbox moves first.

| # | Milestone | Specs | Needs from the operator |
| :-- | :--- | :--- | :--- |
| M1 | **Data-driven tool registry** (tools as registered descriptors; needed by the sandbox tools and by connectors) | 001, 010 §2 | nothing |
| M2 | **Sandbox**: `sandbox-exec` (macOS) and `bwrap` (Linux), deny-default filesystem, network off, `.git` write-denied | 008 §4 | nothing |
| M3 | **Harness mode, non-builder roles first**: launch `claude -p` / `gemini` under the sandbox with CLI tools disabled and structured output only; ephemeral state dir; daemon CONNECT proxy to the provider hosts only; allowlisted environment; subscription token metering and kill-on-breach; taint clamp. This is the first real end-to-end run | 009 §7 | the `claude` and `gemini` CLIs signed in (already installed) |
| M4 | Red-team sweep of M1 to M3 | | |
| M5 | **Worktrees and the Builder**: worktrees outside the repo, host-exec badge, unified retry ladder, five merge gates, `action.intent`/`action.result`; harness Builder via worktree diff | 003 §3, 008 §4, 009 §7.7 | a scratch git repo |
| M6 | **`oauth2`** (device code, client credentials, loopback PKCE), token store, `delphi auth`; **Gemini dialect** (direct, Vertex), model scaler, rate limits; **compat gateway**; **endpoint probing** | 009 §4.1, 002, 009 §6 | API keys or a gateway only when available; built against mocks meanwhile |
| M7 | Red-team sweep of M5 and M6 | | |

The Step 2 go/no-go ("3 real tasks") can be run at M3 through the `claude` harness endpoint, with Delphi's roles doing read-only work.

Go/No-Go (roadmap): a Builder task completes in a sandboxed worktree with all merge gates, on a real project, with a recorded cost.

Step 3b (after Council 006 approves spec 010): `ConnectorHost` and MCP client, then Slack, Google (Drive, Calendar, Gmail) and Zoom.

## M3 findings so far (2026-10-24)

- **The sandbox and egress proxy work** (M2 and M3 part 1, tested against the real `sandbox-exec`).
- **A harness call works unsandboxed** with `claude -p --output-format stream-json --tools "" --no-session-persistence --setting-sources "" --strict-mcp-config`. The stream carries the reply, per-call `usage` and `total_cost_usd`, and a `rate_limit_event` showing the subscription window (`usage_source = reported`).
- **On macOS the `claude` login is in the Keychain**, under an item keyed by the config directory. Two consequences:
  1. Relocating the config directory (`CLAUDE_CONFIG_DIR`) to an ephemeral one makes even an unsandboxed CLI report "Not logged in". The sandboxed CLI therefore has to use the default `~/.claude`, which conflicts with `specs/009` §7.3 ("the operator's real `~/.claude` is not visible"). The spec needs an amendment, or a narrower rule that exposes only what a tools-disabled CLI needs.
  2. The CLI reads the item by running `/usr/bin/security`, so the sandbox must allow the keychain daemon, the keychain files and a long tail of system services. About a dozen rounds of denials were needed and the CLI still reported "Not logged in" inside the sandbox; the exact missing grant is not yet known.
- **Stopped here.** The next diagnostic step was to have `security` look up the login item from inside the sandbox. The session's auto-mode classifier refused that as credential exploration, and it was not worked around. The operator decides how to continue (see the session report).
- Gemini CLI keeps its login in a file (`~/.gemini/oauth_creds.json`), which is easier to expose read-only; untested.

## Reordered again (2026-10-24, operator)
The operator asked to keep thinking about the sandbox, consider other approaches, and wait for the full OAuth, API and MCP path if needed, skipping the CLIs. See `sandbox-review.md`. **M3 (harness mode) is paused.** New order: M6 (`oauth2`, Gemini dialect, compat gateway, probing) first, then the connector layer if spec 010 is approved, then sandbox hardening (resource limits, process-tree kill, Linux checks) for the Builder (M5), and harness mode last, with a VM option evaluated against what the API path has taught us.

## M6 progress (2026-10-24)

Done, against mocks only (no live Gemini call has been made):
- `delphi-adapter/src/gemini.rs`: request compile (`contents`, `systemInstruction`, `functionDeclarations`, `generationConfig`, thinking budget or level) and the streaming assembler. Signatures ride as signed `Thinking` blocks placed before the part they signed; thought summaries are display-only; a missing signature in an open tool loop gets Google's documented placeholder and is reported; synthetic `dg-` call ids are never sent back; thought tokens are billed as output.
- Platform envelopes for the Developer API, native gateways and Vertex; the key goes in a header, never the URL. `Dialect` on `HttpRequest` picks the assembler.
- Engine: per-span model family (`[access] role_family`, or the handoff directive), dialect-aware compile, provider recorded on turns and usage, restart-safe (`provider` is in `agent.turn.complete`). Failover stays inside a family.

Open, and why it matters:
- Unverified against a live endpoint: whether `maxOutputTokens` includes thought tokens (the engine currently sizes it as for Claude), `parametersJsonSchema` on every model, placeholder-signature acceptance, and the `thinkingLevel` values. `doctor --probe` should check each.
- Tool names with a dot (connector tools are `<connector>.<tool>`) are refused by both dialects; the connector layer needs a reversible wire-name mapping.
- Gemini `thinkingLevel` is not yet reachable from config (the dialect supports it).
- Next in M6: `oauth2` (device code, client credentials, loopback PKCE) with the token store and `delphi auth`; the compat gateway; endpoint probing.

Also done in M6 (against a scripted identity provider): `delphi-adapter/src/oauth2.rs` with device code (pending and slow_down handled), client credentials, loopback PKCE (rebinding, forged-state and script-fetch requests refused), OIDC discovery restricted to https, single-flight refresh with rotated-token storage, the keychain writer, and `delphi auth login|status|logout` (via `delphi-core auth`). Not verified: any real provider's quirks, and the real keychain write path (the tests use an in-memory store; try `delphi auth login` against your own IdP and `security find-generic-password -s delphi -a oauth2.<name>` to confirm the entry exists, without printing it). Remaining in M6: the compat gateway and `doctor --probe`.

Compat gateway done (`openai.rs`, spec 002 §2.2): chat-completions compile and stream assembly, any family, `Dialect::OpenAi`; reasoning and signatures never cross it; usage splits cached and reasoning tokens when the gateway reports them. Routing is per endpoint now (`provider_for(ep)`, `compile_for(ep, …)`). Deviations from §2.2 to ratify: blocks 1–4 are one system message joined by blank lines (the nonce-tagged sections already delimit them) rather than Markdown headings; `strict` tool schemas are not forced; prior-turn reasoning is dropped, not carried as a note. Remaining in M6: endpoint probing (`delphi endpoints test --probe`) and the "degraded" badge in the agents pane.

Probing done (`capabilities.rs`, `delphi endpoints test [id] [--probe]`, doctor check 7 reads the cache): plain, tool-call plus minimum-thinking, and signature round-trip probes, each capped (64 and budget+512 output tokens), pin-respecting, results merged by endpoint x model with a 24 h TTL; probe spend is printed and kept out of the mission ledger. Tested with scripted models; no live endpoint has been probed. **M6 is complete against mocks.** What it needs from you to become real: an API key or gateway for Claude, and a Gemini key (the free AI Studio tier is the cheapest way), then `delphi endpoints test --probe`.

Sandbox hardening (first part of M5) done: resource limits, own session and process group, `run_bounded` with wall-clock timeout, output cap and group kill, post-run quota check. While testing I found that two of the new tests would have passed vacuously (a relative path resolved outside the sandbox, and `/dev/zero` is deliberately unreadable); both now assert the positive outcome. See `sandbox-review.md` §4 for what remains (Linux, `setsid` escape, no die-with-parent on macOS).

Builder foundations done (M5, part 2), all against real git and the real sandbox:
- `worktree.rs`: worktrees under the workspace home on `delphi/<task-id>` cut from the project's HEAD, hardened daemon-side git (explicit git dir and work tree, no hooks, no user config, minimal environment), refusal of projects whose config has `include` or content filters, base-commit diff with the host-execution flag, restart-safe adoption from a record kept outside the sandbox's reach. A pre-commit hook in the project provably does not run (and provably does run without the hardening).
- `exec.rs` and the `run_command` tool: argv only (no shell), sandboxed, no network, timeout and output cap, environment built only from the ratified `[sandbox]` table, files the command changes are stamped with the span's tier, `action.intent` / `action.result` around each run. Offered only to roles with `shell-worktree-scoped` and a worktree.
- Found and fixed along the way: `effective_config` only dropped `endpoints`, `access` and `auth` from the live file, so a live `[harness]` or `[connectors]` table would have been trusted without ratification. The trusted set is now one constant (`lock::ACCESS_KEYS`) that also includes the new `[sandbox]` table. Existing ratified locks will show the config as differing until re-ratified (no real installs exist yet).
- **Deviation from D-04 to confirm:** D-04 says code execution falls back to Amber when no OS sandbox exists. I made it refuse instead (unsandboxed execution of model-written code on the host, even behind an approval card, is the thing the sandbox exists to prevent). Say if you want the Amber fallback.
- Known gap: `run_command` output is not path-tracked for taint, so `cat vendor/x.js` in the sandbox returns untrusted text without lowering the ceiling the way `read_file` does. The sandbox has no network, so the worst case is the model being talked into a bad edit, which still passes every merge gate and the operator's gate 5.
- Remaining in M5: the five merge gates evaluated against one commit SHA, merge-back, the unified retry ladder (attempt 2 upscale, attempt 3 other family), and the Builder's spec-gate (`Implements-Spec`, bounded exemption).

Merge gates and merge-back done (M5, part 3): `gates.rs` (pure verdicts for the five gates, all bound to one commit SHA; "no tests configured" is Red; a spec named but edited after ratification is Red even if an exemption was approved; an audit counts only for the SHA it names), `finalize.rs` (`Engine::finalize_build`: commit, run the sandboxed test and check commands, verify every changed file carries a provenance stamp for its exact content, read audit and approval events, show the operator the host-execution files first, merge back in the operator's checkout, write-ahead `action.intent` / `action.result`, clean up), and the spec gate at worktree creation (a build starts only against a spec whose current hash is in `ratified.lock`, or after the operator approves the bounded exemption). `Worktree::merge_back` refuses a moved branch, uncommitted edits to files the merge touches, a detached HEAD or a mid-merge checkout, aborts conflicts, and re-checks the checkout's config for filters and includes. Symlinks and submodules carry the host-execution badge.

Still open in M5 (not built): the unified retry ladder as an automatic loop (attempt 2 upscaled, attempt 3 fresh context on another family); the Orchestrator flow that dispatches a Builder then a Red-Team and calls `finalize_build`; a console command to trigger it (`record_audit` and `finalize_build` have no RPC yet); flake detection for gate 1 (every failure currently counts); PR mode (local merge only); a real run against a real project and model.

## Sandbox made optional (2026-10-24, D-24) and the harness revisited

Operator direction: approvals in the Claude Code style are the default; the sandbox is optional and to be evaluated later.

Built (this branch): `[exec] sandbox = "off" | "prefer" | "required"` (default off); unsandboxed commands run with the same process hardening (own process group, limits, no inherited descriptors, timeout and group kill) and an allowlisted environment; each command asks (Amber, never auto-approved by a permission mode) unless a ratified `allow` rule, the read-only list, or "allow similar" covers it; `deny` rules refuse first; wrappers (`sh`, `env`, interpreters) never match an allow rule; hooks and git config are snapshotted around unsandboxed commands and restored if changed; read-only commands that print vendored files taint the span; merge-gate commands ask too; `doctor` check 9 reports the choice instead of warning. The trusted-config table is now `[exec]` (renamed from `[sandbox]`, which never shipped to anyone).

Harness (spec 009 §7B): the Claude CLI runs as the operator's process with its own login and `--permission-prompts host`, which removes the Keychain obstacle that stalled M3. M3 is reinstated with this order:
1. Spike (needs your go-ahead: one capped subscription call): record the `claude` stream-json permission control messages and confirm `--permission-prompts host` works with `--setting-sources "" --strict-mcp-config`.
2. Harness driver: launch, stream parse, usage metering and kill-on-breach, structured output for non-builder roles with the clean-worktree check.
3. Approval bridge into the broker (the `[exec]` verdict function decides `Bash`).
4. Spike 2: `gemini --acp` as a Gemini bridge.
The strict (sandboxed) harness profile stays specified, for operators who want it, and is blocked on the same Keychain issue.

Spike 1 done (2026-10-24, six calls, about $0.12, `tools/claude_bridge_spike.py`): the approval bridge works with `--permission-prompt-tool stdio` (not with `--permission-prompts host` alone); allow runs the command, deny returns the operator's message to the model and nothing runs; isolation flags leave no user MCP servers. Details and the wire format are in spec 009 §7B. Next in M3: the harness driver, then the bridge into the broker. Spike 2 (`gemini --acp`) is still to do.

Harness driver built (M3, 2026-10-24): `harness.rs` (process and protocol: launch with the isolation flags, parse stream-json, answer permission requests without echoing suggestions, kill the process group on idle or breach, allowlisted env without API keys) and `harness_run.rs` (the engine side: bridge decisions through the same rules as Delphi's own tools, live token cap, usage and cost events, execution attribution, git guard, taint clamp). Tested with a stand-in CLI script for every path (deny without a card, ask-then-allow, tamper restore, token cap, no-result exit, tainted launch refused) and once against the real CLI (`cargo test -p delphi-engine live_claude -- --ignored`, about two cents, passed). Not built: the Gemini harness (spike 2, `gemini --acp`), failover from a harness to an API endpoint within a span, resuming a harness span after a daemon restart, and a `doctor` check that the CLI is installed and signed in.

Harness completion (2026-10-24): the Gemini harness is `agy` (the Gemini CLI refuses personal sign-in now; findings in spec 009 §7B): read-only roles only, tainted L0, project checkouts verified unchanged, no Builder. Failover built in both directions with a "has it acted yet" rule; harness probes in `delphi endpoints test --probe`; `doctor` check 10. Resumption of a harness's own session stays out (Delphi resumes the task and the CLI restarts). Live checks passed for both CLIs (`cargo test -p delphi-engine live_ -- --ignored`).

Build flow and the first real run (2026-10-24): `build.rs` (`Engine::run_build`, `/build` in the console, `build.start` RPC): Builder under the unified retry ladder (attempt 2 one tier up with at least twice the thinking budget and the diagnostic; attempt 3 on another family when one is allowed, otherwise a recorded `homogeneous_retry`), deterministic verification of exactly what was produced, a Red-Team audit of that commit by another family when possible, then the operator's merge card. Anything that is not a failed check (a halted agent, budget, spec gate) stops the ladder. Auditors can read the task's worktree. The gate commands share one approval card. Builder handoffs no longer say "read-only work". A real run through the Claude CLI passed in 17 seconds with one attempt (`live_build_through_the_claude_harness`). It also showed a Builder correctly reporting a spec/task mismatch and the auditor correctly raising P1s against a stub spec, which is the ladder doing its job. Attempt 2 keeps the previous work through the worktree and the diagnostic, not through the previous span's message history (a deviation from "history kept" in the spec). Gemini through your own Google OAuth client is built (Bearer plus quota project, authorization parameters for a refresh token) but unverified live: `first-real-run.md`.

Setup and install simplification (2026-10-24): repo public under MIT; one executable (`delphi __core` is the daemon, so no sibling binary); `delphi setup` (`setup.rs`) detects the `claude` and `agy` CLIs and their sign-in, creates the workspace, writes `config.toml` with comments preserved (endpoints, access order, `[exec]` defaults from the project's toolchain, Gemini auditing Claude when both exist), takes API keys and the Google client secret with hidden input straight into the keychain, discovers real model names from the provider, shows the access change and ratifies it on a keypress; re-running changes nothing; `--check` reports only; `--yes` takes defaults. Still to do: CI-built release binaries, `install.sh`, a Homebrew tap, and `delphi self-update`.

Release and install (2026-10-24): CI on macOS and Linux (`ci.yml`, the first time this code is built on Linux, so it doubles as the Linux verification), a tag-triggered release workflow (strict `vX.Y.Z` tags that are commits on main and match the crate version; four targets; checksums; build-provenance attestations; the tagged `install.sh` published as an asset; a separate job that updates `Formula/delphi.rb`, the Homebrew formula served from this repo through `brew tap jessevondoom/delphi <url>`; actions pinned to commit SHAs, toolchain pinned by `rust-toolchain.toml`), `install.sh` (HTTPS only, checksum, attestation when `gh` is signed in, exact-path extraction, symlink and staging-file care), and `delphi self-update` (checksum, smoke-test the new binary, atomic swap, `.prev` kept, `--rollback`, `--check`). A Sonnet security review through agy found 3 P0 and 8 P1 items in the first draft; every one that held up was fixed (the workflow checked out main instead of the tag; the formula job shared a token with third-party actions; the attestation was off by default; env overrides could redirect the download; loose version and tag patterns; unpinned actions; non-deterministic extraction; a predictable staging file). Open by design: until the first release is published, `install.sh` and the formula have nothing to download, and `main` must allow the `github-actions` bot to push the formula (or protect it and let the formula job open a PR instead).
