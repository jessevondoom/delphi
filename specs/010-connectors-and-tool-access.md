---
id: spec-010-connectors-and-tool-access
title: "010: Native Connectors (MCP) & the Tool Access Layer"
author: architect
timestamp: 2026-10-24T16:00:00Z
status: approved   # by the operator, 2026-10-24, after Council 006 rounds 1 and 2 (D-23); §1 item 6 amended by 010-A1 (D-27, operator 2026-10-24); §5.2, §6 amended by 010-A2 (council 013 D23)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium   # provider facts in §7 come from secondary sources and must be probed (§9)
source_mission: 2026-10-23-foundational-design
tags: [spec, connectors, mcp, auth, tools]
---

# Spec 010: Native Connectors (MCP) & the Tool Access Layer

> **Outside amendments:** §1, §3 bundle-mapped connectors and the read-only `delphi bundle serve` MCP server (ratified 2026-10-25). Live in `specs/022` §5 and `specs/025` §4.

## 1. Scope and Non-Goals
Delphi gives agents access to outside services (v1: **Slack, Google Drive, Google Calendar, Gmail, Zoom**) by running the connectors itself, as an MCP client inside the daemon. This replaces the rejected harness-delegated design (Council 005, D-22; `specs/009` §7A).

- **In scope:** the connector host, MCP transports, connector authentication (browser login), the ratified connector catalog, how connector tools enter the existing tool pipeline (classification, taint, approvals), result handling, and the five v1 connectors.
- **Model families:** Claude and Gemini. Connector tools are ordinary Delphi tools, so they work on every access path that supports tool calls (`specs/009` §3). Other families (OpenAI, local models) and other connectors (Microsoft 365, GitHub, Jira, Notion) are future work (§11) and need no change to this design.
- **Not in scope:** exposing Delphi itself as an MCP server to other tools; connector write support beyond the approval-gated actions in §6.

## 2. Architecture
```
agent span ──tool call──▶ tool pipeline ──▶ ConnectorHost ──▶ MCP client ──▶ remote MCP server (Slack, Google…)
                           classify → taint gate → approval      │              ▲
                                                                 └─ token store (OS keychain) ◀─ browser OAuth login
```
1. **`ConnectorHost`** lives in `delphi-engine`. It owns one MCP client session per connected connector and the token store. It runs in the daemon, never in a sandbox or an agent's process.
2. **Tools are namespaced** `<connector>.<tool>` (for example `slack.search_messages`) and offered to a span only if the span's role is on the connector's ratified `roles` list (§3). `Registry::register_external` enforces the namespace and refuses any name that is already registered, so a server can never shadow `read_file` or another connector's tool.
3. **One pipeline.** Every connector call passes through the same steps as `read_file` and `write_file`: classify → taint gate → approval if needed → execute → persist. There is no second code path.
4. **Harness bridge.** When a model runs through a harness (`specs/009` §7), connector tools reach it through the per-span MCP socket of `specs/009` §7.6. The harness sees them as Delphi's tools; the CLI's own MCP servers and connectors are also on by default, and a workspace may turn them off with `claudeai_connectors = false` (`specs/020` §6 amends `specs/009` §7 item 5). The bridge is built: `specs/020` §2 (`bridge.rs`, `delphi __mcp-bridge`).
5. **Transports:** Streamable HTTP (remote servers; all five v1 connectors) and stdio (local servers, later). The client negotiates the protocol version and pins a minimum (initially the 2025-11-25 revision).
6. **The MCP client is the official Rust SDK, `rmcp`** (amendment 010-A1, D-27, operator 2026-10-24), wrapped in `delphi-adapter::mcp_rmcp` behind a small `McpSession` trait (`list_tools`, `call_tool`) so the connector host sees only Delphi's types. Delphi keeps what makes it safe: the connector's host list is enforced by the HTTP client's redirect policy and a check of the URL before anything is sent (https only, loopback excepted; only 307/308 followed, only to a listed host, at most 3 hops), tokens never appear in errors, and the policy code (taint, approvals, path guard, pinning) stays in-tree. *History:* Council 006 chose a minimal in-tree client so redirects, timeouts and size limits stayed under Delphi's control; the strategist seat argued for a library. The slice-1 spike ran both on the same contract cases and the library matched them except the accepted differences listed in the `mcp_rmcp.rs` header (no cap on a plain JSON response, `Retry-After` dropped, strict parsing of server replies). The protocol floor is still 2025-11-25. A new connector is checked against the real server with the ignored `live_server_smoke` test before it is ratified.

## 3. The Ratified Catalog
Connector configuration is a Red-tier proposal and is hashed with `[access]` (`config.toml#access`; `[connectors.*]` joins `[harness.*]` in `lock::access_blocks`). Nothing is read from an unratified edit.

```toml
[connectors.slack]
transport = "http"
url = "https://mcp.slack.com/mcp"
auth = "oauth"                       # oauth | none
client_id = "<from the operator's Slack app>"   # optional where the server supports dynamic registration
client_secret = "keychain:slack-client-secret"  # only if the provider requires one
scopes = ["search:read", "channels:history"]
account = "T01ABCDEF"                           # required: team id (Slack) or email (Google, Zoom); checked after login (§4.10)
redirect_port = 0                               # 0 = ephemeral; a fixed port for providers that register exact redirect URIs
max_arg_chars = 128                             # cap on any single string argument of a call (§6.2)
first_use = "ask"                                # "ask" (Amber once per task) or "allow" (Green) for read tools; never applies to a tainted span with a query (§6)
roles = ["orchestrator", "strategist"]          # canonical role IDs; never a Council seat
hosts = ["mcp.slack.com", "slack.com"]          # exact names; no wildcards
tools = { "search_messages" = "read", "read_channel" = "read", "post_message" = "write" }
```
1. **`tools` is Delphi's own allowlist and classification.** A server's tool annotations and descriptions are untrusted. A tool the server offers that is not listed is invisible to agents. A listed tool is `read` or `write`, as the operator ratified.
2. **Definition pinning.** At ratification Delphi records a hash of each allowed tool's name, input schema and description. A description is an instruction to the model, so a poisoned one is an injection vector. If the server later changes any of the three, that tool (only that tool) is disabled and the console shows a **diff card** to re-ratify in one step. Cosmetic edits cost one keypress, not a blocked mission. (Council 006 split on this; the stricter position was kept.)
3. **Descriptions are data.** Tool descriptions from servers never enter the system channel; they are shown to the model as tool metadata only after the daemon has escaped delimiter strings (`specs/002` §2).
4. `hosts` is the connector's network allowlist. The daemon's HTTP client refuses any other host, including redirects, and logs refusals.

## 4. Login
Connectors authenticate with OAuth in the operator's browser, following the MCP authorization model (OAuth 2.1 with PKCE, protected-resource discovery, resource indicators). Delphi never reuses another tool's login (`specs/009` §4.2 invariant 2).

1. **`delphi auth login <connector>`** starts the flow from the console or CLI.
2. **Authorization code with PKCE, loopback redirect (default).** The daemon opens a listener on `127.0.0.1` on an ephemeral port, prints and opens the authorization URL, and accepts exactly one callback. It checks `state`, binds PKCE, and closes the listener on success or after a short timeout. The redirect URI is registered as `http://127.0.0.1:<port>/callback` where the provider allows a port wildcard for loopback.
3. **Device code (fallback)** for headless machines and SSH sessions, where the provider supports it (`specs/009` §4.1). Otherwise the console shows the URL and accepts the pasted callback URL.
4. **Client registration.** Prefer dynamic registration or a client-ID metadata document where the server supports it. Where it does not (Zoom is reported not to), the operator registers an app with the provider and puts `client_id` in the ratified catalog. Delphi ships no client IDs or secrets of its own.
5. **Storage.** Only the refresh token (and a client secret, if any) is stored, in the OS keychain under a Delphi-owned name. Access tokens live in daemon memory and are refreshed before expiry. Tokens are registered with the redactor for their lifetime (`specs/009` §4.2 invariant 4).
6. **Authority.** Tokens act as the signed-in user, with the provider's own permissions. Delphi requests the smallest read-only scope set that supports the ratified tools; a write tool needs its write scope listed and ratified.
7. **Expiry or revocation mid-task** pauses the call with `re-authenticate <connector>` and never retries with another account.
8. **Admin approval.** Some providers need a workspace admin to approve the app (reported for Slack). `delphi auth login` explains this when the provider returns that error.
9. Login and logout run only from the console (TTY challenge, like ratification). An agent cannot start, finish or switch a login.
10. **Account pinning.** After the token exchange the daemon asks the provider who signed in (`auth.test`, `userinfo`, the Zoom user endpoint) and compares it with the ratified `account`. A mismatch (the wrong browser profile) discards the token and fails closed, so personal mail never lands in a work mission because the wrong account happened to be signed in.
11. **Loopback listener hardening.** Bind `127.0.0.1` only; require the `Host` header to be `127.0.0.1:<port>`; check `state` and PKCE `S256`; accept one callback; reject any request with a cross-site `Sec-Fetch-Site` or an `Origin` header (a web page cannot drive it); use a fresh 256-bit `state` per login; close after success or 120 seconds. Providers that register exact redirect URIs (reported for Slack and Zoom) use the ratified `redirect_port`; if it is already in use, login stops with a clear error instead of picking another port. `delphi auth login --manual` prints the URL and accepts the pasted callback URL for headless machines and fixed-port failures.

## 5. Trust and Taint
1. **Everything a connector returns is external content.** Results are stamped `external_untrusted` (L0) and lower the calling span's ceiling like any other untrusted read (`specs/001` §5.1). v1 never stamps connector content higher, even from the operator's own mailbox: other people wrote most of it.
2. **Consequences follow the existing table** (`specs/001` §5.2), with one addition (amendment 001-A3): a connector **write** has its own gate, `Action::ConnectorWrite`, which is **Red at every taint level and never denied**. Without that, reading a thread (which taints the span to L0) would make replying to it impossible, because ordinary outbound actions are denied when tainted. The control is the operator's approval of the exact payload (§6). Worktree writes, code execution and ordinary outbound actions stay denied at L1 and below.
3. **Promotion.** Notes written by a tainted span are L0. To use them in later work, the operator reviews the note in the console and approves it. The card shows the note as rendered text with any section delimiter or instruction-shaped line ("ignore previous…", tool-call syntax) highlighted, and promoted content is still wrapped as quoted data when a later span reads it; Delphi then stamps that file's hash at L4 through the same path ratification uses for files today (`provenance.stamp`). This keeps research usable without letting a tainted span launder content into a trusted artifact.
4. A child span inherits its parent's ceiling, and a parent inherits what its child read (Step 2 round-1 fix).

## 6. Tool Tiers and Approvals
| Tool class | Tier | Rules |
| :--- | :--- | :--- |
| `read` | Per `first_use`: Amber on first use per task per connector, then Green for that task; or Green from the start when the ratified entry sets `first_use = "allow"` and holds read-only scopes. **Amber every time, showing the query text, when the span's ceiling is L1 or below and the call has a non-empty query or search argument** (a tainted span could otherwise exfiltrate through search strings) | The card names the connector, the account and the scopes. `[a]` allow-similar is task-scoped (`specs/005` §3.1). |
| `draft` that stays a private, unsent artifact (a Gmail draft, a calendar hold with no invitees) and is made by an untainted span (ceiling above L1) | Amber | Its own card for every call, showing the full payload. No mode and no `[a]` allow-similar covers it (010-A2). |
| `draft` from a span whose ceiling is L1 or below, or one that names recipients or invitees | **Red** | As `write`, below. |
| `write` that leaves the account: sends, posts, shares, invites or deletes | **Red** | Never auto-approved by any mode. The card shows the full payload, bound to its hash (`specs/005` §3.1). `[a]` is refused. |

1. **Read-then-write guard.** Every connector write is Red (§5.2), and the card shows the destination (account, channel or recipients), the full payload and its hash. This is the exfiltration defense: the agent can't quietly post what it read.
2. **Arguments are bounded and checked.** Read calls can also leak, by putting data in a search query. Every string argument is capped at `max_arg_chars` (default 128), and the daemon refuses a call whose arguments contain any registered secret literal or a credential-shaped string (the event-log redactor). Persisted arguments (§8) make anything else visible after the fact. Residual risk: short, non-secret fragments of workspace content can still reach the provider's search endpoint; the host is the user's own service, and the first-use card says so.
3. **Rate and size caps** are per connector in the ratified catalog (calls per minute; results per call). Exceeding them pauses the span and escalates, never silently drops.

## 7. The Five v1 Connectors
Provider facts below come from secondary sources and must be confirmed by the probes in §9 before the first connector ships. Each connector has its own exit criteria in §10.

### 7.1 Slack
- **Server:** Slack's hosted MCP server (`https://mcp.slack.com/mcp`, Streamable HTTP), generally available since February 2026. OAuth acts as the signed-in user, and a workspace admin must approve the app.
- **v1 tools:** search messages, read a channel or thread, list channels, read user profile (all `read`); post a message and add a reaction (`write`, Red).
- **Specific risks:** messages are written by other people and are prompt-injection vectors. Private channels and DMs must be opt-in per ratified channel list where the server supports it; otherwise the connector is limited to scopes that exclude them.
- **Quality:** thread-aware formatting (author, time, permalink), pagination into context pointers, and resolved user and channel names.

### 7.2 Google Drive
- **Server:** Google's remote Drive MCP server (`drivemcp.googleapis.com`, reported), with an OAuth client created by the operator in their own Google Cloud project.
- **v1 tools:** search files, read a document or spreadsheet as text, list a folder (`read`). No write tools in v1 (`write` needs a later amendment).
- **Quality:** native Docs and Sheets exported to Markdown and CSV, size-capped with a pointer to the remainder, file IDs and revision kept in the stamp so a re-read shows if the document changed.

### 7.3 Google Calendar
- **Server:** `calendarmcp.googleapis.com` (reported), same Google OAuth client as Drive.
- **v1 tools:** list calendars and events, read an event, free/busy (`read`); create a private hold with no invitees (Amber); create or change an event with attendees, or delete one (Red).
- **Quality:** correct time zones and recurring events (expanded instances with the rule shown), all-day vs timed events, and attendee lists included in the Red card.

### 7.4 Gmail
- **Server:** `gmailmcp.googleapis.com` (reported), same Google OAuth client.
- **v1 tools:** search, read a message or thread (`read`); create a draft (Amber); send (Red).
- **Specific risks:** email is the largest prompt-injection surface in the catalog and carries third-party personal data. Message bodies are always L0. Quoted history and long threads are cut to the new text with a pointer to the rest.
- **Quality:** MIME handled (plain text preferred, HTML converted), attachments listed but not fetched in v1, headers shown (from, to, date, message ID).

### 7.5 Zoom
- **Server:** Zoom's hosted MCP server (`mcp.zoom.us`, reported), OAuth with manual client registration (no dynamic registration), PKCE required. The operator creates the app in the Zoom Marketplace.
- **v1 tools:** list meetings, read meeting details, read recordings and transcripts the user can access (`read`); create a meeting (Amber); update or delete a meeting (Red).
- **Quality:** transcripts chunked with timestamps and speaker names, long transcripts stored as context pointers, never stuffed into one tool result.

## 8. Results, Persistence and Privacy
1. **Large results become spillover files.** A result over the per-call cap goes to the payload store (item 2); the model receives a bounded excerpt plus a pointer (payload hash and line range) that `read_file` can read in slices. This does not depend on the Step 5 retrieval index (`specs/004` §3).
2. **Payload store, not the event log.** Raw results hold private mail and chat, so `events.jsonl` records only the call's metadata, size, truncation and blake3 hash. The redacted payload goes to `$DELPHI_HOME/workspaces/<name>/payloads/<blake3>` (mode 0600), fsynced before the event that references it, and recovery reads it by hash (`specs/005` §2.5). `gc` deletes payloads after a ratified `connector_result_retention_days` (default 7 for closed missions); the hash and metadata remain, and a replayed call whose payload expired reports `payload expired` instead of failing. Agents reach the store only through the logical scheme `@payload/<blake3>`, where the name must match `^[0-9a-f]{64}$` exactly (no separators, no traversal) and resolves only to a regular file under the store; the directory is never readable by any other path. A per-workspace quota (default 500 MB) refuses new payloads past the limit and escalates instead of filling the disk.
3. **Events:** `connector.call` (connector, tool, class, arguments, account), `connector.result` (size, truncation, hash), `connector.denied`, `connector.reauth_required`. The usage ledger records calls per connector.
4. **The model provider sees what the agent reads.** Content a connector returns is sent to whichever model endpoint runs the span. The first-use card for each connector says so, and `[workspace] isolated` and `allowed_endpoints` already bound where that can go.

## 9. Probing and `doctor`
`delphi connectors test <name>` and `doctor` check, per connector: reachability, a successful token refresh, that each ratified tool exists with the pinned definition, that no unlisted tool is exposed, rate-limit headers, and that read tools return without write scopes granted. Probes use read-only calls on the operator's own account. **The five connectors' endpoints, scopes and registration requirements in §7 are assumptions until these probes pass.**

## 10. Phasing and Tests
- **Step 3 (already planned):** `oauth2` device code and client credentials; keychain token store; tool-registry refactor so tools are data, not a match statement.
- **Step 3b (new, before Step 4), in three shippable slices:** (1) `ConnectorHost`, the in-tree MCP client, loopback PKCE login, the ratified catalog and pinning, the payload store and the promotion card, with **Google** (Drive, Calendar and Gmail share one login) as the pilot; (2) **Slack**; (3) **Zoom**. Each slice has its own contract tests and live probe and can ship alone. The roles that get connectors (`orchestrator`, `strategist`) exist in v1, so no research role is needed (D-07). Slices 2 and 3 depend on third-party administrative steps (Slack workspace approval, a Zoom Marketplace app), so they are scheduled when the operator is ready and **are not on Step 4's critical path**; only slice 1 (host, client, login, Google) precedes Step 4.
- **Tests:**
  1. An unratified `[connectors.*]` edit leaves the connector off and `doctor` reports a pending proposal.
  2. A tool absent from `tools` is unavailable; a server that adds or changes a tool disables it until re-ratified.
  3. A `write` call after a connector read raises a Red card showing the content; `[a]` is refused.
  4. Connector results stamp L0; a note written afterwards stamps L0 until promoted.
  5. A callback with a wrong `state`, a second callback, or a late callback is rejected; tokens never appear in events, logs or errors.
  6. A request to a host outside `hosts`, including through a redirect, is refused and logged.
  7. A crash between a write call's approval and its result does not repeat the write (`action.intent`/`action.result`, `specs/005` §2.5). Remote tools have no idempotency keys, so an unresolved write is never retried: recovery asks the operator whether it happened (a connector may supply a verification query, such as searching sent messages by subject and time, to propose an answer).
  8. Revocation mid-task pauses with `re-authenticate`; no failover to another account.
  9. Each connector passes a recorded-fixture contract test, and a separate opt-in live probe on the operator's account.
  10. A tool name that collides with a built-in or another connector's tool is refused at registration. A call whose arguments exceed `max_arg_chars` or contain a registered secret is refused.
  11. A login whose provider identity differs from the ratified `account` is discarded. A callback with the wrong `Host` header is refused. A fixed `redirect_port` already in use stops the login.

## 11. Future Expansion
Other families (OpenAI, local) need only a working tool-call path. Further connectors (Microsoft 365, GitHub, Notion, Linear) are catalog entries plus a per-connector formatter. Multi-account support, an admin-managed shared catalog, MCP resources and prompts, and Delphi-as-MCP-server are post-v1.

### 11.1 Connector to-dos (official MCP servers confirmed 2026-10-24)
Both servers below were checked against the vendor's documentation and probed with an unauthenticated MCP `initialize` request (protocol revision 2025-11-25). Neither is built or ratified: each needs the §9 probes with a real login, a formatter, a tier assignment from the operator and its own exit criteria in §10 before it ships. Status: **to-do**, not scheduled; start on a recorded need (`delphi needs`).

| Connector | Server | Auth | Status |
|---|---|---|---|
| Atlassian: Jira and Confluence (one server, one login) | `https://mcp.atlassian.com/v2/mcp` (Streamable HTTP) | OAuth 2.1 with protected-resource metadata and dynamic client registration | to-do |
| Google BigQuery | `https://bigquery.googleapis.com/mcp` (Streamable HTTP) | Google OAuth, scope `https://www.googleapis.com/auth/bigquery` | to-do |

#### 11.1.1 Atlassian (Jira and Confluence)
- **Server:** Atlassian's hosted Rovo MCP server. `v2` is the default in Atlassian's client setup pages; `v1` (`/v1/mcp`, `/v1/sse`) still answers and is to be moved to v2 automatically on 2027-03-01, so build against v2. The unauthenticated probe returns `401` with `WWW-Authenticate: Bearer resource_metadata=…/.well-known/oauth-protected-resource/v2/mcp`, and `/.well-known/oauth-authorization-server` lists a `registration_endpoint`, so login can use discovery and dynamic registration (unlike Zoom, §7.5). The same server also covers Jira Service Management, Bitbucket, Projects, Goals and Loom; those are out of scope until asked for. Atlassian says it acts with the user's existing permissions, that MCP use consumes Rovo credits, and that admins have controls; the page does not say whether it is Cloud only.
- **Pinning problem to solve first:** by default v2 exposes two meta-tools, `discovery` and `execute`, which front the real tools. A pin on two generic tools approves everything behind them, which defeats §3. Use `?tools=all` (documented for gateways that need a flat, paginated list) so each real tool has its own definition hash, and refuse the connector if the server returns only the meta-tools.
- **Likely v1 tools (to confirm from the live listing):** search and read Jira issues, read a Confluence page and search Confluence (`read`); create or comment on a Jira issue (`draft`); transition, assign, edit or delete, and create or edit a Confluence page (`write`, Red).
- **Specific risks:** issue and page text is written by many people and is a prompt-injection vector (always L0); Confluence pages can be large (store as context pointers); a site-wide token reaches every project the user can, so a ratified project and space allowlist is wanted where the tools take one. Jira and Confluence may need separate role lists, which the catalog's per-tool tiers already allow.
- **Open questions:** Cloud only or also Server/Data Center; whether `?tools=all` is stable; whether the account pin (§4 item 10) can use Atlassian's profile endpoint; Rovo credit cost per call.

#### 11.1.2 Google BigQuery
- **Server:** Google's remote BigQuery MCP server, enabled with the BigQuery API on a Google Cloud project. Documented tools: `list_dataset_ids`, `list_table_ids`, `get_dataset_info`, `get_table_info` (all `read`), `execute_sql_readonly` (rejects DML, DDL and Python UDFs) and `execute_sql` (not read-only). IAM roles the signed-in user needs: MCP Tool User, BigQuery Job User and BigQuery Data Viewer. The probe showed `tools/list` answers without a login (metadata only); calls need OAuth. Limits per Google's page: a three-minute default query cancel, 3,000 rows per result, no Google Drive external tables, and no quotas beyond the BigQuery API's. The page does not say GA or preview.
- **Tiers:** metadata tools and `execute_sql_readonly` are `read`; `execute_sql` is `write` (Red) and off by default. The server marks each tool with `readOnlyHint`, but the catalog's tiers come from the operator, never from the server's hints (§3).
- **Cost:** a read-only query still runs a billed job in the operator's project. A `read` tier that approves without a card could spend money, so the connector needs a per-query size check (a dry run, if the server offers one) or an approval card showing the SQL, and the Delphi budget should count it. Decide this before ratifying.
- **Specific risks:** results are customer or business data (L0, PII in the event log is handled by §8), the SQL is model-written and can exfiltrate through result size, and a project or dataset allowlist is wanted.
- **Login:** the same Google OAuth client as Drive, Calendar and Gmail (§4), with the added BigQuery scope; adding a scope means a new consent, and the account pin applies.
- **Open questions:** GA status; dry-run support; whether the 3,000-row cap needs a paging strategy in the formatter; whether to prefer the local BigQuery MCP server (stdio, post-v1 transport) for operators who cannot enable the remote one.

## 12. Amendment 010-A2 (council 013 D23)
A draft is not harmless: a tainted span could otherwise create unlimited drafts, holds or meeting titles carrying data it read, covered by one earlier approval. So (1) **every draft call raises its own card** with the full payload; the "allow similar" cache and the auto-approving modes never cover a draft (stricter than hashing the payload into the pattern); (2) a draft from a span whose ceiling is **L1 or below is Red**, with `Action::ConnectorWrite` semantics (§5.2); (3) a draft whose arguments name recipients or invitees (`to`, `cc`, `bcc`, `recipients`, `attendees`, `invitees`) is Red; (4) Amber remains only for private unsent artifacts from untainted spans. Other tool classes are unchanged. Implemented in `connector_run.rs`.

## 13. Council 006, Round 1
Both seats (Gemini 3.8 Flash, red-team and strategist) returned `reject` on the first draft. This revision addresses every finding that held up: the taint-gate contradiction (§5.2, amendment 001-A3), the missing `[connectors.*]` pin (done in `lock::access_blocks`), tool shadowing (§2 item 2, enforced in the registry), loopback listener hardening and fixed ports (§4 item 11), account pinning (§4 item 10), argument exfiltration (§6.2), PII in the event log (§8), the dependency on Step 5 retrieval (§8 item 1), unresolved remote writes (§10 test 7) and phasing (§10). The five open questions were answered: (1) yes, read-mostly with Red writes; (2) separate payload store (§8); (3) loopback with a manual fallback and a fixed-port option, never a hosted page (§4); (4) the seats disagreed; the description stays pinned with a cheap diff card (§3); (5) a minimal in-tree client behind a trait (§2).

Round 2 (both seats still `reject`) turned on one point: it said the revision cited amendment 001-A3, `Action::ConnectorWrite`, the `[connectors.*]` pin and `Registry::register_external` as existing when they did not. They were on a different branch from the one the seats read; all four are now on `main` (PR #4) with tests. The new points that held up are folded in above: tainted-span query reads need an Amber card and a 128-character cap (§6), the `@payload/<hash>` scheme and quota (§8), `Sec-Fetch-Site`/`Origin` checks on the callback (§4.11), a promotion card that highlights injection-shaped text (§5.3), `first_use` to cut approval fatigue (§6) and a phasing that keeps slices 2 and 3 off Step 4's critical path (§10). Positions that stayed split are recorded in §2 item 6 (build vs adopt, resolved by a spike) and §3 item 2 (description pinning kept).
