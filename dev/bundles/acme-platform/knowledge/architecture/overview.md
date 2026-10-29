---
id: acme-platform/architecture/overview
title: "Platform architecture overview"
author: operator
timestamp: "2026-09-12T10:00:00Z"
source_type: internal_generated
confidence: medium
status: active
source_mission: acme-platform
tags: [architecture, kubernetes]
summary: "Three clusters, one control plane; services talk over mTLS; billing is the only stateful exception."
sources: [oecd-2024-report]
pointers:
  - { scheme: https, locator: "https://wiki.acme.example/platform/arch", pin: { kind: none }, label: "Full architecture wiki", content_type: text/markdown }
---

## Shape
Three clusters share one control plane. Every service runs in all three.

## State
Services are stateless. Billing is the exception: it owns a Postgres primary in the east cluster.
