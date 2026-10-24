---
id: council-008-round1-red-team
title: "Council 008 Round 1: red-team seat (blind, verbatim)"
author: red-team
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-008, verbatim]
model: "gemini-3.8-flash-high via agy"
---

# Delphi Council 008, Round 1 (Blind): Red-Team Review

## 1. Findings

- **[P0-BLOCKING]** `specs/011-xenia-trust-model.md#L73-L75`, `specs/011-xenia-trust-model.md#L178-L181`, `specs/012-xenia-record-schema.md#L93-L102` — *Unauthenticated `signed_at` Enables Indefinite Key Revocation Bypass*
  - **Failure Mode:** `signed_at` is self-asserted by the author. Under §3.3.2 and §7.3, records signed prior to `compromised_since` stay valid. An attacker holding a compromised key (or a removed member) can forge new malicious records at any time by backdating `signed_at` prior to `compromised_since` and asserting an earlier `roster_epoch`. Because signatures cover only `xenia/1\n<hash>\n<roster_epoch>\n<signed_at>` with no timestamping authority or ledger proof, receiving daemons accept the forged records as valid, completely defeating revocation. Furthermore, §7.1 ("all of that member's records drop to $L_0$") directly contradicts §7.3 and §3.3.2.
  - **Remediation:** Remove reliance on self-asserted `signed_at` for compromise boundaries. Once a key is marked compromised or removed in roster epoch $E$, all records from that key not already recorded in the local index prior to syncing epoch $E$ must be quarantined ($L_0$) or require steward re-endorsement. Reject any record whose `roster_epoch` lags the store's current epoch by $>1$.

- **[P0-BLOCKING]** `specs/011-xenia-trust-model.md#L127-L128`, `specs/010-connectors-and-tool-access.md#L77`, `specs/001-governance-and-manifests.md#L220-L224` — *Pointer Pin Taint Laundering Overrides Connector & Lattice Invariants*
  - **Failure Mode:** Spec 011 §5.2 states that "content matching the pin inherits the record's tier ($L_2$)". If an endorsed record points to Google Drive or an external repo, resolving it causes external content to enter the agent context at $L_2$ (`federated_verified`). This directly violates `specs/010` §5.1 (all connector content is strictly `external_untrusted` $L_0$) and `specs/001` §5.1. An attacker can place prompt injections in a Google Doc, obtain steward endorsement on a benign summary, and victim agents resolving the pointer will ingest the document at $L_2$, retaining worktree write and code execution tools (`specs/001` §5.2) without tripping taint gates.
  - **Remediation:** Pointer targets must never inherit the record's tier. Strike line 127 in spec 011. Mandate that fetched pointer content is stamped at its native origin tier per `specs/001` §5.1 and `specs/010` §5.1 (`drive` and `https` are always $L_0$; external git is $L_0$). Only the signed summary itself may enter at $L_2$.

- **[P0-BLOCKING]** `specs/012-xenia-record-schema.md#L46`, `specs/012-xenia-record-schema.md#L99`, `specs/011-xenia-trust-model.md#L115-L118` — *Missing Store/Audience Domain Separation in Detached Signatures Allows Cross-Store Replay*
  - **Failure Mode:** Spec 011 §4.4 states separate audiences (e.g. security team vs org-wide) get separate stores within one org. However, the record envelope only records `org` (spec 012 §2 L46) and the signature domain is `xenia/1\n<hash>\n<roster_epoch>\n<signed_at>` (spec 012 §3 L99). Neither binds the store/audience ID. An attacker with read access to a confidential store can copy any signed `.md` and `.sig.json` pair directly into an org-wide store; receiving clients verify the signature against the org roster and ingest the confidential record, destroying audience separation.
  - **Remediation:** Add `store_id: string` to the record frontmatter and update the signature domain in spec 012 §3 to bind both org and store: `xenia/1\n<org>\n<store_id>\n<hash>\n<roster_epoch>\n<signed_at>`. Verify that `store_id` matches `org.toml` upon ingestion.

- **[P0-BLOCKING]** `specs/011-xenia-trust-model.md#L89`, `specs/012-xenia-record-schema.md#L65`, `specs/012-xenia-record-schema.md#L143` — *Unverifiable Evidence Pointers Blindly Grant $L_2$ Status (Enforcement Without Mechanism)*
  - **Failure Mode:** Spec 011 §4.1 awards $L_2$ to any member-signed record carrying a "verifiable evidence pointer... that the install can check". Neither spec defines an attestation format, cryptographic proof schema (e.g. in-toto/Cosign), or deterministic check in `delphi-core`. Spec 012 §4 defines only generic pointer schemes (`git`, `drive`, `https`), and §5.1 marks `verified_by` as informational. Without an implemented verification engine, any member can append an unverified HTTPS URL to `evidence` to launder untrusted data into $L_2$ without steward review.
  - **Remediation:** Strike evidence pointers as an autonomous path to $L_2$ in v1. Require steward endorsement for all $L_2$ promotions (settling O-1). Re-introduce evidence-based $L_2$ only when a signed attestation specification is defined.

- **[P1-MAJOR]** `specs/011-xenia-trust-model.md#L66-L68`, `specs/012-xenia-record-schema.md#L247-L249` — *Single-File Roster & Unchained Quorums Brick Offline Installs and Deadlock 2-Admin Orgs*
  - **Failure Mode:** Spec 011 §3.2 requires roster successors to be signed by a quorum of the *last roster this install accepted*. If an install is offline across two admin rotations ($E_1 \to E_2 \to E_3$), it receives only `roster/members.json` at $E_3$ signed by $E_3$ admins, who lack quorum in $E_1$. Because intermediate signed rosters are overwritten in git, the install cannot verify the quorum chain and permanently rejects updates. Additionally, for $n_{admin} = 2$, quorum is $\lceil 3/2 \rceil = 2$ (2-of-2), meaning a single lost key permanently deadlocks the roster.
  - **Remediation:** Store roster epochs as an immutable chain (`roster/epochs/<epoch>.json` and `.sigs.json`), and require `delphi-core` to verify the transition chain from the locally pinned epoch to the target epoch. Mandate $n_{admin} \ge 3$ (2-of-3 quorum) for production orgs.

- **[P1-MAJOR]** `specs/011-xenia-trust-model.md#L52`, `specs/011-xenia-trust-model.md#L171`, `specs/012-xenia-record-schema.md#L214-L220` — *Unscoped, Irreversible Steward Tombstones Allow Unilateral Denial of Service*
  - **Failure Mode:** Spec 011 §2 and Spec 012 §5.6 permit any steward to sign a `tombstone` for any record `<id>`. Unlike endorsements, tombstones have no `scope` restriction. A single compromised steward can tombstone every record in the store. Because Spec 012 §2.1 rules that "nothing leaves withdrawn", there is no recovery mechanism, permanently hiding all org knowledge.
  - **Remediation:** Bound steward tombstones to their assigned `steward_areas`. Provide an admin-quorum override mechanism to re-activate erroneously tombstoned records.

- **[P1-MAJOR]** `specs/011-xenia-trust-model.md#L148-L155`, `specs/012-xenia-record-schema.md#L150`, `REVIEW.md#L20` — *Skill Namespace Collision & Unreviewed Executable Scripts on Adoption*
  - **Failure Mode:** Spec 012 §5.2 defines `skill_name` as an unnamespaced string `[a-z0-9-]{1,48}` installed into `skills/` (spec 011 §6.3 L153). An adopted skill can collide with and overwrite core built-ins (e.g. `review-md-audit`), violating `REVIEW.md` `[REV-SEC-02]`. Additionally, `manifest_hash` covers only `SKILL.md` frontmatter; adoption diffs show only requested access scopes, hiding executable scripts in `scripts/` from operator review.
  - **Remediation:** Namespace adopted skills strictly to `skills/custom/xenia/<org>/<skill_name>/`. Forbid collisions with built-in skills. Require `delphi xenia adopt` to display a full diff of all files in the git tree during ratification.

- **[P2-MINOR]** `specs/012-xenia-record-schema.md#L225-L238` — *Deep Parsing Unsigned YAML Frontmatter Prior to Signature Verification*
  - **Failure Mode:** Spec 012 §6 validates YAML frontmatter, strings, counts, and dates (steps 2–8) before signature verification (step 9), exposing the daemon parser to malformed payloads from unauthenticated sources.
  - **Remediation:** Validate raw byte size (step 1), verify detached BLAKE3 hash and Ed25519 signature (step 2), and parse YAML/Markdown only after signature verification passes.

- **[P2-MINOR]** `specs/011-xenia-trust-model.md#L128-L129` — *Automatic Ingestion of Drifted Pointer Targets as $L_0$ Enables Taint-Trapping DoS*
  - **Failure Mode:** Ingesting drifted pointer targets automatically at $L_0$ taints the calling agent span to $L_0$, immediately revoking worktree write tools (`specs/001` §5.2) and aborting the task.
  - **Remediation:** Fail closed with an integrity error on pointer pin mismatch; never feed drifted content to an active agent span without explicit operator approval.

- **[P2-MINOR]** `specs/011-xenia-trust-model.md#L161-L162`, `specs/012-xenia-record-schema.md#L191-L199` — *Unbounded Claim Records Enable Inbox Flooding DoS*
  - **Failure Mode:** Spec 011 §6.4 bounds inbound requests but imposes no rate limits on `claim` records. A malicious member can flood hundreds of claim records to pollute requester inboxes.
  - **Remediation:** Enforce per-member open claim quotas (max 10 active claims) deterministically upon sync.

---

## 2. Checked and Sound
1. Detached raw-byte sidecar signatures eliminate canonicalization exploit surfaces (`specs/012` §1.2, §3).
2. Strict record size bounds (16 KiB total, 500-char summary, 4 KiB body) protect agent context hygiene (`specs/012` §6).
3. Publisher-side outbound gate with path/secret scanning and zero-model-turn exact-byte preview preserves operator sovereignty (`specs/011` §6.1).
4. Operator-scope memory isolation is maintained: private memory is never published (`specs/011` §6.1, `specs/004` §5.7).
5. Requests and claims remain inert data without triggering autonomous agent dispatch loops (`specs/011` §6.4).
6. Librarian agent is non-authoritative: zero signing keys and zero endorsement authority (`specs/011` §6.5).

---

## 3. Positions on Open Decisions

- **O-1 (Steward Endorsement for $L_2$):** Approve ONLY for summary text of `memory` and `progress`. Strongly REJECT pointer targets inheriting $L_2$; external targets must retain native connector taint ($L_0$ for Drive/HTTPS per `specs/010` §5.1). Skills always require local operator ratification to $L_5$.
- **O-2 (Store Audience Scope):** Concur with store-per-audience, conditioned on binding `store_id` into record envelopes and signatures to prevent cross-store replay.
- **O-3 (Admin Quorum & Steward Assignment):** Mandate $n_{admin} \ge 3$ (2-of-3 quorum) to prevent 2-of-2 single-key deadlock. Bound steward endorsements and tombstones strictly to assigned topic areas.
- **O-4 (Org Join Pinning):** Concur with both admin-signed invite and out-of-band admin fingerprint confirmation.
- **O-5 (Xenia Pre-Federation):** Concur: asynchronous, git-backed Plane 3 sync is independent of deferred Planes 1–2 live transports (D-06).
- **S-1 (Record Format & Signatures):** Concur with Markdown+YAML and detached raw-byte signatures; require signature verification before YAML parsing.
- **S-2 (Progress Records):** Concur with separate, expiring records; rolling records invite git merge races and signature invalidation.
- **S-3 (Size Caps):** Concur with strict 16 KiB total and 500-char summary caps.
- **S-4 (Skill Storage):** Concur with git tree pointers; require namespace isolation under `skills/custom/xenia/<org>/` and full script diff reviews.
- **S-5 (Claim Visibility):** Concur with public claims in v1; add per-member open claim rate limits.

---

## 4. Verdicts

- `specs/011-xenia-trust-model.md`: **reject**
- `specs/012-xenia-record-schema.md`: **reject**
- `concur: false` (Four unresolved P0 security vulnerabilities: unanchored `signed_at` revoking bypass, pointer pin taint laundering, cross-store replay, and unverifiable evidence pointers).
