---
id: spec-005-console-and-narrator-tui
title: "005: Console, Narrator Window & UDS Event-Bus Daemon"
author: architect
timestamp: 2026-10-23T08:05:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec]
---

# Spec 005: Console, Narrator Window & UDS Event-Bus Daemon

## 1. Scope & Objectives
This specification defines the dual-window terminal interface and local client-daemon architecture of Delphi (implemented in **Rust** using `Tokio`, `Ratatui`, and `Crossterm`):
1. **`delphi-core` Local Daemon & Unix Domain Socket (UDS) Bus:** Event-sourced runtime engine with 60Hz frame coalescing, append-only `events.jsonl` replay, and SQLite WAL state indexing.
2. **Window 1 — `Console` (`delphi` / `delphi chat`):** The sovereign operator interface, prompt input, slash-command palette, and synchronous Human-in-the-Loop (HITL) permission modals.
3. **Window 2 — `Narrator` (`delphi narrator`):** Attach-anywhere secondary live narrative and telemetry window—the epic storyteller chronicling the team's actions in real time (Execution DAG, 3-column Council Chamber, Builder Forge diffs, and Model Scaling telemetry).
4. **Multiplexer & OS Window Auto-Spawning:** Seamless dual-window launching across `tmux`, `zellij`, `WezTerm`, `Kitty`, and macOS windows, plus single-screen `Tab` toggle fallback.

### 1.1 Simpler Alternative Considered
- **Alternative:** A monolithic single-process TUI where the agent runtime and terminal UI run in the same process without a Unix Domain Socket daemon, using log file tailing (`tail -f`) for a second window.
- **Why Rejected:** One-way log tailing cannot support interactive multi-window control (such as pausing a runaway agent from `Narrator`, inspecting live collapsible DAG nodes, or surviving an accidental terminal window closure without killing active `Builder` builds). Auto-spawning a per-workspace UDS daemon (`~/.delphi/run/<workspace-hash>.sock` — the exact pattern used by `tmux` and `rust-analyzer`) adds minimal code while unlocking sub-15µs bidirectional multi-window sync and crash resilience.
- **Decision D-05 (revised 2026-10-23 after the Gemini review): a minimal daemon ships in roadmap Step 2.** A single process would have broken live token streaming in Narrator (deltas aren't persisted) and meant building the runtime twice. Step 2 therefore ships `delphi-core` with the Unix socket, JSON-RPC handshake, coalescer, race-free spawn and slow-client backpressure from §2.1, plus the durability rules from §2.3. It does **not** ship multi-workspace management or remote attach. Narrator gets live control actions (pause, inspect) in Step 5.

---

## 2. Client-Daemon Architecture (`delphi-core` over UDS)

```mermaid
flowchart LR
    subgraph Clients ["Stateless Terminal UI Clients"]
        the operator["Window 1: CHAOS (`delphi`)\nOriginator Operator Deck & HITL Gates"]
        Narrator["Window 2: HOMER (`delphi narrator`)\nLive Epic Narrator, DAG, Council Chamber & Forge"]
    end

    subgraph Socket ["Unix Domain Socket (`0600` perms)"]
        UDS["~/.delphi/run/<workspace-hash>.sock\nJSON-RPC 2.0 + 60Hz Coalesced Event Stream"]
    end

    subgraph Daemon ["delphi-core Daemon"]
        Coalescer["16ms (60fps) Delta Frame Coalescer"]
        Engine["agent State Machine & Adapter"]
        Log["missions/<id>/events.jsonl (Append-Only Truth)"]
        DB[".delphi/state.db (SQLite WAL + FTS5)"]
    end

    the operator <==> UDS
    Narrator <==> UDS
    UDS <==> Coalescer
    Coalescer <==> Engine
    Engine --> Log
    Engine --> DB
```

### 2.1 Daemon Auto-Spawn & Socket Security Invariants
1. **Zero-Config Auto-Spawn (race-free):** Running `delphi` computes `<workspace-hash>` from the *canonical* workspace path (`realpath`, so symlinks and `..` resolve to the same daemon) and connects to `~/.delphi/run/<workspace-hash>.sock`. If the connect fails, the client:
   1. Takes an exclusive `flock` on `~/.delphi/run/<workspace-hash>.lock`, which serializes concurrent launches.
   2. Retries the connect; a daemon may have started while it waited.
   3. Treats a socket as stale only if the PID in `<workspace-hash>.pid` is dead. Only then does it unlink the socket.
   4. Spawns `delphi-core --daemon --workspace <path> --ready-fd N` and blocks until the daemon writes one byte to the readiness pipe, which happens after bind, migrations, and manifest load. The timeout is 5 s; on timeout the client shows the daemon's stderr log.
   The daemon holds the lock for its lifetime, so a second daemon can never start for the same workspace.
2. **File Permissions:** The socket directory `~/.delphi/run/` is created with mode `0700` and socket files with `0600` so no other OS user or browser process can connect. The daemon also verifies `SO_PEERCRED` / `getpeereid` equals its own UID.
3. **Protocol Handshake:** The first JSON-RPC call is `hello {protocol_version, binary_version}`. If the major versions differ, the client refuses to attach and tells the operator to run `delphi daemon restart`. This happens, for example, after `delphi update` swaps the binary under a running daemon.
4. **60Hz Frame Coalescing (`[REV-TRAP-03]`):** When 3–5 Council specialists stream tokens concurrently (emitting 300+ token deltas/sec), `delphi-core` buffers `agent.thinking.delta` and `agent.text.delta` events per span and flushes coalesced frames every **16ms (60Hz)** to connected TUI clients, preventing terminal stdout saturation and layout thrashing. Each frame carries `seq_from`..`seq_to`, so "no dropped sequence numbers" is checkable even though deltas are merged.
5. **Slow-Client Backpressure:** Each client has a bounded outbound queue (default 1,024 frames). On overflow, the daemon drops that client's queued frames and sends `resync_required`; the client then re-fetches `SessionSnapshot`. A stalled `Narrator`, for example over a laggy SSH link, never grows daemon memory or slows other clients.

### 2.3 Durability & Single Source of Truth
- `missions/<id>/events.jsonl` is the **only source of truth**. `.delphi/state.db` is a derived index that can be rebuilt (`specs/004` §3.1). The daemon is the **single writer** of both.
- **Write order:** append the event line → `fsync` (every lifecycle and approval event immediately; content events batched at ≤250 ms) → update SQLite. On startup, the daemon truncates a torn final line and replays any events past SQLite's `last_applied_seq`.
- **What is persisted:** per-token deltas are *not* persisted. Each completed block (text, thinking, tool call, tool result) is persisted once as `agent.block.complete`. Live deltas exist only in the coalescer and a per-span in-memory ring buffer used for reconnects.
- **Council Round 1 buffering:** during Round 1, each seat's `agent.block.complete` events are held in daemon memory and written to `events.jsonl` only when the barrier releases (`specs/003` §4.1). Live Round 1 streams go to operator clients (Narrator's Council Chamber), never to agents. Agents cannot read `events.jsonl` at all (`specs/001` §2.2).
- **Redaction:** before any event reaches disk or a client, a deterministic redactor masks known secret formats (provider API keys, `gh` tokens, PEM blocks, `.env`-style `KEY=value` pairs for configured key names).
- **Reasoning retention (D-16, ratified 2026-10-23):** completed thinking blocks are persisted, redacted, for 30 days. After that, `delphi gc` compacts them to signatures and metadata only. Provider signatures needed to resume an open task are never dropped while the task is open.

### 2.2 Canonical `EventEnvelope` Wire Schema
Every state transition, token delta, tool call, model upscale, and approval request is serialized as an `EventEnvelope`:
```json
{
  "seq": 2048,
  "timestamp": "2026-10-23T07:51:10.120Z",
  "mission_id": "2026-10-23-federated-sync",
  "trace_id": "tr_0192b",
  "span_id": "sp_ares_r2",
  "parent_span_id": "sp_athena_council_01",
  "agent": "red-team",
  "provider": "google",
  "model_id": "gemini-3-pro",
  "thinking_budget": 16384,
  "mode": "council",
  "round": 2,
  "type": "agent.thinking.delta",
  "payload": {
    "delta": "Inspecting Integrator capability token expiry under clock skew..."
  }
}
```

---

## 3. Window 1: `Console` (`delphi` / `delphi chat`) — The Operator Deck

Per [GOVERNANCE.md](../GOVERNANCE.md) §1.1, the console is persona-free: it is the operator's instrument, not an agent. Its window label comes from the active theme.

### 3.1 Core Operator Capabilities
- **Conversational Stream:** Displays **Orchestrator's** high-signal syntheses, Council Verdicts (with collapsible **Minority Reports**), and clickable `file:///path#L10-L20` artifact links.
- **Direct Specialist Invocation:** Prefixing a prompt with `@<role>` (e.g., `@investigator benchmark the SQLite FTS5 query latency` or `@writer clean up README.md`) routes directly to that specialist while logging the interaction in the mission trace.
- **Slash Command Palette:**
  - `/council "<topic>"`: Convenes a 3-round Council debate.
  - `/narrator`: Spawns or focuses Window 2 (`Narrator`).
  - `/scale <agent> <tier|model>`: Manually overrides **Orchestrator's** dynamic model selection for an agent.
  - `/retro`: Triggers **Orchestrator's** 4-channel coaching pass, then presents every staged proposal (heuristics, `REVIEW.md` §4/§5, scripts) as one batched ratification list.
  - `/permissions`: Cycles approval mode (`default`, `accept-edits`, `mission-auto`). The modes change only how Amber-tier actions are handled ([`specs/008`](008-ambient-watch-and-rewind.md) §4). **No mode ever auto-approves a Red-tier action.**
  - `/budget`: Shows and adjusts the mission cost ceiling (`specs/003` §3.2).
- **Synchronous HITL Approval Modal:** Whenever **Builder** or **Integrator** hits a Destructive/Outbound Action Gate (`GOVERNANCE.md` §5.4), `Console` renders an inline keyboard-driven approval card (`[y] Approve Once`, `[a] Allow for Mission`, `[n] Reject & Redirect`, `[d] View Diff`). `[a]` is unavailable on Red-tier cards.
  - Every approval has an `approval_id`, and the daemon accepts the **first** answer. When several `Console` clients are attached, the others see the card resolve, with who answered and how.
  - With no `Console` client attached, the request waits up to `approval_timeout` (default 10 min), then resolves to **deny**. The agent receives a structured denial and does not hang.

---

## 4. Window 2: `Narrator` (`delphi narrator`) — Live Narration & Telemetry Window

The **Narrator** attaches to the workspace UDS socket from any second terminal window or monitor to narrate and visualize what the agent team is doing in real time across 4 switchable views (`1`–`4` hotkeys):

1. **`[1: The Epic & agent DAG]`:**
   - Live topological graph and real-time narrative stream of active and completed subtasks.
   - Displays each active agent's current **Provider + Model Tier + Thinking Budget** badge (e.g., `🔨 Builder [claude-sonnet-4-6 • think:4k]` or `⬆️ Upscaled -> claude-opus-4-6 [think:16k]`), live token/cost burn rate, and worktree path.
2. **`[2: The Council Chamber]`:**
   - Multi-column live split view during **Council Mode**.
   - **Round 1:** Shows parallel progress bars + live blind generation streams for each summoned specialist.
   - **Round 2:** Side-by-side view of **Red-Team's** adversarial attacks, **Strategist's** scope/ROI critique, and **Architect's** structural refinements.
   - **Round 3:** **Orchestrator's** Verdict synthesis alongside the highlighted **Minority Report** box.
3. **`[3: The Forge (Worktrees & Diffs)]`:**
   - Live `git diff` viewer and test-runner output stream for **Builder's** isolated worktrees (`.delphi/worktrees/<task-id>/`), plus line-anchored `REVIEW.md` audit comments from **Red-Team** and **Strategist**.
4. **`[4: The Archives (Knowledge Base & Coaching Ledger)]`:**
   - Instant fuzzy/FTS5 browser over `context/`, `specs/`, `REVIEW.md`, and each agent's private `memory/agents/<role>/lessons.md`.

---

## 5. Dual-Window Spawning & Single-Screen Fallback

| Launch Method / Environment | Behavior |
| :--- | :--- |
| **Manual Attach (`delphi narrator` in Terminal 2)** | Connects to `~/.delphi/run/<workspace-hash>.sock`, fetches `SessionSnapshot` in `<5ms`, and streams live events. Works in **every** terminal emulator or SSH session. |
| **Auto-Dual Flag (`delphi --dual` or `/narrator` / `Ctrl+W`)** | *(v1, D-06 ratified 2026-10-23: `tmux` + manual attach only; the rest is deferred.)* Auto-detects environment in order:<br>1. `$TMUX` $\rightarrow$ `tmux split-window -h -p 45 "delphi narrator"`<br>2. `$ZELLIJ` $\rightarrow$ `zellij action new-pane --direction right -- delphi narrator`<br>3. `WezTerm` $\rightarrow$ `wezterm cli spawn --new-window -- delphi narrator`<br>4. `Kitty` $\rightarrow$ `kitty @ launch --type=os-window delphi narrator`<br>5. macOS fallback $\rightarrow$ spawns second terminal window via `osascript`. |
| **Single-Window Toggle (`Tab` inside `Console`)** | Toggles an in-place `Narrator` drawer/tab inside Window 1 so laptop/single-screen users have full observability without a second window. |

---

## 6. Verification Plan
1. **60Hz Backpressure Benchmark:** Simulate 5 concurrent agents streaming 500 token deltas/sec over UDS; verify `delphi-core` coalesces deltas into $\le 60$ frames/sec per client with `<1%` CPU usage and zero dropped sequence numbers.
2. **Client Reconnect / State Replay Test:** Kill `delphi narrator` (`SIGKILL`) mid-Council Round 2, relaunch `delphi narrator`, and verify it reconstructs the exact Round 1 + Round 2 state from `SessionSnapshot` + ring buffer within `10ms`.
3. **Spawn Race Test:** Launch 10 `delphi` clients simultaneously against a cold workspace; exactly one daemon starts and all 10 attach.
4. **Approval Race & Orphan Test:** Two `Console` clients answer the same approval with `y` and `n` at once; exactly one answer is applied. With zero clients attached, an approval resolves to deny at the timeout.
5. **Slow Client Test:** Pause a `Narrator` client (`SIGSTOP`) for 60 s during a 5-agent Council; daemon RSS stays bounded, other clients are unaffected, and the resumed client resyncs.
6. **Crash Durability Test:** `SIGKILL` the daemon mid-write 100 times; every restart yields a valid log (torn line truncated) and a `state.db` consistent with it.
7. **Version Skew Test:** A client at protocol major N+1 refuses to attach to a daemon at major N and shows the restart hint.
