---
id: acme-platform/behavior/roles/architect
title: "Architect guidance"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [guidance]
summary: "How Acme wants specs drafted."
role: architect
---

When drafting a spec for an Acme platform service: name the goal it serves by id from goals.md; include a rollback section
(perspective P-acme-05); cite the relevant decision records by number instead of restating them; and list which of the
three clusters the change touches. Prefer the service-mesh conventions in knowledge/architecture/service-mesh.md over
new networking.
