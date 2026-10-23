---
id: lessons-builder
title: "Builder Private Toolchain & Forge Ledger"
author: orchestrator
timestamp: 2026-10-23T17:15:00Z
source_type: internal_empirical
confidence: high
status: active
max_rules: 25
max_bytes: 6144
max_rule_chars: 350
active_rules: 1
source_mission: 2026-10-23-foundational-design
tags: [lessons, builder]
---

# Builder — Private Lessons Learned (`memory/agents/builder/lessons.md`)
> **Scope:** Role-specific compiler, test-runner, and Git worktree heuristics. Capped at $\le 25$ active rules and $\le 6\text{ KB}$ per [`specs/004-memory-and-coaching.md`](../../../specs/004-memory-and-coaching.md).

### `[L-BLD-001]` TUI State Animation & Blink Anchors
- **T:** Implementing stateful terminal avatars.
- **D:** Anchor mouth vertical alignment during blinks (`─` vs `_` must not jump). Keep dual scales (5-char badge `⟦•_•⟧` vs 7-col mascot ` x ? x `) in exact sync with zero heap allocation.
- **F:** Distracting vertical mouth hopping during 60Hz redraws (`specs/005a`).
