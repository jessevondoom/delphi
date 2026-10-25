---
id: acme-platform/runbooks/rolling-deploy
title: "Rolling deploy"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [runbook, deploy]
summary: "Ship a service one cluster at a time."
---

## Steps
1. Deploy to the edge cluster and watch the error budget for ten minutes.
2. Deploy to west, then east.
3. Never deploy on Fridays.
