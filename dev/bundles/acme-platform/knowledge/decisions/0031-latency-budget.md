---
id: acme-platform/decisions/0031-latency-budget
title: "0031: latency budget"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [decision, latency]
summary: "Checkout p99 stays under 400 ms."
---

## Decision
Checkout p99 latency stays under 400 ms.

## Why
Conversion drops 3% per 100 ms over 400.

## Consequences
Every service on the checkout path gets a per-hop budget.
