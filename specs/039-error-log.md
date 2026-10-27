---
id: spec-039-error-log
title: "039: The central error log"
author: architect
timestamp: 2026-10-27T12:00:00Z
status: proposed   # built in one pull request; awaiting the operator's approval
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # exercised by tests with temporary homes; not yet run through a long live session
source_mission: 2026-10-23-foundational-design
tags: [spec, logging, debugging, security, draft]
---

# 039: The central error log

**Status:** built, awaiting approval. **Commands:** `delphi logs`, `delphi debug bundle`; `delphi doctor` check 14.
**Code:** `crates/delphi-core/src/elog.rs` (writer, reader, redaction), `crates/delphi/src/logs_cmd.rs` (the two commands),
`ui/src/elog.ts` and `ui/src/boundary.tsx` (the console). **Guide:** `docs/guides/debugging.md`.

## 1. Why

The operator asked: "does the system log all errors? it should, at least as an option, so we can use them to debug with context
without breaking testing flow." It did not. There is no logging framework; the daemon's stderr goes to a per-daemon file that is
never rotated and is shown only when the daemon fails to start; CLI errors reach the terminal and are lost; RPC, tool, provider and
harness errors are returned to a client and forgotten; the console prints a fatal error once and exits. Mission event logs record
`task.failed` but not the rest. Debugging a testing session meant reproducing it.

## 2. Design

**One file.** `$DELPHI_HOME/logs/delphi.log.jsonl` (directory `0700`, file `0600`), JSON Lines, appended by the CLI, the daemon and the
console. Appends use `O_APPEND`, one `write` per record (at most 16 KB), so concurrent processes cannot interleave inside a line
and no lock is held to write. Rotation at `max_mb` (10) keeping `files` (3) files: `delphi.log.jsonl`, `.1`, `.2`; a non-blocking
`flock` on a side file lets one process rotate while the rest keep appending. The file is opened with `O_NOFOLLOW`.

**Record.** `ts` (RFC 3339 UTC, milliseconds), `level`, `component` (`cli daemon engine harness provider ui xenia relay tool`),
`pid`, `version`, `os`, `slug` (a stable kind), `message`, optional `context`, `chain` and `backtrace` (debug only), and `dropped`
(how many records this process lost before this one).

**Context.** Only these keys are kept, whatever the caller passes: `mission task span command args_shape exit_code harness provider
status duration_ms tool method kind model role attempt stderr_tail error_type reason from to retry_after_s bytes count store phase
view`. There is no key for a body, a prompt, a file's contents, an environment value or a key, so none can be logged by accident.

**Sanitizing, before anything is serialized.** Control, bidi, zero-width and line-separator characters are replaced (a newline
becomes the two characters `\n`; one record is one line); the engine's secret redactor runs, including every credential the daemon
resolved at run time (`SecretRegistry::register` also registers it with the log); then URL credentials and sensitive query values,
bearer tokens, JWTs, DIDs, e-mail addresses, 32+ character hex strings and 40+ character base64-like strings with both letters and
digits are replaced; the Delphi home becomes `$DELPHI_HOME` and the user's home `~`; then each field is truncated (message 1500
characters, context values 400, backtrace 4000). Redaction runs on at most the first 16 KB of a field.

**Never in the way.** Logging is best-effort and synchronous but cheap (one small write). A failure of any kind (an unwritable or
missing directory, a full disk, a symlink in the way, a panic inside the logger, caught) drops the record and counts it; the next
record that is written carries the count. It never prints, never returns an error, never changes an exit code. Callers format only
after `elog::enabled(level)`, or pass a closure (`debug_with`).

**Levels.** `off` (nothing, not even the directory), `error` (default), `warn`, `debug`. `DELPHI_LOG` sets it; so does `[logging] level`
in `$DELPHI_HOME/operator/config.toml`, together with `max_mb` and `files`; the environment wins. That file is in the operator's
scope: `pathguard` denies `operator/` to every agent for reading and writing, and it is not one of the ratified workspace files an
agent can propose changes to. `delphi doctor` check 14 reports the effective level and its source, the path, the size, the cap and
the drops recorded in the file, and warns when the directory is unwritable or records were dropped.

**Where agents cannot go.** The log holds text echoed from tools, stores and CLIs: a prompt-injection and leak channel. `pathguard`
denies the logical path `logs/` under any spelling (case, `./`, `//`, symlinks, `..`), for reading and writing, in policy, not only
because no zone maps it. No RPC method returns log contents; the operator and their own tools read the file.

## 3. Capture points

| Where | Slug | Level |
| :-- | :-- | :-- |
| daemon RPC handler returns an error (method and code; the message at debug only, never params) | `rpc_error` | error |
| a method refused on the narrator or relay socket | `rpc_forbidden` | warn |
| a client connection ended in an I/O error | `connection_error` | warn |
| a task ended in an error (all six sites that write `task.failed`) | `task_failed` | error |
| a tool call returned an error (tool, role, span; not the text) | `tool_failed` | warn |
| a harness (`claude`, `agy`) run failed: reason, exit code, redacted stderr tail | `harness_failed` | error |
| a harness run ended on a spent quota | `harness_exhausted` | warn |
| a provider call failed: endpoint, model, HTTP status, error type; never the body or the provider's message | `provider_*` | error / warn (throttle, quota, unreachable) |
| a model is marked exhausted (failover decision) | `quota_exhausted` | warn |
| the daemon failed to start (in the daemon, and in the launcher with the tail of the per-daemon file); the launch lock timed out | `startup_failed`, `daemon_start_failed`, `launch_lock_timeout` | error |
| a panic in the CLI, the daemon or a worker thread (message, location, thread; backtrace at debug), then the previous hook runs | `panic` | error |
| a command ends non-zero: command, exit code, the shape of its arguments (flag names, word count) | `cli_exit` | error; warn for exit code 2 and for exit code 1 of commands that report findings |
| a command prints an error and returns a code (`setup`, `init`, `needs`, start-up) | `cli_error` | error |
| Xenia journal entries other than success (the journal's own slug) | `xenia_<outcome>` | error for `tampering`, `rejected`; else warn |
| relay: card or notice not delivered, poll failed | `relay_delivery_failed` | error |
| relay: refused (rate limit, unknown sender) | `relay_refused` | warn |
| the tool bridge relay cannot reach the span's server | `bridge_unreachable` | error |
| the retrieval reranker or embedder cannot be used | `kb_*` | warn |
| console: uncaught exception, unhandled rejection, React render error, console stopped | `uncaught_exception`, `unhandled_rejection`, `render_error`, `console_stopped` | error |

## 4. Commands

`delphi logs [--lines n] [--since 1h] [--level l] [--component c] [--mission id] [--json] [--follow] [--path]`: read-only,
offline; text is re-sanitized when printed, because the file could have been edited and a terminal is not a safe place for escape
sequences. `--follow` survives rotation.

`delphi debug bundle [--since 24h] [-o file] [--mission id] [--workspace n]`: a gzip tar, created `0600` with `create_new`
(never overwrites), holding `MANIFEST.txt`, `version.txt`, `delphi.log.jsonl` (every record re-sanitized), `config.txt` (logging
settings, and the key paths of the operator and workspace config with each string value replaced by its length), and, when a workspace
resolves, `doctor.txt` and `events.jsonl` (the last 200 events of the mission, event type, ids and an allow-list of short status
fields only). It prints what it included.

## 5. As built, and differences from the first design

- **Tool-call failures are `warn`, not `error`.** An agent's tool call returning an error (a refused path, a failing command) is
  routine; at `error` it would drown the default log. Real failures of the engine around it surface as `task_failed`.
- **The CLI's exit is logged by shape.** There is no central place that sees the text a command prints, so the top-level record has
  the command, exit code and argument shape; the commands that return an error to the top (`setup`, `init`, `needs`, start-up) also
  log the text. The other roughly 240 `eprintln!` sites in the CLI are unchanged.
- **Drop count is reported by the next record,** not kept in a side file, so `delphi doctor` reads it from the log; a process that
  dies before it can write again loses its count.
- **Context keys are a fixed list** rather than free-form; adding a key is a code change.
- **Stderr of a harness CLI is captured only while the log is on,** into a 4 KB tail; otherwise it is discarded as before.
- **Provider messages are not logged,** only status and error type, because they can echo request text.
- **The console's React boundary rethrows** after logging, so Ink stops and prints exactly as before; the ui logs via
  `uncaughtExceptionMonitor` as well as the Term's handlers.

## 6. Known weaknesses and deferred

- Redaction is pattern-based. A secret in a shape no pattern knows, in a field that is logged (a harness stderr tail, a task error
  message), would be written. The stderr tail is the one place text written by a third-party program reaches the log at the default
  level; it is capped at 4 KB, redacted and escaped, and the log is outside agent reach.
- Not converted: roughly 240 `eprintln!` calls in the CLI (usage text, progress, `train`, `bundle`, `skills`, `kb` command output),
  the sandbox and adapter crates, and the in-engine silent `let _ =` sites. Deferred: logging the time a hot path waits on a lock,
  a `delphi logs --grep`, shipping records to a collector, and a console view of the log.
