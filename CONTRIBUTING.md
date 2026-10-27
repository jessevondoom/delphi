# Contributing

Delphi is built mostly by its own agents under one operator, and human contributions follow the same rules the agents do. The charter is [`GOVERNANCE.md`](GOVERNANCE.md), the review rubric is [`REVIEW.md`](REVIEW.md), and the specs in [`specs/`](specs/) say what to build.

## The rules that matter most

- **No Spec, No Build.** Build only against a spec whose `status` is `approved`, and name it in the commit: `Implements-Spec: specs/NNN-*.md`. If no spec covers your idea, propose one first (a draft in `specs/`) and get it approved.
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

## Security

Report a vulnerability privately to the repository owner (a GitHub security advisory, if enabled, or a direct message) rather than in a public issue. Never include credentials, tokens or customer data in an issue, a test fixture or a spec.
