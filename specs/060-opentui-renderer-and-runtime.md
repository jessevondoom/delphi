---
id: spec-060-opentui-renderer-and-runtime
title: "060: OpenTUI renderer and supported UI runtime"
author: architect
timestamp: 2026-10-30T00:00:00Z
status: approved   # approved by the operator on 2026-10-30: "approve 060 and continue"
approved_by: operator (2026-10-30)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, tui, renderer, opentui, runtime]
---

# Spec 060: OpenTUI renderer and supported UI runtime

**Status.** Approved by the operator on 2026-10-30. This supersedes the renderer choice in D-28 and amends the architecture notes in approved specs [`005e`](005e-console-ux-review-and-redesign.md) and [`005f`](005f-console-redesign-build-plan.md). D-33 records the decision; the spec tracker records implementation progress.

## 1. Goal

Replace Ink as the rendering driver for Delphi's full-screen console and observer with OpenTUI, while keeping Delphi's UI model, pure frame functions, socket scopes, terminal input semantics, Rust fallbacks, and release artifact contract intact. Make the runtime required by source-mode UI work explicit and enforce it in CI.

## 2. Scope and boundaries

- Replace Ink's renderer with `@opentui/core` and `@opentui/react` in `ui/`.
- Preserve `frame(model, w, h)` and `observeFrame(model, w, h)` as the source of screen content and retain their golden tests.
- Keep the daemon protocol, console and narrator authority, approval behavior, Rust core, inline console, and plain console unchanged.
- Keep release output as a standalone console executable compiled with the pinned Bun toolchain for every currently supported release target.
- Declare Node.js 26.4 or newer with `--experimental-ffi` as the minimum for running the source UI and its tests under Node. CI must use a pinned supported Node version and the flag. Bun remains supported for compiling the standalone executable.
- Do not add a second terminal-input or terminal-mode owner. Delphi's terminal input layer and OpenTUI must have a documented, tested division of responsibility for raw mode, resize, alternate-screen handling, mouse reporting, suspend/resume, and cleanup.

## 3. Requirements

1. Production dependencies remain exact-pinned, integrity-locked, install-script-free, and checked by `ui/dev/check-deps.mjs`. The allowlist and package limit must reflect the actual locked production tree.
2. Source-mode console and observer start, render, accept input, resize, and exit under the declared Node runtime. Importing the UI under an unsupported Node version must fail with a clear runtime requirement rather than a native binding stack trace.
3. Console and observer use their existing socket scopes. The observer cannot start work, resolve decisions, or gain console authority through the renderer integration.
4. On normal exit, errors, signals, suspend/resume, and daemon disconnect, terminal modes are restored and errors remain visible. The renderer and terminal input layer must not race over stdin or emit conflicting screen-control sequences.
5. Existing UI behavior remains covered: frame geometry and goldens, keyboard and paste handling, approval cards and guard timing, motion settings, color fallback, scroll behavior, and observer navigation.
6. CI runs the dependency gate, typecheck, UI unit tests, and real-daemon pseudo-terminal end-to-end flows on the supported source runtime. It also builds the standalone executable and runs the compiled-binary pseudo-terminal flows.
7. The release workflow compiles and packages the standalone UI for every existing target. A smoke check verifies the executable starts and reports its version beside the matching Delphi binary.
8. Before declaring the renderer ready for operators, complete and record the real-terminal checklist in `ui/TERMINALS.md` on Terminal.app, iTerm2, Ghostty, and tmux, including dark/light themes, `NO_COLOR`, reduced motion, resize, keyboard input, and terminal restoration.

## 4. Acceptance gates

- `cd ui && npm ci --ignore-scripts --no-audit --no-fund`
- `cd ui && npm run check-deps && npm run typecheck && npm test`
- The UI CI pseudo-terminal suite passes against both source mode and the release-style compiled executable.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo run -q --locked -p delphi -- lint --repo .`
- All existing release target builds produce a runnable, version-matched standalone UI artifact.
- Real-terminal results are recorded, and each failure is fixed or explicitly left as a tracked limitation before release.

## 5. Rollout

Land as one focused renderer/toolchain slice after approval. Keep the old Ink implementation available in Git history for rollback. Do not merge or release while any required gate fails. Update `D-28`, the affected 005e/005f architecture references, `ui/README.md`, `ui/TERMINALS.md`, CI runtime configuration, and the spec tracker in the same reviewed change.
