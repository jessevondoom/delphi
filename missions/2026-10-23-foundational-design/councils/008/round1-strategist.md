---
id: council-008-round1-strategist
title: "Council 008 Round 1: strategist seat (blind, verbatim; degraded)"
author: strategist
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: low
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-008, verbatim]
model: "gpt-oss-120b-medium via agy"
---

> **Orchestrator note.** This seat did not answer the assigned focus (scope, sequencing, relation to spec 007) and replied to open decisions that do not exist (its "O-2 to O-5" and "S-1 to S-5" restate invented questions). It also issued verdicts on unrelated files. Usable points were verified against the specs before acting: enumerate secret-scan canaries (adopted as test 20), recompute tiers on roster change (adopted as an explicit invariant), a single-admin roster is risky (adopted as a `doctor` warning). Its P0 requiring endorsement **and** evidence for L2 was rejected (see spec 011 §4.2). Round 2 re-asks the focus questions with the real decision lists quoted.

## Findings  

| Severity | File & Lines | Rule / Ref | Failure Mode | Remediation |
|----------|--------------|------------|--------------|-------------|
| **P1‑MAJOR** | `specs/011-xenia-trust-model.md#L66-L68` | [REV‑SEC‑06] | Quorum rule permits a **single‑admin org**.  If that admin’s key is compromised, an attacker can add/remove members without any additional check, violating the “least‑privilege” invariant in GOVERNANCE §5.1. | Enforce a minimum of **two admins** for any org (reject one‑admin rosters) or require a *second‑factor* (e.g., out‑of‑band verification) for admin changes. |
| **P0‑BLOCKING** | `specs/011-xenia-trust-model.md#L98-L105` | Open‑Decision O‑1 | **Steward endorsement alone** upgrades a record to **$L_2$** without any verifiable evidence.  A compromised steward can thus unconditionally raise the taint ceiling, breaking the taint‑gated tool gate (`specs/001` §5.2). | Require **both** a steward endorsement **and** a verifiable evidence pointer (CI run, signed test artefact) for $L_2$ promotion of *memory* and *progress* records. |
| **P1‑MAJOR** | `specs/011-xenia-trust-model.md#L70-L78` | [REV‑SEC‑07] | “Invalid records are quarantined, never loaded, and listed by `delphi xenia status`.”  The spec does **not** mandate that a *cached* $L_2$ record **immediately** be re‑computed to $L_0$ after the author’s roster entry is removed (revocation lag). | Add a **revocation‑propagation step** in the sync pipeline that recomputes tiers for all cached records when a roster change is applied. |
| **P2‑MINOR** | `specs/012-xenia-record-schema.md#L230-L233` | [REV‑ARCH‑03] | Front‑matter validation rejects unknown keys, but the schema **does not forbid** a field named `tier` or `source_type` (these are caught later by the validator). The rule text says “any attempt … is rejected,” yet the implementation description does not explicitly list those keys. | Explicitly list `tier`, `source_type`, `endorsed` in the **disallowed‑keys** array and add unit tests for them. |
| **P1‑MAJOR** | `specs/012-xenia-record-schema.md#L127‑L129` | [REV‑SEC‑01] | “Never publishable: `file:` … tokens …” – but the *pointer* schema still allows **`scheme: https`** with `pin.kind: none`.  An `https` pointer without a pin is automatically $L_0$, yet a malicious actor could embed a **phishing URL** that points to a malicious site and rely on the operator’s “quarantine read” to open it. | Disallow **any** `https` pointers without a **cryptographic pin** (`content‑hash` or `drive‑revision`).  Enforce a **URL‑sanitizer** that rejects known phishing patterns. |
| **P0‑BLOCKING** | `specs/011-xenia-trust-model.md#L184‑L193` | Threat T5, T12 | Threat table lists “Summary lies about target” (T5) and “Sensitive detail in a summary visible to the whole store” (T12) but **does not provide** an automated detection mechanism; it relies solely on steward review, which is a single point of failure. | Introduce a **static‑analysis scanner** that flags summaries containing URLs, high‑entropy strings, or suspicious phrasing, and surface these as **Red‑Tier findings** before publishing. |
| **P2‑MINOR** | `specs/007-federated-mesh-and-sync.md#L108‑L117` | [REV‑SEC‑06] | The taint lattice definition duplicates the one in `specs/001`; however the spec does **not** state that the lattice enforcement is *computed* by `delphi‑core` for Xenia records.  This could lead to an implementation that treats `federated_peer` as acceptable for always‑loaded files. | Add a cross‑reference: “Xenia records are subject to the same **computed taint** logic as all other artefacts (see spec 001 §5.2).” |
| **P1‑MAJOR** | `specs/011-xenia-trust-model.md#L126‑L130` | [REV‑SEC‑05] | Publishing gate “runs the `[REV‑SEC‑05]` path and secret scan” but **does not** enumerate which secret patterns are rejected (e.g., API keys, OAuth tokens).  Without a concrete list, implementations may miss secrets. | Provide an **enumerated secret‑pattern list** (e.g., `AKIA*`, `ghp_`, `sk_live_*`) and require lint tests to verify that the scanner matches them. |
| **P2‑MINOR** | `specs/012-xenia-record-schema.md#L290‑L300` | Verification Plan | The verification plan lists four tests but omits a test for **pointer‑scheme allowlist** (ensuring only the four schemes are accepted). | Add a fifth test: *Reject records with a pointer scheme outside {git, drive, https, xenia}*. |

### Checked and sound (max 6 lines)

- Governance & Review invariants are consistently referenced.  
- Path‑sandboxing (spec 001 §2.2) aligns with Xenia’s publish gate (spec 011 §6.1).  
- Signature domain (`xenia/1`) and detached‑sidecar format (spec 012 §3) match the verifier description (spec 011 §3.3).  
- Taint‑lattice mapping (spec 011 §4.1) correctly uses the lattice from spec 001.  
- Record size, pointer limits, and YAML sanitisation are enforced by the validator (spec 012 §6).  
- Threat table covers the most severe attacks but omits detection for malformed URLs.

## Open‑decision positions  

| Decision | Position (justify) |
|----------|-------------------|
| **O‑1** (Steward endorsement vs. evidence for $L_2$) | **Reject** – endorsement alone is insufficient; adopt the remediation in the P0 finding (require verifiable evidence). |
| **O‑2** (Whether roster quorum should require *two* admins) | **Approve** – enforce a minimum of two admins; the current “one‑admin allowed” is a safety hole (see P1). |
| **O‑3** (Whether a member‑removed record should be instantly re‑tainted) | **Approve** – add revocation‑propagation step (P0). |
| **O‑4** (Whether the threat table should list a “key‑compromise detection delay” threat) | **Approve** – add T15 “Delayed revocation of compromised keys” with mitigation: immediate sync‑triggered re‑taint. |
| **O‑5** (Whether Xenia should support *mutable* pointers with `pin.kind: none`) | **Reject** – disallow `pin.kind: none` for any scheme; all published pointers must be cryptographically bound. |

| Decision | Position (justify) |
|----------|-------------------|
| **S‑1** (Record schema versioning policy) | **Approve** – current major‑version check is sound (line 289). |
| **S‑2** (Limits on pointers / evidence counts) | **Approve** – limits (≤ 8 pointers, ≤ 4 evidence) are enforced (line 233). |
| **S‑3** (Signature domain binding) | **Approve** – domain prefix (`xenia/1`) prevents cross‑protocol replay (lines 99‑100). |
| **S‑4** (Pointer scheme allowlist) | **Approve** – scheme list is explicit (line 108). |
| **S‑5** (Store layout privacy claim) | **Approve** – store contains no operator‑scope data (line 263). |

## Verdict  

| Spec | Verdict | Concur? | Reason |
|------|---------|---------|--------|
| `specs/011-xenia-trust-model.md` | **approve‑with‑fixes** | **false** | Open decisions O‑1, O‑2, O‑3, O‑5 need changes; P1‑major issues (single‑admin quorum, steward‑only endorsement) must be remedied. |
| `specs/012-xenia-record-schema.md` | **approve‑with‑fixes** | **false** | P2‑minor issues (disallowed‑key list, https‑pin requirement) and missing secret‑pattern list must be added. |
| `specs/001-governance-and-manifests.md` | **approve** | **true** | No unresolved P0 findings; definitions align with Xenia specs. |
| `specs/007-federated-mesh-and-sync.md` | **approve** | **true** | Correctly references taint lattice; only cross‑reference addition needed (P2). |
| `REVIEW.md` | **approve** | **true** | Review rubric matches required output format. |

### Summary  

The Xenia design is fundamentally sound and respects the overall Delphi security architecture, but **critical hardening** is required before the specs can be ratified:

1. **Enforce multi‑admin rosters** and **evidence‑backed steward endorsements** (O‑1, O‑2).  
2. **Add automatic revocation propagation** for roster changes (P0).  
3. **Strengthen pointer security** – disallow unpinned `https` pointers and require explicit pinning for all schemes (P1).  
4. **Complete secret‑scan definition** and **explicitly forbid unknown taint‑tier fields** (P1‑major, P2‑minor).  
5. **Expand the threat table** to cover delayed key‑compromise detection (O‑4).  

Once these fixes are incorporated, the specs can move to **approve** status and proceed to implementation.
