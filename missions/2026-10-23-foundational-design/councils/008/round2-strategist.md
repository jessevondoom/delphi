---
id: council-008-round2-strategist
title: "Council 008 Round 2: strategist seat (verbatim)"
author: strategist
timestamp: 2026-10-24T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-008, verbatim]
model: "gemini-3.8-flash-medium via agy"
---

# Delphi Council 008, Round 2: Strategist Review

**Seat:** [Strategist](../../../../agents/strategist/AGENT.md)  
**Target Files:** [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md) and [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md)  
**Context & Anchors:** [specs/001-governance-and-manifests.md#L190-L240](../../../../specs/001-governance-and-manifests.md#L190-L240) §5, [specs/007-federated-mesh-and-sync.md#L106-L146](../../../../specs/007-federated-mesh-and-sync.md#L106-L146) §3–4, [specs/010-connectors-and-tool-access.md#L76-L81](../../../../specs/010-connectors-and-tool-access.md#L76-L81) §5.1, and [missions/2026-10-23-foundational-design/councils/008/round1-red-team.md](../../../../missions/2026-10-23-foundational-design/councils/008/round1-red-team.md).

---

## 1. Executive Summary & Strategic Posture

The revisions in [spec 011](../../../../specs/011-xenia-trust-model.md) and [spec 012](../../../../specs/012-xenia-record-schema.md) thoroughly resolve the Round 1 Red-Team P0 vulnerabilities (unanchored revocation bypass, pointer pin taint laundering, cross-store replay, and unverifiable evidence pointers). 

From a **scope and sequencing** perspective, Xenia is now architecturally sound, but it sits right on the edge of feature creep. Spec 012 defines **seven record kinds** (`memory`, `skill`, `progress`, `request`, `claim`, `endorsement`, `tombstone`), a full multi-store audience model, and complex cross-record lifecycles. If implemented all at once before the core editor and harness stabilize, Xenia risks stalling delivery. 

Our strategic recommendation is to **approve both specs with a strict phased sequencing cut**: Phase 1 must deliver only the trust core, roster chain, and `memory` knowledge-sharing loop (with `endorsement` and `tombstone`). `skill` adoption belongs in Phase 2, and `progress`/`request`/`claim` belongs in Phase 3.

---

## 2. Positions on Open Decisions

### O-1: Does steward endorsement alone lift a record from $L_1$ to $L_2$?
*Revised answer: Yes, and it is the only path to $L_2$ in v1; pointer targets never inherit it.*
- **Position: Concur.**
- **Rationale:** Round 1 Strategist's demand for "endorsement AND verifiable evidence" was a serious failure of usability analysis. Under [specs/001 §5.2](../../../../specs/001-governance-and-manifests.md#L230-L240), an agent context tainted to $L_0$ or $L_1$ has all worktree write and code execution tools revoked (`❌`). Because v1 has no machine-verifiable attestation engine (in-toto/Cosign), requiring evidence would keep 100% of org records at $L_1$. Reading any org knowledge would immediately paralyze a builder agent. Lifting the signed summary/body to $L_2$ (`federated_verified`) via human steward endorsement makes org memory usable while strictly preserving security boundaries: pointer targets never inherit $L_2$ ([spec 011 §5.2](../../../../specs/011-xenia-trust-model.md#L128-L137)), and promotion to always-loaded files still demands local replay ($L_4$) or local operator ratification ($L_5$).

### O-2: One store per audience (with `store` bound into signatures) vs one per org.
*Revised answer: One store per audience, with `store` bound into records and signatures. Start with a single org-wide store; the client supports several.*
- **Position: Concur.**
- **Rationale:** Git repositories cannot enforce row- or file-level read permissions. Attempting to enforce multi-audience secrecy inside a single git store is cryptographically futile. Binding `store_id` into the record frontmatter and the Ed25519 signature domain (`xenia/1\n<org>\n<store>\n<kind>\n<hash>...`) stops replay attacks between confidential (e.g., security team) and general org stores. Supporting multiple joined stores in the client while shipping a single org-wide store first is the exact right incremental path.

### O-3: Admin quorum (majority of the previous epoch's admins; `doctor` warns below 3) and steward assignment (areas set by admins).
*Revised answer: Majority of the previous epoch's admins; `doctor` warns below 3 admins and recommends 2-of-3. Stewards are named by admins per topic area, and endorsements and tombstones are bound to those areas.*
- **Position: Concur.**
- **Rationale:** Enforcing a hard rejection for $n_{admin} < 3$ would break small teams (1–2 engineers) during initial adoption. Making $\lfloor n_{admin}/2 \rfloor + 1$ the mathematical rule, backed by an active `delphi doctor` warning and the out-of-band re-pin break-glass path ([spec 011 §7.3](../../../../specs/011-xenia-trust-model.md#L188-L192)), balances governance safety with small-team reality. Scoping steward endorsement and tombstone authority to defined topic areas prevents a single compromised steward from compromising the entire store.

### O-4: Org join = admin-signed invite plus operator confirmation of the admin fingerprint.
*Revised answer: Both: admin-signed invite plus operator confirmation of the admin fingerprint.*
- **Position: Concur.**
- **Rationale:** The TOFU (trust-on-first-use) admin fingerprint check binds the client to the genuine roster genesis, preventing MITM attacks on git clone, while the admin-signed invite provides authorization proof for git hosting write access.

### O-5: Xenia may ship before federation Planes 1–2 (D-06).
*Revised answer: Yes. It needs only Plane 3-style git sync plus the existing identity and lattice.*
- **Position: Concur.**
- **Rationale:** D-06 deferred Planes 1 and 2 (live libp2p/quic P2P transports and synchronous A2A). Xenia relies purely on asynchronous, append-oriented git storage, Ed25519 signatures, and local SQLite caching. Tying Xenia to Planes 1–2 would indefinitely block org memory on unnecessary network plumbing.

---

### S-1: Markdown+YAML records with detached raw-byte signatures.
- **Position: Concur.** Matches all Delphi artifacts, allows clean `git diff` previews without proprietary tooling, and detached raw-byte hashing eliminates the canonicalization vulnerabilities inherent to in-band signatures.

### S-2: Progress as separate expiring records.
- **Position: Concur.** Rolling per-project status records create git merge conflicts and signature invalidation races. Discrete, immutable, time-bounded records (30-day default TTL) keep sync clean.

### S-3: 16 KiB total record and 500-char summary caps.
- **Position: Concur.** Strict bounding is non-negotiable for prompt context hygiene and DoS mitigation ([spec 012 §6](../../../../specs/012-xenia-record-schema.md#L232-L248)). Expanding limits later is non-breaking; shrinking them is breaking.

### S-4: Skills always a pinned git tree.
- **Position: Concur.** Small skill bundles embedded in YAML invite encoding headaches and evade script linters. A pinned git tree ensures full-tree diff inspection (including `scripts/`) during operator ratification ([spec 011 §6.3](../../../../specs/011-xenia-trust-model.md#L156-L165)).

### S-5: Claims public in v1.
- **Position: Concur.** Adding private encrypted claims inside an open git store adds enormous key-exchange complexity for zero initial value. Private coordination belongs in standard communication channels (Slack/PRs). The deterministic 10-claim per-member quota adequately checks spam.

---

## 3. Scope & Sequencing: Minimal First Slice vs. Gold-Plating

Spec 012 packs seven distinct kinds and two meta-kinds. To prevent scope creep from derailing the core product, we identify the following cuts and sequencing priorities:

```
┌────────────────────────────────────────────────────────────────────────┐
│                        PHASE 1: TRUST & MEMORY                         │
│  • Roster Epoch Chain & Quorum Verification (§3)                       │
│  • Detached Signature Verifier (pre-parse byte check) (§3.3)           │
│  • Kinds: `memory` + `endorsement` + `tombstone` (§5.1, §5.6)          │
│  • Outbound Publish Gate with Secret Canary Scanner (§6.1)             │
│  • Sync to local SQLite index (read-only queries)                      │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                        PHASE 2: SKILL ADOPTION                         │
│  • Kind: `skill` with git-tree pinning (§5.2)                          │
│  • Namespaced tree installation `skills/custom/xenia/<org>/...`        │
│  • Ratification diff viewer & permission ceiling enforcement (§6.3)    │
│  • Multi-store support (`team/xenia/stores.toml`)                      │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│                     PHASE 3: ASYNC COLLABORATION                       │
│  • Kinds: `progress`, `request`, `claim` (§5.3 - §5.5)                 │
│  • Claim & Request per-member quotas (§6.4)                            │
│  • Connector Pointers (Google Drive revision resolution, spec 010)     │
│  • DEFERRED: Librarian autonomous agent (§6.5)                         │
└────────────────────────────────────────────────────────────────────────┘
```

1. **Defer `request` and `claim` to Phase 3:** Inbound requests and claims are an asynchronous ticketing and work-dispatch system. Operators do not need Xenia to act as a lightweight Jira or issue tracker in v1. The high-value need is retrieving tribal knowledge (`memory`) and sharing proven agent capabilities (`skill`).
2. **Defer `progress` to Phase 3:** Status reporting is easily communicated via existing issue trackers or daily standup notes. Building expiration timers, staleness views, and aggregation queries in v1 is premature.
3. **Firmly keep Librarian Deferred:** [Spec 011 §6.5](../../../../specs/011-xenia-trust-model.md#L172-L175) rightly marks the Librarian agent as "later". It must remain completely out of implementation scope until Phase 3 is stable.
4. **Scope down pointer schemes in Phase 1:** [Spec 012 §4](../../../../specs/012-xenia-record-schema.md#L109-L135) lists `git`, `drive`, `https`, and `xenia`. Drive revision resolution requires integrating [specs/010 connector machinery](../../../../specs/010-connectors-and-tool-access.md#L40-L55). Phase 1 should support only `git`, `https`, and `xenia` pointer schemes; `drive` should activate only when the Google Drive connector ships.

---

## 4. Implementer Gaps & Under-Specified Mechanics

Before a builder writes code, the following three implementation ambiguities must be addressed:

1. **Git Push Concurrency & Non-Fast-Forward Handling:**
   - *The Issue:* [Spec 011 §7.2](../../../../specs/011-xenia-trust-model.md#L184-L187) specifies that installs refuse non-fast-forward updates. However, in an org git store, if Alice publishes `memory/01A.md` and Bob concurrently publishes `memory/01B.md`, one of their git pushes will be rejected by the remote hosting provider as non-fast-forward.
   - *Implementer Requirement:* The spec must explicitly state the client push protocol: `delphi xenia publish` must perform a `git pull --rebase` (or fetch and fast-forward rebase of local record commits) before pushing. Because all records are distinct files (`records/<kind>/<id>.md`), rebasing record commits will never produce git merge conflicts.
2. **Topic Area Matching for Stewards:**
   - *The Issue:* [Spec 011 §5.2](../../../../specs/011-xenia-trust-model.md#L50-L54) and [spec 012 §5.6](../../../../specs/012-xenia-record-schema.md#L208-L218) state that endorsements and tombstones are valid only within the steward's `steward_areas`. But neither spec defines how a record's topic area is matched.
   - *Implementer Requirement:* Clarify the deterministic matching rule: The record's `tags` array must intersect with the steward's `steward_areas` list, OR the steward must hold the wildcard area `*` (org-wide steward).
3. **Roster Epoch Genesis & Initial Pinning:**
   - *The Issue:* [Spec 011 §3.2](../../../../specs/011-xenia-trust-model.md#L64-L70) defines the epoch transition rule ($\lfloor n_{admin}/2 \rfloor + 1$ signatures of epoch $N-1$). For epoch 1 (genesis), there is no $N-1$.
   - *Implementer Requirement:* Explicitly specify that `1.json` is signed by all founding admins named in `1.json`, and the joiner's CLI (`delphi xenia join <store-url> --admin-fingerprint <did-fingerprint>`) checks that the fingerprint matches one of the signing admins of `1.json`.

---

## 5. Usability & Operator Realism

### Steward Endorsement as the $L_1 \to L_2$ Path
The revised mechanism reflects how engineering teams actually operate:
- **No Agent Deadlock:** Without the $L_2$ lift, every org search would taint the agent context to $L_1$, revoking write tools and killing the session. By allowing steward-endorsed summaries into context at $L_2$, agents can leverage org patterns, conventions, and architectural gotchas while actively coding.
- **Quarantine Reads Prevent Surprise Taints:** If an unendorsed ($L_1$) record or a pointer target ($L_0$) is retrieved, Delphi does not silently ingest it and tank the agent's permissions. It halts, displays a quarantine warning to the human operator, and requires an explicit operator choice to proceed. This aligns with [specs/001 §5.2](../../../../specs/001-governance-and-manifests.md#L230-L240).

### Small Team Friction (2–4 Engineers)
- **The Self-Endorsement Block:** [Spec 011 §4.1](../../../../specs/011-xenia-trust-model.md#L88-L98) forbids author self-endorsement. On a 2-person team, Alice cannot endorse her own records; Bob must endorse them. This mirrors code review (PR approval) and provides an appropriate baseline for peer-reviewed org knowledge.
- **Quorum Ergonomics:** With $n_{admin} = 2$, quorum requires both admins (2-of-2). If one is away, admin changes are frozen. Handling this with a non-blocking `delphi doctor` warning rather than an install-time refusal allows 2-person startups to operate pragmatically until they expand to 3 admins.

---

## 6. Relationship to Spec 007 Plane 3, `delphi sync`, and D-06

Xenia is an **architectural extension of Spec 007 Plane 3**, not an incompatible invention:

| Feature Dimension | Spec 007 Plane 3 (`delphi sync`) | Specs 011/012 (`delphi xenia sync`) |
| :--- | :--- | :--- |
| **Storage Medium** | Dedicated git repo cloned into `team/` ([specs/007 §4](../../../../specs/007-federated-mesh-and-sync.md#L137-L145)) | Separate org store git repo (`xenia-<org>`) |
| **Target Artifacts** | Workspace active prompt rules (`heuristics.md`, `REVIEW.md`) | Discrete immutable knowledge records (`records/<kind>/<id>.md`) |
| **Trust Tier Ceiling** | Proposals staged for operator batch ratification to $L_5$ | Records computed to $\le L_2$; local SQLite index; skills ratify to $L_5$ on adoption |
| **Identity & Lattice** | Uses `did:key` & 6-tier lattice ([specs/007 §2-3](../../../../specs/007-federated-mesh-and-sync.md#L106-L125)) | Reuses identical `did:key` format and 6-tier lattice ([spec 011 §2, §4](../../../../specs/011-xenia-trust-model.md#L47-L57)) |
| **Relation to D-06** | Plane 3 is active; Planes 1–2 deferred | Fully complies with D-06; strictly asynchronous and store-mediated |

**CLI Boundary:** `delphi sync` and `delphi xenia` must remain distinct subcommands. `delphi sync` directly mutates project workspace rules and team heuristics. `delphi xenia` manages org-wide knowledge repositories without touching active project workspace files until explicit skill adoption or quarantine inspection occurs.

---

## 7. Phased Roadmap & Go/No-Go Gates

### Milestone Schedule
- **Milestone 1 (Trust & Memory):**
  - Implement Ed25519 detached signature generation and verification.
  - Implement verify-before-parse pipeline ([spec 012 §6](../../../../specs/012-xenia-record-schema.md#L232-L248)).
  - Implement Roster Epoch Chain verification with majority quorum.
  - Ingestion and sync of `memory`, `endorsement`, and `tombstone` into local SQLite index.
  - Outbound publish gate with secret canary scan ([spec 011 §6.1, §9 test 20](../../../../specs/011-xenia-trust-model.md#L242)).
- **Milestone 2 (Skill Sharing):**
  - Implement `skill` record schema and git tree fetcher.
  - Implement `skills/custom/xenia/<org>/<skill_name>/` namespace sandboxing.
  - Implement adoption review diff UI (showing full tree diff of `scripts/` and manifest).
- **Milestone 3 (Collaboration & Meta-Kinds):**
  - Implement `progress`, `request`, and `claim` schemas and sender rate quotas.
  - Implement Google Drive connector pointer resolution.

### Go / No-Go Gate Criteria (Milestone 1 Exit)
To exit Milestone 1 and begin implementation of skills, the following automated gates must pass:
1. **[GATE-SEC-01] Pre-Parse Rejection:** Verification suite confirms that malformed YAML, oversized payloads (>16 KiB), and invalid signatures fail at the raw-byte layer without invoking YAML parsers ([spec 012 test 13](../../../../specs/012-xenia-record-schema.md#L321-L323)).
2. **[GATE-SEC-02] Cross-Store Replay Immunity:** Verifier rejects records copied between store repositories due to cryptographic domain separation ([spec 012 test 12](../../../../specs/012-xenia-record-schema.md#L320)).
3. **[GATE-SEC-03] Revocation Propagation:** Removing a member's key from the roster epoch immediately downgrades all unendorsed records by that author to $L_0$ upon the next sync ([spec 011 test 2](../../../../specs/011-xenia-trust-model.md#L224)).
4. **[GATE-SEC-04] Pointer Taint Isolation:** Resolving a pointer target from Drive, HTTPS, or an external git remote stamps content as $L_0$ regardless of the record's $L_2$ status ([spec 011 test 8](../../../../specs/011-xenia-trust-model.md#L230-L232)).
5. **[GATE-PERF-01] Sync Latency:** Syncing and indexing 1,000 records takes $<1.5$ seconds locally.

---

## 8. Formal Review Verdicts

| File | Verdict | Concur? | Summary / Justification |
| :--- | :---: | :---: | :--- |
| [specs/011-xenia-trust-model.md](../../../../specs/011-xenia-trust-model.md) | **approve** | **true** | Thoroughly addresses all Round 1 P0 security findings. Trust model, taint lattice mapping, and quorum rules are sound. Approved for Phase 1 implementation. |
| [specs/012-xenia-record-schema.md](../../../../specs/012-xenia-record-schema.md) | **approve** | **true** | Detached raw-byte signature scheme, verify-before-parse pipeline, and explicit forbidden key checks are robust. Recommended for phased delivery (`memory` in Phase 1; `skill` in Phase 2; `progress`/`request`/`claim` in Phase 3). |

*(Per instruction, no other files were reviewed or issued verdicts.)*
