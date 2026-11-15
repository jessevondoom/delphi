# The desktop app

A window on what your Delphi team is doing: who is working, a council as it deliberates, the knowledge base, and what was produced. It only reads. It cannot approve anything, change a setting, edit an agent or a skill, or start or stop work, and it never starts a daemon. For any of those, use the console (`delphi chat`). The design and its rules are in [`specs/111`](../../specs/111-the-desktop-app-a-read-only-window-on-the-daemon.md).

It is not in a release yet. You build it from this repository.

## Build it

You need Node 22.12 or newer, Rust with `cargo` on your `PATH`, and `python3`. macOS only for now.

```sh
app/dev/build.sh
```

That installs the window's pinned dependencies without running their install scripts, runs every check (the dependency gate, the type check, the tests of the window and of the bridge, clippy), builds `Delphi.app`, and then starts the built app once against a stand-in daemon to confirm that it attaches read-only and that its window asks for its data. A window opens for a few seconds during that last step; `app/dev/build.sh --no-smoke` skips it.

The app is at `target/app/release/bundle/macos/Delphi.app`. Copy it where you keep applications.

It is not signed. An app you build yourself opens normally. One copied from another machine is stopped by macOS the first time: open it from the Finder with a right-click and **Open**.

## What it needs

- **A running daemon.** Start Delphi in a terminal (`delphi chat`). The app attaches by itself and attaches again if the daemon restarts. With no daemon it says so and waits.
- **Your `delphi`.** The app asks it where the daemon is (`delphi daemon where`). It looks in `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`; set `DELPHI_EXE` to the full path if yours is elsewhere. `DELPHI_HOME` and `DELPHI_WORKSPACE` are honoured (the workspace is `default` otherwise).

## What it shows

| Screen | What is there |
| :-- | :-- |
| Activity | What agents said, read and recorded, newest first, each in its role's colour; the agents and what each is doing; what is waiting on you |
| Councils | The question, the stages, each seat's lens, position and critique; the council's activity while it runs; the verdict and what was left unresolved once it is in. In the blind round a seat's words are not shown |
| Knowledge base | Search, the collections and their sizes, what changed last; results with their labels; a reader with a document's provenance |
| Work output | What the team produced, grouped by what is there: mission documents, research, context, recorded changes, builds |
| Agents and skills | Each role's manifest and each skill |
| Settings | The window's theme and menu; the daemon's approval mode and budget, to read |

An agent is always written the same way: its chip, its name in bold in its colour, then its role in grey. Names and colours are the ones your `delphi` sends, so they follow your theme; a `delphi` older than this app sends role ids as names, and the role is then written once. Documents and replies are drawn as Markdown, with tables, task lists and pictures; a picture or a link to a deck opens outside the window.

## What it can and cannot reach

- It attaches to the daemon's read-only socket (the narrator scope) and to no other.
- It can call nine of the daemon's read methods, and six reads of the knowledge base through one of them. The list is fixed in the app and checked against the daemon's own allowlist by a test.
- It opens no network port and loads nothing from the network.
- It writes nothing under your Delphi home or your projects. Its theme and menu choices are kept by the window.
- Outside its window it does three things, each on your click: **Show in Finder**, **Open** a deck or page, a document, a picture or a recording in your browser or viewer (never a program or a script), and open a web link in your browser. A file must be inside the workspace or one of your registered projects.

## Looking at it without the app

The window is a web page. `npm run dev` in `app/` serves it with sample data at `http://localhost:5199`; add `#council` (or any screen's name) to open that screen, and `?q=some+words#knowledge` to run a search. To look at your own daemon's data in a browser, `cargo run --example probe -- --json > dist/real.json` in `app/src-tauri` and open `?data=./real.json` on a preview of the built page. That file holds your agents' text; delete it afterwards, and never leave it in `dist/` when you build the app.

## When something is wrong

| You see | Do this |
| :-- | :-- |
| "No daemon is running" | Start `delphi chat` in a terminal |
| "No Delphi home was found" | Run `delphi` once to set it up, or set `DELPHI_HOME` |
| Role ids where you expect your theme's names | Your `delphi` predates the names in `daemon where`; update it |
| A screen says the knowledge base answered an error | `delphi doctor` in a terminal says whether the index is healthy |
