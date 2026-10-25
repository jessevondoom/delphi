---
id: acme-platform/architecture/service-mesh
title: "Service mesh conventions"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [architecture, networking]
summary: "How services find and call each other."
---

## Calling another service
Use the mesh name, never an address. Calls are mTLS by default and carry a deadline.

## Retries
Retry idempotent calls twice with jitter; never retry a payment capture.
