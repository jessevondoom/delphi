# The desktop app

A window on what your Delphi team is doing: who is working, a council as it deliberates, the knowledge base, and what was produced. Until you pair it from a terminal it only reads. Paired, it can also ask for four things you could start at a terminal without being asked anything: a research topic continued, a follow-up question researched, a topic page rebuilt, a statement disputed (below). It cannot approve anything, change a setting, edit an agent or a skill, stop work or start any other, and it never starts a daemon. For any of those, use the console (`delphi chat`). The design and its rules are in [`specs/111`](../../specs/111-the-desktop-app-a-read-only-window-on-the-daemon.md).

## Install it

The one-line installer is the way to get the app. It writes files that macOS puts no quarantine flag on, so the app opens without a warning:

```sh
curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh
```

On macOS a release carries the app ([`specs/122`](../../specs/122-the-desktop-app-in-the-release-built-on-macos-installed-and-updated-with-the-program.md); releases made before that spec do not). It is in the same archive as `delphi`, so it passes the same checksum and attestation:

- **`install.sh`** puts it at `~/Applications/Delphi.app` (`--no-app`, or `DELPHI_NO_APP=1`, leaves it out).
- **`delphi update`** replaces it there with the release's, keeps the one it replaced as `~/Applications/.delphi-app.prev`, and `delphi update --rollback` puts that back. It installs the app there if it is missing; `DELPHI_NO_APP=1` leaves the app alone. A running window keeps running; the next start is the new build.
- **Homebrew** keeps it inside the formula; `brew info delphi` gives the one line that links it into `~/Applications`. `install.sh` and `delphi update` leave such a link alone.

Nothing installs or updates it on Linux yet: build it yourself (below). If `/Applications/Delphi.app` is also there, `delphi app` starts that one first; remove it, or set `DELPHI_APP`.

### Not wanting it: the opt-out

`DELPHI_NO_APP=1` has to be set each time. To keep the app off for good, run:

```sh
delphi app opt-out     # writes <home>/app/opt-out, an empty file (mode 0600) in a folder of mode 0700
delphi app opt-in      # removes it
```

While the file exists (`$DELPHI_HOME/app/opt-out`, `~/.delphi/app/opt-out` by default), `install.sh` and `delphi update` leave the app alone and say so in one line. An app that is already installed is not removed. Neither command needs a terminal; both are refused inside an agent seat. `delphi doctor` (check `24 app`, macOS only) reports the opt-out.

### The program inside the app

The app carries its own copy of `delphi` in `Delphi.app/Contents/Resources/delphi`, so it is never a dead end ([`specs/122`](../../specs/122-the-desktop-app-in-the-release-built-on-macos-installed-and-updated-with-the-program.md) §10). The app looks for your installed `delphi` first and uses its own copy only when it finds none. With none installed, its empty state says "Install the delphi command" and shows a command to paste into a terminal:

```sh
"/Applications/Delphi.app/Contents/Resources/delphi" self-install
```

`delphi self-install` copies the program that runs it to `~/.local/bin/delphi` (or `$DELPHI_INSTALL_DIR/delphi`), as `install.sh` does: it writes under a temporary name, sets mode 0755 and moves the file into place, so a reader sees the old program or the new one. It never writes over a symbolic link, or over a file that is not a `delphi` (it reads the file and starts nothing, so it takes a regular program that holds Delphi's own text for one; move anything else away first). It prints what it did and, if the folder is not on your `PATH`, the line to add. It needs no terminal and is refused inside an agent seat. The app copies nothing itself, and `delphi update` replaces the whole bundle, so the bundled copy always matches the app.

### The disk image (arrives with the disk image)

A later release adds a disk image, `Delphi-vX-<target>.dmg`, as a fallback for a machine where you do not want to run the installer. This section describes it ahead of that release; until then there is no image to download.

There is no Apple Developer ID for Delphi yet, so the image is not notarized. A disk image downloaded in a browser, and the app in it, carry macOS's quarantine flag, and macOS will not open the app the first time:

1. Open the image and drag `Delphi.app` onto the `Applications` link.
2. Try to open `Delphi.app`; macOS says it cannot verify it. Close that message.
3. Open **System Settings**, then **Privacy & Security**, scroll to the message about Delphi and choose **Open Anyway**.

Or remove the flag in a terminal:

```sh
xattr -dr com.apple.quarantine /Applications/Delphi.app
```

`install.sh` and `delphi update` never read the image. `delphi doctor` says when the app carries the flag.

## Build it

To build it yourself, you need Node 22.12 or newer, Rust with `cargo` on your `PATH`, `python3`, and Chrome or Chromium (for the diagram check; `CHROME=<path>` names one). macOS only for now.

```sh
app/dev/build.sh
```

That installs the window's pinned dependencies without running their install scripts, runs every check (the dependency gate, the type check, the tests of the window and of the bridge, a check in headless Chrome that hostile Mermaid diagrams run nothing, clippy), builds `Delphi.app`, and then starts the built app once against a stand-in daemon to confirm that it attaches read-only and that its window asks for its data. A window opens for a few seconds during that last step; `app/dev/build.sh --no-smoke` skips it.

The app is at `target/app/release/bundle/macos/Delphi.app`. Copy it where you keep applications.

It is not signed with a developer certificate. An app you build yourself, or one `install.sh` or `delphi update` wrote, opens normally. One downloaded in a browser or copied from another machine is stopped by macOS the first time (see the disk image section above for how to open it).

## What it needs

- **A running daemon.** Start Delphi in a terminal (`delphi chat`). The app attaches by itself and attaches again if the daemon restarts. With no daemon it says so and waits.
- **Your `delphi`.** The app asks it where the daemon is (`delphi daemon where`). It looks in `~/.local/bin`, `/opt/homebrew/bin` and `/usr/local/bin`, then in its own bundle; set `DELPHI_EXE` to the full path if yours is elsewhere. `DELPHI_HOME` and `DELPHI_WORKSPACE` are honoured (the workspace is `default` otherwise).

## What it shows

| Screen | What is there |
| :-- | :-- |
| Activity | What agents said, read and recorded, newest first, each in its role's colour; the agents and what each is doing; what is waiting on you |
| Councils | The question, the stages, each seat's lens, position and critique; the council's activity while it runs; the verdict and what was left unresolved once it is in. In the blind round a seat's words are not shown |
| Encyclopedia | A page for each topic the workspace knows about, generated from cited records and marked as such: topics by letter or by project, what is new or changed since you read it, what is due for a rebuild, what has open questions. On a page a citation marker opens a panel with the claim, its status and its anchors, and an anchor opens the source at the quoted passage, highlighted. Under the page: its bibliography, where to start, related pages, what links to it, open questions and earlier versions. Also a page for a source, for a claim and for a research topic, and a dated list of changes. Needs a paired app (below) |
| Knowledge base | Search, the collections and their sizes, what changed last; results with their labels; a reader with a document's provenance |
| Agent memory | For each role: its ratified lessons, its notes not yet reviewed (with how often each was used and when it expires), its findings, its recent episodes, and "learned lately", one dated list of all four; and the same for every role by date. You are shown all of it. Needs a paired app |
| Work output | What the team produced, grouped by what is there: mission documents, research, context, recorded changes, builds |
| Agents and skills | Each role's manifest and each skill |
| Settings | The window's theme and menu; the daemon's approval mode and budget, to read |

An agent is always written the same way: its chip, its name in bold in its colour, then its role in grey. Names and colours are the ones your `delphi` sends, so they follow your theme; a `delphi` older than this app sends role ids as names, and the role is then written once. Documents and replies are drawn as Markdown, with tables, task lists and pictures; a picture or a link to a deck opens outside the window.

## What it can and cannot reach

- It attaches to the daemon's read-only socket (the narrator scope). Once you have paired it (below), it attaches to the daemon's app socket instead, proving its key each time. It attaches to no other socket and is never given a console token.
- It can call nine of the daemon's read methods, and six reads of the knowledge base through one of them. A paired app can also call the encyclopedia's eleven reads (`pages.list`, `pages.get`, `pages.versions`, `pages.links`, `topics.list`, `topics.get`, `library.source`, `library.claim`, `memory.roles`, `memory.role`, `changes.since`; [`specs/117`](../../specs/117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §3), each proved by a test of the daemon to write nothing. Those eleven are never sent on the read-only socket, because one of them returns the text of your private sources: the app refuses them itself until it has proved its key. The list is fixed in the app and checked against the daemon's own lists by a test.
- It opens no network port and loads nothing from the network.
- It writes nothing under your projects, and under your Delphi home only its own folder, `app/`, readable by you alone: its key (`app/key.json`) once paired; your marks on the encyclopedia (`app/marks.json`), written when you read a page to its end, pin a topic or write a note; and a page you export (`app/exports/<topic>.md`). None of these is sent to the daemon and no agent is given them. Its theme and menu choices are kept by the window.
- A page offers "Ask a follow-up", "Research this section further", "Rebuild now" and "Dispute this statement". In a window whose key holds `request` each is a button that asks the daemon, and the line under it says where the request stands: queued, running, done, refused and why, and that a decision is waiting on you in the console when one is. In a window that is not paired, or whose key holds only `read`, each composes the command instead (`delphi research continue …`, `delphi pages rebuild …`, `delphi pages dispute …`), puts it on your clipboard and says how to pair; what you type is quoted so that a shell reads it as one argument.
- The top of the window says which it is: `read-only`, or `paired as app-1a2b3c` with the grants the key holds. When the app has a key the daemon would not take, it says why beside `read-only`.
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
delphi app opt-out | opt-in              # keep install.sh and delphi update from installing the app, or undo that
```

- Every one of these is run by you, in a terminal (`opt-out` and `opt-in` need none). They refuse when their input is not a terminal and inside an agent seat, and the daemon holds the same line: it gives a pairing ticket, revokes and grants only for a client attached to a terminal.
- `delphi app` asks the daemon for a ticket (good once, for 60 seconds) and starts the app with it on a pipe. Nothing is asked of you. It prints the key's short name (`app-1a2b3c`) and its grants. It starts the app named by `DELPHI_APP` (the executable, or a `Delphi.app`); otherwise the build beside your `delphi` in a source checkout (`target/app/release/…`), then `/Applications/Delphi.app`, then `~/Applications/Delphi.app`. It never starts a program found under the folder you happen to be in.
- A key is recorded in `operator/app-keys.json` under your Delphi home, which no agent can read or write. A key unused for 180 days is removed.
- The grants are `read` (what the app reads today) and `request`. A key gets both when it is paired, and gets a new grant only when you give it: an update of the app or of Delphi never adds one.

### What a paired app may ask for

A key that holds `request` may ask the daemon to start four things, each of which you could start at a terminal without being asked anything:

| The app asks | The same as | It sends |
| :-- | :-- | :-- |
| `app.research_continue` | `delphi research continue <topic> [--note …]` | the topic's id; a section's anchor; a note you typed |
| `app.research_ask` | `/research <question>` in the console | the question you typed; the id of the topic it follows |
| `app.page_rebuild` | `delphi pages rebuild <slug>` | the page's slug |
| `app.page_dispute` | `delphi pages dispute <claim> --note …` | the claim's id; a note you typed |

- **In the window:** "Ask a follow-up" sends your question and the topic it follows; "Research this section further" sends the topic, the section's anchor and what you typed, never the section's heading or text; "Rebuild now" sends the page's slug; "Dispute this statement" sends the claim's id and your words.
- **Ids and what you typed, nothing else.** A request that carries any other parameter is refused. The daemon looks up what an id names; text from a page, a source or an agent cannot be put in a request. A section is named by its heading's anchor (lowercase letters, digits and hyphens), and only when a page that follows the topic has that heading.
- **The daemon's own rules still hold.** A request runs as yours, under the same budgets and limits as the command it stands for: one research run from the console or the app at a time, a note of at most 120 characters, a question of at most 200. Anything that needs your approval asks you in the console; no request answers it, and the app cannot.
- **Limits.** 30 requests an hour for a key and 4 running at once; past either the request is refused and says which. The hour's count is kept in the daemon's memory and starts again when the daemon does.
- **The record.** Each request is an `app.request` event with the key's name, the method, the ids and where it stands (queued, running, done, failed, cancelled, refused and why). What you typed is not in it. `delphi app log` prints them.
- **Revoking.** `delphi app revoke <name>` closes the key's sessions at once. A request that was already being started is not cut halfway: it finishes starting, and its work runs under the daemon's usual limits.

## Several daemons: the start screen and the switcher

Each daemon writes a small presence file beside its sockets (`<home>/run/<hash>.json`, [`specs/130`](../../specs/130-the-desktop-app-finds-every-daemon-and-says-what-each-is-doing.md)), and the app reads them. A file is believed only when the socket it names answers as the daemon that wrote it; a crash leaves a file that the app shows as stopped, and a daemon from before the files is shown as an older daemon.

- **One daemon runs**, or `DELPHI_WORKSPACE` names one: the window attaches, as always.
- **Several run and none is named**: the window starts on a screen with one card per daemon: the workspace, its path, the version, a state word (`needs you`, `working`, `idle`, `stopped`, `older daemon`), and counts (agents running or waiting, approvals waiting and for how long, the session, the tokens used; never money). A card that needs you comes first and says "Answer it in the console": the window answers nothing. Click a card to show that daemon.
- **A stopped workspace** has the command to start it (`delphi --workspace <name>`) and a button that copies it. Nothing in the window starts a daemon.
- **A daemon of another version than the app** shows both versions, and says that `delphi update` restarts idle daemons.
- **None runs**: "Nothing is running", with the command to start Delphi and, for a machine with no Delphi yet, the install line.
- **With several daemons**, the workspace name in the title row is a button that opens the same cards in a panel, to switch. With one it is plain text.

The counts come from `observe.state`, the read the window already makes, over a short-lived read-only connection to each daemon, at most once every five seconds and only while this screen or the panel is showing. A key paired with one daemon is not offered to another: after a switch the window reads on the narrator socket until `delphi app` has paired it with that daemon too. In a browser, `npm run dev` shows sample daemons (`?daemons=none`, `?daemons=one` or the default several).

## Your marks on the encyclopedia

- **Read.** A page version is marked read when you scroll to its end. The home page lists a page as *new* until then and as *changed* when the daemon has rebuilt it since.
- **Notes.** "Add a note" beside a passage keeps a note in the margin. A note stays with its passage through later versions of the page for as long as the passage's words are kept (the citations beside them may change); when the passage is gone the note is listed under the page as orphaned, with the words it was attached to.
- **Pins.** A pinned topic is listed first on the home page.
- **Export.** "Export as Markdown" writes the page as the daemon stored it, banner first and bibliography last, to `app/exports/` under your Delphi home and shows it in the Finder. "Print or save as PDF" opens the system's print dialog. A page with no "Generated page. Not authoritative." banner is not exported.

A source the library keeps privately for retrieval ([`specs/119`](../../specs/119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) §3) is shown in the window only as the passage a claim cites; any other source, a private one included, is shown whole, labelled private.

### Where the app keeps its key

The app's private key never leaves your machine and the daemon never sees it. It is the file named above, `app/key.json` under your Delphi home (mode 0600, in a folder of mode 0700), on every system.

The keychain is not used while the app is unsigned (spec 120 §12). macOS knows an unsigned program by its exact code, so every rebuild or update of the app would be a stranger to its own keychain item and would need you to allow it in a system dialog, or to pair it again. This was checked on macOS 27.0.1 with two builds of one program; the details are in [`app/README.md`](../../app/README.md). Once the app is signed with a stable identity, the keychain will be used again.

- **What the file costs:** any program running as you can read the key and use it with its grants, `read` and `request`. A request only asks; anything that needs your approval still asks you. The keychain would have stopped such a program; nothing in this design defends against software already running as you, which can also type at your terminal.
- **Moving from the keychain:** a key an earlier build kept in your login keychain (`dev.delphi.desktop.app-key`) is moved to the file the first time the app starts, and the keychain item is removed. If macOS will not let the new build read it, the app reads without its key and says so; run `delphi app` again to pair it.

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
