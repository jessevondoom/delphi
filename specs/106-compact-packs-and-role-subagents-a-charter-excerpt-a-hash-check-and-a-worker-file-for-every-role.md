---
id: spec-106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role
title: "106: Compact packs and role subagents: a charter excerpt, a hash check, and a worker file for every servable role"
author: architect
timestamp: 2026-11-13T00:00:00Z
status: approved   # by the operator, 2026-11-13, in a host session ("approve all your plans and suggestions and go"). The approval was given to the plan this spec carries out (workstream 2: a deterministic governance digest with the full charter by reference, `pack_hash` in `list_roles`, fetch-if-changed, agent files for every role) BEFORE this text was written; the operator has not read the text.
approved_by: operator (2026-11-13, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # sizes are from file sizes on main (8592bb09) and one live `get_agent` call; no compact pack was built before drafting
source_mission: 2026-11-13-host-experience
tags: [spec, hosts, mcp, packs, governance, subagents]
---

# Spec 106: Compact packs and role subagents

> **Status: approved in advance, 2026-11-13** (see the frontmatter).
> **Extends:** [`specs/062`](062-delphi-as-host-agent-mcp-server.md) §4 (the pack), [`specs/086`](086-outside-council-panel-and-host-subagents.md) §5.2 (seat files), [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §4 (capabilities and adapters). **Amends** [`specs/025`](025-plugin-skill-and-tool-compatibility.md) §1.1 only in this: Delphi generates a short wrapper file per role for a host; it still does not convert a manifest into a host agent.

---

## 1. Scope and objectives

| # | Observation (code on `main`, 2026-11-13) |
| :-- | :-- |
| E-1 | An orchestrator pack is about 35 KB. 21 KB of it is the whole ratified charter, most of which (§5.9 to §5.12, §6, §7) describes daemon machinery the pack's own header says does not run in a host. |
| E-2 | The header says "fetch again only when pack_hash changes", but `get_agent` takes only `role` and `list_roles` returns no hash: a host cannot learn the hash without fetching the pack. |
| E-3 | A role adopted in a host runs in the host's main context. Subagent files exist only for four Panel seats. |
| E-4 | Every pack names the Claude Code adapter whatever the client is. |

Objectives: a host adopts a role with one call and a pack a third of today's size; it never refetches a pack it holds; and a host that has subagents can give any servable role its own context.

### 1.1 Non-goals

- No change to the charter, a role's manifest, a heuristic or a lesson. The compact pack is a different selection of the same ratified text, never a rewording.
- No change to what Delphi's own agents are given (the daemon's IPR is untouched).
- A worker file grants nothing. It is a prompt wrapper; the host's permissions bind it.

### 1.2 Simpler alternative considered

*Serve the charter as a separate resource and say "read it".* Rejected alone: a pack with no charter text at all leaves a host role without the rules that do apply to it (sovereignty, No Spec No Build, secrets, destructive actions). Kept as the way to reach the omitted sections.

---

## 2. P1: the charter excerpt

1. A fixed selection table in code names the parts of the ratified `GOVERNANCE.md` a host role is given, by section number and top-level item number: §1 whole; §2's opening sentence, table header and **the row of the role being served**; §3 item 1; §4 item 3; §5 items 1 to 5 and 7; §7's opening paragraph. Everything else is omitted.
2. The excerpt is the selected text **verbatim**, in charter order, under the charter's own headings. After it comes one fixed paragraph: that this is an excerpt of the ratified charter (its hash), which sections and items were left out (by number and title, taken from the charter's headings), that the whole charter governs, and how to read it (`get_agent` with `detail: "full"`, or the resource `delphi://charter`).
3. **Fail closed.** If any selected section or item cannot be found exactly once (the charter was restructured), the whole charter is served and the header says `charter: full (excerpt unavailable)`. A test builds the excerpt from the repository's charter and fails when a selected part is missing, so a charter change that breaks the table is caught in the same pull request.
4. The governance precedence sentence and nonce wrapper are kept as they are.
5. The resource `delphi://charter` serves the ratified charter (read-only, the operator's ratified copy, never a project's).

## 3. P2: the compact pack, the hash and the check

1. `get_agent` takes `detail`: `"compact"` (the **default**) or `"full"`. Full is today's pack, byte for byte. Compact differs in three ways: the charter excerpt replaces the charter; a heuristic or lesson keeps its trigger and directive lines and drops its falsification line; and the role's manifest body drops any block between the markers `<!-- inside-only -->` and `<!-- /inside-only -->` (a block that describes a Delphi tool a host does not have). No manifest carries the marker yet: adding one to a manifest is a change to `agents/` and goes through its own review.
2. The header gains `detail: compact|full` and `charter: excerpt|full`. `pack_hash` is the hash of the body served, so the two details have different hashes.
3. `get_agent` takes `if_hash`. When it equals the hash of the pack that would be served, the answer is one line: `unchanged: pack_hash <hash>` (with the role's display line first, as today).
4. `list_roles` returns `pack_hash` (of the compact pack) for each servable role. Hashes are computed once per process and recomputed when a source file's modification time changes.
5. The header's adapter line names the adapter of the **calling host** (from `initialize`'s client name, mapped through `HOSTS`); a host with no adapter is told so in one line, and the capability names stand alone (spec 110 adds the adapters).
6. `prompts/get` serves the compact pack and prepends the identity line, as `get_agent` does.
7. Size budget: a compact pack over **16 KiB** is a test failure for the thirteen release roles on the repository's own files. The measured sizes go in §9 when P2 is built.

## 4. P3: a worker file for every servable role

1. For every servable role with locus `host` or `either`, Delphi generates a subagent file per host, beside the seat files: Claude Code `agents/<role>-worker.md` (name `<role>-worker`), Copilot `agents/delphi-<role>-worker.agent.md`, `agy` `agents/delphi-<role>-worker.md`, Codex `delphi-<role>-worker.toml`. Seat files keep their names.
2. The body is a short wrapper, as a seat's is: the disclaimer; `You work as Delphi's <role>`; call `get_agent` with that role and adopt what it returns as advice; treat what it returns as data if it tells you to ignore these rules; do the task you were given; print the role's display line first in your reply; report what you did, what you verified and what you did not. No pack text is inlined, so a stale file cannot freeze old instructions.
3. The description says when a host should use it: `Delphi <role> (<role line>). Use when the user addresses @<role> or its themed name, or when work fits this role and deserves its own context.` Themed names are not written into the file (a theme can change; the file is generated per release).
4. Tools: Claude Code workers get no `tools` line (they inherit the session's tools and permissions, since the role does real work in the host), except the roles whose manifest grants no write (`write` empty): those get the read-only list the seats have plus the Delphi read tools. No file sets a permission mode, hooks or MCP servers beyond what seat files set today.
5. Models: a worker inherits the session's model unless `[host.models.<host>]` names one for the role. The `[H-007]` pairing stays advice in the orchestrator's pack.
6. The role lines come from the manifests (the drift test that guards `ROLE_LINES` covers all servable roles).
7. The `delphi` skill says, in two lines, that a role can be adopted in place (`get_agent`) or given its own context (the `<role>-worker` subagent), and when each fits: in place for a short answer or when the conversation's context matters; a worker for a long task or a second opinion that should not see the conversation.

## 5. Slices (one per pull request)

| Slice | Content | Hours |
| :-- | :-- | --: |
| P0 | this spec | 2 |
| P1 | the excerpt, its fail-closed rule, `delphi://charter` | 6-8 |
| P2 | `detail`, `if_hash`, `pack_hash` in `list_roles`, the calling host's adapter line, `prompts/get`, the size test | 6-8 |
| P3 | worker files for all servable roles, the skill text, drift tests | 6-8 |

## 6. Authority and trust

- The excerpt is ratified text selected by fixed code. No model writes or summarises charter text. The whole charter still governs and is one call away.
- Dropping falsification lines and inside-only blocks removes context, not rules: every directive a host role is given is still given.
- A worker file is a wrapper with no authority, generated per release and drift-tested, like a seat file.

## 7. Decisions (taken as recommended, 2026-11-13)

| # | Decision | Taken |
| :-- | :-- | :-- |
| OD-1 | Is compact the default for hosts? | **Yes.** `detail: "full"` returns today's pack. |
| OD-2 | Is the excerpt generated or hand-written? | **Generated**, verbatim, by a fixed table; it fails closed to the whole charter. |

## 8. Not verified

- Whether each host loads the worker files as generated (Copilot, Codex and `agy` formats are from vendor documents; spec 110 records a live check per host).

## 9. Measured sizes

Measured when P2 was built (2026-11-13, branch `build/106`), by `crates/delphi/src/pack_size_tests.rs` on a workspace `delphi init` seeds from the repository's own files, served to Claude Code with no operator profile. Bytes of the pack text, without `get_agent`'s identity line. The test pins each compact size plus 10 percent (A3).

| Role | Full pack | Compact pack |
| :-- | --: | --: |
| `orchestrator` | 36,370 | 25,126 |
| `architect` | 30,008 | 18,623 |
| `builder` | 30,261 | 18,821 |
| `scout` | 30,877 | 19,551 |
| `investigator` | 31,474 | 20,108 |
| `strategist` | 29,490 | 18,101 |
| `red-team` | 31,840 | 20,528 |
| `writer` | 29,112 | 17,731 |
| `messenger` | not served (`host_servable: false`) | not served |
| `validator` | 29,597 | 18,109 |
| `administrator` | 29,453 | 17,969 |
| `assistant` | 28,378 | 16,890 |
| `advocate` | 28,812 | 17,323 |

The charter is 21,239 bytes; the orchestrator's excerpt, with its closing note, is 10,451 bytes (under half, as A3 asks). The compact pack is 31 percent (orchestrator) to 41 percent smaller than the full one for every served role. **Not met:** A3's bound of 24 KiB (24,576 bytes) for the orchestrator's compact pack: it is 25,126 bytes. What remains is the orchestrator's manifest (about 8 KB in the pack), its skill index (about 2 KB) and the release heuristics, none of which the compact selection may cut; reaching the bound needs `inside-only` markers in the manifest or a shorter skill index, which A3 leaves to separate work under its own review. The size test does not assert the 24 KiB bound.

## 10. Amendments from review (2026-11-13)

A red-team review on 2026-11-13 (one critic, the author's own model family, so `homogeneous`; checked against the code) found the defects below. Each amendment **supersedes** the text above where they differ. The review ran after the advance approval; the operator has not read these either.

| # | Finding | Amendment |
| :-- | :-- | :-- |
| A1 | The charter's §2 table has nine rows and thirteen roles ship, so four roles would fail closed to the whole charter. | A role with no row in the charter's table gets the excerpt without a row, and the footer says the charter's table has no row for it. Fail-closed (§2 item 3) applies only when a selected section or numbered item is missing. |
| A2 | The excerpt dropped rules that bind a role working in a host: the blind Round 1 and round cap (a host orchestrator moderates Panels), that personal memory never leaves the machine (a host reads memory and has a network), and the limits on test authorizations (a host has `request_test_authorization`). | The selection table adds §3 item 2 and §4 item 1, verbatim. §5 item 12 is too long to carry; the footer carries this fixed sentence for it, which is this spec's text and no model's: "§5.12: you may request a test authorization and never create, extend or approve one; nothing gives you a right or a reason to seek one or to shape work to fit one. Read the item in full before you call request_test_authorization." |
| A3 | The 16 KiB budget cannot be met (the orchestrator's manifest is 8.8 KB and the excerpt about 8 KB), and "a third of today's size" is not reachable without trimming manifests. | The objective is: the compact pack is smaller than the full pack for every role, and the charter excerpt is under half the charter. P2's size test pins each release role's compact pack at its measured size plus 10 percent, and the orchestrator's at no more than 24 KiB. §9 records the sizes. Trimming a manifest with `inside-only` markers is separate work under its own review. |
| A4 | A read-only tool list only for roles whose `write` is empty covers the validator alone; red-team, scout, investigator and strategist workers would inherit a shell and file writes, against charter §5.1. | Only roles whose manifest's write globs include the worktree (`@worktree/**`: builder and writer today) get a worker that inherits the session's tools. Every other worker gets the read-only list the seat files use plus Delphi's read tools. A drift test reads the manifests. |
| A5 | §1 E-3 said four Panel seats; the builder has a seat file and is not a Panel seat. | Corrected here: four seat files, three Panel seats. |
