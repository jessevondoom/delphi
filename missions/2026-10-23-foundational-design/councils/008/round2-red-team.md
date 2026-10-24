---
id: council-008-round2-red-team
title: "Council 008 Round 2: red-team seat (verbatim)"
author: red-team
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-008, verbatim]
model: "gemini-3.8-flash-high via agy"
---

# Delphi Council 008, Round 2: Red-Team Review

## 1. Findings

- **[P0-BLOCKING]** [`specs/012-xenia-record-schema.md#L208-L215`](../../../../specs/012-xenia-record-schema.md#L208-L215), [`specs/012-xenia-record-schema.md#L239`](../../../../specs/012-xenia-record-schema.md#L239), [`specs/011-xenia-trust-model.md#L94`](../../../../specs/011-xenia-trust-model.md#L94) — `[REV-SEC-01]` / `[REV-SEC-06]`
  - **Failure Mode:** *Missing `target_hash` in Endorsement Schema Breaks Revision Binding and Re-enables Content Swapping.* [`specs/012`](../../../../specs/012-xenia-record-schema.md#L208-L213) defines the YAML frontmatter schema for `endorsement` with fields `target`, `verdict`, `scope`, `note`, and `checked`, completely omitting `target_hash`. Line 215 and verification test 8 assert that "an endorsement binds to an exact `id@rev` and the target's `hash`". However, §6 line 239 enforces strict schema validation where all unknown keys are rejected. If an endorser includes `target_hash`, `delphi-core` rejects the endorsement as an invalid schema violation. If omitted, the endorsement cryptographically covers only `target: "<id>@<rev>"`. An author can overwrite `records/<kind>/<id>.md` at the same `rev` with malicious content; because the detached author signature verifies and the endorsement matches `id@rev`, the modified payload is upgraded to $L_2$ without re-review.
  - **Remediation:** Add `target_hash: string` (enforcing `^blake3:[0-9a-f]{64}$`) to the `endorsement` schema in [`specs/012` §5.6](../../../../specs/012-xenia-record-schema.md#L208-L213). Require the verifier in §6 and [`specs/011` §3.3](../../../../specs/011-xenia-trust-model.md#L72-L78) to assert that `target_hash` equals the recomputed BLAKE3 hash of the target record's raw bytes.

- **[P0-BLOCKING]** [`specs/011-xenia-trust-model.md#L74`](../../../../specs/011-xenia-trust-model.md#L74), [`specs/011-xenia-trust-model.md#L79`](../../../../specs/011-xenia-trust-model.md#L79), [`specs/011-xenia-trust-model.md#L179`](../../../../specs/011-xenia-trust-model.md#L179) — `[REV-SEC-06]`
  - **Failure Mode:** *Contradiction Between §3.3 Step 2 and §7.1 Logic Quarantines Historical Knowledge and Permits Post-Removal Key Forgery.* [`specs/011` §3.3 Step 2](../../../../specs/011-xenia-trust-model.md#L74) mandates that a record is valid only if "the sidecar's `kid` is a member of the roster at the install's current epoch". Because validity is evaluated sequentially on raw bytes before reading endorsements, any record by a departed or removed member fails Step 2 and is immediately quarantined as invalid, deadlocking the §7.1 steward preservation mechanism. Conversely, if §7.1 is treated as an override that validates any non-member record carrying a steward endorsement, an attacker holding a compromised key can forge *new* records long after being removed from the roster; if they social-engineer an endorsement on that forged record, it enters the store as valid $L_2$, completely bypassing revocation.
  - **Remediation:** Reconcile §3.3 Step 2 and §7.1: allow a non-member `kid` to pass Step 2 *only* if a valid steward endorsement is present AND the record's commit hash or epoch anchor proves it was published prior to the epoch in which the member was removed. Explicitly reject any record with a new `id` or incremented `rev` signed by a non-member post-removal.

- **[P0-BLOCKING]** [`specs/011-xenia-trust-model.md#L179-L181`](../../../../specs/011-xenia-trust-model.md#L179-L181), [`specs/011-xenia-trust-model.md#L94-L95`](../../../../specs/011-xenia-trust-model.md#L94-L95), [`specs/011-xenia-trust-model.md#L104-L109`](../../../../specs/011-xenia-trust-model.md#L104-L109) — `[REV-SEC-06]`
  - **Failure Mode:** *Steward Key Rotation Invalidation Cascade Causes Organization-Wide Memory Denial of Service.* [`specs/011` §7.1](../../../../specs/011-xenia-trust-model.md#L179) rules that "endorsements signed by a removed steward lapse by the same rule and the records they covered fall back to $L_1$ until re-endorsed." In `did:key`, normal cryptographic key rotation removes the steward's previous DID from the roster and inserts a new one. Because standing is judged strictly at the verifier's current epoch with no distinction between rotation and compromise, rotating a steward's key instantly voids every endorsement that steward ever signed. Hundreds of verified records collapse to $L_1$ across the entire org. Under §4.2 and [`specs/001` §5.2](../../../../specs/001-governance-and-manifests.md#L230-L236), $L_1$ data cannot be automatically retrieved and revokes worktree execution. Routine key hygiene thus paralyzes org memory.
  - **Remediation:** Separate routine key rotation/retirement from active key compromise in the roster schema. Introduce a `retired` status in `roster/epochs/<n>.json` linking an old DID to its successor DID. Endorsements signed by a retired steward in good standing at epoch $E$ remain valid unless explicitly tombstoned or flagged as `compromised`.

- **[P1-MAJOR]** [`specs/011-xenia-trust-model.md#L52`](../../../../specs/011-xenia-trust-model.md#L52), [`specs/011-xenia-trust-model.md#L181`](../../../../specs/011-xenia-trust-model.md#L181), [`specs/012-xenia-record-schema.md#L211`](../../../../specs/012-xenia-record-schema.md#L211), [`specs/012-xenia-record-schema.md#L226-L227`](../../../../specs/012-xenia-record-schema.md#L226-L227) — `[REV-SEC-02]`
  - **Failure Mode:** *Missing Record Area Schema and Matching Mechanism (Enforcement Without Mechanism).* [`specs/011` §2](../../../../specs/011-xenia-trust-model.md#L52), §4.1, and §7.1 state that stewards can endorse and tombstone records "for records within those areas only" and that "a steward tombstone outside the steward's `steward_areas` is invalid." However, the record envelope in [`specs/012` §2](../../../../specs/012-xenia-record-schema.md#L40-L77) defines no `area` field, and the spec provides no deterministic matching algorithm against tags or paths. Furthermore, `tombstone` records ([`specs/012` §5.6](../../../../specs/012-xenia-record-schema.md#L220-L225)) do not even declare a `scope`. Scoped steward authorization cannot be checked by `delphi-core`, rendering area confinement unenforced.
  - **Remediation:** Add a mandatory `area: string` field to the record envelope in [`specs/012` §2](../../../../specs/012-xenia-record-schema.md#L40-L77), or explicitly declare that `scope` maps to the first element of `tags`. Require `delphi-core` to verify that `steward.steward_areas.contains(target_record.area)` before validating endorsements or tombstones.

- **[P1-MAJOR]** [`specs/011-xenia-trust-model.md#L256`](../../../../specs/011-xenia-trust-model.md#L256), [`specs/011-xenia-trust-model.md#L67`](../../../../specs/011-xenia-trust-model.md#L67), [`specs/012-xenia-record-schema.md#L256-L261`](../../../../specs/012-xenia-record-schema.md#L256-L261) — `[REV-ARCH-01]`
  - **Failure Mode:** *Admin-Signed Invite Requirement (O-4) Adopted in Decisions Table Without Specification or Schema.* Decision O-4 in [`specs/011` §10](../../../../specs/011-xenia-trust-model.md#L256) adopts "Both: admin-signed invite plus operator confirmation of the admin fingerprint." However, neither [`specs/011`](../../../../specs/011-xenia-trust-model.md) nor [`specs/012`](../../../../specs/012-xenia-record-schema.md) defines the structure, fields (e.g. `invitee_did`, `org`, `store`, `expires`, `admin_sig`), signature domain, transport mechanism, or verification logic for invites. This is an unbacked requirement with no executable specification.
  - **Remediation:** Specify the invite format either as a detached signed payload (`xenia-invite/1\n<org>\n<store>\n<admin_did>\n<invitee_did>\n<expires>`) verified by `delphi xenia join`, or update §3.2 and §10 to specify that genesis pinning alone establishes root trust.

- **[P1-MAJOR]** [`specs/012-xenia-record-schema.md#L127-L128`](../../../../specs/012-xenia-record-schema.md#L127-L128), [`specs/010-connectors-and-tool-access.md#L74`](../../../../specs/010-connectors-and-tool-access.md#L74) — `[REV-SEC-01]` / `[REV-SEC-05]`
  - **Failure Mode:** *Unpinned HTTPS Pointers Lack Private/Loopback IP and Redirect Guards, Permitting Internal SSRF.* [`specs/012` §4](../../../../specs/012-xenia-record-schema.md#L127-L128) permits `scheme: https` with `pin.kind: none`. Locators are restricted only against userinfo and tokens. There are no restrictions forbidding RFC 1918 private subnets, cloud metadata (`169.254.169.254`), or loopback (`127.0.0.1`). Because Delphi runs an OAuth loopback listener on `127.0.0.1:<port>` ([`specs/010` §4.11](../../../../specs/010-connectors-and-tool-access.md#L74)), an agent or operator following an unpinned pointer can trigger requests against local daemon endpoints or cloud infrastructure.
  - **Remediation:** Mandate in [`specs/012` §4 and §6](../../../../specs/012-xenia-record-schema.md#L127-L128) that `https` pointer resolvers must resolve DNS and reject private, loopback, and link-local IP addresses. Disallow HTTP redirects that target private address spaces.

- **[P1-MAJOR]** [`specs/011-xenia-trust-model.md#L158-L161`](../../../../specs/011-xenia-trust-model.md#L158-L161), [`specs/012-xenia-record-schema.md#L158`](../../../../specs/012-xenia-record-schema.md#L158), [`specs/001-governance-and-manifests.md#L20-L22`](../../../../specs/001-governance-and-manifests.md#L20-L22) — `[REV-SEC-02]` / `[REV-SEC-03]`
  - **Failure Mode:** *Skill Git Tree Extraction Vulnerable to Symlink Directory Escape and Path Traversal.* [`specs/011` §6.3](../../../../specs/011-xenia-trust-model.md#L158-L161) fetches a pinned git tree and installs it under `skills/custom/xenia/<org>/<skill_name>/`. Neither spec requires sanitizing extracted tree objects. A malicious skill git tree can contain symlinks pointing outside the skill root (e.g. `../../../../GOVERNANCE.md` or `.ssh/`), hardlinks, or path-traversal sequences in `<org>` or `<skill_name>`. When ratified, reading or executing skill contents escapes the skill directory and accesses host files.
  - **Remediation:** Mandate that skill adoption extraction rejects any symlink, hardlink, device node, FIFO, or `.git` segment; require `org` and `skill_name` to match `^[a-z0-9-]{1,32}$`; and enforce that every entry in the tree is a regular file within `skills/custom/xenia/<org>/<skill_name>/`.

- **[P2-MINOR]** [`specs/012-xenia-record-schema.md#L102`](../../../../specs/012-xenia-record-schema.md#L102), [`specs/012-xenia-record-schema.md#L254-L272`](../../../../specs/012-xenia-record-schema.md#L254-L272) — `[REV-ARCH-03]`
  - **Failure Mode:** *Missing `.gitattributes` LF Normalization in Store Layout Breaks Detached Signatures on Cross-Platform Clones.* Detached signatures cover the raw bytes of `.md` files. On Windows or environments with `core.autocrlf = true`, git checks out files with CRLF (`\r\n`), causing local BLAKE3 hash checks to fail and quarantining the entire store.
  - **Remediation:** Add `.gitattributes` to the root of the store layout in [`specs/012` §7](../../../../specs/012-xenia-record-schema.md#L254-L272) with `* text=auto eol=lf` and `records/** -text`.

- **[P2-MINOR]** [`specs/012-xenia-record-schema.md#L257-L261`](../../../../specs/012-xenia-record-schema.md#L257-L261) — `[REV-ARCH-03]`
  - **Failure Mode:** *Unspecified JSON Schema for Detached Roster Signatures (`<epoch>.sigs.json`).* [`specs/012` §7](../../../../specs/012-xenia-record-schema.md#L257-L261) specifies that `roster/epochs/<n>.sigs.json` contains detached admin signatures, but unlike record sidecars (§3), it defines no schema for encoding multiple admin signatures.
  - **Remediation:** Explicitly define the schema for `<epoch>.sigs.json` in [`specs/012` §3 or §7](../../../../specs/012-xenia-record-schema.md#L87-L106): `{ "epoch": integer, "signatures": [{ "kid": string, "sig": string }] }`.

- **[P2-MINOR]** [`specs/011-xenia-trust-model.md#L170`](../../../../specs/011-xenia-trust-model.md#L170), [`specs/012-xenia-record-schema.md#L198-L202`](../../../../specs/012-xenia-record-schema.md#L198-L202) — `[REV-ARCH-03]`
  - **Failure Mode:** *Ambiguous Claim Quota Lifecycle and Ingestion Sort Order.* [`specs/011` §6.4](../../../../specs/011-xenia-trust-model.md#L170) caps open claims at 10 per member but does not define which stances release quota or the tie-breaking sort order when hiding excess claims.
  - **Remediation:** Specify in §6.4 and [`specs/012` §5.5](../../../../specs/012-xenia-record-schema.md#L198-L202) that only `stance: offer | working` counts toward the quota, and sort by ULID ascending (earliest first) when hiding excess.

---

## 2. Checked and Sound

1. **Verify-Before-Parse Ordering:** Raw byte size and Ed25519 signature verification strictly precede YAML frontmatter parsing ([`specs/011` §3.3](../../../../specs/011-xenia-trust-model.md#L72-L78), [`specs/012` §6](../../../../specs/012-xenia-record-schema.md#L232-L246)), neutralizing parser exploit vectors.
2. **Strict Cross-Store Domain Separation:** Store ID is bound into the record frontmatter, store layout, and signature domain preimage ([`specs/012` §3](../../../../specs/012-xenia-record-schema.md#L100-L101)), eliminating cross-store replay from private to public stores.
3. **Decoupled Pointer Taint:** Content resolved from `drive`, `https`, or remote git repositories is strictly clamped to $L_0$ ([`specs/011` §5.2](../../../../specs/011-xenia-trust-model.md#L128-L139)), upholding [`specs/010` §5.1](../../../../specs/010-connectors-and-tool-access.md#L76-L81) and [`specs/001` §5.1](../../../../specs/001-governance-and-manifests.md#L209-L229).
4. **Immutable Roster Chain:** Multi-epoch transitions (`roster/epochs/<n>.json` with `prev_hash`) allow offline installs to verify rotations link-by-link without trusting overwrite state ([`specs/011` §3.2](../../../../specs/011-xenia-trust-model.md#L64-L70)).
5. **Skill Namespace Containment:** Adopted skills install exclusively into `skills/custom/xenia/<org>/<skill_name>/` and require operator ratification to $L_5$ ([`specs/011` §6.3](../../../../specs/011-xenia-trust-model.md#L156-L165)), preventing collision with built-in skills.
6. **Disallowed Trust Keys:** Frontmatter explicitly forbids `tier`, `trust`, `trust_class`, `source_type`, and `endorsed` ([`specs/012` §6](../../../../specs/012-xenia-record-schema.md#L239)), preventing self-elevation.

---

## 3. Positions on Open Decisions

> Quoting specs' tables:
> - **O-1:** *Does steward endorsement alone lift a record from L1 to L2? (revised answer: yes, and it is the only path to L2 in v1; pointer targets never inherit it.)*
> - **O-2:** *One store per audience (with `store` bound into signatures) vs one per org.*
> - **O-3:** *Admin quorum (majority of the previous epoch's admins; `doctor` warns below 3) and steward assignment (areas set by admins).*
> - **O-4:** *Org join = admin-signed invite plus operator confirmation of the admin fingerprint.*
> - **O-5:** *Xenia may ship before federation Planes 1-2 (D-06).*
> - **S-1:** *Markdown+YAML records with detached raw-byte signatures.*
> - **S-2:** *Progress as separate expiring records.*
> - **S-3:** *16 KiB / 500-char caps.*
> - **S-4:** *Skills always a pinned git tree.*
> - **S-5:** *Claims public in v1.*

- **O-1:** **Approve with Conditions.** Endorsement lifting signed text to $L_2$ while clamping pointer targets to $L_0$ is lattice-sound. Approval is conditional on resolving the missing `target_hash` field in the endorsement schema (P0-1) and defining the area matching mechanism (P1-1).
- **O-2:** **Concur.** Store-per-audience with `<store>` bound into signature preimages and validated against `org.toml` completely stops cross-store replay.
- **O-3:** **Approve with Warning.** A 2-admin org has a 2-of-2 quorum ($\lfloor 2/2 \rfloor + 1 = 2$), where a single key loss bricks the roster. Relying on `doctor` warnings is acceptable only because manual out-of-band re-pinning exists as break-glass recovery ([`specs/011` §7.3](../../../../specs/011-xenia-trust-model.md#L188-L191)). Area assignment requires concrete schema fields.
- **O-4:** **Reject / Block.** The table adopted both an admin-signed invite and fingerprint confirmation, but neither spec specifies the invite payload, schema, signature domain, or verification algorithm. It is an unbacked requirement (P1-2).
- **O-5:** **Concur.** Asynchronous, git-mediated Plane 3 store sync operates independently of live P2P transports (Planes 1-2, D-06).
- **S-1:** **Concur.** Detached raw-byte sidecars over LF-normalized records avoid canonicalization pitfalls. Must mandate `.gitattributes` to prevent CRLF checkout corruption (P2-1).
- **S-2:** **Concur.** Separate expiring records prevent git merge contention and signature invalidation inherent in rolling logs.
- **S-3:** **Concur.** Strict 16 KiB total and 500-char summary caps protect agent context budgets and mitigate DoS (T15).
- **S-4:** **Concur with Remediation.** Pinned git trees provide auditability, provided git tree extraction is hardened against symlink directory escapes (P1-4).
- **S-5:** **Concur.** Public claims maintain auditability in v1. Inbound claim quotas sufficiently bound mailbox pollution.

---

## 4. Verdicts

| Spec | Verdict | Concur? | Reason |
| :--- | :---: | :---: | :--- |
| [`specs/011-xenia-trust-model.md`](../../../../specs/011-xenia-trust-model.md) | **reject** | **false** | Blocking contradiction between §3.3 Step 2 and §7.1 historical preservation, steward key-rotation denial-of-service cascade, and unbacked O-4 invite requirement. |
| [`specs/012-xenia-record-schema.md`](../../../../specs/012-xenia-record-schema.md) | **reject** | **false** | Missing `target_hash` field in `endorsement` schema while enforcing strict unknown-key rejection, undefined steward area mapping, and unconstrained HTTPS pointer SSRF. |
