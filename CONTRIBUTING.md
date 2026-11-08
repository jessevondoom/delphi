# Contributing

Delphi is built mostly by its own agents under one operator, and human contributions follow the same rules the agents do. The charter is [`GOVERNANCE.md`](GOVERNANCE.md), the review rubric is [`REVIEW.md`](REVIEW.md), and the specs in [`specs/`](specs/) say what to build.

## The rules that matter most

- **No Spec, No Build.** Build only against a spec whose `status` is `approved`, and name it in the commit: `Implements-Spec: specs/NNN-*.md`. If no spec covers your idea, propose one first (a draft in `specs/`) and get it approved. The rule covers software only; documents, decks, images and media need no spec but are logged by prompt, and Markdown notes and scaffolds are regular operation (`specs/070`).
- **One slice per pull request**, and update the spec tracker (`missions/2026-10-23-foundational-design/spec-tracker.md`) in the same PR that changes a spec's state.
- **A format is a spec.** If you change a file format, change its schema in [`schemas/`](schemas/) and its spec together.
- **Agents are data.** Role behavior lives in `agents/<role>/AGENT.md`, `skills/*/SKILL.md` and the charter, never in a theme. Use role IDs in specs, config and manifests; display names appear only under `themes/` (the repo lint refuses them elsewhere).
- **Nothing consequential without a card.** Do not add a code path that acts outside the sandbox, reads outside its scope, or approves its own action. Red actions always ask.

## Build and test

```sh
cargo test --workspace --locked                      # everything; e2e console tests need ui/node_modules, else they skip
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -q --locked -p delphi -- lint --repo .     # this repository's own Delphi files
cargo run -q --locked -p delphi -- bundle validate dev/bundles/acme-platform --claude
python3 -I dev/bench/bench.py self-test
python3 -I dev/train/avail.py self-test

cd ui
npm ci --ignore-scripts && npm run check-deps        # pinned dependencies, no install scripts
npm run typecheck && npm test
```

CI runs the same on macOS and Ubuntu, plus the UI suite and pty end-to-end tests against the real daemon. Gates must pass before merging; do not merge past a failing one.

### Documentation is tested

- `docs/reference/cli.md` and `docs/reference/delphi.1` are generated from the command table behind `--help` in `crates/delphi/src/main.rs`. After you change a command, regenerate them and commit the result:
  ```sh
  cargo run -q -p delphi -- docs cli > docs/reference/cli.md
  cargo run -q -p delphi -- docs man > docs/reference/delphi.1
  ```
  `cargo test -p delphi --test docs` fails with this instruction when they are stale.
- The same test validates `config.example.toml`, the golden bundle, every `AGENT.md` and every `SKILL.md` against `schemas/`.
- User-facing behavior changes belong in [`CHANGELOG.md`](CHANGELOG.md) under Unreleased.

## Working in a scratch setting

Bare `delphi` and `delphi init` create a real workspace. Whenever you run the binary to try something, pass `--home <tmpdir>` (or set `DELPHI_HOME`). `delphi help <command>` and `<command> --help` create nothing.

Tests that start daemons hold pseudo-terminals until the process ends; if `openpty` starts failing, stop strays with `delphi daemon stop --home <home>` and check for left-over `delphi __core` processes. Builds fill disks quickly: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0` helps. Do not symlink `ui/node_modules` into a worktree (removing the worktree can empty the original); copy it instead (`cp -Rc` on macOS).

## Pull requests

Describe what changed and why, name the spec, and say how you checked it. If a step was skipped or a test is flaky, say so plainly. Releases are cut by the maintainers: bump the version in `crates/delphi/Cargo.toml` in its own PR, then tag `vX.Y.Z` on `main`; the release workflow builds, attests and publishes (running it by hand is a dry run).

Words for approvals are fixed: **approve** a spec, **ratify** a proposal (`/ratify`, `/retro`; an end user's workspace), **accept** an update (`delphi update`), **answer** a decision (the decisions pane), **merge** a PR (`delphi merge`). The flow for merging core changes, and what is built so far, is in [Merging core changes](docs/guides/merging-core-changes.md) (spec 094).

## Folding spec amendments

When a spec's amendments are folded into its body, move the removed amendment text verbatim to `specs/history/NNN-<slug>.md` (non-normative). Keep every section number: a folded-away section stays as a stub heading, `## N. (folded into §X.Y; was NNN-An)`, and each amendment ID survives as an inline `(was NNN-An)` anchor. `cargo test -p delphi --test spec_refs` fails when a reference (`specs/NNN §X.Y`, `NNN §X`, `NNN-An`) in specs, docs, code comments, the console, schemas, the changelog or mission notes stops resolving. It has a small allowlist of references that were already broken; fix those rather than add to it.

## Spec numbers

A spec number names one spec. Before you create a spec file, run `ls specs` and `gh pr list` and take the next number nobody has; `delphi lint` fails a pull request that adds a second file with a number another spec has, with no allowlist, so if two sessions pick the same number the later pull request gives its new, unmerged spec the next free one.

Three numbers used to be shared. On 2026-11-08 (spec 090) one spec of each pair was renumbered, so a citation of 014, 037 or 061 written before that date may mean either spec of its pair, and commit trailers and merged pull-request text keep the old numbers:

| Number | Was (file name then) | Now | Kept the old number |
| :-- | :-- | :-- | :-- |
| 014 | `014-documents-and-dataviz.md` | `095-documents-and-dataviz.md` | `014-relay-and-away-mode.md` |
| 037 | `037-console-deck-change-log.md` | `096-console-deck-change-log.md` | `037-training-experiments-quality-and-efficiency.md` |
| 061 | `061-encyclopedia-search-pages.md` | `097-encyclopedia-search-pages.md` | `061-shell-tool.md` |

To tell which a pre-2026-11-08 "spec 014" meant: the documents readers, charts and `delphi-docs` are 095; the relay, away mode and step-up are 014. The console's left deck is 096; training experiments, arms, truth packs and the harness for measuring models are 037. Encyclopedia pages, their cache lifetime and inline media are 097; the `shell` tool and the `!` bang are 061.

## Compiled specs

`delphi specs compile` (spec 088) writes one JSON file per spec to `compiled/specs/data/`: a quotation of the spec's summary and `status:` line with byte spans, a hash of the source, and its outline, for agents to read. It is a derived view and never the authority; the source spec governs, and a builder opens the section it implements.

- **After you edit a spec, run `delphi specs compile` and commit the result.** `cargo test -p delphi --test compiled_specs` fails with the command to run when a compiled file is stale. The files are per spec, so two pull requests conflict only when they edit the same spec; if one does, rerun the command, never edit a compiled file by hand.
- **A new spec adds one line to `compiled/groups.toml`** (its file name without `.md`, and one of the nine groups). That file is the only place a spec's group is declared; a `group:` line in a spec's frontmatter is ignored and `delphi lint` warns about it. The compile fails when a spec is missing from the file or the file names a spec that does not exist.
- Do not read a compiled file to decide anything in code. The test above fails if any Rust or console source other than the compiler names the directory.

## Security

Report a vulnerability privately to the repository owner (a GitHub security advisory, if enabled, or a direct message) rather than in a public issue. Never include credentials, tokens or customer data in an issue, a test fixture or a spec.
