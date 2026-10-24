---
id: spec-011-xenia-trust-model
title: "011: Xenia Trust Model: Identity, Roster, Trust Classes & Adoption Gates"
author: architect
timestamp: 2026-10-24T00:00:00Z
status: draft   # post-v1 design; Council 008 approve-with-fixes (homogeneous), fixes applied; awaiting operator ratification
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

### 1.2a Phase Mapping & Go/No-Go (Council 008)
Xenia is built in three slices so the core editor and harness are never starved. Each phase ships alone and is useful alone.

| Phase | Delivers | Sections built | Tests |
| :--- | :--- | :--- | :--- |
| **1. Trust & memory** | Roster chain and majority rule, verify-before-parse, store-bound signatures, kinds `memory` + `endorsement` + `tombstone`, publish gate with secret canaries, deterministic sync into a local index, quarantine reads, pointer schemes `git`, `https`, `xenia` | 011 §2–§5, §6.1–6.2, §7, §8; 012 §2–§3, §5.1, §5.6, §6–§9 | 011: 1–8, 10, 13–18, 20, 22–26; 012: 1–6, 8–13, 15–22 |
| **2. Skill sharing** | Kind `skill`, pinned git trees, namespaced install, adoption diff viewer and permission ceiling, multiple joined stores | 011 §6.3; 012 §5.2 | 011: 9, 19, 27; 012: 7 |
| **3. Collaboration** | Kinds `progress`, `request`, `claim`, per-member quotas, `drive` pointers (when the Drive connector from `specs/010` ships) | 011 §6.4; 012 §5.3–5.5, `drive` scheme | 011: 11, 21; 012: pointer tests for `drive` |
| **Later** | The librarian agent | 011 §6.5 | 011: 12 |

**Go/No-Go for Phase 1 → Phase 2** (all automated): (1) pre-parse rejection holds (012 test 13); (2) cross-store replay is rejected (012 test 12); (3) removing a member downgrades their unendorsed records to $L_0$ on the next sync (011 tests 2 and 14); (4) pointer content is $L_0$ regardless of record tier (011 test 8); (5) roster chain verification passes for an install offline across several epochs (011 test 16); (6) the Phase 1 CLI (`delphi xenia join | publish | sync | status | endorse`) is exercised by an end-to-end test on a temporary store with two admins, one steward and two members. Initial performance target, to be re-measured: syncing and indexing 1,000 records in under 1.5 seconds. Any failure is No-Go for the next phase, not for Phase 1 itself.

**Relation to `specs/007` Plane 3.** Xenia **reuses** Plane 3's posture and primitives (a separate git store, `did:key` identities, the six-tier lattice, deterministic offline sync that never calls an LLM, snapshot-before-sync) but is a **distinct subcommand family** (`delphi xenia …`). `delphi sync` edits the workspace's active rule files through ratified proposals; `delphi xenia` manages an org knowledge store and touches active workspace files only through skill adoption (§6.3). They share code for identity, signature checking and the lattice, and share no state.

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
| **Steward** | A member with the `steward` flag, per topic area or org-wide | Role `steward`, with named `steward_areas` | Everything a member can, plus sign `endorsement` and `tombstone` records **for records within those areas only** |
| **Admin** | A member with the `admin` flag (small set, default 2–3) | Role `admin` | Change the roster, with the quorum rule in §3.2 |
| **Librarian** (later) | An agent run by an operator | No key of its own | *Propose* dedupe, staleness and routing changes; every change is signed by the operator who accepts it. It has no write authority of its own (§6.5) |

**Area matching (deterministic).** A steward's `steward_areas` is a list of tags, or the single wildcard `*` for an org-wide steward. A record is *within a steward's areas* if the record's `tags` intersect `steward_areas` or the steward holds `*`. An `endorsement` or `tombstone` is valid only if that holds at the install's current epoch; the `scope` field of an endorsement (spec 012 §5.6) must be a subset of that intersection. A record with no tags can be endorsed only by a `*` steward.

Identity binding: a roster entry maps `did` → `handle`, `roles`, `added_by`, `added_at`. Display names and theme names never appear in signed material (D-17).

---

## 3. Write Authority: the Roster
### 3.1 What the store enforces and what it can't
Git hosting enforces who can *push*, but the push permission is not the security boundary. A compromised hosting account, a misconfigured branch rule or a malicious maintainer can push anything. **Every install therefore verifies signatures itself** on every record, roster change and endorsement, and ignores anything that fails. Hosting-level auth is an extra layer that limits noise.

### 3.2 Roster changes
- The roster is an **immutable chain**: one file per epoch, `roster/epochs/<n>.json` with detached admin signatures `roster/epochs/<n>.sigs.json`. Epoch files are never overwritten, so an install that was offline across several changes can fetch every intermediate epoch and verify the chain link by link.
- A change (add, remove, change roles) is valid only if signed by **a majority of the admins of the previous epoch**: $\lfloor n_{admin}/2 \rfloor + 1$. With $n_{admin} = 1$ the single admin signs (a single point of failure); with $n_{admin} = 2$ both must sign (a lost key deadlocks the roster). `delphi doctor` warns for $n_{admin} < 3$ and recommends 3 (2-of-3), the smallest set that tolerates one lost or compromised key. Small teams may run with fewer after reading the warning; the deadlock recovery is the out-of-band re-pin of §7.3.
- Each install **pins the org root**: on first join, the operator confirms the admin DIDs out-of-band (shown as a short fingerprint, like SSH host keys). From then on, the install verifies the chain from its last accepted epoch to the target epoch; every link must be signed by a majority of the previous epoch's admins. A target epoch with a missing or invalid link is refused and the install stays on its last good epoch.
- Epochs are numbered consecutively from 1. **Genesis (`1.json`) has no previous epoch, so it must be signed by every founding admin named in it.** A joiner runs `delphi xenia join <store-url> --admin-fingerprint <fingerprint>`; the command checks that the fingerprint matches one of the signing admins of `1.json` and shows the full admin list for confirmation before pinning. An install rejects any epoch lower than the one it has seen (rollback protection, §7.2).
- **Invites (O-4).** An admin invites a joiner with a small signed JSON file `{ "org", "store", "admin": "<did>", "invitee": "<did>", "expires": "<UTC, at most 14 days out>", "sig" }`, where `sig` is Ed25519 over `xenia-invite/1\n<org>\n<store>\n<admin did>\n<invitee did>\n<expires>`. `delphi xenia join` accepts it only if the signing admin is in the pinned roster, the invitee is the joiner's own DID, the org and store match, and it has not expired. **An invite grants no membership and no write access:** the joiner becomes a member only when an admin-majority roster epoch lists their DID, and hosting access is granted separately. The invite exists so the joiner and the admins confirm the same DID before the roster change, and the fingerprint confirmation below stops a swapped store URL.
- **Store binding.** Every epoch file names the `org` and `store` it belongs to and is signed over a domain string that includes both (spec 012 §3), so a roster or record copied from another store does not verify.

### 3.3 Record validity
A record is **valid** at an install only if all of these hold, checked by `delphi-core` (not a model), **in this order, with no YAML or Markdown parsing before step 3**:
1. The raw file size is within the limit and the small sidecar (strict JSON, fixed fields) parses.
2. The hash matches the raw bytes, the Ed25519 signature verifies over the store-bound domain string (spec 012 §3), **and the signer has standing at the install's current epoch**, which means one of:
   - **(a) member:** the sidecar's `kid` is a member of the current roster; or
   - **(b) vouched:** the `kid` was retired or removed in an epoch whose admin-signed **grandfather list** lists this exact `(id, rev, hash)` as `vouched` (§7.1); or
   - **(c) endorsed orphan:** the exact `(id, rev, hash)` is on that same list as `eligible`, and a current steward has endorsed it with a `target_hash` equal to this record's hash. Such a record is valid **only as an endorsed orphan**: it has no standing of its own, and it is quarantined again the moment that endorsement lapses.
   Anything else is quarantined. **A record the key signed after the list was made can never be on the list**, so a stolen or removed key cannot create a new `id` or a higher `rev` that survives, even if a steward is tricked or colludes: the list, signed by an admin majority, is the only door.
3. Only now is the frontmatter parsed. The schema validates (spec 012 §6), `author` equals the sidecar `kid`, and `org`/`store` match the store.
4. `rev` is greater than any `rev` this install has seen for the same `id` from the same signer, and the `id` was first created by that signer (nobody else can overwrite a record).
5. The kind is allowed for the signer's role (§2). For a **member** this is the current role. For a **vouched** record (b) it is the role the member held in the epoch before their retirement or removal (`removed_at_epoch` minus one). For an **endorsed orphan** (c) only the non-administrative kinds `memory`, `progress`, `request` and `claim` are allowed; an orphaned `skill` must be re-published by a current member before it can be adopted.

**Signer standing is judged at the install's current epoch, never from the record's own claims.** `signed_at` and `roster_epoch` in the sidecar are informational and are not trusted for any validity or revocation decision, because the author controls them (a stolen key could backdate both). A record whose signer is no longer a member (removed or compromised) is therefore not valid on its own; §7.1 gives the one way it stays usable.

Invalid records are **quarantined**, never loaded, and listed by `delphi xenia status`. They are not silently dropped, so tampering is visible.

**Tiers are a pure function of verified inputs.** An install computes every record's tier from (valid records, current roster, valid endorsements, tombstones) on each sync and on each roster change. It never carries a tier forward across a roster change, so revocation takes effect at the next sync (§7.1).

---

## 4. Trust Classes and the Taint Lattice
### 4.1 Mapping
The receiving install computes a record's tier. The record never states its own tier.

| Condition at the receiving install | Tier | `source_type` |
| :--- | :--- | :--- |
| Valid, member-signed, not endorsed | $L_1$ | `federated_peer` |
| Valid and **endorsed** (not by the author) by a steward whose areas cover the record, where the endorsement counts under §7.1: signed by a **current** steward, or listed `vouched` for a retired steward whose terminal `successor` (following `successor` links to the first `active` member) is a current steward with covering areas | $L_2$ | `federated_verified` |
| Signer no longer a member and no valid endorsement (§7.1), or failing §3.3 | $L_0$ (quarantine) | `external_untrusted` |
| Content the record's author marks `source_ceiling: external_untrusted` (derived from the web or third-party data) | $L_0$ | `external_untrusted` |

Effective tier is always $\min(\text{class tier},\ \text{source\_ceiling})$, which is the anti-laundering rule of `specs/007` §3.1 applied to xenia. The author's `source_ceiling` is stamped by the **publishing** daemon from the span ceilings of the content (spec 001 §5.1), not typed by the agent; a steward reviewing an endorsement is expected to spot-check it.

**No xenia record ever exceeds $L_2$**, and **$L_2$ comes only from steward endorsement** in v1 (Council 008). An `evidence` pointer is not a path to $L_2$: v1 defines no attestation format or verifier, so a "checkable" claim would be a rule with no mechanism (any member could attach a URL and call it evidence). Evidence stays informational and is what a local replay uses to reach $L_4$. Evidence-based $L_2$ may return later, but only together with a signed attestation spec (for example in-toto) and a deterministic verifier in `delphi-core`.

Content reaches $L_4$ only if the local install replays a test, and $L_5$ only if the local operator ratifies it, both existing flows. The tier applies to the **signed record text** (title, summary, body). It does not apply to what a pointer leads to (§5.2).

### 4.2 Why steward endorsement must lift to $L_2$ (usability finding)
The taint table (`specs/001` §5.2) forbids worktree writes, code execution and outbound calls for a span that has read $L_0$–$L_1$ data. If **every** org record were $L_1$, any agent that consulted org memory would lose the ability to edit code for the rest of that span. Org memory would be unusable in practice, and operators would route around the control.

Two consequences are built in:
1. **Endorsement is the path from $L_1$ to $L_2$** for knowledge records. It is a human review by a named steward, and it is revocable.
2. **Unendorsed records ($L_1$) are readable only through an explicit "quarantine read"** in the console: the operator sees the record, and the span that reads it is tainted accordingly. Agents never pull $L_1$ records automatically.

*Decision O-1 (resolved in Council 008, pending operator ratification):* steward endorsement alone justifies $L_2$ for the signed record text of `memory` and `progress`, because $L_2$ only unlocks reading without taint loss and never promotion. The Strategist seat proposed requiring endorsement **and** evidence; that is rejected because evidence has no verifier in v1 (§4.1) and the requirement would leave nearly every record at $L_1$, which is the unusable outcome this section exists to avoid.

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
2. **Pins prevent silent swaps; they confer no trust.** Every pointer has a `pin` (git commit/tree SHA, or a content hash plus Drive revision ID). The signed record binds the pin. At resolve time:
   - **Pointed-to content never inherits the record's tier.** It is stamped at its own origin tier under `specs/001` §5.1 and `specs/010` §5.1: content from `drive`, `https` and any git remote other than this install's own tracked repositories is **$L_0$** (`external_untrusted`) and lowers the calling span's ceiling like any other untrusted read. A prompt injection in an endorsed record's Google Doc is still an injection. Only the signed record text is $L_2$.
   - A pin match tells the operator "this is the exact content the signer saw", which is an integrity statement. It is shown as such.
   - Following a pointer is therefore a **quarantine read**: logged, announced to the operator, and the span is tainted accordingly. To use the content in later work without tainting, the operator promotes a local note through the existing connector promotion flow (`specs/010` §5.1.3), which stamps the note's hash at $L_4$ via `provenance.stamp`.
   - **A pin mismatch (the target changed after signing) fails closed.** Delphi reports an integrity error and shows the operator the difference when computable. It does **not** feed drifted content to a running span unless the operator explicitly opens it as a quarantine read. This also prevents a drifting pointer from being used to taint-trap an agent.
3. **Unpinnable targets.** Mutable targets without revision IDs (a plain URL, a Drive file whose revision API is unavailable) are allowed only as `pin.kind: none`. They are always $L_0$ (like every pointer target) and the record shows "unpinned" next to them. Stewards are expected to decline to endorse records whose key claims rest on unpinned targets.
4. **Summary-vs-target mismatch.** A lying summary is a social-engineering vector. Controls: the endorsement step (a steward is expected to open the target), the `evidence` field for claims, and a **report** action that any member can use to ask stewards for review (spec 012 `endorsement` kind with `verdict: dispute`). Xenia does not claim to detect lies automatically.
5. **Allowed schemes** are an explicit list in spec 012. `file://` and any local path are never publishable (they leak the author's filesystem and are unresolvable by others).

---

## 6. Gates
### 6.1 Publishing (outbound, Red-tier)
Publishing sends data out of the install, so it follows `specs/004` §5.5 steps 2–3:
1. Only content from **team or workspace scope** that has already been promoted by the operator can be published. **Operator scope never leaves the machine** (`specs/004` §5.7).
2. `delphi-core` builds the record deterministically from approved inputs, runs the `[REV-SEC-05]` path and secret scan plus the workspace identifier filter, and attaches the result as `redaction` metadata. The preview also **flags** (does not block) summaries containing URLs, long high-entropy strings or customer-identifier patterns, so the operator's eyes go to them. The scan's pattern list is the one maintained under `[REV-SEC-05]` in `REVIEW.md`; this spec adds canary tests for it (§9) instead of a second list that could drift.
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
   The review shows **every file in the pinned tree as a diff**, including `scripts/` and any other executable content, not just the manifest. The requested access scope is recomputed from the fetched `SKILL.md` and compared with the record's `requires` (spec 012 §5.2).
3. **Adoption is ratification.** The operator's approval stamps the local copy $L_5$ and records the pinned hash. This is the same proposal flow as `delphi sync --review`, and it is the only way an xenia skill enters `skills/`. **Extraction is hardened:** the tree is exported from git objects (never checked out in place), and extraction **rejects** any symlink, hardlink, submodule, device node, FIFO, `.git` path segment, absolute or `..` path, and any entry that is not a regular file; `org` and `skill_name` must match `^[a-z0-9-]{1,32}$` and `^[a-z0-9-]{1,48}$` and every path is resolved with the guard of `specs/001` §2.2 before writing. Files are written with no execute bit; the sandbox tier (`specs/008` §4) governs running anything. The skill installs only under its own namespace, `skills/custom/xenia/<org>/<skill_name>/`, and is referred to as `xenia/<org>/<skill_name>`; it can never shadow or overwrite a built-in or an operator's own skill, and an adoption whose name collides is refused. A role may use it only if the operator adds it to that role's `authorized_skills` in their own overrides.
4. **Permissions cannot be widened by adoption.** A skill gets at most what the operator's own overrides allow (D-21 posture). A requested scope beyond it is refused, not prompted.
5. **Updates re-adopt.** A new `rev` of an adopted skill is a new proposal showing the diff; the old pinned version keeps running until the operator accepts. A signer removed from the roster flags their adopted skills for re-review (§7.1).
6. Skills from authors with no steward endorsement can be adopted only after an extra confirmation naming that fact.

### 6.4 Requests and claims
- A `request` is **data**. Receiving one never starts a task. It appears in the operator's inbox, like an inbound A2A task without a standing grant (`specs/007` §2.1).
- A `claim` is a separate record signed by the claimant, referencing the request. Claiming is the claimant operator's explicit act. The requester cannot assign work to anyone.
- Any resulting work is an ordinary local mission under the claimant's own governance. A requester gets back only what the claimant chooses to publish (a `memory` record or a pointer).
- Inbound requests and claims are budgeted per sender (defaults: 20 open requests and 10 open claims per member), enforced deterministically on read. A request counts while `lifecycle: open` and unexpired; a claim counts while its latest `stance` is `offer` or `working`. When a sender is over quota, the **earliest** records by ULID are kept and the rest are hidden (not deleted), and the sender is named in `delphi xenia status` so stewards can act.

### 6.5 The librarian agent (later)
The librarian is a normal Delphi role with the normal sandbox. It may read $L_2$ summaries and draft proposed changes (merge duplicates, flag stale records, suggest routing). It has **no key**: each proposed change is a record the accepting operator signs. A librarian that has read an $L_1$ or $L_0$ record is tainted like any agent and cannot publish. The librarian is not a steward and cannot endorse.

---

## 7. Revocation, Rollback & Compromise
### 7.1 Revocation
- **Member removal, key rotation and key compromise** are roster changes (§3.2) and follow **one rule**: a record or endorsement whose signer is not a member at the install's current epoch has standing only if (b) the admins grandfathered it or (c) it is an endorsed orphan (§3.3 step 2). Otherwise it is quarantined ($L_0$). There is no timestamp-based exception, because `signed_at` is author-controlled (§3.3) and git commit order is not authenticated, so a stolen key has no window in which it can keep publishing.
  - **The grandfather list.** When admins retire or remove a key, the new roster epoch carries, for that DID, `grandfathered_digest` and an accompanying list file `roster/epochs/<n>.grandfathered/<fingerprint>.txt`, where `<fingerprint>` is the 64-character lowercase hex BLAKE3 of the UTF-8 DID string (no colons, so it is a valid file name everywhere). The file has one line per **record and endorsement** the key signed that the admin tooling verified in the store at that moment: `id@rev:blake3 vouched` or `id@rev:blake3 eligible`, sorted by bytes (C locale), LF line endings. The epoch's majority admin signature covers the digest, so a thief cannot add lines.
    - **`vouched`**: the admins vouch for it. It keeps the standing it had (a vouched record is $L_1$ unless endorsed; a vouched endorsement keeps counting, subject to the successor rule in §4.1).
    - **`eligible`**: not vouched, but a current steward *may* endorse it, which makes it an endorsed orphan (§3.3 (c)). An `eligible` endorsement does **not** count.
    - Anything the key signed after the list was made, or that is not on it, is quarantined with no path back. This is what stops a colluding or tricked steward from endorsing a record forged with a removed key.
  - **Routine rotation is therefore free.** `delphi xenia admin rotate <member>` retires the old DID, adds the new one (with `successor` recorded) and writes the list with every entry `vouched`. A steward's endorsements survive rotation and no records drop. The same command with `--compromised` marks every entry `eligible`, so stewards re-endorse what is worth keeping.
  - **Departures** default to all entries `eligible` (the deliberate cost: stewards re-endorse what is worth keeping). Admins may choose to vouch en masse, which is an explicit, signed act. Endorsements by a departed steward are `eligible`, so they lapse and the records they covered fall back to $L_1$ until re-endorsed.
  Skills they authored and the operator adopted are flagged for re-review; they keep running only if the operator confirms (default: keep running the pinned hash, show a banner).
- **Record withdrawal** by the author, or a `tombstone` from a steward **whose areas cover the record** (spec 012 §5.6), hides a record. A tombstone is data about the record, not an edit to it. Its effective status is derived deterministically (spec 012 §5.6): the author's tombstone decides; otherwise stewards must agree, and if they disagree the record is `contested` (hidden, flagged for humans). A mistaken or malicious tombstone is therefore **reversible**. An admin-majority roster action can remove a rogue steward, which voids their tombstones by the rule above. A `sensitive` tombstone is not restorable by anyone except the author. Installs hide a tombstoned record and keep the tombstone. Git history still holds the old bytes: **a secret that was published must be rotated, not just tombstoned.** The publish gate exists because deletion cannot un-leak.
- **Endorsement revocation:** an endorsement record can be superseded with `verdict: withdraw`. The record's tier is recomputed on every sync.

### 7.2 Rollback and fork protection
- Installs remember the highest roster `epoch` and, per store, the last accepted head. They accept only fast-forward updates. A non-fast-forward (force-push) is refused and reported as a probable attack.
- Signatures cover bytes, not commit position, so an attacker who replays an old valid record is stopped by the `rev` rule (§3.3.4), and a replayed old roster by the epoch rule.

### 7.3 Key compromise
- A member who loses or suspects a key reports it; admins publish a roster change that removes the old DID and adds a new one. §7.1 then applies unchanged: the old key's records survive only where a current steward endorses them. There is no `compromised_since` boundary.
- Admin key compromise is handled by the majority rule. If a majority is compromised, or the roster is deadlocked by a lost key (§3.2), the org rebuilds from a new root: operators re-pin out-of-band (§3.2). That re-pin is the break-glass path and is deliberately manual.

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
| T15 | Denial of service via oversized or malformed records | Size check and signature verification happen before any YAML parse (§3.3); then field limits in spec 012 §6; quarantine on failure |
| T16 | Stolen or removed key keeps publishing by backdating `signed_at` or `roster_epoch` | Signer standing judged at the install's current epoch; those fields are never trusted (§3.3); one removal rule with no time window (§7.1) |
| T17 | Replay of a record or roster copied from another store (for example a confidential store into the org-wide one) | `org` and `store` are in the record and in the signed domain string (spec 012 §3); roster epochs name their store (§3.2) |
| T18 | Prompt injection in an endorsed record's pointer target | Pointer content never inherits the record's tier; it is $L_0$ and a quarantine read (§5.2) |
| T19 | Offline install cannot verify a roster that changed several times | Immutable epoch chain verified link by link (§3.2) |
| T20 | Rogue or compromised steward tombstones everything | Tombstones scoped to the steward's areas, reversible, voided by removal (§7.1) |
| T21 | Adopted skill shadows a built-in or hides executable scripts | Namespaced install path, full-tree diff, collision refused (§6.3) |
| T22 | Claim flooding | Per-member open-claim quota enforced on read (§6.4) |
| T23 | Endorsement reused for different bytes at the same `rev` | `target_hash` in every endorsement, recomputed by the verifier (spec 012 §5.6) |
| T24 | Routine key rotation voids a steward's endorsements, or rotation is abused to keep a stolen key alive | Admin-signed grandfather list fixes the set of the old key's records at rotation; later signatures from the old key are quarantined (§7.1) |
| T25 | Forged records from a removed member's key | Standing rules (a)-(c) in §3.3; an endorsed orphan needs a current steward's review of the exact hash |
| T26 | Hostile skill tree (symlinks, traversal, `.git`) | Hardened extraction, name patterns, resolution guard (§6.3) |
| T27 | SSRF through `https` or `git` pointers (loopback, metadata, private ranges) | Resolver guards in spec 012 §4; fetch only on an explicit quarantine read |

---

## 9. Verification Plan
1. **Signature test:** flip one byte in a record; it fails §3.3.1 and is quarantined, not loaded.
2. **Removed-member test:** remove a member by quorum roster change; their records drop to $L_0$ on the next sync; their adopted skill is flagged.
3. **Roster quorum test:** a roster change signed by fewer than the quorum is rejected; a lower `epoch` is rejected.
4. **Rollback test:** replay an older valid record `rev`; rejected. Force-push the store head; refused and reported.
5. **Tier computation test:** a member-signed record is $L_1$; with an `evidence` pointer attached it is still $L_1$; with a valid steward endorsement it is $L_2$; never above $L_2$; a record with `source_ceiling: external_untrusted` is $L_0$ even if endorsed; the same inputs produce the same tiers after deleting the local index.
6. **Self-endorsement test:** an endorsement by the record's own author does not lift the tier.
7. **Taint gate test:** a span that quarantine-reads an $L_1$ record cannot write to the worktree or call out without a Red approval; a span that reads an $L_2$ summary can (per its HITL tier).
8. **Pointer tier and drift test:** a pinned pointer to a Drive file inside an $L_2$ record yields $L_0$ content and lowers the span ceiling to $L_0$ even though the pin matches; changing the target after signing fails closed with an integrity error and no content reaches a span without an explicit quarantine read.
9. **Skill adoption test:** an adopted skill requesting write scope beyond the operator's overrides is refused; a new `rev` stays pending until accepted; the old hash keeps running.
10. **Publish gate test:** a record containing a planted secret fails the scan and cannot be signed; operator-scope content cannot be selected for publish at all; a byte changed between preview and send aborts the send.
11. **Request test:** a received `request` starts no mission and costs zero tokens until the operator claims it.
12. **Librarian test:** the librarian role has no access to the signing key; its proposal has no effect until an operator signs it; after reading an $L_1$ record it cannot publish.
13. **Ingress test:** a summary containing bidi controls, invisible tag characters and a fake `</governance>` tag is stripped and spotlighted; the span ceiling is still lowered.
14. **Backdating test:** remove a member by roster change, then present a record signed by their key with `signed_at` and `roster_epoch` set before the removal; it is quarantined. The same record with a valid endorsement from a current steward is accepted at $L_2$. An endorsement signed by a removed steward does not count.
15. **Cross-store replay test:** copy a valid record and sidecar from store A into store B of the same org; signature verification fails on the `store` binding.
16. **Roster chain test:** an install pinned at epoch 1 syncs a store at epoch 4 and verifies 1→2→3→4; with epoch 3's file missing or its signatures short of a majority, it refuses and stays at epoch 2 or 1.
17. **Parse-order test:** a record with an oversized or malformed body and an invalid signature is rejected without the YAML parser ever running (instrumented parser).
18. **Tombstone scope and reversal test:** a steward outside the record's areas cannot tombstone it; a covering steward's tombstone is reversed by a `restore`; a `sensitive` tombstone is restorable only by the author.
19. **Skill namespace test:** adopting a skill named like a built-in installs under `skills/custom/xenia/<org>/…` and is not usable by any role until added to `authorized_skills`; a tree containing `scripts/` shows those files in the adoption diff.
20. **Secret canary test:** the publish scan rejects canary strings for each pattern class in `[REV-SEC-05]` (cloud keys, token prefixes, private-key headers, credentials in URLs) placed in the summary, body and pointer labels.
21. **Claim quota test:** the eleventh open claim from one member is hidden on read (earliest ULIDs kept) and named in `delphi xenia status`; a `delivered` or `withdrawn` claim frees quota.
22. **Rotation test:** rotate a steward's key with `delphi xenia admin rotate`; every record and endorsement the old key signed before the rotation keeps its tier (all `vouched`), nothing drops to $L_1$ because the successor is a steward with covering areas; a record the old key signs after the rotation is quarantined; rotate to a successor who is **not** a steward and the old endorsements stop counting; with `--compromised`, the old key's entries are `eligible` and drop unless a current steward endorses them.
23. **Grandfather integrity test:** adding a line to a grandfather list file, removing one, or flipping `eligible` to `vouched` fails the epoch's digest check and the epoch is refused; a fingerprint file name containing a colon is refused.
24. **Endorsed-orphan test:** an `eligible` record by a removed member is accepted only while a current steward's endorsement with a matching `target_hash` exists, and is re-quarantined when it lapses; a record the removed key signs after the list was made, **even with a current steward's endorsement**, is quarantined because it is not on the list; an `eligible` endorsement by a departed steward does not count; an orphaned `skill` or `endorsement` record is not accepted via (c); role checks for vouched records use the member's role in the epoch before removal.
25. **Endorsement binding test:** replace a record's bytes at the same `rev` after endorsement; the endorsement no longer counts (hash mismatch) and the record is $L_1$.
26. **Invite test:** an invite signed by a non-admin, for a different DID, for another store, or expired is refused; a valid invite alone grants no membership until a roster epoch lists the DID.
27. **Hostile skill tree test:** a pinned tree containing a symlink, hardlink, submodule, `.git` segment, `..` path or executable file entry is refused whole; nothing is written outside `skills/custom/xenia/<org>/<skill_name>/`; `org` or `skill_name` outside the allowed patterns is refused.


---

## 10. Open Decisions for the Operator
Council 008 Round 1 positions are folded in. The operator approved the Council run and adopting these recommendations (2026-10-24); they remain **proposed** until the operator ratifies the spec itself.

| # | Question | Position after Round 1 |
| :-: | :--- | :--- |
| O-1 | Does steward endorsement alone lift $L_1 \to L_2$ (§4.2)? | Yes, for the signed text of `memory` and `progress`, and **it is the only path to $L_2$ in v1**. Pointer targets never inherit it. Skills always need local ratification |
| O-2 | One store per org, or one per audience from the start (§4.4)? | One store per audience, with `store` bound into records and signatures. Start with a single org-wide store; the client supports several |
| O-3 | Admin quorum size and default steward assignment | Majority of the previous epoch's admins; `doctor` warns below 3 admins and recommends 2-of-3. Stewards are named by admins per topic area, and endorsements and tombstones are bound to those areas |
| O-4 | Is out-of-band fingerprint pinning enough for org join (§3.2), or should join require an admin-signed invite? | Both: admin-signed invite plus operator confirmation of the admin fingerprint |
| O-5 | May xenia run before federation Planes 1–2 ship (D-06)? | Yes. It needs only Plane 3-style git sync plus the existing identity and lattice |
