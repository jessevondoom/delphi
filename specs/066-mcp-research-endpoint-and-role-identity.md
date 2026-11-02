---
id: spec-066-mcp-research-endpoint-and-role-identity
title: "066: MCP research endpoint for host agents, and Delphi role identity in answers"
author: architect
timestamp: 2026-11-01T17:55:00Z
status: approved   # approved by the operator on 2026-11-01 ("approve the spec and start building"); D-1..D-6 were given by the operator the same day
approved_by: operator (2026-11-01)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, mcp, host-agents, research, scout, validator, library, trusted-sources, identity, badge]
---

# Spec 066: Research endpoint and role identity for host agents

> **Status: approved by the operator, 2026-11-01.** Amends [`specs/062`](062-delphi-as-host-agent-mcp-server.md) (§2 principle 2, §6.3, §9). Uses [`specs/052`](052-knowledge-status-capture-on-contact-and-the-digest.md) (status vocabulary, capture on contact), [`specs/055`](055-the-central-library-sources-claims-and-pages.md) (library), [`specs/028`](028-knowledge-window.md) §10.3 (trusted lists), [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md) (retrieval) and [`specs/005a`](005a-terminal-avatar-system.md) (badges). Reviewed by a three-seat council (architect, investigator as red team, scout) on 2026-11-01.

## 1. Scope & Objectives

1. **Research for host agents.** A host asks Delphi to research a question. The answer comes back with cited claims, and what was learned is captured so the knowledge base grows and agents improve over time (§3-§6).
2. **Identity in answers.** Every answer that Delphi produces says which role gave it, with the one-line badge and the coloured name with role in parentheses, as the console shows them (§7).

### 1.1 Non-Goals

- Raising any claim's status by use, or by the source being trusted (§5).
- Background crawling, embedding-only confidence scores, automatic promotion and multi-agent debate on research.
- Host write access to the project (062 §6.3 stays).
- Per-call quotas (062 §9 stays), other than the concurrency cap in §6.

## 2. Decisions (operator, 2026-11-01)

- **D-1.** Scout and validator may use the network for host-raised research.
- **D-2.** Trusted sources are searched first, with a bias, and carry operator-granted *source policy* (§5).
- **D-3.** Identity goes out as both plain text and a styled variant; the default is plain; the host agent chooses.
- **D-4.** Captured sources enter the library as `not validated`, automatically. Claims are staged by capture on contact (052).
- **D-5.** Research jobs are capped at **2 running at once**; further requests queue. There is **no upper limit** on the number of requests.
- **D-6.** Everything else in the council's recommendations is adopted (§8).

## 3. Tools

All three are on the host socket (062 §3.2) and are refused when `[mcp] enabled = false`.

| Tool | Purpose |
| :-- | :-- |
| `request_research({ question, depth?, want?, context_pointers? })` | Starts a job and returns `research_id` and `status: queued\|running` at once. `depth` is `quick` (default), `survey` or `deep`; `want` is `answer`, `sources` or `both`. |
| `research_status({ research_id })` | State, step count, `budget_used`, the queries sent and URLs fetched or refused. Same session only. |
| `research_result({ research_id, wait? })` | The result (§4). `wait` is bounded (at most 25 seconds), as `help_result`. |

The question is untrusted content at the host's tier (L0), fenced in the prompt (062 §2 principle 3). Context pointers are named, not read by the host server.

## 4. Result

```json
{ "status": "done|partial|failed|cancelled",
  "answer": "...", "claims": [ { "id", "assertion", "source_ids", "quote", "locator", "retrieved", "licence", "knowledge_status", "contested": false } ],
  "citations": [], "source_ids": [], "kb_hits": [], "new_claims": 0,
  "contradictions": [], "freshness": { "retrieved": "...", "last_checked": "..." },
  "budget_used": { "steps": 0, "ms": 0 },
  "answered_by": "scout", "consulted": ["scout", "validator"],
  "identity": { }, "signature": "..." }
```

- `answer` is at most 8000 characters, redacted, and labelled `not validated` model synthesis. Raw page text is never returned.
- `new_claims` counts deduplicated staged candidates, not search hits. A repeated question reports `new_claims: 0` and its `kb_hits`.
- Conflicting claims are both returned, each marked `contested`; they are never averaged.
- `status: partial` is returned when a depth budget runs out. There are no hidden retries.

## 5. Knowledge status and trusted sources

- **Search order:** `search_kb`, then library claims and sources, then trusted-domain queries, then the wider web, and only when trusted coverage is thin. At most 3 planned queries: the topic, a trusted-constrained one, and one gap or contradiction query.
- **Depth budgets** (hard): `quick` about 5 steps; `survey` about 15; `deep` about 30.
- **Trusted membership is a source policy, not claim trust.** Entries in `context/sources/trusted.toml` and the operator's `operator/trusted-sources.toml` give: a ranking bias, automatic capture (no staging step), and a validator fast-lane. They never change the claim's status or tier.
- **Status mapping (052):** every captured claim starts `noted`, tier L0. `checking` only once the validator has it queued. `validated` only from a validator record or operator ratification. The validator cannot write trust metadata. The UI shows "from trusted source" as provenance.
- **Library:** the daemon, never an agent, captures sources through the validation fetcher (https only, no private addresses, size cap, cloaking check), licence-gated, marked `not validated` (055). Fetches use `TrustedList::allows_url`; no second check is written.
- **Dedupe:** canonical URL, content hash and claim fingerprint (subject, predicate, object, scope and time). More evidence for a known claim merges citations.
- **Freshness:** a source last checked more than 30 days ago is re-checked once on open (055); a changed source becomes a new version.
- **Learning loop:** a result may emit one `propose_note` candidate (query, outcome, evidence, failure modes). The operator ratifies it. Research prose is never promoted on its own.

## 6. Authority and safety (amends 062)

- 062 §2 principle 2 and §6.3 are amended for research only: the scout and validator receive **separate egress scopes**, each limited to the validation fetcher and the search provider, with no connector tools, no writes by the agent and no command execution. The validator does not inherit the scout's scope. `exec_tool` refuses any tool the span was not offered.
- **Concurrency cap of 2** running research jobs per daemon; the rest queue in order. No other limit on count (D-5). Each job has a timeout, can be cancelled, and writes an audit record (`host.research.requested`).
- **Query privacy.** Host context is never added to a search query automatically. Queries are redacted for secrets, paths and private addresses and are listed in `research_status`.
- **Content, not commands.** Fetched text is fenced evidence. It cannot call tools, raise trust or choose scope. Source trust never alters claim taint.
- **Trusted list.** Operator-only edits. A wildcard entry cannot match a blocked or unrelated host.
- A research job narrowed this way raises no card and approves nothing. Red actions still always ask.

## 7. Role identity

- The daemon builds `identity` from the avatar library and theme pack that the console uses (one builder, never a second renderer): `{ role, display_name, badge, hex, ansi256, host? }`. Role IDs only in code; the display name comes from the theme pack at run time.
- `badge` is the static one-line frame (`⟦•_•⟧`), so it obeys reduce motion. `NO_COLOR` forces plain.
- **Both variants are sent.** `signature` is plain text: `⟦•_•⟧ Name (role)`. `styled` is an ANSI variant, generated server-side from an allow-list. `[mcp] badge_style = plain|ansi|markdown` sets which one `signature` carries (default `plain`); the other stays in `identity.variants`.
- All identity strings are sanitised: ESC, OSC, other control characters and newlines are removed. Fetched text and model output never enter `identity`.
- Applied to `request_help`, `request_council`, `council_result`, `research_result` and any other answer by a Delphi role. When a host adopts a role with `get_agent`, its signature carries `via <host>`.
- The badge is attribution, not proof of identity, and grants no authority.
- **Teaching the host:** the MCP `instructions` string, a plugin skill `delphi-attribution` (from `delphi plugin export`), and the `--print-config` snippet each say: put `signature` first in any answer that came from Delphi, and prefer `styled` only if the host renders it. Encouraging research uses the same channels: a `search_kb` miss suggests `request_research`, and the skill says to research before guessing.

## 8. Slices (one per pull request)

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| S0 | this spec; tracker entry | |
| S1 | claim, status, research-result and identity schemas; status mapping | S0 |
| S2 | daemon job lifecycle, queue (cap 2), egress scopes, audit | S1 |
| S3 | planner, depth budgets, KB and library dedupe | S2 |
| S4 | validator integration, contradiction records, trusted fast-lane | S3 |
| S5 | `request_research`, `research_status`, `research_result`; identity builder on every answer | S2 |
| S6 | `badge_style` config and schema, `delphi-attribution` skill, `instructions`, `--print-config` | S5 |
| S7 | generated docs, golden `tools/list`, tracker | S6 |
| S8 | security and red-team pass over every new egress, write and identity path | all |

## 9. Authority invariants (what the tests prove)

1. A malicious fetched page cannot cause a tool call, a write, a trust change or an outbound request outside the allow-list.
2. A query containing a secret or private host is redacted before the transport sees it.
3. A claim from a trusted source stays `noted` and L0; no code path raises it.
4. A wildcard trusted entry cannot authorise a blocked or unrelated host.
5. A name or fetched string containing ESC, OSC or newline cannot alter `signature`.
6. A third research request while two run is queued, not refused, and a flood never raises an error.
7. Duplicate URLs and claims create one record.
8. The validator's scope cannot fetch what the scout's scope would not, and cannot write trust metadata.
9. With `[mcp] enabled = false`, every research tool answers `disabled_by_operator`.

## 10. Acceptance criteria

- A host can run `request_research`, poll, and receive a cited result with `identity` and `signature`.
- A repeated question reports `new_claims: 0` with `kb_hits`.
- `plain` is the default; `ansi` and `markdown` are selectable; both variants are present in the result.
- `cargo test --workspace --locked`, clippy with `-D warnings` and `delphi lint` pass; `docs/reference/` and schemas are regenerated.

## 11. Known limits

- A host may not render ANSI or may omit the signature; the signature is a request to the host, not a guarantee.
- Research depends on a search provider being configured; with none, trusted-domain fetches of known URLs still work and the result says so.
- Claim fingerprinting catches the same assertion, not every paraphrase.
