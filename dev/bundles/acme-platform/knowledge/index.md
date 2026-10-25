---
id: acme-platform/index
title: "Knowledge index"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [index]
summary: "Where to start in the Acme platform bundle."
---

- [Glossary](glossary.md): Words the platform team uses, defined once.
- [Platform architecture overview](architecture/overview.md): Three clusters, one control plane; services talk over mTLS; billing is the only stateful exception.
- [Service mesh conventions](architecture/service-mesh.md): How services find and call each other.
- [Rolling deploy](runbooks/rolling-deploy.md): Ship a service one cluster at a time.
- [Incident: the first 30 minutes](runbooks/incident-first-30-minutes.md): Who does what from the page to the first update.
- [0031: latency budget](decisions/0031-latency-budget.md): Checkout p99 stays under 400 ms.
- [0042: Postgres over Dynamo for billing](decisions/0042-postgres-over-dynamo.md): Why billing is on Postgres.
