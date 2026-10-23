---
id: spec-008-ambient-watch-and-trajectories
title: "008: Zero-Token Ambient Watch, Time-Travel Rewind & Trajectory Distillation"
author: daedalus
timestamp: 2026-10-23T07:54:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
---

# Spec 008: Zero-Token Ambient Watch, Time-Travel Rewind & Trajectory Distillation

## 1. Scope & Objectives
This module emerged directly from benchmarking Delphi against Nous Research's `hermes-agent`, LangGraph, and 2025–2026 practitioner field reports:
1. **Zero-Token Ambient Watch ("No-Agent Mode" Triggers):** Allowing `delphi-core` to run scheduled or event-driven deterministic check scripts (`skills/*/scripts/`) at **zero LLM token cost**, waking **Athena** or **Hermes** *only* when an anomaly, CI failure, or new PR comment is detected.
2. **Coupled Time-Travel Rewind (`/rewind <seq>`):** Synchronizing `events.jsonl` sequence checkpoints with Git worktree refs so **Chaos** can rewind both agent cognitive state and filesystem state in a single command.
3. **3-Tier HITL Approval Batching:** Eliminating operator "Approval Blindness" by separating Green (auto-approved worktree actions), Amber (Spec-ratified capability envelopes), and Red (synchronous outbound/destructive gates).
4. **DPO / RL Trajectory Export (`delphi export-trajectories`):** Exporting multi-agent Council debates and bug-fix loops from `events.jsonl` into ShareGPT / DPO training pairs.

### 1.1 Simpler Alternative Considered (*The Icarus Middle-Course Rule*)
- **Alternative:** Have an LLM agent wake up on a cron timer every 5 minutes to poll GitHub CI or repository health.
- **Why Rejected:** Polling via an LLM burns hundreds of thousands of tokens per day on no-op checks. Running a deterministic script (`gh_sync.py --check-delta`) in "No-Agent Mode" costs $0.00 and only invokes an LLM when state actually changes.

---

## 2. Zero-Token Ambient Watch ("No-Agent Mode" Scheduler)

Inside `delphi-core`, the Watch Engine executes lightweight deterministic triggers defined in `.delphi/watches.yaml`:

```yaml
watches:
  - id: pr-ci-and-review-monitor
    interval_sec: 120
    no_agent_script: "skills/github-ops-sync/scripts/gh_sync.py --check-active-prs"
    wake_condition: "exit_code == 2" # Exit 0 = no change (0 tokens spent); Exit 2 = actionable event
    target_agent: athena
    routing_tier_override: fast
    action_prompt: "New CI failure or review comment detected on active PR. Triage and dispatch Hephaestus or Artemis."
```

---

## 3. Coupled Time-Travel Rewind (`/rewind`)

Every time **Hephaestus** completes a tool step or test cycle inside `.delphi/worktrees/<task-id>/`, `delphi-core` creates a lightweight hidden Git tree object (`git write-tree`) and records its SHA inside the `EventEnvelope` in `missions/<id>/events.jsonl`:

```json
{
  "seq": 1410,
  "type": "worktree.checkpoint",
  "agent": "hephaestus",
  "payload": {
    "worktree_id": "ho_0042",
    "git_tree_sha": "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
  }
}
```
When the operator runs `/rewind 1410` in **Chaos**, `delphi-core` truncates/forks the active turn history at `seq: 1410` AND restores the worktree working directory to `git_tree_sha` simultaneously.

---

## 4. Three-Tier HITL Permission Model (Anti-Fatigue)

To prevent the "Wiped Out by 11 AM" approval-blindness syndrome documented in parallel-agent field reports:

| Permission Tier | Scope | UX in `Chaos` Window |
| :--- | :--- | :--- |
| **Tier 0: Green (Auto-Allowed)** | Read-only tools (`Argos`, `Artemis`), Markdown writes to `missions/`, and test/build execution strictly inside `.delphi/worktrees/<task-id>/`. | Zero modal interruptions; narrated live in `Homer`. |
| **Tier 1: Amber (Spec-Ratified Envelope)** | Worktree creation, approved dependency installs, and local branch commits declared upfront in an `approved` Spec (`specs/NNN-*.md`). | Batch-approved once when **Chaos** ratifies the Stage 1 Spec. |
| **Tier 2: Red (Synchronous Sovereign Gate)** | Outbound external mutations via **Hermes** (`gh pr create/merge`, webhooks), edits to `GOVERNANCE.md`/`REVIEW.md`, or destructive host shell commands. | High-salience Red Modal in `Chaos` with diff preview and blast-radius badge. |

---

## 5. DPO / Fine-Tuning Trajectory Distillation (`delphi export-trajectories`)

Because Delphi's **Council Mode** naturally produces:
- **Rejected / Flawed Drafts:** Round 1 proposals that **Ares** or **Odysseus** proved vulnerable or out-of-scope.
- **Chosen / Verified Syntheses:** Round 3 Council Verdicts and **Hephaestus** PR commits that passed all 5 verification gates.

Running `delphi export-trajectories --format sharegpt|dpo` mines `missions/*/events.jsonl` to output clean `(prompt, chosen, rejected)` preference pairs—allowing operators to fine-tune local/open-weights models on their own Pantheon's best reasoning traces.

---

## 6. Verification Plan
1. **Zero-Token Watch Test:** Run a watch trigger for 10 cycles with exit code `0`; verify `0` LLM API calls are made until exit code `2` is simulated.
2. **Time-Travel Rewind Test:** Mutate 3 files in a worktree across `seq: 10..20`, execute `/rewind 12`, and verify both the conversation state and the worktree files match `seq: 12` byte-for-byte.
