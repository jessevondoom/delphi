---
id: spec-025-plugin-skill-and-tool-compatibility
title: "025: Plugin, Skill & Tool Compatibility: Claude Code Plugins, Agent Skills, MCP & AGENTS.md"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the external formats were read from their published references on 2026-10-25 (§1.3); the mapping is unbuilt
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, skills, mcp, plugins, portability, security, post-v1]
---

# Spec 025: Plugin, Skill & Tool Compatibility

> **Status: draft, proposed.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Builds on the bundle format of [`specs/023`](023-org-bundle-standard.md) and the adoption rules of [`specs/024`](024-bundle-trust-adoption-and-precedence.md). Nothing here is built except what `specs/019` (skills import/export) and `specs/010`/`020` (MCP client, harness bridge) already provide.

## 1. Scope & Objectives
The operator asked that skill and tool plugins be as widely compatible as possible, with Claude Code plugins and skills, MCP servers, `AGENTS.md`-style conventions and other agent ecosystems, rather than a Delphi-only format. This spec fixes, for each external format, **what Delphi adopts as-is, what it extends, and what it deliberately does not do**, and defines the import, export and permission model that makes a third-party skill or tool safe to load.

1. **Claude Code plugins** (§2): the bundle *is* a plugin directory; what each plugin component becomes in Delphi.
2. **Agent Skills** (§3): already the skill format (`specs/019`); the two remaining gaps.
3. **MCP servers** (§4): declared by a bundle, adopted as connector-catalog proposals; and, later, Delphi serving a bundle *as* an MCP server.
4. **`AGENTS.md`** (§5): read as project knowledge, exported as a fragment.
5. **The capability declaration and permission model** for anything third-party (§6).
6. **Import and export commands** (§7).

### 1.1 Non-Goals
- Running Claude Code hooks, commands, output styles, LSP servers, monitors, workflows or `bin/` executables. These are the plugin's executable authority; Delphi has its own (`specs/018` hooks, `specs/010` connectors) and does not import another tool's (`specs/020` §1.1 already keeps the CLI's user-level hooks, plugins, skills and subagents out of harness spans).
- Converting Delphi role manifests into Claude Code subagents or the reverse. The two agent models differ (Delphi roles carry scopes and counterweights enforced by a daemon; Claude Code agents are prompt files with a tool list); a lossy conversion would claim a safety property that does not hold.
- A marketplace *service*. Delphi can emit and read `marketplace.json` (§7.3); hosting is git.

### 1.2 Simpler Alternative Considered
- **Alternative: define nothing and tell operators to use `delphi skills import` per skill.** That exists and works for one skill at a time. Rejected as the whole answer because an organization's package has knowledge, goals and policy that no skill format carries, and because per-skill import loses the publisher, the version and the unit of update (`specs/024` §1.2). The compatibility layer is small precisely because `specs/019` did the skill part already; this spec adds the plugin shell around it and the MCP and `AGENTS.md` edges.

### 1.3 Sources (read 2026-10-25)
- Claude Code plugin manifest reference: `https://code.claude.com/docs/en/plugins-reference` (fields of `.claude-plugin/plugin.json`, standard layout, `${CLAUDE_PLUGIN_ROOT}`, `userConfig`, path rules).
- Claude Code marketplace reference: `https://code.claude.com/docs/en/plugins/marketplace-reference` (`.claude-plugin/marketplace.json`, plugin entry fields, source types `github`/`url`/`git-subdir`/`npm`/`archive`/`command`, `ref` and `sha` pinning, `strict`).
- Agent Skills specification: `https://agentskills.io/specification` (`SKILL.md` frontmatter: `name` ≤ 64 chars kebab-case, `description` ≤ 1,024, optional `license`, `compatibility` ≤ 500, `metadata` string map, experimental `allowed-tools`; `scripts/`, `references/`, `assets/`; progressive disclosure; `skills-ref validate`).
- MCP specification 2025-11-25: `https://modelcontextprotocol.io/specification/2025-11-25` and `/server/tools` (tools, resources, prompts; tool fields `name`, `title`, `description`, `inputSchema`, `outputSchema`, `annotations`, `icons`, `execution.taskSupport`; "clients MUST consider tool annotations to be untrusted unless they come from trusted servers"; `tools/list` pagination and `listChanged`). The MCP Registry `server.json` (schema `2025-12-11`, `packages` and `remotes`) is in preview per `https://modelcontextprotocol.io/registry/remote-servers`.
- `AGENTS.md`: `https://agents.md/` (plain Markdown, no schema; nearest file wins; explicit chat prompts override; stewarded by the Agentic AI Foundation under the Linux Foundation; adopters listed there include Codex, Jules, Aider, Cursor, Gemini CLI, Copilot coding agent and others).

---

## 2. Claude Code Plugins
### 2.1 Decision: a bundle is a plugin directory (proposed, B-1)
Every bundle (`specs/023` §2) is a valid Claude Code plugin root: `skills/<name>/SKILL.md` is where Claude Code looks, `.claude-plugin/plugin.json` is where its manifest goes, and everything Delphi adds (`bundle.toml`, `knowledge/`, `behavior/`, `evals/delphi/`, `.delphi-bundle/`) is content a plugin loader does not read. The reverse also holds: any Claude Code plugin is a bundle with only a `skills/` section once `delphi bundle import` writes a `bundle.toml` for it (§7.1).

Why a superset and not a sibling format: an organization that already publishes a plugin should not maintain two trees, and an organization that starts with Delphi should get the plugin for free. The cost is that a few plugin directories carry authority Delphi must refuse; §2.3 lists them.

### 2.2 `plugin.json` generation and agreement
`delphi bundle check --claude` generates `.claude-plugin/plugin.json` from `bundle.toml` when absent, and when present verifies the fields it governs:

| `plugin.json` | From `bundle.toml` | Rule |
| :--- | :--- | :--- |
| `name` | `bundle.name` | must be equal (Claude Code namespaces skills as `<name>:<skill>`) |
| `version` | `bundle.version` | must be equal; Claude Code pins users on `version` until it changes, so the bundle hash and the version must move together (`specs/023` §8) |
| `description` | `bundle.description` | must be equal |
| `author.name`, `author.email` | `publisher.name`, `publisher.contact` | copied |
| `license` | `bundle.license` | copied |
| `homepage` | `bundle.homepage` | copied |
| `keywords` | `[org] + applies_to.tech` | generated; free to edit |
| `metadata.delphi.bundle`, `.bundle_hash`, `.org` | schema, the bundle hash, `bundle.org` | generated; Claude Code does not read `metadata` |
| `skills` | — | not written; the default `skills/` scan suffices |

Any other key in an existing `plugin.json` (`commands`, `agents`, `hooks`, `mcpServers`, `lspServers`, `userConfig`, `channels`, `outputStyles`, `workflows`, `experimental`, `dependencies`, `settings`) is **left as the publisher wrote it** for Claude Code's benefit and is **ignored by Delphi**, except `mcpServers` (§4) and `userConfig` (§2.3). `check` lists what it ignored so the publisher knows the two readers see different things.

### 2.3 What each plugin component becomes in Delphi
| Plugin component | Default location | In Delphi | Why |
| :--- | :--- | :--- | :--- |
| Skills | `skills/` | **Adopted** through `specs/019` rules as pending Red proposals (`specs/024` §3.1) | the shared format |
| Commands | `commands/` | **Ignored with a note.** A command is a prompt invoked by slash; Delphi's console commands are code, not plugin text | a bundle cannot add console commands (`specs/024` §6 row 9) |
| Agents | `agents/` | **Ignored with a note.** Claude Code agent files are not Delphi manifests, and Delphi roles are fixed canonical ids (D-17) | no silent new role; org guidance for a role goes in `behavior/roles/` (`specs/023` §6.3) |
| Hooks | `hooks/hooks.json`, `hooks` key | **Ignored with a note**, never translated into `specs/018` hooks | hooks run arbitrary commands; Delphi hooks live only in ratified operator config |
| MCP servers | `.mcp.json`, `mcpServers` key | **Converted into connector-catalog proposals** (§4) | servers are tools; tools need the operator's classification and pins (`specs/010` §3) |
| LSP servers | `.lsp.json` | Ignored with a note | execution |
| Output styles, themes | `output-styles/`, `themes/` | Ignored with a note | Delphi themes are ratified packs with their own schema (`specs/001` §6) |
| Workflows, monitors | `workflows/`, `monitors/` | Ignored with a note | execution |
| `bin/` | `bin/` | **Refused** (`specs/023` §9 step 1) | on the Bash `PATH` while enabled in Claude Code; Delphi never adds executables |
| `userConfig`, `channels` | `plugin.json` | **Refused** when a `userConfig` option is `sensitive`, else ignored with a note | a bundle must not ask for credentials (`specs/024` §6 row 5) |
| `settings.json` | root | Ignored | Claude Code session defaults |
| `CLAUDE.md` at the root | root | Ignored (Claude Code also ignores it and warns) | — |
| `${CLAUDE_PLUGIN_ROOT}` in skill bodies | skill text | **Left as text.** Delphi does not substitute it; a skill that depends on it is flagged at `check` as "Claude Code only" | Delphi skills have no plugin root to point at; `templates/` are referenced relatively |

The rule behind the table: **Delphi takes a plugin's text and leaves its authority.** Everything that would run, grant or configure stays with the tool that defined it.

### 2.4 Versions and pinning
Claude Code pins a plugin by `version` and, for `github`/`url`/`git-subdir` sources, by `sha`. Delphi pins by the bundle hash and records the publisher key (`specs/024` §2.3, §4.1). A marketplace entry Delphi emits (§7.3) always carries `sha`, so both readers end up on the same bytes.

---

## 3. Agent Skills
Already adopted: `specs/019` checked all 43 built-in skills against the specification and found them conforming, with Delphi's four extra fields and `templates/`. Two gaps remain and are closed here:
1. **`assets/` as a synonym of `templates/`.** The standard recommends `assets/` for templates; Delphi names the directory `templates/`. A bundle may use either; `delphi lint`'s "every `templates/…` a `SKILL.md` names exists" check (`skills/CATALOG.md` §1) treats `assets/…` the same way. Export keeps the publisher's spelling.
2. **`references/`.** Allowed and indexed as part of the skill (zone `skills`, `specs/004` §3.6), so an agent can find a reference file by meaning. Each reference file must satisfy the text rules of `specs/023` §2 and is loaded on demand by `read_file` through the `@bundle/` path, never inlined into the skill's Tier 2 text (progressive disclosure, `specs/001` §3).

Unchanged from `specs/019`: `description` capped at 200 characters (refused, never cut); `allowed-tools` ignored with a note; scripts never executed; the operator names the roles; least-privilege defaults for missing schemas; 100 files / 512 KiB / 2 MiB per skill package. `skills-ref validate` is not run by Delphi; `delphi bundle check` applies the rules the specification states.

---

## 4. MCP
### 4.1 Servers a bundle declares → connector-catalog proposals
A bundle may ship `.mcp.json` (and `plugin.json` `mcpServers`) in the Claude Code shape. Delphi reads **only** these fields per server: the name, `type` (`http` or `stdio`), `url` for `http`, `command`/`args` for `stdio`, and `headers` keys (never values). It produces, per server, one **`[connectors.<name>]` proposal** (`specs/010` §3) on the adoption card, pre-filled from the bundle and **incomplete by design**:
- `transport` and `url`/`hosts` from the declaration; `hosts` as the exact host of `url`, no wildcards.
- `roles = []` and `tools = {}` **empty**: the operator fills both. A bundle never classifies a tool as `read` or `write` and never says which role may use it (`specs/010` §3 item 1: a server's own descriptions and annotations are untrusted; the MCP specification says the same of annotations).
- `auth = "oauth"` by default; `client_id`/`client_secret` **never** from the bundle (a `headers` value or an `env` entry that looks like a credential fails `specs/023` §9 step 8).
- `stdio` servers are **refused in schema 1**: a local command is execution on the operator's machine. `specs/010` §2 item 5 defers stdio to a later slice for Delphi's own catalog too. The proposal is still created so the operator sees what the bundle wanted, with the refusal reason.

On ratification the connector behaves like any ratified connector: definition pinning of each allowed tool's name, schema and description (`specs/010` §3 item 2), results at $L_0$, reads Amber on first use, writes Red. The bundle's `touches.connectors` must list every server it declares (`specs/023` §9 step 9), so the card's first screen says "this bundle proposes 1 connector" before anything else.

### 4.2 What a bundle may *not* do with MCP
It cannot make a server trusted, raise the tier of its results, pre-pin tool definitions, or run a local server. Delphi's own MCP client (`rmcp`, `specs/010` §2 item 6) is the only client; a bundle's `.mcp.json` never reaches a harness CLI through Delphi (the CLI's own servers are governed by `specs/020` §6 and the account's policy).

### 4.3 MCP Registry `server.json`
Not adopted now. The registry format is in preview and a bundle's `.mcp.json` already names what Delphi needs (a URL and a transport). When the registry stabilizes, a `remotes[]` entry maps onto the same proposal; `packages[]` (npm, PyPI, OCI) would be stdio and is refused by §4.1 anyway. Recorded so the decision is not re-litigated: **a bundle references servers; it never installs them.**

### 4.4 Delphi serving a bundle as an MCP server (later; B-8)
`specs/010` §1 and `specs/020` §1.1 keep Delphi from acting as an MCP server for anything but one span's CLI. The operator's long-term note (memory, 2026-10-24) was that the org layer might be exposed "as an MCP/plugin/integration". The compatible way is a **read-only server** `delphi bundle serve <name>` that exposes, to a local client (another tool on the same machine over stdio, or a teammate's Claude Code through the harness bridge socket), **resources** (one per knowledge file, URI `delphi-bundle://<org>/<name>/knowledge/<path>`, `mimeType: text/markdown`, annotations `audience: ["assistant","user"]`), **prompts** (one per adopted skill and one per perspective, as templated messages), and **no tools**. It serves only $L_2$-or-better content of adopted bundles, never behavior that is not ratified, never operator scope, and never anything from `missions/` or `memory/`. It is listed as slice B10 in `specs/022` §6 and is optional; the operator decides whether the surface is wanted at all (B-8).

---

## 5. `AGENTS.md`
### 5.1 Reading
A project's `AGENTS.md` (root, or the nearest one to a path) is a tracked project file, so it already has tier $L_4$ at the base commit under `specs/001` §5.1 and is readable through `@project/<name>/AGENTS.md`. This spec adds one deterministic behavior: when a workspace's project has an `AGENTS.md`, the org-context slice (`specs/026` §3) includes a pointer to it under "project conventions", and the builder's handoff envelope template (`specs/001` §4) adds a `context_pointers` entry for the nearest `AGENTS.md` to the files the task touches, with the nearest-file precedence the format states. It is **pointed to, not inlined**, and it is never treated as a rule file: an `AGENTS.md` that says "skip tests" is data about the project's wishes, and Delphi's gates still apply (the format itself says explicit prompts override it).

### 5.2 Writing
`delphi bundle export --agents-md <name>` writes a fragment: the bundle's `knowledge/index.md` entries that match the current project's `applies_to.paths`, the glossary terms, and a one-paragraph "how this organization works" taken from `bundle.description` and the active goals' titles. **Never** the guidance blocks, perspectives in full, policies or heuristics: those are Delphi-ratified instructions and would become unreviewed instructions in another tool. The fragment is written to a path the operator names (default `AGENTS.delphi.md` next to the project root, not committed by Delphi: D-03, Delphi never writes into a project repository; the operator commits it if they wish).

---

## 6. Capability Declarations and the Permission Model for Third-Party Content
Everything external lands in one of four classes, and the class, not the source, decides what it may do:

| Class | Examples | Loaded as | Permission effect |
| :--- | :--- | :--- | :--- |
| **Text to retrieve** | `knowledge/**`, skill `references/`, `AGENTS.md` | bounded, spotlighted search results and `read_file` slices at the source's tier | none; the reader's span ceiling drops to the content's tier |
| **Text to follow** | `SKILL.md`, `behavior/roles/*.md`, goals, perspectives, heuristics | only after per-file ratification ($L_5$); skills only for roles the operator names | **never widening**: `required_permissions` must fit the role's built-in scope (001-A1); guidance has no permission keys |
| **Tools to call** | MCP servers from `.mcp.json` | connector proposals; tools invisible until the operator lists and classifies them; definitions pinned | the operator's classification decides Green/Amber/Red; a tainted span is further restricted by `specs/001` §5.2 and `specs/010` §6 |
| **Things that run** | hooks, `bin/`, scripts, LSP, monitors, workflows, stdio servers | **not loaded** (ignored or refused) | none, because nothing runs |

**Capability declaration.** A bundle declares what it *touches* (`touches.roles`, `touches.connectors`, `touches.network = false`), and `check` verifies the declaration against the tree (`specs/023` §9 step 9). A skill declares `required_permissions` in Delphi's vocabulary (`read:all`, `write:worktree`, `write:missions`, `shell:scoped_scripts`); the standard's `allowed-tools` is noted and ignored. There is deliberately **no** declaration that grants: a declaration can only say "I will need X of what the role already has", and adoption refuses if the role does not have it. This is the same shape as D-21 (narrow-only, no permission-widening grant) applied to imported content.

**Taint of imported text.** A skill's body is instruction text; once ratified it is $L_5$ like any ratified skill, because the operator read it. Knowledge stays $L_2$ (`specs/024` §3). An `AGENTS.md` keeps its project tier. Nothing imported ever carries a tier in itself (`specs/023` §3.1).

**Harness spans.** A role served by a subscription CLI (`specs/009` §7, `specs/020`) gets ratified bundle skills and guidance the same way it gets Delphi's own skills: through the compiled prompt, with the bridge offering Delphi's tools. The CLI's own plugins stay out (`--setting-sources ""`), so a Claude Code user who has the *same* bundle installed as a plugin does not get it twice in a Delphi span.

---

## 7. Commands
### 7.1 `delphi bundle import <dir|archive> [--as <name> --org <org>]`
Brings a plain Claude Code plugin (or any directory with `skills/`) in as a bundle: writes a `bundle.toml` from `plugin.json` where one exists (`name`, `version`, `description`, `author`, `license`, `homepage`) or from the flags; sets `audience = "team"`, `review_by` 180 days out, `touches` from the tree; runs `specs/023` §9; reports every component it ignored (§2.3). The result is a directory the operator can inspect and then `adopt` (`specs/024` §4.2). Import never adopts.

### 7.2 `delphi bundle export <name> --claude-plugin <out>`
Writes the adopted bundle's tree with a fresh `.claude-plugin/plugin.json` (§2.2) and with `metadata.delphi.*` fields on every skill (`specs/019` export form). Nothing is removed: the plugin loader ignores what it does not know. `--strip-delphi` removes `bundle.toml`, `knowledge/`, `behavior/`, `evals/delphi/` and `.delphi-bundle/` for a publisher who wants a plugin-only artifact; the knowledge index is then emitted as `skills/<name>-knowledge/SKILL.md` whose body is the `knowledge/index.md` list with relative links, so a Claude Code user still finds the knowledge by name (the knowledge files themselves are copied under that skill's `references/`). This generated skill is marked `metadata.delphi.generated: "knowledge-index"` so a round trip back into Delphi recognizes and drops it rather than indexing the knowledge twice.

### 7.3 `delphi bundle export --marketplace <dir>`
Writes `.claude-plugin/marketplace.json` for a set of bundles (`name`, `owner` from the org's publisher, one `plugins[]` entry per bundle with `source: { "source": "github" | "url", "repo"/"url", "ref", "sha" }` where `sha` is the git commit holding the bundle at its hash, `version`, `description`, `category: "org-bundle"`, `tags`, `metadata.delphi.bundle_hash`). The marketplace is a static file in a git repository; Claude Code users add it with `claude plugin marketplace add <owner>/<repo>`. Delphi can **read** a `marketplace.json` too (`delphi bundle adopt --from-marketplace <repo> <name>`): it resolves the entry's source, requires `sha`, fetches the tree with the git hardening of the xenia store clone (`xenia-phase-1-plan.md` R2: hooks, filters, LFS, submodules off), and then runs the ordinary adoption; the marketplace is a locator, never a trust source (`specs/011` §6).

---

## 8. Acceptance Criteria
1. **AC-1:** the `dev/bundles/acme-platform` fixture, exported with `--claude-plugin`, is loaded by Claude Code (`claude --plugin-dir`) with exactly its two skills visible and nothing else (manual, recorded in the PR); the same tree passes `claude plugin validate` without errors.
2. **AC-2:** a plugin fixture with `commands/`, `agents/`, `hooks/hooks.json`, `.lsp.json`, `output-styles/`, `workflows/`, `monitors/`, `settings.json` and a root `CLAUDE.md` imports with one note per ignored component and no failure; the same fixture plus `bin/` fails; plus a `sensitive` `userConfig` fails; plus a non-sensitive `userConfig` passes with a note.
3. **AC-3:** a `.mcp.json` with one `http` server produces one `[connectors.<name>]` proposal with empty `roles` and `tools`, the exact host, and no credential; a `stdio` server produces a refused proposal with the reason; a `headers` value that looks like a token fails the secret scan; nothing is connected or called until ratification (network-denied test run).
4. **AC-4:** a skill using `assets/` passes the template-existence check; a `references/` file is indexed under zone `skills` and is readable only through `read_file`, never inlined into the skill text.
5. **AC-5:** a project `AGENTS.md` appears as a pointer in the builder's handoff envelope and in the org-context slice, is tiered $L_4$, and a line in it saying "skip tests" changes no gate (the merge gates still run).
6. **AC-6:** `export --agents-md` contains index entries, glossary terms and goal titles and contains no text from `behavior/roles/`, `policies.toml` or `heuristics.md`; the output path is outside the project checkout unless the operator names one inside it, and Delphi never commits it.
7. **AC-7:** `export --marketplace` writes entries with `sha` for every bundle; `adopt --from-marketplace` refuses an entry without `sha`, fetches with hooks/filters/LFS/submodules disabled, and runs the full `specs/023` §9 check before any card.
8. **AC-8:** a round trip `export --strip-delphi` then `import` recognizes the generated knowledge-index skill and drops it, so no knowledge file is indexed twice.

## 9. Verification Plan
Unit tests in `delphi-bundle` for the `plugin.json` agreement table (each governed field mismatched once), the component table (one fixture directory per row), the `.mcp.json` conversion (http, stdio, token-shaped header, `touches.connectors` missing the server), the `AGENTS.md` pointer (nearest-file selection with two nested files), the `--agents-md` content exclusion (canaries planted in each behavior file), the marketplace writer and reader (missing `sha`, a `ref` only, a `command` source refused), and the knowledge-index round trip. The Claude Code checks (`claude plugin validate`, `--plugin-dir`) are manual and recorded with the CLI version. The git-fetch hardening is covered by the xenia X4a tests and reused.

## 10. Open Questions
B-1 (bundle as plugin superset), B-8 (serving as MCP), B-9 (`AGENTS.md` fragment path and whether to offer committing it) in `specs/022` §7.
