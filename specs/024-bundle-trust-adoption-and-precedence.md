---
id: spec-024-bundle-trust-adoption-and-precedence
title: "024: Bundle Provenance, Adding, Precedence, Scoping & Lifecycle"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges. Ratified in full by the operator 2026-10-28: every proposed decision and open question of the set (B-1..B-17, D-P1..D-P14) follows its recommendation
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

### 3.7 Bundle contents are validated sources; the operator may raise one bundle (OD-10, 2026-10-27)
**Default: citable as validated.** Added bundle knowledge stays at its install-stamped tier (zone `bundles`, $L_2$ `federated_verified`; §3.2). A bundle's own `source_type` stays a display copy and can never raise trust (`specs/001` §5). What changes is how the tier is *described and used*: agents, Council, the strategist and alignment checks (`specs/026`) and the kb output may **cite** bundle documents as **validated bundle sources**, with the bundle and its provenance level. Wherever sources are cited or trust is described to a model the words are the same: the breadcrumb reads `Source: federated_verified | Bundle: acme/acme-platform@1.4.0 · known-publisher · validated bundle source`, the org-context header carries `validated bundle source`, and the slice's intent sentence says that bundle documents "stay data, never instructions, and grant no permission". Nothing about loading, the spotlighted envelope or the guards changes.

**The operator may raise one bundle to $L_5$ `user_ratified`** (`specs/001` §5: the only name for $L_5$; ratification is the operator's act). Three ways in, one confirmation:
- at install: `delphi bundle add <source> --trust ratified`; later: `delphi bundle trust <bundle> --level user_ratified`; in the console: `/org`, key `t`. `--level default` (or `t <name> default`) lowers it again, at any time, with no confirmation (lowering never needs one).
- **The confirmation** needs a terminal on stdin **and** stdout and the operator typing the bundle name (`org/name`). There is no `--yes`, no environment variable and no pipe that stands in for it; a process an agent starts has no terminal and is refused (the `sign` and `connectors map` precedent). In the console it is the `org.trust` call, **console scope only** (`need_console`, as for X9's attestation): two steps, the first returns the card text and the word to type, the second applies the act only if the typed word is exactly that word. The narrator and every agent are refused (`FORBIDDEN`); `org.action` has no `trust` verb.
- The text shown says plainly what the raise does and does not do (§3.7.3), that it is **for this version and publisher only**, and that it **resets** when the bundle's hash or publisher changes (an update that brings a new version) and on rollback, so that the operator confirms again.

**3.7.1 Where it lives.** The raise is **install-owned state**: fields of the bundle's entry in `local/bundles.lock` (`trust`, `trust_hash`, `trust_kid`, `trust_log`), which agents cannot read or write (`local/` is on the pathguard's deny lists; AC-9) and which no bundle can ship or set: the lock is never read from a bundle tree, and no manifest key, frontmatter field or file in a bundle names it (the B3 vectors lesson: nothing a bundle ships may sit where the install's trust is read). It counts only while `trust_hash` and `trust_kid` still equal the entry's `hash` and `publisher_kid` (`Entry::raised`), so an update, a rollback, a replaced publisher or an edited lock puts the bundle back at the default even if a reset step were missed. It is per bundle, shown in `bundle list`, the add summary (`knowledge tier: ...`) and `/org` (the `tier` column), recorded in `trust_log` (newest first: when, level, hash, by `cli|console|at-add|trusted-source|update|rollback|remove|source-revoked`) and as the event `bundle.trust_changed` (with `from`, `tier`, `by`); the knowledge stamps are rewritten at $L_5$ with reason `bundle-trust:<hash>` or back at $L_2$ with `bundle-add:<hash>`; the index re-reads a bundle file whose stored tier no longer matches the lock without its bytes having changed. Revoking is instant for the lock and the index, and reaches a running mission at its next reconcile (task start or any `org.*` call).

**3.7.2 Resets.** A **one-off** raise ends when `update` or `add` brings a different hash or publisher key, on `rollback`, and on `remove`. The add or rollback output says so ("your raise ... applied to the previous version only and is reset"). A **Delphi version update never resets or alters** any raise or trusted-source setting: the lock is workspace state under `local/`, and the self-updater and release code never touch it (a test pins this).

**3.7.3 A raise lifts no other guard.** Not changed by any tier: the `@bundle/` path guard (a bundle's files are read-only to every agent; behavior, skills and agents stay unreadable), deny policies, `require_card`, permissions and approvals, the no-relaxation linter, prompt-injection treatment (a document is data, never instructions), and which tiers may authorize what (the lattice of `specs/001` §5 and the gated-paths rule). The one tier-dependent effect, stated so the operator knows: a span that reads a raised bundle's document is not held to the $L_2$ external ceiling for it, as with a file the operator ratified; promotion into always-loaded files still needs $L_4$ or higher **and** the ratification card for the protected files.

**3.7.4 Xenia bundles.** A bundle added `--from-xenia` is raised the same way, by the operator, locally. Nothing in a store record, its endorsements or its roster raises a tier; the lock's entry is not synced.

### 3.8 Trusted sources (OD-10, third option)
The operator may deem the **source** of a bundle trusted, so that the bundle can evolve and sync without a prompt at every version. It is the bundle-level counterpart of Delphi's own originating-repository rule for the self-updater (`specs/036`).
- **Anchor.** A trusted source is the pair **origin** and **publisher key**, never a name or URL alone. Origin is `xenia:<org>/<store>` for a store (not the record or revision) and `<kind>:<path or repository URL>` otherwise (`bundles::origin_of`, the `Source` the lock already records). An unsigned bundle cannot anchor one (no key).
- **Grant.** `delphi bundle add ... --trust-source` (with `--trust ratified` for the raised level), or any time `delphi bundle trust-source <bundle|origin> [--level user_ratified|default]` and `/org` key `s`. The same confirmation as §3.7, with the operator typing the **source id** (the origin). The text states that future updates from this source will be treated as validated (and raised, if the level is `user_ratified`) **without further prompts**, and that hash and signature verification, scanning, deny policies and every other guard still run on every update. The level is the operator's choice: `default` (citable as validated, $L_2$, the tier every bundle has) or `user_ratified`. It is stored per origin and key in `[[trusted_source]]` of the same lock, and shown in `bundle list`, `/org` (a `*` after the tier), the add summary, `trust_log` and `bundle.trust_changed`.
- **What it survives.** Unlike a one-off raise, a grant does **not** reset on content changes: new hashes, new versions, added or removed files and new revisions keep the grant and the tier, with no prompt; a rollback to an earlier version from the same source keeps it. The grant ends, with a message and the bundle at the default tier until the operator confirms again, only when the **source identity** changes: the origin differs from the recorded one; the publisher key is not the same key or, for a store, a key the **verified roster's `successor` chain** continues from it (the grant then follows the key); the update is unsigned or cannot be verified; or the operator revokes it (`delphi bundle trust-source <bundle|origin> --revoke`, `/org` `s <name> revoke`, no confirmation; bundles it raised go back to the default). A key that only the bundle's **own manifest** claims to succeed (`publisher.predecessors`) lets the update go through under §2.3 but does **not** extend the grant (the publisher claim on its own is not enough; §10.4 weakness).
- **A failed update is not a revocation.** An update that fails validation, signature verification or the scan is refused as before (`add` and `update` change nothing: the lock and the cache stay as they were) and the operator is told; the grant stays.
- **Same limits.** §3.7.1 (install-owned, never in a bundle), §3.7.3 (no guard lifted) and §3.7.4 (xenia: operator-only, local) apply unchanged.

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

11. **AC-11 (validated sources and the raise):** a bundle document's breadcrumb, the org-context header and the intent sentence say `validated bundle source`; a document whose frontmatter, a `trust` key in `bundle.toml`, or a shipped `bundles.lock`, state file or `trust.toml` claims `user_ratified` is still `federated_verified`; the raise needs a terminal on both ends and the typed name (refused with none, no `--yes` or environment switch), is per bundle, appears in `list`, `/org`, the add summary, the lock's `trust_log` and `bundle.trust_changed`, and is revocable; it resets on an update that changes the hash or publisher, on rollback and on remove; an edited lock without the hash and key binding raises nothing; `org.trust` is refused to the narrator; with a raised bundle a deny policy still refuses its command, `require_card` still raises its card, and a write under `@bundle/` is still denied.
12. **AC-12 (trusted sources):** a granted source keeps its tier through several updates that change, add and remove files, and through a rollback; it does not cover the same key at another origin, the same origin with another key, a key the bundle's own manifest merely names as predecessor, or an unsigned update; a tampered update is refused with the lock byte-for-byte unchanged and the grant intact; revoking lowers what it raised; a Delphi release never writes the lock.

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
- **Provenance** was always `unsigned` in B2; B6 verifies signatures (§10.4).
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
- **Not yet:** the `/org` view (B5). Stamps, the `bundle.*` events, bundle agents and the deny policies are slice B4b, §10.3.

### 10.3 As built (slice B4b: agents, policy, record)
`Implements-Spec: specs/024-bundle-trust-adoption-and-precedence.md` (AC-3 behavior part for agents, deny rules and stamps; AC-5, AC-8 rows on policy).
- **A bug in B2, fixed here:** `tools::zone_of` had no case for `@bundle/…`, so an agent's `read_file` of bundle knowledge fell into the release zone and got **L5**, the highest tier, whatever the bundle said. There is now `Zone::Bundle` whose origin default is **L2** (federated, like `team/`), and a read through a tool says L2. The index already used L2; this is the tool path.
- **Bundle agents:** `dispatch_agent` accepts `bundle:<bundle>/<agent>` (or `bundle:<org>/<name>/<agent>`; several bundles with one name are an error, never a guess). The span is the **host role's** (`bundle.toml` `[agents.<name>] role`, else the file's `delphi-role`; none means "Claude Code only" and is refused), so the host's manifest, scope, memory, taint and approvals apply unchanged. The agent's body follows the org guidance as a labelled block (at most 8 KiB, no-relaxation checked). The `tools` list is mapped from Claude Code names (`Read` to `read_file`, `read_doc`, `check`; `Grep` to `search`; `Glob` to `list_dir`; `Write`/`Edit` to `write_file`; `Bash` to `run_command`; `Task` to `dispatch_agent`) and **only removes** tools from the host's set: both when the tools are offered and when a call is made. A name with no equivalent (a web tool, an MCP tool) grants nothing. No `tools` list means the host's own. The handoff carries `bundle_agent` (`org/name:agent`), so a resumed span reloads the same agent; `dag.node.added` records it. Loading is verified like every bundle file and fails closed: an agent that cannot be loaded gets no tools and its span ends with the reason. Dispatchers are told what exists in a `@runtime/bundle-agents` slice.
- **Deny policies (`behavior/policies.toml`):** read fresh from the cache each time and verified, so `remove` takes effect at once; one that fails verification refuses rather than being skipped. `deny_commands` (a regular expression over the command line) refuses a `run_command` before any card; `red_paths` are added to `host_exec_paths` for the merge gate and the optimistic approver (so a change to one is Red at merge); `connector_denied` withdraws the tool from the role's list and refuses a forged call. `validate` step 10 now refuses a `red_paths` glob the gate cannot enforce: only `dir/**`, `dir/`, `*.ext` or one path. **`require_card` was accepted by the schema but not enforced; §10.5 enforces it.**
- **Stamps and events:** the first task a mission runs after a bundle is added appends `bundle.added` and one `provenance.stamp` per file (zone `bundle`, path `org/name/<rel>`): behavior, skills and agents at **L5** with reason `bundle-add:<hash>`, knowledge at **L2**. A new hash is `bundle.updated` (with `from_hash`), a removed bundle `bundle.removed`; a restarted engine reads what the mission was told from its log. This is the record, not the mechanism: what loads is read from the lock and the cache. `bundle.rolled_back` arrived with B6 (§10.4).
- **Not yet:** the console form `@<bundle>:<agent>` and `require_card` (both in §10.5), narrator rows for `bundle.*` and `/org` (both B5b, `specs/026` §11.2).

### 10.4 As built (slice B6: provenance, archive, update, rollback)
`Implements-Spec: specs/024-bundle-trust-adoption-and-precedence.md` (AC-1, AC-2, AC-7; `specs/027` AC-3, AC-4). `delphi bundle sign|package|update|rollback|gc` are new, `add` takes an `.xbundle` and `--replace-publisher`. Where the code differs from, or settles, the text above:
- **Signature.** `Domain::bundle` (`xenia-bundle/1`, fields as §2.1, no store field) and `delphi_xenia::bundle_sig` (parse, sign, verify) are in the xenia leaf crate; `delphi-bundle` depends on it (no cycle, still no engine). Validate step 6 verifies strictly over `org`/`name`/`version` read from `bundle.toml` and the bytes of `files.json`, requires the key to equal `publisher.did`, and, when a signed tree no longer matches its manifest, fails at step 6 as well as step 5 (so every flipped byte fails "at the signature step"). A verified signer is `Report::signer`. The known-bad vectors of `specs/011` §10 test 25 (identity-key forgery, non-canonical `S`) run through the bundle path. An unsigned bundle with `publisher.did` set gets a note recommending `sign`, not a warning.
- **Levels.** A directory or archive gets `unsigned`, `signed` (the table's "signed by <kid>, first time seen") or `known-publisher`; `endorsed` and `member-signed` need the store (B8). The lock pins `publisher_kid` at first add. A removed bundle pins nothing, so adding it again is a first add.
- **Continuity** (`delphi_bundle::provenance::continuity`, the §2.3 matrix): same key, or a key whose new manifest lists the pinned key in `publisher.predecessors`, is an update; any other key, or a signed bundle that turns unsigned, is a change that stops `update` and `add` with the §2.3 message and changes nothing until `add --replace-publisher`; an unsigned bundle's later version is refused by `update` and accepted by `add`. **Weakness the spec leaves:** the predecessor list is the new manifest's own word, and the old key does not countersign it, so whoever holds a new signing key can name the old key. A countersignature (the old key signing the successor's did) would close it; not built. The store successor chain (§2.3, `specs/011` §3.4) waits for B8.
- **`sign`** has no non-interactive mode: it needs stdin and stdout to be terminals, prints the file list with hashes, the bundle and the signer, and requires the first 8 hex digits of the bundle hash typed back. (The repository has no earlier TTY challenge on the xenia key, so this is its first; a `ratify` style daemon prompt would be a larger change.) It uses `$DELPHI_HOME/xenia/identity.key`, refuses when `publisher.did` is not that key's did, refuses a tree that does not validate for packaging or a version below `--against`, and needs a current `files.json` (`manifest` first). Nothing prints key material; a signature file holds the public did only.
- **`package`** refreshes `files.json` (and `plugin.json` if the bundle has one), validates for distribution, and writes a ustar archive with the `tar` crate (a dependency of `delphi-bundle` now, already in the workspace through `delphi`): sorted bytewise, regular files, mode 0644, no owners. **mtime is 0, not `files.json`'s write time**: the latter differs between two checkouts and would break the byte-reproducibility the same sentence asks for. The default output is `<name>-<version>.xbundle`; it refuses a path inside the bundle. `package --vectors` is B3.
- **Import** (`delphi_bundle::archive`): the reader is hand-written over raw 512-byte headers, because the `tar` crate silently interprets pax and GNU extensions. It accepts only ustar regular files, rejects links, directories, devices, extensions, base-256 numbers, bad checksums, names with `..`, absolute, empty or odd components, depth over 12, entries out of bytewise order or repeated, non-zero padding and data after the end marker. Caps are checked from each header before its data is read: 2000 files, 1 MiB each, 64 MiB outside `vectors/`; under `vectors/` 64 files, 256 MiB each, 1 GiB in all. Files are made with `create_new` in a fresh `0700` directory under `bundles/.incoming/`, then the full `validate` runs there and the very tree is renamed into the cache; any failure removes the staging directory. `add` decides directory or archive by whether the path is a file.
- **`update [name]`** re-adds from the lock's `source` (directory or archive; a xenia or marketplace source is refused for now), under the continuity rule, and prints the notice: the top `CHANGELOG.md` entry, the version bump and whether it matches `validate --against` against the previous cached tree (reported, not enforced), and added/changed/removed file counts per section. "Superseded" ids and kept-vector counts are not reported (B3). Same hash and level is "up to date". A source that now holds another bundle is refused.
- **`rollback <name> [--to <hash>]`** re-validates the cached tree (it must still hash to the entry), then swaps the lock: the replaced version goes to the head of `history` marked `left_by = "rollback"`, the target leaves `history`, and the version, hash, key and level it had are restored (`history` entries now carry `publisher_kid`, `provenance`, `gone`, `left_by`; an older lock is read as before and the key is re-derived from the cached `sig.json`). The index follows at once; prompt content from the next task.
- **`gc`** keeps the current tree and `KEEP_HASHES - 1` earlier ones per active bundle (the "last 3 hashes" include the current one), marks the rest `gone` in the lock, and deletes every cached tree that no workspace's lock still needs (the cache is shared between workspaces), including the trees of removed bundles, plus staging directories older than an hour. There is no `--dry-run`. It is `delphi bundle gc`, not `delphi gc`. A tree's vector verdicts (`<hash>.vectors.json`, beside the tree, `bundle_vectors::verdicts_path`) are removed with it, orphaned verdict files are swept, and the empty `<org>/<name>` and `<org>` directories go last. `rollback` re-runs the vector adoption after it re-points the lock, so the older tree's surviving verdict applies at once; `update` to a new hash starts without a verdict and the vector pass writes its own, with unchanged chunks keeping their vectors (`upsert_with`, B3).
- **Events.** The engine appends `bundle.rolled_back` (with `from_hash`) instead of `bundle.updated` when the head of the entry's history is marked as left by a rollback and is the hash the mission last saw; the Narrator says "rolled back".
- **`/org`.** `rollback` is a key (`r`, name and optional hash), and `update` now runs `delphi bundle update <name>`, so a publisher change is refused there too. `org.action` validates `to` as 8 to 64 lowercase hex digits (optionally `blake3:`) before it reaches the command line. "Update available" in `/org` and `doctor` was not built here; §10.5 builds it.

### 10.5 As built (batch A of the deferred items: `require_card`, `@bundle:agent`, update available)
`Implements-Spec: specs/024-bundle-trust-adoption-and-precedence.md` (AC-3 and AC-8 policy rows; §3.5).
- **`require_card`** (`[[policy.require_card]]` with `pattern`, a regular expression, and an optional `reason`). It applies to every Green read-kind tool call that has a `path` argument (`read_file`, `read_doc`, `list_dir`, `check` and the like), matched against the path **as the agent wrote it** (`@project/app/secrets/x.env`, `missions/…`), unanchored. A match raises the tool's tier from Green to Amber in `engine::exec_tool` before `broker.need`, so the normal Amber rules apply: a card, "allow similar" for the rest of the task (the pattern is the tool and path), the permission modes do not auto-approve a read, a deny hook can still say no. The card's summary names the bundle and the reason. It never lowers a tier and never touches a call that is already Amber or Red. Like every policy it is read fresh and verified on each call, and a policy that fails verification refuses the read (fail closed), so the cost is one hash check of `files.json` and `policies.toml` per read while any bundle is added. Not covered: `search` and `dispatch` results, which have no path; a read made by the daemon on an agent's behalf (the org-context slice, the index) is not a tool call and is not asked about.
- **`@<bundle>:<agent>`** from the console: the shell console (`chat.rs`) and the Ink console (`ui/src/complete.ts` `bundleTarget`, `app.tsx`) send `task.submit` with `target = "bundle:<bundle>:<agent>"` (or `bundle:<org>/<name>:<agent>`). A first word that looks like a path (`@specs/a.md:12`, first segment `specs`, `project`, `bundle`, `runtime`, `team`, `missions`) is not taken for one. The daemon resolves it with the same loader as `dispatch_agent` (verified, no-relaxation checked, several bundles with one name is an error, never a guess) and answers `INVALID_PARAMS` with the loader's reason before any task starts; a host role that is not enabled is refused too. The task then runs as the **host role** with the agent's block and the intersected tool list, `task.created` and `dag.node.added` carry `bundle_agent`, and the handoff does, so a resumed span reloads the same agent. There is no completion list for bundle agents in the Ink console yet (`Info` does not carry them), and a timer cannot target one.
- **Update available** (`delphi_bundle::source_check`): a light, offline look at the lock's recorded source: for a `directory` source its `.delphi-bundle/files.json` hashed with `bundle_hash`, `bundle.toml`'s version, and whether a `sig.json` exists; for an `archive` the same two files and `sig.json` read from the tar (each capped at 4 MiB; nothing is extracted). A different hash is "available", with the new version and the publisher state taken from `continuity` over the pinned key and the new manifest's **claimed** `publisher.did` and `predecessors` (a claim, not a verification: only `update` verifies a signature): `same publisher`, `unsigned before: needs add`, or `publisher changed: needs --replace-publisher`. A source it cannot read (moved, a `xenia` or `marketplace` kind) is **unknown**, never "up to date". It does not run `validate`: a source whose `files.json` is stale against its files shows as available and `update` then refuses it with the validation errors. `org.view` fills `update_available` (`"1.5.0 (same publisher)"` or `null`), adds `update_note` (why a source could not be looked at) and an open item per available update; the `/org` table shows the version in its `update` column. `delphi doctor` check 12 gives a warning per bundle (`update available: 1.0.0 -> 1.5.0 (same publisher) at <source>; delphi bundle update <name> applies it`), which does not change the exit code. Nothing is fetched and nothing applied: `update` stays the operator's command. `delphi xenia sync` and `--auto-update-bundles` still wait for B8.

### 10.6 As built (slice B8: the store levels)
`Implements-Spec: specs/027-bundle-authoring-distribution-and-evaluation.md` (§10.4 has the slice; this records what it settles here).
- **Levels.** `endorsed` and `member-signed` exist now (`provenance::Level`). They are written only by `delphi bundle add --from-xenia` and by `bundle_xenia::sync_bundles`, from the store's computed standing (`BundleLevel` in `delphi-xenia`), never by `continuity` and never from anything in the tree or the record. The lock's `provenance` for a store bundle is recomputed on every store sync (a statement that expired drops `endorsed` to `member-signed` with the hash unchanged, and a fresh one brings it back); the add summary says `endorsed by <handles> (store <store>)` or `signed by <handle>, member of <store>, not endorsed` (`describe_store`, with "store stale" when no endorsement counts). `bundle serve` accepts both words.
- **Continuity through the roster.** For a store bundle the pinned key is continued by the new signer when the verified roster's `successor` chain says so (`StoreBundle::predecessors`), as well as by the manifest's `publisher.predecessors`; any other key is a publisher change that needs `--replace-publisher`, and the message names `--from-xenia`. The bundle's own `sig.json` must be by the record's signer or a key in that signer's chain in either direction (`StoreBundle::signer_dids`).
- **Update available** for a `xenia` source reads the store's last verified state from disk (`source_check::check_in`; no network): a newer `rev` or another hash shows once a sync has brought it in.


### 10.7 As built (OD-10: validated sources, the raise, trusted sources)
`Implements-Spec: specs/024-bundle-trust-adoption-and-precedence.md` (§3.7, §3.8; AC-11, AC-12). Where the code settles or differs from the text:
- **Where.** `delphi_core::bundles` (`Entry::raised`, `tier_name`, `label`, `TrustedSource`, `origin_of`) and `delphi_core::bundle_trust` (`prepare` builds the confirmation text and the word to type; `apply` changes the lock; neither asks anyone). The command line (`delphi bundle trust`, `trust-source`, `add --trust ratified --trust-source`) is `crates/delphi/src/bundle_trust.rs` with the terminal check; the console is the `org.trust` RPC in `service.rs`; the lock logic for update and rollback is in `bundle_add.rs` and `bundle_life.rs`.
- **The terminal check** is `isatty(0) && isatty(1)` and the typed name, as `bundle sign`; `--trust` / `--trust-source` on an `add` are refused **before** anything is added when there is no terminal. A declined or failed confirmation after an add leaves the bundle added at the default tier and says so.
- **Source anchor on update.** The grant is looked up by the **new** add's origin (from the recorded `Source`) and key: equal key, or a key in the store roster's chain (`store_predecessors`, which `provenance::continuity` also uses). The manifest's own `publisher.predecessors` is deliberately **not** used for the grant. If the existing entry had a grant and this add does not match, the output says "this version is NOT from the trusted source" and the bundle is at the default tier. A rotated key that the roster continues moves the grant to the new key (and says so).
- **Marketplace origin.** The lock records a marketplace add as `<repo>#<name>@<commit>`; the origin is the repository as the operator gave it, **without** the commit (a new commit is the same origin, a different repository or mirror URL is another). Origin is compared as a recorded string: Delphi does not canonicalize or follow a URL for this purpose, so a redirect to another repository is not recognized as the same source unless the recorded string is.
- **No cap in the tool path.** `tools::observe` takes the tier from the mission's stamps (`provenance.stamp`), which `reconcile_bundles` rewrites when the lock's tier changes; a long-running task that already read a document keeps the tier it read it at. The index, `bundle serve`, the org-context slice and `/org` read the lock directly.
- **Narrator.** `bundle.trust_changed` joins `observe::KINDS`; the Narrator says "knowledge tier raised to user_ratified (cli)" or "back at federated_verified (update)".
- **UI.** `/org` gains `t` and `s` and a `tier` column (`validated`, `ratified`, `*` for a trusted source); the card text is shown above a `confirm›` prompt, and esc sends nothing.
- **Not built.** A trusted source for `directory` origins is per canonical path (a copy elsewhere is another origin, by design). `delphi bundle trust` has no per-file raise (that is the existing promotion card, §3.3).

## 11. Open Questions
B-6 (person scope) in `specs/022` §7. B-6 is decided 2026-10-28: later. Everything else in this spec is decided (OD-1, OD-3) or ratified as recommended (2026-10-28).
