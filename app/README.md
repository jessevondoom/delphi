# delphi-app: the desktop app (spec 111)

A read-only window on the daemon. The guide is [`docs/guides/desktop-app.md`](../docs/guides/desktop-app.md); the rules it keeps are in [`specs/111`](../specs/111-the-desktop-app-a-read-only-window-on-the-daemon.md).

```
dev/build.sh        # every check, the unsigned app, and a smoke test of the built app
npm run dev         # the window as a web page with sample data, at http://localhost:5199
npm run check       # dependency gate, type check, tests, build
npm run app         # only the app bundle
```

How it is put together:

- `src/` is the window (TypeScript and React). `bridge.ts` is its only way to the daemon: a few named reads and three kinds of notification, served by the app or, with no app, by a fixture. `view.ts`, `kb.ts`, `output.ts`, `team.ts` and `md.ts` (Markdown, as data) turn what the daemon sends into what a screen shows; they are pure and are where the tests are. `screens/` draws, `Rich.tsx` draws Markdown and `DocView.tsx` a document. `avatar.ts` and `Badge.tsx` draw an agent (chip, name, role) from the avatar table the CLI sends. `theme.css` is the look.
- `src-tauri/` is the bridge (Rust). `locate.rs` finds the daemon by asking `delphi daemon where`; `client.rs` is a small client of the narrator socket that can name nine read methods and no others; `bridge.rs` keeps the two connections, forwards notifications in order and re-attaches; `lib.rs` is the commands the window may call: seven reads, and three that act outside the window on a click (show a file, open a file of a listed kind, open a web address). It is outside the repository's Cargo workspace, with its own lock file, and builds into `target/app/`.
- `dev/` holds the dependency gate, the icon makers, the build script and the smoke test.

The fixture (`src/fixture/sim.json`) is a recording: the state and events of the terminal narrator's simulator, taken before that simulator was retired, with two councils added in the daemon's shape. It is edited by hand from here on; the shapes it must keep are the daemon's (`crates/delphi-engine/src/observe.rs`). The built-in avatar table (`src/avatar-spec.json`) is the console library's, and a test fails when the two differ.
