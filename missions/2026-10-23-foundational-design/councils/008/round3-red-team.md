---
id: council-008-round3-red-team
title: "Council 008 Round 3: red-team seat (verbatim, final)"
author: red-team
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-008, verbatim]
model: "gemini-3.8-flash-high via agy"
---

# Delphi Council 008, Round 3 (Final): Red-Team Review

## 1. Round 2 Findings Resolution Status

- **[P0-1] Endorsement Schema Binding:** `resolved`. `target_hash` added to `endorsement` schema in [`specs/012-xenia-record-schema.md#L216`](../../../../specs/012-xenia-record-schema.md#L216) and asserted by verifier in [`specs/011-xenia-trust-model.md#L94`](../../../../specs/011-xenia-trust-model.md#L94) and [`specs/012-xenia-record-schema.md#L222-L223`](../../../../specs/012-xenia-record-schema.md#L222-L223).
- **[P0-2] Post-Removal Standing vs Historical Preservation:** `partly`. [`specs/011-xenia-trust-model.md#L91-L95`](../../../../specs/011-xenia-trust-model.md#L91-L95) adds standing rules (a)–(c), but rule (c) permits an un-grandfathered non-member key to forge *new* records post-removal if endorsed by a steward.
- **[P0-3] Steward Key Rotation Memory DoS:** `partly`. [`specs/011-xenia-trust-model.md#L201-L202`](../../../../specs/011-xenia-trust-model.md#L201-L202) and [`specs/012-xenia-record-schema.md#L268-L273`](../../../../specs/012-xenia-record-schema.md#L268-L273) introduce grandfather lists for rotation, but [`specs/011-xenia-trust-model.md#L115`](../../../../specs/011-xenia-trust-model.md#L115) still restricts $L_2$ strictly to "a current steward", dropping grandfathered endorsements to $L_1$.
- **[P1-1] Area Matching Mechanism:** `resolved`. Defined via tag intersection and `*` in [`specs/011-xenia-trust-model.md#L70-L71`](../../../../specs/011-xenia-trust-model.md#L70-L71) and [`specs/012-xenia-record-schema.md#L223-L224`](../../../../specs/012-xenia-record-schema.md#L223-L224).
- **[P1-2] Admin-Signed Invite Specification:** `resolved`. JSON format, 14-day expiry, and `xenia-invite/1` domain specified in [`specs/011-xenia-trust-model.md#L85`](../../../../specs/011-xenia-trust-model.md#L85).
- **[P1-3] Pointer Resolver SSRF Guards:** `resolved`. Mandatory DNS resolution blocking private, loopback, link-local, metadata, and rebinding with redirect checks specified in [`specs/012-xenia-record-schema.md#L130-L131`](../../../../specs/012-xenia-record-schema.md#L130-L131).
- **[P1-4] Skill Tree Extraction Hardening:** `resolved`. Rejection of symlinks, hardlinks, submodules, `.git`, `..`, non-regular files, and name regexes added in [`specs/011-xenia-trust-model.md#L182-L183`](../../../../specs/011-xenia-trust-model.md#L182-L183).
- **[P2-1] Line Ending Normalization:** `resolved`. Root `.gitattributes` with `* -text` and client clone assertion added in [`specs/012-xenia-record-schema.md#L263`](../../../../specs/012-xenia-record-schema.md#L263).
- **[P2-2] Roster Signature File Schema:** `resolved`. `<epoch>.sigs.json` schema and `xenia-roster/1` domain specified in [`specs/012-xenia-record-schema.md#L105`](../../../../specs/012-xenia-record-schema.md#L105).
- **[P2-3] Claim Quota Semantics:** `resolved`. Quota restricted to `offer | working` and sorted by ULID ascending in [`specs/011-xenia-trust-model.md#L191-L192`](../../../../specs/011-xenia-trust-model.md#L191-L192) and [`specs/012-xenia-record-schema.md#L208`](../../../../specs/012-xenia-record-schema.md#L208).

---

## 2. New Defects Introduced by the Revision

- **[P0-BLOCKING] Unbounded Endorsed Orphan Rule (c) Enables Post-Removal Key Forgery and Steward Laundering.**  
  - **Failure Mode:** Attacker compromises member Mallory's key. Admins remove Mallory at epoch 2 with `--compromised` (writing an empty grandfather list, [`specs/011-xenia-trust-model.md#L202`](../../../../specs/011-xenia-trust-model.md#L202)). At epoch 5, attacker signs a new record `records/memory/01EVIL.md` using Mallory's revoked key. Attacker social-engineers or colludes with current Steward Bob, who signs `records/endorsement/01END.md` covering `01EVIL@1` with `target_hash`. On sync, [`specs/011-xenia-trust-model.md#L94`](../../../../specs/011-xenia-trust-model.md#L94) evaluates `01EVIL`: Mallory is not a member and not grandfathered, but meets condition (c) ("endorsed orphan"). The post-removal forged record passes Step 2 and is elevated to $L_2$ (`federated_verified`) under §4.1. Revocation fails closed against unendorsed records but fails completely open against collusive or tricked stewards.
  - **Remediation:** In [`specs/011-xenia-trust-model.md#L94-L95`](../../../../specs/011-xenia-trust-model.md#L94-L95) and §7.1, require that an endorsed orphan's `(id, rev, hash)` MUST appear on the admin-signed grandfather list created when that member was removed/retired. Explicitly reject any record with an ID or revision signed by a non-member post-removal.

- **[P0-BLOCKING] Invalidation Cascade Contradiction Between §4.1 and §7.1 Causes Silent Drop to $L_1$ on Steward Rotation.**  
  - **Failure Mode:** Steward Alice rotates key $K_1 \to K_2$ at epoch 2. Epoch 2 grandfather list includes Alice's historical endorsements $E_1, E_2$. [`specs/011-xenia-trust-model.md#L201-L202`](../../../../specs/011-xenia-trust-model.md#L201-L202) states "a grandfathered endorsement keeps counting; routine rotation is free". However, [`specs/011-xenia-trust-model.md#L115`](../../../../specs/011-xenia-trust-model.md#L115) and §3.3 Step 2(c) mandate that $L_2$ requires endorsement by a "current steward". A conforming verifier sees $K_1$ is `retired`, rejects the endorsements, and drops all covered records across the organization to $L_1$. If an implementer instead honors grandfathered endorsements unconditionally, a steward removed for misconduct at epoch 4 retains active endorsements via $K_1$ indefinitely.
  - **Remediation:** In [`specs/011-xenia-trust-model.md#L115`](../../../../specs/011-xenia-trust-model.md#L115), state that an endorsement signed by a retired DID $K_{old}$ remains valid if $K_{old}$ is grandfathered AND the terminal `successor` in the active member chain holds an active `steward` role with covering `steward_areas`.

- **[P1-MAJOR] Unspecified `<did-fingerprint>` Format with Colons Causes Windows Filesystem Failures.**  
  - **Failure Mode:** [`specs/012-xenia-record-schema.md#L273`](../../../../specs/012-xenia-record-schema.md#L273) defines store layout `roster/epochs/<n>.grandfathered/<did-fingerprint>.txt` without defining the fingerprint algorithm. If admin tools format the fingerprint using the raw DID (`did:key:z6Mk...`) or standard SSH fingerprint (`SHA256:...`), the path contains colons (`:`). On Windows (mandated in test 20), colons are reserved NTFS characters; git clones fail immediately with OS path errors.
  - **Remediation:** In [`specs/012-xenia-record-schema.md#L273`](../../../../specs/012-xenia-record-schema.md#L273), specify that `<did-fingerprint>` is the 64-character lowercase hex BLAKE3 hash of the canonical UTF-8 DID string (`blake3(did)`). Mandate `\n` line endings and `LC_ALL=C` byte sort.

- **[P1-MAJOR] Tombstone Conflict Resolution Deadlock and Priority Inversion via Erroneous Per-Record `rev` Ordering.**  
  - **Failure Mode:** Steward Alice withdraws target $T$ with `01TOMB_A.md` at `rev: 1`. Bob (author) restores $T$ with `01TOMB_B.md` at `rev: 1` (different ID per §3.3.4). [`specs/012-xenia-record-schema.md#L234`](../../../../specs/012-xenia-record-schema.md#L234) states effective status derives from the latest tombstone "(by `rev` of the tombstone record)". Both records have `rev: 1`, creating a non-deterministic tie. If Alice bumps `01TOMB_A` to `rev: 2`, a subsequent restore by steward Carol (`01TOMB_C` at `rev: 1`) is ignored because Alice's record has a higher `rev`.
  - **Remediation:** In [`specs/012-xenia-record-schema.md#L234`](../../../../specs/012-xenia-record-schema.md#L234), order multiple tombstone records targeting the same ID strictly by the tombstone record's ULID timestamp ascending (latest ULID wins), ignoring internal `rev`.

- **[P1-MAJOR] Step 5 Role Check Fails and Quarantines Grandfathered and Orphan Signers.**  
  - **Failure Mode:** [`specs/011-xenia-trust-model.md#L98`](../../../../specs/011-xenia-trust-model.md#L98) enforces: "The kind is allowed for the signer's role (§2)", while line 100 dictates standing is judged at the current epoch. In the current epoch, retired or removed signers hold no active role in §2. A conforming validator executing Step 5 finds no allowed role and quarantines all grandfathered records and endorsed orphans.
  - **Remediation:** In [`specs/011-xenia-trust-model.md#L98`](../../../../specs/011-xenia-trust-model.md#L98), specify that for grandfathered records (2b) and orphans (2c), permitted kinds are evaluated against the role recorded in the epoch where the member was last active (`removed_at_epoch`), restricted strictly to non-administrative kinds (`memory`, `progress`, `request`, `claim`).

---

## 3. Verdicts & Required Fixes

| Spec | Verdict | Concur? | Reason |
| :--- | :---: | :---: | :--- |
| [`specs/011-xenia-trust-model.md`](../../../../specs/011-xenia-trust-model.md) | **approve-with-fixes** | **true** | Concurred strictly conditional on applying Fixes 1, 2, and 5 below, resolving all P0 blocking defects. |
| [`specs/012-xenia-record-schema.md`](../../../../specs/012-xenia-record-schema.md) | **approve-with-fixes** | **true** | Concurred strictly conditional on applying Fixes 3 and 4 below, resolving all P1 defects. |

### Required Fixes to Apply:
1. **Fix 1 ([`specs/011-xenia-trust-model.md#L94-L95`](../../../../specs/011-xenia-trust-model.md#L94-L95), §7.1):** Amend Step 2(c) to require that an endorsed orphan record's `(id, rev, hash)` must be listed in the admin-signed grandfather list created when that member was removed or retired; `--compromised` snapshots all existing pre-compromise records into the grandfather file with orphan status (c).
2. **Fix 2 ([`specs/011-xenia-trust-model.md#L115`](../../../../specs/011-xenia-trust-model.md#L115)):** Update the $L_2$ rule: "Valid and endorsed by a current steward, or covered by a grandfathered endorsement from a retired steward whose terminal active successor in the roster remains a steward in good standing."
3. **Fix 3 ([`specs/012-xenia-record-schema.md#L273`](../../../../specs/012-xenia-record-schema.md#L273)):** Define `<did-fingerprint>` as `blake3(did)` formatted as 64 lowercase hex characters.
4. **Fix 4 ([`specs/012-xenia-record-schema.md#L234`](../../../../specs/012-xenia-record-schema.md#L234)):** Replace "(by `rev` of the tombstone record)" with "ordered strictly by tombstone record ULID timestamp ascending (latest ULID wins)".
5. **Fix 5 ([`specs/011-xenia-trust-model.md#L98`](../../../../specs/011-xenia-trust-model.md#L98)):** Evaluate kind permissions for rules 2(b) and 2(c) against the member's historical role at `removed_at_epoch`, restricted strictly to non-administrative kinds.
