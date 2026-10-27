# Debugging with the error log

When something goes wrong in a testing session you should not have to stop, scroll back or reproduce it. Delphi writes errors to one central, redacted, size-capped file as they happen, so you can read them afterwards, with context, while the session carries on. Nothing about logging can slow down, block or fail an operation: if a record cannot be written it is dropped and counted.

## Where the log is

```sh
delphi logs --path
```

prints it: `$DELPHI_HOME/logs/delphi.log.jsonl` (default `~/.delphi/logs/`). The directory is `0700` and the file `0600`. The CLI, the daemon and the full-screen console all append to the same file, one JSON object per line. At 10 MB it rotates to `delphi.log.jsonl.1` and `.2` and the oldest is deleted, so the log never takes more than about 30 MB.

## Levels

| Level | What is recorded |
| :-- | :-- |
| `off` | nothing; no file or directory is created |
| `error` (the default) | errors and panics: failed tasks, daemon RPC errors, harness and provider failures, daemon start-up failures, a command that exits non-zero, a console crash, relay delivery failures, Xenia sync tampering or rejection |
| `warn` | the above plus decisions that were not errors: a model out of quota or throttled (failover), a tool call that returned an error, a refused RPC method, a usage mistake (exit code 2), Xenia refusals, relay refusals |
| `debug` | the above plus the error source chain, a backtrace for panics and crashes, the RPC error message |

Turn it up for one command with the environment, or for good in the operator-scope file `$DELPHI_HOME/operator/config.toml`:

```sh
DELPHI_LOG=debug delphi chat
```

```toml
# $DELPHI_HOME/operator/config.toml
[logging]
level = "warn"   # off | error | warn | debug
max_mb = 10      # size of each file
files = 3        # the log and its rotated copies
```

`DELPHI_LOG` wins over the file. That file is yours: it is not a workspace `config.toml`, no agent can read it and no proposal can change it. `delphi doctor` (check 14) shows the level in force and where it came from, the path, the size and how many records were reported dropped.

## Reading it

```sh
delphi logs                            # the last 50 records, one line each
delphi logs --since 1h --level warn    # a window, at least this severe
delphi logs --component harness        # cli daemon engine harness provider ui xenia relay tool
delphi logs --mission 2026-10-27-fix   # one mission's records
delphi logs --json                     # the raw records, for jq
delphi logs --follow                   # keep printing as records arrive (Ctrl+C to stop)
```

Each record has a timestamp, level, component, process id, Delphi version and OS, a stable `slug` (for example `harness_failed`, `provider_http`, `rpc_error`, `task_failed`, `panic`), a message, and a small `context`: mission, task and span ids, command, exit code, harness or provider name, HTTP status, tool name. `jq` works on the file directly: `jq 'select(.slug=="harness_failed")' ~/.delphi/logs/delphi.log.jsonl`.

## Sharing a session

```sh
delphi debug bundle --since 24h -o session.tar.gz
```

writes one archive, mode `0600`, and refuses to overwrite a file. It holds: the log slice, the `delphi doctor` report, the version and OS, the effective logging settings and the key paths of your config files (every string value replaced by its length), and the last 200 events of the mission (event types, ids and short status fields only; no prompts, outputs or file contents). It prints exactly what it included. Both `logs` and `debug bundle` are read-only and offline.

## What is and is not recorded

Recorded: slugs, ids, command and tool names, exit codes, HTTP statuses, durations, the text of Delphi's own error messages, and, for a failed `claude` or `agy` run, the last few lines of the CLI's standard error.

Never recorded: prompts, model output, request or response bodies, file contents, environment values, keys, tokens, signing material, store text. The writer keeps only a fixed list of context fields, so a caller cannot add a body by accident.

Everything is cleaned before it is written: known secret formats and every credential the daemon resolved at run time are masked; URL credentials and sensitive query values, bearer tokens, DIDs, e-mail addresses, long hex and base64 strings are replaced; your home directory becomes `$DELPHI_HOME` or `~`; control, escape, bidirectional and zero-width characters are replaced and newlines become `\n`, so a record is always one line; and every field is shortened.

## Privacy and safety

The log can contain text that came from tools, web pages and stores, which is untrusted, and (in the stderr tail of a failed CLI) text you did not write. For that reason it is outside everything an agent can read: the path guard refuses `logs/` under any spelling, and neither the console nor any RPC method returns log contents to an agent. You and your own developer tools read the file directly. Read a bundle before you send it; the redaction is thorough but a pattern-based redactor cannot recognize every secret.

If something you expected is missing: the level may be `error`, the disk may have been full or the directory unwritable (the next record that is written says how many were dropped, and `delphi doctor` warns), or `DELPHI_LOG=off` may be set.
