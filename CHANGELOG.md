# Changelog

What changed in each release, newest first. Delphi is pre-1.0: minor versions may change commands and formats, and the notes say so when they do. The `dev` tag is a rolling build of `main`, made on request (`delphi update --dev`).

Release builds, with their notes and build attestations, are on the [releases page](https://github.com/jessevondoom/delphi/releases). Entries that cite a pull request (`#nnn`) refer to this repository's pull requests.

This file starts with the work after v0.2.7. For earlier releases, the history is the merged pull requests and the per-spec notes under `specs/`; their entries below give dates, not detail.

## Unreleased

## 0.6.2 — 2026-11-14

A summary by spec; the pull requests are listed on the releases page. What has and has not been run for real is said per item: most of the host work was checked live in Claude Code, Copilot CLI and Codex; Gemini CLI could not be signed in to, and Codex role files were not checked.

- **Removed: the terminal narrator** (spec 111 §10, #701; the operator's decision of 2026-11-14). `delphi observe`, its alias `delphi narrator`, `--once`, and `/observe`, `/narrator` and `/visualize` in the console are gone, with the narrator's views. Each now answers with one line naming the desktop app's guide and exits 2. With it goes a read-only view over ssh or in tmux; the console and `delphi status` remain there. `[tui] auto_spawn_narrator` is still accepted in a config and does nothing. The daemon's read-only narrator socket and its methods are unchanged.
- The desktop app, in source only (spec 111, #700, #701): `app/` holds a window that shows the daemon's state and changes nothing: activity, a Council as it deliberates, the knowledge base, what the team produced, and each agent's manifest. It attaches on the read-only narrator scope only and can send nine read methods. **This release contains no built app**: build it with `app/dev/build.sh` (macOS, unsigned; no Linux build has been made). CI does not cover `app/`, and nobody has yet checked the window against its spec screen by screen. See [the desktop app](docs/guides/desktop-app.md).
- Role colours (spec 111 and spec 106 G6 and G7; #696, #699, #701): on a light background each role now has a text colour (4.5:1) and a brighter fill (3:1 as a graphic). `delphi connect claude-code --theme` installs, with a `[Y/n]` question at a terminal, a Claude Code theme that changes only its eight subagent colours to the roles' own and sets nothing else in your settings; `--dry-run` previews and `--remove` undoes it. Seen live with scratch agents, not with Delphi's own role subagents.
- Foreign seats that do not block a host (spec 104, #689, #694): `ask_family` is a job with `ask_result` and `cancel_job`; `delphi mcp` answers up to 8 tool calls at once; a waiting call sends progress; `[access] standing_foreign_tokens` (default 0, off) is a standing consent with a daily cap; a Panel runs in four calls. No host's display of the progress lines has been seen.
- Session hooks for Claude Code, on by default (spec 105, #688, #697, #698): the plugin's hooks print a brief at session start, route an `@role` mention at the prompt, and write a marker at session end. Turn them off with `delphi mcp --hooks off`.
- Compact packs and a worker for every role (spec 106, #690, #696): `get_agent` serves a compact pack by default (31 to 41 percent smaller; `detail: "full"` gives the whole one), the charter is the resource `delphi://charter`, and the Claude Code plugin carries a `<role>-worker` subagent for twelve roles, each given its pack by a `SubagentStart` hook.
- The orchestrator mention reads the task (spec 107, #692): `consult_orchestrator` ranks the roster for the task as `route` does and says what to do next, and every instruction text says so the same way.
- The learning loop for host sessions (spec 108, #691): `recall_sessions` returns what earlier host sessions of the project left; `delphi learn keep` keeps a staged note after you type yes at a terminal; transcript ingest is opt-in per project; the trace harvester writes an episode when a daemon task ends.
- One retrieval path for host search (spec 109, #693): `search_kb`, `kb_retrieve`, `search_memory` and `query_memory` use one retrieval service over a fresh index; registered projects are indexed as their own layers; results are labelled by tier instead of hidden; `get_agent` takes a `task` and returns passages for it; a role may set `search_use`.
- Other hosts (spec 110, #695): `delphi connect codex` and `delphi connect gemini`, adapters for Copilot CLI and Codex, role files in each host's own terms, `session_brief`, and live-check scripts under `dev/hosts/`. Claude Code, Copilot CLI and Codex were run against a temporary home; Gemini CLI was not.
- The host socket outlasts the daemon's longest wait, so a long `ask_family` or Council read is not cut off (spec 098, #686). The specs for all of the above are #687.
- The rolling dev release is built on request, by a `dev-*` tag or a manual run, not on every merge (#685).
- Fixed: `query_memory` no longer returns another role's memory file or another project's mission file.

## 0.6.1 — 2026-11-13

A summary by spec; the pull requests are listed on the releases page. Nothing in specs 098 to 103 below has been run against a real host, Council, Telegram bot or mailbox yet: the tests use fakes and scratch homes.

- The first `delphi` start in a terminal now asks one question about the history (spec 102, below). Nothing is recorded until you answer it; an answer of no turns the history off.
- The exchange record, a history of your own prompts (spec 102, #668 to #676, #681): on by default, but it records nothing until a person at a terminal has answered the install question. It stores your prompts and capped reply summaries on this machine, with secrets redacted (best effort: some secret shapes may not be masked), pasted blocks as a hash and a length, and short pasted text kept as written; the mission logs keep prompts after a purge. `delphi history off|purge [--since]|delete|status`, the console view `/history`, a daemon timeline, and a citation check on decisions. It never leaves the machine and is never read as an instruction.
- The private-read mark (spec 103, #674, #677 to #680): a task that read a private entry asks you before it sends a web request or a connector call, and a relay or mailbox gets a fixed phrase in place of the text an agent wrote for it. A child task has the same scope as its parent within a session. Only the host's `search_kb` sets the mark today, so Delphi's own tasks are not marked yet.
- Where agents run, host first (specs 098 and 099): Councils run inside Delphi only, and a host that cannot run one gets a labelled unaudited agenda (#617); roles and skills carry a locus (#618); `route` takes any query (#622); research reports its coverage and thin answers in `doctor` (#619); `corroborated` status with an origin count (#623); a claims-based `propose_kb_entry` with a private flag and an egress check (#626, #645); red-team review and `ask_family` (#627, #648); seat states, a stall flag and `cancel_job` (#621); a Round 2 critique is kept when it is valid, Markdown restatements are read, and an unreadable Round 2 is repaired once (#613, #614, #616). The five feature flags added along the way were removed again, with a roll-back plan in spec 098 §13.2 (#642, #653).
- The confidence ladder (spec 100): a tier table, `would_ratify` and a fast lane, a roll-back with a tier audit and a latch, accept-all for vetted claims, a per-tier calibration report, KB-prior tags, and a contradiction step that reads the validated facts (#625, #636, #637, #640, #644, #646, #657, #663). The background validation worker (spec 040 §18.3, #667) is off by default (`[validator] background`).
- Library practice for research (spec 101): a search log for a research run, a local Retraction Watch snapshot and a retraction mark, archived copies read from a web archive as a labelled version, and citation chaining through one pinned index (#635, #651, #652, #655, #658, #664). The lookups send only public URLs or DOIs.
- The merge flow: `delphi merge` judges and records every spec named by trailers (#650) and refuses a bundle across specs (#654).
- Fixed: on a fresh install, the first host `search_kb` no longer fails with `no such table: kb_meta` (#682).
- CI and build, no change to behaviour: package-host outage survival, caches, split Linux tests, shorter test waits, longer limits for the embedding-model jobs (#629, #630, #638, #647, #649, #659, #665, #666).

## 0.6.0 — 2026-11-09

A summary by spec; the pull requests are listed on the releases page.

- Operator-authorized tests (spec 092, GOVERNANCE §5.12): an agent or a host may ask for an advance authorization of a few exact commands for a headless test (`request_test_authorization`). It grants nothing until the operator reviews and authorizes it at a terminal (`delphi test`), and `delphi run --on-card authorization:<id> --auth-run <run>` then approves only that pinned, sandboxed `run_command`, on macOS.
- The core-repo merge flow (spec 094): a merge queue label, `delphi merge <PR>` and `--all` with a recorded operator approval, a session-scoped `--auto`, cleanup after merge, a merge error log and a builder git skill. Agents never merge on their own.
- A spec compile step (spec 088): `delphi specs compile|show|list|report`, the derived views under `compiled/specs/`, the `specs_show` and `specs_list` MCP tools and the `spec-driven-build` skill. Reads through Delphi are counted.
- Three spec numbers that two specs shared are renumbered once (spec 090): documents and data visualization is 095, the console deck change log is 096, encyclopedia search pages is 097. `delphi lint` refuses a second spec on a taken number.
- Councils and Panels: several foreign-family seats on a Panel, counted by family; a carrier plan in `council_frame`; option ids compared by id in Round 2; Copilot seats answer Round 2; host pointers reach a registered project as `@project/<name>/<path>`.
- Fixed: a tool-less `agy` seat ran without Delphi's gate and is now always gated, and receipts show the tools actually granted (#591); `delphi update` could not download a release asset (#575); four causes of flaky tests (#604); the sandbox process cap (#611); `cargo deny` advisories (#610).
- The README introduction is rewritten around four ideas: agents that work together, memory and context that carry over, one team in many environments, and a knowledge base that grows into expertise. It adds a "Why it exists" section, host examples (`@delphi`, a Panel), and a status section that says what has been measured. The one-line summaries in `docs/README.md` and `llms.txt` follow it. No behavior change.

## 0.5.6 — 2026-11-08

Not otherwise itemized; see the releases page.

- Agent identity outside the console (spec 087 S3): a Delphi answer's identity now reads `⟦badge⟧ Name (role, via delphi)` (it was `Name (role) via delphi`) and arrives as one ready-to-print `display` line. A role a host adopts with `get_agent` shows the same identity and notice; the old "acting as" wording is gone. New `[mcp] badge_mode = "dark" | "light"` picks the colour pair for the ANSI form. Results may carry advisory `boundary_line` and `return_line`, and `delphi_roster` opens with a roster line (it takes an optional `speaking` list). Hosts may drop all of these.

## 0.5.5 — 2026-11-05

- Two specs approved, nothing built (079, 080): an ingress taint registry test where core assigns each ingress a ceiling by transport origin, and a knowledge-base read-count report with a `host_search_kb` event and an `invariant: security|constraint` attribute. Drafts 078 (Council scorecard) and 081 (conventions note) come from the same Council and are not approved. The spec tracker has the order and the open items.
- `.delphi-project`, the marker `delphi project register` writes into a registered repository, is now ignored by this repository's `.gitignore`.
- Install and update from a private repository (spec 076): `install.sh` and `delphi update` use your GitHub credentials (`gh`, `GH_TOKEN`, `GITHUB_TOKEN`) for the lookup and download, never sending a token to a mirror or a redirect host. Where a repository has no build attestations, the installer checks the maintainers' release signature instead; an attestation that exists and fails still stops the install. Homebrew works only while the repository is public.
- Knowledge base search (spec 075): `delphi kb search` shows summary pages by default, with `--summaries` and `--bibs` to choose; the console follows `[tui] hide_bibliographies`. Agents still get the bibliography sidecar.

## 0.5.4 — 2026-11-04

- Host plugin marketplace (spec 071): Delphi is installable as a plugin in Claude Code, Copilot CLI and Codex from this repository (`<host> plugin marketplace add jessevondoom/delphi`). `delphi mcp --check <host>` and a `delphi doctor` line read each host's state, and `delphi mcp --print-config <host> --absolute` prints a snippet with this binary's full path (#467).
- `delphi connect` (spec 074): `delphi connect claude-code|copilot` with `--dry-run`, `--status` and `--remove`. It shows the exact plan and the host binary's checksum and runs only after you type a confirmation phrase at a terminal; it refuses inside an agent seat. A launch-path line in `--check` and `doctor` says whether the host can find `delphi`. Codex is not yet supported (#467, #468, #469).
- Host cognitive parity (spec 072): MCP hosts gain `consult_orchestrator`, `spawn_agent_pack`, `kb_retrieve`, `query_memory`, `scaffold_status` and `propose_council_record`, and answers are labelled `via delphi` (#466).
- Console adaptive contrast (spec 073): a dual-mode palette for dark and light terminals at WCAG AA contrast, with role colours adapted for light backgrounds (#471).
- The README is rewritten around research, trust and decisions; its reference detail moved into `docs/` (`features.md`, `how-it-works.md`, `specifications.md`, `reference/console.md`) (#470).

## 0.5.3 — 2026-11-03

- Scope of *No Spec, No Build* and the artifact prompt log (spec 070): the spec gate covers software only. Documents, decks, images and media need no spec; each create or edit by an operator-started task is recorded in a daemon-written, hash-chained log of the operator's redacted prompt (`local/artifact-log/<mission>/artifact-prompts.jsonl`, schema `prompt-log.schema.json`). Markdown notes and scaffolds stay regular operation. GOVERNANCE §1.3 amended.
- Workspace administration and governed operator assistance (spec 069) (#464).

## 0.5.2 — 2026-11-02

- Self-updater size limit expansion (spec 006): separate downloaded archive safety limit (`MAX_ARCHIVE = 512 MiB`) and extracted binary limit (`MAX_BINARY = 384 MiB`) to support embedded neural models, seed compendiums, and preloaded vector caches (#457).
- Specialized agent fleet expansion: Administrator, Assistant, and Advocate canonical roles, 8 companion skills, and seed reference compendiums (#456).

## 0.5.1 — 2026-11-01

- Research and identity (spec 066): research jobs, planner and runner, a research endpoint, attribution, docs and security invariants for the MCP research surface (#445–#453).
- Episodic memory traces (spec 065) and knowledge retrieval (spec 064) (#442, #443).
- The Integrator role is renamed Messenger (#444).
- Copilot builder parity (spec 051) and agy builder activation (spec 020) (#440, #441).

## 0.5.0 — 2026-11-01

### Fleet Architecture and Dynamic Orchestration (Spec 063)

- **Fleet-wide visibility & decoupled dispatch**: The Orchestrator has complete visibility into all registered canonical roles and bundle specialists to autonomously decompose tasks, route specialist assignments, and coordinate learning.
- **Interactive focus & dynamic session introductions**: Operators interact directly via `@` mentions with the interactive roster (`interactive = [...]`, default 5 core roles). When the orchestrator dispatches any specialist into the conversation, the agent is introduced and becomes dynamically addressable for the remainder of the session.
- **Operator controls**: Added `/solo` dialog mode (capping sub-consultation tokens to keep interaction 1:1) and dynamic quarantine controls (`/quarantine <role>`, `/unquarantine <role>`) to block or restore dispatch and direct interaction.
- **Cognitive & specialist skills**: Added 8 modular skills (`strategic-deployment`, `engineering-patterns-breadth`, `socratic-council-interrogation`, `competitive-analysis-swot`, `dual-horizon-search`, `hypothesis-flipping`, `oblique-strategies`, `tonal-calibration`).
- **Curated reference compendiums**: Added curated knowledge bases in `context/reference/` for Oblique Strategies, strategic maneuvers, engineering architecture paradigms, and tonal registers.
- **Collaboration tracking & tenacity pushback**: Dynamic metrics tracking for agent collaboration outcomes and persistent tenacity pushback for the Investigator.

### Shell

- An agent that asked for research with `request_research` is now told how it went, once, at the start of its next turn (and the orchestrator too): counts of pages kept, held for validation or left out, or why it did not start or failed; never page text (spec 058-A10). A failed or cancelled run is logged too, and the outcome goes to the requesting session's log.
- `delphi check fact` now looks up a stored journal page's DOI at doi.org, as the daemon does, so the two agree; `--offline` keeps the text-only check (spec 040-A6).
- New sessions are written under `local/sessions/<id>/` (spec 058-A9); sessions already under `local/missions/` stay where they are and keep working, and every listing, scan and check reads both. Agents still use the `missions/<id>/…` path.
- A `shell` tool: a full shell line for agents, sandboxed when a sandbox is on, otherwise asked about. Lines are classed Safe, Caution or Hard; Hard lines (`sudo`, `rm -rf`, `git push`, writes outside the worktree, `[exec] hard` patterns) are always a Red card (spec 061).
- `shell-auto` mode runs safe shell lines without asking; `[s]` / `/s` allows similar shell commands for the session, and `/permissions revoke` ends the grants.
- `!<line>` in the console runs a line yourself via `shell.run`; the output is never sent to a model.

### Validator

- Weak sources are labelled, never evidence (spec 040-A2): a claim only Weak sources support is `unverified (weak)`, and agreement among Weak sources is recorded but changes no label. New `[validator] weak_sources = "label"|"off"` (default `label`).
- A Wikipedia sentence tagged "citation needed" or carrying no citation, and a journal-publisher page with no DOI, no longer count as sources (spec 040-A3).

### Research

- A keyless search engine, `parallel` (Parallel's hosted MCP server; no key or account, rate limited; it sees the query text), and it is now the default when `[search] providers` is unset (spec 028-A3). Brave and the others are unchanged; `providers = []` turns research off.
- Research routing (spec 058-A1): in the default scope only a Primary or Reputable source is indexed at once; a weak page is held under `local/pending/web/` (not indexed) and is dropped when `[validator] weak_sources = "off"`. `research.start` takes an optional registered `project`, whose research is written to its `research/web/` and not shared. Entries carry `source_class`.
- `/validate` (also `/retro research`) lists research held back from the knowledge base, in the default scope or a project (`project=<name>`), and ratifies or discards each entry; ratified entries are searchable as noted, not validated (spec 058-A2). New daemon methods `pending.list`, `pending.ratify`, `pending.discard` (console only).
- `/projects` lists the home workspace and the registered projects in an arrow-key menu; Enter restarts the console in the chosen directory as a new session (spec 058-A3). A request outside the home workspace and registered roots is ignored.

- `/projects` reads the registry each time (new console-only `projects.list`), and `research.start` takes the console's `cwd`: research started inside a registered project is written to that project (spec 058-A4).
- The fact validator now looks a journal-publisher page's DOI up at doi.org (spec 040-A5): a made-up DOI, one registered outside a publisher, or any lookup failure means the page is not counted.
- Agent-started research (spec 058-A8): the `request_research` tool asks you with a card every time, then runs the same job in the session's own scope.
- `/research <topic>` (spec 058-A7): start the research job from the prompt, with `status` and `cancel`; it uses the console's directory, so project routing applies.
- Validator pass (spec 058-A6): `/validate pass` promotes held research whose source the allow list now trusts; it runs automatically when a session opens (default scope), never promotes weak sources, and moves a project's pages only when you name the project.
- Weekly digest (spec 058-A5): when at least five research pages are waiting outside the knowledge base (held, or in a project and not yet ratified) and a week has passed, the orchestrator says so once in the chat (`research.digest`) and points to `/validate`. A message, never a card.
## 0.4.0 — 2026-10-31

Docs and tests: `cargo test -p delphi --test spec_refs` checks that every spec section and amendment reference (`NNN §X.Y`, `NNN-An`) still resolves, so folding amendments into specs cannot silently break one.

### Security

- **Promotion hardening** (spec 058-A2): records below `federated_verified` cannot be promoted to home memory (`source_untrusted`), and `local/promoted/` is capped at 200 active entries / 256 KiB (`promoted_cap_reached`).

### Fixed

- **A rejected edit no longer buys a stronger model** (spec 015-A2, council 013 D10): an operator rejection still counts toward the three attempts but never upscales the model or changes the family; only gate failures, turn exhaustion and audit findings climb the retry ladder.
- **Taint no longer fades between missions** (spec 001-A4, 004-A4): files under `missions/**/research/**` and `missions/**/reports/**` and `missions/**/artifacts/**` read as `L0` in later missions when they have no stamp; a span at `L1` or below can write only such paths; the gated-path check sees through `@worktree/` and `@project/<name>/`; and `suggest_rule` refuses from a tainted span.

### Added

- **Connector drafts always ask** (spec 010-A2): every draft gets its own card with the payload and "allow similar" never covers one; a draft from a span that read untrusted content, or one naming recipients or invitees, is Red.
- **Relay merge boundary** (spec 014 amendment 014-A1, REV-SEC-09): a merge, pull-request or constitution/ratification card can no longer be approved by relay reply, even with a valid authenticator code; and step-up's used-code and lockout state now persist in `local/totp-state`, failing closed if the file is corrupt, so a restart no longer resets them.
- **Harness hook boundary** (specs 018-A1, 021-A1; council 013 D13): a ratified `[harness.claude.settings]` (or agent) `hooks` entry of kind `PreToolUse` or `PermissionRequest` is refused with a notice naming the key, because an allow-capable hook is runtime-only enforceable; observational hooks still work and guards belong to `[[hooks.deny]]`.
- **Excerpts, anchors and bibliographies in the central library** (spec 055 SK2): `delphi sources excerpt add|list` keeps quoted passages with a locator and purpose (4 KiB each; for a restricted source not held whole, 10% of the work or 16 KiB in total) and an anchor with prefix and suffix that is found again when the source changes; `delphi sources bib` writes APA, MLA, Chicago, plain text, CSL-JSON or BibTeX and never a private source. `delphi sources add` now reads Wikipedia, DOI and Gutenberg metadata by code, and `sources show` rechecks a source last checked over 30 days ago through the fetcher.
- **Sources in org bundles** (spec 055 SK0, spec 023 §2.1): an optional `sources/` section with full-text captures and images, gated by licence (only open SPDX ids, `public-domain:<jurisdiction>:<basis>` and `own` travel; `all-rights-reserved`, `unknown` and `private` are refused), with its own size class, `schemas/source-meta.schema.json`, and a sources table in `bundle validate --report`.

### Changed

- **Optimistic is a per-build choice, not an approval mode** (spec 015-A1, council 013): `optimistic` is gone from `/permissions`, `delphi run --permissions` and the `Shift+Tab` cycle; `/build --optimistic` is unchanged. Entering `auto` with `Shift+Tab` now takes a second press, and bulk-approving Amber decisions or cards (`a`, `y`, inbox approve-all) covers only those whose detail was shown at least once (specs 005e-A1, 044-A1).
- Docs: folded spec 004 (memory and coaching): amendments 004-A1 to A4 merged into the normative text, measurements and history moved to `specs/history/004-memory-and-coaching.md`; no behavior change.
- Docs: folded spec 006 (installer and updater) so its amendments 006-A1 to A6 read as current text; no change in behavior; history in `specs/history/006-installer-and-updater.md`.
- Docs: folded spec 007 (federated mesh and sync) so it reads as current normative text; amendment banners and history moved to `specs/history/007-federated-mesh-and-sync.md`. No change in behavior.
- Docs: folded spec 010 (connectors) so amendments 010-A1 and 010-A2 read inline; history moved to `specs/history/010-connectors-and-tool-access.md`. No change in behavior.

- Docs: folded spec 043 (condensed rationale prose; no behavior change); history in `specs/history/043-knowledge-window-search-page-and-growing-corpus.md`.

## 0.3.0 — 2026-10-28

### Added

- **A central error log.** Errors, panics and (opt in) warnings and debug detail from the CLI, the daemon and the console go to one redacted, size-capped file, `$DELPHI_HOME/logs/delphi.log.jsonl`, without ever disturbing the operation; `delphi logs`, `delphi debug bundle`, a `delphi doctor` check, `DELPHI_LOG` and `[logging]` in the operator config (spec 039).
- **Documentation and machine-readable specs.** A user guide in `docs/`, a command reference and man page generated from the program (`delphi docs cli`, `delphi docs man`), shell completions (`delphi completions bash|zsh|fish`), JSON Schemas in `schemas/` for `config.toml`, `bundle.toml`, event and handoff envelopes, Xenia records and signatures, and `AGENT.md` and `SKILL.md` frontmatter, plus `llms.txt`, `AGENTS.md`, `CONTRIBUTING.md` and this file. Tests fail when the reference or a schema drifts from the program.
- **Org bundles.** Authoring (`bundle init`, `distill`, `validate --report`, `package --vectors`; spec 027, #211); compatibility with Claude Code plugins (`bundle import`, `export --claude-plugin|--agents-md|--marketplace`, `add --from-marketplace`; #210); provenance, archives, `update` and `rollback` (#200); shipped vectors with incremental embedding (#197); alignment checks and Council lenses (spec 026, #198); the org-context slice for planning roles (#191) and `/org` in the console (#194); bundle agents and deny policies (#183).
- **Xenia, the org memory store .** Signed-record validation (#212), roster chain and pins (#208), a hardened store clone and sync (#209, #219), standing, tiers, endorsements and tombstones (#224), the index and retrieval views (#229), the publish gate (#227). Admin commands (`delphi xenia admin add|remove|rotate|show|sign|publish|refresh|genesis`, #248) sign only after a person types a challenge over the exact bytes at a terminal. The console can answer `sign`, `publish`, `refresh` and a proposal for a diff that touches no admin, one card at a time, five an hour, only with `[xenia] console_admin = true` in `operator/xenia.toml` (default off, shown by `delphi doctor`); admin changes, rotations, genesis and rewrite recovery stay in the terminal and the console says which command to run. `delphi xenia sync --accept-rewritten-head` recovers from an admin-approved history rewrite (terminal only: it lists every affected file and wants the commit and the count typed).
- **Signed releases** and the start-time refresh of built-in files; `[update] require_signed` (spec 036, #213, #216). Inert until the maintainers make a release key.
- **Layered learning and training mode** (spec 031): staged notes from the mission log and `delphi learn` (#196, #203), `delphi train validate|plan|run` (#186, #202).
- **Training experiments** (spec 037): research and code cases, truth packs, arms, a blinded jury, operator review and a report (#215, #217, #218, #223, #226, #230).
- **Change log and monitoring** (specs 032-034, 037): a `record_change` tool, `delphi changelog`, `delphi sessions`, `delphi tools`, narrator panes for pulse, timeline and tools, live usage for subscription seats, a Prometheus export (#187, #192, #195, #205, #204).
- **Console sessions** (spec 030): `/new`, `/context`, `/export all`, the dashboard view (#182, #185, #199).
- **Settings by prompt, Council on demand, `/follow`**: a Council can be raised with `--tier` and never lowered to save cost, and ends in a decision and next steps that `/follow` starts (#228).
- **Epic threads** (spec 005g): orchestrator-named threads and the critical path in the epic pane; tokens by thread (#168, #222, #225).
- **Knowledge window and `delphi kb serve`**, a read-only search API (#171, #188).
- **Documents and data visualization** (spec 014): sheets, more chart types, interactive charts, PNG output, diagrams, decks and reports (#158-#160, #163, #167, #169, #172-#174).
- **`agy` as a Builder**, gated by a `PreToolUse` hook, with a conformance probe (#170, #175, #177, #189). **Ollama** as a keyless local fallback endpoint (#156).
- **Interface:** `/observe` opens the narrator as one window (#159, #161); `@tags` route by role ID or theme name (#178); `delphi status` and `delphi missions` (#150).

### Changed

- Equal tiers fail over silently, no role starts on the top tier, and panes show the model that actually served (#220). Failover saves first and stays within the family (spec 035, #206).
- The console deck spec was renumbered 037 (#216); there are now two specs each numbered 014 and 037 (see the README).

### Fixed

- Timing flakes in the console tests (#152, #193, #154, #231); a price check that stopped the Ollama fallback from ever serving a call (#199); a bundle tier bug found by B4b (#183).

## 0.2.7 — 2026-10-25

Not itemized; see the releases page.

## 0.2.6 — 2026-10-25

Claude seats get Delphi's tools over the MCP bridge (spec 020), and the first eight slices of the console redesign (spec 005e) ship. Not otherwise itemized; see the releases page.

## 0.2.1 – 0.2.5 — 2026-10-24 and 2026-10-25

Not itemized; see the releases page.

## 0.1.0 — 2026-10-23

The first tagged build.
