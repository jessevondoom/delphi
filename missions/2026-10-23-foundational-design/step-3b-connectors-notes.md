---
id: step-3b-connectors-notes
title: "Step 3b Implementation Notes: the connector host (slice 1) and what is still open"
author: builder
timestamp: 2026-10-24T07:30:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [step-3b, connectors, mcp, specs-010]
---

# Step 3b, slice 1: the connector host

Everything in `specs/010` that does not need a live provider is built and tested against a scripted MCP server. The Google endpoints in the spec are "reported", not confirmed, and no Google OAuth client exists yet, so **no connector has been run against a real service**.

## What shipped

- **Catalog** (`delphi-adapter/src/config.rs`): `[connectors.<name>]` with `url`, `hosts` (exact names; the URL's own host must be listed), `login` (an `[auth.*]` block), `account`, `roles`, `tools = { name = "read" | "draft" | "write" }`, `max_arg_chars`, `first_use`, `calls_per_minute`, `max_result_chars`. Pinned with `[access]` (it was already in `ACCESS_KEYS`). A login without a pinned `account` is refused. `identity = "google"` needs `openidconnect.googleapis.com` in `hosts`.
- **MCP client** (`delphi-adapter/src/mcp.rs`): in-tree, Streamable HTTP, `initialize` / `tools/list` / `tools/call`, JSON and SSE replies, revision floor 2025-11-25, 8 MB response cap, redirects followed by hand only for 307/308 inside the host list. Behind an `McpTransport` trait so the spike against a library stays open.
- **Host** (`delphi-engine/src/connectors.rs`): sessions per connector; a tool is offered only if it is in the catalog, the role is listed, and its definition hash (name, schema, description) matches the **ratified pin**. A server that changes one tool loses only that tool until the operator re-ratifies. Pins are a normal proposal (`local/connectors/<name>.pins.json`, `connectors/<name>.pins.json` in the lock), so `/ratify` shows what is being pinned. After login the signed-in identity must equal `account` (checked before any call; a mismatch deletes the stored login).
- **Pipeline** (`connector_run.rs`): role check, allowed check, arguments bounded and screened for registered secrets, then by class: read = Amber the first time per task (or Green with `first_use = "allow"`), **and Amber every time, with the query on the card, once the span has read untrusted content**; draft = Amber; write = Red, never "allow similar", recorded as `action.intent` before sending and **never re-sent** if the outcome is unknown after a crash. Results are L0, redacted, kept in `payloads/<blake3>` (0600, 500 MB quota), and the event log holds only `content_ref` and the hash. Recovery resolves the reference (`recovery::rebuild_with`); an expired payload reads as "payload expired". `@payload/<64 hex>` is readable through `read_file`, nothing else.
- **Wire names** are `<connector>__<tool>`: some model APIs reject a dot in a tool name. Catalog and events use `<connector>.<tool>`. A built-in tool name never contains `__`, so a server cannot shadow one.
- **CLI**: `delphi connectors status` (offline), `test <name>`, `pin <name>`. `delphi doctor` check 7b reports connectors without ratified pins (warn) or with a tampered pin copy (fail).

## A consequence of the spec worth knowing

Every connector result lowers the span to L0, so after the first read in a task, any further call that carries a query asks the operator again (§6). Reading the next page with no arguments does not. This is the exfiltration defense working, and it will feel strict.

## Not done

- **No live run** against Google. `delphi connectors test <name>` is the first step once an OAuth client exists. Tool names, result shapes and the per-connector formatters (Docs and Sheets as Markdown and CSV, time zones and recurrences, MIME, quoted-history trimming) wait for that.
- **Slack and Zoom** (slices 2 and 3) wait for the operator's workspace approval and Marketplace app.
- **Promotion card** (§5.3) for notes written by a tainted span: the notes stay L0 until a later change adds the review and `provenance.stamp`.
- **Harness bridge** (§2.4): connector tools are offered to API-path spans only. A span served by a subscription CLI does not get them yet.
- **`delphi auth login <connector>`**: login is `delphi auth login <block>` for the `[auth.*]` block the connector names. Loopback hardening beyond what `oauth2.rs` already does (Host header, Sec-Fetch-Site) was not re-audited for the connector case.
- `connector_result_retention_days` and the `gc` command: `gc_payloads` exists and is tested by hand only; nothing calls it yet.
- The strategist's spike (adopt an MCP library or keep the in-tree client) was not run; the trait keeps it reversible.
