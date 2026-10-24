# delphi-ui: the full-screen console (D-28)

An [Ink](https://github.com/vadimdemedes/ink) client of the daemon's JSON-RPC socket. `delphi chat` starts it (`delphi-ui` next to the `delphi` executable, or `$DELPHI_UI`) and falls back to `delphi chat --inline` when it is absent. The look follows `specs/005b` and `specs/ui-wireframes-and-blueprints.md`; the plan is `PLAN.md`; the spike's findings are `FINDINGS.md`.

```
./try.sh          # fake daemon + the UI from source (Ctrl+C quits)
npm test          # unit tests: input parser, composer, event fold, the screen as data
npm run typecheck
npm run check-deps  # supply-chain gate: allowlist, exact pins, hashes, no install scripts
npm run build     # one executable with Bun (needs the bun dev dependency's binary)
```

How it is put together: `terminal.ts` owns stdin (bracketed paste, wheel, background-colour query) and feeds `input.ts`; `fold.ts` turns daemon notifications into state; `screen.ts` turns state into exactly `h` lines of `w` columns (pure, tested); `app.tsx` wires them and Ink paints the lines. `dev/drive.py` drives it in a pseudo-terminal for a look at the real screen.
