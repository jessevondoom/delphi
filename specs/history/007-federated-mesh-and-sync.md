---
id: spec-007-history
title: "007: amendment history (non-normative)"
author: architect
timestamp: 2026-10-30T00:00:00Z
governed_by: "GOVERNANCE.md"
source_type: internal_generated
confidence: medium
status: draft   # non-normative history; the approved text is specs/007
source_mission: 2026-10-23-foundational-design
tags: [spec-history]
---

# Spec 007: amendment history (non-normative)

This file records text removed from `specs/007-federated-mesh-and-sync.md` when it was folded on 2026-10-30; it is history only, and the spec is the normative text. Removed text is quoted verbatim.

## Banner: amendment list
> **Amendments (all ratified by the operator; folded into the text below, so this spec alone is complete):** 007-A1 (2026-10-25, from `specs/011` §5.1, §3), 007-A2 (2026-10-25, §3 and §4 item 4), 007-A3 (2026-10-29, §2.2, D-32 in `missions/2026-10-23-foundational-design/decisions.md`), 007-A4 (2026-10-29, bundle-vouched content, §3, `specs/024`, `specs/052`, `specs/055`), 007-A5 (2026-10-29, scope, below).

## Banner: status
> **Status (D-06, ratified 2026-10-23):** Planes 1 and 2 are **deferred past v1**. They are the largest attack surface and serve the fewest users, and Plane 3 (plain Git plus ratified rule proposals) covers team sharing in the meantime. The security amendments below are binding whenever these planes ship.

## Banner: scope (007-A5)
> **Scope (amendment 007-A5, ratified by the operator 2026-10-29): Planes 1 and 2 are a design reservation.** They are kept so the security rules are written down, and are revived only if a team needs **live cross-install delegation** that Xenia's asynchronous model cannot serve. Plane 3's aspiration, sharing knowledge between installs, is served by **Xenia** (`specs/011`, `specs/012`) and **bundles** (`specs/023`–`027`). The Xenia `fact` kind stays **deferred** until `specs/040` V2 has real audit data (`specs/055` SK7, `specs/012` §11). Nothing here authorizes building Plane 1 or 2.

## Amendment banner in §2.2 (007-A3)
> **Amendment 007-A3 (ratified 2026-10-29, D-32; text changed by #354).** The scheme is `xenia://` (named for the protocol), replacing `delphi://`, which is accepted nowhere. Plane 2 uses only the reserved authority `cap`. It is **visibly distinct from record references** (`xenia://<kind>/<id>[@rev]`, `specs/012` §4): `cap` is never a record kind, Phase 1 parsers reject `xenia://cap/...` as a locator ("reserved, not built"), and sharing a scheme shares no resolver, trust tier or authority. The name may be revisited while Plane 2 is unbuilt.

Original §2.2 lead-in sentence (superseded by the folded text):

To share context via pointer without copying huge files or granting full repo access, Delphi defines the **reserved `xenia://cap/` URI authority** (named for the protocol, decided 2026-10-29; `cap` is never a record kind and Phase 1 parsers reject it, `specs/012` §4):

## §3 table row for L2 (007-A1, 007-A4)
| **$L_2$** | `federated_verified` | Signed by a pinned teammate DID + includes verifiable git commit & CI test attestation. **Or** (amendment 007-A1, ratified 2026-10-25 with `specs/011`; its text is here and in `specs/011` §5.1): a xenia `memory` record validly endorsed by a steward other than its author's person, under a current freshness statement (`specs/011` §4 and §5.1). **Or** (amendment 007-A4, 2026-10-29): content vouched by an added bundle, a second route that is separate from steward endorsement (see below). | **No** (Allowed in `context/domains/`; requires local test replay or local the **operator** ratification to promote to $L_4/L_5$). |

## §3 routes paragraph
**Three routes to $L_2$, none implying another (007-A1, 007-A4).** (1) A signed teammate DID with a git commit and CI attestation. (2) A steward-endorsed Xenia `memory` record. (3) **Bundle-vouched content:** knowledge files of a bundle the operator added are indexed as `federated_verified` by the `bundles` zone, whatever their frontmatter says, and shown as `validated · vouched by <org>` (`specs/024` §3.2 and the validated-sources default, `specs/052` §3.1, `specs/055` §11.1). It needs no steward endorsement, and endorsing a record gives a bundle nothing. A bundle's own `source_type` or `trust` claim never raises it; only the operator's per-file promotion ($L_4$/$L_5$) or the typed-name raise of `specs/024` does. At $L_2$ it stays data: never in `heuristics.md`, `REVIEW.md`, `lessons.md` or `skills/`, and it grants no permission.

## §4 item 4 (007-A2)
4. Non-prompt Zone 2 files (`context/domains/**`, `specs/**`) merge with ordinary Git semantics, stamped `federated_peer` ($L_1$) in frontmatter. **Amendment 007-A2 (ratified by the operator, 2026-10-25):** nothing in Plane 3 verifies a signature, so a plain merge cannot earn `federated_verified`. Shared knowledge that should reach $L_2$ travels through Xenia (`specs/011`, `specs/012`), where a signed record needs a steward's endorsement under a current freshness statement. `delphi sync` keeps items 1–3 (rule proposals, which all end in operator ratification) and this item at $L_1$.
