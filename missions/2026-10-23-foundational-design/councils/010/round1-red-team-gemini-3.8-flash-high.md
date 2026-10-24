---
id: council-010-red-team-gemini
title: "Council 010: red-team seat, Gemini Flash (verbatim)"
author: red-team
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-010, verbatim]
model: "gemini-3.8-flash-high via agy"
---

# Delphi Council 010: Confirming Review of Revised (v2) Xenia Specs
**Seat:** Red-Team (`agents/red-team/AGENT.md`)  
**Rubric:** [REVIEW.md](../../../../REVIEW.md) §1–§2, [GOVERNANCE.md](../../../../GOVERNANCE.md)  
**Specs Under Review:** [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md), [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md), and Amendment 007-A1 in [specs/007-federated-mesh-and-sync.md](../../../../specs/007-federated-mesh-and-sync.md#L115) §3.

---

### 1. Disposition of Earlier Council Findings

#### Council 009 P0 Findings
- **P0-1 (Freeze/suppression by store):** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L128-L140) §4 & §3.5 define signed freshness statements with descendant/no-deletion checks, expiry falling back to $L_1$, and persistent sticky state.
- **P0-2 (Person-vs-DID self-endorsement):** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L71-L73) §2.1 & §7.3 bind `person` in admin-signed roster epochs; quorum and self-endorsement bans check `person` across successor chains.
- **P0-3 (Pointer-tier exception):** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L177-L185) §6.1–6.2 make pointer targets display-only external $L_0$ content with no local repository exceptions.

#### Council 008 P0 Findings
- **Backdating defeats revocation:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L112-L114) §3.4 step 3 evaluates standing strictly against the install's current epoch.
- **Pointer pin inherits $L_2$:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L177-L185) §6.1–6.3 mandate targets are display-only $L_0$; pins confer integrity only.
- **Signatures not bound to store:** `resolved` — [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L98-L101) §3 constructs the signed domain string from verifier config (`org`, `store`, `kind`, `id`, `kid`).
- **Verifiable evidence grants $L_2$:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L156-L157) §5.1 makes `evidence` pointers informational; steward endorsement is the sole $L_2$ path.
- **Evidence AND endorsement required for $L_2$:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L156-L157) §5.1 confirms steward endorsement alone lifts `memory` to $L_2$.
- **Endorsement lacks `target_hash`:** `resolved` — [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L159) §5.2 requires `target_hash` matching the exact revision's raw bytes.
- **Standing contradicts §7.1:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L112-L114) §3.4 step 3 & §8 simplify standing to `active` roster status.
- **Key rotation voids endorsements:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L215-L220) §8.1 allows successor re-signing of endorsements via `republish --resign`.
- **Endorsed orphan launders post-removal record:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L112-L114) §3.4 & §8 eliminate orphans; non-active DIDs are immediately quarantined.
- **Grandfathered endorsement contradiction:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L215-L220) §8.1 replaces grandfathering states with explicit successor re-signing.

#### Council 009 P1 Spot-Check
- **Stale/contradictory text:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L74-L83) §2.2, [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L188-L204) §6, and test suites are aligned to Phase 1.
- **Filename not bound to `id`:** `resolved` — [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L98-L101) §3 binds `<id>` in the domain string; §6 step 6 cross-checks filename ULID.
- **`status` not tied to standing/quorum:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L95-L97) §3.2 & [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L216-L222) §7 evaluate quorum on `active` epoch $n-1$ entries.
- **Genesis pinning:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L102-L106) §3.3 requires pinning a current active admin plus head epoch hash.
- **Departure voids tombstones:** `partly` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L208-L210) §7.4 specifies survival, but §3.4 step 3 quarantines them on ingest (see Defect 1).
- **Areas matched on author tags:** `partly` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L84-L86) §2.3 evaluates tombstones across tag unions, but concedes areas are routing hints, not authority boundaries.
- **Rotation strands authorship:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L115) §3.4.5 traces `rev` ownership through `successor` chains.
- **Rollback on fresh install / roster fork:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L100-L101) §3.2 & §4.2 bind accepted epoch hashes and store head commit.
- **Over-engineering:** `resolved` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L48-L53) §1.4 defers skills, resolver, invites, and vouch lists to §11.
- **Steward endorsement bottleneck:** `partly` — [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L160-L164) §5.2 adds `--all-pending`, but un-endorsed revs drop to $L_1$.
- **Spec 007 amendment:** `resolved` — [specs/007-federated-mesh-and-sync.md](../../../../specs/007-federated-mesh-and-sync.md#L115) §3 incorporates amendment 007-A1.

---

### 2. New Defects in v2

- **[P0-BLOCKING]** `specs/011-xenia-trust-model.md#L112-L114` — `[REV-SEC-01]`
  - **Failure Mode:** Alice publishes tombstone `records/tombstone/01T.md` (`action: withdraw, reason: sensitive`) hiding poisoned record `01M`. Alice rotates her key (`status: retired`) or leaves the team (`status: removed`) without cause (`voids_tombstones: false`). On next sync, an install executes §3.4 step 3: Alice's DID is not `active`, so `01T` fails standing and is quarantined. Record `01M` loses its tombstone and resurfaces as active, violating §7.4 and test 17.
  - **Remediation:** In [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L112-L114) §3.4 step 3 and §8, add an explicit exception: a record of `kind: tombstone` with `action: withdraw` retains standing if the signer was `active` when the tombstone was created, provided the signer's current roster entry does not set `voids_tombstones: true`.

- **[P0-BLOCKING]** `specs/012-xenia-record-schema.md#L225-L227` — `[REV-SEC-01]`
  - **Failure Mode:** Alice publishes `records/memory/01M.md` at `rev: 1`. Malicious peer Mallory overwrites `records/memory/01M.md` with `rev: 2` signed by Mallory. Existing installs reject `rev: 2` under §3.4.5 (creator mismatch). A fresh install clones the store and sees only `01M.md` at `rev: 2`. Because frontmatter has no bound creator DID, files overwrite on rev, and xenia does not traverse git history on sync, the fresh install accepts Mallory as the creator of `01M`, producing an undetectable split-brain between new and existing installs.
  - **Remediation:** In [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L38-L75) §2 and §3, add a mandatory `creator: <did>` field to frontmatter and the signed domain string, or store revisions immutably as `records/<kind>/<id>/<rev>.md`.

- **[P1-MAJOR]** `specs/011-xenia-trust-model.md#L128-L135` — `[REV-SEC-01]`
  - **Failure Mode:** A store maintainer deletes tombstone `records/tombstone/01T.md` at commit $H_2$. An admin's scheduled cron job runs `delphi xenia admin refresh`. Because §4.1 specifies no deletion checks before signing, `refresh` signs $H_2$ as `store_head`. Installs syncing $H_2$ check deletions only against $H_2$ (§4.2 check 3), permanently laundering the deleted tombstone into signed history for all fresh installs.
  - **Remediation:** In [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L128-L130) §4.1, require `delphi xenia admin refresh` to verify that candidate `store_head` contains all paths under `records/` and `roster/` present in the previous `store_head`, failing closed on any deletion.

- **[P1-MAJOR]** `specs/011-xenia-trust-model.md#L227-L229` — `[REV-TRAP-04]`
  - **Failure Mode:** Admins force-push a store to purge a secret and issue a fresh statement. The operator executes `delphi xenia sync --accept-rewritten-head`. Local sticky state (§3.5) "never forgets" highest seen revisions and records missing files as tampering. The sync fails at §3.4.5 (`rev <= highest_seen`) and halts with a tampering error.
  - **Remediation:** In [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md#L227-L229) §8.4, specify exact reconciliation semantics for `--accept-rewritten-head`, resetting sticky revision counters and tombstone caches to the newly accepted head.

- **[P1-MAJOR]** `specs/012-xenia-record-schema.md#L166-L168` — `[REV-ARCH-01]`
  - **Failure Mode:** Endorsements are saved as arbitrary ULIDs (`records/endorsement/<ulid>.md`). A steward creates `01A.md` (`verdict: endorse`) and `01B.md` (`verdict: withdraw`) targeting the same `id@rev`. Validation (§6) evaluates files individually. Receiving daemons evaluate the conflicting endorsements in indeterminate filesystem order, computing conflicting tiers ($L_1$ vs $L_2$) across teammates.
  - **Remediation:** In [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L188-L204) §6, add a uniqueness check rejecting duplicate endorsement files for the same `(author, target)` pair, or name endorsement files deterministically as `<target_id>_<rev>_<kid_fingerprint>.md`.

- **[P2-MINOR]** `specs/007-federated-mesh-and-sync.md#L115` & `specs/012-xenia-record-schema.md#L252` — `[REV-ARCH-01]`
  - **Failure Mode:** Amendment 007-A1 in [specs/007-federated-mesh-and-sync.md](../../../../specs/007-federated-mesh-and-sync.md#L115) cites `specs/011 §5.1` for freshness, but freshness is defined in `specs/011 §4`. In [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md#L252) §8, withdrawn records map to `status: superseded` without setting `superseded_by`, violating [specs/001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L199) §5 schema rules.
  - **Remediation:** Correct the section pointer in 007-A1 to `specs/011 §4`. In `specs/012` §8, map tombstoned records to an allowed status representation compliant with `specs/001` §5.

---

### 3. Buildability & Critical Risk

- **Is Phase 1 small and clear enough to build?**  
  **Yes.** Deferring skills, active pointer resolution, invites, and vouch lists reduced Phase 1 to a manageable cryptographic sync protocol.
- **Single Biggest Risk:**  
  **The Steward Endorsement Bottleneck.** Because $L_1$ records taint spans and block agent tool execution ([specs/001-governance-and-manifests.md](../../../../specs/001-governance-and-manifests.md#L235) §5.2), org memory is useless to active agents without $L_2$ endorsement. With self-endorsement banned and no carry-forward on revisions, any revision drops records back to $L_1$. In a 2–3 person team, endorsement lag will leave the store effectively dormant.

---

### 4. Verdicts

- **`specs/011-xenia-trust-model.md`:** `approve-with-fixes`
- **`specs/012-xenia-record-schema.md`:** `approve-with-fixes`
- **`specs/007-federated-mesh-and-sync.md` (Amendment 007-A1):** `approve-with-fixes`
- **`concur:`** `false` (unresolved P0 defects identified above)

#### Required Fixes Before Approval Stands:
1. **Exempt withdraw tombstones from active-only standing (§3.4 step 3)** so departures and routine key rotations do not resurface withdrawn records.
2. **Bind original creator identity to records (§2 & §3)** to prevent creator spoofing across revisions on fresh clones.
3. **Add deletion checks to `admin refresh` (§4.1)** to stop cron jobs from laundering deleted tombstones.
4. **Specify sticky-state reset on `--accept-rewritten-head` (§8.4)** to allow recovery from force-pushes.
5. **Enforce unique `(author, target)` endorsement paths or validation checks (§5.2 & §6)** to prevent conflicting endorsement files.
6. **Correct 007-A1 section reference and align local tombstone frontmatter mapping to spec 001 §5.**
