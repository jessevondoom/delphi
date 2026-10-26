---
status: approved
---
# Record which rules a task carried, and read tasks back as records

To learn whether a rule helps, Delphi must know which rules each task had in its prompt and how the task ended. Today neither is recorded. This spec adds the evidence plumbing; it changes no behavior of any agent.

## Required behavior (crate `delphi-engine`)

### 1. Which rules a snapshot carries (`workspace.rs`)

- `Snapshot` (what `load_snapshot` returns) gains `pub loaded: Vec<LoadedRule>`.
- New `pub struct LoadedRule { pub scope: String, pub id: String }`, deriving `Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord`.
- New `pub fn rule_ids(text: &str) -> Vec<String>`: the IDs of the `### ` rule headings in `text`, in order. A heading's ID is its first `[...]` token, trimmed; backticks around the token are ignored. A heading with no bracketed token is identified by its title (backticks trimmed), so an unnumbered rule is still counted. A heading that is empty after `### ` yields nothing. Lines that merely start with more than three `#` are not rule headings. Example: `### `[H-001]` Heterogeneous Councils` gives `H-001`; `### Unnumbered rule` gives `Unnumbered rule`.
- `load_snapshot` fills `loaded` with the rules and lessons that **made it into** the snapshot after trimming (a rule dropped to fit the size budget is not listed), each once, under its scope: `release`, `team`, `operator`, `workspace` for heuristics, and `operator-lessons`, `workspace-lessons` for the role's lessons files.

### 2. A `snapshot.loaded` event (`engine.rs`)

When a span opens, alongside its `snapshot.trim` events, log one event `snapshot.loaded` with payload `{"rules": [{"scope": ..., "id": ...}, ...]}` taken from the snapshot's `loaded`. One span writes one such event.

### 3. Task records (new file `crates/delphi-engine/src/learn.rs`, declared `pub mod learn;` in `lib.rs`)

A pure fold over a mission's events (`delphi_core::event::EventEnvelope`), no I/O:

- `pub enum Outcome { Done, Escalated, Failed, Cancelled, Open }` (derive `Debug, Clone, PartialEq, Eq`). `task.completed` with payload status `escalated` is `Escalated`, any other `task.completed` is `Done`; `task.failed` is `Failed`; `task.cancelled` is `Cancelled`; a task with none of these is `Open`.
- `pub struct TaskRecord { pub task_id: String, pub outcome: Outcome, pub rules: BTreeSet<(String, String)>, pub attempts: u32, pub denied_cards: u32, pub reverted_decisions: u32, pub cost_usd: f64 }` (derive `Debug, Clone, PartialEq`). `rules` is the set of `(scope, id)` pairs from every `snapshot.loaded` event of the task, each once however many spans carried it. `attempts` counts `build.attempt` events. `denied_cards` counts `approval.resolved` events whose payload `resolution` is `"denied"` (an approved card does not count). `reverted_decisions` counts `decision.reverted` events. `cost_usd` sums `agent.usage` payload `cost_usd`.
- `impl TaskRecord { pub fn first_pass(&self) -> bool }`: true when the outcome is `Done`, `attempts == 1`, `denied_cards == 0` and `reverted_decisions == 0`.
- `pub fn task_records(events: &[EventEnvelope]) -> Vec<TaskRecord>`: one record per task, in order of first appearance. Events with no `task_id` are skipped; tasks do not mix.

## Where

`crates/delphi-engine/src/workspace.rs`, `engine.rs`, `lib.rs`, and the new `learn.rs`. Put unit tests for `learn.rs` in a `#[cfg(test)] mod tests` in that file (your own tests will be replaced by the real ones when the work is checked).

## Notes

- The real tests are not visible to you. They exercise the names and the rules above, including a span that logs `snapshot.loaded` and `learn::task_records` reading it back.
- Run `cargo test -p delphi-engine --lib` to check you have not broken anything.
