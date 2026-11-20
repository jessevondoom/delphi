# delphi-app: the desktop app (spec 111)

A window on the daemon: read-only until a terminal pairs it, and then able to ask for four things (spec 120). The guide is [`docs/guides/desktop-app.md`](../docs/guides/desktop-app.md); the rules it keeps are in [`specs/111`](../specs/111-the-desktop-app-a-read-only-window-on-the-daemon.md).

```
dev/build.sh        # every check, the unsigned app, and a smoke test of the built app
npm run dev         # the window as a web page with sample data, at http://localhost:5199
npm run check       # dependency gate, type check, tests, build
npm run app         # only the app bundle
```

How it is put together:

- `src/` is the window (TypeScript and React). `bridge.ts` is its only way to the daemon: a few named reads, one way to ask (`request`: one of four requests, made of ids and what was typed) and three kinds of notification, served by the app or, with no app, by a fixture. `requests.ts` says what each request carries, whether this window may ask, what the title row says and where a request stands. `view.ts`, `kb.ts`, `output.ts`, `team.ts`, `ency.ts` (the encyclopedia of spec 117: citation markers, a page's parts, the home page's lists, a quotation found in a source, the commands for the clipboard), `marks.ts` (the operator's own marks: read state, notes on passages, pins, export) and `md.ts` (Markdown, as data) turn what the daemon sends into what a screen shows; they are pure and are where the tests are. `screens/` draws, `Rich.tsx` draws Markdown and `DocView.tsx` a document. A Mermaid block is drawn by the bundled renderer (`@mermaid-js/tiny`, one pinned file with no dependencies, loaded by `mermaid-load.ts` the first time a diagram is shown) in strict mode; `diagram.ts` cleans what it draws and shows it only as an `<img>`, or shows the text when it cannot (spec 118 §5 item 4). `npm run check-diagrams` (`dev/diagram-check.mjs`, part of `dev/build.sh`, needs Chrome or Chromium) draws hostile diagrams from `test/browser/cases.ts` in headless Chrome and fails if any runs script, navigates or fetches. `avatar.ts` and `Badge.tsx` draw an agent (chip, name, role) from the avatar table the CLI sends. `theme.css` is the look.
- `src-tauri/` is the bridge (Rust). `locate.rs` finds the daemon by asking `delphi daemon where`, and every daemon of the Delphi home by reading `<home>/run/*.json` and probing the sockets they name (`discover`, spec 130; the bridge's `daemons` and `attach` commands, and the window's `daemons.ts` and `screens/Start.tsx`); `client.rs` is a small client of the daemon that can name twenty read methods and four requests and no others (the nine reads of spec 111; the encyclopedia's eleven of spec 117 §3; and the four methods of the daemon's `request` grant, spec 120 §4.2. The eleven and the four are sent only on a connection that proved a paired key and refused on any other before a byte is written), and that begins a connection in one of two ways: with the narrator socket's token, or, for a paired app, by signing the app socket's challenge (spec 120); `key.rs` is the app's key: the ticket a terminal hands over, the key pair, where it is kept, and the one shape that is ever signed; `bridge.rs` keeps the two connections, forwards notifications in order and re-attaches; `ency.rs` is the encyclopedia's reads as the window may name them: it picks the method and builds the parameters from the few known arguments, each checked for shape, and cuts a source kept privately for retrieval down to the cited passage; `ask.rs` is the four requests as the window may name them: it picks the method and builds the parameters from ids and typed text, each checked for shape, and passes on nothing else; `marks.rs` keeps the operator's marks (`app/marks.json`) and writes an exported page (`app/exports/`), both for the operator alone and neither ever given to the daemon; `lib.rs` is the commands the window may call: eight reads of the daemon, one request (asked only when the proved key holds `request`), the marks (read and keep), export and print, and three that act outside the window on a click (show a file, open a file of a listed kind, open a web address). It is outside the repository's Cargo workspace, with its own lock file, and builds into `target/app/`.
- `dev/` holds the dependency gate, the icon makers, the build script and the smoke test.

## The app's key (spec 120)

`delphi app`, in a terminal, starts the app with a pairing ticket on its standard input. The bridge reads it (only from a pipe, one line of 64 hex digits, for at most two seconds), makes an Ed25519 key pair, sends the public half and the ticket to the daemon's app socket (`app.pair`), and keeps the private half under the short name the daemon gave it. From then on, started from anywhere, it opens both of its connections on the app socket by signing the daemon's challenge (`app.hello`): the nonce, the daemon's instance id, the protocol version and the path of the socket it connected to. With no key, a key the daemon refuses, or a daemon with no app socket, it attaches on the narrator socket and reads, as before, and says why in the connection's `detail`. A refused key is offered once per run, not on every re-attach. The window can ask for the same nine reads either way; a paired window can also ask for the encyclopedia's eleven (spec 117 §3), and one whose key holds `request` for the four requests (spec 120 §4.2). The title row says which of these the window is.

Two programs with no code in common have to sign and check the same text, so one test vector is held by both: `crates/delphi-core/src/app_handshake.vector`, checked by `key.rs` here and by `app_keys.rs` in the daemon.

`cargo run --example pair_probe` (named in `DELPHI_APP`, with a scratch `DELPHI_HOME`) is the same path with no window, for checking a daemon and a bridge against each other. With `DELPHI_PROBE_ASK='{"what":"page.rebuild","arg":{"slug":"<a topic>"}}'` it also asks for that once its session is open, through the same two steps the window's command takes.

### The end-to-end check (spec 120 §9), for a person at a terminal

Not done by the builders: a ticket is given only to a client at a terminal, and an agent has none. On a scratch home (`H=$(mktemp -d)`; every command below takes `--home $H`, and the probe `DELPHI_HOME=$H DELPHI_EXE=<the built delphi>`):

1. `delphi init --home $H`, then `delphi pages new "Tides" --home $H` so there is a topic to ask about.
2. Pair: `DELPHI_APP=<path to target/app/debug/examples/pair_probe> delphi app --home $H` once `delphi app` starts the app in your build (its process-start site is a gate-file change of its own); it prints the key's name, and `delphi app list --home $H` shows one key with `read, request`.
3. Start again with no ticket: `DELPHI_HOME=$H pair_probe` reports `paired` and both grants.
4. Ask: `DELPHI_PROBE_ASK='{"what":"page.rebuild","arg":{"slug":"tides"}}' DELPHI_HOME=$H pair_probe` reports the request's id and state; `delphi app log --home $H` lists the pairing and the request.
5. Revoke: `delphi app revoke <name> --home $H`; the probe then reports `read-only` and why, and asking is refused without the daemon being asked.

The same five steps with the built window in place of the probe are the check of the window itself: the title row reads `paired as <name> · read, request`, "Rebuild now" on a topic page is a button, and its line goes from running to done.

### Where the private key is kept

- **Every system:** `app/key.json` under the Delphi home, mode 0600 in a folder of mode 0700 (spec 120 §12, 2026-11-15: the keychain is used again once the app is signed with a stable identity). A key file that others can read is refused, not used. The app writes only in this folder under the home: the key, the operator's marks (`app/marks.json`) and exported pages (`app/exports/`). Any program running as the operator can read it; the key's grants (`read`, and `request`, which only asks) and `delphi app revoke` bound what that gives.
- **A key an earlier build kept in the macOS keychain** (one generic-password item, service `dev.delphi.desktop.app-key`, account the Delphi home's path) is moved once: read, written to the file, and the item removed. When macOS will not let this build read the item, the app reads without a key and `delphi app` pairs it again; nothing is written to the keychain any more (`key::Moving`).

The key store is asked for the key at most once in the app's life, and only when a daemon with an app socket is there.

### What an unsigned build gets from the keychain

Spec 120 §8 asks for this to be checked. Checked on 2026-11-15, macOS 27.0.1, with `examples/keychain_probe.rs` (two builds of one program, each signed only by the linker: `Signature=adhoc`, `flags=adhoc,linker-signed`, no team, different code hashes), with the keychain's prompts turned off so that a read the system would have asked about fails instead:

| What | Result | |
| :-- | :-- | :-- |
| A build stores an item and reads it back | read, no prompt | verified |
| The same build copied to another path reads it | read, no prompt: the keychain knows the program by its code, not its path | verified |
| A different build (another code hash, same name) reads it | refused (`-25293`) | verified |
| A different build deletes it | refused (`-25244`) | verified |
| A different build writes over it | **succeeds, with no prompt**; afterwards neither build can read the item (`-25293` for both), and only the build that created it can delete it | verified |
| With prompts on, the refused read becomes a dialog asking the operator to allow the new build, once or always | from Apple's documentation of keychain access control | not verified (no dialog was put on the operator's screen) |
| The item cannot go in the data-protection keychain, which would tie it to the app's identity across updates | that keychain needs an entitlement only a signed, provisioned app has | not verified |
| The built `Delphi.app` is signed the same way as the probes | its program is `adhoc,linker-signed`, with the same code hash as the bare `delphi-app`, no team, `Info.plist` not bound, no sealed resources | verified (`codesign -dvvv`) |
| So `Delphi.app` gets from the keychain what the probes got | follows from the row above | not verified (the app itself was not run against the keychain) |

What followed for the app while it kept its key in the keychain (it no longer does, spec 120 §12; kept as the record of why):

1. **The key is protected from other programs' eyes, including other builds of this app.** A program running as the operator cannot read it without the operator answering a dialog. That is more than the key file gives.
2. **Every rebuild or update of an unsigned app is a stranger to its own key.** The first time a new build needs the key, macOS refuses it, or, since the app leaves the keychain's prompts on, asks the operator to allow it (the dialog is the part not verified above). Allowed always, that build should not be asked again. Denied, the app reads on the narrator socket and says so; pairing again does not help until the old item is allowed or deleted in Keychain Access, because the app will not write over a key it could not read (and writing over it would leave an item no build can read). A signed app would be known by its developer identity and keep its key across updates.
3. **The key can be destroyed, not stolen, by any program running as the operator** (the write-over above). The app then reads on the narrator socket until `delphi app` pairs it again. Spec 120 §8 does not defend against software running as the operator, and this is inside that.

The encyclopedia's fixture (`src/fixture/ency.json`) answers the eleven reads in the daemon's shapes: it began as what the daemon's own test of those reads returned (`every_encyclopedia_read_leaves_the_workspace_byte_identical_and_calls_no_model`) and was extended by hand with a topic that has sections, key terms, citations of every kind, an earlier version and a source kept privately. The shapes it must keep are those of `crates/delphi-engine/src/encyclopedia.rs`. A page of the encyclopedia can be opened by its address in a browser (`?topic=tides#encyclopedia`, `?source=<id>&quote=…`, `?claim=<id>`, `?research=<id>`, `?changes`, `?role=scout#memory`).

The fixture (`src/fixture/sim.json`) is a recording: the state and events of the terminal narrator's simulator, taken before that simulator was retired, with two councils added in the daemon's shape. It is edited by hand from here on; the shapes it must keep are the daemon's (`crates/delphi-engine/src/observe.rs`). The built-in avatar table (`src/avatar-spec.json`) is the console library's, and a test fails when the two differ.
