---
id: spec-025-plugin-skill-and-tool-compatibility
title: "025: Plugin, Skill, Agent & Tool Compatibility: Claude Code Plugins, Agent Skills, MCP & AGENTS.md"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # the external formats were read from their published references on 2026-10-25 (§1.3); the mapping is unbuilt
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, skills, agents, mcp, plugins, portability, security, post-v1]
---

# Spec 025: Plugin, Skill, Agent & Tool Compatibility

> **Status: approved by the operator 2026-10-25 (decisions), text approved on merge.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Builds on `specs/023` (format) and `specs/024` (adding). Nothing here is built except what `specs/019` (skills import/export), `specs/010`/`020` (MCP client, harness bridge) already provide; §10 is the build plan.

## 1. Scope & Objectives
The operator asked that skill, agent and tool plugins be as widely compatible as possible, with Claude Code plugins, skills and agents, MCP servers, `AGENTS.md`-style conventions and other agent ecosystems, rather than a Delphi-only format. This spec fixes, for each external format, **what Delphi adopts as-is, what it extends, and what it deliberately does not do**, and defines the import, export and capability model that keeps a third-party skill, agent or tool within the host's authority.

1. **Claude Code plugins** (§2): the bundle *is* a plugin directory; what each component becomes in Delphi, agents included.
2. **Agent Skills** (§3): the two remaining gaps after `specs/019`.
3. **MCP** (§4): bundle-declared servers are inert until mapped; Delphi serves added bundles read-only to other agents (decided, OD-5).
4. **`AGENTS.md`** (§5): read as project knowledge, exported as a fragment.
5. **The capability model** (§6) and the **commands** (§7).

### 1.1 Non-Goals
- Running Claude Code hooks, commands, output styles, LSP servers, monitors, workflows or `bin/` executables (`specs/020` §1.1 already keeps the CLI's user-level hooks and plugins out of harness spans).
- Converting Delphi role manifests into Claude Code agents or the reverse. A Claude Code agent *runs on* a Delphi role (§2.3); it never *becomes* one.
- A marketplace *service*. Delphi emits and reads `marketplace.json` (§7.3); hosting is git.

### 1.2 Simpler Alternative Considered
- **Alternative: define nothing and tell operators to use `delphi skills import` per skill.** That exists and works for one skill at a time. Rejected as the whole answer because an organization's package has knowledge, agents, goals and policy that no skill format carries, and per-skill import loses the publisher, the version and the unit of update. The compatibility layer is small because `specs/019` did the skill part; this spec adds the plugin shell, the agent mapping and the MCP and `AGENTS.md` edges.

### 1.3 Sources (read 2026-10-25)
- Claude Code plugin manifest reference, `https://code.claude.com/docs/en/plugins-reference`: fields of `.claude-plugin/plugin.json`, the standard layout (`skills/`, `commands/`, `agents/` as Markdown files with subfolders in the name, `hooks/hooks.json`, `.mcp.json`, `.lsp.json`, `bin/` on the Bash `PATH`), `${CLAUDE_PLUGIN_ROOT}`, `userConfig`, path rules.
- Claude Code marketplace reference, `https://code.claude.com/docs/en/plugins/marketplace-reference`: `.claude-plugin/marketplace.json`, entry fields, source types `github`/`url`/`git-subdir`/`npm`/`archive`/`command`, `ref` and `sha`, `strict`.
- Agent Skills specification, `https://agentskills.io/specification`: `name` ≤ 64 kebab-case, `description` ≤ 1,024, optional `license`, `compatibility` ≤ 500, `metadata`, experimental `allowed-tools`; `scripts/`, `references/`, `assets/`; progressive disclosure.
- MCP specification 2025-11-25, `https://modelcontextprotocol.io/specification/2025-11-25` and `/server/tools`: tools, resources, prompts; tool fields `name`, `title`, `description`, `inputSchema`, `outputSchema`, `annotations`, `icons`; "clients MUST consider tool annotations to be untrusted unless they come from trusted servers"; `tools/list` pagination, `listChanged`. The MCP Registry `server.json` (schema `2025-12-11`, `packages`, `remotes`) is in preview.
- `AGENTS.md`, `https://agents.md/`: plain Markdown, no schema; nearest file wins; explicit prompts override; adopters include Codex, Jules, Aider, Cursor, Gemini CLI and the Copilot coding agent.

---

## 2. Claude Code Plugins
### 2.1 Decision: a bundle is a plugin directory (OD-2)
Every bundle is a valid Claude Code plugin root: `skills/<name>/SKILL.md` and `agents/<name>.md` are where Claude Code looks, `.claude-plugin/plugin.json` is its manifest, and everything Delphi adds (`bundle.toml`, `knowledge/`, `behavior/`, `evals/delphi/`, `vectors/`, `.delphi-bundle/`) is content a plugin loader does not read. The reverse holds too: any Claude Code plugin is a bundle once `delphi bundle import` writes a `bundle.toml` for it (§7.1).

### 2.2 `plugin.json` generation and agreement
`delphi bundle validate --claude` generates `.claude-plugin/plugin.json` from `bundle.toml` when absent, and when present verifies the fields it governs:

| `plugin.json` | From `bundle.toml` | Rule |
| :--- | :--- | :--- |
| `name` | `bundle.name` | equal (Claude Code namespaces `<name>:<skill>` and `<name>:<agent>`) |
| `version` | `bundle.version` | equal; the bundle hash and the version move together (`specs/023` §9) |
| `description` | `bundle.description` | equal |
| `author.name`, `author.email` | `publisher.name`, `publisher.contact` | copied |
| `license`, `homepage` | `bundle.license`, `bundle.homepage` | copied |
| `keywords` | `[org] + applies_to.tech` | generated; free to edit |
| `metadata.delphi.bundle`, `.bundle_hash`, `.org` | schema, hash, org | generated; Claude Code does not read `metadata` |
| `skills`, `agents` | — | not written; the default scans suffice |

Other keys in an existing `plugin.json` (`commands`, `hooks`, `mcpServers`, `lspServers`, `userConfig`, `channels`, `outputStyles`, `workflows`, `experimental`, `dependencies`, `settings`) are **left as written** for Claude Code and **ignored by Delphi**, except `mcpServers` (§4.1) and `userConfig` (§2.3). `validate` lists what it ignored.

### 2.3 What each plugin component becomes in Delphi
| Plugin component | Default location | In Delphi | Why |
| :--- | :--- | :--- | :--- |
| Skills | `skills/` | **Loaded on add** for the roles they name, within those roles' scopes (`specs/024` §3) | the shared format |
| Agents | `agents/*.md` | **Loaded on add as bundle subagents** of a host role named in `bundle.toml` or `delphi-role:`; `tools` intersected with the host's; `model` mapped to a tier; without a host role, "Claude Code only" (`specs/023` §5B) | OD-4; capped authority |
| Commands | `commands/` | Ignored with a note | a command is a slash prompt; Delphi's console commands are code |
| Hooks | `hooks/hooks.json`, `hooks` key | Ignored with a note, never translated into `specs/018` hooks | hooks run arbitrary commands |
| MCP servers | `.mcp.json`, `mcpServers` | **Inert entries** until the operator maps them (§4.1) | servers are tools; the operator classifies tools (`specs/010` §3) |
| LSP servers, workflows, monitors | `.lsp.json`, `workflows/`, `monitors/` | Ignored with a note | execution |
| Output styles, themes | `output-styles/`, `themes/` | Ignored with a note | Delphi themes are ratified packs (`specs/001` §6) |
| `bin/` | `bin/` | **Refused** (`specs/023` §10 step 1) | on the Bash `PATH` in Claude Code; Delphi never adds executables |
| `userConfig`, `channels` | `plugin.json` | **Refused** when an option is `sensitive`, else ignored with a note | a bundle must not ask for credentials |
| `settings.json`, root `CLAUDE.md` | root | Ignored | session defaults; Claude Code ignores the `CLAUDE.md` too |
| `${CLAUDE_PLUGIN_ROOT}` in bodies | skill and agent text | **Left as text**; flagged "Claude Code only" at `validate` | Delphi has no plugin root to substitute |

The rule: **Delphi takes a plugin's text and caps its authority.** Everything that would run, grant or configure stays with the tool that defined it, or waits for an explicit operator mapping.

### 2.4 Versions and pinning
Claude Code pins by `version` and, for git sources, by `sha`. Delphi pins by the bundle hash and the publisher key (`specs/024` §2.3). A marketplace entry Delphi emits (§7.3) always carries `sha`, so both readers land on the same bytes.

---

## 3. Agent Skills
Already adopted (`specs/019`). Two gaps closed here:
1. **`assets/` as a synonym of `templates/`.** `delphi lint`'s "every `templates/…` a `SKILL.md` names exists" check treats `assets/…` the same way; export keeps the publisher's spelling.
2. **`references/`.** Indexed as part of the skill (zone `skills`, `specs/004` §3.6) so an agent finds a reference by meaning; loaded on demand by `read_file` through `@bundle/`, never inlined into Tier 2 (`specs/001` §3).

Unchanged: `description` capped at 200 characters (refused, never cut); `allowed-tools` ignored with a note; scripts never executed; least-privilege defaults for missing schemas; 100 files / 512 KiB / 2 MiB per skill. One change from OD-1: a skill imported **as part of a bundle add** does not stage a Red proposal (`specs/022` §5 amends `specs/019` §3); the standalone `delphi skills import` is unchanged.

---

## 4. MCP (decided, OD-5)
### 4.1 Servers a bundle declares: inert until mapped
A bundle may ship `.mcp.json` (and `plugin.json` `mcpServers`) in the Claude Code shape. At `add` Delphi reads **only** the server name, `type` (`http` or `stdio`), `url` for `http`, `command`/`args` for `stdio`, and `headers` keys (never values), and records each server in the lock as **inert**: nothing is connected, listed, launched or offered to any role. The add summary prints them with the mapping command.

**The explicit mapping step** (an operator act, not a gate):
```text
delphi bundle connectors map <bundle> <server> --roles strategist,orchestrator --tool search_messages=read --tool read_channel=read [--first-use ask|allow]
```
writes a `[connectors.<bundle>-<server>]` entry (`specs/010` §3) with `transport`, `url` and `hosts` (the exact host of `url`, no wildcards) from the declaration and `roles`, `tools` and classifications **from the operator's flags only**; a server's own descriptions and annotations are untrusted (`specs/010` §3 item 1 and the MCP specification). `auth` defaults to `oauth`; `client_id`/`client_secret` never come from the bundle (a `headers` value or `env` entry that looks like a credential fails `specs/023` §10 step 8). The operator's command is the approval of that catalog entry (the 006-A6 pattern; `specs/022` §5 amends `specs/010` §3 to say so); from then on the connector behaves like any ratified one: definition pinning of each allowed tool (`specs/010` §3 item 2), results at $L_0$, reads Amber on first use, writes Red. An unmapped server stays inert forever; `remove` deletes inert entries and leaves mapped catalog entries in place with a note.

**`stdio` servers are never auto-launched** (OD-1 schema limit). `map` on a `stdio` server is refused in schema 1 with the reason: a local command is execution on the operator's machine, and `specs/010` §2 item 5 defers stdio for Delphi's own catalog too. The entry is still listed so the operator sees what the bundle wanted.

### 4.2 What a bundle may *not* do with MCP
Make a server trusted, raise the tier of its results, pre-pin tool definitions, pre-classify tools, or run a local server. Delphi's own `rmcp` client (`specs/010` §2 item 6) is the only client; a bundle's `.mcp.json` never reaches a harness CLI through Delphi (the CLI's own servers are governed by `specs/020` §6).

### 4.3 MCP Registry `server.json`
Not adopted now (preview). A `remotes[]` entry maps onto the same inert entry when it stabilizes; `packages[]` is stdio and is refused by §4.1. **A bundle references servers; it never installs them.**

### 4.4 `delphi bundle serve`: Delphi as a read-only MCP server (decided, late slice B12)
To make bundle knowledge usable from Claude Code and other agents **without Delphi in the loop** (the operator's "not replacing Claude Code" intent), Delphi ships one small server:
```text
delphi bundle serve [--workspace <name>] [--bundle <name>]...     # stdio, local only; no network listener
```
- **Resources:** one per knowledge file of each added bundle: URI `delphi-bundle://<org>/<name>/knowledge/<path>`, `mimeType: text/markdown`, `title` from the frontmatter, `annotations.audience: ["assistant","user"]`, `annotations.lastModified` from `timestamp`. `resources/list` is paginated; `resources/read` returns the file through the same path guard as `@bundle/`.
- **Tools:** exactly two, both with `annotations.readOnlyHint: true`, `destructiveHint: false`, `openWorldHint: false`: `search` (`{query, bundle?, limit<=10, rerank?}` → the hybrid search over zone `bundles` of added bundles, each hit as `rel_path#Lstart-Lend`, heading, preview, bundle, provenance level, tier) and `get_doc` (`{uri, start?, end?}` → a slice of one knowledge file, 256 KiB cap). No write tool, no prompt, no sampling, no roots.
- **What it never serves:** `behavior/`, `skills/`, `agents/`, `vectors/`, `missions/`, `memory/`, operator scope, removed bundles, or a bundle whose knowledge is below $L_2$ (none exist after OD-3, but the filter stays). It runs as the invoking user, reads the same `state.db` the daemon writes (read-only), and holds no key.
- **For Claude Code:** `{"mcpServers": {"delphi-bundles": {"command": "delphi", "args": ["bundle", "serve"]}}}` in the user's own `.mcp.json`; Delphi never writes it (`specs/009` §4.2 invariant 2). The tool names appear there as `mcp__delphi-bundles__search` and `…__get_doc`.
- **Why this is the one exception** to `specs/010` §1's "Delphi is not an MCP server": it exposes only data the operator has already added, read-only, on stdio, to a client on the same machine. `specs/022` §5 amends `specs/010` §1 accordingly. The bridge of `specs/020` §2 already implements the JSON-RPC server side (`initialize`, `tools/list`, `tools/call`) and is reused.

---

## 5. `AGENTS.md`
### 5.1 Reading
A project's `AGENTS.md` (root, or the nearest to a path) is a tracked project file at $L_4$ (`specs/001` §5.1), readable through `@project/<name>/AGENTS.md`. This spec adds: the org-context slice (`specs/026` §3) includes a pointer to it under "project conventions", and the builder's handoff envelope (`specs/001` §4) gains a `context_pointers` entry for the nearest `AGENTS.md` to the files the task touches, with the nearest-file precedence the format states. It is **pointed to, not inlined**, and never treated as a rule file: an `AGENTS.md` that says "skip tests" is data about the project's wishes; Delphi's gates still apply (the format itself says explicit prompts override it).

### 5.2 Writing
`delphi bundle export <name> --agents-md <path>` writes a fragment: the bundle's `knowledge/index.md` entries matching the current project's `applies_to.paths`, the glossary terms, and a paragraph from `bundle.description` and the active goals' titles. **Never** guidance blocks, agent bodies, perspectives in full, policies or rules: those are instructions Delphi loaded under the operator's add and would become unreviewed instructions elsewhere. Default path `AGENTS.delphi.md` beside the project root, not committed by Delphi (D-03; B-9).

---

## 6. The Capability Model for Third-Party Content
| Class | Examples | Loaded as | Authority |
| :--- | :--- | :--- | :--- |
| **Text to retrieve** | `knowledge/**`, skill `references/`, `AGENTS.md` | bounded, spotlighted results and `read_file` slices at the source's tier | none; the reader's ceiling drops to the content's tier |
| **Text to follow** | `SKILL.md`, `agents/*.md`, `behavior/roles/*.md`, goals, perspectives, rules | on add, for the roles named; agents on a host role | **never widening**: `required_permissions` fit the role (001-A1); `tools` intersected; guidance has no permission keys |
| **Tools to call** | MCP servers from `.mcp.json` | inert until `connectors map`; then pinned, classified by the operator | the operator's classification; a tainted span further restricted by `specs/001` §5.2 and `specs/010` §6 |
| **Things that run** | hooks, `bin/`, scripts, LSP, monitors, workflows, stdio servers | **not loaded** (ignored, inert or refused) | none |

**Capability declaration.** A bundle declares what it *touches* (`touches.roles`, `touches.connectors`, `touches.network = false`) and `validate` checks the declaration against the tree. A skill declares `required_permissions` in Delphi's vocabulary; an agent declares `tools` in Claude Code's. Neither can grant: a declaration says "I will need X of what the host already has", and `validate` or `add` drops or refuses what the host lacks. This is D-21 (narrow-only, no permission-widening grant) applied to imported content, and it is a schema limit, not a ratification (OD-1).

**Harness spans.** A role served by a subscription CLI (`specs/020`) gets bundle skills, agents and guidance through the compiled prompt like Delphi's own; the CLI's own plugins stay out (`--setting-sources ""`), so a Claude Code user with the same bundle installed as a plugin does not get it twice in a Delphi span.

---

## 7. Commands
### 7.1 `delphi bundle import <dir|archive> [--as <name> --org <org>]`
Brings a plain Claude Code plugin (or any directory with `skills/` or `agents/`) in as a bundle: writes `bundle.toml` from `plugin.json` where one exists, sets `audience = "team"`, `review_by` 180 days out, `touches` from the tree, an `[agents.<name>]` table per agent file with `role` left for the operator (`--agent-role <name>=<role>` fills it); runs `validate`; reports every ignored component (§2.3). Import never adds.

### 7.2 `delphi bundle export <name> --claude-plugin <out> [--strip-delphi]`
Writes the added bundle's tree with a fresh `.claude-plugin/plugin.json` (§2.2) and `metadata.delphi.*` on every skill. `--strip-delphi` removes `bundle.toml`, `knowledge/`, `behavior/`, `evals/delphi/`, `vectors/` and `.delphi-bundle/`; the knowledge index is then emitted as `skills/<name>-knowledge/SKILL.md` whose body is the index with relative links and the knowledge files copied under that skill's `references/`, marked `metadata.delphi.generated: "knowledge-index"` so a round trip drops it rather than indexing twice.

### 7.3 `delphi bundle export --marketplace <dir>` and `delphi bundle add --from-marketplace <repo> <name>`
Writes `.claude-plugin/marketplace.json` for a set of bundles (`name`, `owner`, one `plugins[]` entry per bundle with `source: {github|url, repo/url, ref, sha}`, `version`, `description`, `category: "org-bundle"`, `tags`, `metadata.delphi.bundle_hash`). Reading one: the entry's source is resolved, `sha` is required, the tree is fetched with the xenia store clone hardening (hooks, filters, LFS, submodules, fsmonitor off), and the ordinary `add` runs; the marketplace is a locator, never a trust source.

---

## 8. Acceptance Criteria
1. **AC-1:** the `dev/bundles/acme-platform` fixture, exported with `--claude-plugin`, is loaded by Claude Code (`claude --plugin-dir`) with exactly its two skills and one agent visible (manual, recorded in the PR); the same tree passes `claude plugin validate`.
2. **AC-2:** a plugin fixture with `commands/`, `hooks/hooks.json`, `.lsp.json`, `output-styles/`, `workflows/`, `monitors/`, `settings.json` and a root `CLAUDE.md` imports with one note per ignored component and no failure; plus `bin/` fails; plus a `sensitive` `userConfig` fails; plus a non-sensitive `userConfig` passes with a note.
3. **AC-3:** an `agents/reviewer.md` imports with `[agents.reviewer]` lacking `role`; `--agent-role reviewer=red-team` fills it; after `add` the agent dispatches as `red-team` with intersected tools (`specs/024` AC-3).
4. **AC-4:** a `.mcp.json` with one `http` server produces one inert lock entry and no network activity (network-denied run); `connectors map` with roles and tool classifications writes the `[connectors.<bundle>-<server>]` entry with the exact host and no credential; `map` on a `stdio` server is refused with the reason; a token-shaped `headers` value fails the secret scan.
5. **AC-5:** a skill using `assets/` passes the template check; a `references/` file is indexed under zone `skills` and reachable only through `read_file`.
6. **AC-6:** a project `AGENTS.md` appears as a pointer in the builder's handoff envelope and the org-context slice at $L_4$; a line in it saying "skip tests" changes no gate.
7. **AC-7:** `export --agents-md` contains index entries, glossary terms and goal titles and no text from `behavior/`, `agents/` or rules (canaries); the default path is outside the checkout and Delphi never commits it.
8. **AC-8:** `export --marketplace` writes `sha` for every bundle; `add --from-marketplace` refuses an entry without `sha` and fetches with the hardening on.
9. **AC-9:** a `--strip-delphi` export then `import` recognizes the generated knowledge-index skill and drops it.
10. **AC-10 (serve):** `delphi bundle serve` answers `initialize`, `resources/list` (paginated), `resources/read`, `tools/list` (exactly `search` and `get_doc`, both `readOnlyHint: true`) and `tools/call`; a read of a `behavior/`, `agents/` or `vectors/` URI, a removed bundle, or any path outside the cache is refused; it opens no socket and makes no network call; a Claude Code session configured with it lists the two tools (manual, recorded).

## 9. Verification Plan
Unit tests in `delphi-bundle` for the `plugin.json` agreement table (each governed field mismatched once), the component table (one fixture directory per §2.3 row, including `bin/` and both `userConfig` cases), agent import with and without a host role and with foreign tools, the `.mcp.json` reader (http, stdio, token-shaped header) with a network-denied run around `add` and `connectors map`, the `AGENTS.md` nearest-file selection with two nested files, the `--agents-md` content exclusion (canaries planted in every behavior and agent file), the marketplace writer and reader (missing `sha`, `ref` only, a `command` source refused), and the knowledge-index round trip. Protocol tests for `serve` with a scripted stdio client covering every method and every refusal, plus a `tools/list` golden. The Claude Code checks (`claude plugin validate`, `--plugin-dir`, a session listing the two served tools) are manual and recorded with the CLI version. The git-fetch hardening is covered by the xenia X4a tests once they exist and by B7's own wrapper tests until then.

## 10. Build Plan (slices B7 and B12 of `specs/022` §6)
| Slice | Touches | Tests and fixtures | Needs | Effort |
| :-- | :--- | :--- | :--- | ---: |
| **B7 Compatibility** (AC-1..9) | `crates/delphi-bundle/src/claude_plugin.rs` (new: `plugin.json` read/write/agree), `import.rs` (new: component table, `bundle.toml` synthesis), `agents.rs` (agent file frontmatter parse, host-role mapping, `tools` list), `mcp_decl.rs` (new: `.mcp.json` reader, inert entries); `crates/delphi/src/bundle_cmd.rs` (`import`, `export --claude-plugin|--strip-delphi|--agents-md|--marketplace`, `add --from-marketplace`, `connectors map`); `crates/delphi-engine/src/connectors_cli.rs` + `connectors.rs` (catalog entry writer for `map`, TTY challenge as in `auth_cli.rs`); `crates/delphi-adapter/src/config.rs` (`[connectors.*]` key for bundle-mapped entries); `crates/delphi-engine/src/handoff.rs` (nearest `AGENTS.md` pointer); `crates/delphi/src/lint.rs` (`assets/` synonym in `skill_package_errors`); `crates/delphi-engine/src/kb.rs` (`references/` under zone `skills`); the git fetch hardening shared with the xenia store clone (X4a; until X4a merges, B7 lands its own minimal `git clone --no-checkout` wrapper in `crates/delphi-engine/src/worktree.rs` with hooks/filters/LFS/submodules off) | plugin fixtures under `dev/bundles/plugin-fixtures/` (one per §2.3 row); `plugin.json` agreement matrix; `.mcp.json` http/stdio/token cases with a network-denied run; `AGENTS.md` nearest-file selection with nested files; `--agents-md` canaries; marketplace writer/reader (`ref` only, missing `sha`, `command` source refused); knowledge-index round trip; manual Claude Code checks recorded with the CLI version | B1 (export), B2 (import), B4 (agents), B6 (marketplace `sha` fetch) | 18 h |
| **B12 `delphi bundle serve`** (AC-10) | `crates/delphi-engine/src/bundle_serve.rs` (new: stdio JSON-RPC loop reusing the request handling of `bridge.rs` for `initialize`/`tools/list`/`tools/call`, plus `resources/list`/`resources/read`); `crates/delphi-engine/src/kb.rs` (`search_scored` with `SearchOpts` limited to zone `bundles`, read-only `Kb` open); `crates/delphi-core/src/pathguard.rs` (the `@bundle/` resolver reused for `resources/read`); `crates/delphi/src/bundle_cmd.rs` (`serve`); `crates/delphi/src/doctor.rs` (prints the `.mcp.json` snippet) | protocol tests with a scripted stdio client (`testkit.rs`): list, read, search, get_doc, every refusal; a network-denied run; a `tools/list` golden with the two annotations | B3, B4 | 10 h |

Merge sequencing: B7 after B4 and B6; B12 last in the set. B7's import path and B2's add path must share `validate` (one code path) so a plugin imported then added gets the same checks as a native bundle.

## 11. Open Questions
B-9 (`--agents-md` path) in `specs/022` §7. The MCP questions are decided (OD-5).
