---
id: spec-024-bundle-trust-adoption-and-precedence
title: "024: Bundle Trust, Adoption, Precedence, Scoping & Lifecycle"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # builds on the ratified lattice and ratification machinery; the bundle-specific parts are unbuilt
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, security, provenance, post-v1]
---

# Spec 024: Bundle Trust, Adoption, Precedence, Scoping & Lifecycle

> **Status: draft, proposed.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Depends on the bundle format of [`specs/023`](023-org-bundle-standard.md). Nothing here is built.

## 1. Scope & Objectives
[`specs/023`](023-org-bundle-standard.md) says what a bundle *is*. This spec says what happens when one meets an install:
1. **Signing and provenance** (§2): who published it, and how the install knows.
2. **Trust tiers** (§3): what tier each section gets, and why knowledge and behavior differ.
3. **Adoption, update, rollback and removal** (§4): the lock file, the cache, the cards.
4. **Precedence and tenancy** (§5): org / team / project / person, and who wins when two sources disagree.
5. **What a bundle can never do** (§6): the refusal list, with the threat each refusal answers.
6. **Conflict handling** (§7): structured collisions versus prose disagreement.

### 1.1 Non-Goals
- The store, roster, endorsement and freshness machinery: `specs/011` and `specs/012` stand unchanged; a bundle rides on them as one record (`specs/027` §4).
- Running anything from a bundle. Scripts, hooks and servers are refused or turned into proposals (§6).
- Per-reader access control inside a bundle (`specs/011` §5.4: one artifact is one audience).

### 1.2 Simpler Alternative Considered
- **Alternative: copy the bundle's files into `team/` and let the existing rules do the rest.** `team/` content already defaults to $L_2$ (`specs/001` §5.1), `team/skills/custom/` already needs ratification, and `delphi sync` already stages rule changes as proposals. Rejected because (a) copying loses the bundle's identity: two bundles and the team's own notes become one undifferentiated zone, so nothing can be updated, rolled back or removed as a unit, and a search result cannot say which organization said it; (b) a raw copy of `behavior/roles/*.md` has no place to land (there is no "guidance block" today) and `goals.md` would simply be a file nobody is told about; (c) a bundle from a vendor would sit beside the team's own `REVIEW.md` with the same default tier. The design below keeps the bundle **outside the workspace zones** in a content-addressed cache, exposes it through a logical path, and records exactly one lock entry per adopted bundle, so the existing rules apply *and* the unit stays intact.

### 1.3 Principles (inherited, restated for bundles)
1. **The receiving install computes trust** (`specs/011` §1.3). A bundle never states its tier; the install derives it from how the bundle arrived, who signed it, and what the operator ratified.
2. **Pull, not push.** Nothing changes until the operator adopts; an update is offered, never applied (`specs/011` §1.3 item 1).
3. **Humans sign; agents don't.** `delphi bundle sign` exists only behind a console approval of the exact file manifest, with the install's xenia key (`xenia-phase-1-plan.md` §2); no tool reaches it.
4. **Adoption of behavior is ratification; adoption of knowledge is not.** The one new rule this spec adds. Knowledge becomes readable at $L_2$ because the operator admitted the bundle; behavior becomes active at $L_5$ only file by file, because it will be put in front of an agent as instructions (GOVERNANCE §4.3).
5. **Fail closed and visible.** Anything that cannot be verified is refused and listed; nothing is silently dropped or silently accepted.

---

## 2. Signing and Publisher Identity
### 2.1 The signature
`.delphi-bundle/sig.json` is a detached signature over the file manifest (`specs/023` §3.4), in the sidecar shape of `specs/012` §3:
```json
{ "alg": "ed25519", "kid": "did:key:z6Mk...", "signed_at": "2026-10-25T20:00:00Z", "hash": "blake3:<of files.json>", "sig": "base64url..." }
```
`sig` is strict Ed25519 (RFC 8032 strict, the `verify_strict` built in X1) over the UTF-8 string
`xenia-bundle/1\n<org>\n<name>\n<version>\n<kid>\n<hash>\n<signed_at>`
where the verifier takes `<org>`, `<name>` and `<version>` **from `bundle.toml` as parsed in `specs/023` §9 step 4** and `<kid>` from the sidecar, never from `files.json`. A bundle renamed, re-versioned, moved to another org or presented under another key fails. The domain prefix `xenia-bundle/1` cannot be confused with `xenia/1`, `xenia-roster/1` or `xenia-fresh/1`, so the one key signs three kinds of thing without any cross-use (the `Domain` type of X1 gains a fourth constructor).

### 2.2 What a signature means, and what it does not
A valid signature means **these exact bytes were approved by the holder of this key**. It does not mean the content is true, safe or endorsed by the organization. Organizational standing comes only from the xenia roster when the bundle arrives through a store (`specs/011` §3.4; the publisher must be an `active` member and, for $L_2$, the record must be endorsed by a steward of a different person). Outside a store, the signature gives **continuity** (§2.3) and **attribution**, nothing more.

### 2.3 Publisher continuity (trust on first use)
At first adoption the install records the publisher's `kid` in the lock entry. A later version offered as an **update** (§4.3) must be signed by the same `kid`, or by a `kid` listed in the new manifest's `publisher.predecessors` chain that ends at the pinned one, or, when the bundle came from a xenia store, by a DID in the pinned member's `successor` chain (`specs/011` §3.4 step 5). Anything else is **not an update**: it is offered as a *different bundle with the same name*, on a card that says so in the first line, and adopting it replaces the pin only after the operator confirms the key change. This is the whole defense against a stolen repository or mirror serving a look-alike; it is the same TOFU-plus-continuity rule git signing and package managers use, and it is deliberately simple.

### 2.4 Unsigned bundles
A bundle without `sig.json` can be adopted **only from a local directory or archive the operator names on the command line**, never from a URL and never from a xenia store, and the card's first line says `UNSIGNED`. It gets no continuity: every later version is a "different bundle with the same name". Unsigned adoption exists so that a team can develop a bundle in a repository checkout and try it before anyone has a key; `delphi bundle check` recommends signing whenever `publisher.did` is set.

---

## 3. Trust Tiers
### 3.1 The rule
| Section | At adoption | Can rise to | Never |
| :--- | :--- | :--- | :--- |
| `knowledge/**` | **$L_2$** `federated_verified`, as a bounded, spotlighted retrieval result | $L_4$/$L_5$ **per file** through the existing promotion card (`specs/010` §5.3) or ratification | always-loaded |
| `skills/**` | **not loaded** (pending Red proposal per skill, `specs/019` §3) | $L_5$ when the operator ratifies that `SKILL.md` hash and names the roles | loaded unratified |
| `behavior/goals.md`, `perspectives.md` | **not loaded** (pending proposal) | $L_5$ on ratification; then assembled into the org-context slice (`specs/026` §3) | treated as rules |
| `behavior/roles/<role>.md` | **not loaded** | $L_5$ on ratification; then the role's org guidance block | widen a scope |
| `behavior/heuristics.md` | **proposals** into `context/team-heuristics.md`, one per rule | $L_5$ on ratification at `/retro` or `/ratify` | bypass the Rule of 25 |
| `behavior/policies.toml` | **pending proposal** | $L_5$ on ratification; then daemon-owned deny rules | loosen anything |
| `evals/**` | read by `kb eval` and the bench only; never in a prompt | — | — |

### 3.2 Why knowledge is $L_2$ and not lower
`specs/001` §5.2 forbids worktree writes, code execution and outbound calls for a span whose ceiling is $L_1$ or below. If adopted knowledge were $L_1$, any agent that consulted the organization's own architecture notes would lose the ability to edit code for the rest of the span, which is exactly the cost `specs/011` §5.2 identified for unendorsed records. The operator's adoption card (§4.2), which lists every file with its hash and the publisher's identity, is the human act that lifts the bundle to the same tier `team/` content already has by default (`specs/001` §5.1). **This is open question B-2** in `specs/022` §7: whether an *unsigned* local bundle should also reach $L_2$ on adoption, or stay at $L_1$ until signed. The recommendation is $L_2$ for both, because the operator's review of the file list is the same act either way and because unsigned adoption is confined to local paths; the counter-argument is that a signature is cheap and the asymmetry would nudge publishers to sign.

### 3.3 Why knowledge is never higher than $L_2$ without a per-file act
`federated_verified` content may enter an agent's context and may be written about, but it never enters `heuristics.md`, `REVIEW.md`, `lessons.md` or `skills/` (`specs/007` §3). A bundle cannot change that by being big or official. Promotion of one knowledge file to $L_4$ (because a local test replayed it) or $L_5$ (because the operator ratified it) uses the paths that already exist and stamps the file's hash, not the bundle.

### 3.4 Effective tier
As for every federated input: $\min(\text{section tier},\ \text{source\_ceiling})$. A bundle published from a span that had read untrusted data carries `source_ceiling` in its xenia record (`specs/012` §2); a directory or archive bundle has no stamped ceiling and is treated as `internal_generated` for this purpose, so the $\min$ is the section tier. A bundle arriving through a store that is **stale** under `specs/011` §4.3 counts no endorsement and reads at $L_1$ until freshness returns, exactly as a `memory` record would; the knowledge then leaves default search results and the console shows why.

### 3.5 How tiers reach the index
Bundle knowledge enters `state.db` as zone `bundles` with `rel_path` `bundles/<org>/<name>/<path>` (`specs/004` §3.3 gains one zone value). `source_type` is the computed tier, written by the daemon as `provenance.stamp` events for the file's hash (`specs/001` §5.1), so a tier change (staleness, promotion, removal) flips `trusted` without re-embedding: vectors are keyed by content hash, as the xenia plan already requires for records (`xenia-phase-1-plan.md` §5.1 item 2). The `[File: … | Source: federated_verified | Bundle: acme/acme-platform@1.4.0]` breadcrumb (`specs/004` §3.2) carries the bundle, so a result always says where it came from.

---

## 4. Adoption, Update, Rollback, Removal
### 4.1 Where things live
```text
$DELPHI_HOME/
├── bundles/                                   # content-addressed cache, outside every workspace zone, daemon-readable only
│   └── <org>/<name>/<bundle-hash>/            #   the verified tree, files without the execute bit
│       └── .delphi-bundle/{files.json,sig.json}
└── workspaces/<ws>/team/
    └── bundles.lock                           # ratified; hashed in local/ratified.lock like REVIEW.md (specs/006 §3.1)
```
Agents reach a bundle **only** through the logical path `@bundle/<org>/<name>/knowledge/**` (read-only, resolved by the daemon to the cache with the `specs/001` §2.2 resolution rules), and through the index. `skills/` and `behavior/` are never readable as files by any agent: their ratified copies live in `local/ratified/` like every other ratified file and reach a prompt only as a skill or a guidance block. A second workspace that adopts the same bundle hash shares the cache entry and has its own lock line.

`team/bundles.lock` (TOML, ratified, no agent write path):
```toml
[[bundle]]
org = "acme"
name = "acme-platform"
version = "1.4.0"
hash = "blake3:…"                              # the bundle hash (specs/023 §3.4)
publisher_kid = "did:key:z6Mk…"                # pinned at first adoption (§2.3); "unsigned" for §2.4
source = { kind = "xenia", store = "acme-main", record = "01JBX…@3" }   # or { kind = "archive", path = "…" } | { kind = "directory", path = "…" }
adopted_at = "2026-10-25T20:30:00Z"
order = 10                                     # overlay order among bundles (§5.3); lower applies first, later wins on ties
status = "active"                              # active | stale | superseded | removed
previous = ["blake3:…"]                        # earlier hashes still in the cache, for rollback (§4.4)
```

### 4.2 `delphi bundle adopt <source>`
1. Fetch or open the source (directory, archive, or a xenia `bundle` record's pinned tree, `specs/027` §4). **No network for a directory or archive.**
2. Run every step of `specs/023` §9 on the install. A failure stops here with the report.
3. Verify the signature and compute the publisher state (§2): `signed by <handle> (member of acme-main, active)`, `signed by <kid> (unknown key)`, or `UNSIGNED`.
4. Check `compat.delphi`, `overlays.depends_on` (every dependency adopted at a satisfying version) and `overlays.supersedes` (offer removal of the named bundles).
5. **The adoption card (Red tier, synchronous).** First line: publisher state and whether this is a first adoption, an update with continuity, or a key change. Then: the manifest fields; `touches` (roles, connectors); every section with file counts and sizes; every behavior file **in full**, because each is an instruction the operator is about to ratify; every knowledge file by path, summary and hash with the step-11 warnings beside it; the skills with their roles pre-filled from `authorized_agents` for the operator to edit; the proposed heuristics; the policy tables. The card has three answers per behavior item (ratify, skip, later) and one for the whole (adopt / refuse). Timeout denies.
6. On approval the daemon: writes the cache entry; appends the lock line and updates `ratified.lock`; stamps `provenance.stamp` for every knowledge file at the §3 tier; copies ratified behavior and skill files into `local/ratified/` with their hashes; files the heuristic proposals; applies ratified policies as daemon deny rules; indexes `knowledge/`; emits `bundle.adopted` with the hash and the list of ratified items. **No model turn happens anywhere in adoption.**

### 4.3 Updates
`delphi bundle update [name]` and `delphi xenia sync` look for a newer version from the lock's `source`. A candidate is **an update** only under the continuity rule (§2.3); otherwise it is offered as a key change. The update card shows: the version bump and whether it matches `check --against` (`specs/023` §8); a **per-section diff** (knowledge files added, changed, removed, superseded, with summaries; each behavior file as a text diff against the ratified copy; skill diffs; policy diffs); and which previously ratified items would **lapse** (a changed behavior file is un-ratified until re-ratified, exactly as a changed `AGENT.md` is staged as a proposal, `specs/001` §2.3). Knowledge updates apply on adoption of the new hash; behavior changes apply item by item on ratification; unchanged behavior files keep their ratification because their hash is unchanged. Updates are never automatic; `delphi doctor` and `/org` show "update available" with the publisher state.

### 4.4 Rollback
`delphi bundle rollback <name> [--to <hash>]` re-points the lock at a hash in `previous` (still in the cache; the cache keeps the last 3 hashes per bundle and `delphi gc` prunes older ones). Knowledge tiers follow the lock at once; behavior files that were ratified at that hash are ratified again **without a new card** because their hashes are already in `ratified.lock`'s history (`ratified.lock` keeps superseded entries with the date they lapsed; this is a small amendment to `specs/006` §3.1 listed in `specs/022` §7). A rollback is logged as `bundle.rolled_back`.

### 4.5 Removal
`delphi bundle remove <name>` sets `status = "removed"`, drops the knowledge from default search (tier stamps are revoked by a `provenance.stamp` to `external_untrusted` for the bundle's hashes), un-ratifies the bundle's skills, guidance and policies, and **leaves ratified team heuristics in place**, because they were ratified into the team's own file and may have been edited since; the card lists them with their `[T-<org>-NN]` ids and offers to stage their removal as proposals. The cache entry is kept until `gc`.

### 4.6 Staleness
Past `bundle.review_by` (or a file's own), the bundle shows as **stale** in `/org`, `doctor` and search breadcrumbs, and its results rank lower by the same fixed penalty as a stale xenia `memory` record (`specs/012` §5.1). Nothing is unloaded: staleness is information for humans, and the fix is a new version.

---

## 5. Precedence, Scoping and Tenancy
### 5.1 Where each audience lands
| `audience` | Lands in | Visible to | Notes |
| :--- | :--- | :--- | :--- |
| `org`, `team` | workspace `team/bundles.lock` | the workspace's operator(s); every teammate adopts separately, from the same source | the normal case; slice B2 |
| `project` | the same, with `applies_to.repos` expected to match the workspace's `projects` | as above | a mismatch is shown, not refused (`specs/023` §3.3) |
| `person` | `$DELPHI_HOME/operator/bundles.lock` (operator scope, `specs/004` §5.1) | only this operator, in every workspace | **deferred**: open question B-6. A personal bundle's knowledge would be the first operator-scope content ever indexed, which `specs/004` §3.1 forbids today; it needs its own amendment |

A workspace with `[workspace] isolated = true` may adopt bundles but never publishes one and never contributes to one (`specs/004` §5.4): nothing leaves an isolated workspace.

### 5.2 Precedence for *how to work*
The existing chain (`specs/001` §2.3 as amended by 001-A1) is workspace `core/` > operator `core/` > team `custom/` > built-in. Bundles insert **one layer below the team's own customizations**:

> workspace `core/` > operator `core/` > team `custom/` > **adopted bundles (by `order`, later wins)** > built-in release.

Consequences, each enforced where the chain already is:
- A team's own `team/skills/custom/<name>` shadows a bundle skill of the same name; the adoption card says so.
- An operator's personal override of a role's instructions comes *before* a bundle's guidance block in the prompt (`specs/004` §5.2 puts more specific scopes later and nearer the task; guidance is the least specific of the non-release layers and goes right after the manifest).
- Two bundles with the same skill name: the higher `order` wins and the card says which; `delphi bundle adopt --order N` sets it; `overlays.depends_on` fixes a floor.
- **Nothing in any layer relaxes governance** (`specs/004` §5.2): a bundle guidance block, a goal or a perspective is subject to the same no-relaxation linter as an operator rule, and the IPR governance block says so.

### 5.3 Precedence for *what is true* (knowledge)
There is none, deliberately. Knowledge from every adopted bundle is retrieved side by side with the team's own `context/` and the workspace's missions, each result carrying its bundle, tier and date. Ranking prefers matched `applies_to` over unmatched, fresh over stale, and otherwise follows the hybrid score. Deciding which of two disagreeing documents is right is a human or strategist judgment (§7), not a precedence rule, because a precedence rule would hide the disagreement.

### 5.4 Org versus personal
Operator scope never enters a bundle (`specs/004` §5.7) and a bundle never writes operator scope. `delphi bundle distill` (`specs/027` §2) reads only team and workspace content already promoted by the operator; the identifier filter and `[privacy] identifiers` apply; an isolated workspace is excluded. A personal preference that conflicts with a bundle's guidance is resolved by the chain in §5.2: the person wins on *how*, and the bundle is still visible on *what*.

---

## 6. What a Bundle Can Never Do
Each refusal is a check in `specs/023` §9 or an adoption rule above; this table says which threat it answers.

| # | A bundle cannot… | Enforced by | Threat |
| :-: | :--- | :--- | :--- |
| 1 | widen any role's `default_access_scope` or add `execute` capabilities | no manifest keys in guidance (`023` §6.3); narrow-only (`001` §2.3, D-21) | privilege escalation by a vendor or a compromised publisher |
| 2 | pre-approve a tool or command (`allowed-tools`, `allow`, `auto_approve`) | ignored with a note / refused keys (`023` §5, §6.5) | silent permission grant |
| 3 | run anything at adoption or later (`hooks/`, `bin/`, `scripts/`, `monitors`, `workflows`) | refused (`bin/`) or ignored (`hooks/`, `workflows/`, `monitors/`); scripts copied inert (`023` §2, §5) | arbitrary execution on the operator's machine |
| 4 | add an MCP server or connector | `.mcp.json` becomes a catalog **proposal** with the operator classifying tools (`025` §4; `010` §3) | exfiltration or injection through a server the bundle controls |
| 5 | ask for or carry a credential | no `userConfig`, `env`, `secrets`; `[REV-SEC-05]` scan (`023` §9 step 8) | credential theft; shipping a key |
| 6 | state or raise its own trust tier | rejected keys (`023` §3.1); tier computed (§3) | laundering |
| 7 | reach a model before verification | `check` before anything is indexed (§4.2 step 2) | parser attacks, poisoned bytes |
| 8 | fetch anything | pointers display-only (`023` §4.4); `touches.network = false` enforced | SSRF, hidden payload |
| 9 | put instruction text in front of an agent without ratification | behavior and skills not loaded until ratified (§3.1) | prompt-level control by a document |
| 10 | be installed as an update by a different key | continuity (§2.3) | repository or mirror compromise serving a look-alike |
| 11 | write into the workspace, `team/` or operator scope | cache outside zones; only `bundles.lock` changes, by the daemon (§4.1) | tampering with ratified files |
| 12 | hide a disagreement | no knowledge precedence (§5.3); structured collisions refused (§7.1) | one bundle silently overriding another's facts |

Residual risk, stated plainly: a bundle's **knowledge** is data an agent reads at $L_2$, and $L_2$ spans may write code and dispatch. A well-crafted false "runbook" can therefore mislead an agent as any trusted document can. The controls are the spotlighted envelope, the adoption card's warnings for instruction-shaped text, steward endorsement when the bundle comes through xenia, `dispute` records, and the alignment and contradiction surfaces of `specs/026`. Delphi does not claim to detect lies in prose.

---

## 7. Conflict Handling
### 7.1 Structured collisions (refused at adoption)
Two adopted bundles, or a bundle and the team's own files, must not both define: a knowledge `id`, a decision number, a goal id, a perspective id, a skill name, or a heuristic id, unless the later one's manifest names the earlier one in `overlays.supersedes` or the file carries `supersedes`. A collision without a declared supersession fails adoption with both paths named. This is cheap, deterministic, and catches the common case of two teams publishing "the" architecture overview.

### 7.2 Prose disagreement (surfaced, not resolved)
When two documents say different things in prose, no check can tell. What Delphi does:
- Search shows both, each with its bundle, tier and date (§5.3).
- The strategist's alignment check (`specs/026` §5) and the `contradiction-pruning` skill (an existing built-in) may **propose** a `dispute` xenia record against the record that published the bundle (`specs/012` §5.2), or a `knowledge/decisions/` entry for the team's own `context/`, through the normal publish gate. Agents never resolve a disagreement by editing a bundle: bundles are read-only on the install.
- `/org` lists open disputes and the stale documents, so a human resolves them where the knowledge lives: by publishing a new bundle version.

### 7.3 Rules
Heuristics are proposals into one capped file; if a bundle's `[T-acme-02]` and the team's `[T-07]` conflict, the operator sees both at `/retro` and keeps one. `rule_stats` (`specs/004` §5.6) then scores whichever was kept.

---

## 8. Acceptance Criteria
1. **AC-1 (signature):** flipping one byte of any file, of `files.json`, of `bundle.toml`'s `name`, `org` or `version`, or presenting `sig.json` with another `kid`, each fails adoption at the signature step with the reason; a valid bundle verifies with the strict verifier and its known-bad vectors (`specs/011` §10 test 25).
2. **AC-2 (continuity):** a newer version signed by a different key is offered as a key change, never applied as an update; one signed by a listed predecessor's successor is an update; an unsigned bundle is adoptable only from a local path and never from a URL or a store.
3. **AC-3 (tiers):** after adoption, a knowledge chunk reads `federated_verified` and appears in default search with the bundle breadcrumb; no `skills/`, `behavior/` or `evals/` text is in any prompt until ratified; a ratified guidance block appears in the role's prompt after the manifest and before memory and is at most 2 KiB; a span that reads bundle knowledge keeps its ability to write to a worktree (per its HITL tier) and a span that quarantine-reads an $L_1$ (stale) bundle does not.
4. **AC-4 (no model turn):** adoption, update, rollback and removal complete with the model client instrumented to refuse every call.
5. **AC-5 (precedence):** a `team/skills/custom/x` shadows bundle skill `x`; of two bundles both shipping `y`, the higher `order` wins and the card names the loser; an operator override's instructions precede the guidance block in the IPR.
6. **AC-6 (collisions):** two bundles with the same decision number fail adoption unless one `supersedes` the other; two disagreeing runbooks both appear in search with their bundles shown.
7. **AC-7 (lifecycle):** `update` shows a per-section diff and lapses only changed behavior items; `rollback` restores the previous hash's tiers and re-ratifies unchanged-hash items without a card; `remove` drops knowledge from default results, un-ratifies behavior, and lists the ratified team heuristics it leaves behind.
8. **AC-8 (refusals):** each row of §6 has a fixture that is refused or neutralized exactly as the row says (a `bin/`, a `hooks/hooks.json`, an `.mcp.json` server, a `userConfig`, an `allowed-tools`, a guidance file with `default_access_scope`, a `policies.toml` with `allow_commands`, a `file:` pointer).
9. **AC-9 (isolation):** the cache is outside every zone and unreadable to agents except through `@bundle/<org>/<name>/knowledge/**`; a read of `@bundle/…/behavior/goals.md` or `…/skills/x/SKILL.md` through a file tool is denied; `team/bundles.lock` is unwritable by any agent.
10. **AC-10 (staleness):** past `review_by` the bundle shows stale in `/org`, `doctor` and breadcrumbs, ranks lower, and nothing is unloaded; a store that goes stale under `specs/011` §4.3 drops the bundle's knowledge to $L_1$ and out of default results until freshness returns.

## 9. Verification Plan
Unit tests in `delphi-bundle` for AC-1, AC-2 and the §6 fixtures; daemon tests in `delphi-engine` for AC-3 (taint gate through `exec_tool`), AC-4 (instrumented model client), AC-5 (IPR assembly order), AC-6, AC-7 and AC-10 (clock injected); path-guard tests in `delphi-core` for AC-9 (symlink from a worktree into the cache; a hard link; a `..` path); an end-to-end test that adopts the `dev/bundles/acme-platform` fixture into a temp home, searches for a runbook question, ratifies one guidance file, updates to a mutated fixture, rolls back, and removes, checking `events.jsonl` for `bundle.adopted`, `bundle.updated`, `bundle.rolled_back`, `bundle.removed` and the `provenance.stamp` sequence. The red-team sweep per `REVIEW.md` runs on the adoption card code before the slice merges.

## 10. Open Questions
B-2 (adoption tier for unsigned bundles), B-6 (person-scope bundles), B-12 (`ratified.lock` history for rollback without a card) in `specs/022` §7.
