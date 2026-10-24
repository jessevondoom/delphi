---
id: spec-012-xenia-record-schema
title: "012: Xenia Record Schema: Envelope, Kinds, Pointers & Store Layout"
author: architect
timestamp: 2026-10-24T00:00:00Z
status: draft   # post-v1 design; Council 008 approve-with-fixes (homogeneous), fixes applied; awaiting operator ratification
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low
source_mission: xenia-org-memory
tags: [spec, xenia, schema, post-v1]
---

# Spec 012: Xenia Record Schema

> **Status: draft, post-v1.** Depends on [`specs/011`](011-xenia-trust-model.md); every field below exists to support a rule there. If a field has no trust or retrieval purpose, it should not be here.

## 1. Scope & Objectives
Defines the on-disk and on-wire shape of everything xenia shares:
1. The **record envelope** common to all kinds (§2).
2. **Signing** and the sidecar format (§3).
3. The **pointer** object (§4).
4. The **kinds**: `memory`, `skill`, `progress`, `request`, plus the meta-kinds `claim`, `endorsement`, `tombstone` (§5).
5. **Limits and validation** (§6) and **store layout** (§7).
6. How a received record maps onto Delphi's local provenance frontmatter (§8).

### 1.1 Non-Goals
Transport and sync algorithm, MCP tool names, search ranking, and the librarian's behavior (later specs).

### 1.2 Simpler Alternative Considered
- **Alternative:** one JSON blob per record with the signature inside it.
- **Why Rejected:** a signature inside the signed document needs a canonicalization step, and canonicalization bugs are a classic way to forge records. Records are **Markdown with YAML frontmatter** (consistent with every other Delphi artifact, readable in a diff, editable by hand before approval), and the signature is a **detached sidecar over the raw bytes**, so there is nothing to canonicalize.

---

## 2. Record Envelope
A record is one file: YAML frontmatter, then a Markdown **body**. The frontmatter is the machine contract; the body is human detail and is **optional and small** (the real depth lives behind pointers).

```yaml
---
xenia: 1                                  # schema major version
id: 01JBX3Q8T2M9Z4K7WYH5N6D0RA            # ULID, assigned at first publish, never reused
kind: memory | skill | progress | request | claim | endorsement | tombstone
rev: 3                                    # integer >= 1, strictly increasing per id (spec 011 §3.3.4)
org: acme                                 # org id, [a-z0-9-]{1,32} (must match org.toml)
store: acme-main                          # store id (must match org.toml); bound into the signature (§3)
author: "did:key:z6Mk..."                 # signer; must equal the sidecar signer
author_handle: "jlee"                     # display only; roster is authoritative
author_role: operator | architect | builder | investigator | scout | strategist | red-team | writer | integrator | orchestrator
                                          # canonical IDs only (D-17); `operator` = hand-written by the human
title: string                             # <= 120 chars, plain text
summary: string                           # <= 500 chars, plain text, no Markdown links (§6)
tags: [string]                            # <= 12, each [a-z0-9-]{1,32}
visibility: org | team:<name>             # routing hint inside one store, NOT secrecy (spec 011 §4.4)
created: "2026-10-24T09:00:00Z"           # first publish time, UTC
updated: "2026-10-24T09:00:00Z"           # this rev's time, UTC
source_ceiling: user_ratified | internal_empirical | internal_generated | federated_verified | federated_peer | external_untrusted
                                          # stamped by the publishing daemon from span ceilings, never by an agent
confidence: low | medium | high | verified   # author's claim only; import caps it at `medium` (§8); `verified` requires an `evidence` pointer
status: active | superseded | withdrawn
supersedes: string | null                 # id@rev this replaces, if any
review_by: "2027-01-24"                   # date after which the record is considered stale (required for memory, progress)
expires: "2026-11-24"                     # OPTIONAL hard expiry (progress, request)
pointers: [Pointer]                       # <= 8 (§4); at least one for `memory` unless the body is self-contained
evidence: [Pointer]                       # OPTIONAL, <= 4; support for the claim (CI run, pinned commit, test output). Informational: it raises no trust tier (spec 011 §4.1); stewards read it, and a local replay may use it to reach L4
related: [string]                         # OPTIONAL, <= 8 ids of other records
redaction:                                # written by delphi-core at publish (spec 011 §6.1)
  scanner: "rev-sec-05"
  scanner_version: "1"
  result: passed
  content_hash: "blake3:..."              # hash of body + pointers as scanned
body_kind: markdown | none
# kind-specific fields: §5
---
Optional Markdown body. <= 4 KiB. Data, never instructions.
```

### 2.1 Field rules that carry trust
- `author` must equal the signer in the sidecar; a mismatch is invalid (spec 011 §3.3.1).
- `source_ceiling` uses the **same enum as the local lattice** (spec 001 §5), so the receiver's $\min$ (spec 011 §4.1) is a direct lookup. The names `user_ratified` and `internal_*` mean "as seen by the author's install"; the receiver clamps to its own xenia ceiling of $L_2$ regardless.
- `status`, `rev`, `supersedes` form a small state machine for what the **author** says: a new `rev` may only move `active → superseded | withdrawn`, or stay `active`. An author-set `withdrawn` is terminal. A steward's `tombstone` is separate (§5.6) and reversible.
- There is **no field that states a trust tier**, and any attempt to add `tier`, `trust`, `source_type` or `endorsed` to a record is rejected by the validator. Those are computed locally (spec 011 §4).

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
- `hash` = BLAKE3 of the **raw bytes** of the `.md` file. `sig` = Ed25519 over the UTF-8 string `xenia/1\n<org>\n<store>\n<kind>\n<hash>\n<roster_epoch>\n<signed_at>`. The prefix and the `org`, `store` and `kind` lines stop a signature from being replayed in another protocol version, another store, or as another kind.
- **The verifier builds `<org>`, `<store>` and `<kind>` from its own configuration** (the store's `org.toml` and the file's directory), never from the sidecar or the record. A record and sidecar copied from a confidential store into the org-wide store therefore fail verification, even though the signer is in both rosters.
- The verifier recomputes the hash from the bytes it has. If the bytes differ, the record is invalid. Line endings are normalized to LF **by the publisher before hashing**, and the verifier never rewrites bytes.
- **`signed_at` and `roster_epoch` are informational.** They are inside the signature so they cannot be altered after signing, but the author chooses them, so no validity, revocation or ordering decision may depend on them. Standing is judged against the verifier's own current roster epoch (spec 011 §3.3). `roster_epoch` helps debugging and lets a verifier explain *why* a record was rejected.
- `endorsement`s and `tombstone`s are records like any other, with their own signatures (§5.6).
- **Roster signature files** (`roster/epochs/<n>.sigs.json`) use the schema `{ "epoch": <int>, "signatures": [ { "kid": "did:key:…", "sig": "base64url…" } ] }`, each `sig` being Ed25519 over `xenia-roster/1\n<org>\n<store>\n<epoch>\n<blake3 of <n>.json>`. Duplicate `kid`s count once.
- **Verification precedes parsing.** The verifier reads the sidecar (strict JSON, fixed fields, tiny), checks the signer against the roster, checks the hash and signature over the raw bytes, and only then parses the YAML frontmatter (§6).

---

## 4. Pointers
```yaml
- scheme: git | drive | https | xenia
  locator: string                         # per scheme, below
  pin:
    kind: git-commit | git-tree | content-hash | drive-revision | none
    value: string                         # sha / revision id / "blake3:..."
  hash: "blake3:..."                      # OPTIONAL content hash of the pinned bytes
  range: "L40-L120"                       # OPTIONAL line range (git, https text only)
  label: string                           # <= 80 chars, plain text
  content_type: text/markdown | text/plain | application/pdf | application/json | image/png | other
  bytes: 18234                            # OPTIONAL expected size
  access_hint: string                     # OPTIONAL, <= 120 chars, e.g. "payments-eng Drive group"; plain text
```
| `scheme` | `locator` | Allowed `pin.kind` | Resolved by |
| :--- | :--- | :--- | :--- |
| `git` | `<remote-url>#<path>` (remote is `https://…` or `ssh://…`; no embedded credentials) | `git-commit`, `git-tree` | Reader's own git credentials |
| `drive` | Drive file ID (never a share URL with a token) | `drive-revision`, `content-hash` | Reader's Google connector (`specs/010` §7.2) |
| `https` | URL without userinfo or query tokens | `content-hash`, `none` | Plain fetch |

**Resolver guards (SSRF).** Fetching a pointer happens only on an explicit quarantine read, with no cookies, credentials or ambient auth for `https`. The resolver (1) accepts only `https`, and `git` remotes over `https` or `ssh`; (2) resolves DNS itself and **rejects loopback, private (RFC 1918 and unique-local), link-local (including `169.254.169.254` and cloud metadata hosts) and unspecified addresses**, re-checking the connected address to defeat DNS rebinding; (3) follows at most 3 redirects and applies the same address check to every hop; (4) caps response size at the record's declared `bytes` (or 2 MiB) and time at 15 seconds. An operator may allowlist an internal host in their own config; a record cannot.

**Phasing (spec 011 §1.2a):** Phase 1 supports `git`, `https` and `xenia` pointers; `drive` is accepted by the schema from the start but is not resolvable, and is not publishable, until the Drive connector ships (Phase 3). An install that cannot resolve a scheme shows the pointer as plain text with its `access_hint`.

**Pointer content is always read as $L_0$** (`drive`, `https` and any non-local git remote), whatever the pin and whatever the record's tier (spec 011 §5.2, `specs/010` §5.1). A pin proves integrity (the bytes the signer saw), not trust. A `xenia` pointer resolves to another record, which has its own tier.
| `xenia` | `<kind>/<id>[@rev]` | none needed | Another record in this store |

- **Never publishable:** `file:`, relative or absolute local paths, `delphi://` capability URIs (spec 007 §2.2: tokens must never appear in URIs), any URL containing credentials, tokens or signed-URL query parameters. The publish scan rejects them.
- `access_hint` is plain text for humans. It never grants anything (spec 011 §5.1).
- A pointer's `pin` is covered by the record signature, so changing the target after signing is detectable (spec 011 §5.2).

---

## 5. Kinds
All kinds share §2. Below are the additional fields. Unknown fields are rejected (strict schema), because an ignored field is a smuggling channel.

### 5.1 `memory`: a finding, decision, gotcha or how-to
```yaml
memory_type: finding | decision | gotcha | how-to
applies_to:                               # retrieval hints
  repos: [string]                         # logical names, not local paths
  tech: [string]                          # e.g. [postgres, stripe-webhooks]
  paths: [string]                         # OPTIONAL repo-relative globs
verified_by: none | review | test | ci   # how the author says they checked it (informational only)
```
- `review_by` is required (default 90 days from `updated`). Past it, the record shows as **stale** and ranks lower; it isn't deleted.
- `decision` records should link the discussion in `pointers` and name any superseded decision in `supersedes`.

### 5.2 `skill`: a shareable, adoptable skill
```yaml
skill_name: string                        # [a-z0-9-]{1,48}; installs as skills/custom/xenia/<org>/<skill_name>/, never at skills/<skill_name>/ (spec 011 §6.3)
skill_version: "1.4.0"                    # semver
manifest_hash: "blake3:..."               # hash of the skill's SKILL.md frontmatter as published
tree: Pointer                             # REQUIRED: git pointer to the skill directory, pin.kind: git-tree
compat: ">=0.1.0 <0.3.0"                  # Delphi versions the skill was tested on
requires:                                 # what adoption will show as a scope diff (spec 011 §6.3)
  roles: [builder]                        # canonical roles that would use it
  access_scope:
    read: [string]
    write: [string]
    execute: [string]
  network: none | allowlist
  network_hosts: [string]                 # present only if network: allowlist
```
- The skill's code and prompts live in the pinned git tree, **not in the record**. The record is the signed index entry; the tree hash is what adoption verifies. `manifest_hash` is a convenience for spotting a changed manifest; the **tree pin covers every file**, including `scripts/`, and adoption shows all of them as a diff (spec 011 §6.3).
- `requires` is what the author *claims* the skill needs. Adoption recomputes it from the fetched `SKILL.md` and **refuses on mismatch**, so the record cannot understate its needs.
- `skill` records are never auto-adopted. See spec 011 §6.3.

### 5.3 `progress`: a status update
```yaml
subject: string                           # project or initiative name, plain text, <= 80 chars
period: { from: "2026-10-17", to: "2026-10-24" }
state: on-track | at-risk | blocked | done
highlights: [string]                      # <= 5 items, each <= 160 chars
blockers: [string]                        # <= 5 items, each <= 160 chars
next: [string]                            # <= 5 items, each <= 160 chars
```
- `expires` is required; progress records age out (default 30 days) and are summarized by the librarian later.
- Progress records describe *outcomes*. They must not include mission transcripts or event logs (`events.jsonl` is never publishable, spec 001 §2.2).

### 5.4 `request`: a call for collaboration
```yaml
ask: string                               # <= 300 chars
needs: [string]                           # skills or areas wanted, tags
urgency: low | normal | high
deadline: "2026-11-01"                    # OPTIONAL
lifecycle: open | closed
```
- Only the author can change `lifecycle`. Whether the request is *claimed* is derived from `claim` records (§5.5), not stored on the request, because nobody else may edit the author's signed record.
- `expires` is required (default 30 days).

### 5.5 `claim`: an offer to work on a request
```yaml
request: "<id>@<rev>"                     # the request this answers
stance: offer | working | delivered | withdrawn
result: [Pointer]                         # OPTIONAL; present when delivered (<= 4)
note: string                              # <= 300 chars
```
- Only `stance: offer | working` counts toward a member's open-claim quota (spec 011 §6.4); `delivered` and `withdrawn` free it.
- Multiple claims on one request are normal. The requester picks, outside the store, or closes the request.
- `delivered` claims usually point to a `memory` record or other pointer holding the result.

### 5.6 Meta-kinds: `endorsement` and `tombstone`
**`endorsement`** (stewards and, for `dispute`, any member):
```yaml
target: "<id>@<rev>"                      # an exact revision; a new rev of the target needs a new endorsement
target_hash: "blake3:<64 hex>"            # REQUIRED: BLAKE3 of the target record's raw bytes as the steward reviewed them
verdict: endorse | dispute | withdraw
scope: [string]                           # topic areas the endorsement covers (must be within the endorser's steward areas for `endorse`)
note: string                              # <= 300 chars; required for `dispute`
checked: [target-open, evidence-open, summary-matches]   # what the steward attests they did
```
- An endorsement binds to an exact `id@rev` **and** `target_hash`. The verifier recomputes the BLAKE3 of the target's raw bytes and the endorsement counts only if it equals `target_hash`. Overwriting a record at the same `rev` with different bytes (which the monotonic-`rev` rule rejects for installs that saw the original, but a fresh install would not know) therefore never inherits an endorsement.
- **Area check.** Whether a steward may endorse or tombstone is decided from the **target record's `tags`** and the steward's `steward_areas` (spec 011 §2, area matching); a tombstone needs no `scope` field. The endorsement's `scope` must be a subset of that intersection.
- `endorse` is valid only from a steward whose areas include `scope`, and the endorser must not be the target's author (spec 011 §4.1, test 6).
- `dispute` flags the target to stewards and shows a warning on it; it does not change the tier by itself.

**`tombstone`** (the record's author, or a steward whose areas cover the record):
```yaml
target: "<id>"                            # whole record, not a revision
action: withdraw | restore
reason: superseded | mistake | sensitive | departed | other
note: string                              # <= 200 chars
```
- A tombstone is data **about** a record; it never changes the record's bytes. A signer has **at most one tombstone record per target**: changing one's mind is a new `rev` of that same tombstone (ordered by its monotonic `rev`, which only that signer controls). The record's effective status is then derived deterministically from the valid tombstones of different signers, never from ULIDs or timestamps, because the signer chooses those:
  1. If the **author** has a valid tombstone, its `action` decides (a `sensitive` withdraw can only be restored by the author, so it is final unless they restore it).
  2. Otherwise, if every valid steward tombstone says the same `action`, that `action` decides.
  3. Otherwise (stewards disagree) the status is **`contested`**: the record is hidden like a withdrawn one (fail safe), and `delphi xenia status` names the stewards involved so the humans or the admins resolve it. Removing a rogue steward voids their tombstones (spec 011 §7.1).
  So a mistaken or malicious `withdraw` is reversible by a `restore` from the author, or by agreement among covering stewards.
- A steward tombstone outside the steward's `steward_areas` is invalid. A `withdraw` with `reason: sensitive` can be restored only by the record's author.
- `sensitive` should trigger the rotate-secrets reminder (spec 011 §7.1).

---

## 6. Limits & Validation
Checked by `delphi-core`, in this order, failing closed into quarantine. **Steps 1–2 run on raw bytes and the tiny JSON sidecar; the YAML parser does not run until both pass.**

| Check | Limit |
| :--- | :--- |
| 1. Raw size | record file $\le$ 16 KiB, sidecar $\le$ 1 KiB; body $\le$ 4 KiB (checked after step 3 parses) |
| 2. Signature | spec 011 §3.3 steps 1–2: signer in the current roster, hash and Ed25519 signature over the store-bound domain string (§3) |
| 3. Frontmatter | strict schema per kind; unknown or duplicate keys rejected; **explicitly rejected keys include `tier`, `trust`, `trust_class`, `source_type`, `endorsed` and `verified_by_org`** (a record never states its own trust); YAML anchors, aliases, tags (`!!`) and merge keys rejected |
| Strings | NFC-normalized; no control characters except `\n` in the body; no bidi controls or `U+E0000`–`U+E007F`; plain text fields contain no Markdown links or HTML |
| `summary` | $\le$ 500 chars, single paragraph |
| Counts | pointers $\le$ 8, evidence $\le$ 4, tags $\le$ 12, related $\le$ 8 |
| IDs | valid ULID; `kind` directory matches `kind` field; `org` and `store` match `org.toml` |
| Pointers | scheme allowlist (`git`, `drive`, `https`, `xenia`; anything else rejected), no credentials in locators, pin kinds valid for the scheme |
| Dates | valid ISO-8601 UTC; `updated` $\ge$ `created`; `review_by`/`expires` in the future at publish |
| Model exposure | nothing from a record reaches a model until all of the above pass |

Size limits matter for the threat model (spec 011 T15) and for context hygiene: a bounded summary is what enters an agent's context.

---

## 7. Store Layout
```
xenia-<org>/                      # a git repository (the "store")
├── .gitattributes                # `* -text` : no line-ending conversion of any file, so raw-byte hashes survive every checkout. Clients also clone with core.autocrlf=false and verify the file is present
├── org.toml                      # org id, store id, schema versions accepted, min client version, store audience name
├── roster/
│   └── epochs/                   # immutable chain; files are never overwritten (spec 011 §3.2)
│       ├── 1.json                # { org, store, epoch, prev_hash, members:[{did, handle, roles, steward_areas, added_by, added_at,
│       │                         #   status: active | retired | removed, successor?, removed_at_epoch?, grandfathered_digest? }] }
│       ├── 1.sigs.json           # { epoch, signatures:[{kid, sig}] } over the exact bytes of 1.json (§3)
│       ├── 2.json                # prev_hash = blake3 of 1.json; signed by a majority of epoch 1's admins (epoch 1 by all founding admins)
│       ├── 2.sigs.json
│       └── 2.grandfathered/      # only for epochs that retire or remove a key
│           └── <fingerprint>.txt       # <fingerprint> = lowercase hex blake3 of the UTF-8 DID (64 chars, no colons). Lines `id@rev:blake3 vouched|eligible`,
│                                       #   byte-sorted (C locale), LF endings; its blake3 must equal that member's `grandfathered_digest` (spec 011 §7.1)
├── records/
│   ├── memory/<id>.md            # one file per record, overwritten on new rev (history is git's)
│   ├── memory/<id>.sig.json
│   ├── skill/…
│   ├── progress/…
│   ├── request/…
│   ├── claim/…
│   ├── endorsement/…
│   └── tombstone/…
└── indexes/                      # OPTIONAL generated caches, never trusted, always rebuildable
```
- The `indexes/` directory is a convenience and is **never** read for trust decisions. Every install rebuilds its own index from verified records into a local SQLite table in `state.db`.
- Writes are append-style: add a file or overwrite with a higher `rev`. History in git lets any install diff a record against what it saw before.
- **Publish protocol.** `delphi xenia publish` commits the record and sidecar on a local branch, runs `fetch` and a fast-forward rebase onto the store's remote head, then pushes. Every record is a distinct file, so concurrent publishers rebase without content conflicts; if the rebase touches a file with the same `id` (a same-id race, which only the same signer can create), the publish stops and asks the operator. The client never force-pushes, and it re-runs signature verification on the rebased head before pushing. Roster epoch files are written only by the admin tooling, the same way.
- The store carries **no** operator-scope data, no tokens and no private keys. The roster contains public DIDs only.
- One store = one audience (spec 011 §4.4). The client keeps a list of joined stores in `team/xenia/stores.toml`.

---

## 8. Mapping to Local Delphi Artifacts
On ingestion, `delphi-core` creates a **local cache entry** (not a file in the always-loaded set) with Delphi's provenance frontmatter (spec 001 §5), computed locally:

| Local frontmatter | Value |
| :--- | :--- |
| `id` | `xenia:<org>/<kind>/<id>@<rev>` |
| `title` | record `title` |
| `author` | the record's `author_role`, or `operator` |
| `timestamp` | record `updated` |
| `source_type` | computed (spec 011 §4.1): `federated_verified` or `federated_peer` or `external_untrusted` |
| `confidence` | record `confidence`, **capped at `medium` unless a local replay makes it `verified`** |
| `status` | map: active→`active`, superseded→`superseded`, withdrawn→`superseded` plus hidden |
| `source_mission` | `xenia:<org>` |
| `tags` | record `tags` |
| `ttl_missions` | derived from `review_by` |

- The provenance index stamp is written by the daemon as a `provenance.stamp` event (spec 001 §5.1). The record's own claims about itself are display copies only.
- `skill` adoption (spec 011 §6.3) writes a **new** artifact into `skills/` through the ratification path, stamped $L_5$, with the pinned tree hash in its manifest. The xenia cache entry stays $\le L_2$.

---

## 9. Versioning
- `xenia: 1` is the major version. A client rejects records with a higher major version it doesn't understand, quarantining them as `unsupported-schema` (not as attacks), and `org.toml` declares the accepted majors.
- Adding an optional field is a **minor**-compatible change only if the validator ships the field before any publisher writes it. Because unknown fields are rejected (§5), new fields roll out reader-first.
- Signatures name the domain `xenia/1`, so a v2 signature can never be mistaken for a v1 one.

---

## 10. Verification Plan
1. **Round-trip test:** build one valid record of each kind, publish to a temp store, sync into a second install; the cache entry has the expected local frontmatter and tier.
2. **Strict schema test:** unknown key, duplicate key, YAML alias/anchor/merge key, bidi character, over-limit summary, over-count pointers, and a trust-tier field (`tier`, `trust`, `source_type`, `endorsed`) are each rejected into quarantine with a specific reason.
3. **Detached-signature test:** change one byte of the `.md`, then change only the sidecar `hash`; both fail. A signature from a different `kid` than `author` fails.
4. **Pointer rules test:** `file:`, local paths, `delphi://` URIs, userinfo URLs and signed-URL query tokens are rejected at publish and at ingest; `pin.kind: none` on `https` yields $L_0$ content.
5. **Monotonic rev test:** a record whose `rev` is not greater than the stored one is rejected; a different signer cannot reuse an existing `id`.
6. **State machine test:** `withdrawn → active` is rejected; `active → superseded` with a valid `supersedes` is accepted.
7. **Skill requires test:** a `skill` record whose `requires` understates what its fetched `SKILL.md` asks for causes adoption to be refused with a diff.
8. **Endorsement binding test:** an endorsement of `id@2` does not apply to `id@3`; an endorsement whose recorded target hash doesn't match is ignored.
9. **Derivation test:** deleting `indexes/` and the local SQLite index and re-syncing reproduces identical tiers.
10. **Version test:** a record with `xenia: 2` is quarantined as `unsupported-schema` and does not break sync of other records.
11. **Confidence cap test:** a record claiming `confidence: verified` imports as `medium` until a local replay raises it.
12. **Store-binding test:** a valid record and sidecar copied into a different store (same org, different `store`) fails signature verification; the same pair in its original store passes; a roster epoch file copied across stores fails.
13. **Verify-before-parse test:** an oversized file, a file with a bad signature and a file from a non-member each fail without the YAML parser being invoked (instrumented parser); a file that passes steps 1–2 and then fails the schema is quarantined with the schema reason.
14. **Backdating test:** a record whose sidecar `signed_at` and `roster_epoch` predate a signer's removal is still judged against the current roster (spec 011 test 14).
15. **Tombstone test:** `withdraw` then `restore` tombstones change a record's derived status without changing its bytes; a steward outside the areas is rejected; a `sensitive` withdraw is restorable only by the author. Resolution: an author tombstone beats steward tombstones; two stewards who disagree make the status `contested` (hidden, named in `status`); a tombstone with a far-future ULID gains no priority; a second tombstone by the same signer for the same target is rejected unless it is a higher `rev` of the first.
16. **Roster chain layout test:** epoch files with a wrong `prev_hash`, a different `store`, or a gap in numbering are rejected; overwriting an existing epoch file in the store is detected by the verifier (hash of the file differs from what the next epoch's `prev_hash` pins).
17. **Pointer scheme allowlist test:** a pointer with a scheme outside `git`, `drive`, `https`, `xenia` is rejected at publish and at ingest.
18. **Rejected-keys test:** each of `tier`, `trust`, `trust_class`, `source_type`, `endorsed`, `verified_by_org` is rejected with a specific reason.
19. **Resolver guard test:** `https` pointers resolving to `127.0.0.1`, `10.0.0.5`, `169.254.169.254` or `::1`, a redirect to a private address, and a DNS name that rebinds between lookup and connect are each refused; an operator allowlist entry permits an internal host, a record cannot.
20. **Line-ending test:** a store cloned with `core.autocrlf=true` still verifies every record (via `.gitattributes`); a store missing `.gitattributes` is refused with a specific reason.
21. **Endorsement `target_hash` test:** an endorsement without `target_hash`, with a malformed one, or with one that does not match the target's recomputed hash is rejected or ignored respectively.
22. **Roster signature file test:** duplicate `kid`s count once; a signature over a different epoch or store fails; a grandfather list whose blake3 differs from `grandfathered_digest` is refused.

---

## 11. Open Decisions for the Operator
Council 008 Round 1 concurred with S-1 through S-5 (the Red-Team seat) and noted per-member claim quotas for S-5 (added in spec 011 §6.4). They remain **proposed** until the operator ratifies the spec.

| # | Question | Recommendation |
| :-: | :--- | :--- |
| S-1 | Markdown+YAML records with detached signatures (this spec) vs JSON? | Markdown+YAML; matches every other Delphi artifact and avoids canonicalization |
| S-2 | Should `progress` records exist as separate files, or fold into a rolling per-project record? | Separate and expiring first; a rolling digest is a librarian output later |
| S-3 | Is the 16 KiB / 500-char cap right? | Start strict; relaxing a cap later is safe, tightening is breaking |
| S-4 | Should skills embed small bundles in the record, or always point to a git tree? | Always a git tree: one code path, immutable hashes, reviewable diffs |
| S-5 | Are claims public to the whole store, or can a claimant reply privately? | Public in v1 (simple, auditable); private replies are a connector feature (Slack/email), outside xenia |
