---
id: spec-011-xenia-trust-model
title: "011: Xenia Trust Model: Identity, Roster, Trust Classes & Adoption Gates"
author: architect
timestamp: 2026-10-24T00:00:00Z
status: draft   # post-v1 design; not yet reviewed by Strategist or Red-Team
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low
source_mission: xenia-org-memory
tags: [spec, xenia, security, post-v1]
---

# Spec 011: Xenia Trust Model

> **Status: draft, post-v1.** Xenia is the org-level sharing layer (memory, skills, progress, requests) described by the operator on 2026-10-24. The core editor and harness take priority. This spec fixes the trust model first because the [record schema](012-xenia-record-schema.md) and every later piece (store, MCP surface, librarian agent) depend on it. Nothing here is binding until the operator ratifies it after Strategist and Red-Team review (GOVERNANCE §1.3).

## 1. Scope & Objectives
Xenia lets many operators in one **org** publish compact records (summaries plus pointers) to a shared **store** and pull each other's records into their own Delphi installs. This spec defines:
1. **Principals** and how they are identified (§2).
2. **Who may write** to the store, and how that is proven (§3).
3. **How much a received record is trusted**, expressed in the existing taint lattice (§4).
4. **What a pointer does and does not prove** (§5).
5. **Gates** for publishing, adopting skills and answering requests (§6).
6. **Revocation, rollback and compromise handling** (§7).
7. A **threat table** with the control for each threat (§8) and a verification plan (§9).

### 1.1 Non-Goals
- Record fields (spec 012), the store's wire protocol and the MCP surface (a later spec).
- Replacing `specs/007`. Xenia reuses its lattice, its DID identities and its Plane 3 posture; it does **not** require Planes 1 and 2 (live peer transport), which stay deferred (D-06). Xenia is asynchronous and store-mediated.
- Per-record read ACLs inside one store (§4.4 explains why not).

### 1.2 Simpler Alternative Considered
- **Alternative:** a shared folder or wiki everyone can edit, with no signatures.
- **Why Rejected:** the first poisoned note reaches every operator's agents with no attribution, no way to tell a reviewed record from a drive-by edit, and no way to remove a departed member's contributions. Signed, append-only records in git give attribution, history and revocation for little extra machinery.

### 1.3 Design Principles
1. **Pull, not push.** Nothing from the store changes an install's behavior until that install's operator adopts it. Publishing is deliberate and previewed; consuming is gated by the taint lattice.
2. **The store is untrusted storage; the roster is the root of trust.** A record is believed because of who signed it and who endorsed it, never because of where it sits.
3. **Humans sign; agents don't.** Every record is signed by an operator key after the operator approved the exact bytes. Agents appear only as a canonical `author_role` label (D-17).
4. **No new trust tier above $L_2$.** Xenia content never exceeds `federated_verified`. Only the local operator's ratification (existing `delphi` flow) produces $L_5$.
5. **Summaries are data, never instructions.** Same rule as every other federated payload (`specs/007` §3.2).

---

## 2. Principals & Identity
| Principal | Is | Holds | Can |
| :--- | :--- | :--- | :--- |
| **Operator** | A human in the org, one Ed25519 `did:key` per install (reuses `specs/007` §2.1 identity) | Private key in the install's keystore (never in the store) | Sign and publish records after approval; adopt records locally |
| **Member** | An operator listed in the signed roster | Role `member` | Publish `memory`, `progress`, `request`, `claim`; propose `skill` records |
| **Steward** | A member with the `steward` flag, per topic area or org-wide | Role `steward` | Everything a member can, plus sign `endorsement` and `tombstone` records |
| **Admin** | A member with the `admin` flag (small set, default 2–3) | Role `admin` | Change the roster, with the quorum rule in §3.2 |
| **Librarian** (later) | An agent run by an operator | No key of its own | *Propose* dedupe, staleness and routing changes; every change is signed by the operator who accepts it. It has no write authority of its own (§6.5) |

Identity binding: a roster entry maps `did` → `handle`, `roles`, `added_by`, `added_at`. Display names and theme names never appear in signed material (D-17).

---

## 3. Write Authority: the Roster
### 3.1 What the store enforces and what it can't
Git hosting enforces who can *push*, but the push permission is not the security boundary. A compromised hosting account, a misconfigured branch rule or a malicious maintainer can push anything. **Every install therefore verifies signatures itself** on every record, roster change and endorsement, and ignores anything that fails. Hosting-level auth is an extra layer that limits noise.

### 3.2 Roster changes
- The roster is a file in the store (`roster/members.json`) with detached admin signatures.
- A change (add, remove, change roles) is valid only if signed by **a quorum of admins**: $\lceil (n_{admin}+1)/2 \rceil$ of the current admins, and at least 2 whenever $n_{admin} \ge 2$. A one-admin org is allowed but `delphi doctor` flags it.
- Each install **pins the org root**: on first join, the operator confirms the admin DIDs out-of-band (shown as a short fingerprint, like SSH host keys). From then on, a roster is accepted only if it is a valid successor of the last roster this install accepted (signed by a quorum of *that* roster's admins).
- Roster files carry a monotonic `epoch`. An install rejects any roster whose epoch is lower than the one it has seen (rollback protection, §7.2).

### 3.3 Record validity
A record is **valid** at an install only if all of these hold, checked by `delphi-core` (not a model):
1. The sidecar signature verifies over the exact record bytes (spec 012 §3).
2. The signer's DID is in the roster at the record's `signed_at` epoch or later and was not removed before it.
3. The record's schema validates (spec 012) and its size limits hold.
4. `rev` is greater than any `rev` this install has seen for the same `id` from the same signer, and the `id` was first created by that signer (nobody else can overwrite a record).
5. The kind is allowed for the signer's role (§2).

Invalid records are **quarantined**, never loaded, and listed by `delphi xenia status`. They are not silently dropped, so tampering is visible.

---

## 4. Trust Classes and the Taint Lattice
### 4.1 Mapping
The receiving install computes a record's tier. The record never states its own tier.

| Condition at the receiving install | Tier | `source_type` |
| :--- | :--- | :--- |
| Valid, member-signed, no endorsement, no verifiable evidence | $L_1$ | `federated_peer` |
| Valid, member-signed **and** carries a verifiable evidence pointer (CI run, test attestation, pinned commit) that the install can check | $L_2$ | `federated_verified` |
| Valid and **endorsed** by a steward (not the author) in the topic area, endorsement still current | $L_2$ | `federated_verified` |
| Signed by a removed member, or failing §3.3 | $L_0$ (quarantine) | `external_untrusted` |
| Content the record's author marks `source_ceiling: external_untrusted` (derived from the web or third-party data) | $L_0$ | `external_untrusted` |

Effective tier is always $\min(\text{class tier},\ \text{source\_ceiling})$, which is the anti-laundering rule of `specs/007` §3.1 applied to xenia. The author's `source_ceiling` is stamped by the **publishing** daemon from the span ceilings of the content (spec 001 §5.1), not typed by the agent; a steward reviewing an endorsement is expected to spot-check it.

**No xenia record ever exceeds $L_2$.** Content reaches $L_4$ only if the local install replays a test, and $L_5$ only if the local operator ratifies it, both existing flows.

### 4.2 Why steward endorsement must lift to $L_2$ (usability finding)
The taint table (`specs/001` §5.2) forbids worktree writes, code execution and outbound calls for a span that has read $L_0$–$L_1$ data. If **every** org record were $L_1$, any agent that consulted org memory would lose the ability to edit code for the rest of that span. Org memory would be unusable in practice, and operators would route around the control.

Two consequences are built in:
1. **Endorsement is the path from $L_1$ to $L_2$** for knowledge records. It is a human review by a named steward, and it is revocable.
2. **Unendorsed records ($L_1$) are readable only through an explicit "quarantine read"** in the console: the operator sees the record, and the span that reads it is tainted accordingly. Agents never pull $L_1$ records automatically.

*Open decision O-1:* whether steward endorsement alone justifies $L_2$, or whether $L_2$ also needs evidence. Recommendation: endorsement alone for `memory` and `progress`, because $L_2$ only unlocks reading without taint loss and never promotion.

### 4.3 What each tier may do (summary of existing rules, applied)
| Tier | May enter an agent's context | May become always-loaded (`heuristics.md`, `skills/`, `REVIEW.md`, lessons) |
| :--- | :--- | :--- |
| $L_2$ | Yes, as a spotlighted `<xenia_record>` data envelope, with the span ceiling lowered to $L_2$ | **No.** Needs local replay ($L_4$) or the operator's ratification ($L_5$) |
| $L_1$ | Only by an explicit quarantine read | **No** |
| $L_0$ | Only by an explicit quarantine read | **Never** (`[H-004]`) |

### 4.4 Visibility is not trust, and the index is shared
All principals with read access to a store can read every summary in it. Git cannot hide one file from a reader of the repo. So:
- **A store is one audience.** Separate audiences (for example, a security team and the whole org) get separate stores, each with its own roster. An install can join several.
- **Sensitive detail lives behind the pointer.** The summary must be safe for everyone who can read the store. The publish gate (§6.1) scans for this but cannot guarantee it; the operator's approval is the control.
- `visibility` in a record (spec 012) is a *routing hint inside one store* (for example "team:payments"), used for relevance, not for secrecy.

---

## 5. Pointers: What They Prove
A pointer says "the full material is at X". It is **not** an authorization and **not** evidence the material says what the summary claims.

1. **Pointers carry their own access.** Resolving one uses the *reader's* own credentials through the install's connectors (`specs/010` Drive, or git). Xenia mints no access and never stores tokens. A reader without access sees the summary and a "request access" hint, nothing more. This also avoids the confused-deputy problem: xenia cannot be used to read something the reader couldn't already read.
2. **Pins prevent silent swaps.** Every pointer has a `pin` (git commit/tree SHA, or a content hash plus Drive revision ID). The signed record binds the pin. At resolve time:
   - content matching the pin inherits the record's tier (it is exactly what the signer vouched for);
   - content that doesn't match the pin (the file changed after signing) is **$L_0$**, flagged "drifted", and shown to the operator with the diff when computable.
3. **Unpinnable targets.** Mutable targets without revision IDs (a plain URL, a Drive file whose revision API is unavailable) are allowed only as `pin.kind: none`, and their content is always $L_0$ when fetched.
4. **Summary-vs-target mismatch.** A lying summary is a social-engineering vector. Controls: the endorsement step (a steward is expected to open the target), the `evidence` field for claims, and a **report** action that any member can use to ask stewards for review (spec 012 `endorsement` kind with `verdict: dispute`). Xenia does not claim to detect lies automatically.
5. **Allowed schemes** are an explicit list in spec 012. `file://` and any local path are never publishable (they leak the author's filesystem and are unresolvable by others).

---

## 6. Gates
### 6.1 Publishing (outbound, Red-tier)
Publishing sends data out of the install, so it follows `specs/004` §5.5 steps 2–3:
1. Only content from **team or workspace scope** that has already been promoted by the operator can be published. **Operator scope never leaves the machine** (`specs/004` §5.7).
2. `delphi-core` builds the record deterministically from approved inputs, runs the `[REV-SEC-05]` path and secret scan plus the workspace identifier filter, and attaches the result as `redaction` metadata.
3. The operator previews the **exact bytes** and approves; the daemon signs and pushes exactly those bytes. **No model turn happens between preview and send.**
4. A publish from a span below $L_4$ is allowed (an agent may *draft*), but the `source_ceiling` carries the span's taint into the record.

### 6.2 Reading and ingestion
- Sync (`delphi xenia sync`) is deterministic and offline-capable like `delphi sync`: it fetches, verifies (§3.3), computes tiers (§4) and updates a local index. **It never calls an LLM.**
- Ingress runs the `specs/007` §3.2 sanitizer on summaries (invisible Unicode, bidi controls, fake prompt tags) before any model sees them. As there, the sanitizer is defense in depth; the taint table is the control.
- Records are retrieved into context as bounded summaries only. Following a pointer is a separate, logged read.

### 6.3 Adopting a skill (the highest-risk path)
A skill is executable guidance and can request tools, so it gets the strictest gate. A `skill` record can be *seen* at $L_1$/$L_2$ but only becomes usable through **adoption**:
1. The operator runs `delphi xenia adopt <id>`. The install fetches the pinned skill tree and verifies the hash.
2. The skill is lint-checked with the existing `delphi lint` (spec 001 §3.1 schema). Its requested access scope is shown **as a diff against the least-privilege default**.
3. **Adoption is ratification.** The operator's approval stamps the local copy $L_5$ and records the pinned hash. This is the same proposal flow as `delphi sync --review`, and it is the only way an xenia skill enters `skills/`.
4. **Permissions cannot be widened by adoption.** A skill gets at most what the operator's own overrides allow (D-21 posture). A requested scope beyond it is refused, not prompted.
5. **Updates re-adopt.** A new `rev` of an adopted skill is a new proposal showing the diff; the old pinned version keeps running until the operator accepts. A signer removed from the roster flags their adopted skills for re-review (§7.1).
6. Skills from authors with no steward endorsement can be adopted only after an extra confirmation naming that fact.

### 6.4 Requests and claims
- A `request` is **data**. Receiving one never starts a task. It appears in the operator's inbox, like an inbound A2A task without a standing grant (`specs/007` §2.1).
- A `claim` is a separate record signed by the claimant, referencing the request. Claiming is the claimant operator's explicit act. The requester cannot assign work to anyone.
- Any resulting work is an ordinary local mission under the claimant's own governance. A requester gets back only what the claimant chooses to publish (a `memory` record or a pointer).
- Inbound requests are budgeted per sender and per day (default 20 open requests per member, enforced on read: excess is hidden, not deleted).

### 6.5 The librarian agent (later)
The librarian is a normal Delphi role with the normal sandbox. It may read $L_2$ summaries and draft proposed changes (merge duplicates, flag stale records, suggest routing). It has **no key**: each proposed change is a record the accepting operator signs. A librarian that has read an $L_1$ or $L_0$ record is tainted like any agent and cannot publish. The librarian is not a steward and cannot endorse.

---

## 7. Revocation, Rollback & Compromise
### 7.1 Revocation
- **Member removal** is a roster change (§3.2). On the next sync, all of that member's records drop to $L_0$ (quarantine) unless a steward re-endorses them. Skills they authored and the operator adopted are flagged for re-review; they keep running only if the operator confirms (default: keep running the pinned hash, show a banner).
- **Record withdrawal** by the author, or a steward `tombstone`, marks a record `withdrawn`. Installs hide it and keep the tombstone. Git history still holds the old bytes: **a secret that was published must be rotated, not just tombstoned.** The publish gate exists because deletion cannot un-leak.
- **Endorsement revocation:** an endorsement record can be superseded with `verdict: withdraw`. The record's tier is recomputed on every sync.

### 7.2 Rollback and fork protection
- Installs remember the highest roster `epoch` and, per store, the last accepted head. They accept only fast-forward updates. A non-fast-forward (force-push) is refused and reported as a probable attack.
- Signatures cover bytes, not commit position, so an attacker who replays an old valid record is stopped by the `rev` rule (§3.3.4), and a replayed old roster by the epoch rule.

### 7.3 Key compromise
- A member who loses or suspects a key reports it; admins publish a roster change that removes the old DID and adds a new one. Records signed by the old DID *before* a recorded compromise point `compromised_since` stay valid; later ones are quarantined. Admins set `compromised_since` in the roster change (an honest estimate; err early).
- Admin key compromise is handled by the quorum rule. If a quorum is compromised, the org rebuilds from a new root: operators re-pin out-of-band (§3.2).

---

## 8. Threat Table
| # | Threat | Control |
| :-: | :--- | :--- |
| T1 | Poisoned memory record (prompt injection in a summary) | Sanitizer (defense in depth); spotlighted envelope; taint-gated tools (the control); summaries length-capped; no auto-promotion to always-loaded files |
| T2 | Backdoored shared skill | Adoption gate §6.3: lint, scope diff, pinned hash, ratification, no permission widening, re-adopt on update |
| T3 | Compromised or malicious member | Signatures + attribution; removal drops their records to $L_0$; rate limits; steward endorsement required for $L_2$ |
| T4 | Pointer swap after endorsement ("rug pull") | Pins bound by signature; drift makes content $L_0$ and visible |
| T5 | Summary lies about target | Steward review expectation; `evidence`; `dispute` verdicts. Residual risk accepted, stated in §5.4 |
| T6 | Exfiltration through publishing | Promoted-scope-only, scan, exact-bytes preview, no model turn after approval; deletion cannot un-leak, so rotation is mandatory |
| T7 | Forged records / store tampering | Per-record detached signatures verified locally; roster pin; hosting auth is not trusted |
| T8 | Rollback or replay of old records or roster | `rev` monotonicity, roster `epoch`, fast-forward-only heads |
| T9 | Request used to hijack a teammate's agents | Requests are data; claims are explicit; no auto-start; inbound budgets |
| T10 | Taint laundering through a trusted member | $\min$ with `source_ceiling`, stamped by the publishing daemon |
| T11 | Librarian prompt injection | No key, no endorsement, tainted like any agent, proposals need an operator signature |
| T12 | Sensitive detail in a summary visible to the whole store | Audience-per-store (§4.4), publish scan, operator preview |
| T13 | Spam or flooding | Per-member quotas, enforced when reading |
| T14 | Org-wide reputation gaming (collusion of members to self-endorse) | An endorser cannot be the author; stewards are named in the roster by admins, not elected by votes |
| T15 | Denial of service via oversized or malformed records | Size and field limits in spec 012 §6 checked before parsing deeply; quarantine on failure |

---

## 9. Verification Plan
1. **Signature test:** flip one byte in a record; it fails §3.3.1 and is quarantined, not loaded.
2. **Removed-member test:** remove a member by quorum roster change; their records drop to $L_0$ on the next sync; their adopted skill is flagged.
3. **Roster quorum test:** a roster change signed by fewer than the quorum is rejected; a lower `epoch` is rejected.
4. **Rollback test:** replay an older valid record `rev`; rejected. Force-push the store head; refused and reported.
5. **Tier computation test:** member-signed record with no evidence is $L_1$; with a checkable evidence pointer it is $L_2$; with a steward endorsement it is $L_2$; never above $L_2$; a record with `source_ceiling: external_untrusted` is $L_0$ even if endorsed.
6. **Self-endorsement test:** an endorsement by the record's own author does not lift the tier.
7. **Taint gate test:** a span that quarantine-reads an $L_1$ record cannot write to the worktree or call out without a Red approval; a span that reads an $L_2$ summary can (per its HITL tier).
8. **Pin drift test:** change the pointer target after signing; the fetched content is $L_0$, flagged drifted, and cannot raise the span ceiling's tier above $L_0$.
9. **Skill adoption test:** an adopted skill requesting write scope beyond the operator's overrides is refused; a new `rev` stays pending until accepted; the old hash keeps running.
10. **Publish gate test:** a record containing a planted secret fails the scan and cannot be signed; operator-scope content cannot be selected for publish at all; a byte changed between preview and send aborts the send.
11. **Request test:** a received `request` starts no mission and costs zero tokens until the operator claims it.
12. **Librarian test:** the librarian role has no access to the signing key; its proposal has no effect until an operator signs it; after reading an $L_1$ record it cannot publish.
13. **Ingress test:** a summary containing bidi controls, invisible tag characters and a fake `</governance>` tag is stripped and spotlighted; the span ceiling is still lowered.

---

## 10. Open Decisions for the Operator
| # | Question | Recommendation |
| :-: | :--- | :--- |
| O-1 | Does steward endorsement alone lift $L_1 \to L_2$ (§4.2)? | Yes for `memory` and `progress`; skills always need local ratification regardless |
| O-2 | One store per org, or one per audience from the start (§4.4)? | One store per audience; start with a single org-wide store and design the client for several |
| O-3 | Admin quorum size and default steward assignment | 2–3 admins; stewards named by admins per topic area |
| O-4 | Is out-of-band fingerprint pinning enough for org join (§3.2), or should join require an admin-signed invite? | Both: admin-signed invite plus operator confirmation of the admin fingerprint |
| O-5 | May xenia run before federation Planes 1–2 ship (D-06)? | Yes. It needs only Plane 3-style git sync plus the existing identity and lattice |
