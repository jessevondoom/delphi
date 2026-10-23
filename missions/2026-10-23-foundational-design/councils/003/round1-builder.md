---
id: council-003-round1-builder
title: "Council 003 Round 1: Builder seat (blind)"
author: builder
timestamp: 2026-10-23T20:10:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-003, verbatim]
model: "Claude (this session)"
---

# Council 003, Round 1 (blind): Builder seat (Claude)
Focus: can each item be built and tested in its stated step? *(This seat drafted most of the material under review; the critics should weigh that.)*

## Findings
- **[P1]** `specs/009#L155`: **Hostname allowlists aren't something the OS sandboxes can do.** `sandbox-exec` network rules filter by address or port, not hostname, and `bubblewrap` only offers all-or-nothing network namespaces. "Network allowed only to its provider's hosts" can't be built as written.
  - **Remediation:** give the harness no direct network. Route it through a daemon-run local CONNECT proxy (`HTTPS_PROXY` pointing at a Unix socket or localhost port), which allows only the endpoint's configured hosts by SNI/CONNECT target and logs every denied connection. Test 7 then checks the proxy log.
- **[P1]** `specs/009#L155`, `#L139`: **A harness needs its own state directory.** `claude`, `gemini` and `codex` read their login and write session and cache files under the home directory. A sandbox allowing writes "only to its worktree" breaks login and sessions.
  - **Remediation:** per harness, allow read and write access to exactly that CLI's own state directory (declared in a per-harness profile in config) and nothing else under `$HOME`. Delphi itself still never reads those files (§4, invariant 2).
- **[P1]** `specs/009#L160`: **Prompt injection into a harness is unspecified.** "Through the CLI's system-prompt mechanism" differs per CLI (an append-system-prompt flag, a context file, an instructions file). A context file written into the worktree could also be committed and merged.
  - **Remediation:** add a per-harness injection table (flag or file, path, size limit). Any injected file is written outside the tracked tree, or is listed in the worktree's `info/exclude` and is never part of a merge diff. Test that a merge diff never contains it.
- **[P1]** `specs/009` §4 (`oauth2` row): **The OAuth config is missing its required fields.** Device-code and client-credentials flows need `issuer` (or the token and device endpoints), `client_id`, `scopes` and an `audience`, and Delphi can't ship a universal `client_id` for corporate identity providers.
  - **Remediation:** specify the `[auth.<name>]` schema. `client_id` is supplied per organization. Client secrets for client-credentials go only to the keychain.
- **[P2]** `specs/009#L106` vs `specs/002` §4: **Two model maps.** Tier→model mappings exist both globally (`[models]`) and per endpoint.
  - **Remediation:** the endpoint mapping takes precedence, and the global map is only a fallback for endpoints that omit one.
- **[P2]** `specs/009` §6: **Probing costs money and quota, and harness probing launches the CLI.**
  - **Remediation:** define a minimal probe suite, a per-probe token cap, a cache TTL for results (e.g. 24 h), and a rule that `doctor` uses cached results unless `--probe` is passed.
- **[P1]** `specs/005#L167`: **"Retry from the last completed block" doesn't work with reasoning models.** You can't send a partially streamed assistant turn (for example a complete thinking block plus a truncated text block) as a continuation. With thinking enabled, the provider rejects it or the signature chain breaks.
  - **Remediation:** discard the entire interrupted turn and retry the whole turn from the last *completed turn*.
- **[P1]** `specs/005#L167`: **Providers can't report usage for an interrupted call after the fact.** There's no API to look up an interrupted request's usage later, so "reconciled from the provider's report if the call completed server-side" can't be built, and releasing the reservation undercounts spend.
  - **Remediation:** an interrupted call with unknown usage is charged its full reservation (`usage_source = estimated`, `outcome = cancelled`). That is conservative, but the budget can never be silently exceeded.
- **[P1]** `specs/004#L207`: **The memory budget doesn't add up.** Per-file caps allow 6 KB each for the release, team, operator and workspace heuristics, plus 2 × 6 KB of lessons, plus a 1 KB profile, well over the 12 KB combined cap. Release and team rules alone can fill it, leaving no room for the operator scope that D-20 exists to serve.
  - **Remediation:** allocate the 12 KB per scope, for example release 4 KB, team 3 KB, operator 3 KB (rules, lessons and profile), workspace 2 KB, with unused allocation passing to the next more specific scope. Trim within a scope by rule score.
- **[P1]** `specs/004` §5.4: **Promotion can leak one client's details into another's workspace.** A lesson promoted from a confidential client workspace to operator scope ("client A's API rejects X") gets loaded into client B's workspace.
  - **Remediation:**
    - Operator-scope proposals must be project-agnostic: the deterministic linter rejects source-workspace identifiers (repo names, hosts, paths, domain terms listed in the workspace's `config.toml`).
    - Orchestrator generalizes the wording.
    - A workspace can be marked `isolated = true`, which means nothing is promoted out of it.
- **[P2]** `specs/001` §6A: **The operator files shouldn't be readable through file tools at all.** The amendment says they're "read-only for agents; loaded through the snapshot". Since they arrive only through the snapshot, file-tool read access adds nothing, and it lets a low-taint span quote private profile text into artifacts.
  - **Remediation:** put `$DELPHI_HOME/operator/**` on the never-readable list. It reaches agents only through the snapshot.
- **[P2]** `specs/004` §5.6: every rule of a role is loaded in every task of that role, so "tasks ran with it loaded" has no variation and gives no causal signal. The evidence is before/after only, and that should be said plainly, along with a note that rules ratified together can't be told apart.

## Open question: operator permission-widening
**Recommend: allow it by explicit grant, with hard limits.**
- **Limits:** `delphi customize grant <role> <scope>` is Red-tier and shows the permission diff. It's allowed only in operator or workspace `core/`; **team overrides stay narrow-only**, so a teammate can never widen *your* agents. It can never cross the hard invariants: no `.git`, no daemon internals, no governance or `REVIEW.md` writes, no outbound action without Red approval, no change to the taint table. It's recorded as a waiver in `ratified.lock`, and `doctor` lists every active grant.
- **Why not absolute narrow-only:** it contradicts operator sovereignty (GOVERNANCE §1.1), and in practice operators would fork built-ins to get around it.
- **Risks of allowing it:** approval fatigue leads to rubber-stamped grants; mitigated by an optional expiry and the `doctor` listing.

## Checked and sound
- Endpoint failover confined to `allowed_endpoints`, including when every allowed endpoint is down.
- Credential invariants 1–3 (nothing on disk, no foreign credential reuse, never in agent context).
- Write-ahead `action.intent`/`action.result` with idempotency checks; no blind retry.
- Re-raised approvals on recovery; Round 1 rerun on recovery.
- The usage ledger rebuilds from events.
- Narrowest-default scope routing and the explicit operator→team step.

## Recommendation
- **009:** `approve-with-fixes` (proxy, harness state dirs, injection table, OAuth schema).
- **005 §2.4–2.5, §4:** `approve-with-fixes` (whole-turn retry, charging the reservation).
- **004 §5:** `approve-with-fixes` (scope budget, cross-client leak).
- **001-A1 / 006-A1:** `approve-with-fixes` (never-readable operator dir).

`concur: true` (no P0 found by this seat).
