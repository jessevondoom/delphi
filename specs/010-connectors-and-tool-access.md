---
id: spec-010-connectors-and-tool-access
title: "010: Native Connectors (MCP) & the Tool Access Layer"
author: architect
timestamp: 2026-10-24T16:00:00Z
status: draft   # needs a Council and operator approval before any code (D-23)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # provider facts in §7 come from secondary sources and must be probed (§9)
source_mission: 2026-10-23-foundational-design
tags: [spec, connectors, mcp, auth, tools]
---

# Spec 010: Native Connectors (MCP) & the Tool Access Layer

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
2. **Tools are namespaced** `<connector>.<tool>` (for example `slack.search_messages`) and offered to a span only if the span's role is on the connector's ratified `roles` list (§3).
3. **One pipeline.** Every connector call passes through the same steps as `read_file` and `write_file`: classify → taint gate → approval if needed → execute → persist. There is no second code path.
4. **Harness bridge.** When a model runs through a harness (`specs/009` §7), connector tools reach it through the per-span MCP socket of `specs/009` §7.6. The harness sees them as Delphi's tools; its own connectors stay off (§7 item 5 stands).
5. **Transports:** Streamable HTTP (remote servers; all five v1 connectors) and stdio (local servers, later). The client negotiates the protocol version and pins a minimum (initially the 2025-11-25 revision).

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
roles = ["orchestrator", "strategist"]          # canonical role IDs; never a Council seat
hosts = ["mcp.slack.com", "slack.com"]          # exact names; no wildcards
tools = { "search_messages" = "read", "read_channel" = "read", "post_message" = "write" }
```
1. **`tools` is Delphi's own allowlist and classification.** A server's tool annotations and descriptions are untrusted. A tool the server offers that is not listed is invisible to agents. A listed tool is `read` or `write`, as the operator ratified.
2. **Definition pinning.** At ratification Delphi records a hash of each allowed tool's name, input schema and description. If the server later changes any of them (a "rug pull" or a poisoned description), the tool is disabled and `doctor` reports a pending re-ratification.
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

## 5. Trust and Taint
1. **Everything a connector returns is external content.** Results are stamped `external_untrusted` (L0) and lower the calling span's ceiling like any other untrusted read (`specs/001` §5.1). v1 never stamps connector content higher, even from the operator's own mailbox: other people wrote most of it.
2. **Consequences follow the existing table** (`specs/001` §5.2): a span at L1 or below has no mutating, outbound or dispatch tools without a Red approval. Connector writes are outbound actions (§6).
3. **Promotion.** Notes written by a tainted span are L0. To use them in later work, the operator reviews the note in the console and approves it; Delphi then stamps that file's hash at L4 through the same path ratification uses for files today (`provenance.stamp`). This keeps research usable without letting a tainted span launder content into a trusted artifact.
4. A child span inherits its parent's ceiling, and a parent inherits what its child read (Step 2 round-1 fix).

## 6. Tool Tiers and Approvals
| Tool class | Tier | Rules |
| :--- | :--- | :--- |
| `read` | Amber on first use per task per connector, then Green for that task | The card names the connector, the account and the scopes. `[a]` allow-similar is task-scoped (`specs/005` §3.1). |
| `write` that creates a private draft (for example a Gmail draft, a calendar hold with no invitees) | Amber | Shows the full payload. |
| `write` that sends, posts, shares, invites or deletes | **Red** | Never auto-approved by any mode. The card shows the full payload, bound to its hash (`specs/005` §3.1). `[a]` is refused. |

1. **Read-then-write guard.** A span that has read connector content cannot make any connector write without a Red approval, and the card shows the content that will leave. This is the exfiltration defense: the agent can't quietly post what it read.
2. **Arguments are logged.** The exact arguments of every call are persisted (§8), so a search query carrying data out is visible after the fact.
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
1. **Large results become context pointers.** A result over the per-call cap is stored under the mission and the model receives a bounded excerpt plus a pointer (`specs/004` §3 retrieval), not the whole payload.
2. **Event log content.** Recovery needs the result of a finished call (`specs/005` §2.5), so results are persisted in `events.jsonl` after redaction. Because they hold private mail and chat, a ratified `connector_result_retention_days` (default 30, like thinking retention, D-16) lets `gc` blank bodies of closed missions while keeping hashes and metadata.
3. **Events:** `connector.call` (connector, tool, class, arguments, account), `connector.result` (size, truncation, hash), `connector.denied`, `connector.reauth_required`. The usage ledger records calls per connector.
4. **The model provider sees what the agent reads.** Content a connector returns is sent to whichever model endpoint runs the span. The first-use card for each connector says so, and `[workspace] isolated` and `allowed_endpoints` already bound where that can go.

## 9. Probing and `doctor`
`delphi connectors test <name>` and `doctor` check, per connector: reachability, a successful token refresh, that each ratified tool exists with the pinned definition, that no unlisted tool is exposed, rate-limit headers, and that read tools return without write scopes granted. Probes use read-only calls on the operator's own account. **The five connectors' endpoints, scopes and registration requirements in §7 are assumptions until these probes pass.**

## 10. Phasing and Tests
- **Step 3 (already planned):** `oauth2` device code and client credentials; keychain token store; tool-registry refactor so tools are data, not a match statement.
- **Step 3b (new, before Step 4):** `ConnectorHost`, the MCP client, loopback PKCE login, the ratified catalog and pinning, the promotion card, then connectors one at a time in the order Slack, Google (Drive, Calendar, Gmail share one login), Zoom.
- **Tests:**
  1. An unratified `[connectors.*]` edit leaves the connector off and `doctor` reports a pending proposal.
  2. A tool absent from `tools` is unavailable; a server that adds or changes a tool disables it until re-ratified.
  3. A `write` call after a connector read raises a Red card showing the content; `[a]` is refused.
  4. Connector results stamp L0; a note written afterwards stamps L0 until promoted.
  5. A callback with a wrong `state`, a second callback, or a late callback is rejected; tokens never appear in events, logs or errors.
  6. A request to a host outside `hosts`, including through a redirect, is refused and logged.
  7. A crash between a write call's approval and its result does not repeat the write (`action.intent`/`action.result`, `specs/005` §2.5).
  8. Revocation mid-task pauses with `re-authenticate`; no failover to another account.
  9. Each connector passes a recorded-fixture contract test, and a separate opt-in live probe on the operator's account.

## 11. Future Expansion
Other families (OpenAI, local) need only a working tool-call path. Further connectors (Microsoft 365, GitHub, Jira, Notion, Linear) are catalog entries plus a per-connector formatter. Multi-account support, an admin-managed shared catalog, MCP resources and prompts, and Delphi-as-MCP-server are post-v1.

## 12. Open Questions for the Council
1. Is a v1 limited mostly to reads, with only Slack post, Gmail send and Calendar events as Red writes, the right balance?
2. Should connector results be persisted in the event log at all, or kept in a separate encrypted store with only hashes in the log?
3. Is the loopback redirect acceptable for every provider's rules, or must some use a hosted redirect page (which Delphi cannot ship without running a service)?
4. Should tool-definition pinning cover the full description text or only name and schema, given providers revise descriptions often?
5. Build the MCP client on an existing Rust crate or write a minimal one (the surface is small: initialize, list tools, call tool, Streamable HTTP, OAuth discovery)?
