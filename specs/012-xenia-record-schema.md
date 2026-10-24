---
id: spec-012-xenia-record-schema
title: "012: Xenia Record Schema: Envelope, Kinds, Pointers & Store Layout"
author: architect
timestamp: 2026-10-24T00:00:00Z
status: draft   # post-v1 design; not yet reviewed by Strategist or Red-Team
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
org: acme                                 # store's org id (must match org.toml)
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
confidence: low | medium | high | verified   # `verified` requires an `evidence` pointer
status: active | superseded | withdrawn
supersedes: string | null                 # id@rev this replaces, if any
review_by: "2027-01-24"                   # date after which the record is considered stale (required for memory, progress)
expires: "2026-11-24"                     # OPTIONAL hard expiry (progress, request)
pointers: [Pointer]                       # <= 8 (§4); at least one for `memory` unless the body is self-contained
evidence: [Pointer]                       # OPTIONAL, <= 4; checkable support for the claim (CI run, pinned commit, test output)
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
- `status`, `rev`, `supersedes` form a small state machine: a new `rev` may only move `active → superseded | withdrawn`, or stay `active`. Nothing leaves `withdrawn`.
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
- `hash` = BLAKE3 of the **raw bytes** of the `.md` file. `sig` = Ed25519 over the UTF-8 string `xenia/1\n<hash>\n<roster_epoch>\n<signed_at>`. The domain prefix stops a signature from being replayed in another context.
- The verifier recomputes the hash from the bytes it has. If the bytes differ, the record is invalid. Line endings are normalized to LF **by the publisher before hashing**, and the verifier never rewrites bytes.
- `roster_epoch` is the epoch of the roster the signer saw. A record is valid if the signer is in the roster at that epoch **and** was not removed in any later epoch before `signed_at` (spec 011 §3.3.2).
- `endorsement`s and `tombstone`s are records like any other, with their own signatures (§5.6).

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
| `https` | URL without userinfo or query tokens | `content-hash`, `none` | Plain fetch; content always $L_0$ if `pin.kind: none` |
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
verified_by: none | review | test | ci   # how the author checked it (informational; evidence pointer is what's checkable)
```
- `review_by` is required (default 90 days from `updated`). Past it, the record shows as **stale** and ranks lower; it isn't deleted.
- `decision` records should link the discussion in `pointers` and name any superseded decision in `supersedes`.

### 5.2 `skill`: a shareable, adoptable skill
```yaml
skill_name: string                        # [a-z0-9-]{1,48}; the directory name once adopted
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
- The skill's code and prompts live in the pinned git tree, **not in the record**. The record is the signed index entry; the tree hash is what adoption verifies.
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
- Multiple claims on one request are normal. The requester picks, outside the store, or closes the request.
- `delivered` claims usually point to a `memory` record or other pointer holding the result.

### 5.6 Meta-kinds: `endorsement` and `tombstone`
**`endorsement`** (stewards and, for `dispute`, any member):
```yaml
target: "<id>@<rev>"                      # an exact revision; a new rev of the target needs a new endorsement
verdict: endorse | dispute | withdraw
scope: [string]                           # topic areas the endorsement covers (must be within the endorser's steward areas for `endorse`)
note: string                              # <= 300 chars; required for `dispute`
checked: [target-open, evidence-open, summary-matches]   # what the steward attests they did
```
- An endorsement binds to an exact `id@rev` **and** the target's `hash`, so a changed revision is not covered.
- `endorse` is valid only from a steward whose areas include `scope`, and the endorser must not be the target's author (spec 011 §4.1, test 6).
- `dispute` flags the target to stewards and shows a warning on it; it does not change the tier by itself.

**`tombstone`** (author or steward):
```yaml
target: "<id>"                            # whole record, not a revision
reason: superseded | mistake | sensitive | departed | other
note: string                              # <= 200 chars
```
- `sensitive` should trigger the rotate-secrets reminder (spec 011 §7.1).

---

## 6. Limits & Validation
Checked by `delphi-core` before any deep parse, in this order, failing closed into quarantine:

| Check | Limit |
| :--- | :--- |
| Total record file size | $\le$ 16 KiB; body $\le$ 4 KiB |
| Frontmatter | strict schema per kind; unknown or duplicate keys rejected; YAML anchors, aliases, tags (`!!`) and merge keys rejected |
| Strings | NFC-normalized; no control characters except `\n` in the body; no bidi controls or `U+E0000`–`U+E007F`; plain text fields contain no Markdown links or HTML |
| `summary` | $\le$ 500 chars, single paragraph |
| Counts | pointers $\le$ 8, evidence $\le$ 4, tags $\le$ 12, related $\le$ 8 |
| IDs | valid ULID; `kind` directory matches `kind` field; `org` matches `org.toml` |
| Pointers | scheme allowlist, no credentials in locators, pin kinds valid for the scheme |
| Dates | valid ISO-8601 UTC; `updated` $\ge$ `created`; `review_by`/`expires` in the future at publish |
| Signature | spec 011 §3.3, performed **before** any model sees the content |

Size limits matter for the threat model (spec 011 T15) and for context hygiene: a bounded summary is what enters an agent's context.

---

## 7. Store Layout
```
xenia-<org>/                      # a git repository (the "store")
├── org.toml                      # org id, schema versions accepted, min client version, store audience name
├── roster/
│   ├── members.json              # { epoch, members:[{did, handle, roles, steward_areas, added_by, added_at, removed_at?, compromised_since?}] }
│   └── members.sigs.json         # detached admin signatures over members.json bytes
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

---

## 11. Open Decisions for the Operator
| # | Question | Recommendation |
| :-: | :--- | :--- |
| S-1 | Markdown+YAML records with detached signatures (this spec) vs JSON? | Markdown+YAML; matches every other Delphi artifact and avoids canonicalization |
| S-2 | Should `progress` records exist as separate files, or fold into a rolling per-project record? | Separate and expiring first; a rolling digest is a librarian output later |
| S-3 | Is the 16 KiB / 500-char cap right? | Start strict; relaxing a cap later is safe, tightening is breaking |
| S-4 | Should skills embed small bundles in the record, or always point to a git tree? | Always a git tree: one code path, immutable hashes, reviewable diffs |
| S-5 | Are claims public to the whole store, or can a claimant reply privately? | Public in v1 (simple, auditable); private replies are a connector feature (Slack/email), outside xenia |
