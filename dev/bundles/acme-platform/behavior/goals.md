---
id: acme-platform/behavior/goals
title: "Goals"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [goals]
summary: "This year's platform goals."
---

## [G-acme-01] Cut p99 checkout latency below 400 ms
- **Horizon:** 2026-Q4 to 2027-Q1
- **Owner role:** architect
- **Measure:** p99 of `checkout.complete` in the production dashboard, weekly
- **Status:** active
- **Why:** conversion drops 3% per 100 ms over 400 (see knowledge/decisions/0031-latency-budget.md)
