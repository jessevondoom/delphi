# The desktop app

A window on what your Delphi team is doing: who is working, a council as it deliberates, the knowledge base, and what was produced. It only reads. It cannot approve anything, change a setting, edit an agent or a skill, or start or stop work, and it never starts a daemon. For any of those, use the console (`delphi chat`). The design and its rules are in [`specs/111`](../../specs/111-the-desktop-app-a-read-only-window-on-the-daemon.md).

It is not in a release yet. You build it from this repository.

## Build it

You need Node 22.12 or newer, Rust with `cargo` on your `PATH`, `python3`, and Chrome or Chromium (for the diagram check; `CHROME=<path>` names one). macOS only for now.

```sh
app/dev/build.sh
```

That installs the window's pinned dependencies without running their install scripts, runs every check (the dependency gate, the type check, the tests of the window and of the bridge, a check in headless Chrome that hostile Mermaid diagrams run nothing, clippy), builds `Delphi.app`, and then starts the built app once against a stand-in daemon to confirm that it attaches read-only and that its window asks for its data. A window opens for a few seconds during that last step; `app/dev/build.sh --no-smoke` skips it.

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

- It attaches to the daemon's read-only socket (the narrator scope). Once you have paired it (below), it attaches to the daemon's app socket instead, proving its key each time. It attaches to no other socket and is never given a console token.
- It can call nine of the daemon's read methods, and six reads of the knowledge base through one of them. The list is fixed in the app and checked against the daemon's own allowlist by a test. Pairing does not add to it.
- It opens no network port and loads nothing from the network.
- It writes nothing under your Delphi home or your projects, with one exception: on a system with no keychain, a paired app keeps its key in `app/key.json` under your Delphi home, readable by you alone. Its theme and menu choices are kept by the window.
- Outside its window it does three things, each on your click: **Show in Finder**, **Open** a deck or page, a document, a picture or a recording in your browser or viewer (never a program or a script), and open a web link in your browser. A file must be inside the workspace or one of your registered projects.

## Pairing it with a terminal

The rules are in [`specs/120`](../../specs/120-the-app-scope-a-key-paired-from-a-terminal-and-grants-that-widen-one-at-a-time.md). An app that is not paired reads, as described above. A paired app holds a key of its own, proves it to the daemon on every connection, and may then call the methods of the grants its key holds, and no others.

```sh
delphi app                               # start the app from a terminal; that is what vouches for it
delphi app list                          # the paired keys, their grants and when each was last used
delphi app grant <name> <grant>          # give a key a grant
delphi app grant <name> --remove <grant> # take one away; the key's open sessions are closed
delphi app revoke <name>                 # remove a key; its open sessions are closed at once
delphi app log                           # what paired, what was refused, what was revoked or granted
```

- Every one of these is run by you, in a terminal. They refuse when their input is not a terminal and inside an agent seat, and the daemon holds the same line: it gives a pairing ticket, revokes and grants only for a client attached to a terminal.
- `delphi app` asks the daemon for a ticket (good once, for 60 seconds) and starts the app with it on a pipe. Nothing is asked of you. It prints the key's short name (`app-1a2b3c`) and its grants. It starts the app named by `DELPHI_APP` (the executable, or a `Delphi.app`); otherwise the build beside your `delphi` in a source checkout (`target/app/release/…`), then `/Applications/Delphi.app`, then `~/Applications/Delphi.app`. It never starts a program found under the folder you happen to be in.
- A key is recorded in `operator/app-keys.json` under your Delphi home, which no agent can read or write. A key unused for 180 days is removed.
- The grants are `read` (what the app reads today) and `request`. A key gets both when it is paired, and gets a new grant only when you give it: an update of the app or of Delphi never adds one.

Not built yet: `delphi app` does not start the app in this build (it says so; the place where it starts a process must first be added to the list of such places), and the `request` grant has no methods, so a paired app can do nothing an unpaired one cannot. The window does not yet say whether it is paired.

### Where the app keeps its key, and what an unsigned app gets from the keychain

The app's private key never leaves your machine and the daemon never sees it. On macOS it is one item in your login keychain (`dev.delphi.desktop.app-key`); on a system with no keychain it is the file named above.

The app is not signed, and that changes what the keychain does for it. This was checked on macOS 27.0.1 with two builds of one program; the details and what was not checked are in [`app/README.md`](../../app/README.md).

- **Another program cannot read the key without asking you.** That includes another build of this app: macOS knows an unsigned program by its exact code, not by its name or where it is.
- **So after you rebuild or update the app, it cannot read its own key.** macOS is expected to ask you to allow the new build (this dialog was not checked). If you allow it always, that build is not asked again. If you deny it, the app reads without its key and says so; delete the item in Keychain Access and run `delphi app` again to pair the new build. A signed app would keep its key across updates.
- **Another program running as you can write over the key, though not read it.** The app then has no usable key and reads, until you pair it again. Nothing in this design defends against software already running as you; it can also type at your terminal.

If a key is lost or you no longer trust it, `delphi app revoke <name>` removes it on the daemon's side, which is the side that counts.

## Looking at it without the app

The window is a web page. `npm run dev` in `app/` serves it with sample data at `http://localhost:5199`; add `#council` (or any screen's name) to open that screen, and `?q=some+words#knowledge` to run a search. To look at your own daemon's data in a browser, `cargo run --example probe -- --json > dist/real.json` in `app/src-tauri` and open `?data=./real.json` on a preview of the built page. That file holds your agents' text; delete it afterwards, and never leave it in `dist/` when you build the app.

## When something is wrong

| You see | Do this |
| :-- | :-- |
| "No daemon is running" | Start `delphi chat` in a terminal |
| "No Delphi home was found" | Run `delphi` once to set it up, or set `DELPHI_HOME` |
| Role ids where you expect your theme's names | Your `delphi` predates the names in `daemon where`; update it |
| A screen says the knowledge base answered an error | `delphi doctor` in a terminal says whether the index is healthy |
