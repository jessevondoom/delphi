# Getting started

About ten minutes from nothing to your first reviewed change. macOS (Apple silicon or Intel) and Linux (x86_64 or aarch64) are supported; Windows is not.

## 1. Install

```sh
curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh
```

The installer checks the download against its SHA-256 and the release's build attestation, so it needs the `gh` CLI signed in. Without `gh` it stops, unless you set `DELPHI_SKIP_ATTESTATION=1` to accept the checksum alone. It puts `delphi` and `delphi-ui` in `~/.local/bin`; make sure that directory is on your `PATH`. Homebrew works too: `brew tap jessevondoom/delphi https://github.com/jessevondoom/delphi && brew install delphi`.

Check it: `delphi --version`. To build from source instead, see the README's [Install](../README.md#install).

## 2. Open the console

```sh
delphi
```

You can run Delphi from any directory. `cd` into a Git project first if you want to use it as the project for Git-backed build workflows; a Git repository is not required to open the console.

The first time, Delphi:

1. creates a workspace for the project under `~/.delphi/workspaces/` (your project repository is never written to);
2. looks for model access. If it finds none it runs the guided setup, which detects your `claude` and `agy` programs, asks only for what it cannot find, and ratifies the result. See [Model access](guides/model-access.md);
3. runs the doctor and offers to fix what it can;
4. opens the console.

Outside a git repository it uses a `default` workspace. Run `delphi setup` later if you deferred model setup.

## 3. Ask, then build

Type at the `›` prompt. Paths are logical: `@project/<name>/…` for your repository, `specs/…` and `context/…` for the workspace's own files.

```
› What does @project/my-project/src/lib.rs do?
› @architect draft a spec for the retry ladder
› /build implement specs/011-retry.md in @project/my-project
```

`/build` is where Delphi earns its keep: a throwaway worktree, your tests and checks as merge gates, a second agent auditing the exact commit, and a card asking you to merge. [Builds and Councils](guides/builds-and-councils.md) walks through one.

Delphi builds against an approved spec ("No Spec, No Build"). If you have none, ask the architect to draft one first, read it, and approve it.

## 4. Answer the cards

Anything beyond reading asks you first:

| Key | Meaning |
| :-- | :-- |
| `/y` | approve once |
| `/a` | approve similar actions for this task (Amber cards only) |
| `/n` | reject |
| `/d` | show the card's full payload |

Red cards (outbound calls, merges, new dependencies, destructive commands) always ask, and an unanswered card is a no after `[hitl] approval_timeout_sec` (600 seconds by default). `?` on an empty prompt lists the keys.

## 5. Look around, and keep it healthy

| To | Run |
| :-- | :-- |
| see the live team in a second window | `/observe` in the console, or `delphi observe` |
| see the workspace, daemon, models and mission at a glance | `delphi status` |
| check the workspace and fix what is fixable | `delphi doctor --fix` |
| see which models are usable, exhausted or cooling down | `delphi models` |
| see your spend | `/usage` in the console |
| reopen an earlier mission | `delphi missions`, then `delphi chat --mission <id>` |
| update | `delphi update` (`--check` first if you like) |

## Where things live

Everything is under `$DELPHI_HOME` (default `~/.delphi`): `workspaces/<name>/config.toml`, the mission logs, the knowledge index, and your agent memory. Delphi never writes into the project itself. When you try commands in a scratch setting, always pass `--home <tmpdir>`: bare `delphi` and `delphi init` create a real workspace.

## Next

- [Model access](guides/model-access.md) to add or change what Delphi can call.
- [Headless runs and CI](guides/headless-and-ci.md) to use it from a script.
- [Troubleshooting](troubleshooting.md) when something is off.
