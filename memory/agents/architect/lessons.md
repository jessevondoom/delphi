---
id: lessons-architect
title: "Architect Private Architecture & Spec Ledger"
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
tags: [lessons, architect]
---

# Architect — Private Lessons Learned (`memory/agents/architect/lessons.md`)
> **Scope:** Role-specific specification authoring, schema design, and Mermaid diagram heuristics. Capped at $\le 25$ active rules and $\le 6\text{ KB}$ per [`specs/004-memory-and-coaching.md`](../../../specs/004-memory-and-coaching.md).

### `[L-ARC-001]` Terminal Monospace Font Immunity
- **T:** Designing terminal sprites or TUI widgets.
- **D:** Ban vertical half-blocks (`▌`, `▐`) for frames; font margins create gaps. Use cell background color with `█`/`▄`/`▀` for gapless geometry. Keep row width invariant.
- **F:** Stepped diamond distortion and cross-font tearing (`specs/005a`).
