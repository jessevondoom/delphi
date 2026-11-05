# Getting started

About ten minutes from nothing to your first reviewed change. macOS (Apple silicon or Intel) and Linux (x86_64 or aarch64) are supported; Windows is not.

## 1. Install

```sh
gh release download --repo jessevondoom/delphi --pattern install.sh --output - | sh
```

The installer checks the download against its SHA-256 and the release's build attestation, so it needs the `gh` CLI signed in (`gh auth login`, or `GH_TOKEN`; this is also what lets it read a private repository, with the token authorized for an organization that uses single sign-on). Where the repository has no attestations, it checks the maintainers' release signature carried by the build instead. Without `gh` it stops, unless you set `DELPHI_SKIP_ATTESTATION=1` to accept the checksum alone. (A public repository also installs anonymously: `curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh`.) It puts `delphi` and `delphi-ui` in `~/.local/bin`; make sure that directory is on your `PATH`. Homebrew works too, while the repository is public: `brew tap jessevondoom/delphi https://github.com/jessevondoom/delphi && brew install delphi`.

Check it: `delphi --version`. Homebrew works too: `brew tap jessevondoom/delphi https://github.com/jessevondoom/delphi && brew install delphi`. `install.sh` also takes `--dir` (or `DELPHI_INSTALL_DIR`), `--version vX.Y.Z` and `--no-setup`.

**Updating.** `delphi update` fetches and verifies the newest release, replaces both executables, refreshes the built-in agents, skills and themes in your workspaces (as ratification proposals when they differ from yours; `--review` leaves them pending) and restarts any running daemon (`--no-restart` leaves it). `--check` shows what it would do, `--rollback` undoes all three steps, and `--dev` follows the rolling build of `main`. Release builds are signed (`delphi release` says whether yours is, and `delphi doctor` check 2): the first `delphi chat` or `delphi run` after an update ratifies the release's own files itself, your customized files are never touched, and a build whose signature does not hold is refused ([`specs/036`](../specs/036-signed-releases-and-start-time-refresh.md); `--require-signed`, `DELPHI_REQUIRE_SIGNED=1` or `[update] require_signed = true` also refuse an unsigned build; `DELPHI_NO_AUTO_UPDATE=1` turns the refresh off).

**From source** needs Rust 1.85 or newer (the repo pins a toolchain in `rust-toolchain.toml`):

```sh
cargo build --release                      # target/release/delphi (the daemon is `delphi __core`)
cd ui && npm ci && npm run build           # ui/dist/delphi-ui, the full-screen console (needs Bun from devDependencies)
```

Put `ui/dist/delphi-ui` next to `delphi` or point `$DELPHI_UI` at it; without it `delphi` uses the inline console. Release builds also pass `--features embedded-model` to compile the search models in; a plain `cargo build` searches by keyword only unless you install the models under `$DELPHI_HOME/models/` (`dev/fetch-model.sh`).

## 2. Open the console

```sh
delphi
```

You can run Delphi from any directory. `cd` into a Git project first if you want to use it as the project for Git-backed build workflows; a Git repository is not required to open the console.

The first time, Delphi:

1. creates or resolves a workspace under `~/.delphi/workspaces/` (opening the console writes nothing into your directory);
2. looks for model access. If it finds none it runs the guided setup, which detects your `claude` and `agy` programs, asks only for what it cannot find, and ratifies the result. See [Model access](guides/model-access.md);
3. runs the doctor and offers to fix what it can;
4. opens the console.

Outside a git repository it uses a `default` workspace. Run `delphi setup` later if you deferred model setup.

**Projects.** A project is a registered directory, not necessarily a Git repository. `delphi project register` records it and writes only a small `.delphi-project` identity marker; `delphi project init` (asks first, or `--yes`) adds `specs/`, `plans/`, `context/`, `research/`, `work/` and a generated `_index.md`, creating only what is missing. Everything else Delphi remembers stays under `~/.delphi`. Inside a project you see its context and the shared home knowledge; outside any registered project, the default project is active. Type `/projects` to switch: arrow keys and Enter restart the console in that directory as a new session.

## 3. Ask, then build

Type at the `›` prompt. Paths are logical: `@project/<name>/…` for your repository, `specs/…` and `context/…` for the workspace's own files.

```
› What does @project/my-project/src/lib.rs do?
› @architect draft a spec for the retry ladder
› /build implement specs/011-retry.md in @project/my-project
```

`/build` is where Delphi earns its keep: a throwaway worktree, your tests and checks as merge gates, a second agent auditing the exact commit, and a card asking you to merge. [Builds and Councils](guides/builds-and-councils.md) walks through one.

Delphi builds against an approved spec ("No Spec, No Build"). If you have none, ask the architect to draft one first, read it, and approve it.

### Research and the knowledge base

Type `/research <topic>` (or ask in chat and approve the card the agent raises, or use the knowledge window's expand page; `/research status` and `/research cancel` manage it) and the job fetches pages and sorts them by the source's trust class, which code decides, never a model:

- **Default project:** pages from primary or reputable-secondary sources (standards bodies, governments, Wikipedia, peer-reviewed publishers and so on) go into the knowledge base at once. Pages from anything else are held under `local/pending/web/`, labelled weak, and are not searchable until you decide.
- **Registered project:** research is saved in the project's `research/web/` and is never shared automatically. Promote what is worth keeping with `/retro`.

When an agent asks for research, you always get a card first. Once the job ends, the agent is told how it went (how many pages were kept, held or left out, or why it did not run) at the start of its next turn, so it can search what was kept. It never receives page text in that note.
- **Reviewing held pages:** `/validate` lists them; `/validate pass` promotes every page whose source the allow list now trusts (it also runs when a session opens); `/validate ratify <n>` or `discard <n>` decides one at a time. Add `project=<name>` for a project's pages. Ratified pages are searchable but noted as not validated.
- **A nudge:** about once a week, when five or more pages are waiting, the orchestrator says so in the chat.

Weak sources are never treated as corroboration. The fact validator looks a journal page's DOI up at doi.org and counts the page only if it is registered to a publisher; a failed lookup is a no. Not built yet: renaming the word "mission" in commands (new session logs are already stored under `local/sessions/`, but "session" already names a span of a mission, so the words wait for a decision).

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

A **mission** is one piece of work: its own budget, notebook, knowledge and log, kept until you stop caring about it. `delphi chat` opens today's mission or the one you name. `/new` starts a fresh conversation inside the same mission (a "session" in the logs and in `delphi sessions`) without losing the notebook or the knowledge.

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

Everything is under `$DELPHI_HOME` (default `~/.delphi`): `workspaces/<name>/config.toml`, the mission logs, the knowledge index, and your agent memory. Runtime state never goes into a project; a project directory holds only the marker and scaffold you asked for. When you try commands in a scratch setting, always pass `--home <tmpdir>`: bare `delphi` and `delphi init` create a real workspace.

## Next

- [Model access](guides/model-access.md) to add or change what Delphi can call.
- [Headless runs and CI](guides/headless-and-ci.md) to use it from a script.
- [Troubleshooting](troubleshooting.md) when something is off.
