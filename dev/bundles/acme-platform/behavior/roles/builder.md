---
id: acme-platform/behavior/roles/builder
title: "Builder guidance"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [guidance]
summary: "How Acme wants changes built."
role: builder
---

When implementing a change on an Acme service: add the reverse of any migration in the same change; keep calls inside the
service mesh and give each one a deadline; and run the service's own test target before proposing the change.
