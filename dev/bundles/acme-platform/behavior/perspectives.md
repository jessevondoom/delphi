---
id: acme-platform/behavior/perspectives
title: "Perspectives"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [perspectives]
summary: "How the platform team weighs choices."
---

## [P-acme-03] Boring technology by default
- **Statement:** choose the well-understood option unless it is measurably the bottleneck.
- **Apply when:** picking a datastore, queue, framework or language for a new service.
- **Counter-case:** the boring option cannot meet a stated goal's measure (cite the goal).

## [P-acme-05] Every change has a way back
- **Statement:** a change ships with its rollback, or with a written reason there is none.
- **Apply when:** a change touches data, schema or traffic routing.
- **Counter-case:** the rollback costs more than the failure it prevents (say so in the change).
