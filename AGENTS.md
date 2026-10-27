# AGENTS.md

Guidance for AI coding agents working in this repository. Humans: read [`CONTRIBUTING.md`](CONTRIBUTING.md); this file is the short, machine-oriented version of the same rules.

## What this is

Delphi: a Rust workspace (`crates/`) plus an Ink/TypeScript console (`ui/`). One executable, `delphi`; its daemon is `delphi __core`. Design lives in `specs/`, the charter in `GOVERNANCE.md`, the rubric in `REVIEW.md`. Start with `README.md`, then `docs/README.md`.

## Rules

- **No Spec, No Build.** Work only against a spec with `status: approved`. Commit trailer: `Implements-Spec: specs/NNN-*.md`. A draft spec is a proposal, not permission.
- **One slice per pull request.** Update the spec tracker (`missions/2026-10-23-foundational-design/spec-tracker.md`) in the PR that changes a spec's state.
- **Never commit past a failing gate.** Run before committing:
  ```sh
  CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo run -q --locked -p delphi -- lint --repo .
  ```
- **Role IDs, not theme names**, outside `themes/` (the lint enforces it). Theme packs change display only, never behavior.
- **Do not widen authority.** No new code path may act outside the sandbox, approve its own action, or let content read from outside raise its own trust. Red actions always ask.
- **Scratch runs need `--home <tmpdir>`.** Bare `delphi` and `delphi init` create a real workspace under `~/.delphi`.

## Where things are

| To change | Look in |
| :-- | :-- |
| a CLI command, its help, the reference | the `CMDS` table in `crates/delphi/src/main.rs`; regenerate `docs/reference/` (below) |
| `config.toml` keys | `crates/delphi/config.example.toml`, `crates/delphi-adapter/src/config.rs`, `crates/delphi-engine/src/boot.rs`; update `schemas/config.schema.json` and `docs/reference/config.md` |
| a role or skill | `agents/<role>/AGENT.md`, `skills/<name>/SKILL.md` (frontmatter is validated by `schemas/`) |
| the bundle format | `crates/delphi-bundle/src/manifest.rs`; `schemas/bundle.schema.json`; specs 023-027 |
| Xenia records and signatures | `crates/delphi-xenia`, `crates/delphi-core/src/xenia_*.rs`; `schemas/xenia-*.schema.json`; specs 011-012 |
| events and handoffs | `crates/delphi-core/src/event.rs`, `crates/delphi-engine/src/handoff.rs`; their schemas |
| the console | `ui/src` (the screen is pure data; golden screens under `ui/test/snapshots*`) |

## Generated files: do not edit by hand

```sh
cargo run -q -p delphi -- docs cli > docs/reference/cli.md
cargo run -q -p delphi -- docs man > docs/reference/delphi.1
```

`cargo test -p delphi --test docs` fails when these, or a schema, drift from the program.

## Machine-readable entry points

- [`llms.txt`](llms.txt): an index of the documentation for language models.
- [`schemas/`](schemas/): JSON Schemas for the file formats ([guide](docs/reference/schemas.md)).
- [`docs/reference/cli.md`](docs/reference/cli.md): every command and option; `delphi help <command>` prints one without creating anything.
- Exit codes of `delphi run` (for scripting): 0 ok, 1 gates failed, 2 card denied, 3 budget, 4 infrastructure, 5 optimistic build awaits review.
