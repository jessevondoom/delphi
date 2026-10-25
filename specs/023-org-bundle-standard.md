---
id: spec-023-org-bundle-standard
title: "023: The Org Bundle Standard: Manifest, Layout, Knowledge, Skills & Behavior Sections"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the format is reasoned against the external formats cited in specs/025; nothing is built
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, knowledge, skills, schema, post-v1]
---

# Spec 023: The Org Bundle Standard

> **Status: draft, proposed.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Nothing in this document is built. Where it says "Delphi does", read "Delphi would, once the slice in `specs/022` §6 that names this section is merged".

## 1. Scope & Objectives
An **org bundle** is a directory an organization publishes so that every Delphi install in that organization starts from the same knowledge, the same working skills and the same stated goals and perspective. The operator's intent (2026-10-25): Delphi is not a replacement for Claude Code; it is a tool that accelerates humans working in a specific collaborative environment, and the critical chore is a **standard** by which an organization's knowledge and skills can be bundled, defined as a plugin, and worked against by a team.

This spec defines:
1. The **bundle manifest** `bundle.toml` (§3) and the **directory layout** (§2).
2. The four content sections: **knowledge** (§4), **skills** (§5), **behavior** (§6) and **evals** (§7), with their file formats, limits and the deterministic checks each must pass.
3. **Versioning and compatibility** fields (§8) and the **validation order** (§9).
4. A **worked example**: a complete sample bundle tree with its manifest (§10).

What a bundle may *do* on an install (adoption, trust tiers, precedence, update, rollback) is [`specs/024`](024-bundle-trust-adoption-and-precedence.md). How the layout maps onto Claude Code plugins, Agent Skills, MCP and `AGENTS.md` is [`specs/025`](025-plugin-skill-and-tool-compatibility.md). How bundle content reaches the strategist and the console is [`specs/026`](026-team-alignment-and-strategist-integration.md). Authoring, transports and evaluation are [`specs/027`](027-bundle-authoring-distribution-and-evaluation.md).

### 1.1 Non-Goals
- Runtime behavior (loading, tiers, approvals): `specs/024`.
- Anything a bundle cannot carry by design: executable hooks, credentials, permission grants, `allow` rules, operator-scope content (§9 lists the refusals; the reasons are `specs/024` §6).
- A query language or taxonomy for knowledge. A bundle ships Markdown with the provenance frontmatter Delphi already lints (`specs/001` §5); retrieval is the existing hybrid index (`specs/004` §3).
- Replacing `specs/011`/`012`. A bundle travels through xenia as one `bundle` record pointing at a pinned tree (`specs/027` §4); the record kinds and the trust model stay where they are.

### 1.2 Simpler Alternative Considered
- **Alternative 1: a shared `team/` git repository, cloned into every workspace, and nothing else.** This is `specs/007` Plane 3 and it already covers "the team edits the same files". Rejected as the standard because (a) it has no manifest, so nothing says what the repository is for, who publishes it, what version it is, or which roles it touches; (b) `team/` is one zone with one default tier, so a bundle from a vendor, a bundle from a sister team and the team's own notes become indistinguishable; (c) it cannot be handed to a Claude Code user, a vendor or an air-gapped machine as one artifact; (d) it has no place for goals and perspective that the strategist can be *told* about rather than left to find.
- **Alternative 2: a plain Claude Code plugin (`skills/`, `agents/`, `.mcp.json`) with knowledge stuffed into skills.** Closest to a real standard, and this spec keeps its layout as the compatibility target (`specs/025`). Rejected as the whole answer because a plugin has no notion of provenance tiers, review dates, pointers to secure sources, goals, perspectives or tighten-only policy, and because its `agents/`, `hooks/` and `bin/` carry executable authority that Delphi must refuse (`specs/024` §6). A bundle is therefore a **superset of a plugin**: every bundle is a valid plugin directory for the parts a plugin understands, and Delphi reads the rest.

### 1.3 Design Principles
1. **Knowledge is data; behavior is instructions.** Everything under `knowledge/` is retrieved, bounded and spotlighted and is never always-loaded. Everything under `skills/` and `behavior/` is instruction text an agent will follow and loads only after the operator ratifies that exact file hash (GOVERNANCE §4.3, §5.7). The two never mix in one file.
2. **A bundle can never widen anything.** No field in a bundle grants a permission, allows a command, pre-approves a tool, adds a hook or asks for a credential. Where a compatible format has such a field (`allowed-tools`, `hooks`, `userConfig`), Delphi ignores it with a note or refuses the bundle (§9). Narrow-only is already the rule for every override (`specs/001` §2.3, D-21); bundles inherit it.
3. **Mostly retrieval, rarely always-loaded.** A bundle may propose at most a handful of always-loaded rules (§6.4) and at most one bounded guidance block per role (§6.3). The Rule of 25 (GOVERNANCE §6) is not relaxed for bundles; the bulk of an organization's knowledge belongs in `knowledge/`, where there is no cap on size, only on what one span can see at once.
4. **Compatible by construction, not by conversion.** `skills/` is the Agent Skills format with Delphi's extra fields in `metadata` exactly as `specs/019` already exports them; the layout is the Claude Code plugin layout; pointers are the xenia pointer format. Nothing in a bundle is a Delphi-only re-spelling of something another ecosystem already names.
5. **Everything is checkable without a model.** `delphi bundle check` (§9) is deterministic. A bundle that passes it is well-formed; whether it is *true* or *useful* is for stewards (`specs/011` §7.3) and for evaluation (`specs/027` §6).
6. **One identity across transports.** The bundle's identity is the BLAKE3 of its file manifest (§3.4). A directory, an archive and a xenia record that share that hash are the same bundle, whichever way it arrived.

---

## 2. Directory Layout
```text
<bundle-root>/                         # the plugin root in Claude Code terms (specs/025 §2)
├── bundle.toml                        # REQUIRED. The Delphi manifest (§3). Human-edited; the source of truth
├── README.md                          # RECOMMENDED. For humans; never loaded into a prompt
├── LICENSE                            # RECOMMENDED when distributed beyond the org
├── .delphi-bundle/                    # machine-written by `delphi bundle pack` / `sign` (specs/027 §3); never hand-edited
│   ├── files.json                     #   the file manifest: path -> {blake3, bytes} for every file in the bundle (§3.4)
│   └── sig.json                       #   detached Ed25519 signature over files.json (specs/024 §2); absent in an unsigned bundle
├── .claude-plugin/
│   └── plugin.json                    # OPTIONAL. Generated or verified by `delphi bundle check --claude` (specs/025 §2)
├── knowledge/                         # §4: data. Markdown with provenance frontmatter; indexed, retrieved, never always-loaded
│   ├── index.md                       #   REQUIRED when knowledge/ exists: the pointer index (path, heading, one-line summary)
│   ├── glossary.md                    #   RECOMMENDED: terms the org uses, one definition each
│   ├── decisions/                     #   RECOMMENDED: one decision record per file (§4.3)
│   └── <any>/**.md                    #   domains, architecture notes, runbooks, how-tos, postmortem summaries
├── skills/                            # §5: Agent Skills format, one <name>/SKILL.md per skill, Delphi fields in `metadata`
│   └── <skill-name>/
│       ├── SKILL.md
│       ├── references/                #   on-demand reading (Agent Skills convention)
│       └── templates/                 #   output contracts (Delphi convention, specs/001 §3); `assets/` is accepted as a synonym
├── behavior/                          # §6: instructions and stated intent, ratified per file
│   ├── goals.md                       #   §6.1 what the org is trying to achieve, as bounded, dated, measurable entries
│   ├── perspectives.md                #   §6.2 lenses the org applies to decisions; given to the strategist and Council seats
│   ├── roles/<role>.md                #   §6.3 bounded "org guidance" for a canonical role; instructions only
│   ├── heuristics.md                  #   §6.4 candidate team rules [T-...], Trigger/Directive/Falsification, at most 10
│   └── policies.toml                  #   §6.5 tighten-only approval policy (denies, Red upgrades); allows are refused
├── evals/                             # §7: how to tell whether the bundle helps
│   └── delphi/
│       ├── kb-eval.json               #   retrieval questions with expected pointers (the `dev/kb-eval.json` format)
│       └── tasks/*.md                 #   task cases with a hidden rubric, for the bench runner (specs/027 §6)
└── <anything else>                    # ignored by Delphi with a note (§9 step 3); must still pass the name and size rules
```

**Rules that apply to the whole tree** (checked in §9, step 2):
- Regular files only. No symbolic links, hard links, device nodes, FIFOs, sockets or submodule gitlinks. No `.git` segment at any depth. No absolute or `..` path. No path component that differs from another only by Unicode normalization or case. Names match `^[A-Za-z0-9._-]{1,64}$` per component (the `README.md`, `LICENSE` and `SKILL.md` spellings are the only uppercase names Delphi itself writes).
- **Never executable.** Files are written to the cache without the execute bit (`specs/011` §11.1 rule, extended to the whole bundle). A bundle has no `bin/` (Claude Code's `bin/` is on the Bash `PATH` while a plugin is enabled; Delphi refuses a bundle that has one, §9).
- **Size.** At most 2,000 files, 1 MiB per file, 64 MiB in all. Markdown bodies under `knowledge/` at most 256 KiB each (the existing `MAX_READ_BYTES`), `SKILL.md` under the 500-line cap of `specs/001` §3, behavior files under the caps in §6.
- **Text.** UTF-8, NFC-normalized, LF line endings normalized by the packer before hashing (as `specs/012` §3), no control characters except `\n` and `\t`, no bidi controls, no `U+E0000`–`U+E007F` tag characters. Binary files are allowed only under `skills/*/templates/`, `skills/*/assets/` and `knowledge/**` with the content types `image/png`, `image/svg+xml`, `application/pdf`, `text/csv`; they are never put in front of a model except through the document readers of `specs/014`.

---

## 3. The Manifest: `bundle.toml`
TOML, because every other ratified configuration in Delphi is TOML and the no-widening linter already reads it. Unknown keys are **errors**, not warnings: an ignored key is a smuggling channel (`specs/012` §5), and a typo in a pinned file must not pass silently (`specs/020` §3).

```toml
[bundle]
schema = 1                                  # bundle schema major; a reader rejects a higher major it does not know (§8)
name = "acme-platform"                      # ^[a-z0-9][a-z0-9-]{0,47}$ ; equals plugin.json `name` when that file exists
org = "acme"                                # ^[a-z0-9-]{1,32}$ ; equals the xenia `org` when published through a store
version = "1.4.0"                           # semver (MAJOR.MINOR.PATCH, optional pre-release); see §8 for what each part means
title = "Acme Platform Engineering"         # <= 80 chars, plain text
description = "How Acme builds and runs its platform: architecture, decisions, runbooks, review habits, this year's goals."
                                            # <= 200 chars, plain text, no Markdown links (the same cap as a skill description)
license = "Proprietary"                     # SPDX identifier, or a short phrase naming the bundled LICENSE file
homepage = "https://wiki.acme.example/platform-bundle"   # OPTIONAL; https only
audience = "org"                            # org | team | project | person: the INTENDED scope. Routing, not secrecy (specs/011 §5.4)
review_by = "2027-01-31"                    # REQUIRED; after it every surface shows the bundle as stale (nothing is unloaded)
language = "en"                             # BCP 47; what the prose is written in

[compat]
delphi = ">=0.3.0"                          # semver requirement on the Delphi version; adoption refuses outside it
bundle_schema = 1                           # repeated for readers that skip [bundle]; must equal bundle.schema

[publisher]
name = "Acme Platform Engineering"          # <= 80 chars
contact = "platform-eng@acme.example"       # OPTIONAL; display only
did = "did:key:z6Mk..."                     # OPTIONAL in a directory bundle; REQUIRED to sign (specs/024 §2) and to publish through xenia
predecessors = []                           # OPTIONAL: earlier publisher DIDs this one succeeds (continuity across rotation, specs/024 §2.3)

[applies_to]                                # retrieval and routing hints; the same shape as a xenia `memory` record (specs/012 §5.1)
repos = ["acme/platform-*", "acme/billing"] # logical repository names or globs; never local paths
tech = ["rust", "postgres", "kubernetes"]
paths = ["services/**", "infra/**"]         # OPTIONAL repo-relative globs that the knowledge mostly concerns

[sections]                                  # which sections exist. A listed directory must exist; an unlisted one is ignored with a note
knowledge = true
skills = true
behavior = true
evals = true

[touches]                                   # declared so the adoption card can show it; checked against the tree (§9 step 9)
roles = ["strategist", "architect", "builder", "red-team"]   # canonical IDs only (D-17); the roles that skills/ or behavior/roles/ name
connectors = ["slack"]                      # names of connector catalog entries the bundle proposes (specs/025 §4); none run without ratification
network = false                             # a bundle never needs network at adoption; `true` is refused in schema 1

[overlays]
depends_on = ["acme-security@>=2.0.0"]      # OPTIONAL: other bundles (name@semver-requirement) that must be adopted first (specs/024 §4)
supersedes = ["acme-platform-legacy"]       # OPTIONAL: bundles this one replaces; adoption offers to remove them
```

### 3.1 Field rules that carry trust
- `name`, `org` and `version` are bound into the signature domain (`specs/024` §2), so a bundle renamed, re-versioned or moved to another org fails verification.
- `publisher.did` must equal the signing key's DID when `.delphi-bundle/sig.json` exists. A bundle cannot claim another publisher.
- **No field states a trust tier.** `tier`, `trust`, `source_type`, `endorsed`, `verified`, `approved` and `ratified` are rejected anywhere in `bundle.toml` (as in `specs/012` §2.1). Trust is computed by the receiving install (`specs/024` §3).
- **No field grants.** There is no `permissions`, `allow`, `allowed_tools`, `hooks`, `env`, `secrets`, `user_config` or `mcp_servers` key in `bundle.toml`. MCP servers a bundle wants to propose live in `.mcp.json` in the Claude Code shape and become connector-catalog *proposals* (`specs/025` §4); nothing else is read from them.

### 3.2 Why `audience` is routing, not secrecy
A bundle is one artifact. Everyone who can open it can read all of it, exactly as every reader of a xenia store reads every summary (`specs/011` §5.4). `audience` tells an install *where to place* the bundle (`specs/024` §5) and tells the strategist how general the goals are. Sensitive detail belongs behind a pointer (§4.4) in the system that already controls access to it.

### 3.3 `applies_to` is how retrieval stays relevant
A workspace with `projects` that match none of `applies_to.repos` still indexes the bundle, but the org-context slice (`specs/026` §3) marks it "adopted, not matched", and search results from it rank below matched bundles by a fixed fusion penalty rather than being hidden. Nothing is silently dropped.

### 3.4 The file manifest `.delphi-bundle/files.json`
```json
{
  "bundle": 1,
  "name": "acme-platform", "org": "acme", "version": "1.4.0",
  "files": {
    "bundle.toml":                      { "blake3": "blake3:<64 hex>", "bytes": 1831 },
    "knowledge/index.md":               { "blake3": "blake3:...",      "bytes": 4120 },
    "skills/incident-review/SKILL.md":  { "blake3": "blake3:...",      "bytes": 6210 }
  }
}
```
- Every regular file in the tree except `.delphi-bundle/files.json` itself and `.delphi-bundle/sig.json` appears exactly once, keys sorted bytewise, no other keys, strict JSON, at most 1 MiB.
- The **bundle hash** is `blake3(files.json raw bytes)`. It is the bundle's identity in the lock file, the cache directory name and the xenia record's pin (`specs/027` §4). `delphi bundle check` recomputes every entry; one mismatch fails the whole bundle.
- The spelling `blake3:` plus 64 lowercase hex follows the choice recorded for xenia (`xenia-phase-1-plan.md` §3, X1).

---

## 4. The Knowledge Section (`knowledge/`)
### 4.1 What goes here
Anything the organization knows that an agent should be able to *find*: architecture overviews, how things are deployed, who owns what (by role or team, never personal data beyond a work handle), the vocabulary, the decisions and why, the gotchas, the runbooks, summaries of past incidents and postmortems. Prose, tables and Mermaid diagrams. Not transcripts, not raw logs, not secrets, not customer data (§9 step 8 scans for all of these).

### 4.2 File format
Markdown with the **provenance frontmatter of `specs/001` §5**, so `delphi lint` and `delphi check provenance` apply unchanged, plus four optional fields:

```yaml
---
id: acme-platform/architecture/overview        # REQUIRED; unique within the bundle; `<bundle name>/<path without .md>` by convention
title: "Platform architecture overview"
author: operator                                # a canonical role, or `operator` for human-written (D-17)
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated                 # a DISPLAY COPY (specs/001 §5); the install stamps the real tier at adoption (specs/024 §3)
confidence: medium
status: active                                  # active | superseded; superseded needs `superseded_by`
source_mission: acme-platform                    # the bundle name, so the field is never a local mission id
tags: [architecture, kubernetes]
summary: "Three clusters, one control plane; services talk over mTLS; billing is the only stateful exception."   # <= 160 chars, plain text (specs/013 §5)
domain: platform                                # OPTIONAL; one of the bundle's own vocabulary (knowledge/glossary.md)
review_by: "2027-03-01"                         # OPTIONAL per file; defaults to the bundle's review_by
pointers:                                       # OPTIONAL, <= 8, the xenia pointer format (specs/012 §4); display-only (§4.4)
  - { scheme: https, locator: "https://wiki.acme.example/platform/arch", pin: { kind: none }, label: "Full architecture wiki", content_type: text/markdown }
---
Body. Data, never instructions. Headings every few paragraphs: the index chunks by heading (specs/004 §3.2).
```

- `summary` is what `knowledge/index.md` and the org-context slice show; it is the 160-character field `specs/013` §5 proposed, adopted here for bundles only (a `specs/001` amendment is still needed to add it to local files; `specs/022` §7 lists it).
- A knowledge file **must not** contain instruction-shaped text aimed at an agent ("always", "never", "you must", tool names, approval words) outside a fenced quotation. The §9 linter uses the same deterministic no-relaxation check as theme packs and rule files (`specs/001` §6.4, `specs/004` §5.2), applied as a **warning** with the line shown, because prose about deployment legitimately says "never deploy on Fridays". The operator sees the warnings on the adoption card. Instructions belong in `behavior/` where they are ratified.

### 4.3 Decision records (`knowledge/decisions/`)
One file per decision, same frontmatter, with three required headings in the body: `## Decision`, `## Why`, `## Consequences`, and an optional `## Superseded by`. The `id` carries a stable decision number (`acme-platform/decisions/0042-postgres-over-dynamo`). The strategist's alignment check (`specs/026` §5) reads these by id, so a bundle that changes a decision publishes a new file that `supersedes` the old one rather than editing it in place; the old file stays with `status: superseded`.

### 4.4 Pointers are display-only
A pointer says "the full material is at X" and is shown as text (scheme, locator, label, `access_hint`). Delphi never fetches a pointer target when adopting or reading a bundle (`specs/011` §6, unchanged). `file:` and local paths are refused. This keeps the bundle thin and keeps access control where it already is (the wiki, the drive, the repository).

### 4.5 `knowledge/index.md`
Required when `knowledge/` exists. One list item per file, in the order the author wants a reader to meet them: a Markdown link whose text is the file's `title` and whose target is the file's path relative to `knowledge/`, then a colon and the file's `summary`. `delphi bundle check` fails on a knowledge file missing from the index or an index line pointing at nothing. The index is the first thing the org-context slice offers (`specs/026` §3) and the first thing a new teammate reads.

---

## 5. The Skills Section (`skills/`)
Exactly the Agent Skills format (`agentskills.io/specification`, read 2026-10-25) with Delphi's fields where `specs/019` already puts them:

- `skills/<name>/SKILL.md` with frontmatter `name` (equal to the directory, `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 64 characters), `description` (Delphi caps at 200 characters, stricter than the standard's 1,024; a longer one is refused with a request to shorten, never cut), optional `license`, `compatibility`, `metadata`, `allowed-tools`.
- Delphi's `authorized_agents`, `input_schema`, `output_schema` and `required_permissions` as JSON strings under `metadata.delphi.*`, which is what `delphi skills export` writes today (`specs/019` §3). A `SKILL.md` that carries them as top-level keys (the native Delphi form) is also accepted; `check` says which form it found and `pack` normalizes to the `metadata` form so the bundle stays a valid standard skill.
- **`allowed-tools` is ignored with a note.** It names another tool's tools and pre-approves them; Delphi's permission model has no pre-approval that a file can grant (`specs/019` §3).
- **`scripts/` is not executed** by Delphi in bundle schema 1: the directory may exist (a Claude Code user may want it), `check` lists its files, and adoption copies them without the execute bit and never declares them in `deterministic_scripts`. Whether a later schema allows scripts under the adoption-is-ratification rule of `specs/011` §11.1 is open question B-7 in `specs/022`.
- `required_permissions` must be satisfiable within the **built-in** `default_access_scope` of every role in `authorized_agents` (`specs/001` §2.3 as refined by 001-A1). A skill that needs more is refused at `check`, not at adoption, so a publisher learns early.
- Role names in `authorized_agents` are **hints**. The operator names the roles at adoption (`specs/019` §3 rule: a foreign skill never decides who may use it); the hint pre-fills the card.

A skill in a bundle is therefore usable unchanged by Claude Code (which reads `name`, `description` and the body) and by any Agent Skills client, and gains Delphi's schemas and permission fit when adopted here.

---

## 6. The Behavior Section (`behavior/`)
Everything here is instruction text or stated intent. **None of it loads before the operator ratifies that file's hash** (`specs/024` §3). Each file has the provenance frontmatter of §4.2 (so the same linter applies) and the caps below.

### 6.1 Goals (`behavior/goals.md`)
```markdown
## [G-acme-01] Cut p99 checkout latency below 400 ms
- **Horizon:** 2026-Q4 to 2027-Q1
- **Owner role:** architect              <- a canonical role or a team handle, never a person's name
- **Measure:** p99 of `checkout.complete` in the production dashboard, weekly
- **Status:** active                      <- active | met | dropped | superseded
- **Why:** conversion drops 3% per 100 ms over 400 (see knowledge/decisions/0031-latency-budget.md)
```
At most **12 active goals**, each at most **600 characters**, ids `[G-<org>-NN]` unique within the bundle. Goals are what the strategist is told the organization is trying to do (`specs/026` §3); they are *information*, and a goal can never authorize an action, waive a gate or change a tier. The no-relaxation linter runs on the text.

### 6.2 Perspectives (`behavior/perspectives.md`)
A **perspective** is a lens the organization wants applied when a decision is made: "prefer boring technology unless the boring option is the bottleneck", "every external call has a budget and a fallback", "a migration without a rollback plan is not a plan".
```markdown
## [P-acme-03] Boring technology by default
- **Statement:** choose the well-understood option unless it is measurably the bottleneck.
- **Apply when:** picking a datastore, queue, framework or language for a new service.
- **Counter-case:** the boring option cannot meet a stated goal's measure (cite the goal).
```
At most **12 perspectives**, each at most **350 characters** across the three lines (the rule-size cap of GOVERNANCE §6, so a perspective is never longer than a rule). The Council can assign one perspective per seat as its Round 1 framing (`specs/026` §4). A perspective is advice to a model, never an instruction to the runtime; the linter rejects any that mentions tools, approvals, taint, `REVIEW.md`, "ignore" or "override" (`specs/001` §6.4 keyword check).

### 6.3 Role guidance (`behavior/roles/<role>.md`)
Bounded, instruction-only guidance for one canonical role: how this organization wants its architects to write specs, what its builders must check before a migration, what its red-team should always try. **It is not a manifest and never merges with one.** At adoption it becomes an **org guidance block** of at most **2 KiB** per role, placed in the prompt after governance and the role manifest and before memory (an amendment to the IPR order of `specs/002` §2, listed in `specs/022` §7). Rules:
- Body only; frontmatter is §4.2 plus `role: <canonical id>`. No `default_access_scope`, `authorized_skills`, `execute` or any manifest key: their presence fails `check`. Permissions are never touched by a bundle; a role that needs an org skill gets it through the operator adding the ratified skill to `authorized_skills` (001-A1).
- One file per role, roles from the nine canonical ids (or `librarian` once `specs/013` is built). A file for an unknown role fails.
- The no-relaxation linter runs as an **error** here (not a warning as in `knowledge/`): guidance that says "skip approval for docs" is refused outright, as an operator override saying it would be (`specs/004` test 15).
- The governance block of the IPR states that org guidance cannot override it, exactly as it states for memory and overrides (`specs/004` §5.2).

### 6.4 Candidate team heuristics (`behavior/heuristics.md`)
At most **10 rules** in the Trigger / Directive / Falsification form with ids `[T-<org>-NN]`, each under 350 characters (`delphi check rules` applies). At adoption every rule is a **proposal** into `context/team-heuristics.md` (`specs/004` §5.1 team scope), subject to the Rule of 25 and the 6 KB cap. A bundle cannot force a rule in; it offers. Overflow is reported, never applied (`specs/007` §4 item 3). The ids keep the bundle name so a `/retro` can tell which bundle a rule came from and `rule_stats` (`specs/004` §5.6) can score it.

### 6.5 Tighten-only policy (`behavior/policies.toml`)
```toml
[policy]
schema = 1

[[policy.deny_commands]]                 # becomes a `[[hooks.deny]]`-equivalent rule (specs/018): can only refuse
pattern = "^terraform (apply|destroy)"
reason = "Infrastructure changes go through the platform pipeline, never from an agent."

[[policy.red_paths]]                     # a worktree write matching this glob is Red at merge, like the host-executed files of GOVERNANCE §5.5
glob = "infra/prod/**"
reason = "Production infrastructure."

[[policy.connector_denied]]              # a connector tool that is never offered to a role while this bundle is adopted
tool = "slack.post_message"
roles = ["*"]
```
Accepted keys: `deny_commands`, `red_paths`, `connector_denied`, `require_card` (a pattern that upgrades a Green read to Amber). **Refused keys:** `allow`, `allow_commands`, `green`, `auto_approve`, `permission_mode`, `env`, anything with "allow" in it. A policy file can only make Delphi ask more or refuse more; the no-widening rule of D-21 is enforced by the schema, not by review. Policies are ratified like every other behavior file and are applied as deny hooks owned by the daemon, not as text any agent sees.

---

## 7. The Evals Section (`evals/delphi/`)
A bundle says how to tell whether it helps, in two files the existing tooling already reads:
- `kb-eval.json`: retrieval questions with the expected pointer, in the format of `dev/kb-eval.json`, run by `delphi kb eval --bundle <name>` (`specs/027` §6.1). At least 10 questions, written **before** the bundle is tuned on them, some worded differently from the documents, and at least 2 that the bundle does not answer (the off-topic control of `specs/004` §3.5).
- `tasks/*.md`: task cases for the bench runner (`evaluation-plan-2026-10-25.md` §7): a prompt, the repository it runs against (logical name), and a hidden rubric the arm never sees (`specs/027` §6.2).

Claude Code's own plugin evals (`claude plugin eval`) may live beside these under `evals/`; Delphi reads only `evals/delphi/` and does not judge the rest.

---

## 8. Versioning and Compatibility
- **`version` is semver with these meanings:** MAJOR changes when the bundle removes or renames a knowledge id, a skill, a goal or a perspective, or changes a decision's outcome (anything a team may have built on); MINOR adds; PATCH edits prose without changing meaning. `delphi bundle check --against <previous bundle>` computes the smallest honest bump from the two file manifests and the structured ids, and refuses a version lower than that.
- **`schema` / `compat.bundle_schema`** is the format major. A reader rejects a higher major as `unsupported-schema` (not as an attack) and keeps the last adopted version loaded. New optional fields roll out reader-first (`specs/012` §9).
- **`compat.delphi`** is a semver requirement on the Delphi version; adoption refuses outside it with the reason, and `delphi bundle check` on the publisher's machine warns when the requirement excludes the current release.
- **Content identity is the bundle hash** (§3.4), never the version string. Two bundles with the same version and different hashes are different bundles and `check --against` says so.
- **What is stable across versions:** `name`, `org`, knowledge `id`s, decision numbers, goal and perspective ids, skill names. Renaming any of them is a MAJOR bump and must be accompanied by a `supersedes`/`superseded_by` pair so the alignment check (`specs/026` §5) and `rule_stats` keep continuity.

---

## 9. Validation Order (`delphi bundle check <dir>`)
Deterministic, fail-closed, in this order, so that nothing parses before its container is known to be sane. Steps 1–3 run on raw bytes and paths only.

| # | Check | Fails when |
| :-: | :--- | :--- |
| 1 | Tree shape | a non-regular file, `.git` segment, absolute or `..` path, name outside the regex, more than 2,000 files, a file over 1 MiB, the tree over 64 MiB, a `bin/` directory |
| 2 | Bytes | a text file that is not UTF-8/NFC/LF, has control or bidi or tag characters; a binary file outside the allowed places or content types |
| 3 | Unknown top-level entries | logged as notes (`agents/`, `commands/`, `hooks/`, `output-styles/` are expected from a plugin and are **ignored** by Delphi, `specs/025` §2); never an error unless step 1 already failed |
| 4 | `bundle.toml` | missing; TOML parse error; unknown key; a rejected key (§3.1); `name`/`org`/`version` regex or semver; `audience` enum; `review_by` not a date in the future at pack time; `sections` naming a missing directory; `touches.network = true`; `touches.roles` outside the canonical ids |
| 5 | `.delphi-bundle/files.json` | present but any entry's hash or size differs from the file, a file missing from it, an extra entry, keys unsorted (a directory under development may omit the file; `pack` writes it) |
| 6 | Signature | `sig.json` present but does not verify over `files.json` under the domain of `specs/024` §2, or `publisher.did` differs from the signing key (an unsigned bundle passes this step with the note `unsigned`) |
| 7 | Frontmatter | every `knowledge/**.md` and `behavior/**.md` passes `delphi check provenance` plus the §4.2 fields; `id` unique; `summary` at most 160 chars; `pointers` valid under `specs/012` §4 with `file:` and local paths refused; `knowledge/index.md` complete and dangling-free |
| 8 | Secrets and identifiers | the `[REV-SEC-05]` path and secret scan over every text file; canary classes for API keys, tokens, private keys, local user paths, e-mail addresses other than the publisher's `contact`, and the publisher's own `[privacy] identifiers` list when `check` runs inside a workspace (`specs/004` §5.4); any hit is an error |
| 9 | Skills | each `skills/<name>/SKILL.md` passes `delphi skills check` (`specs/019`); `required_permissions` fit the built-in scope of each `authorized_agents` role; `touches.roles` covers every role named by a skill or guidance file |
| 10 | Behavior caps | goals at most 12 active and 600 chars; perspectives at most 12 and 350 chars; one guidance file per role, at most 2 KiB, no manifest keys, no-relaxation **error**; heuristics at most 10 and `check rules` passes; `policies.toml` only the accepted keys |
| 11 | No-relaxation warnings | instruction-shaped lines in `knowledge/**` reported as warnings with path and line |
| 12 | Compatibility | with `--claude`: `.claude-plugin/plugin.json` absent (offer to generate) or disagreeing with `bundle.toml` on `name`, `version`, `description`, `license` (`specs/025` §2); `skills/` entries valid for the standard |
| 13 | Evals | `evals/delphi/kb-eval.json` parses in the `dev/kb-eval.json` format with at least 10 questions and 2 controls when `sections.evals = true` |

A bundle that fails any step is **not adoptable**; `delphi bundle adopt` runs the same checks again on the install (it never trusts a publisher's "it passed").

---

## 10. Worked Example
A complete small bundle for a fictional organization. Every file shown passes §9.

```text
acme-platform/
├── bundle.toml
├── README.md
├── LICENSE
├── .delphi-bundle/files.json
├── .delphi-bundle/sig.json
├── .claude-plugin/plugin.json
├── knowledge/
│   ├── index.md
│   ├── glossary.md
│   ├── architecture/overview.md
│   ├── architecture/service-mesh.md
│   ├── runbooks/rolling-deploy.md
│   ├── runbooks/incident-first-30-minutes.md
│   ├── decisions/0031-latency-budget.md
│   └── decisions/0042-postgres-over-dynamo.md
├── skills/
│   ├── incident-review/SKILL.md
│   ├── incident-review/templates/postmortem.md
│   └── migration-plan/SKILL.md
├── behavior/
│   ├── goals.md
│   ├── perspectives.md
│   ├── roles/architect.md
│   ├── roles/builder.md
│   ├── heuristics.md
│   └── policies.toml
└── evals/delphi/
    ├── kb-eval.json
    └── tasks/add-retry-to-billing-client.md
```

`bundle.toml` is the one in §3 with `touches.connectors = []` and no `overlays`. The generated `.claude-plugin/plugin.json` (`specs/025` §2):
```json
{
  "name": "acme-platform",
  "version": "1.4.0",
  "description": "How Acme builds and runs its platform: architecture, decisions, runbooks, review habits, this year's goals.",
  "author": { "name": "Acme Platform Engineering", "email": "platform-eng@acme.example" },
  "license": "Proprietary",
  "keywords": ["acme", "platform", "kubernetes", "postgres"],
  "metadata": { "delphi.bundle": "1", "delphi.bundle_hash": "blake3:…", "delphi.org": "acme" }
}
```
Claude Code loads `skills/incident-review` and `skills/migration-plan` from it and ignores everything else; Delphi loads everything and ignores nothing it refuses.

`skills/incident-review/SKILL.md` (frontmatter only):
```yaml
---
name: incident-review
description: "Run Acme's blameless incident review: timeline, contributing factors, three actions with owners. Use after any Sev-1 or Sev-2."
license: Proprietary
metadata:
  delphi.authorized_agents: '["strategist","architect"]'
  delphi.input_schema: '{"type":"object","required":["incident_id"],"properties":{"incident_id":{"type":"string"}}}'
  delphi.output_schema: '{"type":"object","required":["postmortem_path"],"properties":{"postmortem_path":{"type":"string"}}}'
  delphi.required_permissions: '["read:all","write:missions"]'
---
```

`behavior/roles/architect.md` body (within 2 KiB):
```markdown
When drafting a spec for an Acme platform service: name the goal it serves by id from goals.md; include a rollback section
(perspective P-acme-05); cite the relevant decision records by number instead of restating them; and list which of the
three clusters the change touches. Prefer the service-mesh conventions in knowledge/architecture/service-mesh.md over
new networking.
```

`behavior/heuristics.md` (one of the rules):
```markdown
## [T-acme-02] Migrations ship with a rollback
- **Trigger:** a change adds or alters a database migration.
- **Directive:** the same change includes the reverse migration and a note on data loss, or states why none is possible.
- **Falsification:** a merged migration with no reverse and no note.
```

`evals/delphi/kb-eval.json` (excerpt, the `dev/kb-eval.json` shape):
```json
[
  { "q": "what do we do in the first half hour of an incident", "expect": "knowledge/runbooks/incident-first-30-minutes.md" },
  { "q": "why not DynamoDB for billing", "expect": "knowledge/decisions/0042-postgres-over-dynamo.md" },
  { "q": "how do we rotate the on-call pager vendor", "expect": null }
]
```

---

## 11. Acceptance Criteria
1. **AC-1:** a directory laid out as §2 with a valid §3 manifest passes `delphi bundle check` with zero failures; removing `knowledge/index.md`, adding a symlink, adding `bin/`, or adding an unknown manifest key each produces exactly one named failure.
2. **AC-2:** `files.json` written by `pack` covers every file once; changing one byte anywhere fails step 5 with the path named; the bundle hash changes.
3. **AC-3:** every `knowledge/**.md` and `behavior/**.md` in the §10 example passes `delphi check provenance` unchanged, and the `summary`, `domain`, `review_by` and `pointers` fields are accepted.
4. **AC-4:** a `SKILL.md` with `allowed-tools` passes with a note; one with `required_permissions` outside its roles' built-in scope fails step 9; one with a 300-character description fails with a request to shorten.
5. **AC-5:** `goals.md` with 13 active goals, `perspectives.md` with a 351-character entry, a `roles/architect.md` containing `default_access_scope`, a `roles/architect.md` saying "skip approval", `heuristics.md` with 11 rules, and `policies.toml` with an `allow_commands` table each fail step 10 with the cap or key named.
6. **AC-6:** a planted secret, a local user path and an e-mail address outside `publisher.contact` each fail step 8 (one canary per `[REV-SEC-05]` class).
7. **AC-7:** `check --against <v1.3.0 tree>` of a bundle that removed a knowledge id refuses `1.4.0` and names `2.0.0` as the smallest honest version.
8. **AC-8:** `check --claude` on the example either generates `.claude-plugin/plugin.json` or verifies that the existing one agrees with `bundle.toml` on `name`, `version`, `description` and `license`; the generated plugin passes `claude plugin validate` when that CLI is present (manual step).
9. **AC-9:** a bundle with `schema = 2` is refused as `unsupported-schema` with no other check run.

## 12. Verification Plan
Unit tests in the (future) `delphi-bundle` crate, one per acceptance criterion, each named `ac_N_…`: tree-shape fixtures (symlink, `bin/`, deep `.git`, 2,001 files, a 1 MiB + 1 byte file); byte fixtures (CRLF, a bidi override, a tag character, a PNG under `knowledge/` and under `behavior/`); manifest fixtures (every rejected key, every enum, `review_by` in the past); `files.json` round trip and single-byte mutation; frontmatter fixtures through the existing `checks::check_text("provenance")`; skill fixtures through the existing `skills_cmd` checks; behavior cap fixtures at the boundary (12/13 goals, 350/351 characters, 10/11 rules, 2 KiB ± 1); canary scan with one planted value per class; the §10 example as a golden fixture under `dev/bundles/acme-platform/` that CI checks on every push (`cargo run -- bundle check dev/bundles/acme-platform`). The `claude plugin validate` step is manual and recorded in the PR that adds the fixture.

## 13. Open Questions
Collected with the rest of the set in `specs/022` §7 (B-1 layout as a plugin superset, B-2 adoption tier, B-3 guidance block, B-7 scripts, B-11 the 160-character `summary` amendment to `specs/001`).
