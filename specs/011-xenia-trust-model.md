---
id: spec-011-xenia-trust-model
title: "011: Xenia Trust Model: Identity, Roster, Freshness, Trust Classes & Gates"
author: architect
timestamp: 2026-10-24T00:00:00Z
status: approved   # by the operator, 2026-10-24 ("ratify all"); whole document
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: xenia-org-memory
tags: [spec, xenia, security, post-v1]
---

# Spec 011: Xenia Trust Model (v2)

> **Ratification record (D-29, 2026-10-24; superseded in part 2026-10-25).** The operator ratified the design basis of this spec and of [`specs/012`](012-xenia-record-schema.md) on 2026-10-24, and approved both documents as written ("ratify all", which waived the two-family confirming read). On 2026-10-25 the operator confirmed that this direction is approved, ratified amendments 007-A1 and 007-A2, and directed that implementation start without waiting for the core editor and harness (D-29 item 4 is lifted). The plan is [`xenia-phase-1-plan.md`](../missions/2026-10-23-foundational-design/xenia-phase-1-plan.md).
>
> **Status: v2.1, approved, Phase 1 in build.** Xenia is the org-level sharing layer (memory, and later skills and requests) described by the operator on 2026-10-24. This revision follows Council 008 (Gemini, three rounds) and Council 009 (Claude Opus and Sonnet), see `missions/2026-10-23-foundational-design/councils/008-verdict.md` and `009-verdict.md`. Its main change is a **smaller Phase 1**: the normative text covers only what Phase 1 builds; everything else is in §11 as provisional and is re-reviewed when its phase starts.

## 1. Scope & Objectives
Operators in one **org** publish compact signed **records** to a shared git **store** and pull each other's records into their own Delphi installs. Phase 1 shares one kind of knowledge (`memory`) with human endorsement. This spec defines:
1. **Principals**, their identity and the role matrix (§2).
2. **Write authority**: the roster chain, quorum, joining, and record validity (§3).
3. **Freshness**: how an install notices a store that withholds or rewrites history (§4).
4. **Trust classes** mapped onto the existing taint lattice (§5).
5. **Pointers** (§6), **gates** (§7) and **rotation, removal and compromise** (§8).
6. A **threat table** (§9) and **verification plan** (§10).

### 1.1 Non-Goals
- Record fields and store layout ([`specs/012`](012-xenia-record-schema.md)); the store's MCP surface (a later spec).
- Replacing [`specs/007`](007-federated-mesh-and-sync.md). Xenia reuses its lattice and identities and does **not** need Planes 1–2 (live peer transport, deferred by D-06).
- Per-record read ACLs inside one store (§5.4).
- Skills, requests, progress, signed invites, drive pointers, multi-store (all §11).

### 1.2 Simpler Alternative Considered
- **Alternative:** a shared folder or wiki everyone can edit, with no signatures.
- **Why Rejected:** the first poisoned note reaches every operator's agents with no attribution, no way to tell a reviewed record from a drive-by edit, and no way to remove a departed member's contributions.
- **Second, simpler alternative considered in Council 009:** drop endorsement and treat every member-signed record as equal. Rejected: every record would then be $L_1$ (§5.2), and org memory would taint every agent that reads it.

### 1.3 Design Principles
1. **Pull, not push.** Nothing from the store changes an install's behavior until its operator adopts or reads it. Publishing is deliberate and previewed; consuming is gated by the taint lattice.
2. **The store is untrusted storage; the roster is the root of trust.** A record is believed because of who signed it and who endorsed it, never because of where it sits or whether the store says it is current.
3. **Humans sign; agents don't.** Every record is signed by an operator key after the operator approved the exact bytes. Agents appear only as a canonical `author_role` label (D-17).
4. **No tier above $L_2$.** Xenia content never exceeds `federated_verified`. Only the local operator's ratification produces $L_5$.
5. **Summaries are data, never instructions** (`specs/007` §3.2).
6. **Fail closed and visible.** Anything that cannot be verified is quarantined and listed, never silently dropped or silently accepted.

### 1.4 Phase Mapping & Go/No-Go
| Phase | Delivers | Sections | Tests |
| :--- | :--- | :--- | :--- |
| **1. Trust & memory** | Roster chain, join pinning, freshness statements, verify-before-parse, store-bound signatures, kinds `memory` + `endorsement` + `tombstone`, display-only pointers, publish gate, deterministic sync, quarantine reads, rotation by re-signing | 011 §2–§10; 012 §2–§10 | all of 011 §10 and 012 §10 |
| **2. Skills & hardening** | Kind `skill`, adoption gate, multiple stores, signed invites, pointer resolver (SSRF guards), optional vouch lists | 011 §11.1–11.4; 012 §11 | defined when Phase 2 is specified |
| **3. Collaboration** | `progress`, `request`, `claim`, quotas, `drive` pointers, the librarian | 011 §11.5–11.7; 012 §11 | defined when Phase 3 is specified |

**Phase 1 command set**, each mapped to a record or action:
| Command | Does |
| :--- | :--- |
| `join`, `whoami` | pin a store (§3.3); print this install's DID |
| `publish` | new `memory` record or a new `rev` (§7.1) |
| `sync`, `status` | fetch, verify, compute tiers (§7.2); report quarantine, `contested`, freshness age, tampering |
| `endorse [--all-pending] [--withdraw]` | `endorsement` with `verdict: endorse`, or `withdraw` of one's own |
| `dispute <id>` | `endorsement` with `verdict: dispute` |
| `withdraw <id>` / `restore <id>` | `tombstone` with `action: withdraw` / `restore` |
| `republish --resign` | re-sign one's own records, endorsements and tombstones after a routine rotation (§8.1) |
| `admin add \| remove \| rotate \| propose \| sign \| publish \| refresh` | roster epochs (§3.2, §8) and freshness statements (§4.1); `add` and `remove` create proposals; `contested` records are resolved by the author, by stewards agreeing, or by removing a steward (§7.4), so there is no separate resolve command |

**Go/No-Go for Phase 1 → Phase 2** (all automated unless noted): (1) 011 tests 1–31 pass; (2) 012 tests 1–23 pass; (3) a 3-admin, 2-of-3 roster chain test passes, including a fork (same epoch number, different file) and an install offline across several epochs; (4) the freshness and history tests (§10 tests 12–15 and 27) pass, including a hidden newest epoch and every revert case; (5) signature verification uses strict Ed25519 with the published test vectors; (6) a fuzz run of the sidecar and frontmatter parsers finds no crash or accepted malformed record; (7) a **live recovery drill** of the re-pin path (§8.4) succeeds on a scratch store; (8) a **dogfood gate**: at least 3 humans use it for 2 weeks and the team records endorsement latency and the share of records at $L_2$, with thresholds agreed beforehand (suggested: median endorsement within 3 working days; at least 70% of records read at $L_2$). Any failure is No-Go for the next phase, not for Phase 1.

**Relation to `specs/007` Plane 3.** Xenia reuses Plane 3's posture (a separate git store, `did:key` identities, the six-tier lattice, deterministic offline sync that never calls an LLM). It is a distinct subcommand family: `delphi sync` edits active workspace rule files through ratified proposals; `delphi xenia` manages an org knowledge store and never edits active workspace files in Phase 1. They share code for identity, signature checking and the lattice, and share no state. The $L_2$ definition conflict is resolved by amendment 007-A1 (§5.1).

---

## 2. Principals, Identity & Roles
| Principal | Is | Can |
| :--- | :--- | :--- |
| **Operator** | A human with one or more installs; each install has an Ed25519 `did:key` (reuses `specs/007` §2.1) | Sign and publish after approval; read records |
| **Member** | A DID with an `active` roster entry | Publish `memory`; sign `endorsement` with `verdict: dispute`; sign `tombstone` for their own records |
| **Steward** | A member with named `steward_areas` (or `*`) | Everything a member can, plus `endorsement` (`endorse`, `withdraw`) and `tombstone` for records within their areas |
| **Admin** | A member with the `admin` role | Propose, sign and publish roster epochs and freshness statements |
| **Librarian** (Phase 3) | An agent run by an operator, no key | Proposes only (§11.7) |

### 2.1 Person identity
Operators can have several installs and rotate keys, so rules about "different people" cannot compare DIDs. Each roster entry carries a **`person`** field (a short stable identifier assigned by the admins, covered by the admin-majority signature). All DIDs of one human share one `person`; a `successor` DID keeps its predecessor's `person`. Whenever this spec says *same person* it means the same `person` value; *different people* means different `person` values. Quorum counts people (§3.2); the self-endorsement ban (§7.3) compares people.

### 2.2 Role matrix (checked by `delphi-core`)
| Kind / action | member | steward (within areas) | admin |
| :--- | :-: | :-: | :-: |
| `memory`, new `id` | ✓ | ✓ | ✓ |
| new `rev` of an `id` | own records (§3.4) | own records | own records |
| `endorsement` `endorse` / `withdraw` | ✗ | ✓ | ✓ if also steward |
| `endorsement` `dispute` | ✓ | ✓ | ✓ |
| `tombstone` | own records only | records within areas, plus own | own records only |
| roster epoch, freshness statement | ✗ | ✗ | ✓ |

### 2.3 Area matching
A steward's `steward_areas` is a list of tags, or the wildcard `*`. A record is *within* a steward's areas if its `tags` intersect `steward_areas` or the steward holds `*`. For **endorsement** the check uses the tags of the exact revision endorsed. For **tombstone** it uses the **union of tags across all revisions** of the record, so an author cannot escape a steward's tombstone by publishing a new rev with different tags. **Areas are routing, not an authority boundary:** an author chooses tags, so a colluding steward can endorse anything the author tags for them. The real controls are the no-self-endorsement rule (§7.3), a stewards' review expectation, and removal by admins. Records with no tags can be endorsed only by a `*` steward.

---

## 3. Write Authority
### 3.1 The store is untrusted
Git hosting enforces who may push, but a compromised hosting account or maintainer can push anything or withhold anything. Every install verifies signatures itself and decides what is current from signed statements (§4), never from the store's say-so. Hosting auth limits noise only.

### 3.2 The roster chain
- The roster is an **immutable chain** of epoch files `roster/epochs/<n>.json` with detached signature files `<n>.sigs.json` (spec 012 §7). Epoch files are never overwritten. Epochs are numbered from 1 and each names `prev_hash` (BLAKE3 of the previous epoch file).
- Each epoch lists members as `{did, person, handle, roles, steward_areas, status, successor?, removed_at_epoch?, voids_tombstones?}`, where `status` is `active`, `retired` or `removed`. Entries are **never dropped** from later epochs, and a DID's `person` never changes between epochs (a `successor` inherits it exactly). A `retired` or `removed` DID can never become `active` again; a returning human gets a new DID.
- **Standing and quorum use `active` entries of the previous epoch only.** $n_{admin}$ is the number of distinct **persons** holding an `active` DID with the `admin` role in epoch $n-1$. Epoch $n$ is valid only if signed by **a majority of those people**, $\lfloor n_{admin}/2 \rfloor + 1$, one signature per person (two DIDs of one person count once). Signatures from `retired`/`removed` DIDs are ignored.
- $n_{admin}=1$: the single admin signs (single point of failure). $n_{admin}=2$: both sign (a lost key deadlocks; recovery is §8.4). `delphi doctor` warns below 3 and recommends 3 (2-of-3).
- **Genesis (`1.json`)** has no previous epoch and must be signed by every founding admin named in it.
- **Collecting signatures (no ambiguity about the flow).** `delphi xenia admin propose` writes the draft epoch file to a pending branch `roster-proposal/<n>` in the store. Each admin runs `admin sign <n>`, which verifies the file's bytes and chain link, then commits their signature entry (the same `<n>.json` bytes are what every admin signs). `admin publish <n>` verifies the quorum and fast-forwards the main branch, never force-pushes. A signature over a different byte string counts for nothing.
- **Pinned hashes (fork protection).** Every install persists the hash of every epoch it has accepted. A different file under an accepted epoch number is refused and reported (equivocation).

### 3.3 Joining and pinning (O-4)
1. An admin adds the joiner's DID by a roster epoch. The joiner runs `delphi xenia whoami` and sends the printed DID and a proposed handle to an admin, out of band.
2. The joiner runs `delphi xenia join <store-url> --admin <fingerprint> --head <epoch>:<hash>`. The fingerprint is a **current** admin's (the 64-character lowercase hex BLAKE3 of the UTF-8 DID); the head pair is the current head epoch number and hash. Both come from an existing member, out of band.
3. The client downloads the chain, verifies it **forward from genesis** under §3.2 and requires that (a) the chain contains the pinned epoch number with exactly the pinned hash, (b) the pinned admin was an `active` admin in that pinned epoch, and (c) the chain continues validly from there to its head (so a roster change made after the pin was handed out does not break joining). A chain that verifies but does not contain the pinned epoch hash, including one forged with retired founder keys, is refused.
4. The client records `org`, `store`, the store URL and the pinned admin in `$DELPHI_HOME/xenia/<org>/<store>/state/pins.toml` (§3.5), **not** in `team/`. **The store's own `org.toml` is advisory only**; a disagreement with the pinned values is refused.
5. The store is cloned to `$DELPHI_HOME/xenia/<org>/<store>/`, **outside every workspace zone** (not under `team/`), read only by `delphi-core`. Files there never take the `team/` origin default tier; the only things agents ever see are the daemon's sanitized index entries (§7.2), each at its computed tier.

### 3.4 Record validity and standing
A record is **valid** at an install only if all of these hold, checked by `delphi-core`, **in this order**:
1. The record file and sidecar are within size limits and the sidecar (strict JSON, fixed fields) parses. The record's `kind` and `id` are taken from its **path** `records/<kind>/<id>.md` (spec 012 §7).
2. The hash matches the raw bytes and the **strict** Ed25519 signature verifies over the domain string, which includes `org`, `store`, `kind`, `id` and `kid` from the verifier's own configuration and path (spec 012 §3). **No YAML or Markdown is parsed before this step succeeds.**
3. **Standing:** the sidecar's `kid` has an `active` entry in the roster at the install's **current epoch**. Standing never depends on `signed_at` or `roster_epoch`; those are informational because the author chooses them. Retired and removed DIDs have no standing: their records and endorsements are quarantined (§8). **One exception, fail-hidden:** a `tombstone` with `action: withdraw`, and an `endorsement` with `verdict: withdraw` or `dispute`, signed by a DID that is no longer `active` **still take effect**, because they only hide or demote. `restore` and `endorse` never do. The roster entry's `voids_tombstones` flag removes this exception for that DID's tombstones and is **mandatory with `rotate --compromised`**, so a stolen key cannot leave withdrawals behind (§8.2).
4. Only now is the frontmatter parsed. The schema validates (spec 012 §6); `id` equals the path id, `kind` equals the directory, `org` and `store` equal the pinned values, and `author` equals the sidecar `kid`.
5. **Revisions and ownership.** `rev` is greater than any `rev` this install has seen for the same `id`, **or equal with an identical hash** (re-syncing an unchanged record is idempotent). The `id` must have been first created by this signer **or by a predecessor in the signer's `successor` chain** (so a rotated member can continue their own records). **Duplicates:** an endorsement or tombstone is one record per (signer, target); if two files with different ULIDs exist for the same (signer, target), the **smallest ULID survives** and the other is quarantined as `duplicate`. (This is a tie-break only; ULIDs never decide tombstone *effect*, spec 012 §5.3.)
6. The kind and action are allowed for the signer by the role matrix (§2.2), including the area check (§2.3) for endorsements and tombstones.

Invalid records are **quarantined**, never loaded, and listed by `delphi xenia status`, so tampering is visible.

### 3.5 Sticky state
An install persists, per store and never forgets: the highest epoch and the hash of each accepted epoch; and **for every path under `records/` and `roster/`, the highest `rev` seen and its hash**. Rules:
- A path that **disappears** is tampering (reported; the last seen version keeps its effect).
- A path whose `rev` is **lower** than the stored one, or whose hash differs at the **same** `rev`, is tampering (a rollback or a swap); the stored version keeps its effect.
- A **valid higher `rev`** of the same record by the same signer (or its `successor` chain) simply supersedes the stored one. This is how a mistaken tombstone is undone by a `restore`, and how a steward changes a verdict; sticky state does not block it.

**Where it lives:** sticky state and the pins of §3.3 live under `$DELPHI_HOME/xenia/<org>/<store>/state/`, writable only by the daemon. They are **never** stored under `team/` (a shared registry that `delphi sync` or a raw `git pull` can change) or in any ratified-config file an agent can propose changes to. Sticky state is local, so it helps only installs that saw the file; §4 protects fresh installs and withheld updates.

---

## 4. Freshness: Detecting a Store That Withholds or Rewrites
A hosting attacker can serve a stale view (no newest epoch, so a removed member is still valid), a fast-forward commit that deletes a tombstone, or a fast-forward commit that **reverts** a file to an older valid revision (so a steward's `withdraw` becomes their earlier `endorse` again). Local state helps only installs that saw the originals. Xenia therefore requires a signed statement of *what is current* and a check of *history*, not just of the tree.

### 4.1 The freshness statement
`roster/freshness.json` with `freshness.sigs.json` (spec 012 §7) holds `{org, store, epoch, epoch_hash, store_head, issued_at, expires_at}`, signed by **one `active` admin** of the named epoch. One signature is enough because the statement confers no authority; it only attests currency. `store_head` is the git commit the admin verified when signing.
- **Validity is capped by the verifier, not chosen by the signer.** An install rejects a statement whose `expires_at − issued_at` exceeds its locally configured maximum (default **14 days**), whose `expires_at` is later than its own clock plus that maximum, or whose `issued_at` is more than 5 minutes in the future. A statement for an older epoch than the newest chain head the install knows is **stale** whatever its expiry.
- **A new statement accompanies every roster epoch.** `delphi xenia admin publish <n>` writes a fresh statement for the new epoch in the same fast-forward, so a roster change never leaves the org stale.
- **Refresh is manual in Phase 1.** `delphi xenia admin refresh` first runs the full §4.2 history walk and the §3.4 verification from the previous statement's `store_head` and from the admin's own last verified head, and **refuses to sign if any check fails**. An unattended job must not hold an admin key and sign whatever head the host serves; scheduled refresh with a dedicated freshness-only key is deferred (§11.8). `status` and `doctor` warn when the statement has used 50% of its life.

### 4.2 What an install checks on every sync
1. The chain verifies and its head epoch equals the statement's `epoch`, with matching `epoch_hash`, and the statement is valid under §4.1 by the **install's own clock**.
2. The fetched store head is a **descendant** of the statement's `store_head` and a fast-forward of the last head this install accepted. A non-fast-forward (force-push) is refused.
3. **History walk.** For every commit from the last accepted head (or, on a fresh install, from `store_head`) to the fetched head, and for every path under `records/` and `roster/`: the path is **never deleted**, and its `rev` is **never lower** than at the previous commit (an equal `rev` requires an identical hash). The signature is verified **before** `rev` is read from a file. **Signer continuity:** a path's signer may change between versions only to a member of the previous signer's `successor` chain (the same rule as §3.4.5 ownership). A different member overwriting someone else's `id` at a higher `rev` is tampering even though the new file is validly signed; this closes the case where a fresh install sees only the latest overwrite and cannot otherwise tell who created the `id`. For a path that already exists at `store_head`, its owner is the signer shown there (the admin signed that head); for a path first appearing after `store_head`, its owner is the signer of the first commit in which it appears. New files and valid higher revisions by the owner (or successor chain) are fine; deletions, reverts, same-`rev` swaps and takeovers are tampering. History up to `store_head` is covered by the admin's signature on that head.

### 4.3 Failure behavior
- **Statement expired, stale or missing:** the install keeps tombstones and withdrawals in force (fail hidden) but **counts no endorsements**, so every record reads at most $L_1$, and `status` shows a prominent "stale" warning with the statement age. Nothing is deleted.
- **Check 2 or 3 fails:** the whole sync is rejected, the previous accepted state remains, and a "possible tampering" report lists the offending paths.
- **Rewrite recovery (§8.4):** a rewritten history is accepted only with a statement signed by an **admin quorum** (a majority, counted per person), after the operator has been shown every signed file that was deleted or reverted. Sticky state and pinned epoch hashes are never cleared.
- **Residual risk (stated, not hidden):** an attacker holding a valid, unexpired statement can hold an install at that view for at most the validity cap (default 14 days). A stolen or departing admin key can issue a statement for the epoch where that admin was still `active`, bounded by the same cap and by the rule that a newer chain head makes any older statement stale. Operators who want a tighter window shorten the cap.

---

## 5. Trust Classes and the Taint Lattice
### 5.1 Mapping
The receiving install computes a record's tier. The record never states its own tier (spec 012 rejects such keys).

| Condition at the receiving install | Tier | `source_type` |
| :--- | :--- | :--- |
| Valid (§3.4), member-signed, not endorsed | $L_1$ | `federated_peer` |
| Valid, kind `memory`, and **endorsed** (§7.3) by a steward other than the author's person, whose areas cover the exact revision, with `target_hash` equal to the revision's hash, and the freshness statement is current (§4) | $L_2$ | `federated_verified` |
| Failing §3.4, signer without standing, or quarantined | $L_0$ | `external_untrusted` |

Effective tier is $\min(\text{class tier},\ \text{source\_ceiling})$, the anti-laundering rule of `specs/007` §3.1: a record whose publishing span had read untrusted data is clamped to that ceiling. The author's `source_ceiling` is stamped by the publishing daemon from span ceilings (`specs/001` §5.1), not typed by an agent.

**No xenia record exceeds $L_2$, and $L_2$ comes only from steward endorsement.** `evidence` pointers are informational: v1 has no attestation format or verifier, and a "checkable" claim with no mechanism would let any member attach a URL and call it evidence. In Phase 1 **only `memory` can be endorsed to $L_2$**.

**Amendment 007-A1 (ratified by the operator, 2026-10-25).** `specs/007` §3 defines $L_2$ as "signed by a pinned teammate DID + verifiable git commit and CI test attestation". This spec adds a second route to $L_2$: a valid steward endorsement of a xenia `memory` record. `specs/007` §3 is amended to say so (the table row cites this section). Local use of $L_2$ content is unchanged: it never enters always-loaded files without replay ($L_4$) or ratification ($L_5$).

### 5.2 Why steward endorsement must lift to $L_2$
`specs/001` §5.2 forbids worktree writes, code execution and outbound calls for a span that has read $L_0$–$L_1$ data. If every org record were $L_1$, any agent that consulted org memory would lose the ability to edit code for the rest of that span. Endorsement by a named human steward is the revocable path from $L_1$ to $L_2$. Unendorsed ($L_1$) records are readable only by an explicit **quarantine read** in the console (the span is tainted accordingly); agents never pull $L_1$ records automatically.

*Cost, stated plainly (Council 009):* a new `rev` of a record needs a new endorsement, so edits drop a record to $L_1$ until re-endorsed; a departing steward's endorsements lapse together (§8); and in a team where all stewards are also authors, the self-endorsement ban leaves a sole steward's own records at $L_1$. The minimum useful roster is **two stewards**. `delphi xenia endorse --all-pending` lets a steward review and endorse a batch in one sitting.

### 5.3 What each tier may do
| Tier | May enter an agent's context | May become always-loaded (`heuristics.md`, `skills/`, `REVIEW.md`, lessons) |
| :--- | :--- | :--- |
| $L_2$ | Yes, as a bounded summary in a spotlighted `<xenia_record>` envelope; span ceiling lowered to $L_2$ | **No.** Needs local replay ($L_4$) or the operator's ratification ($L_5$) |
| $L_1$ | Only by an explicit quarantine read | **No** |
| $L_0$ | Only by an explicit quarantine read | **Never** (`[H-004]`) |

### 5.4 Visibility is not trust
Every principal with read access to a store can read every summary in it; git cannot hide one file from a repo reader. A store is therefore **one audience**. Sensitive detail belongs behind a pointer (§6) in its original secure location, and a summary must be safe for everyone who can read the store. `visibility` in a record is a routing hint, not secrecy. Separate audiences use separate stores (Phase 2 supports joining several).

---

## 6. Pointers (display-only in Phase 1)
A pointer says "the full material is at X". It is **not** an authorization and **not** evidence the material says what the summary claims.

1. **Phase 1 never fetches pointer targets.** Delphi shows a pointer as text (scheme, locator, pin, label, `access_hint`). The operator opens it with their own tools. Anything they bring into a mission is external content under the existing rules (`specs/010` §5.1: $L_0$; promotion through the existing flow). This removes the pointer resolver, its SSRF surface and any question about which targets deserve which tier.
2. **No exception for "our own" repositories.** A git URL pointing at the org's own repository is still just text; a commit pinned from a fork pull request is served by the same URL, so a locator can never prove a trusted origin.
3. **Pins prevent silent swaps and confer no trust.** The pin (git commit/tree SHA, or content hash plus revision id) is covered by the record signature, so a changed pointer shows as a new `rev`. A pin means "the signer saw these bytes", never "these bytes are safe".
4. **A lying summary** is a social-engineering risk. Controls: the steward review expectation, the `evidence` field for claims, and `dispute` endorsements any member may sign. Xenia does not claim to detect lies automatically.
5. **Allowed schemes** are an explicit list (spec 012 §4). `file:` and local paths are never publishable. `xenia` pointers refer to another record in the same store and are resolved locally.

---

## 7. Gates
### 7.1 Publishing (outbound, Red-tier)
Publishing sends data out of the install, so it follows `specs/004` §5.5 steps 2–3:
1. Only content from **team or workspace scope** already promoted by the operator can be published. **Operator scope never leaves the machine** (`specs/004` §5.7).
2. `delphi-core` builds the record deterministically, runs the `[REV-SEC-05]` path and secret scan and the workspace identifier filter, and attaches the result as `redaction` metadata. The preview **flags** (does not block) URLs, long high-entropy strings and customer-identifier patterns. The pattern list is maintained under `[REV-SEC-05]` in `REVIEW.md`; this spec adds canary tests (§10) rather than a second list.
3. The operator previews the **exact bytes** and approves; the daemon signs and pushes exactly those bytes. **No model turn happens between preview and send.**
4. **Publish protocol.** The client commits on a local branch, runs `fetch` and a fast-forward rebase onto the store head, **re-verifies signatures on the rebased head**, then pushes. Records are distinct files, so concurrent publishers rebase without content conflicts; a same-`id` race (only the same signer can create one) stops and asks the operator. The client never force-pushes.
5. A draft from a span below $L_4$ is allowed, but `source_ceiling` carries the span's taint into the record. Deletion cannot un-leak: **a published secret must be rotated**, not just tombstoned.

### 7.2 Ingestion
- `delphi xenia sync` is deterministic and offline-capable like `delphi sync`. It fetches, runs the §4 freshness checks, validates (§3.4), computes tiers (§5) and updates a local index. **It never calls an LLM.**
- Ingress runs the `specs/007` §3.2 sanitizer on summaries (invisible Unicode, bidi controls, fake prompt tags). As there, the sanitizer is defense in depth; the taint table is the control.
- Records reach a model only as bounded summaries (spec 012 §6). **Tiers are a pure function** of (valid records, current roster, validated endorsements and tombstones, freshness state), recomputed on each sync and on each roster change, never carried across one.

### 7.3 Endorsement
- A steward reviews the exact revision and signs an `endorsement` with `target_hash` of its bytes. **An endorsement counts only if** the steward's `person` differs from the author's `person` (including any predecessor in the author's `successor` chain), the steward's areas cover the revision (§2.3), the steward has standing (§3.4), and the freshness statement is current (§4).
- A new `rev` needs a new endorsement. `verdict: withdraw` retracts one; `verdict: dispute` (any member) flags a record to stewards and shows a warning but does not by itself change the tier.
- The console shows, before the steward signs, what they attest: that they opened the record and its pointers, and that the summary matches.

### 7.4 Tombstones
A tombstone is data **about** a record; it never changes the record's bytes. Effective status is derived deterministically (spec 012 §5.3): the author's tombstone decides, except that a steward's `sensitive` withdraw can be reversed only by the steward who made it; otherwise covering stewards must agree, and if they disagree the status is **`contested`** (hidden, flagged to humans and admins). A `withdraw` keeps its effect (fail hidden) **after its signer is no longer `active`** (§3.4.3), unless the roster entry has `voids_tombstones: true`, which voids that DID's tombstones; the flag is set for cause on removal and is **mandatory with `rotate --compromised`** (§8.2). A signer's tombstone is one record per target; a later change is a higher `rev` of it. One rogue steward can therefore hide a record only until admins remove them for cause.

---

## 8. Rotation, Removal & Compromise
One rule governs all of them: **a DID that is not `active` at the install's current epoch has no standing; its records and endorsements are quarantined** (§3.4), apart from the fail-hidden exception for `withdraw` and `dispute` there. There is no timestamp exception (`signed_at` is author-controlled and git commit order is not authenticated). A DID's `person` is **immutable across epochs**, and a `successor` inherits it exactly (§2.1).

### 8.1 Routine rotation
`delphi xenia admin rotate <member> --routine` writes a roster epoch that marks the old DID `retired` with `successor` set to the new DID (same `person`). The member (who holds both keys) then runs `delphi xenia republish --resign`:
- **their records** are re-published as new revisions signed by the successor (ownership follows the `successor` chain, §3.4.5); they read at $L_1$ until re-endorsed, and the content may be unchanged;
- **a steward's endorsements** are re-issued by the successor with the same `target_hash`, so the records they covered **keep their tier**; the tool does this in one pass;
- **tombstones and disputes** the old key signed are re-signed by the successor so their effect continues without relying on the fail-hidden exception;
- records with an **effective `withdraw`** (author's or steward's) are **skipped**, never re-published, so rotation cannot resurface hidden content.
`--routine` or `--compromised` is **mandatory**; there is no default.

### 8.2 Compromise
`rotate --compromised` marks the old DID `retired` with `successor`, **sets `voids_tombstones`** (mandatory), and does **no** automatic re-signing. Nothing the old key signed has standing: records, endorsements **and tombstones** are all void, so a thief cannot leave `sensitive` withdrawals behind. The successor re-publishes only what they reviewed; stewards re-endorse whatever is worth keeping; a record a thief hid can be hidden or restored afresh by its author or a covering steward. "The steward who made it" in the `sensitive` rule means the same `person` through the `successor` chain.

### 8.3 Removal and departure
Admins mark the DID `removed`. Its records and endorsements lose standing; records it endorsed fall back to $L_1$ until re-endorsed (§5.2 cost). Its `withdraw` tombstones keep effect unless admins set `voids_tombstones`, which they do for cause (§7.4). **No one else can publish a new revision of a removed member's `id`.** To keep a valuable record, a current member publishes a new record that `supersedes` it (and gets it endorsed); the old one stays quarantined.

### 8.4 Break-glass re-pin and rewrite recovery
If a majority of admins is compromised, or a lost key deadlocks the roster ($n_{admin}=2$), the org rebuilds from a new genesis and operators re-pin out of band (§3.3). The recovery path for a rewritten (force-pushed) store is lighter but is not a single-admin decision: after investigation a **majority of the epoch's admins, counted per person**, sign a freshness statement for the new head that names the rewritten head it replaces (`rewrite_of`); each install's operator is shown **every signed file that was deleted or reverted** and confirms with `delphi xenia sync --accept-rewritten-head`. Without both, the rewrite stays refused. **Sticky state is reconciled, not reset:** only the paths on the operator-approved list (the deleted or reverted signed files shown to the operator) have their sticky entries replaced by the new head's versions, each change logged; every other sticky entry, and all pinned epoch hashes, are kept, so a rewrite cannot quietly un-hide anything the operator did not approve. If a rewrite also changes the creator of any `id`, that path is listed too. It is deliberately manual and is exercised in the Go/No-Go drill (§1.4).

---

## 9. Threat Table
| # | Threat | Control |
| :-: | :--- | :--- |
| T1 | Poisoned memory record (prompt injection in a summary) | Sanitizer (defense in depth); spotlighted envelope; taint-gated tools (the control); bounded summaries; no promotion to always-loaded files |
| T2 | Compromised or malicious member | Signatures + attribution; removal or rotation quarantines their records and endorsements (§8); endorsement required for $L_2$ |
| T3 | Stale or withheld store (hidden epoch, hidden removal) | Freshness statement with expiry; stale ⇒ no endorsements count, warning (§4) |
| T4 | Store deletes a tombstone, withdrawal or dispute | No-deletion check from the signed `store_head`; sticky local state; deletion reported (§3.5, §4.2) |
| T5 | Forged records / store tampering | Per-record signatures verified locally; pinned roster; hosting auth not trusted |
| T6 | Rollback or replay (old `rev`, old roster) | `rev` monotonic; pinned epoch hashes; fast-forward-only heads; strict domain string with `id` |
| T7 | Cross-store replay | `org`, `store`, `kind`, `id`, `kid` in the signed domain from the verifier's own config; pinned locally, not from `org.toml` |
| T8 | Backdating with a stolen or removed key | Standing at the current epoch only; `signed_at` ignored (§3.4) |
| T9 | One person endorsing their own record (second install, rotated key) | `person` identity and ban by person across the successor chain (§2.1, §7.3); quorum counts people |
| T10 | Collusion between a steward and an author (area shopping) | Documented as unmitigated by areas; no-self-endorsement, steward review, admin removal (§2.3) |
| T11 | Endorsement reused for different bytes | `target_hash` of the exact revision (spec 012 §5.2) |
| T12 | Taint laundering via a trusted member | $\min$ with `source_ceiling` stamped by the publishing daemon |
| T13 | Pointer target injection | Pointers are never fetched in Phase 1 (§6) |
| T14 | Exfiltration through publishing | Promoted-scope-only, scan, exact-bytes preview, no model turn after approval; rotate any leaked secret (§7.1) |
| T15 | Sensitive detail visible to the whole store | One audience per store (§5.4); publish scan; operator preview |
| T16 | Rogue steward tombstones everything | Area-bound; `contested` is visible; removal for cause voids their tombstones (§7.4) |
| T17 | Joiner given a forged chain (retired founder keys) | Pin of a current admin and the head epoch hash; verify forward from genesis (§3.3) |
| T18 | Store clone treated as trusted workspace content | Clone outside workspace zones, read only by the daemon (§3.3.5) |
| T19 | Parser attacks on malformed records | Size and signature checks before any YAML parse (§3.4); strict schema (spec 012 §6) |
| T20 | Lenient signature verification diverging between installs | Strict Ed25519 (RFC 8032, reject non-canonical S and small-order keys) with test vectors (§10) |
| T21 | Store reverts a file to an older valid revision (a `withdraw` back to `endorse`, a `restore`, an old `memory` rev) | History walk requires non-decreasing `rev` per path from the last accepted head or the signed `store_head`; sticky state per path (§3.5, §4.2) |
| T22 | One admin signs a far-future freshness statement, or a stolen admin key keeps installs on a stale view | Verifier caps `expires_at − issued_at` and `expires_at` against its own clock; a newer chain head makes an older statement stale; `publish` writes a statement per epoch (§4.1) |
| T23 | One compromised admin plus the host force-pushes a clean history and signs it | Rewrite acceptance needs an admin quorum (per person) naming the replaced head, plus operator confirmation listing every deleted or reverted file (§8.4) |
| T24 | Pins or sticky state altered through the shared `team/` registry or a raw `git pull` | Stored under `$DELPHI_HOME`, daemon-writable only (§3.5) |
| T25 | A stolen key leaves `sensitive` withdrawals that hide records permanently | `voids_tombstones` mandatory with `rotate --compromised`; the fail-hidden exception covers only `withdraw`/`dispute`, never `restore`/`endorse` (§3.4.3, §8.2) |
| T26 | Unattended refresh signs whatever head the host serves | Refresh is manual in Phase 1 and runs the history walk before signing; scheduled refresh needs a separate freshness-only key (§4.1, §11.8) |

---

## 10. Verification Plan
1. **Signature test:** flip one byte of a record; it is quarantined at step 2 and the YAML parser never runs (instrumented parser). The same for an oversized file and a non-member signer.
2. **Standing test:** a record signed by a `retired` or `removed` DID is quarantined, with `signed_at` and `roster_epoch` set to predate the change; a record signed by an `active` DID is valid.
3. **Roster quorum test:** a roster change signed by fewer than $\lfloor n_{admin}/2 \rfloor + 1$ **people** is rejected; two DIDs of one person count once; signatures by retired/removed DIDs are ignored.
4. **Roster chain test:** an install pinned at epoch 1 syncs a store at epoch 4 and verifies 1→2→3→4; a missing link or short signatures refuses the target and keeps the last good epoch; a different file under accepted epoch 3 (fork) is refused.
5. **Join pin test:** a chain forged with retired founder keys that ends at a different head is refused; a chain that verifies but whose head hash differs from the pinned one is refused; the pinned admin must be `active` at the head.
6. **Store pin test:** an `org.toml` that disagrees with the pins in `state/pins.toml` is refused.
7. **Tier test:** a member-signed `memory` record is $L_1$; with a valid steward endorsement it is $L_2$; an `evidence` pointer alone changes nothing; never above $L_2$; a record with `source_ceiling: external_untrusted` is $L_0$ even if endorsed; deleting the local index and re-syncing gives the same tiers.
8. **Self-endorsement test:** a steward endorsing their own record is ignored; so is the same person endorsing from a second install, and a person endorsing after a key rotation (successor chain).
9. **Endorsement binding test:** replacing a record's bytes at the same `rev`, or publishing a new `rev`, voids the earlier endorsement (hash mismatch); an endorsement without `target_hash` is rejected.
10. **Area test:** a steward outside the revision's tags cannot endorse it; a tombstone by a covering steward still applies after the author publishes a new rev with different tags (union of tags).
11. **Role matrix test:** a member cannot sign `endorse`; a member can sign `dispute` and a tombstone of their own record; a steward cannot tombstone outside their areas; an endorsement of a non-`memory` kind never lifts a tier.
12. **Hidden-epoch test:** serve an install a store without the newest epoch while a valid freshness statement for the older epoch is still current; the install's `status` shows the statement age; after it expires, no endorsement counts and the stale warning appears. An install that already knows a newer chain head treats an older statement as stale immediately.
13. **History-walk test:** a fast-forward commit that deletes a tombstone, an endorsement `withdraw`, a `dispute`, or any roster file is rejected as tampering; a valid higher `rev` overwriting a record file is accepted. **Revert cases** (all rejected on a fresh install and on an install with sticky state): a steward's endorsement `withdraw` rev 2 replaced by its valid rev 1 `endorse`; a `restore` tombstone reverted to `withdraw` or the reverse; a `memory` rev replaced by an older rev; a same-`rev` file with a different hash; **a takeover** (member Mallory overwrites Alice's `01M.md` at a higher `rev` with a file Mallory validly signed), detected on a fresh install by signer continuity.
14. **Rewrite test:** a force-pushed store is refused; `--accept-rewritten-head` works only with a statement signed by an admin **quorum** (per person) that names the replaced head, after the operator is shown every deleted or reverted signed file; a single-admin statement is refused; only the sticky entries for operator-approved paths are replaced and every other sticky entry and all pinned epoch hashes survive the acceptance; a rewrite that deletes a tombstone the operator did not approve is refused even with a quorum statement.
15. **Freshness validity test:** a statement signed by a non-admin, for another epoch or store, or expired by the install's clock, is ignored; one whose `expires_at − issued_at` exceeds the local maximum, whose `expires_at` is beyond clock plus maximum (for example 2099), or whose `issued_at` is more than 5 minutes ahead is rejected; a store head that is not a descendant of `store_head` is rejected; `admin publish` writes a statement for the new epoch in the same commit; `admin refresh` refuses to sign when the history walk from the previous `store_head` fails.
16. **Tombstone resolution test:** the author's tombstone decides; two stewards in disagreement give `contested` (hidden); a `sensitive` steward withdraw is reversed only by that steward; a far-future ULID gains no priority; one tombstone record per signer per target (a new `rev` of the same record is the only change).
17. **Tombstone after departure test:** a removed or retired author's `withdraw` still hides the record (fail hidden); their `restore` and `endorse` have no effect; a signer whose entry has `voids_tombstones` has their tombstones voided; `rotate --compromised` without `voids_tombstones` is refused by the tool; after a routine rotation, `republish --resign` re-signs the predecessor's tombstones and skips records that have an effective `withdraw`, so a hidden record is not resurfaced.
18. **Rotation test:** routine rotation then `republish --resign`: the member's records become new revs (at $L_1$ until re-endorsed) and a steward's re-issued endorsements keep the covered records at $L_2$; after `rotate --compromised` nothing the old key signed has standing; omitting `--routine`/`--compromised` is an error.
19. **Authorship test:** a successor can publish a new `rev` of a predecessor's `id`; a different member cannot.
20. **Publish gate test:** a planted secret fails the scan (canaries for each `[REV-SEC-05]` class in summary, body and pointer labels); operator-scope content cannot be selected; a byte changed between preview and send aborts the send; the publish rebase re-verifies signatures.
21. **Ingress test:** a summary with bidi controls, invisible tag characters and a fake `</governance>` tag is stripped and spotlighted, and the span ceiling is still lowered.
22. **Taint gate test:** a span that quarantine-reads an $L_1$ record cannot write to the worktree or call out without a Red approval; a span that reads an $L_2$ summary can (per its HITL tier).
23. **Pointer test:** `file:`, local paths, `delphi://` URIs, userinfo URLs and signed-URL query tokens are rejected at publish and ingest; no pointer is fetched at any point in Phase 1 (network-denied test run).
24. **Clone location test:** the store clone is created under `$DELPHI_HOME/xenia/`, not under `team/` or any workspace zone, and agents cannot read it.
25. **Strict signature test:** the published Ed25519 test vectors (including non-canonical S and small-order keys) are rejected by the verifier.
26. **Join flow test:** `whoami` prints the DID; an admin adds it by epoch; `join` succeeds only with the correct admin fingerprint and pinned epoch pair, and a wrong fingerprint or pinned hash is refused with no state written.
27. **Sticky-state test:** a valid higher `rev` (a `restore`, or a steward changing a verdict) supersedes stored sticky state; a lower `rev` or a changed hash at the same `rev` does not; syncing an unchanged record twice, and rebuilding the local index and re-syncing, are idempotent; deleting a seen file is reported and the last seen version keeps its effect.
28. **Pins-location test:** pins and sticky state live under `$DELPHI_HOME/xenia/<org>/<store>/state/`; editing anything under `team/` (including a `stores.toml`) or running `delphi sync` changes none of them.
29. **Person immutability test:** a roster epoch that changes an existing DID's `person`, or a `successor` whose `person` differs from its predecessor's, is rejected.
30. **Duplicate test:** two endorsement (or tombstone) files by one signer for the same target with different ULIDs: the smallest ULID survives, the other is quarantined as `duplicate`.
31. **Join pin test:** a roster change after the pin was handed out does not break joining when the chain contains the pinned epoch hash and continues validly; a chain without the pinned hash is refused.

---

## 11. Deferred to Later Phases (provisional, not binding)
Each item is re-reviewed before its phase starts. The earlier Councils' conclusions are kept here so they are not lost.

1. **Skill adoption (Phase 2).** Kind `skill` (spec 012 §11). A skill is seen like any record but usable only after `delphi xenia adopt <id>`: the pinned tree is fetched and hash-verified; `delphi lint` runs; the recomputed access scope is shown as a diff against least privilege and refused on mismatch with the record's `requires`; every file in the tree, including `scripts/`, is shown as a diff; adoption **is ratification** (stamps $L_5$ with the pinned hash); permissions are never widened beyond the operator's overrides (D-21); the skill installs only under `skills/custom/xenia/<org>/<skill_name>/` and is usable by a role only if the operator adds it to `authorized_skills`; updates re-adopt. Extraction rejects symlinks, hardlinks, submodules, device nodes, FIFOs, `.git` segments, absolute and `..` paths and non-regular files; `org` and `skill_name` match `^[a-z0-9-]{1,32}$` and `^[a-z0-9-]{1,48}$`; files are written without the execute bit.
2. **Pointer resolver (Phase 2).** Optional fetching on an explicit quarantine read, with guards: `https` only (and `git` over `https`/`ssh`); own DNS resolution rejecting loopback, private, link-local, metadata and unspecified addresses, re-checked at connect to defeat rebinding and on each of at most 3 redirects; size and time caps; no ambient credentials. **Pointer content is always $L_0$**, whatever the pin and the record's tier; a pin mismatch fails closed.
3. **Signed invites (Phase 2).** `{org, store, admin, invitee, expires (≤14 days), sig}` over `xenia-invite/1\n<org>\n<store>\n<admin did>\n<invitee did>\n<expires>`; grants nothing by itself; replaces the out-of-band DID paste of §3.3.
4. **Vouch lists and multi-store (Phase 2).** If dogfooding shows the re-sign and re-endorse cost of §8 is real, admins may attach an admin-signed list of `id@rev:blake3` entries to a retirement epoch so a retired key's records keep their standing; Council 008/009 found this mechanism complicated, so it is deferred until evidence supports it. An install may join several stores, each pinned separately (§3.3).
5. **Requests and claims (Phase 3).** Data only; never start a mission; a claim is the claimant's explicit act; per-member quotas (defaults 20 open requests, 10 open claims; a claim counts while `offer` or `working`; earliest ULIDs kept; excess hidden and named in `status`).
6. **Progress and `drive` pointers (Phase 3).** `drive` needs the Drive connector (`specs/010`).
7. **The librarian (Phase 3).** A normal role with the normal sandbox; reads $L_2$ summaries; proposes dedupe, staleness and routing changes; has no key, cannot endorse, and is tainted like any agent after reading $L_1$/$L_0$.
8. **Scheduled freshness refresh (Phase 2).** A roster-listed `freshness` role: a DID that may sign only freshness statements, with no roster or record authority, held by a CI job that runs the §4.2 history walk before every signature. Never an admin key on an unattended host.

---

## 12. Open Decisions for the Operator
| # | Question | Position after Councils 008 and 009 |
| :-: | :--- | :--- |
| O-1 | Does steward endorsement alone lift $L_1 \to L_2$? | Yes, for `memory` only in Phase 1, conditional on the person-level self-endorsement rule (§7.3); amend `specs/007` §3 (007-A1) |
| O-2 | One store per audience? | Yes. Phase 1 is one store; the client pins each store separately so Phase 2 can join several |
| O-3 | Admin quorum | Majority of active admins counted per person; `doctor` warns below 3; two stewards minimum for a useful roster |
| O-4 | Org join | Phase 1: out-of-band pin of a current admin and the head epoch hash, admin adds the DID by epoch. Signed invites are deferred (§11.3) |
| O-5 | Before federation Planes 1–2 (D-06)? | Yes. It needs only git sync plus the existing identity and lattice |
| O-6 | Freshness validity default | **Confirmed (D-29):** capped at 14 days by the verifier, one admin signature, refreshed by hand in Phase 1 (scheduled refresh needs a freshness-only key, §11.8); operators may shorten it |
| O-7 | Phase 1 dogfood thresholds | **Confirmed (D-29):** median endorsement latency ≤ 3 working days and ≥ 70% of records read at $L_2$; to be agreed again before the run |

O-1 to O-5 are **ratified as the design basis** (D-29) with the positions in the table above.
