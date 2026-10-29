---
id: spec-023-org-bundle-standard
title: "023: The Org Bundle Standard: Manifest, Layout, Knowledge, Skills, Agents, Behavior & Vectors"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges. Ratified in full by the operator 2026-10-28: every proposed decision and open question of the set (B-1..B-17, D-P1..D-P14) follows its recommendation
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the format is reasoned against the external formats cited in specs/025; nothing is built
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, knowledge, skills, agents, schema, post-v1]
---

# Spec 023: The Org Bundle Standard

> **Status: approved by the operator 2026-10-25 (decisions), text approved on merge** (`specs/022` §4.1). Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Nothing in this document is built; §14 is the build plan.

## 1. Scope & Objectives
An **org bundle** is a directory an organization publishes so that every Delphi install in that organization starts from the same knowledge, the same working skills and agents, and the same stated goals and perspective. The operator's intent (2026-10-25): Delphi accelerates humans working in a specific collaborative environment, and the critical chore is a **standard** by which an organization's knowledge and skills are bundled, defined as a plugin, and worked against by a team.

This spec defines:
1. The **taxonomy** (§1.4): a bundle is the package; skills, agents, knowledge and the rest are component types inside it.
2. The **bundle manifest** `bundle.toml` (§3) and the **directory layout** (§2).
3. The component sections: **knowledge** (§4), **skills** (§5A), **agents** (§5B), **behavior** (§6), **evals** (§7) and **vectors** (§8), with file formats, limits and deterministic checks.
4. **Versioning and compatibility** (§9) and the **validation order** (§10).
5. A **worked example** (§11).

What happens when a bundle meets an install (adding, provenance, tiers, precedence, update, rollback) is [`specs/024`](024-bundle-trust-adoption-and-precedence.md). The mapping onto Claude Code plugins and agents, Agent Skills, MCP and `AGENTS.md` is [`specs/025`](025-plugin-skill-and-tool-compatibility.md). How bundle content reaches the strategist and the console is [`specs/026`](026-team-alignment-and-strategist-integration.md). Authoring, transports, the vector pipeline and evaluation are [`specs/027`](027-bundle-authoring-distribution-and-evaluation.md).

### 1.1 Non-Goals
- Runtime behavior: `specs/024`.
- Anything a bundle cannot carry by design: executable hooks, credentials, permission grants, `allow` rules, operator-scope content (§10 lists the refusals; the reasons are `specs/024` §6).
- A query language or taxonomy for knowledge: a bundle ships Markdown with the provenance frontmatter Delphi already lints (`specs/001` §5); retrieval is the existing hybrid index (`specs/004` §3).
- Replacing `specs/011`/`012`: a bundle travels through xenia as one `bundle` record pointing at a pinned tree (`specs/027` §4).

### 1.2 Simpler Alternative Considered
- **Alternative 1: a shared `team/` git repository, cloned into every workspace, and nothing else.** This is `specs/007` Plane 3 and it already covers "the team edits the same files". Rejected as the standard because (a) it has no manifest, so nothing says what the repository is for, who publishes it, what version it is, or which roles it touches; (b) `team/` is one zone with one default tier, so a bundle from a vendor, a bundle from a sister team and the team's own notes become indistinguishable; (c) it cannot be handed to a Claude Code user, a vendor or an air-gapped machine as one artifact; (d) it has no place for goals and perspective that the strategist can be *told* about rather than left to find.
- **Alternative 2: a plain Claude Code plugin (`skills/`, `agents/`, `.mcp.json`) with knowledge stuffed into skills.** Closest to a real standard, and this spec keeps its layout as the compatibility target (`specs/025`). Rejected as the whole answer because a plugin has no notion of provenance, review dates, pointers to secure sources, goals, perspectives, tighten-only policy or shipped vectors, and because its `hooks/` and `bin/` carry executable authority that Delphi must refuse. A bundle is therefore a **superset of a plugin**: every bundle is a valid plugin directory for the parts a plugin understands, and Delphi reads the rest.

### 1.3 Design Principles
1. **Knowledge is data; behavior is instructions.** Everything under `knowledge/` is retrieved, bounded and spotlighted and is never always-loaded. Everything under `skills/`, `agents/` and `behavior/` is instruction text an agent will follow; it loads when the operator **adds** the bundle, which is the explicit act that permits it (operator decision OD-1, `specs/024` §1.3). The two never mix in one file.
2. **A bundle can request but never widen.** No field in a bundle grants a permission, allows a command, pre-approves a tool, adds a hook or asks for a credential. A skill or agent may *name* tools and permissions; the host role's built-in scope caps them (`specs/001` §2.3, D-21). Where a compatible format has a granting field (`allowed-tools`, `hooks`, `userConfig`), Delphi ignores it with a note or refuses the bundle (§10).
3. **Mostly retrieval, rarely always-loaded.** A bundle may carry at most ten rules (§6.4) and one bounded guidance block per role (§6.3). The Rule of 25 (GOVERNANCE §6) is not relaxed for bundles; the bulk of an organization's knowledge belongs in `knowledge/`, where there is no cap on size, only on what one span sees at once.
4. **Compatible by construction, not by conversion.** `skills/` is the Agent Skills format; `agents/` is the Claude Code agent file format; the layout is the Claude Code plugin layout; pointers are the xenia pointer format; vectors are the `DLPHVEC1` cache format the release already ships (`specs/004` §3.6).
5. **Everything is checkable without a model.** `delphi bundle validate` (§10) is deterministic. A bundle that passes it is well-formed; whether it is *true* or *useful* is for stewards (`specs/011` §7.3) and for evaluation (`specs/027` §6).
6. **One identity across transports.** The bundle's identity is the BLAKE3 of its file manifest (§3.4). A directory, an archive and a xenia record that share that hash are the same bundle.

### 1.4 Taxonomy
**Bundle** (the package) ⊃ **component types**: knowledge documents · skills (Agent Skills) · agents (Claude Code agent files, mapped to a Delphi host role) · role guidance (Delphi overlays) · goals and perspectives · rules · policy · evals · vectors. A skill is not a bundle; a one-skill plugin imported into Delphi becomes a bundle with one component (`specs/025` §7.1). The table in `specs/022` §3 gives each type's format, how it loads and what authority it has.

---

## 2. Directory Layout
```text
<bundle-root>/                         # the plugin root in Claude Code terms (specs/025 §2)
├── bundle.toml                        # REQUIRED. The Delphi manifest (§3). Human-edited; the source of truth
├── README.md                          # REQUIRED. What the operator agrees to when adding (specs/024 §4.2); never loaded into a prompt
├── LICENSE                            # RECOMMENDED when distributed beyond the org
├── CHANGELOG.md                       # RECOMMENDED; the top entry is shown as the update notice (specs/024 §4.3)
├── .delphi-bundle/                    # machine-written by `delphi bundle package` / `sign`; never hand-edited
│   ├── files.json                     #   file manifest: path -> {blake3, bytes} for every file (§3.4)
│   ├── sig.json                       #   OPTIONAL detached Ed25519 signature over files.json (specs/024 §2)
│   └── eval.json                      #   OPTIONAL result of the advisory retrieval gate at package time (specs/027 §6.1)
├── .claude-plugin/
│   └── plugin.json                    # OPTIONAL. Generated or verified by `delphi bundle validate --claude` (specs/025 §2)
├── knowledge/                         # §4: data. Markdown with provenance frontmatter; indexed, retrieved, never always-loaded
│   ├── index.md                       #   REQUIRED when knowledge/ exists: the pointer index
│   ├── glossary.md                    #   RECOMMENDED
│   ├── decisions/                     #   RECOMMENDED: one decision record per file (§4.3)
│   └── <any>/**.md
├── sources/                           # §2.1: OPTIONAL full-text captures and images of redistributable sources; retrieved, never always-loaded
│   └── <source-id>/{source.md, meta.json, excerpts.json, assets/}
├── skills/                            # §5A: Agent Skills, one <name>/SKILL.md per skill, Delphi fields in `metadata`
│   └── <skill-name>/{SKILL.md, references/, templates/ | assets/}
├── agents/                            # §5B: Claude Code agent files, one <name>.md each, with a Delphi host-role mapping
├── behavior/                          # §6: Delphi instructions and stated intent; loaded on add
│   ├── goals.md                       #   §6.1
│   ├── perspectives.md                #   §6.2
│   ├── roles/<role>.md                #   §6.3 bounded org guidance for a canonical role
│   ├── heuristics.md                  #   §6.4 at most 10 rules [T-<org>-NN]
│   └── policies.toml                  #   §6.5 tighten-only
├── evals/                             # §7
│   └── delphi/{kb-eval.json, tasks/*.md, prereg/*.md}
├── vectors/                           # §8: OPTIONAL precomputed vectors, a verified cache (specs/027 §5)
│   ├── index.json                     #   which caches exist and their keys
│   └── <key>.dlphvec                  #   one DLPHVEC1 file per (model id, dimension, embedding format, chunker version)
└── <anything else>                    # ignored by Delphi with a note (§10 step 3); must still pass the name and size rules
```

**Rules that apply to the whole tree** (the size rule below has a separate class for `sources/`, §2.1) (checked in §10, step 1–2):
- Regular files only. No symbolic links, hard links, device nodes, FIFOs, sockets or submodule gitlinks. No `.git` segment at any depth. No absolute or `..` path. No two components that differ only by Unicode normalization or case. Names match `^[A-Za-z0-9._-]{1,64}$` per component.
- **Never executable.** Files are written to the cache without the execute bit. A bundle has no `bin/` (Claude Code puts `bin/` on the Bash `PATH`; Delphi refuses a bundle that has one).
- **Size.** At most 2,000 files outside `vectors/`, 1 MiB per file outside `vectors/`, 64 MiB for the tree outside `vectors/`; `vectors/` has its own caps (§8). Markdown bodies under `knowledge/` at most 256 KiB each (the existing `MAX_READ_BYTES`); `SKILL.md` under the 500-line cap of `specs/001` §3; agent files at most 8 KiB; behavior files under the caps in §6.
- **Text.** UTF-8, NFC-normalized, LF line endings normalized by the packager before hashing (as `specs/012` §3), no control characters except `\n` and `\t`, no bidi controls, no `U+E0000`–`U+E007F` tag characters. Binary files are allowed only under `vectors/`, `skills/*/templates/`, `skills/*/assets/` and `knowledge/**` with the content types `image/png`, `image/svg+xml`, `application/pdf`, `text/csv`; they are never put in front of a model except through the document readers of `specs/014`.

---

### 2.1 The `sources/` section (amendment, 2026-10-29, [`specs/055`](055-the-central-library-sources-claims-and-pages.md))
An organization can ship the **full text** of the documents its knowledge rests on, so every install can retrieve from them, validate against them and read them. This is data, like `knowledge/`; it is never instructions.

- **Layout.** `sources/<source-id>/` holds `source.md` (the capture as Markdown; absent for a pointer-only source), `meta.json` (CSL-JSON citation fields plus Delphi's: `licence`, `rights_holder`, `attribution_text`, `share_alike`, `capture_level`, `retrieved`, `canonical_url`, `content_hash`, `version`; field list in `specs/055` §3.2.1), `excerpts.json` (optional) and `assets/` (images and attachments). `<source-id>` matches `^[a-z0-9][a-z0-9-]{0,63}$`.
- **Licence is information, not a gate (revised 2026-10-29).** A bundle is its organisation's own trusted material (`specs/024` §3.7), and most are internal: process, corporate facts, proprietary knowledge. So **no source is refused for its licence**, and `meta.json` needs only `type` and `title`. `licence` is optional. Its values are an SPDX identifier, `public-domain:<jurisdiction>:<basis>`, `own`, `internal` (the default when absent), `all-rights-reserved` or `unknown`. The licence record is kept and shown; it is bibliographic fact, not permission.
- **Warnings, with codes.** `bundle validate` reports, per source and never with source text: `source-licence` when the bundle's `distribution` is `public` and the source's class is not redistributable (`all-rights-reserved`, `unknown`, `internal`, or absent); `source-attribution` when the licence needs attribution and `attribution_text` is empty (either distribution); `source-sharealike` when a share-alike licence lacks `share_alike = true` or its notice. Warnings never fail `validate` and never block `add`.
- **Publishing outside the org is the operator's judgement (a warning state, not a gate).** The `distribution` key (`internal`, the default, or `public`; §3) says where the author intends the bundle to go. When `bundle package` runs for a `public` bundle, or `bundle publish`, a Xenia store upload or a mirror runs for **any** bundle, and there are `source-licence` warnings **or** the bundle is `internal`, the command prints the affected source ids with their class (and, for an internal bundle, "this bundle is marked internal"), then asks the operator to confirm. The confirmation is the `sign` precedent: a terminal on stdin and stdout, no `--yes`, no environment variable, refused for a process an agent started. The acknowledged ids and the time are written to `.delphi-bundle/report.json` as `licence_acknowledged`. After that, the command proceeds: the content is never refused.
- **The one hard rule: `private`.** A source the operator flagged `private` in the library (`specs/055` §10.1) is not copied into a bundle (`bundle sources add` refuses it with the reason), and a `meta.json` carrying `"private": true` fails `validate`, `package` and `add` with `source-private`. The flag is the operator's own earlier decision, so it is the one thing a later confirmation does not override; unflagging it in the library is the way, and is a deliberate act.
- **Size class (separate from the rule above).** `sources/` has its own caps: at most **20,000 files**, **16 MiB per file** for text and **32 MiB per file** for an asset, and **1 GiB** for the section, all outside the 2,000-file and 64 MiB tree limits. The Markdown body cap of §4 does not apply to `source.md`; the index chunks it by heading and every read is by range.
- **Bytes.** `source.md` and `meta.json` follow the text rules (UTF-8, NFC, LF, no control, bidi or tag characters). Assets may be `image/png`, `image/jpeg`, `image/webp`, `image/svg+xml`, `application/pdf` and `text/csv`; they are never executed, rendered by a model, or served inline without the safe-reader rules of `specs/029`.
- **Frontmatter and index.** `source.md` has no provenance frontmatter (the record is `meta.json`); `knowledge/**` documents may list `sources: [<source-id>…]` in their frontmatter (§4.2), and each id must exist.
- **Trust: org-vouched.** Knowledge and sources in an added bundle are **validated by the operator's act of adding the bundle from its publisher** (`specs/024` §3.7), and are shown `● validated · vouched by <org>` (`specs/052` §3.1), with the bundle's provenance level ([`specs/024`](024-bundle-trust-adoption-and-precedence.md) §2.2). Vouching is for **reliability and citability only**: the contents stay data (the stored tier stays $L_2$; they cannot instruct, raise any other file, or relax any guard). A source's own `meta.json` or a document's frontmatter cannot grant, raise or claim the label; only the lock entry of an added bundle does (`specs/024` §3.7.1). The label ends when the bundle is removed, its hash or publisher key changes without a fresh `add`, or on rollback (§3.7.2).
- **No instructions.** The no-relaxation check of step 11 runs over `source.md` as a **warning**; sources legitimately quote instruction-shaped text.

## 3. The Manifest: `bundle.toml`
TOML, because every other configuration in Delphi is TOML and the no-widening linter already reads it. Unknown keys are **errors**, not warnings: an ignored key is a smuggling channel (`specs/012` §5), and a typo in a manifest must not pass silently.

```toml
[bundle]
schema = 1                                  # bundle schema major; a reader rejects a higher major it does not know (§9)
name = "acme-platform"                      # ^[a-z0-9][a-z0-9-]{0,47}$ ; equals plugin.json `name` when that file exists
org = "acme"                                # ^[a-z0-9-]{1,32}$ ; equals the xenia `org` when published through a store
version = "1.4.0"                           # semver; §9 says what each part means
title = "Acme Platform Engineering"         # <= 80 chars, plain text
description = "How Acme builds and runs its platform: architecture, decisions, runbooks, review habits, this year's goals."
                                            # <= 200 chars, plain text, no Markdown links
license = "Proprietary"                     # SPDX identifier, or a short phrase naming the bundled LICENSE file
homepage = "https://wiki.acme.example/platform-bundle"   # OPTIONAL; https only
audience = "org"                            # org | team | project | person: the INTENDED scope. Routing, not secrecy (specs/011 §5.4)
distribution = "internal"                   # internal (default) | public: where the author intends the bundle to go (§2.1: a warning state, never a gate)
review_by = "2027-01-31"                    # REQUIRED; after it every surface shows the bundle as stale (nothing is unloaded)
language = "en"                             # BCP 47

[compat]
delphi = ">=0.3.0"                          # semver requirement; `add` refuses outside it
bundle_schema = 1                           # must equal bundle.schema

[publisher]
name = "Acme Platform Engineering"          # <= 80 chars
contact = "platform-eng@acme.example"       # OPTIONAL; display only
did = "did:key:z6Mk..."                     # OPTIONAL; REQUIRED to sign (specs/024 §2) and to publish through xenia
predecessors = []                           # OPTIONAL: earlier publisher DIDs this one succeeds (specs/024 §2.3)

[applies_to]                                # retrieval and routing hints; the same shape as a xenia `memory` record (specs/012 §5.1)
repos = ["acme/platform-*", "acme/billing"]
tech = ["rust", "postgres", "kubernetes"]
paths = ["services/**", "infra/**"]

[sections]                                  # a listed directory must exist; an unlisted one is ignored with a note
knowledge = true
skills = true
agents = true
behavior = true
evals = true
vectors = true
sources = true                             # 023 §2.1 (amended 2026-10-29): full-text captures for retrieval

[agents.reviewer]                           # one table per agents/<name>.md (§5B); REQUIRED for an agent to be dispatchable in Delphi
role = "red-team"                           # the HOST role: a canonical id (D-17). The agent runs as a span of this role and is capped by it

[touches]                                   # declared so the add summary can show it; checked against the tree (§10 step 9)
roles = ["strategist", "architect", "builder", "red-team"]   # roles that skills/, agents/ or behavior/roles/ name
connectors = ["slack"]                      # MCP servers the bundle declares in .mcp.json (specs/025 §4); inert until mapped
network = false                             # a bundle never needs network at add; `true` is refused in schema 1

[overlays]
depends_on = ["acme-security@>=2.0.0"]      # OPTIONAL: bundles that must be added first (specs/024 §4)
supersedes = ["acme-platform-legacy"]       # OPTIONAL: bundles this one replaces; `add` offers to remove them
```

### 3.1 Field rules that carry provenance
- `name`, `org` and `version` are bound into the signature domain (`specs/024` §2), so a signed bundle renamed, re-versioned or moved to another org fails verification.
- `publisher.did` must equal the signing key's DID when `.delphi-bundle/sig.json` exists.
- **No field states a trust tier.** `tier`, `trust`, `source_type`, `endorsed`, `verified`, `approved` and `ratified` are rejected anywhere in `bundle.toml` (as in `specs/012` §2.1). Trust is computed by the receiving install (`specs/024` §3).
- **No field grants.** There is no `permissions`, `allow`, `allowed_tools`, `hooks`, `env`, `secrets`, `user_config` or `mcp_servers` key. MCP servers a bundle declares live in `.mcp.json` in the Claude Code shape and stay inert until the operator maps them (`specs/025` §4.1).

### 3.2 `audience` is routing, not secrecy
A bundle is one artifact; everyone who can open it can read all of it (`specs/011` §5.4). `audience` tells an install *where to place* the bundle (`specs/024` §5) and the strategist how general the goals are. Sensitive detail belongs behind a pointer (§4.4) in the system that already controls access to it.

### 3.3 `applies_to` keeps retrieval relevant
A workspace whose `projects` match none of `applies_to.repos` still indexes the bundle, but the org-context slice (`specs/026` §3) marks it "added, not matched", and its search results rank below matched bundles by a fixed fusion penalty rather than being hidden.

### 3.4 The file manifest `.delphi-bundle/files.json`
```json
{
  "bundle": 1,
  "name": "acme-platform", "org": "acme", "version": "1.4.0",
  "files": {
    "bundle.toml":                      { "blake3": "blake3:<64 hex>", "bytes": 1831 },
    "knowledge/index.md":               { "blake3": "blake3:...",      "bytes": 4120 },
    "vectors/bge-base-en-v1.5-int8-768-w1-c3.dlphvec": { "blake3": "blake3:...", "bytes": 2621440 }
  }
}
```
- Every regular file except `.delphi-bundle/files.json`, `sig.json` and `eval.json` appears exactly once, keys sorted bytewise, no other keys, strict JSON, at most 4 MiB.
- The **bundle hash** is `blake3(files.json raw bytes)`: the identity in the lock, the cache directory name and the xenia record's pin. `validate` recomputes every entry; one mismatch fails the bundle.
- The spelling `blake3:` plus 64 lowercase hex follows the xenia X1 choice.

---

## 4. The Knowledge Section (`knowledge/`)
### 4.1 What goes here
Anything the organization knows that an agent should be able to *find*: architecture, deployment, ownership by role or team (never personal data beyond a work handle), vocabulary, decisions and why, gotchas, runbooks, postmortem summaries. Prose, tables, Mermaid. Not transcripts, logs, secrets or customer data (§10 step 8 scans for them).

### 4.2 File format
Markdown with the **provenance frontmatter of `specs/001` §5**, so `delphi lint` and `delphi check provenance` apply unchanged, plus optional fields (`summary`, `domain`, `review_by`, `pointers`, and from 2026-10-29 `sources` and `licence`):
```yaml
---
id: acme-platform/architecture/overview        # REQUIRED; unique within the bundle; `<bundle name>/<path without .md>` by convention
title: "Platform architecture overview"
author: operator                                # a canonical role, or `operator` for human-written (D-17)
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated                 # a DISPLAY COPY (specs/001 §5); the install stamps the real tier at add (specs/024 §3)
confidence: medium
status: active                                  # active | superseded; superseded needs `superseded_by`
source_mission: acme-platform                    # the bundle name, never a local mission id
tags: [architecture, kubernetes]
summary: "Three clusters, one control plane; services talk over mTLS; billing is the only stateful exception."   # <= 160 chars (specs/013 §5)
domain: platform                                # OPTIONAL; from knowledge/glossary.md
review_by: "2027-03-01"                         # OPTIONAL per file; defaults to the bundle's
sources: [oecd-2024-report]                      # OPTIONAL: ids under sources/ that this file cites (§2.1)
licence: "CC-BY-4.0"                            # OPTIONAL: for a file reproducing or adapting a source; the bundle licence applies otherwise
pointers:                                       # OPTIONAL, <= 8, the xenia pointer format (specs/012 §4); display-only (§4.4)
  - { scheme: https, locator: "https://wiki.acme.example/platform/arch", pin: { kind: none }, label: "Full architecture wiki", content_type: text/markdown }
---
Body. Data, never instructions. Headings every few paragraphs: the index chunks by heading (specs/004 §3.2).
```
- `summary` is what `knowledge/index.md` and the org-context slice show.
- A knowledge file **should not** contain instruction-shaped text aimed at an agent outside a fenced quotation. The §10 linter uses the deterministic no-relaxation check of `specs/001` §6.4 as a **warning** with the line shown, because prose about deployment legitimately says "never deploy on Fridays". Instructions belong in `behavior/`.

### 4.3 Decision records (`knowledge/decisions/`)
One file per decision with the headings `## Decision`, `## Why`, `## Consequences`, optional `## Superseded by`; the `id` carries a stable number (`acme-platform/decisions/0042-postgres-over-dynamo`). A changed decision is a new file that `supersedes` the old one; the old stays with `status: superseded`. The alignment check (`specs/026` §5) reads these by id.

### 4.4 Pointers are display-only
Shown as text; never fetched when adding or reading (`specs/011` §6 unchanged). `file:` and local paths are refused.

### 4.5 `knowledge/index.md`
Required when `knowledge/` exists. One list item per file, in the order the author wants a reader to meet them: a Markdown link whose text is the file's `title` and whose target is the path relative to `knowledge/`, then a colon and the file's `summary`. `validate` fails on a knowledge file missing from the index or an index line pointing at nothing.

---

## 5A. The Skills Section (`skills/`)
Exactly the Agent Skills format (`agentskills.io/specification`, read 2026-10-25) with Delphi's fields where `specs/019` already puts them:
- `skills/<name>/SKILL.md` with `name` (equal to the directory, `^[a-z0-9]+(-[a-z0-9]+)*$`, ≤ 64 characters), `description` (Delphi caps at 200 characters; longer is refused with a request to shorten, never cut), optional `license`, `compatibility`, `metadata`, `allowed-tools`.
- Delphi's `authorized_agents`, `input_schema`, `output_schema`, `required_permissions` as JSON strings under `metadata.delphi.*` (what `delphi skills export` writes, `specs/019` §3). The native top-level form is also accepted; `package` normalizes to the `metadata` form so the bundle stays a valid standard skill.
- **`allowed-tools` is ignored with a note.** It pre-approves another tool's tools; Delphi has no pre-approval a file can grant.
- **`scripts/` is never executed** by Delphi (OD-1 schema limit). The directory may exist for Claude Code users; `validate` lists its files; `add` copies them without the execute bit and never declares them in `deterministic_scripts`.
- `required_permissions` must be satisfiable within the **built-in** `default_access_scope` of every role in `authorized_agents` (`specs/001` §2.3, 001-A1). A skill that needs more fails `validate`.
- `authorized_agents` names the roles the skill loads for on add (`specs/024` §3). The operator may narrow the list at add (`--skill-roles <skill>=<roles>`); a bundle can never name a role that does not exist.

## 5B. The Agents Section (`agents/`)
Bundles may include agent definitions (operator decision OD-4). The file format is the **Claude Code agent file**: `agents/<name>.md` with YAML frontmatter (`name`, `description`, optional `tools`, `model`, and other keys Claude Code defines) and a Markdown body that is the agent's instructions (`specs/025` §1.3 sources). Delphi reads it as follows:
- **Host role.** An agent is dispatchable in Delphi only when `bundle.toml` has `[agents.<name>] role = "<canonical id>"` (or the frontmatter carries `delphi-role: <canonical id>`; the manifest wins on disagreement and `validate` says so). The agent runs as a **span of the host role**: the host's manifest, scope, memory isolation, taint rules and HITL tiers apply unchanged, and the agent's body is placed as a bounded block (≤ 8 KiB) after the host's manifest and guidance. An agent without a host role is listed as "Claude Code only" and is not dispatchable (`specs/022` B-17).
- **Capped authority.** The `tools` list is **intersected** with the tools the host role may use; a tool the host lacks is dropped with a note at add. Nothing in an agent file can add a tool, a write glob, an execute capability or a network permission (D-21). `model` is advisory: Delphi routes by tier (`specs/002`), so `model: opus` maps to `frontier`, `sonnet`/`haiku` to `workhorse`/`fast`, and anything else is ignored with a note.
- **Dispatch.** `dispatch_agent` accepts the target `bundle:<bundle>/<agent>`; the console accepts `@<bundle>:<agent>` (the Claude Code namespacing `plugin:agent`). The child span is logged under the host role with `bundle_agent = "<bundle>/<agent>"`, so events, costs and `rule_stats` attribute correctly.
- **Linting.** The body goes through the no-relaxation check as an **error** (as role guidance, §6.3): an agent that says "skip approval for small diffs" is refused. Frontmatter keys Delphi does not use (`permissionMode`, `hooks`, anything executable) are ignored with a note; a `hooks` key is a **warning** that the agent behaves differently in Claude Code.
- **Delphi role overlays are not agents.** Guidance for a canonical role goes in `behavior/roles/<role>.md` (§6.3). The two are deliberately separate so a Claude Code loader never reads a Delphi overlay as a subagent.

---

## 6. The Behavior Section (`behavior/`)
Instruction text and stated intent. It **loads when the bundle is added** (OD-1): the operator's explicit `add`, having read `README.md`, is the approval, as `delphi update` is for a release (006-A6). Each file has the provenance frontmatter of §4.2 and the caps below; the caps are schema, not ratification.

### 6.1 Goals (`behavior/goals.md`)
```markdown
## [G-acme-01] Cut p99 checkout latency below 400 ms
- **Horizon:** 2026-Q4 to 2027-Q1
- **Owner role:** architect              <- a canonical role or a team handle, never a person's name
- **Measure:** p99 of `checkout.complete` in the production dashboard, weekly
- **Status:** active                      <- active | met | dropped | superseded
- **Why:** conversion drops 3% per 100 ms over 400 (see knowledge/decisions/0031-latency-budget.md)
```
At most **12 active goals**, each at most **600 characters**, ids `[G-<org>-NN]`. Goals are information for the planning roles (`specs/026` §3); a goal can never authorize an action, waive a gate or change a tier. The no-relaxation linter runs on the text.

### 6.2 Perspectives (`behavior/perspectives.md`)
```markdown
## [P-acme-03] Boring technology by default
- **Statement:** choose the well-understood option unless it is measurably the bottleneck.
- **Apply when:** picking a datastore, queue, framework or language for a new service.
- **Counter-case:** the boring option cannot meet a stated goal's measure (cite the goal).
```
At most **12 perspectives**, each at most **350 characters** across the three lines (the rule-size cap of GOVERNANCE §6). The Council can assign one per seat (`specs/026` §4). The linter rejects any that mentions tools, approvals, taint, `REVIEW.md`, "ignore" or "override".

### 6.3 Role guidance (`behavior/roles/<role>.md`)
Bounded, instruction-only guidance for one canonical role. **Not a manifest; never merges with one.** On add it becomes an **org guidance block** of at most **2 KiB**, placed after governance and the role manifest and before memory (an IPR slot, `specs/022` §5). Rules: body only, frontmatter §4.2 plus `role: <canonical id>`; any manifest key (`default_access_scope`, `authorized_skills`, `execute`) fails `validate`; one file per role from the canonical ids (or `librarian` once built); the no-relaxation linter is an **error** here; the IPR governance block states that org guidance cannot override it (`specs/004` §5.2).

### 6.4 Rules (`behavior/heuristics.md`)
At most **10 rules** in the Trigger / Directive / Falsification form with ids `[T-<org>-NN]`, each under 350 characters (`delphi check rules` applies). On add they load as a **bundle rules block** inside the team scope's 3 KB budget (`specs/004` §5.2), after `context/team-heuristics.md`; they are never written into that file. **The Rule of 25 applies to the combined count** of team rules and bundle rules: when the sum exceeds 25 or the budget, bundle rules are trimmed oldest-added first, each trim logged and shown in `/org`, and the team's own file is never touched. The ids keep the org so `/retro` and `rule_stats` (`specs/004` §5.6) can tell where a rule came from.

### 6.5 Tighten-only policy (`behavior/policies.toml`)
```toml
[policy]
schema = 1

[[policy.deny_commands]]                 # a daemon deny rule (the shape of specs/018 deny hooks): can only refuse
pattern = "^terraform (apply|destroy)"
reason = "Infrastructure changes go through the platform pipeline, never from an agent."

[[policy.red_paths]]                     # a worktree write matching this glob is Red at merge, like GOVERNANCE §5.5's host-executed files
glob = "infra/prod/**"
reason = "Production infrastructure."

[[policy.connector_denied]]              # a connector tool never offered to a role while this bundle is added
tool = "slack.post_message"
roles = ["*"]
```
Accepted keys: `deny_commands`, `red_paths`, `connector_denied`, `require_card` (a regular expression over the path of a read that upgrades a Green read to Amber; enforced since `specs/024` §10.5). **Refused keys:** `allow`, `allow_commands`, `green`, `auto_approve`, `permission_mode`, `env`, anything containing "allow". A policy can only make Delphi ask more or refuse more; D-21 is enforced by the schema. Policies apply on add as daemon-owned deny rules, not as text any agent sees.

---

## 7. The Evals Section (`evals/delphi/`)
- `kb-eval.json`: retrieval questions with the expected pointer, in the format of `dev/kb-eval.json`, run by `delphi kb eval --bundle <name>` (`specs/027` §6.1). At least 10 questions written **before** tuning, some worded differently from the documents, at least 2 the bundle does not answer (the off-topic control of `specs/004` §3.5).
- `tasks/*.md`: task cases for the bench runner with a hidden rubric (`specs/027` §6.2).
- `prereg/*.md`: pre-registration records for confirmatory bench runs (`specs/027` §6.2); optional, usually written by the organization that runs the study, not the bundle publisher.

Claude Code's own plugin evals may live beside these under `evals/`; Delphi reads only `evals/delphi/`.

---

## 8. The Vectors Section (`vectors/`, optional)
A bundle **may** ship precomputed vectors so that a large corpus is searchable by meaning as soon as it is added, instead of after hours of local embedding (OD-8). The pipeline that verifies and uses them is `specs/027` §5; this section fixes the format.

- **Format: `DLPHVEC1`, unchanged** (`specs/004` §3.6): a cache from the hash of *exactly the text given to the model for a chunk* (every window, in order, each length-prefixed) to the vectors the model produced, with a header recording the model id, dimension and `embedding_format`. Delphi already builds this file for the release with `delphi kb seed-export`; `delphi bundle package --vectors` builds one for a bundle the same way. Nothing in it depends on paths or schema, so the same file serves every install with the same embedder.
- **Key.** One file per key `<model id>-<dimension>-<embedding format>-<chunker version>`, named in `vectors/index.json`:
  ```json
  { "vectors": 1, "caches": [ { "file": "bge-base-en-v1.5-int8-768-w1-c3.dlphvec", "model_id": "bge-base-en-v1.5-int8", "dim": 768,
      "embedding_format": "w1", "chunker_version": "c3", "chunks": 48213, "windows": 61190, "dtype": "f32" } ] }
  ```
  An adopter uses a cache **only when all four key parts equal its own** (`kb_meta.embedding_model_id`, `embedding_dim`, `embedding_format`, `chunker_version`); otherwise the file is ignored whole and the install embeds locally (`specs/027` §5.2). A publisher may ship several caches (for example Delphi's default embedder and one other).
- **Per-chunk content hash** is the cache key itself: a vector is found by hashing the bundle's own chunk text, so a vector for text that is not in the bundle is unreachable, and an unchanged chunk across bundle versions keeps its vector with no work (`specs/027` §5.4).
- **Quantization.** `dtype` is `f32` (the format today, about 3 KB per 768-d window) or `i8` (per-vector scale, about 0.8 KB per window; a `DLPHVEC1` minor revision that the reader must accept before any publisher writes it, reader-first as in `specs/012` §9). Recommendation: `i8` for corpora over 5,000 chunks; `kb eval` must show no recall loss on the bundle's own questions before a publisher ships `i8`.
- **Caps (schema 1).** `vectors/` at most **256 MiB** and **100,000 chunks** per cache; a cache over either is ignored with a note and the install embeds locally. These are outside the 64 MiB tree cap of §2, because vectors are incompressible and a corpus of 50,000 chunks is about 150 MiB at `f32` or 40 MiB at `i8`.
- **Trust.** Vectors are a cache for ranking and nothing else: they never carry text, never change a tier, and are verified per `specs/027` §5.3 (re-keyed from the bundle's text, spot-checked by local re-embedding, discarded whole on any failure) because unsigned bundles are allowed.
- **Which embedder.** The default recommendation (B-16): ship vectors when the publisher's embedder is Delphi's default model (`bge-base-en-v1.5` int8 package, `specs/004` §3.5), because that is what every default install matches; always allow local embedding; `add --no-vectors` ignores shipped caches.

---

## 9. Versioning and Compatibility
- **`version` is semver:** MAJOR when a knowledge id, skill, agent, goal, perspective or decision outcome is removed, renamed or reversed; MINOR adds; PATCH edits prose without changing meaning. `delphi bundle validate --against <previous>` computes the smallest honest bump from the two file manifests and the structured ids and refuses a lower version.
- **`schema` / `compat.bundle_schema`** is the format major; a reader rejects a higher major as `unsupported-schema` and keeps the last added version loaded. New optional fields roll out reader-first.
- **`compat.delphi`** is a semver requirement; `add` refuses outside it with the reason.
- **Content identity is the bundle hash**, never the version string.
- **Stable across versions:** `name`, `org`, knowledge `id`s, decision numbers, goal and perspective ids, skill and agent names. Renaming any is MAJOR with a `supersedes`/`superseded_by` pair.

---

## 10. Validation Order (`delphi bundle validate <dir>`)
Deterministic, fail-closed, in this order. Steps 1–3 run on raw bytes and paths only. The same checks run again on the install at `add`; nothing trusts a publisher's "it passed".

| # | Check | Fails when |
| :-: | :--- | :--- |
| 1 | Tree shape | (the limits below apply outside `sources/` and `vectors/`; `sources/` has the §2.1 caps) a non-regular file, `.git` segment, absolute or `..` path, name outside the regex, more than 2,000 files or 64 MiB outside `vectors/`, a file over 1 MiB outside `vectors/`, a `bin/` directory, no `README.md` |
| 2 | Bytes | a text file that is not UTF-8/NFC/LF or has control, bidi or tag characters; a binary file outside the allowed places or content types |
| 3 | Unknown top-level entries | notes only (`commands/`, `hooks/`, `output-styles/`, `workflows/`, `monitors/` are expected from a plugin and **ignored**, `specs/025` §2.3) |
| 4 | `bundle.toml` | missing; parse error; unknown or rejected key (§3.1); regexes and semver; `audience` and `distribution` enums; `review_by` in the past at package time; `sections` naming a missing directory; `touches.network = true`; a role outside the canonical ids; `[agents.<name>]` for a file that does not exist |
| 5 | `.delphi-bundle/files.json` | present but an entry's hash or size differs, a file missing, an extra entry, keys unsorted (a directory under development may omit it; `package` writes it) |
| 6 | Signature | `sig.json` present but fails to verify under `specs/024` §2.1, or `publisher.did` differs from the key (absent `sig.json` passes with the note `unsigned`) |
| 7a | Sources | `sources/` per §2.1: a `meta.json` that is missing or fails the CSL-JSON field checks (`type` and `title` required); `"private": true` (`source-private`, **fails**); a `knowledge` `sources:` id that does not exist; a size over the §2.1 caps. **Warnings only:** `source-licence`, `source-attribution`, `source-sharealike`, and an undeclared-licence note |
| 7 | Frontmatter | every `knowledge/**.md` and `behavior/**.md` passes `delphi check provenance` plus §4.2; `id` unique; `summary` ≤ 160; pointers valid under `specs/012` §4 with `file:` and local paths refused; `knowledge/index.md` complete and dangling-free |
| 8 | Secrets and identifiers | the `[REV-SEC-05]` path and secret scan over every text file; canaries for API keys, tokens, private keys, local user paths, e-mail addresses other than `publisher.contact`, roster handles and the workspace's `[privacy] identifiers` when run inside a workspace (`specs/004` §5.4); a hit is an error (handles and identifiers: a warning, §27 of `specs/027` §5) |
| 9 | Skills and agents | each `SKILL.md` passes `delphi skills check`; `required_permissions` fit each role's built-in scope; each `agents/<name>.md` parses, is ≤ 8 KiB, has a host role or is marked "Claude Code only", and passes the no-relaxation check as an error; `touches.roles` covers every role named anywhere |
| 10 | Behavior caps | goals ≤ 12 active and 600 chars; perspectives ≤ 12 and 350 chars; one guidance file per role ≤ 2 KiB with no manifest keys and no-relaxation **error**; heuristics ≤ 10 and `check rules` passes; `policies.toml` only the accepted keys |
| 11 | No-relaxation warnings | instruction-shaped lines in `knowledge/**`, with path and line |
| 12 | Vectors | `vectors/index.json` parses; each named file exists, has a valid `DLPHVEC1` header agreeing with the index entry, is under the caps; every key in a cache is the hash of some chunk of the bundle's own text (an unreachable key is a warning; more than 1% unreachable is an error, because the cache was built from other text) |
| 13 | Compatibility | with `--claude`: `.claude-plugin/plugin.json` absent (offer to generate) or disagreeing with `bundle.toml` on `name`, `version`, `description`, `license`; agents and skills valid for their standards |
| 14 | Evals | `evals/delphi/kb-eval.json` in the `dev/kb-eval.json` format with ≥ 10 questions and ≥ 2 controls when `sections.evals = true`; `prereg/*.md` have the required headings (`specs/027` §6.2) |
| 15 | Retrieval gate (advisory, `package` only) | `delphi kb eval --bundle` below the frozen thresholds (`specs/027` §6.1) is a **warning** recorded in `.delphi-bundle/eval.json`, never a failure |

---

## 11. Worked Example
```text
acme-platform/
├── bundle.toml  README.md  LICENSE  CHANGELOG.md
├── .delphi-bundle/{files.json, sig.json, eval.json}
├── .claude-plugin/plugin.json
├── knowledge/
│   ├── index.md  glossary.md
│   ├── architecture/{overview.md, service-mesh.md}
│   ├── runbooks/{rolling-deploy.md, incident-first-30-minutes.md}
│   └── decisions/{0031-latency-budget.md, 0042-postgres-over-dynamo.md}
├── skills/
│   ├── incident-review/{SKILL.md, templates/postmortem.md}
│   └── migration-plan/SKILL.md
├── agents/
│   └── reviewer.md
├── behavior/
│   ├── goals.md  perspectives.md  heuristics.md  policies.toml
│   └── roles/{architect.md, builder.md}
├── evals/delphi/{kb-eval.json, tasks/add-retry-to-billing-client.md}
└── vectors/{index.json, bge-base-en-v1.5-int8-768-w1-c3.dlphvec}
```

`bundle.toml` is §3 with `touches.connectors = []`. The generated `.claude-plugin/plugin.json` (`specs/025` §2):
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
Claude Code loads the two skills and the `reviewer` agent and ignores the rest; Delphi loads everything and refuses nothing here.

`agents/reviewer.md`:
```markdown
---
name: reviewer
description: Reviews a change against Acme's platform conventions and decision records before it is merged.
tools: Read, Grep, Glob
delphi-role: red-team
---
Review the change for: a cited goal id; the rollback section (P-acme-05); conflicts with decisions 0031 and 0042;
service-mesh conventions from knowledge/architecture/service-mesh.md. Report findings as P1/P2 with a line anchor.
```
In Delphi this runs as a `red-team` span with the red-team manifest's tools intersected with `Read, Grep, Glob` (all reads, all allowed) and this body as its block; in Claude Code it is the subagent `acme-platform:reviewer`.

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

`behavior/heuristics.md` (one rule):
```markdown
## [T-acme-02] Migrations ship with a rollback
- **Trigger:** a change adds or alters a database migration.
- **Directive:** the same change includes the reverse migration and a note on data loss, or states why none is possible.
- **Falsification:** a merged migration with no reverse and no note.
```

`evals/delphi/kb-eval.json` (excerpt):
```json
[
  { "q": "what do we do in the first half hour of an incident", "expect": "knowledge/runbooks/incident-first-30-minutes.md" },
  { "q": "why not DynamoDB for billing", "expect": "knowledge/decisions/0042-postgres-over-dynamo.md" },
  { "q": "how do we rotate the on-call pager vendor", "expect": null }
]
```

---

## 12. Acceptance Criteria
1. **AC-1:** a directory laid out as §2 with a valid §3 manifest passes `validate` with zero failures; removing `README.md` or `knowledge/index.md`, adding a symlink, adding `bin/`, or adding an unknown manifest key each produces exactly one named failure.
2. **AC-2:** `files.json` written by `package` covers every file once; changing one byte anywhere fails step 5 with the path named; the bundle hash changes.
3. **AC-3:** every `knowledge/**.md` and `behavior/**.md` in the §11 example passes `delphi check provenance` unchanged, with `summary`, `domain`, `review_by` and `pointers` accepted.
4. **AC-4:** a `SKILL.md` with `allowed-tools` passes with a note; one with `required_permissions` outside its roles' built-in scope fails step 9; a 300-character description fails with a request to shorten.
5. **AC-5:** `goals.md` with 13 active goals, a 351-character perspective, a `roles/architect.md` containing `default_access_scope`, a `roles/architect.md` saying "skip approval", `heuristics.md` with 11 rules, and `policies.toml` with an `allow_commands` table each fail step 10 with the cap or key named.
6. **AC-6:** a planted secret, a local user path and an e-mail address outside `publisher.contact` each fail step 8 (one canary per `[REV-SEC-05]` class); a roster handle is a warning.
7. **AC-7:** `validate --against <v1.3.0>` of a bundle that removed a knowledge id refuses `1.4.0` and names `2.0.0`.
8. **AC-8:** `validate --claude` generates `.claude-plugin/plugin.json` or verifies agreement on `name`, `version`, `description`, `license`; the generated plugin passes `claude plugin validate` when that CLI is present (manual).
9. **AC-9:** an `agents/reviewer.md` with a host role passes; one with `tools` the host lacks passes with a note naming the dropped tools; one with no host role passes marked "Claude Code only"; one whose body says "skip approval" fails; one over 8 KiB fails; a `[agents.x]` table with no `agents/x.md` fails step 4.
10. **AC-10:** a `vectors/index.json` naming a missing file, a file whose header disagrees with the index, a cache over 256 MiB or 100,000 chunks, and a cache with 2% unreachable keys each fail or warn as step 12 says; a cache whose keys all re-hash from the bundle's text passes.
11. **AC-11:** a bundle with `schema = 2` is refused as `unsupported-schema` with no other check run.

## 13. Verification Plan
Unit tests in the (future) `delphi-bundle` crate, one per criterion, named `ac_N_…`: tree-shape fixtures (symlink, `bin/`, deep `.git`, 2,001 files, a 1 MiB + 1 byte file, no README); byte fixtures (CRLF, a bidi override, a tag character, a PNG under `knowledge/` and under `behavior/`); manifest fixtures (every rejected key, every enum, `review_by` in the past, `[agents.x]` without a file); `files.json` round trip and single-byte mutation; frontmatter through the existing `checks::check_text("provenance")`; skill fixtures through the existing `skills_cmd` checks; agent fixtures at the 8 KiB boundary and with each tool-intersection case; behavior caps at the boundary; canary scan with one planted value per class; vector fixtures built with the existing `DLPHVEC1` writer (matching key, foreign key, header mismatch, 1%/2% unreachable); the §11 example as a golden fixture under `dev/bundles/acme-platform/` that CI validates on every push. The `claude plugin validate` and `--plugin-dir` steps are manual and recorded in the PR that adds the fixture.

## 14. Build Plan (slice B1 of `specs/022` §6)
| Slice | Touches | Tests and fixtures | Needs | Effort |
| :-- | :--- | :--- | :--- | ---: |
| **B1 Format and `validate`** (AC-1..11) | `crates/delphi-bundle/` (new leaf crate, pure functions, no I/O beyond reading the tree: `manifest.rs` for `bundle.toml` with the rejected-key list, `tree.rs` for steps 1–3, `files.rs` for `files.json` and the bundle hash, `frontmatter.rs` thin wrapper over `delphi-engine::checks::check_text("provenance")` plus the §4.2 fields, `skills.rs` calling `crates/delphi/src/skills_cmd.rs` checks (moved into a library module so both callers share them), `agents.rs` for §5B parsing and the tool list, `behavior.rs` for the §6 caps reusing `checks::check_text("rules")`, `policy.rs` for §6.5, `vectors_index.rs` for `vectors/index.json` and the `DLPHVEC1` header read from `delphi-engine/src/embed/`, `scan.rs` calling `delphi-core::redact` for `[REV-SEC-05]`, `semver.rs` for `--against`); `crates/delphi/src/bundle_cmd.rs` (new: `validate`, `--against`, `--claude` stub that only generates/compares `plugin.json`) and the subcommand table in `crates/delphi/src/main.rs`; `crates/delphi/src/lint.rs` (no-relaxation check exposed as a library function for guidance, agents, goals and perspectives); `dev/bundles/acme-platform/` (the §11 golden fixture) and a CI step that runs `cargo run -- bundle validate dev/bundles/acme-platform`; `Cargo.toml` workspace member | one test per acceptance criterion (`ac_N_…`), fixture directories under `crates/delphi-bundle/tests/fixtures/` for every refusal in §10; the golden fixture validated in CI; the manual `claude plugin validate` recorded in the PR | B0 | 20 h |

`delphi-bundle` must not depend on `delphi-engine` (so it can be fuzzed alone, as `delphi-docs` is); the few engine checks it needs (`checks::check_text`, the `DLPHVEC1` header reader) are either already in `delphi-core`-level crates or are moved to a small shared module in the same PR, with the move reviewed as its own commit.

### 14.1 As built (slice B1)
`Implements-Spec: specs/023-org-bundle-standard.md` (AC-1..11). The crate is `crates/delphi-bundle`; the command is `delphi bundle validate|manifest`. Where the build differs from, or settles, the text above:
- **Engine checks arrive through a `Host` trait**, not by moving `skills_cmd.rs` into a library: `delphi-bundle` depends only on `delphi-core` (for the redactor), and `crates/delphi/src/bundle_cmd.rs` supplies the provenance check, the skill-standard check and the built-in role scopes. The effect is the one §14 asks for (the crate is fuzzable alone); nothing moved.
- **The no-relaxation check is new and lives in the crate** (`relax.rs`): a keyword and pattern linter over one line at a time, fenced quotations skipped. It is an error in agents, role guidance, goals, perspectives and rules, and a warning in `knowledge/`.
- **Step 6 verifies since slice B6** (`specs/024` §10.4): a `sig.json` must verify strictly or the bundle fails; it was a warning ("not verified by this build") in B1.
- **Step 12 reachability needs the install's chunker**, which arrived with slice B3 (`specs/027` §10.1): `Host::chunk_keys(org, name, texts)` is real in the `delphi` binary, so the 1% / 0% thresholds run against the bundle's own text. A host that cannot chunk (a test fake) still gets the "key reachability was not checked" note. Header, index, cap and truncation checks always ran.
- **`delphi bundle manifest [--claude]`** writes `.delphi-bundle/files.json` (and the generated `plugin.json`) and prints the bundle hash, as the stepping stone before `package` (B6). The generated `plugin.json` carries no `delphi.bundle_hash`: the hash covers `plugin.json`, so it cannot also contain it.
- **`write:missions` fits** a role that holds any `missions/` write glob (a planning role holds a slice of the area); the role's own scope still caps what it may write. The tools a role may use, for intersecting an agent's `tools` list, are derived from its scope (read tools for all; `Write`/`Edit` with a write glob; `Bash` with the scoped shell).
- **`review_by` in the past** is a warning in `validate` and an error with `--package`.
- **Kept unchecked for now:** the decision-record headings of §4.3 (not in the §10 table) and `compat.delphi` against the running version (that is `add`'s refusal, slice B2).

## 15. Open Questions
B-4, B-5, B-11, B-17 in `specs/022` §7.
