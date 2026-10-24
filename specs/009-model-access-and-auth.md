---
id: spec-009-model-access-and-auth
title: "009: Model Access Paths, Endpoints & Authentication"
author: architect
timestamp: 2026-10-23T21:15:00Z
status: approved   # by the operator, 2026-10-23, after Council 003
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, auth, providers]
---

# Spec 009: Model Access Paths, Endpoints & Authentication

## 1. Scope & Objectives
How Delphi reaches a model and proves it is allowed to. Decision **D-19**: every setup below is required in v1, and direct API keys are treated as the *least* common configuration, not the default.

1. **Access paths:** direct API, cloud platform (AWS Bedrock, Google Vertex AI), enterprise gateway, and subscription CLI (harness).
2. **Endpoints:** named, ratified ways of reaching a model family, each with its own credential source and probed capabilities.
3. **Credential sources:** delegated wherever possible, so Delphi rarely holds a secret.
4. **Policy:** endpoint selection, failover and the residency pin, applied to **all** model traffic.
5. **Harness mode** for subscription users, and the containment it runs under (§7).

### 1.1 Non-Goals
- Dialect compilation and signature handling (`specs/002`). This spec decides *where* a request goes and *how it authenticates*. 002 decides *what* is sent, including the compat dialect (`specs/002` §2.2).
- Running a local model server.

### 1.2 Phase Mapping
| Roadmap step | Delivers |
| :--- | :--- |
| **2** | §5 endpoint config, selection, failover and pinning; credential sources `keychain`, `env`, `command`, `aws`, `gcloud-adc`; Claude through **direct API, Bedrock, Vertex and native gateways**; §4 redaction; §8 ledger columns; tests 1–4, 6, 10 |
| **3** | Gemini (direct API, Vertex); `gateway-compat` (`specs/002` §2.2); `oauth2` device-code and client credentials (`delphi auth login`); §6 active probing; then, once the Step 3 sandbox exists, §7 harness mode for `claude`, `gemini`, `codex` with the egress proxy and approval bridge; tests 5, 7–9, 11–14 |
| **Later** | Direct OpenAI API driver; more platforms (for example Azure) as endpoint types |

All four access paths ship within v1 (Steps 2–3). Step 2 already serves cloud-platform and gateway operators, because the `command:` helper and the cloud credential chains cover corporate SSO.

### 1.3 Simpler Alternative Considered
- **Alternative A: API keys only.** Rejected by the operator. Most real setups are cloud platforms, corporate gateways with SSO, or subscriptions.
- **Alternative B: route everything through one proxy** (for example LiteLLM in OpenAI format). Rejected as the *only* path: it drops reasoning signatures and explicit caching (`[H-003]`) and adds a server. Kept as one supported option (`gateway-compat`).

---

## 2. Model: Family → Endpoint → Credential
- A **model family** is reached through one or more **endpoints**. Diversity rules (`[H-001]`) count families, never endpoints: Claude through Bedrock and through Anthropic's API is one family.
- Routing (`specs/002` §4) chooses a *model* by fit. This spec chooses the *endpoint* for that model by policy (§5).
- **Model IDs:** an endpoint's own `models` map takes precedence. The global `[models]` tier map in `config.toml` is the fallback for endpoints that omit one.

---

## 3. Access Paths

| Path | Examples | Transport & dialect | Fidelity | Credential sources (§4) |
| :--- | :--- | :--- | :--- | :--- |
| **Direct API** | Anthropic API, Google AI (Gemini API) | Native HTTPS, native dialect | Full | keychain, env |
| **Cloud platform** | AWS Bedrock, Google Vertex AI | Platform SDK/HTTPS; native model dialect inside the platform envelope | Full, as probed (§6) | AWS SDK default chain (SSO profiles, assumed roles, instance roles); Google ADC (user login, service account, workload identity) |
| **Gateway, native passthrough** | Corporate LLM gateways forwarding Anthropic- or Gemini-format requests unchanged | HTTPS to `base_url`; native dialect | Full, if the probe confirms passthrough | keychain header, `oauth2`, `command` |
| **Gateway, compat** | Gateways exposing only `/v1/chat/completions` | OpenAI chat-completions dialect (`specs/002` §2.2) | **Degraded:** tools work; reasoning signatures don't carry across turns; cache breakpoints may be ignored. The agents pane shows a badge. | same as above |
| **Subscription CLI (harness)** | `claude`, `gemini`, `codex`, signed in to the operator's subscription | Subprocess, stream-JSON | **Delegated:** the CLI runs its own loop under §7 containment | owned by the CLI; Delphi never reads it |

### 3.1 v1 Support Matrix
| Family | Direct API | Bedrock | Vertex | Gateway (native) | Gateway (compat) | Harness |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| Claude | ✅ (2) | ✅ (2) | ✅ (2) | ✅ (2) | ✅ (3) | ✅ `claude` (3) |
| Gemini | ✅ (3) | — | ✅ (3) | ✅ (3) | ✅ (3) | ✅ `gemini` (3) |
| OpenAI (GPT/Codex) | — (later) | — | — | — | ✅ (3) | ✅ `codex` (3) |

The number in parentheses is the roadmap step.

---

## 4. Credential Sources

| `credential` | What Delphi stores | Notes |
| :--- | :--- | :--- |
| `keychain:<name>` | A static key or header value in macOS Keychain / Linux Secret Service | API keys and gateway tokens |
| `env:<VAR>` | Nothing | Read at call time |
| `aws:<profile>` | Nothing | AWS SDK default chain (`aws sso login`, assumed roles, instance roles) |
| `gcloud-adc` / `gcloud-sa:<keychain-ref>` | Nothing, or a keychain reference | Google Application Default Credentials |
| `oauth2:<auth-name>` | Refresh token (and any client secret), in the keychain only | Configured by an `[auth.<name>]` block (§4.1). Access tokens stay in memory and are refreshed before expiry. |
| `command:<argv>` | Nothing | Exec credential helper that prints `{"token": "...", "expires_at": "..."}`. It plugs in corporate SSO tooling without Delphi implementing each identity provider. Its stderr is never logged raw. |
| `harness-login` | Nothing | The official CLI authenticates itself (`claude login`, and so on) |

### 4.1 `[auth.<name>]` Schema (OAuth2)
```toml
[auth.corp-okta]
flow = "device_code"                  # device_code | client_credentials | loopback_pkce
issuer = "https://idp.example.com"    # OIDC discovery; or token_url + device_authorization_url
client_id = "<per-organization client id>"   # supplied by the operator's IdP admin; Delphi ships none
scopes = ["llm.invoke"]
audience = "llm-gateway"              # optional
client_secret = "keychain:corp-okta-secret"  # client_credentials (or a confidential PKCE client); keychain reference, never inline
# token_url / device_authorization_url / authorization_url   # override OIDC discovery; https only
# redirect_port = 8765                  # loopback_pkce: the fixed port registered with the provider
```

`delphi auth login <name> [--manual]`, `delphi auth status` and `delphi auth logout <name>` read the **ratified** blocks (a local edit cannot redirect a login). Login stores one thing: the refresh token, in Delphi's own keychain entry `oauth2.<name>` (`service=delphi`), written through the system tool's standard input so it never appears on a command line. `loopback_pkce` binds `127.0.0.1`, checks `Host` and fetch metadata, requires a 256-bit `state`, and falls back to pasting the redirect URL with `--manual`. A revoked token (`invalid_grant`) removes the entry and pauses the call with `re-authenticate <endpoint>`; an unreachable provider does not.

### 4.2 Credential Invariants
1. **Never on disk.** Delphi never writes a credential to any file: not config, logs or `events.jsonl`. The only exception is keychain entries it created.
2. **Never another tool's.** Delphi never reads, copies or reuses another tool's stored credentials (CLI session files, browser cookies, SDK caches).
3. **Never in agent context.** Credentials are resolved by the daemon at call time and never enter an agent's context, tool results or `HandoffEnvelope`.
4. **Literal redaction:** every *runtime-resolved* credential value (keychain, env, AWS/STS, Google tokens, OAuth access tokens, helper output, secret `headers`) is registered with the event redactor (`specs/005` §2.3) as a literal, for its lifetime. Error bodies and helper stderr are therefore masked even when the token has no recognizable format.
5. **Expiry mid-task:** the call pauses and the console shows `re-authenticate <endpoint>`. It never fails over to an endpoint outside the policy (§5).

---

## 5. Endpoint Configuration, Selection & Policy

```toml
[[endpoints]]
id = "bedrock-us"
family = "claude"
path = "cloud"                    # api | cloud | gateway-native | gateway-compat | harness
platform = "bedrock"
region = "us-east-1"
credential = "aws:work-sso"
models = { workhorse = "<bedrock model id>", frontier = "<bedrock model id>" }   # overrides global [models]

[[endpoints]]
id = "corp-gateway"
family = "claude"
path = "gateway-native"
base_url = "https://llm-gateway.example.internal/anthropic"
credential = "oauth2:corp-okta"
headers = { "X-Cost-Center" = "eng-123" }

[[endpoints]]
id = "claude-sub"
family = "claude"
path = "harness"
harness = "claude"
credential = "harness-login"
cost_model = "subscription"

[access]
order = { claude = ["corp-gateway", "bedrock-us", "claude-sub"] }
allowed_endpoints = ["corp-gateway", "bedrock-us", "claude-sub"]   # compliance pin
```

- **The pin applies to all model traffic:** primary selection, failover, per-role overrides, capability probes (§6) and `doctor`. A request is sent only to an endpoint in `allowed_endpoints`. When every allowed endpoint is down, the task waits or escalates; it never leaves the allowed set.
- **Gemini endpoints:** `family = "gemini"` with `path = "api"` (Developer API, `x-goog-api-key` header), `"gateway-native"` (Bearer) or `"cloud"` with `platform = "vertex"` (publisher `google`, Bearer). Bedrock does not serve Gemini. A role is served by Claude unless `[access] role_family = { <role> = "gemini" }` says otherwise (or a handoff's `routing_directive.model_family` does); failover never crosses families, because reasoning signatures and tool formats are not portable. `role_family` is part of the ratified `[access]` block.
- **No cross-origin redirects:** HTTP redirects to a different scheme, host or port than the endpoint's `base_url` are refused, never followed with credentials attached.
- **Configuration is ratified:** the `[[endpoints]]`, `[access]`, `[auth.*]` and `[harness.*]` blocks of `config.toml` are hashed in `local/ratified.lock` (`specs/006` §3.1). Any change to a `command:` credential, `base_url`, `headers`, `allowed_endpoints`, `order` or `[auth.*]` is staged as a **Red-tier** proposal and doesn't take effect until ratified. A local process that edits the config therefore can't inject a helper command or move traffic.
- **Signatures across endpoints:** reasoning blocks are treated as foreign when a turn moves between endpoints (`specs/002` §3.1), unless an explicit `E1 → E2` replay probe (§6) passed for that ordered pair.

---

## 6. Capability Probing
`delphi endpoints test [id] [--probe]` sends a **minimal, capped** probe suite to allowed endpoints only. Each probe has a token cap, for example ≤ 2K tokens per endpoint. For each endpoint × model it records:
- authentication OK and expiry;
- tool calling;
- reasoning, and whether signatures round-trip within the endpoint and, for configured pairs, from `E1` to `E2`;
- explicit caching (whether cache-read tokens are reported);
- effective context and output limits;
- whether usage is reported;
- for gateways: native passthrough vs compat, detected rather than assumed.

Probes run in order (plain reply and usage; forced tool call with the model's minimum thinking budget; the tool result sent back with signed reasoning replayed) and stop at the first failure that makes the rest meaningless. Prompt caching, context limits and `E1 → E2` replay are not probed in v1 and are recorded as such. Results are stored in Zone 3 (`local/endpoint-capabilities.json`; the `state.db` mirror is deferred) with a 24-hour TTL. They are **never** written to the team-tracked `context/model-capabilities.md`, which holds only static per-family defaults. `doctor` reads cached results and probes only with `--probe`, which keeps it offline and free by default. Routing uses probed features, not vendor assumptions.

---

## 7. Harness Mode (Subscription CLIs)

The harness runs its own agent loop, so Delphi can't see its internal reads or tool calls. Every condition below is mandatory; a harness that can't meet one is unavailable.

1. **Official binary, operator-owned.** Delphi launches the operator's installed `claude`, `gemini` or `codex` binary as the operator's own process. It never extracts tokens or calls subscription backends directly. `doctor` shows a one-time notice that each provider's terms govern automated use of a subscription.
2. **Always sandboxed, for every task type** (code, discovery, Council). No sandbox means no harness mode.
3. **Filesystem (deny-by-default for reads *and* writes):**
   - **Read and write:** only the task worktree, minus any `.git` path segment (`specs/001` §2.2), plus a fresh **ephemeral per-task state dir** used as the CLI's session and cache home.
   - **Read-only:** the CLI's credential file(s), exposed at the path that CLI expects. Delphi itself still never reads them.
   - **Not visible:** the operator's real `~/.claude`, `~/.gemini` or `~/.codex` directories. A harness can't plant persistent hooks, plugins or MCP servers on the host. `$DELPHI_HOME` (including `run/`, `state.db`, `operator/`) and everything else under `$HOME` (`~/.ssh`, `~/.aws`, and so on) are also hidden.
4. **Network:** no direct egress. All traffic goes through a **daemon-run local CONNECT proxy** (`HTTPS_PROXY`). It allows only the endpoint's configured provider hosts (by CONNECT target and SNI) and logs every denied attempt. Hostname filtering is done by the proxy because the OS sandboxes can't filter by hostname.
5. **Environment:** the process gets an explicit allowlisted environment (no inherited `AWS_*`, `GITHUB_TOKEN`, and so on). It is launched with flags that disable user-global hooks, plugins and non-Delphi MCP servers.
6. **Approvals:** where the CLI supports a permission-prompt tool, it connects to a **per-span MCP socket** inside its ephemeral state dir. The socket carries a single-use token bound to `(span_id, role, external_ceiling)` and exposes only `request_approval`, so one span can't speak for another. Requests go through the normal approval broker and tier rules.
7. **No bridge, or a non-builder role:**
   - Where a CLI has no permission bridge, it runs with tools in their most restrictive mode, and its file changes come back only as a worktree diff.
   - Council seats and non-builder roles always run with a **read-only worktree and CLI tools disabled**, and return structured output only.
   - Every returned diff goes through the merge-gate classifier: any `.git` or `host_exec_paths` match is **Red**, the rest is Amber.
8. **Taint clamp:** before launch, the span's `external_ceiling` is clamped to the **minimum provenance tier of every file the sandbox exposes** (`specs/001` §5.1). Launch is refused if that is $\le L_1$. Harness output is stamped `min(L3, ceiling)`. Delphi can't see what the harness read internally, so the clamp assumes it read everything it could.
9. **Prompt injection, per harness:** a table in config (`[harness.<name>]`) records how governance, the role manifest, the persona slot and the `HandoffEnvelope` are passed: an append-system-prompt flag where available, otherwise a context file. Context files are written only to the ephemeral state dir, or are listed in the worktree's `info/exclude` and stripped from every merge diff. A test verifies that no merge diff contains them. The CLI's own built-in instructions stay outside Delphi's control; this is an accepted, documented deviation from `[REV-ARCH-02]`.
10. **Budget, enforced live:**
    - Harness usage comes from the CLI's reported usage when present (`usage_source = reported`); otherwise it is estimated as `ceil(utf8_bytes / 4)` of stream input and output (`estimated`).
    - `cost_model = "subscription"` endpoints are capped by `subscription_budget_tokens` (default 500,000 per mission, 2,000,000 per Council) and by `max_turn_budget`.
    - The daemon reserves worst-case tokens before launch, meters the stream as it runs, and **kills the subprocess** on any breach, then escalates.

### 7A. Amendment 009-A2 (REJECTED): Harness Connectors
> **Status: rejected by Council 005 (2026-10-24, D-22 Option B).** Kept for the record. Harness connectors are not available in v1: §7 item 5 stands, and harnesses run with every non-Delphi MCP server and connector disabled. The Council found that no current CLI can isolate or filter connectors, that the credential carve-out in rule 2 is impossible, that the taint rules contradict §7.8, and that an SNI-only proxy cannot enforce read-only access. The post-v1 path is a native Delphi connector layer.

**Motivation.** Operators already connect their subscription CLIs to services such as Google Drive, Slack or Jira. For example, a `scout` task run through `claude` could research a team's Drive folder using that CLI's own Drive connector. Rebuilding every connector natively in Delphi is out of scope for v1. Delegating to the harness reuses the operator's existing, provider-managed logins without Delphi touching their credentials (§4.2).

**Proposed rules.** These are additions to §7; every other §7 condition still applies.
1. **Ratified allowlist, per workspace and role.** Connectors are off unless listed in `config.toml`:
   ```toml
   [harness.claude.connectors.google-drive]
   roles = ["scout", "investigator"]   # canonical role IDs only
   mode = "read"                       # "read" in v1; "write" is post-v1 (rule 5)
   ```
   - The block is pinned with `[access]` in `ratified.lock` (`config.toml#access`, §5), so adding or widening a connector is a Red-tier proposal.
   - Council seats never get connectors.
2. **Only listed connectors are passed.** The daemon builds the CLI's MCP and connector configuration in the ephemeral state dir from the allowlist alone. The operator's real `~/.claude` (or equivalent) stays hidden (§7 item 3).
   - Where a CLI can load a connector's credentials only from its global config, Delphi exposes that one entry read-only, the way §7 item 3 exposes CLI credential files. Delphi never reads it.
   - A connector that can't be isolated this way is unavailable.
3. **Network.** The §7 item 4 proxy allows each listed connector's hosts, declared in the allowlist entry and logged per span. Everything else stays denied.
4. **Taint.** Any span that had a connector available is stamped `external_untrusted` ($L_0$). Delphi can't see what the connector returned, so it assumes the worst (the same reasoning as §7 item 8). The output then flows only through the existing $L_0$ rules (`specs/001` §5.1–5.2). It can inform research notes and drafts, but it can't satisfy a spec gate or trigger taint-gated tools without operator review.
5. **Writes.** v1 connectors are read-only.
   - The allowlist passes only a connector's read tools where the CLI exposes per-tool filtering. A connector that can't be limited to reads requires `mode = "write"`, which is post-v1.
   - Post-v1 write mode routes each write through the per-span approval bridge (§7 item 6) as a **Red** action that is never auto-approved by any permission mode.
6. **Exfiltration bound.** A connector span gets a read-only worktree and no other write tools (as for non-builder roles in §7 item 7). Its only output channel is its structured result, so content it read can't leave through the connector in v1.
7. **Visibility.** The agents pane and the ledger show which connectors each span had (`connectors: ["google-drive"]`). `doctor` lists the allowlisted connectors per harness and warns if the CLI doesn't report them as signed in.

**Proposed tests.**
- An unlisted connector is unavailable in the harness.
- A listed read-only connector can't call a write tool.
- A connector span is stamped $L_0$, and its output can't satisfy a spec gate unreviewed.
- Adding a connector block without ratification leaves it disabled and `doctor` reports a pending proposal.
- A connector span's network traffic outside its declared hosts is denied and logged.

**Open questions for the Council.**
- Which CLIs support per-connector isolation and per-tool filtering today? This needs probing per harness version (§6).
- Should connector-backed research be allowed in Step 3, when harness mode lands, or wait until after the Step 4 Council?
- Should a native Delphi connector layer, with Delphi-owned OAuth (§4.1), replace or complement this post-v1?

---

## 8. Usage Ledger & Budget
`usage_ledger` (`specs/005` §2.4) carries `endpoint_id`, `access_path`, `usage_source` (`reported` | `estimated`) and `cost_model` (`metered` | `subscription`).
- Metered paths price from the config price table, with optional per-endpoint overrides (negotiated gateway rates).
- Subscription paths record tokens with `cost_usd = 0` and are capped in tokens (§7.10).
- The agents pane shows `est.` beside estimated figures, and a fidelity badge for compat and harness endpoints.

---

## 9. Verification Plan
1. **No-Secret-On-Disk Test:** run init and a mission with every credential type. Scan `$DELPHI_HOME`, logs and `events.jsonl` for token material: zero hits apart from keychain entries.
2. **Residency Pin Test:** with `allowed_endpoints = ["vertex-eu"]` and that endpoint returning `503`, no request (failover, probe, or per-role override) reaches any other endpoint, and the task escalates.
3. **Redirect Test:** a gateway returning `307` to another host does not receive a follow-up request, and no credential is sent cross-origin.
4. **Config Tamper Test:** a process edits `config.toml` to change `credential = "command:…"` and `allowed_endpoints`. The daemon keeps using the ratified values and stages a Red proposal.
5. **SSO Expiry Test:** an `oauth2` access token expires mid-task. The call pauses with a re-authenticate prompt and resumes after `delphi auth login`.
6. **Redaction Test:** a gateway error body echoing the bearer token, and helper stderr containing it, appear masked in every log.
7. **Gateway Detection Test:** a mock gateway that strips thinking blocks is classified `gateway-compat`, and signature round-trips are not attempted.
8. **Family-Not-Endpoint Test:** Claude through Bedrock plus Claude through Anthropic's API counts as one family for Council diversity.
9. **Harness Containment Test:**
   - a harness attempting `curl example.com`, reading `~/.ssh/id_ed25519`, reading `$DELPHI_HOME/local/state.db`, connecting to `$DELPHI_HOME/run/*.sock`, writing `@worktree/.git`, or writing to the real `~/.claude/` is blocked;
   - the proxy log records the denied connection.
10. **Model Map Precedence Test:** an endpoint's `models` entry overrides the global `[models]` alias for that endpoint only.
11. **Harness Taint Test:** a worktree containing an $L_1$ file causes harness launch to be refused. A worktree with `vendor/**` ($L_0$) clamps the span to $L_0$.
12. **Approval Spoof Test:** a second sandboxed process presenting a captured MCP token gets nothing, because the token is single-use and span-bound.
13. **Subscription Budget Test:** a harness loop exceeding `subscription_budget_tokens` mid-stream is killed and escalated, with ledger rows marked `subscription` and `estimated` where applicable.
14. **Council-Seat Harness Test:** a Council seat on a harness has no writable worktree and no tools, and returns only structured output.
