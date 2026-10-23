# Delphi on One Page

> Status: v0.4 design, amended by [Council 001](missions/2026-10-23-foundational-design/councils/001-spec-suite-red-team.md) (Claude and Gemini seats). Nothing is built yet. **(D-nn)** marks a decision ratified 2026-10-23 in [`decisions.md`](missions/2026-10-23-foundational-design/decisions.md). Build order: [roadmap](missions/2026-10-23-foundational-design/roadmap.md).

## What it is
Delphi is a terminal-first, multi-agent work environment for software engineering. It is written in Rust as a single binary. You (the operator, the **operator**) give it intent. **Orchestrator** breaks the work down and routes it to specialist agents, and nothing irreversible happens without your approval. v1 targets code repos on macOS and Linux (D-13, D-15).

## Architecture
```
 console ──┐        delphi-core daemon (from Step 2, D-05)
 (input, approvals)├─ UDS ─┤  ├── Claude driver ─┐
 narrator window ──┘        │  ├── Gemini driver ─┴─ Adapter IPR compiler (dialects, signatures)
 (live narration)        │  ├── sandboxed worktrees (D-04)
                         │  ├── events.jsonl   ← single source of truth (agents can't read it)
                         │  └── state.db       ← derived index: FTS5 search, cost ledger (D-01)
 $DELPHI_HOME/workspaces/<name>/  ← all Delphi state; project repos stay clean (D-03)
```
- **One runtime owns everything.** `delphi-core` is the only writer of the event log and index. It runs the task DAG and the Council, and it enforces every security invariant in code ([enforcement map](context/invariant-enforcement.md)). It is a small local daemon from Step 2, so the operator and Narrator can both attach and Narrator streams live (D-05).
- **Integrator (the provider layer)** compiles portable agent and skill Markdown into each provider's native format. It keeps cryptographic reasoning signatures intact across tool loops and backs off on rate limits before failing over. Model *tiers* (`fast` / `workhorse` / `frontier`) map to concrete models in config.
- **Roles are canonical; names are a theme (D-17).** Specs, config and files refer to `orchestrator`, `builder`, `architect`, `red-team` and so on. A per-install theme pack (Greek by default: Athena, Hephaestus, …; console "Chaos", narrator "Homer") sets display names and a short voice line. You approve a theme before it loads, and it can't change behavior.
- **Agents are data.** Each agent is an `agents/<id>/AGENT.md` with write globs and allowed tools; each skill is a `SKILL.md` that loads only when used. The v1 roster is Orchestrator, Architect, Builder, Red-Team and Strategist (D-07).
- **Three escalation modes:**
  - **Direct:** one specialist.
  - **DAG:** discover → spec → build → audit.
  - **Council:** 3–5 agents on different model families. Round 1 is blind, Round 2 is cross-examination, then a verdict whose Minority Report the daemon copies verbatim from the critics.
- **Files are owned by zones:**
  - Zone 1 (core): ships in the binary and is 3-way merged on update.
  - Zone 2 (team): Git-shared.
  - Zone 3 (private): never touched by update or sync, and gitignored.
  - Delphi is an installed tool: all zones live in its own home (`$DELPHI_HOME`, default `~/.delphi`), never in a project repo. This repo is just the source the install is built from (D-03).

## Trust model (the part that must not break)
- **Taint is computed, not declared.** Every agent span carries the lowest trust tier of anything it read: `external_untrusted` < `federated_peer` < `federated_verified` < `internal_generated` < `internal_empirical` < `user_ratified`.
- **Taint limits actions.** A span that has read untrusted content cannot write, execute, call out, or dispatch without a Red approval.
- **Three approval tiers:**
  - 🟢 **Green:** reads, mission notes, and tests inside an OS sandbox. Runs without asking.
  - 🟠 **Amber:** commits, pinned installs, and unsandboxed runs. Approved once when you ratify the spec.
  - 🔴 **Red:** anything outbound, merges, new dependencies, constitution edits, and destructive commands. Always asks; denied if nobody answers.
- **Worktrees are not sandboxes.** Execution is sandboxed (D-04), and agents can never write `.git/` (no hook planting).

## The learning loop
1. **During a mission**, a failed check climbs a fixed ladder: upscale the model, then reset the context on the other provider family, then **escalate to you**. Spending is capped: $5 per mission and $20 per Council by default (D-10).
2. **At `/retro`**, Orchestrator distills four kinds of lesson. All of them are **proposals**:
   - escaped bug → a heuristic **and** a review check;
   - your "by design" ruling → a scoped suppression;
   - a rule violated twice → a deterministic script;
   - role coaching → that agent's `lessons.md`.
3. **You ratify** the proposals in one batch. Only `internal_empirical` (test-verified) or higher content can be promoted, and nothing, agent lessons included, enters an always-loaded prompt without your ratification (D-11).
4. **Memory stays small.** At most 25 rules per always-loaded file, under 6 KB, at most 350 characters per rule (D-09). Overflow means merge, demote to `context/domains/`, or promote to a script.
5. **Each task starts from a frozen snapshot** of those files, so mid-task learning never invalidates the prompt cache.

## How you operate it
| You want to… | You do | Delphi does |
| :--- | :--- | :--- |
| Ask or fix something small | Type in the operator, or `@builder …` | Direct or short DAG; diffs appear in Narrator |
| Build a feature | Describe it | Architect drafts a spec, Red-Team/Strategist review it, **you approve**; then a sandboxed TDD build and the 5 merge gates, ending with **your** merge |
| Settle a hard trade-off | `/council "<question>"` | Two rounds, then a verdict with Minority Report, or a structured decision for you |
| Control cost or risk | `/budget`, `/permissions`, `/scale` | Changes ceilings, Amber handling, or model tiers. Red-tier always asks |
| Undo agent work | `/rewind <seq>` *(Step 6)* | Forks the history and restores the worktree; refuses if external side effects would remain |
| Improve the team | `/retro` | Shows the staged lessons for one batch ratification |
| Check health | `delphi doctor` | Schemas, taint, caps, links, sandbox, gitignore, provider keys |

**Deferred from v1 (D-06):** federation (A2A and `delphi://` pointers), the OpenAI/local drivers, the CLI harness drivers, and multi-terminal auto-spawn. These specs remain as designs. DPO export was removed (D-14).
