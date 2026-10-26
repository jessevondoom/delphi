---
id: xenia-phase-1-plan
title: "Xenia Phase 1 plan: milestones, and what Xenia changes for the librarian and for spec 007"
author: strategist
timestamp: 2026-10-25T12:00:00Z
source_type: internal_generated
confidence: low                    # estimates are mine and nothing is built; no Xenia code exists yet
status: active
source_mission: 2026-10-23-foundational-design
tags: [xenia, plan, milestones, librarian, federation, step-6]
---

# Xenia Phase 1 plan

Scope: build Phase 1 of [`specs/011`](../../specs/011-xenia-trust-model.md) and [`specs/012`](../../specs/012-xenia-record-schema.md) (D-29): a roster-rooted, signed git store; kinds `memory`, `endorsement`, `tombstone`; display-only pointers; deterministic sync; the publish gate. Nothing in Phase 2 or 3 (skills, requests, progress, invites, vouch lists, pointer resolver, multi-store, the librarian).

Effort is focused build hours including tests, the unit `specs/005c` uses. They are my estimates before any code is written: read them as ±30%. The roadmap's old "60+ h, rough" for Xenia-adjacent work was a size guess; this plan replaces it.

## 0. Settled (operator, 2026-10-25)

1. The 011/012 banners were stale; the specs are approved as written and the banners now say so.
2. D-29 item 4 is lifted: building starts now, without waiting for the core. Amendments 007-A1 and 007-A2 are ratified and applied to `specs/007` (decisions.md, 2026-10-25).
3. The 013 notes in §5.1 (the `summary` field, the tag-alias rule, the third category) are still proposals for when 013 is amended.

## 1. What exists and what does not

| Needed by Xenia | State in code today |
| :--- | :--- |
| BLAKE3 | In `delphi-core`, `delphi-engine`, `delphi`. |
| SQLite, FTS5, `sqlite-vec`, local embeddings, hybrid fusion | Built (`kb.rs`, `embed/`, `vecext.rs`; specs 004-A1, 004-A2). |
| Six-tier lattice, `source_type` enum, provenance stamps | Built (`provenance.rs`, `kb.rs`, `frontmatter.rs`). `kb`'s `trusted()` already excludes `federated_peer` and `external_untrusted` and admits `federated_verified`, which is the Xenia rule. |
| Secret and path scan `[REV-SEC-05]`, redaction | `redact.rs`. |
| Approval broker with Red tier | Built (`approval.rs`). |
| git driven by the CLI (12 call sites) | Built (`worktree.rs` and others). No libgit2. |
| **Ed25519, `did:key`, install identity** | **Not in the repo.** `specs/007` §2.1 specifies it; no crate depends on an Ed25519 library. Xenia is the first thing that needs it. |
| Strict YAML (no anchors, aliases, merge keys, duplicate keys) | Not available. `serde_yaml_ng` accepts all four. See risk R1. |
| ULID, Unicode NFC | Not in the repo. |
| `delphi sync` (007 Plane 3) | Not built; gated by `delphi needs sync`. |

## 2. Shape

- A new crate **`delphi-xenia`**: pure functions, no network, no LLM, no clock reads except through an injected `Clock`. It holds the record and roster types, the verify-before-parse pipeline, the history walk over an abstract commit list, tier computation and tombstone resolution. Pure code is what the 31 + 23 spec tests and the fuzzers need.
- **`delphi-core`** (the daemon) owns what must not be reachable by an agent: the signing key, `state/` (pins, sticky state), the store clone under `$DELPHI_HOME/xenia/<org>/<store>/`, and the git network operations.
- **`delphi xenia ...`** is a new CLI family; the console gets the endorsement card and the quarantine read.
- A **test-store builder** (`xenia-testkit`, inside the crate's tests) builds signed stores, forks, reverts, takeovers and rewrites. Without it the spec's tests 12–15 and 27 cannot be written.
- **Humans sign, agents don't** is enforced structurally: the sign operation exists only behind a console approval of the exact bytes, and no tool in `tools.rs` can reach it.

## 3. Milestones

| # | Milestone | Specs | Effort | Needs from the operator |
| :-- | :--- | :--- | ---: | :--- |
| X0 ✅ | **Settle status.** Answer §0; fix the 011/012 banners; make 007's A1 row "accepted" rather than "proposed"; refresh `ratified.lock` (`delphi update`). | 011 header, 007 §3 | 2 h | The two answers in §0 |
| X1 ✅ | **Primitives.** Install identity (Ed25519 key in the keychain or a `0600` file under `$DELPHI_HOME`, `did:key` encode and decode, 64-hex fingerprint), `whoami`; strict verification (`verify_strict`) with the RFC 8032 and known-bad vectors (non-canonical S, small-order keys); domain-string builders; ULID; sidecar parser (strict JSON, fixed fields, 1 KiB). Key is separate from any future peer-transport key (see §5.2). | 011 §3.4, 012 §3 | 10 h | nothing |
| X2 | **Record validation.** Envelope and the three kinds as types; the validation order of 012 §6 with an instrumented parser proving YAML never runs before steps 1–3; strict frontmatter subset (R1); NFC, bidi and tag-character rejection; limits; pointer allowlist; rejected trust keys. | 012 §2, §4–§6; tests 1–5, 10, 12, 18, 20 | 14 h | nothing |
| X3 | **Roster chain and joining.** Epoch files, `prev_hash`, genesis, per-person quorum, `person` immutability, never-dropped entries, pinned epoch hashes, `join` with the admin fingerprint and head pair, `pins.toml`. | 011 §3.2–§3.3; 011 tests 3–6, 26, 29, 31; 012 tests 13–14 | 14 h | nothing |
| X4a ✅ | **Store and sticky state.** Clone outside every workspace zone with hooks, filters, LFS, submodules and fsmonitor disabled; `.gitattributes` check; fast-forward-only fetch; sticky state per path (highest `rev` and hash); idempotent re-sync. | 011 §3.3.5, §3.5; tests 24, 27, 28; 012 test 16 | 12 h | nothing |
| X4b | **Freshness and the history walk.** Freshness statement validation with the verifier-side cap; stale means no endorsement counts; the walk over commits (no deletion, no lower `rev`, signer continuity, takeover); rewrite recovery with an admin quorum and the operator-approved path list. | 011 §4, §8.4; tests 12–15, 27; 012 tests 6, 15, 21 | 14 h | nothing |
| X5 | **Standing, tiers, endorsements, tombstones.** Standing at the current epoch; the role matrix and area matching (union of tags for tombstones); self-endorsement ban by `person` across the `successor` chain; `target_hash` binding; duplicates; tombstone resolution including `contested`; tier as a pure function of (records, roster, freshness). | 011 §2, §3.4, §5, §7.3–§7.4; tests 2, 7–11, 16–19, 30 | 14 h | nothing |
| X6 | **Index and retrieval.** Records land in `state.db` as a new zone with the local frontmatter of 012 §8, `source_type` from the computed tier, hidden statuses mapped to `superseded` with `superseded_by`. Vectors keyed by content hash so a tier change flips the `trusted` bit without re-embedding (§5.1). Agents see only L2 summaries in the spotlighted `<xenia_record>` envelope with the span ceiling lowered; L1 and L0 only through an explicit quarantine read. `provenance.stamp` events. The 007 §3.2 sanitizer on ingress. | 011 §5.3, §7.2; 012 §8; tests 21–22; 012 tests 17, 19, 23 | 12 h | nothing |
| X7 | **Publish and the member commands.** Publish gate (promoted scope only, `[REV-SEC-05]` scan, flags, exact-bytes preview, Red approval, no model turn between preview and sign), fast-forward rebase with re-verify, never force-push; `endorse`, `dispute`, `withdraw`, `restore`, `republish --resign`. A "similar records already in the store" list in the preview, from the hybrid search (§5.1). | 011 §7.1, §8.1; tests 20, 23 | 16 h | A store to publish to (a private git repo) |
| X8 | **Admin operations.** `admin add/remove/rotate/propose/sign/publish/refresh`; the `roster-proposal/<n>` branch flow; `rotate` requires `--routine` or `--compromised`, the latter forces `voids_tombstones`; `refresh` runs the history walk first and refuses to sign on failure. | 011 §3.2, §4.1, §8 | 12 h | Three admin identities (2-of-3) for the real run |
| X9 | **Surfaces.** `delphi xenia status` (quarantine, `contested`, freshness age, tampering), `delphi doctor` checks (fewer than 3 admins, statement past 50% of its life), the steward's attestation card in the console, `delphi needs` item `xenia`, events for the Narrator. | 011 §4.1, §7.3 | 8 h | nothing |
| X10 | **Verification.** Every numbered test of 011 §10 and 012 §10 mapped to a named test; a 3-admin 2-of-3 chain test including a fork and an offline install across epochs; fuzzers on the sidecar and frontmatter parsers; the live recovery drill of §8.4 on a scratch store; then an independent red-team sweep per `REVIEW.md`. | 011 §1.4 gates 1–7 | 18 h | Reads the sweep result |
| X11 | **Dogfood gate.** At least 3 humans use it for 2 weeks; record endorsement latency and the share of records read at L2. Thresholds are agreed again beforehand (O-7: median endorsement ≤ 3 working days, ≥ 70% at L2). | 011 §1.4 gate 8 | 4 h of ours, 2 weeks elapsed | Three humans, a store, two stewards |

**Total about 150 h** (range roughly 110–200), about twice the model pool. Critical path: X1 → X2 → X3 → X4a → X4b → X5, then X6 and X7 in parallel, then X8, X9, X10, X11. The first thing that works end to end is after X6: an install can join a hand-built test store, sync, and search endorsed records. The first thing a second human can use is after X8.

### Built so far

- **X0 (2026-10-25):** banners, D-29 item 4 lifted, 007-A1 and 007-A2 applied. `ratified.lock` hashes are not refreshed; `delphi update` in the operator's home does that.
- **X1 (2026-10-25):** crate `delphi-xenia` (34 tests) and `delphi xenia whoami`. Choices the spec left open, recorded here:
  - Every hash anywhere (sidecar `hash`, roster `prev_hash`, the hash inside the roster and freshness signing domains, `target_hash`) is spelled `blake3:` plus 64 lowercase hex. Spec 012 §3 wrote "blake3 of `<n>.json`" for the roster domain without a spelling.
  - A signature can only be made over a `Domain`, which only the three validated constructors can build. The install key cannot be asked to sign free text, and a newline in `org`, `store` or `id` cannot shift the signed fields.
  - Key custody is a `0600` file in a `0700` directory under `$DELPHI_HOME/xenia/`, refused if group- or world-readable, a symlink, or damaged, and never overwritten. An OS-keychain backend is later (R4).
  - The strict-verification tests include a check that a lenient verifier accepts the identity-key forgery, so the test proves it discriminates.
  - Still to do in X2: the record envelope, the strict frontmatter parser, and the schema limits. `verify_record` already enforces spec 012 §6 steps 1–3 on raw bytes, so YAML is never reached for a bad record.

- **X4a (2026-10-26):** pure rules in `delphi-xenia` (`store.rs`, `sticky.rs`), git and disk in `delphi-core` (`xenia_store.rs`), and a reusable git test remote (`xenia_testkit.rs`, behind `test-support`). Choices the spec left open, and deviations:
  - **Layout.** Spec 011 §3.3.5 puts the clone at `$DELPHI_HOME/xenia/<org>/<store>/` and §3.5 puts `state/` at `.../<store>/state/`: those collide (state inside the attacker-filled tree). Resolved as siblings: `<store>/clone/` (bare repository), `<store>/state/` (`sticky.json`, and X3's `pins.toml`), all `0700`. Spec text should say so.
  - **Bare clone, nothing checked out.** Files are read by object id (`git cat-file`), so hooks, filters, `.gitattributes` conversions, symlink creation and submodule handling have nothing to act on; `.gitattributes` is still checked at every sync as the spec requires (012 test 16: bytes read back exactly even with `core.autocrlf=true` in the clone's config).
  - **One git command builder** (`git_command`): cleared environment, empty `HOME`, `GIT_CONFIG_GLOBAL=/dev/null`, `core.hooksPath` to an empty directory, fsmonitor/symlinks/autocrlf/credential helpers/LFS filters off, `protocol.allow=never` plus the single transport of the store URL (also `GIT_ALLOW_PROTOCOL`), `GIT_CEILING_DIRECTORIES` so git never discovers a repository from the working directory, `fetch`-time `--no-recurse-submodules`. The remote URL is passed on the command line, never stored in the clone's config (no credential or URL at rest in the clone).
  - **Accepted URLs:** `https://`, `ssh://` (no embedded `user:password`), and local paths or `file://` only when the caller opts in. `http://`, `git://`, `ext::` and all other helpers are refused. The ssh transport gets the operator's `HOME` and `SSH_AUTH_SOCK` and `-o BatchMode=yes -o StrictHostKeyChecking=yes -o ProxyCommand=none`; it is built but not exercised against a real ssh server.
  - **Caps (the spec gives none for git objects):** 512 KiB per object, 64 MiB per fetch (the fetch is killed when the clone grows past it), 100,000 new objects, 20,000 new commits, 20,000 tree entries, 255-byte paths, depth 6. Measured on every new object reachable from the fetched head and not from the accepted one, so a huge blob hidden in history and deleted at the tip is refused too. A refused fetch removes its objects (`gc --prune=now`).
  - **Tip tree rules (refused outright, whole sync):** symlinks, gitlinks/submodules, executable files, any `.git*` name other than the root `.gitattributes` (case-insensitive, Windows trailing dots, `git~1`), `..`, backslash, colon, control characters, non-UTF-8, oversize file. Refusing instead of ignoring is the fail-closed reading; a store host that wants to be synced does not ship these.
  - **`.gitattributes`:** must contain a `* -text` line and may otherwise only add `-text`, `binary`, `-diff`, `-merge` or `linguist-*`. `filter=`, `text`, `eol`, `crlf`, `ident`, `working-tree-encoding`, `export-subst`, `diff=`, `merge=`, `[attr]` macros and CR bytes are refused. Only the root file is allowed.
  - **Fast-forward only:** the fetch lands in `refs/xenia/fetched`; the accepted head lives in `state/sticky.json` (install-owned state stays out of the clone; `refs/xenia/accepted` only anchors history against gc). A head that is not a descendant is a typed `NonFastForward { accepted, fetched, kind }` with `kind` `Diverged` (rewrite) or `Behind` (the host went back in time); nothing is applied and the staging ref is left for X4b's recovery path to inspect.
  - **Sticky state:** per path `Rev(n)`, `Immutable` (roster epoch files: any other hash is equivocation) or `Present` (sidecars, `*.sigs.json`, `freshness.*`: must not disappear, content may change; their validity is X3/X4b). Spec 011 §3.5 says "highest `rev` per path" but only records have a `rev`, so this is X4a's reading. The update rule is pure (`sticky.rs`): new path, higher rev supersede; lower rev, same rev with another hash, deletion, or a stored path whose file no longer verifies are refusals.
  - **A sync with any sticky refusal applies nothing** (no state change, accepted head unchanged, report lists the paths). The spec says the stored version of a tampered path keeps its effect (§3.5) and that a failed check 3 rejects the whole sync (§4.3); applying the clean paths while refusing the others would let a host smuggle new records past a withheld deletion. Cost: a store stays stuck on its last accepted head until the recovery path (X4b) is used. The state file is only written, atomically (`0600`, fsync, rename), when something changed; a second sync of an unchanged host reads and writes nothing.
  - **Classifier hook.** `sync` takes a classifier `(path, bytes) -> Tracking | quarantine reason`. X2/X5 pass the verifier (signature first, then `rev`); a file that fails it is quarantined and can never raise a stored `rev` and lock the real record out. X4a ships `classify_by_layout` with a caller-supplied `rev` reader for tests.
  - **Damage is an error:** a missing sticky file is a fresh install; a damaged, wrong-store, group/world-readable or symlinked one stops the sync (it is never treated as empty, which would let an attacker reset it).
  - **Known weaknesses:** (1) a delta-compressed pack object that inflates huge is measured only after `index-pack` resolved it (git streams large objects to disk, and the fetch byte cap and timeout bound the damage); (2) the history walk, signature checks, freshness and rewrite recovery are X4b, not here: X4a alone only guarantees the clone and fetch are inert, the head only moves forward, and no path regresses; (3) `PATH` is inherited to find `git`; (4) syncs of one store are serialised with an advisory lock, but nothing stops two different `$DELPHI_HOME`s sharing a clone directory.
  - Test 28 covers the daemon-owned half (state under `xenia/<org>/<store>/`, unchanged by edits under `team/`); `delphi sync` does not exist yet, so the other half of the test waits for it.

## 4. Risks that decide the estimate

- **R1 Strict YAML.** The spec requires rejecting anchors, aliases, tags, merge keys and duplicate keys. `serde_yaml_ng` does not. Recommended: do not use a general YAML parser at all; define the frontmatter as a small subset (scalars, flow lists, nested maps for pointers and `applies_to`) and write a ~300-line parser fuzzed from day one. This is within X2's 14 h, but if the subset proves too small for `pointers`, add 4 h.
- **R2 Git as an attack surface.** A hostile store can carry hooks, filters, submodules, symlinks and huge objects. X4a pins git's config for every call (`core.hooksPath` to an empty dir, `protocol.allow` list, `core.symlinks=false`, no `--recurse-submodules`) and caps object size. This is the part most likely to grow.
- **R3 History-walk cost.** The walk runs from the signed `store_head`; a store with thousands of commits makes sync slow. Phase 1 stores are small. Cache the last accepted head and walk only the delta; measure in X10.
- **R4 Key custody.** The key is the org's trust anchor for one human. Keychain on macOS is straightforward; on Linux a `0600` file is weaker. The roster allows rotation, so a lost key is recoverable, a stolen one is `rotate --compromised`.
- **R5 People, not code.** Council 010's biggest remaining risk is steward endorsement latency and admin refresh cadence. No test measures that; X11 does.

## 5. What Xenia changes elsewhere

### 5.1 The librarian (spec 013)

Vectors and hybrid search are built (004-A1, 004-A2), so 013 §7 steps 1–3 are done and the librarian no longer waits on retrieval quality. What Xenia changes:

1. **Build trigger 4 will fire.** 013 §6 lists "the Xenia shared-memory work lands and the corpus is no longer one person's." The other three are not met: `context/`, `memory/` and `skills/` hold 78 Markdown files against the ~150 guess, `context/index.md` is still hand-written, and no span-turn measurement exists. So the librarian's trigger is Xenia's X11, not corpus size. (The operator waived §6 on 2026-10-24, so this is information, not a gate.) Decision the operator may want: build the librarian after X11, when there is a corpus that is not one person's; 011 already puts it in Xenia Phase 3.
2. **The index gains a zone.** Xenia records enter `state.db` as a derived zone (X6). Tier is a pure function recomputed on every sync, so vectors must be keyed by content hash and tier carried in `source_type` / the `trusted` column, never baked into the vector row. Otherwise every roster change re-embeds the store. `kb.rs` already separates the two (`trusted` and `scope` skip rows; the embedding does not depend on them).
3. **Two tag systems must not merge.** 013 §2 has the librarian merge near-duplicate tags and write `context/taxonomy.md`. In Xenia, `tags` are not labels: they decide which steward may endorse or tombstone a record (011 §2.3). A librarian that "normalizes" tags on org records would change authority, and it cannot anyway, because records are signed. Rule: the local taxonomy may hold **aliases used at search time** for Xenia tags; it never rewrites a signed record; a change to the vocabulary of `steward_areas` is a roster epoch, signed by admins.
4. **The `summary` field disagrees.** 013 §5 proposes an optional local `summary` of at most 160 characters; 012 has a required `summary` of at most 500. Decision for the operator when 013 §5 is amended: keep the local field at 160 and map a Xenia record's local `summary` to a deterministic truncation, with the full 500 in the body of the cache entry. (013 §5 item 4 already asked for this coordination.)
5. **A third category beside scout and librarian.** 013 §3 splits external (scout) from internal (librarian). An endorsed org record is neither: it is internal to the org and external to the install, at most L2. Answers that mention it must show the tier and the store. This is a wording fix in 013 §3 when the role is built.
6. **A cheap win now, no role needed.** The publish preview (X7) lists the existing records most similar to the draft using the hybrid search. This is deterministic, local, and is the first piece of the librarian's dedupe duty done by plain code.
7. **Taint.** A librarian span that reads L2 Xenia summaries has its ceiling lowered to L2, which `specs/001` §5.2 allows to write; one that quarantine-reads L1 does not. The librarian should read pointer indexes (path, heading, summary), not bodies, which keeps its ceiling where it was.

### 5.2 Federation, spec 007

1. **007-A1 is already in the 007 table as "proposed".** X0 changes it to accepted. L2 now has two routes: the original (pinned teammate DID plus commit and CI attestation) and a steward endorsement of a Xenia `memory` record. The first has no implementation and no verifier; the second does, and is the only L2 that will exist for a while.
2. **Xenia is the first implementation of 007 §2.1's identity.** The DID, fingerprint, strict verification and key storage built in X1 are the code Plane 1's Agent Card and Plane 2's `DelphiCap` would reuse, so the 60+ h estimate for Planes 1–2 shrinks by the identity work (my guess: 8–10 h). **Recommendation: separate keys by purpose.** Xenia's key signs content a human approved; a peer-transport key authenticates a daemon. One key doing both lets a transport-signing bug become a content-signing bug. Domain separation (`xenia/1`) already protects the bytes; separate keys protect the custody. This is a decision for the operator and an amendment to 007 §2.1 when Plane 1 is specified for building.
3. **The roster could become Plane 1's peer allowlist.** 007 §2.1 asks the operator to paste standing grants per peer DID. With a roster, "who is a peer" is a signed, revocable, per-person fact. This is a possible simplification, not a plan: Plane 1 peers need not be Xenia members. Park it until Plane 1 is scheduled.
4. **Plane 3 (`delphi sync`) overlaps and is weaker.** 007 §4 item 4 merges `context/domains/**` and `specs/**` with ordinary Git semantics and stamps them `federated_verified`. Nothing verifies a signature there, so a teammate's unsigned file gets the same tier that Xenia only gives to a signed, endorsed, fresh record. Once Xenia exists that is the inconsistent door. **Amendment 007-A2 (ratified and applied 2026-10-25):** Plane 3 keeps only the rule-file proposals of §4 items 1–3, which all end in operator ratification anyway; non-prompt shared knowledge moves to Xenia; files that Plane 3 still stamps from plain git are stamped `federated_peer` (L1), not `federated_verified`. This shrinks `delphi sync` (the ~30 h rough estimate; my guess is about half) and keeps one definition of L2.
5. **Plane 2 stays out of Xenia.** 012 §4 rejects `delphi://` pointers, and 011 §6.2 forbids treating "our own" locators as trusted. Plane 2 must not become the Xenia pointer resolver of 011 §11.2; that resolver needs its own SSRF-guarded fetcher with a pointer content tier of L0.
6. **The case for Planes 1–2 gets narrower.** Sharing knowledge, the main driver for federation, is now met by Xenia without live transport (that is O-5: Xenia first). What Planes 1–2 would still add is live delegation of tasks and ad hoc line-scoped slices. `delphi needs federation` ("a second operator or machine") should stay the gate; its evidence will be clearer after X11.
7. **Taint ceiling interplay.** 007 §3.1 clamps a payload to `min(payload, PeerTrustCeiling)`. Xenia's 011 §5.1 clamps to `min(class tier, source_ceiling)` and caps at L2. If Plane 1 ever carries a Xenia record, the lower of the two applies; nothing in either spec lets a peer raise a Xenia tier.

## 6. Order against the rest of the roadmap

Xenia (X1–X11) and the Step 6 items are independent. The red-team sweep findings (`oauth2.rs`, `credential.rs`, `harness.rs`, `finalize.rs`, `setup.rs`) should be cleared before X1, because X1 puts key custody next to `credential.rs`. 14 h, no operator gate.
