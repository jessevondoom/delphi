# Ink spike findings (2026-10-24)

Throwaway client: `src/main.tsx` (Ink 8 + React 19 + marked), `src/rpc.ts` (daemon JSON-RPC client), `src/markdown.ts`.
`src/fake-daemon.mjs` stands in for the daemon (the real one needs model endpoints). `delphi daemon where` prints the socket paths.
Build: `bun build --compile src/main.tsx` (tsconfig `paths` stubs the optional `react-devtools-core`).
Reference terminal was meant to be Terminal.app; the numbers below come from a pty plus a pyte emulator, **not** a real Terminal.app.

| Check | Result |
| :-- | :-- |
| Compiles to one executable | yes, 90 MB (arm64 macOS); Node-free |
| Cold start to first frame | 0.17 s |
| Streaming a ~2 KB markdown answer in 111 frames | no corruption in the final frame; 45 KB written (Ink repaints the changed region each frame) |
| Markdown (marked): headings, bold, code, lists, table, quote, links | works, ~70 lines of styling code |
| Multi-line composer, Alt+Enter | works; `\r` arriving in the same read as text is treated as paste, not Enter |
| PgUp / PgDn scroll, "lines below" marker | works |
| Mouse wheel | **did not work**: Ink hands SGR mouse sequences to `useInput` unreliably; needs our own stdin parser |
| Shift+Enter | not tested in a real terminal; Terminal.app does not send a distinct code without the kitty protocol |
| Redraw artifacts | the first column of the top transcript row was lost after scrolling in two frames (` ission`, ` ine 17`); not diagnosed (Ink diffing vs emulator) |
| Dependencies | 109 lockfile entries (about 50 top-level packages in production, the rest are dev: bun binaries, tsx, esbuild, typescript) |
| Council panel | simple list view from `council.*` events works; no layout work needed |

Not tested: real daemon (TTY challenge for a non-Rust client), real terminals, Linux build, 4-target CI, flicker on a real display, memory use.
