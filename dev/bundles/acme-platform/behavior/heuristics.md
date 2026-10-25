---
id: acme-platform/behavior/heuristics
title: "Rules"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [rules]
summary: "One rule: migrations ship with a rollback."
---

## [T-acme-02] Migrations ship with a rollback
- **Trigger:** a change adds or alters a database migration.
- **Directive:** the same change includes the reverse migration and a note on data loss, or states why none is possible.
- **Falsification:** a merged migration with no reverse and no note.
