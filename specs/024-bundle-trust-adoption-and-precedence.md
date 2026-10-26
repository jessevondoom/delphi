---
id: spec-024-bundle-trust-adoption-and-precedence
title: "024: Bundle Provenance, Adding, Precedence, Scoping & Lifecycle"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # builds on the ratified lattice and the 006-A6 precedent; the bundle-specific parts are unbuilt
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, security, provenance, post-v1]
---

# Spec 024: Bundle Provenance, Adding, Precedence, Scoping & Lifecycle

> **Status: approved by the operator 2026-10-25 (decisions), text approved on merge.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Depends on the bundle format of [`specs/023`](023-org-bundle-standard.md). Nothing here is built; §9 is the build plan.

## 1. Scope & Objectives
[`specs/023`](023-org-bundle-standard.md) says what a bundle *is*. This spec says what happens when one meets an install:
1. **Signing and provenance** (§2): optional, who published it, and how the install shows that.
2. **Trust tiers** (§3): what each component gets, and why knowledge and behavior differ.
3. **Adding, update, rollback and removal** (§4): the lock with history, the cache, the printed summary and notices.
4. **Precedence and tenancy** (§5): org / team / project / person, and who wins when two sources disagree.
5. **What a bundle can never do** (§6): the schema limits, with the threat each answers.
6. **Conflict handling** (§7).
7. **Acceptance criteria, verification and the build plan** (§8–§10).

### 1.1 Non-Goals
- The store, roster, endorsement and freshness machinery: `specs/011` and `specs/012` stand unchanged; a bundle rides on them as one record (`specs/027` §4).
- Running anything from a bundle: scripts, hooks and servers are inert or refused (§6).
- Per-reader access control inside a bundle (`specs/011` §5.4: one artifact is one audience).

### 1.2 Simpler Alternative Considered
- **Alternative: copy the bundle's files into `team/` and let the existing rules do the rest.** `team/` content already defaults to $L_2$ (`specs/001` §5.1). Rejected because (a) copying loses the bundle's identity: two bundles and the team's own notes become one undifferentiated zone, so nothing can be updated, rolled back or removed as a unit, and a search result cannot say which organization said it or how it was signed; (b) a raw copy of `behavior/roles/*.md` has no place to land, and `goals.md` would be a file nobody is told about; (c) `team/skills/custom/` is a protected path whose changes stage Red proposals, which the operator has ruled out for bundles. The design below keeps the bundle **outside the workspace zones** in a content-addressed cache, exposes it through a logical path, and records one lock entry per added bundle with its history.

### 1.3 Principles
1. **Adding is the permission** (operator decision OD-1, 2026-10-25). `delphi bundle add` is an explicit operator act. The bundle's `README.md` is what the operator agreed to; the printed summary (publisher, provenance level, counts, what will load) is information, not a gate. Nothing blocks, no card is raised, nothing is ratified file by file. The precedent is `specs/006` §3.7 (006-A6): the operator running `delphi update` *is* the approval of the files the release ships.
2. **Schema limits are not ratification.** They stay (OD-1): policies tighten only; no scripts, hooks, executables or credentials; stdio MCP servers are never auto-launched; no skill or agent can hold more tool authority than its host role already grants.
3. **The receiving install computes provenance and tier** (`specs/011` §1.3). A bundle never states its tier.
4. **Humans sign; agents don't.** `delphi bundle sign` is a console action over the exact file manifest with the install's xenia key; no tool reaches it.
5. **Knowledge is data; behavior is instructions.** Knowledge becomes retrievable at $L_2$; behavior loads as instructions on add and is stamped by that act.
6. **Fail closed and visible** on *integrity*: a bundle that fails `validate`, a signature that does not verify, or a tree that does not match its manifest is refused and listed. Nothing is refused for lack of a signature.

---

## 2. Signing and Publisher Provenance
### 2.1 The signature (optional)
`.delphi-bundle/sig.json` is a detached signature over the file manifest (`specs/023` §3.4), in the sidecar shape of `specs/012` §3:
```json
{ "alg": "ed25519", "kid": "did:key:z6Mk...", "signed_at": "2026-10-25T20:00:00Z", "hash": "blake3:<of files.json>", "sig": "base64url..." }
```
`sig` is strict Ed25519 (the `verify_strict` of X1) over `xenia-bundle/1\n<org>\n<name>\n<version>\n<kid>\n<hash>\n<signed_at>`, where `<org>`, `<name>`, `<version>` come from `bundle.toml` as parsed in `specs/023` §10 step 4 and `<kid>` from the sidecar. A signed bundle renamed, re-versioned, moved or presented under another key fails. The domain `xenia-bundle/1` is a fourth constructor of X1's `Domain` type.

### 2.2 Provenance levels (shown everywhere the bundle is)
| Level | Condition | Shown as |
| :--- | :--- | :--- |
| `endorsed` | came through a xenia store; record endorsed by a steward of another person under a current freshness statement (`specs/011` §5.1) | `endorsed by <handle> (store <store>)` |
| `member-signed` | came through a store; publisher is an `active` member; not endorsed | `signed by <handle>, member of <store>, not endorsed` |
| `known-publisher` | signed; key equals the pinned publisher of an earlier version, or a listed predecessor's successor | `signed by <publisher.name>, same publisher as <previous version>` |
| `signed` | valid signature, key not seen before | `signed by <kid short form>, first time seen` |
| `unsigned` | no `sig.json` | `UNSIGNED, added from <path>` |

A signature means *these exact bytes were approved by the holder of this key*; it does not mean the content is true or endorsed. **No level is a prerequisite for anything** (OD-3): every level indexes knowledge at $L_2$ and loads behavior on add. Levels are information in the add summary, `/org`, `doctor`, and every search breadcrumb and spotlight envelope.

### 2.3 Publisher continuity and updates
At first add the install records the publisher's `kid` (or `unsigned`) in the lock. A later version is **an update** (applied by `delphi bundle update` with a notice, §4.3) only when signed by the same `kid`, by a key in the new manifest's `publisher.predecessors` chain ending at the pinned one, or, for a store bundle, by a DID in the pinned member's `successor` chain (`specs/011` §3.4 step 5). **A publisher change is not an update.** `update` stops with `publisher changed: <old> -> <new>; run \`delphi bundle add <source> --replace-publisher\` to accept the new publisher` and changes nothing. The explicit re-add is the operator's act of accepting a new publisher; it is not a card, it is a different command (OD-1). An unsigned bundle has no continuity: every later version is a re-add.

### 2.4 Unsigned bundles
Adoptable from any transport except a xenia store (which requires member signatures by `specs/011`). Indexed and loaded like any other (OD-3). `validate` recommends signing whenever `publisher.did` is set; `/org` shows `UNSIGNED` in the first column.

---

## 3. Trust Tiers
### 3.1 The rule
| Component | On add | Can rise to | Never |
| :--- | :--- | :--- | :--- |
| `knowledge/**` | **$L_2$** `federated_verified`, as bounded, spotlighted retrieval results | $L_4$/$L_5$ **per file** through the existing promotion card (`specs/010` §5.3) | always-loaded |
| `skills/**` | loaded for the roles the skill names (narrowed by `--skill-roles`), stamped by the add | — | widen a role's scope |
| `agents/**` | dispatchable as bundle subagents of their host role, stamped by the add | — | hold a tool the host lacks |
| `behavior/goals.md`, `perspectives.md` | assembled into the org-context slice (`specs/026` §3) | — | treated as rules |
| `behavior/roles/<role>.md` | the role's org guidance block | — | touch permissions |
| `behavior/heuristics.md` | the bundle rules block within the team scope budget (`specs/023` §6.4) | — | write `context/team-heuristics.md`; bypass the Rule of 25 |
| `behavior/policies.toml` | daemon deny rules | — | loosen anything |
| `evals/**`, `vectors/**` | read by `kb eval`, the bench and the vector pipeline; never in a prompt | — | — |

### 3.2 Why knowledge is $L_2$
`specs/001` §5.2 forbids worktree writes, code execution and outbound calls for a span whose ceiling is $L_1$ or below. If added knowledge were $L_1$, an agent that consulted the organization's own architecture notes would lose the ability to edit code for the rest of the span. The operator's explicit add is the act that lifts the bundle to the tier `team/` content already has by default (`specs/001` §5.1). This holds for unsigned local bundles too (OD-3).

### 3.3 Why knowledge is never higher without a per-file act
`federated_verified` content may enter a span's context and be written about, but never enters `heuristics.md`, `REVIEW.md`, `lessons.md` or `skills/` (`specs/007` §3). A bundle cannot change that. Promotion of one knowledge file to $L_4$ (local replay) or $L_5$ (the operator ratifies that file) uses the existing paths and stamps the file's hash, not the bundle.

### 3.4 How behavior is stamped
Behavior files are instruction text. They are never written into the protected always-loaded files; they load **from the bundle cache** into their own prompt slots (`specs/022` §5 IPR amendment). Their provenance stamp is $L_5$ with reason `bundle-add:<bundle hash>`, written by the daemon at add, on the 006-A6 precedent: the operator's command is the approval of exactly those bytes. A GOVERNANCE §5.7 amendment records this (`specs/022` §5). Removing the bundle revokes the stamps (§4.5).

### 3.5 Effective tier and staleness
As for every federated input: $\min(\text{section tier},\ \text{source\_ceiling})$. A bundle published from a span that read untrusted data carries `source_ceiling` in its xenia record (`specs/012` §2); a directory or archive bundle has no stamped ceiling and is treated as `internal_generated` for the $\min$. A store bundle whose store is **stale** under `specs/011` §4.3 loses its `endorsed` level and shows `member-signed, store stale`; **its tier does not drop**, because tier here comes from the operator's add, not from endorsement (this differs from `memory` records and is deliberate: a stale store is a reason to look, not a reason to lose search).

### 3.6 How tiers reach the index
Knowledge enters `state.db` as zone `bundles` with `rel_path` `bundles/<org>/<name>/<path>` (`specs/004` §3.3 gains one zone value). `source_type` is the computed tier, written as `provenance.stamp` events for each file's hash (`specs/001` §5.1), so a tier change flips `trusted` without re-embedding. The breadcrumb (`specs/004` §3.2) gains `Bundle: acme/acme-platform@1.4.0 · known-publisher` so every result says where it came from and how it was signed.

---

## 4. Adding, Update, Rollback, Removal
### 4.1 Where things live
```text
$DELPHI_HOME/
├── bundles/                                   # content-addressed cache, outside every workspace zone, daemon-readable only
│   ├── .incoming/<random>/                    #   archives are verified here before they are moved (specs/027 §3)
│   └── <org>/<name>/<bundle-hash>/            #   the verified tree, files without the execute bit
│       ├── .delphi-bundle/{files.json,sig.json}
│       └── vectors/                           #   shipped caches, and the install's own verification marks (specs/027 §5)
└── workspaces/<ws>/local/
    └── bundles.lock                           # daemon state with history; NOT a ratified file (OD-1)
```
Agents reach a bundle **only** through the logical path `@bundle/<org>/<name>/knowledge/**` (read-only, resolved by the daemon with the `specs/001` §2.2 rules) and through the index. `skills/`, `agents/`, `behavior/`, `vectors/` are never readable as files by any agent: they reach a prompt only as a skill, a subagent block, a guidance block, a rules block or the slice. A second workspace adding the same hash shares the cache entry.

`local/bundles.lock` (TOML, written only by the daemon):
```toml
[[bundle]]
org = "acme"
name = "acme-platform"
version = "1.4.0"
hash = "blake3:…"                              # the bundle hash (specs/023 §3.4)
publisher_kid = "did:key:z6Mk…"                # pinned at first add; "unsigned" (§2.4)
provenance = "known-publisher"                 # §2.2, recomputed on sync for store bundles
source = { kind = "xenia", store = "acme-main", record = "01JBX…@3" }   # or { kind = "archive", path = "…" } | { kind = "directory", path = "…" } | { kind = "marketplace", … }
added_at = "2026-10-25T20:30:00Z"
order = 10                                     # overlay order among bundles (§5.3); later wins on ties
status = "active"                              # active | stale | superseded | removed
history = [                                    # every hash this install has had for this bundle, newest first, for rollback (§4.4)
  { hash = "blake3:…", version = "1.3.2", from = "2026-09-01T10:00:00Z", to = "2026-10-25T20:30:00Z" },
]
```

### 4.2 `delphi bundle add <source> [--order N] [--no-vectors] [--skill-roles <skill>=<roles>] [--replace-publisher]`
1. Fetch or open the source (directory, archive, marketplace entry, or a xenia `bundle` record's pinned tree). No network for a directory or archive.
2. Run every step of `specs/023` §10 on the install. An integrity failure stops here with the report.
3. Compute the provenance level (§2.2) and check continuity (§2.3): a publisher change without `--replace-publisher` stops with the message.
4. Check `compat.delphi`, `overlays.depends_on` (every dependency added at a satisfying version), `overlays.supersedes` (offer removal; `--yes` accepts).
5. **Print the summary** (non-blocking, OD-1): the first lines of `README.md` (up to 40 lines, the rest by path); publisher and provenance level; `touches` (roles, connectors, with "inert until mapped" for connectors, `specs/025` §4.1); per section the counts and sizes; the names of skills with their roles, agents with their host roles and any dropped tools, guidance roles, rule and policy counts; the vector caches found and whether they match this install's embedder (`specs/027` §5). With `--dry-run` it stops here.
6. The daemon then: writes the cache entry; appends or updates the lock line (pushing the previous hash onto `history`); stamps `provenance.stamp` for every knowledge file at $L_2$ and every behavior, skill and agent file at $L_5$ with reason `bundle-add:<hash>`; registers skills and bundle subagents for their roles; installs the guidance and rules blocks and the deny rules; indexes `knowledge/` lexically **at once** and schedules vectors (`specs/027` §5); emits `bundle.added`. **No model turn anywhere in adding.**

### 4.3 Updates
`delphi bundle update [name]` and `delphi xenia sync` look for a newer version from the lock's `source`. Under the continuity rule (§2.3) the new version is applied like an add, and a **changelog notice** is printed and shown in `/org`: the top entry of `CHANGELOG.md` if present, the version bump and whether it matches `validate --against` (`specs/023` §9), and a per-section summary (knowledge files added, changed, removed, superseded; skills, agents, guidance, rules, policies changed; vector caches). Unchanged chunks keep their vectors (content-hash keyed). Behavior that changed is reloaded from the next task (frozen-prefix hygiene, `specs/004` §2.1). `delphi doctor` and `/org` show "update available" with the publisher state; updates are applied by the operator's command, never automatically, except that `delphi xenia sync --auto-update-bundles` may be set per store for same-publisher updates (a configuration the operator writes; off by default).

### 4.4 Rollback
`delphi bundle rollback <name> [--to <hash>]` re-points the lock at an entry in `history` whose tree is still in the cache (the cache keeps the last 3 hashes per bundle; `delphi gc` prunes older ones and the lock says which are gone). Knowledge tiers, behavior blocks, skills, agents and deny rules follow the lock at once (next task for prompt content); `bundle.rolled_back` is emitted. No card, no ratification: the lock history is the record (OD-1, B-12 resolved).

### 4.5 Removal
`delphi bundle remove <name>` sets `status = "removed"`, revokes the knowledge stamps (`provenance.stamp` to `external_untrusted` for the bundle's hashes, so results leave default search), unloads skills, subagents, guidance, rules and deny rules, and emits `bundle.removed`. The cache entry stays until `gc`. Because bundle rules were never written into `context/team-heuristics.md` (`specs/023` §6.4), removal leaves nothing behind in the team's own files.

### 4.6 Staleness
Past `bundle.review_by` (or a file's own), the bundle shows **stale** in `/org`, `doctor` and breadcrumbs, and its results rank lower by the same fixed penalty as a stale xenia record. Nothing is unloaded.

---

## 5. Precedence, Scoping and Tenancy
### 5.1 Where each audience lands
| `audience` | Lands in | Visible to | Notes |
| :--- | :--- | :--- | :--- |
| `org`, `team` | workspace `local/bundles.lock` | the workspace's operator; each teammate adds from the same source (`team/bundles.toml` may list recommended sources, which `/org` offers to add: still an explicit act) | the normal case; slice B2 |
| `project` | the same, with `applies_to.repos` expected to match `projects` | as above | a mismatch is shown, not refused (`specs/023` §3.3) |
| `person` | `$DELPHI_HOME/operator/bundles.lock` | only this operator, every workspace | **deferred** (B-6): the first indexed operator-scope content; needs a `specs/004` §3.1 amendment |

A workspace with `[workspace] isolated = true` may add bundles but never publishes or distills one (`specs/004` §5.4).

### 5.2 Precedence for *how to work*
The existing chain (`specs/001` §2.3 as amended by 001-A1) is workspace `core/` > operator `core/` > team `custom/` > built-in. Bundles insert **one layer below the team's own customizations**:

> workspace `core/` > operator `core/` > team `custom/` > **added bundles (by `order`, later wins)** > built-in release.

- A team's own `team/skills/custom/<name>` shadows a bundle skill of the same name; the add summary says so.
- An operator's personal override of a role's instructions comes *before* a bundle's guidance block in the prompt (`specs/004` §5.2: more specific scopes later and nearer the task; guidance is the least specific non-release layer).
- Two bundles with the same skill or agent name: the higher `order` wins and the summary says which.
- **Nothing in any layer relaxes governance** (`specs/004` §5.2): bundle guidance, agents, goals and perspectives pass the same no-relaxation linter as an operator rule, and the IPR governance block says so.

### 5.3 Precedence for *what is true* (knowledge)
None, deliberately. Knowledge from every added bundle is retrieved beside the team's own `context/` and the workspace's missions, each result carrying bundle, provenance level, tier and date. Ranking prefers matched `applies_to` over unmatched, fresh over stale, then the hybrid score. Which of two disagreeing documents is right is a human or strategist judgment (§7).

### 5.4 Org versus personal
Operator scope never enters a bundle (`specs/004` §5.7) and a bundle never writes operator scope. `distill` reads only allow-listed team and workspace content (`specs/027` §2.3). A personal preference that conflicts with bundle guidance is resolved by §5.2: the person wins on *how*; the bundle stays visible on *what*.

---

## 6. What a Bundle Can Never Do (schema limits, OD-1)
| # | A bundle cannot… | Enforced by | Threat |
| :-: | :--- | :--- | :--- |
| 1 | widen any role's scope or add `execute` capabilities; give an agent a tool its host role lacks | no manifest keys in guidance (`023` §6.3); `tools` intersected (`023` §5B); narrow-only (`001` §2.3, D-21) | privilege escalation by a vendor or a compromised publisher |
| 2 | pre-approve a tool or command (`allowed-tools`, `allow`, `auto_approve`) | ignored with a note / refused keys (`023` §5A, §6.5) | silent permission grant |
| 3 | run anything at add or later (`hooks/`, `bin/`, `scripts/`, `monitors`, `workflows`, stdio servers) | `bin/` refused; the rest ignored or copied inert (`023` §2, §5A; `025` §2.3, §4.1) | arbitrary execution on the operator's machine |
| 4 | connect an MCP server by itself | `.mcp.json` entries are **inert until the operator maps them** (`025` §4.1) | exfiltration or injection through a server the bundle controls |
| 5 | ask for or carry a credential | no `userConfig`, `env`, `secrets`; `[REV-SEC-05]` scan (`023` §10 step 8) | credential theft; shipping a key |
| 6 | state or raise its own trust tier or provenance level | rejected keys (`023` §3.1); level and tier computed (§2.2, §3) | laundering |
| 7 | reach a model before verification | `validate` before anything is indexed (§4.2 step 2) | parser attacks, poisoned bytes |
| 8 | fetch anything | pointers display-only (`023` §4.4); `touches.network = false` enforced | SSRF, hidden payload |
| 9 | load behavior by being present on disk | behavior loads only through `add`; a bundle directory inside a project or `team/` is never auto-discovered | prompt-level control by a dropped-in directory |
| 10 | replace itself under a new publisher silently | continuity (§2.3); explicit `--replace-publisher` | repository or mirror compromise serving a look-alike |
| 11 | write into the workspace, `team/`, operator scope or the protected files | cache outside zones; only `local/bundles.lock` changes, by the daemon (§4.1); rules block instead of file writes | tampering with ratified files |
| 12 | hide a disagreement | no knowledge precedence (§5.3); structured collisions refused (§7.1) | one bundle silently overriding another's facts |
| 13 | make shipped vectors change what text an agent sees | vectors affect ranking only and are verified (`specs/027` §5) | poisoned ranking, or a cache for other text |

Residual risk, stated plainly: a bundle's **knowledge** is data an agent reads at $L_2$, and its **behavior** is instruction text the operator admitted by adding it. A well-crafted false runbook or a subtly bad guidance block can mislead an agent as any trusted document can. The controls are the printed summary and README, the no-relaxation linter, the spotlighted envelope, steward endorsement when the bundle comes through xenia, `dispute` records, and the alignment and contradiction surfaces of `specs/026`. Delphi does not claim to detect lies in prose, and after OD-1 it does not claim a second human review on the receiving side; the review is on the publishing side (`specs/027` §2.6).

---

## 7. Conflict Handling
### 7.1 Structured collisions (refused at add)
Two added bundles, or a bundle and the team's own files, must not both define a knowledge `id`, a decision number, a goal id, a perspective id, a skill name, an agent name or a rule id, unless the later one's manifest names the earlier one in `overlays.supersedes` or the file carries `supersedes`. A collision without a declared supersession fails `add` with both paths named (an integrity failure, not a gate).

### 7.2 Prose disagreement (surfaced, not resolved)
Search shows both with bundle, provenance and date (§5.3). The strategist's alignment check (`specs/026` §5) and the `contradiction-pruning` skill may **propose** a `dispute` record against the bundle's xenia record (`specs/012` §5.2) or a team decision record, through the normal publish gate. Bundles are read-only on the install; agents never edit them. `/org` lists open disputes and stale documents.

### 7.3 Rules
Bundle rules live in their own block under the team budget (`specs/023` §6.4). A bundle rule and a team rule that conflict are both visible at `/retro`; the operator edits the team file or lowers the bundle's `order`; `rule_stats` scores whichever stays loaded.

---

## 8. Acceptance Criteria
1. **AC-1 (signature):** flipping one byte of any file, of `files.json`, or of `bundle.toml`'s `name`, `org` or `version`, or presenting `sig.json` under another `kid`, each fails `add` at the signature step; a valid bundle verifies with the strict verifier and its known-bad vectors (`specs/011` §10 test 25); a bundle with no `sig.json` adds with level `unsigned`.
2. **AC-2 (continuity):** a newer version signed by a different key stops `update` with the re-add message and changes nothing; `add --replace-publisher` accepts it and pins the new key; a listed predecessor's successor is an update; an unsigned bundle's later version is a re-add.
3. **AC-3 (tiers and loading):** after `add`, a knowledge chunk reads `federated_verified`, appears in default search with the bundle and provenance level, and reaches the model in the spotlighted envelope; a skill is offered to exactly its roles; an agent dispatches as its host role with the intersected tools and never with a dropped one; the guidance block is in the role's prompt after the manifest and before memory at ≤ 2 KiB; bundle rules appear in the team scope slot and never in `context/team-heuristics.md`; a deny policy refuses its command; all stamps are present with reason `bundle-add:<hash>`; a span reading bundle knowledge keeps its worktree write ability per its HITL tier.
4. **AC-4 (no cards, no model):** `add`, `update`, `rollback` and `remove` complete with the model client instrumented to refuse every call and the approval broker instrumented to fail on any card.
5. **AC-5 (precedence):** `team/skills/custom/x` shadows bundle skill `x`; of two bundles both shipping `y`, the higher `order` wins and the summary names the loser; an operator override's instructions precede the guidance block in the IPR; a Rule-of-25 overflow trims bundle rules oldest-added first and never touches the team file.
6. **AC-6 (collisions):** two bundles with the same decision number fail `add` unless one `supersedes` the other; two disagreeing runbooks both appear in search with their bundles shown.
7. **AC-7 (lifecycle):** `update` prints the changelog notice and per-section summary, keeps vectors for unchanged chunks, and reloads changed behavior from the next task; `rollback` restores the previous hash's stamps and blocks with no card; `remove` drops knowledge from default results, unloads behavior and leaves `context/team-heuristics.md` byte-identical.
8. **AC-8 (limits):** each row of §6 has a fixture that is refused or neutralized exactly as the row says.
9. **AC-9 (isolation):** the cache is outside every zone and unreadable to agents except through `@bundle/<org>/<name>/knowledge/**`; a file-tool read of `@bundle/…/behavior/goals.md`, `…/skills/x/SKILL.md`, `…/agents/x.md` or `…/vectors/…` is denied; `local/bundles.lock` is unwritable by any agent; a bundle directory copied into a project or `team/` loads nothing.
10. **AC-10 (staleness and store state):** past `review_by` the bundle shows stale, ranks lower, and nothing is unloaded; a store that goes stale drops the level from `endorsed` to `member-signed, store stale` and the tier stays $L_2$.

## 9. Verification Plan
Unit tests in `delphi-bundle` for AC-1, AC-2 and the §6 fixtures; daemon tests in `delphi-engine` for AC-3 (taint gate through `exec_tool`, prompt assembly, skill and agent registration, deny rules), AC-4 (instrumented `ModelClient` and `approval` broker), AC-5, AC-6, AC-7 and AC-10 (injected clock); path-guard tests in `delphi-core` for AC-9; an end-to-end test that adds the `dev/bundles/acme-platform` fixture into a temp home, searches, dispatches the bundle agent, updates to a mutated fixture, rolls back and removes, checking `events.jsonl` for `bundle.added|updated|rolled_back|removed` and the `provenance.stamp` sequence. A red-team sweep per `REVIEW.md` runs on the add path before slice B2 merges.

## 10. Build Plan (slices B2, B4, B6 of `specs/022` §6)
Each slice is one PR, pickable by a separate session, in the order below. "Touches" names real modules; new files are marked (new).

| Slice | Touches | Tests and fixtures | Needs | Effort |
| :-- | :--- | :--- | :--- | ---: |
| **B2 Add from a directory** (AC-3 knowledge part, AC-4, AC-6, AC-9, AC-10 staleness) | `crates/delphi-engine/src/workspace.rs` (bundle cache paths, `local/bundles.lock` read/write with history); `crates/delphi-core/src/pathguard.rs` (`@bundle/<org>/<name>/knowledge/**` logical root, deny the other directories); `crates/delphi-core/src/provenance.rs` (stamp reasons `bundle-add:<hash>`); `crates/delphi-engine/src/kb.rs` (`md_files` source for zone `bundles`, `SourceFile.default_source`, breadcrumb with bundle and level, `SearchOpts` filters `--bundle/--org/--no-bundles`, fixed penalties for unmatched and stale); `crates/delphi-engine/src/service.rs` (RPC `bundle.add|list|remove`, `bundle.*` events in `crates/delphi-core/src/event.rs`); `crates/delphi/src/bundle_cmd.rs` (new: `add`, `list`, `remove`, `--dry-run` summary printer) and the subcommand table in `crates/delphi/src/main.rs`; `crates/delphi/src/doctor.rs` (stale bundles, cache orphans); `crates/delphi/src/needs.rs` (`bundles` row) | `delphi-engine` tests: add the golden fixture into a temp home (`testkit.rs`), search with and without filters, breadcrumb text, collision refusal, remove leaves `team/` unchanged; `delphi-core` pathguard tests for the four denied directories and a symlink from a worktree into the cache; CLI test in `crates/delphi/tests/cli.rs` for `--dry-run` output | B1 | 16 h |
| **B4 Behavior and agents on add** (AC-3 behavior part, AC-5, AC-8 rows 1–3, 9, 11) | `crates/delphi-engine/src/boot.rs` (frozen snapshot: bundle rules block inside the team budget with oldest-first trimming and `rule.trimmed` events); `crates/delphi-engine/src/engine.rs` (IPR slots: guidance block after the manifest where `ContextSlice`s are pushed near `@runtime/tool-inventory`; `dispatch_agent` target `bundle:<name>/<agent>` beside the role path at the `"dispatch_agent"` branch; `span_tool_defs` intersection for a bundle agent); `crates/delphi-adapter/src/ipr.rs` (slot order and the governance sentence); `crates/delphi-engine/src/capabilities.rs` and `skills` loading (register bundle skills for their roles from the cache, never from `team/skills/custom/`); `crates/delphi-engine/src/policy.rs` + `hooks.rs` (deny rules from `policies.toml`, same shape as `[[hooks.deny]]`); `crates/delphi/src/lint.rs` (no-relaxation check reused as a library call for guidance and agent bodies) | prompt-assembly tests (order, caps, governance sentence present); dispatch test (host role, tool intersection, dropped-tool note, logged `bundle_agent`); deny-rule test (a command matching `deny_commands` is refused, an `allow` key fails `validate`); Rule-of-25 overflow test (team file untouched); the §6 fixtures rows 1–3, 9, 11 | B2 | 18 h |
| **B6 Provenance, archive, update, rollback** (AC-1, AC-2, AC-7, AC-8 row 10; `specs/027` AC-3, AC-4) | `crates/delphi-xenia/src/sig.rs` (`Domain::bundle` constructor for `xenia-bundle/1`), `identity.rs` (sign over a manifest hash behind the console TTY challenge, as `ratify_console.rs` does); `crates/delphi-bundle/src/archive.rs` (new: ustar writer/reader using the `tar` crate already in `crates/delphi`, limits checked while reading, `.incoming/` staging); `crates/delphi/src/bundle_cmd.rs` (`sign`, `package`, `update`, `rollback`, `--replace-publisher`); `crates/delphi-engine/src/workspace.rs` (lock `history`, cache retention 3, `gc`); `crates/delphi-engine/src/service.rs` (`bundle.update|rollback`, changelog notice) | signature and domain tests with the X1 known-bad vectors; continuity matrix (same key, predecessor chain, store successor, unsigned, changed key); archive limits at every boundary and byte-reproducibility; update/rollback end-to-end on two fixture versions with vectors kept for unchanged chunks | B2 (B4 for behavior reload) | 14 h |

Merge sequencing: B2 first (it is what everything else loads from); B4 and B6 may proceed in parallel from B2; B6's archive is a prerequisite of `specs/027` B8 (store) and `specs/025` B7's marketplace fetch.

### 10.1 As built (slice B2)
`Implements-Spec: specs/024-bundle-trust-adoption-and-precedence.md` (AC-3 knowledge part, AC-4, AC-6, AC-9, AC-10 staleness). `delphi bundle add <dir> [--order N] [--yes] [--dry-run]`, `list` and `remove <name>` run in the CLI process against one workspace; `delphi kb search` gained `--bundle <org/name>`, `--org <org>` and `--no-bundles`; `delphi doctor` gained check 12. Where the code differs from, or settles, the text above:
- **No daemon RPC, no `bundle.*` events and no `provenance.stamp` events yet.** The code has no workspace-level event log (events are per mission), the index does not read stamps, and the CLI has no precedent for writing events. The tier is fixed by the zone instead: files of the `bundles` zone are indexed as `federated_verified` whatever their frontmatter says (that field is a display copy, `specs/023` §4.2), recorded in `local/bundles.lock`, and a removed bundle drops out of the index at the next sync, which has the effect §4.5 asks of revoking the stamps. The stamps with reason `bundle-add:<hash>` for **behavior** files, and the events, arrive with B4, which needs the daemon.
- **The cache and lock live in `delphi-core::bundles`** (shared by the CLI, the index and the path guard). `local/bundles.lock` is written atomically under an advisory lock; a corrupt one is an error, never an empty lock.
- **`@bundle/<org>/<name>/knowledge/**`** is a path-guard root (`Zones.bundles`, one entry per active bundle, resolved from the lock to the active hash's `knowledge/`). Reads only; every other directory of a bundle, any listing above `knowledge/`, a bundle that is not added and any write are denied; symlinks are never followed. Search hits for a bundle carry the `@bundle/…#L…` pointer, the logical path an agent reads them by. The index's own `rel_path` is `bundles/<org>/<name>/knowledge/…`.
- **Staleness ranks, it does not hide:** a result from a bundle past its `review_by` keeps 80% of its fused score (`STALE_BUNDLE_FACTOR`). The "added, not matched" penalty for `applies_to` is not built (it needs the workspace's projects in the index); it arrives with the org-context slice (B5).
- **Provenance is always `unsigned`** until B6 verifies signatures; a `sig.json` is kept in the cache but never reported as verified.
- **`--yes`** removes bundles named in `overlays.supersedes`; without it they are kept and the summary says so. Collisions (§7.1) compare structured ids across the other active bundles' cache entries; collisions with the team's own files are not checked yet.
- **Vectors are not used yet** (B3): `add` indexes words only and says so; `delphi kb index` still embeds.

### 10.2 As built (slice B4a: the prompt side of B4)
`Implements-Spec: specs/024-bundle-trust-adoption-and-precedence.md` (AC-3 behavior part for guidance, rules and skills; AC-5 for skills and rules). B4 is split in two PRs; this one is everything that reaches a role's prompt. The second (B4b) is `bundle:` agent dispatch with tool intersection, the deny policies (`deny_commands`, `red_paths`, `connector_denied`) and the `provenance.stamp` / `bundle.*` events, which need the daemon.
- **Loading is part of the frozen snapshot.** `WorkspacePaths::load_snapshot(role)` reads the lock and the cache (`delphi-engine::bundle_load`) and returns two new fields beside the heuristics: `guidance` and `bundle_skills`. A change to the lock or the cache is therefore seen from the next task, never mid-task (`specs/004` §2.1).
- **Every file is verified at load.** `delphi_bundle::load::read_verified` proves that the cache's `files.json` hashes to the hash the lock recorded at add and that the file hashes to its entry; a cache entry edited afterwards loads nothing. Guidance and rules also pass the no-relaxation check again. Each refusal is a `snapshot.trim` event naming the bundle and the reason (the existing event, not a new `rule.trimmed` kind).
- **IPR.** Two new system-channel sections, `guidance` (after the role manifest, before the heuristics) and `bundle_skills` (after the heuristics, before the lessons), both in `SECTION_TAGS` so interpolated text cannot close them. The governance block now says nothing below it relaxes it "not by org guidance or bundle skills". Behavior text is never in the user channel.
- **Guidance:** `behavior/roles/<role>.md` of each active bundle, at most 2 KiB each and 4 KiB in all; over the total, the lowest `order` is dropped first. Only for the role the file names.
- **Rules:** a bundle's `behavior/heuristics.md` rules are placed after the team's own rules inside the team scope (`### [T-org-NN] …`), taking what the team's rules leave of the 3 KB budget, and held with them to the Rule of 25 (the combined count). Over either limit, rules are dropped oldest-added bundle first, each drop logged; `context/team-heuristics.md` is never read for this or written.
- **Skills:** the skills whose `authorized_agents` name the role, narrowed by `delphi bundle add --skill-roles <skill>=<roles>` (stored in the lock, kept across updates; it can only remove roles). A team skill under `team/skills/custom/<name>` shadows a bundle skill of the same name; of two bundles with one name the higher `order` wins, even for a role it was narrowed away from. Bodies are inlined up to 3 KiB each and 6 KiB in all; the rest are listed by name and description. `scripts/` is never read.
- **Not yet:** stamps `bundle-add:<hash>` and the `bundle.*` events (B4b, with the daemon), bundle agents, deny policies and red paths (B4b), and the `/org` view (B5).

## 11. Open Questions
B-6 (person scope) in `specs/022` §7. Everything else in this spec is decided (OD-1, OD-3) or proposed and recommended.
