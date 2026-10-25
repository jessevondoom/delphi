---
id: acme-platform/decisions/0042-postgres-over-dynamo
title: "0042: Postgres over Dynamo for billing"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [decision, billing]
summary: "Why billing is on Postgres."
---

## Decision
Billing runs on Postgres, not DynamoDB.

## Why
Billing needs multi-row transactions and ad hoc audit queries.

## Consequences
Billing is the one stateful service and fails over by promotion, not by redeploy.
