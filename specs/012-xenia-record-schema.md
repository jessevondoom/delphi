---
id: spec-012-xenia-record-schema
title: "012: Xenia Record Schema: Envelope, Phase 1 Kinds, Signing & Store Layout"
author: architect
timestamp: 2026-10-24T00:00:00Z
status: draft   # v2 after Councils 008 and 009; awaiting a confirming review and operator ratification
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: xenia-org-memory
tags: [spec, xenia, schema, post-v1]
---

# Spec 012: Xenia Record Schema (v2)

> **Status: draft v2, post-v1.** Depends on [`specs/011`](011-xenia-trust-model.md); every field exists to support a rule there. Phase 1 kinds (`memory`, `endorsement`, `tombstone`) are normative; the other kinds are in §11 as provisional and are re-reviewed when their phase starts.

## 1. Scope & Objectives
Defines the on-disk shape of everything Phase 1 shares:
1. The **record envelope** (§2) and **signing** (§3).
2. **Pointers** (§4), display-only in Phase 1.
3. The Phase 1 **kinds** (§5): `memory`, `endorsement`, `tombstone`.
4. **Limits and validation order** (§6), **store layout** including the roster chain and freshness statement (§7), **local mapping** (§8), **versioning** (§9) and **tests** (§10).

### 1.1 Non-Goals
Sync algorithm details beyond spec 011, MCP tool names, search ranking, and the provisional kinds (§11).

### 1.2 Simpler Alternative Considered
- **Alternative:** one JSON blob per record with the signature inside it.
- **Why Rejected:** a signature inside the signed document needs canonicalization, and canonicalization bugs are a classic way to forge records. Records are **Markdown with YAML frontmatter** (like every Delphi artifact, readable in a diff, editable by hand before approval) and the signature is a **detached sidecar over the raw bytes**, so there is nothing to canonicalize.

---

## 2. Record Envelope
A record is one file at `records/<kind>/<id>.md`: YAML frontmatter, then an optional small Markdown body. The **path is authoritative** for `kind` and `id` (spec 011 §3.4); the frontmatter must agree.

```yaml
---
xenia: 1                                  # schema major version
id: 01JBX3Q8T2M9Z4K7WYH5N6D0RA            # ULID, assigned at first publish, never reused; equals the file name
kind: memory | endorsement | tombstone    # equals the directory name
rev: 3                                    # integer >= 1, strictly increasing per id (spec 011 §3.4.5)
org: acme                                 # [a-z0-9-]{1,32}; must equal the install's pinned value
store: acme-main                          # [a-z0-9-]{1,32}; must equal the pinned value; bound into the signature
author: "did:key:z6Mk..."                 # signer; must equal the sidecar kid
author_handle: "jlee"                     # display only; the roster is authoritative
author_role: operator | architect | builder | investigator | scout | strategist | red-team | writer | integrator | orchestrator
                                          # canonical IDs only (D-17); `operator` = hand-written by the human
title: string                             # <= 120 chars, plain text
summary: string                           # <= 500 chars, plain text, one paragraph, no Markdown links
tags: [string]                            # <= 12, each [a-z0-9-]{1,32}; also used for steward area matching (spec 011 §2.3)
visibility: org                           # Phase 1: `org` only; a routing hint, NOT secrecy (spec 011 §5.4)
created: "2026-10-24T09:00:00Z"           # first publish time, UTC
updated: "2026-10-24T09:00:00Z"           # this rev's time, UTC
source_ceiling: user_ratified | internal_empirical | internal_generated | federated_verified | federated_peer | external_untrusted
                                          # stamped by the publishing daemon from span ceilings, never by an agent
confidence: low | medium | high | verified   # author's claim only; import caps it at `medium` (§8)
status: active | superseded | withdrawn   # what the AUTHOR says; steward tombstones are separate (§5.3)
supersedes: string | null                 # id@rev this replaces, if any
review_by: "2027-01-24"                   # required for `memory`; after it the record shows as stale
expires: "2026-11-24"                     # OPTIONAL hard expiry
pointers: [Pointer]                       # <= 8 (§4)
evidence: [Pointer]                       # OPTIONAL, <= 4; informational, raises no tier (spec 011 §5.1)
related: [string]                         # OPTIONAL, <= 8 ids of other records
redaction:                                # written by delphi-core at publish (spec 011 §7.1)
  scanner: "rev-sec-05"
  scanner_version: "1"
  result: passed
  content_hash: "blake3:..."              # hash of body + pointers as scanned
body_kind: markdown | none
# kind-specific fields: §5
---
Optional Markdown body, <= 4 KiB. Data, never instructions.
```

### 2.1 Field rules that carry trust
- `author` must equal the sidecar `kid`; `id` and `kind` must equal the path; `org` and `store` must equal the install's pinned values.
- `source_ceiling` uses the **same enum as the local lattice** (spec 001 §5) so the receiver's $\min$ (spec 011 §5.1) is a lookup. The receiver clamps to its own xenia ceiling of $L_2$ regardless.
- `status` is the author's own state machine: a new `rev` may only move `active → superseded | withdrawn`, or stay `active`; an author-set `withdrawn` is terminal. A tombstone is separate and reversible (§5.3).
- **No field states a trust tier.** `tier`, `trust`, `trust_class`, `source_type`, `endorsed` and `verified_by_org` are rejected by the validator (§6): trust is computed locally from the roster, endorsements and freshness.

---

## 3. Signing and the Sidecar
For each record `records/<kind>/<id>.md` there is a sidecar `records/<kind>/<id>.sig.json`:

```json
{
  "alg": "ed25519",
  "kid": "did:key:z6Mk...",
  "signed_at": "2026-10-24T09:00:03Z",
  "roster_epoch": 7,
  "hash": "blake3:...",
  "sig": "base64url..."
}
```
- `hash` = BLAKE3 of the **raw bytes** of the `.md` file. `sig` = Ed25519 over the UTF-8 string:
  `xenia/1\n<org>\n<store>\n<kind>\n<id>\n<kid>\n<hash>\n<roster_epoch>\n<signed_at>`
- **The verifier builds `<org>`, `<store>`, `<kind>`, `<id>` and `<kid>` from its own pinned configuration, the file's path and the sidecar's `kid`, never from the record.** A record copied into another store, renamed to another `id`, moved to another kind directory, or presented under another key therefore fails verification.
- **Strict Ed25519 verification is mandatory** (RFC 8032 strict: reject non-canonical `S`, small-order public keys and non-canonical encodings; no batch or lenient (ZIP-215) mode for validity decisions). The repository carries test vectors, including known-bad ones (§10 test 20).
- The verifier recomputes the hash from the bytes it has. Line endings are LF, normalized **by the publisher before hashing**; the verifier never rewrites bytes. The store's `.gitattributes` (§7) keeps checkouts byte-exact.
- `signed_at` and `roster_epoch` are inside the signature so they cannot be altered, but the author chooses them, so **no validity, revocation or ordering decision may depend on them** (spec 011 §3.4). They exist for debugging and for explaining a rejection.
- **Verification precedes parsing.** The verifier reads the sidecar (strict JSON, fixed fields, at most 1 KiB), checks the hash and the strict signature over the raw bytes, then the signer's standing in the roster (the same order as spec 011 §3.4), and only then parses the YAML (§6).

**Roster and freshness signatures** use the same discipline:
- `roster/epochs/<n>.sigs.json` is `{ "epoch": <int>, "signatures": [ { "kid": "did:key:…", "sig": "base64url…" } ] }`, each `sig` over `xenia-roster/1\n<org>\n<store>\n<epoch>\n<blake3 of <n>.json>`. Signatures count per **person** (spec 011 §3.2); duplicate people count once.
- `roster/freshness.sigs.json` is `{ "signatures": [ { "kid", "sig" } ] }`, each `sig` over `xenia-fresh/1\n<org>\n<store>\n<blake3 of freshness.json>`.

---

## 4. Pointers (display-only in Phase 1)
```yaml
- scheme: git | https | xenia | drive    # `drive` is accepted by the schema but not publishable until Phase 3
  locator: string                         # per scheme, below
  pin:
    kind: git-commit | git-tree | content-hash | drive-revision | none
    value: string                         # sha / revision id / "blake3:..."
  hash: "blake3:..."                      # OPTIONAL content hash of the pinned bytes
  range: "L40-L120"                       # OPTIONAL line range (git, https text only)
  label: string                           # <= 80 chars, plain text
  content_type: text/markdown | text/plain | application/pdf | application/json | image/png | other
  bytes: 18234                            # OPTIONAL expected size
  access_hint: string                     # OPTIONAL, <= 120 chars, plain text, e.g. "payments-eng Drive group"
```
| `scheme` | `locator` | Allowed `pin.kind` |
| :--- | :--- | :--- |
| `git` | `<remote-url>#<path>`; remote is `https://…` or `ssh://…` with no embedded credentials | `git-commit`, `git-tree` |
| `https` | URL without userinfo or query tokens | `content-hash`, `none` |
| `xenia` | `<kind>/<id>[@rev]`, another record in this store | none needed |
| `drive` | Drive file ID (never a share URL with a token) | `drive-revision`, `content-hash` |

- **Phase 1 never fetches a pointer target.** The client shows the fields as text; the operator opens the target with their own tools, and whatever they bring into a mission is external content under the usual rules ($L_0$, promotion through the existing flow). There is no resolver, hence no SSRF surface in Phase 1 (spec 011 §6, §11.2).
- **A pin proves integrity, not trust,** and **no locator makes a target trusted**, including a URL that points at the org's own repository (a fork-PR commit is served through the same URL).
- **Never publishable:** `file:`, relative or absolute local paths, `delphi://` capability URIs (spec 007 §2.2: tokens never appear in URIs), URLs containing credentials, tokens or signed-URL query parameters. The publish scan rejects them; ingest rejects them again.
- A pointer's `pin` is covered by the record signature, so changing the target means a new `rev` (and a new endorsement).
- `access_hint` is plain text. It never grants anything.

---

## 5. Kinds (Phase 1)
Unknown fields are rejected (strict schema), because an ignored field is a smuggling channel.

### 5.1 `memory`: a finding, decision, gotcha or how-to
```yaml
memory_type: finding | decision | gotcha | how-to
applies_to:                               # retrieval hints
  repos: [string]                         # logical names, not local paths
  tech: [string]                          # e.g. [postgres, stripe-webhooks]
  paths: [string]                         # OPTIONAL repo-relative globs
verified_by: none | review | test | ci   # how the author says they checked it (informational only)
```
- `review_by` is required (default 90 days from `updated`). Past it the record shows as **stale** and ranks lower; it is not deleted.
- `decision` records should link the discussion in `pointers` and name any superseded decision in `supersedes`.

### 5.2 `endorsement`: a steward's attestation (or a member's dispute)
```yaml
target: "<id>@<rev>"                      # an exact revision; a new rev of the target needs a new endorsement
target_hash: "blake3:<64 hex>"            # REQUIRED: BLAKE3 of the target's raw bytes as the signer reviewed them
verdict: endorse | dispute | withdraw
scope: [string]                           # topic areas covered; must be a subset of the signer's areas intersect the target's tags
note: string                              # <= 300 chars; required for `dispute`
checked: [target-open, evidence-open, summary-matches]   # what the signer attests they did
```
- The verifier recomputes the BLAKE3 of the target's raw bytes; an endorsement counts only if it equals `target_hash`. Overwriting a record at the same `rev` therefore never inherits an endorsement.
- `endorse` and `withdraw` are valid only from a steward (spec 011 §2.2) whose areas cover the target (spec 011 §2.3), and `endorse` counts only if the signer's `person` differs from the author's (spec 011 §7.3). `dispute` may come from any member.
- One endorsement record per (signer, target `id@rev`); changing the verdict is a higher `rev` of that same record. `withdraw` retracts the signer's own earlier endorsement. `dispute` flags the target and shows a warning; it does not change the tier by itself.
- Only `memory` targets can be lifted to $L_2$ in Phase 1; an endorsement of another kind is stored but has no tier effect.

### 5.3 `tombstone`: hide or restore a record
```yaml
target: "<id>"                            # the whole record, not a revision
action: withdraw | restore
reason: superseded | mistake | sensitive | departed | other
note: string                              # <= 200 chars
```
- A tombstone is data **about** a record and never changes its bytes. **A signer has at most one tombstone record per target**; changing one's mind is a higher `rev` of that same tombstone record (ordered by its monotonic `rev`, which only that signer controls). Resolution never uses ULIDs or timestamps, because the signer chooses them.
- **Who may sign:** the record's author (the same person, including the successor chain), or a steward whose areas cover the target's tags **taken as the union across all revisions** (spec 011 §2.3).
- **Effective status**, deterministic:
  1. If the author has a valid tombstone, its `action` decides, **except** that a steward's `withdraw` with `reason: sensitive` can be reversed only by that steward.
  2. Otherwise, if every valid steward tombstone says the same `action`, that decides.
  3. Otherwise the status is **`contested`**: hidden like a withdrawn record, and `delphi xenia status` names the stewards so humans or admins resolve it.
- A `withdraw` keeps effect after its signer is no longer `active` (retired or removed), unless the signer's roster entry has `voids_tombstones` (mandatory with `rotate --compromised`); `restore` from a non-active DID has no effect (spec 011 §3.4.3, §7.4). "That steward" in the `sensitive` rule means the same **person** through the `successor` chain.
- If two tombstone files by one signer for one target exist with different ULIDs, the smallest ULID survives and the other is quarantined as `duplicate` (spec 011 §3.4.5); this is a tie-break only and never decides effect.
- `sensitive` should trigger the rotate-secrets reminder (spec 011 §7.1).

---

## 6. Limits & Validation
Checked by `delphi-core`, in this order, failing closed into quarantine. **Steps 1–3 run on raw bytes, the path and the tiny sidecar; the YAML parser does not run until all three pass.**

| # | Check | Limit |
| :-: | :--- | :--- |
| 1 | Raw size, sidecar parse | record file $\le$ 16 KiB, sidecar $\le$ 1 KiB, strict JSON with the fixed fields; body $\le$ 4 KiB (checked after step 5 parses) |
| 2 | Path | `records/<kind>/<id>.md` where `kind` is a known kind directory and `<id>` is a valid ULID |
| 3 | Signature and standing | spec 011 §3.4 steps 2–3: hash, strict Ed25519 over the store-bound domain (§3), signer `active` at the current epoch |
| 4 | Frontmatter parse | YAML anchors, aliases, tags (`!!`) and merge keys rejected; duplicate keys rejected |
| 5 | Schema | strict per kind; unknown keys rejected; **explicitly rejected keys:** `tier`, `trust`, `trust_class`, `source_type`, `endorsed`, `verified_by_org` |
| 6 | Cross-checks | `id` and `kind` equal the path; `org`, `store` equal the pinned values; `author` equals the sidecar `kid` |
| 7 | Strings | NFC-normalized; no control characters except `\n` in the body; no bidi controls or `U+E0000`–`U+E007F`; plain-text fields contain no Markdown links or HTML |
| 8 | Counts | `summary` $\le$ 500 chars; pointers $\le$ 8, evidence $\le$ 4, tags $\le$ 12, related $\le$ 8 |
| 9 | Pointers | scheme allowlist (`git`, `https`, `xenia`; `drive` rejected in Phase 1), no credentials in locators, pin kinds valid for the scheme |
| 10 | Dates | valid ISO-8601 UTC; `updated` $\ge$ `created`; `review_by`/`expires` in the future at publish |
| 11 | Model exposure | nothing from a record reaches a model until all of the above pass |

Size limits matter for the threat model (spec 011 T19) and for context hygiene: a bounded summary is what enters an agent's context.

---

## 7. Store Layout
```
xenia-<org>/                      # a git repository (the "store")
├── .gitattributes                # `* -text`: no line-ending conversion, so raw-byte hashes survive every checkout.
│                                 #   Clients also clone with core.autocrlf=false and refuse a store without this file
├── org.toml                      # ADVISORY only: org id, store id, accepted schema majors, min client version.
│                                 #   Never trusted; installs use values pinned locally at join (spec 011 §3.3)
├── roster/
│   ├── epochs/                   # immutable chain; files are never overwritten (spec 011 §3.2)
│   │   ├── 1.json                # { org, store, epoch, prev_hash, members:[{did, person, handle, roles, steward_areas, added_at,
│   │   │                         #     status: active|retired|removed, successor?, removed_at_epoch?, voids_tombstones?}] }
│   │   ├── 1.sigs.json           # { epoch, signatures:[{kid, sig}] }; epoch 1 signed by all founding admins
│   │   ├── 2.json                # prev_hash = blake3 of 1.json; signed by a majority (per person) of epoch 1's active admins
│   │   └── 2.sigs.json
│   ├── freshness.json            # { org, store, epoch, epoch_hash, store_head, issued_at, expires_at, rewrite_of? }  (spec 011 §4)
│   │                             #   `rewrite_of` (the replaced head) is present only for a rewrite-recovery statement
│   └── freshness.sigs.json       # one active admin of the named epoch; an admin QUORUM (per person) when `rewrite_of` is present
├── records/
│   ├── memory/<id>.md            # one file per record; overwritten on a new rev (history is git's)
│   ├── memory/<id>.sig.json
│   ├── endorsement/…
│   └── tombstone/…
└── indexes/                      # OPTIONAL generated caches, never trusted, always rebuildable
```
- **`roster/proposal` flow.** Draft epochs live on a pending branch `roster-proposal/<n>` until admins have signed the same bytes; main only ever receives a fast-forward of a fully signed epoch (spec 011 §3.2).
- The `indexes/` directory is a convenience and is **never** read for trust decisions. Every install rebuilds its own index from verified records into a local SQLite table.
- **Clone location:** the store is cloned to `$DELPHI_HOME/xenia/<org>/<store>/`, outside every workspace zone, readable only by `delphi-core` (spec 011 §3.3.5). It never sits under `team/`, so it cannot take the `team/` origin default tier.
- Writes are append-style: add a file, or overwrite a record with a higher `rev` (a valid sidecar must accompany it). **Deleting any file under `records/` or `roster/` is tampering** (spec 011 §4.2.3).
- The store carries **no** operator-scope data, no tokens and no private keys. The roster contains public DIDs only.
- One store = one audience (spec 011 §5.4). Each is pinned locally in `$DELPHI_HOME/xenia/<org>/<store>/state/pins.toml`, never in `team/` (spec 011 §3.5).

---

## 8. Mapping to Local Delphi Artifacts
On ingestion `delphi-core` creates a **local cache entry** (never a file in the always-loaded set) with Delphi's provenance frontmatter (spec 001 §5), computed locally:

| Local frontmatter | Value |
| :--- | :--- |
| `id` | `xenia:<org>/<store>/<kind>/<id>@<rev>` |
| `title` | record `title` |
| `author` | the record's `author_role`, or `operator` |
| `timestamp` | record `updated` |
| `source_type` | computed (spec 011 §5.1): `federated_verified`, `federated_peer` or `external_untrusted` |
| `confidence` | record `confidence`, **capped at `medium` unless a local replay makes it `verified`** |
| `status` | active→`active`; superseded, author-`withdrawn`, tombstoned or `contested`→`superseded` and hidden |
| `superseded_by` | required when `status` is `superseded` (spec 001 §5): the local id of the superseding record if `supersedes` names one, otherwise `tombstone:<tombstone id>`, `author-withdrawn` or `contested` |
| `source_mission` | `xenia:<org>` |
| `tags` | record `tags` |
| `ttl_missions` | derived from `review_by` |

The provenance index stamp is written by the daemon as a `provenance.stamp` event (spec 001 §5.1). The record's own claims about itself are display copies only.

---

## 9. Versioning
- `xenia: 1` is the major version. A client rejects records with a higher major it does not understand, quarantining them as `unsupported-schema` (not as attacks); the pinned configuration declares accepted majors.
- Because unknown fields are rejected, new optional fields roll out **reader-first**: the validator ships the field before any publisher writes it.
- Signatures name the domain `xenia/1`, so a v2 signature can never be mistaken for a v1 one.

---

## 10. Verification Plan
1. **Round-trip test:** build one valid record of each Phase 1 kind (`memory`, `endorsement`, `tombstone`), publish to a temp store, sync into a second install; the cache entry has the expected local frontmatter and tier.
2. **Strict schema test:** unknown key, duplicate key, YAML alias/anchor/merge key, bidi character, over-limit summary, over-count pointers, and each of `tier`, `trust`, `trust_class`, `source_type`, `endorsed`, `verified_by_org` are rejected into quarantine with a specific reason.
3. **Detached-signature test:** change one byte of the `.md`; change only the sidecar `hash`; use a `kid` different from `author`: each fails.
4. **Domain-binding test:** copy a valid record and sidecar into another store of the same org; rename the files to another `id`; move them to another kind directory; present them under another `kid`: each fails signature verification.
5. **Path/id test:** a file at `records/memory/B.md` whose frontmatter `id` is `A` is rejected, even if its signature is valid for `B`; two files can never present the same `id`.
6. **Monotonic rev and ownership test:** a record whose `rev` is not greater than the stored one is rejected; a different member cannot reuse an existing `id`; a successor of the creator can publish the next `rev`; on a **fresh install** a record overwritten by a different member at a higher `rev` is detected as a takeover through the history walk (signer continuity, spec 011 §4.2.3).
7. **Author state test:** an author's `withdrawn → active` is rejected; `active → superseded` with a valid `supersedes` is accepted.
8. **Endorsement binding test:** an endorsement of `id@2` does not apply to `id@3`; one whose `target_hash` does not match the recomputed hash is ignored; one without `target_hash` is rejected; replacing a record's bytes at the same `rev` voids the endorsement.
9. **Tombstone resolution test:** author beats stewards; stewards in disagreement give `contested` (hidden); a `sensitive` steward withdraw is reversed only by that steward; a far-future ULID gains no priority; a second tombstone record by the same signer for the same target is rejected unless it is a higher `rev` of the first; area coverage uses the union of tags across revisions.
10. **Pointer rules test:** `file:`, local paths, `delphi://` URIs, userinfo URLs, signed-URL query tokens and a `drive` pointer in Phase 1 are rejected at publish and at ingest; a scheme outside the allowlist is rejected.
11. **No-fetch test:** a run with all network access denied ingests, displays and verifies records with pointers and makes no network attempt for any pointer target.
12. **Verify-before-parse test:** an oversized file, a bad signature and a non-`active` signer each fail with the YAML parser never invoked (instrumented parser); a file that passes steps 1–3 and then fails the schema is quarantined with the schema reason.
13. **Roster epoch schema test:** a wrong `prev_hash`, a different `store`, a numbering gap, an entry dropped from a later epoch, a `retired`/`removed` DID returned to `active`, or a changed `person` for an existing DID (a `successor` must inherit its predecessor's `person` exactly) is rejected; overwriting an existing epoch file is detected through the next epoch's `prev_hash` and the pinned epoch hashes.
14. **Roster signature test:** duplicate people count once; signatures by `retired`/`removed` DIDs are ignored; a signature over a different epoch or store fails; a signature over different bytes counts for nothing.
15. **Freshness file test:** `freshness.json` with another org/store/epoch, a signature from a non-admin, or an expired `expires_at` is ignored; one whose validity exceeds the local cap or whose `issued_at` is in the future is rejected; one carrying `rewrite_of` with fewer than a quorum of signers (per person) is rejected; a store head that is not a descendant of `store_head`, that has deleted a path present at `store_head`, or in which any path's `rev` decreased (or changed hash at the same `rev`) since `store_head` is rejected (spec 011 §4.2).
16. **Line-ending test:** a store cloned with `core.autocrlf=true` still verifies every record (via `.gitattributes`); a store missing `.gitattributes` is refused with a specific reason.
17. **Derivation test:** deleting `indexes/` and the local SQLite index and re-syncing reproduces identical tiers.
18. **Version test:** a record with `xenia: 2` is quarantined as `unsupported-schema` and does not break the sync of other records.
19. **Confidence cap test:** a record claiming `confidence: verified` imports as `medium` until a local replay raises it.
20. **Strict signature vectors:** valid vectors pass; vectors with non-canonical `S`, small-order public keys and non-canonical encodings fail; the same vectors give the same results on every supported platform.
21. **Revision revert test:** a record file replaced by its own valid older `rev` (an endorsement `withdraw` rev 2 back to `endorse` rev 1; a tombstone `restore` back to `withdraw`; a `memory` rev 3 back to rev 2) is rejected on a fresh install via the history walk and on an install with sticky state; the same `rev` with a different hash is rejected; the same `rev` with an identical hash is accepted (idempotent re-sync).
22. **Non-active tombstone test:** a `withdraw` tombstone from a retired or removed DID still hides its target; a `restore` from the same DID does nothing; with `voids_tombstones` set, the `withdraw` has no effect.
23. **Local mapping test:** a hidden, withdrawn, tombstoned or `contested` record maps to local `status: superseded` with a populated `superseded_by`, and passes `delphi lint`'s provenance frontmatter check.

---

## 11. Provisional Kinds (Phase 2–3, not binding)
Re-reviewed before their phase starts. Common envelope (§2) applies.
- **`skill`** (Phase 2): `skill_name` (`^[a-z0-9-]{1,48}$`, installs as `skills/custom/xenia/<org>/<skill_name>/`, spec 011 §11.1), `skill_version` (semver), `manifest_hash`, `tree` (a git pointer with `pin.kind: git-tree`, covering every file), `compat`, and `requires` (roles, `access_scope` read/write/execute, `network`, `network_hosts`). Adoption recomputes `requires` from the fetched `SKILL.md` and refuses on mismatch.
- **`progress`** (Phase 3): `subject`, `period`, `state: on-track|at-risk|blocked|done`, up to five `highlights`/`blockers`/`next` of 160 chars each; `expires` required (default 30 days). Eligible for $L_2$ endorsement only when Phase 3 says so.
- **`request`** (Phase 3): `ask` ($\le$ 300 chars), `needs` tags, `urgency`, `deadline?`, `lifecycle: open|closed`, `expires` required.
- **`claim`** (Phase 3): `request: "<id>@<rev>"`, `stance: offer|working|delivered|withdrawn`, `result?` pointers, `note`; only `offer` and `working` count toward quotas (spec 011 §11.5).
- **Roster additions** (Phase 2): invite file format (spec 011 §11.3) and an optional vouch list per retirement epoch (spec 011 §11.4), each with its own signature domain.

---

## 12. Open Decisions for the Operator
| # | Question | Position after Councils 008 and 009 |
| :-: | :--- | :--- |
| S-1 | Markdown+YAML with detached signatures vs JSON? | Markdown+YAML; matches every Delphi artifact; with the `id` in the signed domain and the path/id check |
| S-2 | `progress` as separate expiring records? | Yes, Phase 3 |
| S-3 | 16 KiB / 500-character caps? | Start strict; relaxing later is safe, tightening is breaking |
| S-4 | Skills as pinned git trees only? | Yes, Phase 2 |
| S-5 | Claims public in v1? | Yes, Phase 3; per-member quotas |
| S-6 (new) | Which kinds can be lifted to $L_2$ in Phase 1? | `memory` only |
