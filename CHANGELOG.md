# Changelog

What changed in each release, newest first. Delphi is pre-1.0: minor versions may change commands and formats, and the notes say so when they do. The `dev` tag is a rolling build of `main` (`delphi update --dev`).

Release builds, with their notes and build attestations, are on the [releases page](https://github.com/jessevondoom/delphi/releases). Entries that cite a pull request (`#nnn`) refer to this repository's pull requests.

This file starts with the work after v0.2.7. For earlier releases, the history is the merged pull requests and the per-spec notes under `specs/`; their entries below give dates, not detail.

## Unreleased

### Added

<<<<<<< HEAD
- **Connector drafts always ask** (spec 010-A2): every draft gets its own card with the payload and "allow similar" never covers one; a draft from a span that read untrusted content, or one naming recipients or invitees, is Red.
=======
- **Harness hook boundary** (specs 018-A1, 021-A1; council 013 D13): a ratified `[harness.claude.settings]` (or agent) `hooks` entry of kind `PreToolUse` or `PermissionRequest` is refused with a notice naming the key, because an allow-capable hook is runtime-only enforceable; observational hooks still work and guards belong to `[[hooks.deny]]`.
>>>>>>> origin/main
- **Excerpts, anchors and bibliographies in the central library** (spec 055 SK2): `delphi sources excerpt add|list` keeps quoted passages with a locator and purpose (4 KiB each; for a restricted source not held whole, 10% of the work or 16 KiB in total) and an anchor with prefix and suffix that is found again when the source changes; `delphi sources bib` writes APA, MLA, Chicago, plain text, CSL-JSON or BibTeX and never a private source. `delphi sources add` now reads Wikipedia, DOI and Gutenberg metadata by code, and `sources show` rechecks a source last checked over 30 days ago through the fetcher.
- **Sources in org bundles** (spec 055 SK0, spec 023 §2.1): an optional `sources/` section with full-text captures and images, gated by licence (only open SPDX ids, `public-domain:<jurisdiction>:<basis>` and `own` travel; `all-rights-reserved`, `unknown` and `private` are refused), with its own size class, `schemas/source-meta.schema.json`, and a sources table in `bundle validate --report`.

### Changed

- **Optimistic is a per-build choice, not an approval mode** (spec 015-A1, council 013): `optimistic` is gone from `/permissions`, `delphi run --permissions` and the `Shift+Tab` cycle; `/build --optimistic` is unchanged. Entering `auto` with `Shift+Tab` now takes a second press, and bulk-approving Amber decisions or cards (`a`, `y`, inbox approve-all) covers only those whose detail was shown at least once (specs 005e-A1, 044-A1).

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
