---
id: spec-027-bundle-authoring-distribution-and-evaluation
title: "027: Bundle Authoring, Distribution Transports, the Vector Pipeline & Evaluation"
author: architect
timestamp: 2026-10-25T23:00:00Z
status: approved   # by the operator, 2026-10-25: decisions OD-1..OD-9 and the recommendations of the set (relayed by the parent session); the text as written is approved when this pull request merges
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the transports and the vector pipeline reuse built seams (kb.rs, embed/, DLPHVEC1); the authoring workflow and the evaluation pilot are untested proposals
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, distribution, vectors, evaluation, authoring, post-v1]
---

# Spec 027: Bundle Authoring, Distribution Transports, the Vector Pipeline & Evaluation

> **Status: approved by the operator 2026-10-25 (decisions), text approved on merge.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Depends on `specs/023` (format), `specs/024` (adding) and, for the store transport, on Xenia Phase 1 (`specs/011`, `specs/012`; X1 built, X2–X11 in build). Nothing here is built; §9 is the build plan.

## 1. Scope & Objectives
1. **Authoring** (§2): scaffold, write, distill (agent-assisted, OD-6), sanitize with a redaction report, validate, package, sign; the `org-bundle-authoring` skill (OD-9).
2. **Packing** (§3): the `.xbundle` archive, including air-gapped use.
3. **Transports** (§4): directory, archive, the xenia `bundle` record, static mirror, marketplace, peer sync; the order.
4. **The vector pipeline** (§5): shipping, verifying and using precomputed vectors; local embedding; incremental update (OD-8).
5. **Privacy when sharing** (§6).
6. **Evaluation** (§7): three tiers with a pilot and a hash-pinned pre-registration (OD-7).
7. **Acceptance criteria, verification, build plan** (§8–§10).

### 1.1 Non-Goals
- A hosted registry or service; every transport is files or git.
- Live peer transport (`specs/007` Planes 1–2, deferred).
- Automatic publishing to a store: publishing is outbound and keeps the publish gate (`specs/011` §7.1).

### 1.2 Simpler Alternative Considered
- **Alternative: the bundle is a git repository; distribution is `git clone`; vectors are always computed locally; evaluation is whether people keep using it.** Most of this is right for a single team, and the store transport is a git repository. Rejected as the whole answer because (a) a clone has no identity or continuity (`specs/024` §2.3); (b) an air-gapped machine and a Claude Code user need one file or a marketplace entry; (c) a 50,000-chunk corpus takes hours to embed on a laptop CPU (§5.6) and the organization has already paid that cost once; (d) "people keep using it" cannot tell a bundle that helps from one that is present, and the operator's pre-registration discipline (D-30) should apply here too.

---

## 2. Authoring Workflow
### 2.1 `delphi bundle init <dir> --org <org> --name <name>`
Scaffolds the layout of `specs/023` §2 with a filled `bundle.toml`, a `README.md` skeleton (what the operator agrees to by adding), `CHANGELOG.md`, empty `knowledge/index.md`, `behavior/goals.md` and `perspectives.md` with one commented example each, `evals/delphi/kb-eval.json` with the two required controls. Never inside a registered project repository (D-03).

### 2.2 Writing by hand
Most of a good bundle is written by people. `delphi bundle validate` runs on save in the console's deck or from the shell, so an author sees caps and warnings while writing.

### 2.3 `delphi bundle distill` (agent-assisted; proposals only; OD-6)
The organization already has knowledge in Delphi. `distill` asks the **writer** role (GOVERNANCE §2: truth before harmony) to propose bundle files from it:
- **Inputs it may read (allow-list, enforced by the path guard for the span):** `team/context/**`, `team/specs/**`, ratified `[T-…]` rules, mission `decisions.md` and `councils/*-verdict.md`, endorsed ($L_2$) xenia records, the previous bundle version. **Never:** operator scope, `memory/agents/**`, `events.jsonl`, raw transcripts, connector payloads, worktrees, an `isolated` workspace (`specs/004` §5.4, §5.7).
- **Outputs:** proposed files under `<bundle>/.distill/<date>/`, each with provenance frontmatter stamped by the daemon at the span's tier (normally $L_3$; a span that read an $L_0$ source stamps $L_0$ and `validate` refuses such a file for `behavior/`), a `summary`, `pointers` to the sources, and a diff against any existing file.
- **It proposes; the human moves files into the bundle.** Nothing in `distill` writes under `knowledge/` or `behavior/`. Nothing is packaged or added until the explicit `package` and `add` steps, and the privacy and secret scan (§6) is mandatory before `package`.
- **Dedupe:** before proposing a knowledge file, the hybrid search over the current bundle shows the nearest existing files (the deterministic step the xenia publish preview also uses).
- **It never invents goals or perspectives.** It may quote one a ratified document states, with the pointer.

### 2.4 `delphi bundle validate` with `--report`
Runs `specs/023` §10 and, with `--report <path>`, writes the **redaction report** from `skills/org-bundle-authoring/templates/redaction-report.md`: every scan hit with file, line, class and the action taken. The human approves this report before `package` (§2.6). This is authoring-side review of content going out, distinct from the removed adoption-side ratification (OD-1).

### 2.5 `delphi bundle sign <dir>` and `package`
`package` writes `.delphi-bundle/files.json`, runs the advisory retrieval gate (§7.1) into `.delphi-bundle/eval.json`, optionally builds `vectors/` (`--vectors`, §5.1), and writes the `.xbundle` (§3). `sign` writes `sig.json` with the install's xenia identity after showing the file list and hashes (a console action with a TTY challenge like ratification; no `--yes`); `publisher.did` must equal the key's DID. `validate --against <previous>` names the smallest honest semver bump; `sign` and `publish` refuse a lower one.

### 2.6 The `org-bundle-authoring` skill (OD-9)
Drafted in this PR at `skills/org-bundle-authoring/SKILL.md` with `templates/redaction-report.md`, in the Agent Skills format with Delphi's fields, authorized for the `writer` role (`agents/writer/AGENT.md` gains it in `authorized_skills`, `specs/022` §5). It walks an agent through: open or scaffold; `distill` from allow-listed sources; write what only a person knows (README, changelog, guidance, policy); keep instructions out of knowledge; `validate --report` and work every finding; evals; optional vectors; **stop for the human's approval of the redaction report**; then `package` and, by the human, `sign`. Its safety rules: never credentials, PII beyond a work handle, customer data, transcripts or local paths; never write into a project repository; never package, sign or publish without the approved report; carry the lowest tier forward. In Claude Code the same skill runs without the `delphi` commands: the agent does the gathering, distilling and report by hand and asks the human to run `validate`, `package` and `sign`; the bundle directory is already a valid plugin. The `delphi bundle` commands it names do not exist yet (slices B1, B6, B10).

---

## 3. The Archive: `<name>-<version>.xbundle`
- **Format:** a POSIX ustar tar of the bundle tree (including `.delphi-bundle/*`, `.claude-plugin/plugin.json` and `vectors/`), **uncompressed** (the text is small, vectors are incompressible, and a decompressor would hide the size check: zip bombs are a documented threat in `specs/014` §6), entries sorted bytewise, regular files only, mode `0644`, `mtime` fixed to `files.json`'s write time, no uid/gid names, no extended headers. Two archives of the same tree are byte-identical.
- **Caps on import:** the limits of `specs/023` §2 and §8 checked **while reading**; a violating entry aborts extraction. Extraction goes to `$DELPHI_HOME/bundles/.incoming/<random>/`, is validated in full, then moved into the cache under its hash (`specs/024` §4.1); `.incoming/` is removed on any failure.
- **Identity:** the bundle hash inside, not the archive's own hash.
- **Air-gapped flow:** `package` on a connected machine; carry the file; `add <file>` on the isolated machine with no network. Shipped vectors make the bundle searchable by meaning at once if the embedder matches (§5). The provenance level is `signed`/`known-publisher`/`unsigned` (`specs/024` §2.2); endorsement does not travel in an archive.

---

## 4. Transports
### 4.1 Directory (slice B2; first)
`delphi bundle add <dir>`. No network; unsigned allowed. Every other transport ends by handing a validated directory to the same code.

### 4.2 Archive (slice B6)
§3. Signing and import hardening; still no infrastructure.

### 4.3 The xenia store: kind `bundle` (slice B8; Xenia Phase 2)
Proposed to replace the provisional `skill` kind (`specs/012` §11; B-4): a skill is a bundle with one `skills/` entry. Record fields under the common envelope:
```yaml
kind: bundle
bundle_name: acme-platform
bundle_version: "1.4.0"
bundle_hash: "blake3:…"                    # specs/023 §3.4: the identity across transports
tree:                                      # a git pointer pinned by tree, or an archive pointer (https, content-hash)
  scheme: git
  locator: "https://git.acme.example/platform/bundle.git#."
  pin: { kind: git-tree, value: "<tree sha>" }
touches: { roles: [strategist, architect, builder, red-team], connectors: [slack] }
compat: { delphi: ">=0.3.0", bundle_schema: 1 }
```
- Signed by the publisher (a roster member); the bundle's own `sig.json` must be by the same person (same DID or successor chain), checked at add.
- **Endorsement is provenance, not a gate** (OD-1, OD-3): a steward's endorsement of `bundle_hash` gives the level `endorsed`; without it the level is `member-signed`. Knowledge indexes at $L_2$ either way (`specs/024` §3).
- **Fetching the tree** is the one place a pointer is followed: `delphi xenia add <id>` clones the `tree` locator with the store hardening (R2), checks out the pinned sha, verifies file hashes against `files.json` and `blake3(files.json) == bundle_hash`, then runs the ordinary `add`. A mismatch is tampering, reported.
- **Updates** are new `rev`s of the same record, covered by `specs/011` §3.4 ownership and the history walk; `delphi xenia sync` lists them; `delphi bundle update` applies under the continuity rule.

### 4.4 Static HTTP mirror (later)
`index.json` listing `{name, version, bundle_hash, sha256 of the .xbundle, url}` plus the files on any static host; read as a locator for archives; confers no trust. Not scheduled.

### 4.5 Claude Code marketplace (slice B7)
`specs/025` §7.3.

### 4.6 Peer sync (deferred)
A bundle would cross `specs/007` Planes 1–2 as its `bundle` record plus archive bytes; the gateway verifies both as above; no bundle-specific protocol.

### 4.7 Order (B-5, recommended)
**Directory → archive → store.** Directory needs nothing and unblocks the strategist work; archive adds provenance and air gap; the store adds organizational standing and depends on xenia X7/X8, not yet built.

---

## 5. The Vector Pipeline (OD-8)
Built on `specs/004` §3.5–3.7: chunks are embedded in windows, vectors live in `kb_vec` keyed by chunk rowid, `kb_chunk_vec` records which chunks have vectors, the preloaded-vector cache `DLPHVEC1` maps *the hash of exactly the text given to the model for a chunk* to its vectors, `apply_seed_vectors` fills what the cache covers before any model call, `embed_pending` embeds the rest in the background with no write lock held while the model runs, and `kb_meta` records `embedding_model_id`, `embedding_dim`, `embedding_format`, `chunker_version`. Everything below reuses those pieces.

### 5.1 Shipping (publisher side)
`delphi bundle package --vectors` indexes the bundle's `knowledge/` and skill `references/` into a scratch index with the publisher's installed embedder, embeds every chunk, and writes `vectors/<key>.dlphvec` with `kb seed-export` semantics plus `vectors/index.json` (`specs/023` §8). Recommendation and default (B-16): ship when the publisher's embedder is Delphi's default model (`bge-base-en-v1.5` int8 package); `package --vectors` warns when it is not, because few adopters will match. `i8` quantization is recommended above 5,000 chunks once the reader supports it.

### 5.2 Key match (adopter side)
At `add`, for each cache in `vectors/index.json`: use it **only if** `model_id`, `dim`, `embedding_format` and `chunker_version` all equal the install's `kb_meta`. Otherwise ignore the file whole and note `vectors: no matching cache for <model id>; embedding locally`. `add --no-vectors` ignores all shipped caches.

### 5.3 Verification (because unsigned bundles are allowed and vectors are a cache)
1. **Re-keying.** The install chunks the bundle's own text with its own chunker and computes each chunk's cache key itself; a shipped vector is reachable only through a key that the bundle's text produces. A cache entry whose key matches no chunk is unreachable and ignored; more than 1% unreachable fails `validate` step 12 (the cache was built from other text).
2. **Spot check.** A deterministic sample of chunks (2%, at least 20, at most 500, seeded from the bundle hash so it is reproducible) is re-embedded locally and compared: cosine similarity ≥ **0.98** for `f32`, ≥ **0.97** for `i8` (proposed; within the quantization error `specs/004` test 19 already accepts at 0.99 for pack fidelity; `kb eval` prints the observed range so the floor can be re-set). **Any** failure discards the whole cache, logs `bundle.vectors_rejected` with the failing keys, and falls back to full local embedding. A rejected cache is never retried for that bundle hash.
3. **Scope of trust.** Vectors affect ranking only. They never carry text, never change a tier, never bypass the `trusted`/`scope` visibility filter of `specs/004` §3.4, and the reranker (`specs/004` §3.7) still reads the actual text. A poisoned vector can at worst move a chunk up or down a list that the text then has to earn.

### 5.4 Loading, progress, incremental update
- **Lexical at once.** `add` upserts the knowledge into FTS5 before returning; keyword search works immediately (`specs/004` §3.5 "indexing the words is unchanged and quick").
- **Semantic as it comes.** Accepted shipped vectors are applied with `apply_seed_vectors` (no model call); remaining chunks are embedded by the background pass. Progress is visible: `bundle.vectors_progress` events, `/org` and `delphi kb stats` show `vectors: 61,190 / 61,190 shipped, verified` or `12% embedded locally (ETA …)`; breadcrumbs say `(lexical only: vectors N% ready)` until complete (`specs/026` §6).
- **Incremental.** Because vectors are keyed by content hash, an update re-embeds **only chunks whose text changed**; unchanged chunks keep their rows (`specs/004` §3.5 "only new or changed chunks are embedded after the first time"). A new version's shipped cache is verified the same way; entries for unchanged chunks are redundant and skipped.
- **Resumable.** A chunk without a `kb_chunk_vec` row is simply embedded next; a daemon restart resumes from the table. No separate job state.
- **Large corpora.** Ingest streams: files are chunked and upserted in batches (the existing `refresh` path), embedding runs a few chunks at a time with the lock released between batches, and the model is loaded once per process. Caps: 100,000 chunks per cache in schema 1 (`specs/023` §8); a bundle over that is indexed lexically and embedded locally in the background with the same progress.

### 5.5 Storage
Shipped caches stay in the bundle's cache directory (`$DELPHI_HOME/bundles/<org>/<name>/<hash>/vectors/`) with the spot-check result recorded beside the tree as `<hash>.vectors.json` (install-owned, outside the bundle's files); applied vectors live in `state.db` (`kb_vec`, `kb_chunk_vec`) like every other vector. `remove` drops the rows; `gc` removes the directory. `state.db` stays a derived index: deleting it and re-adding reproduces the same vectors from the cache or the model.

### 5.6 Cost note
Measured in `specs/004` §3.5: about 4–5 minutes for 910 chunks on an M1 CPU (~200 chunks/min; faster on the GPU, slower on a loaded machine; Linux CPU unmeasured). A 50,000-chunk corpus is therefore roughly **4 hours of laptop CPU on first add**, against about **150 MB (`f32`) or 40 MB (`i8`) shipped** and verified in under a minute (the spot check embeds at most 500 chunks). Shipping is worth it from a few thousand chunks up when the embedder matches; below that, local embedding finishes in minutes and shipping adds little.

---

## 6. Privacy and Redaction When Sharing
Publishing a bundle is outbound, so it follows the publish gate of `specs/011` §7.1 and the contribution scrub of `specs/004` §5.5, applied to a directory:
1. **Source scope.** `distill` reads only allow-listed content (§2.3); operator scope never enters a bundle; an `isolated` workspace cannot publish or `distill`.
2. **Scan** (`validate` step 8): `[REV-SEC-05]` path and secret classes, local user paths, e-mail addresses other than `publisher.contact`, the workspace's `[privacy] identifiers`, and **person names from the roster's handles and the operator's profile** as warnings (a decision record may legitimately name its author by handle). Canary tests plant one value per class.
3. **The redaction report** (§2.4) is where the author accounts for every hit; the human approves it before `package`.
4. **Mission text.** A knowledge file derived from a mission carries a pointer to the mission file at its commit, never the mission's text beyond the summary; `events.jsonl` is unreadable to any role (`specs/001` §2.2).
5. **Exact-bytes preview on publish.** `delphi bundle publish` (store) shows the file list with hashes and the scan report; the operator approves (Red, it is outbound); the daemon signs and sends exactly those bytes; no model turn after the preview. Local `package` needs no approval: nothing leaves the machine.
6. **Un-leak is rotation.** A published secret must be rotated; a tombstone hides, it does not recall (`specs/011` §7.1 item 5).

---

## 7. Evaluation (OD-7): Three Tiers
### 7.1 Retrieval gate (frozen now; advisory)
`delphi kb eval --bundle <name>` runs the bundle's `evals/delphi/kb-eval.json` against an index containing the bundle and prints recall at 1/5/10, MRR and the share of off-topic controls answered, as `dev/kb-eval.json` is run today (`specs/004` §3.5–3.7). **Frozen thresholds:** recall@10 ≥ **0.80**, MRR ≥ **0.50**, **0** controls answered, measured with the shipped models on the publisher's machine. Below them, `package` prints a warning and records the numbers in `.delphi-bundle/eval.json`; it **never blocks** (adding is permissionless, OD-1, so a publish-side block would only be routed around). The add summary shows the recorded numbers so an adopter sees a weak bundle before adding. This measures findability of the publisher's own questions, not usefulness.

### 7.2 Task outcome: pilot, then pre-registered confirmatory run
The bench runner (`evaluation-plan-2026-10-25.md` §7; skeleton in `dev/bench/`) gains arm **A3b** (Delphi with the bundle added) paired against **A3** on task set **T8** (org tasks from `evals/delphi/tasks/*.md` with a hidden rubric). The protocol, in order:
1. **Pilot** on a **disjoint pilot task set** (at least 8 tasks that will not be in the confirmatory set), 2 runs each, A3 and A3b. Purpose: estimate variance, tune the rubric and the grader prompt, check the runner. Effect sizes seen here are **not** reported as results and set no threshold by themselves.
2. **Pre-registration file**, committed to the repository **before** any confirmatory run, at `missions/<mission>/bench/prereg-<bundle>-<date>.md`, with these required headings (checked by `validate` step 14 when it lives in a bundle's `evals/delphi/prereg/`): `## Hypotheses` (H-ids), `## Arms` (exact configs), `## Task set` (the blake3 of the `tasks/` directory and the task list), `## Rubric` (the blake3 of the rubric file and grader prompt), `## Sample size and runs` (tasks, runs per task, from the pilot variance), `## Thresholds` (the pass-rate difference and cost ratio that count as success, chosen from the pilot), `## Analysis plan` (paired comparison, McNemar for pass/fail, bootstrap CI for cost, blind graders from a family not in the arm), `## Exclusions and stopping` (what removes a run; when the study stops), `## Deviations` (empty at registration). The spec is then **amended** with the file's path and blake3 in a dated line under this section; that is the hash pin. Nothing in the file changes after the pin except `## Deviations`.
3. **Confirmatory run** exactly as registered. **No edits after seeing results**; any deviation is appended under `## Deviations` and the report carries it. The result is reported against the registered thresholds, with the pilot shown separately.
4. **What the result means** is written before the run too: if A3b does not clear the registered threshold, the honest conclusion is that the knowledge is not reaching the work, and §7.3 says where it stops.

Statistics follow the evaluation plan: paired, 3 runs, blind graders from a different model family for free-text items, event logs kept.

### 7.3 Usage telemetry (descriptive only, never a gate)
Derived from events, never from a model's claim:
- `bundle.cited`: a span's context included a chunk from bundle B (zone `bundles` result or `@bundle/` read) **and** the span's output was accepted (spec approved, PR merged, card answered yes). Per bundle and per file.
- `align.finding` counts by goal id and level (`specs/026` §5).
- `org_context.built` trims; `bundle.vectors_progress` and `bundle.vectors_rejected`; staleness; disputes open; superseded decisions still cited.
- Endorsement latency and `endorsed` share for store bundles (the xenia dogfood numbers, O-7).

`/org` shows them; `delphi bundle report <name>` prints aggregates only (never span text). A knowledge file never cited in 50 accepted tasks is a candidate for the publisher to shorten or drop. These are correlational signals and the report says so; **they gate nothing** (OD-7).

### 7.4 Not measured
Whether a team "feels aligned", and the model's own claim that it used the organization's context.

---

## 8. Acceptance Criteria
1. **AC-1 (init/validate loop):** `init` produces a tree that passes `validate` with warnings only for the empty index; a goal over 600 characters fails within the deck's check.
2. **AC-2 (distill):** with a fixture workspace holding a ratified `[T-…]` rule, a Council verdict and two endorsed records, `distill` writes only under `.distill/`, each file with daemon-stamped provenance, a `summary` and pointers; a span that read an $L_0$ file produces files `validate` refuses for `behavior/`; operator scope, `memory/`, `events.jsonl` and an `isolated` workspace are never read (path-guard denial recorded); no `package` or `add` happens.
3. **AC-3 (sign):** `sign` shows the file list and hashes, refuses when `publisher.did` differs from the key, refuses a version below `validate --against`, writes a `sig.json` that verifies under `specs/024` §2.1, has no non-interactive mode.
4. **AC-4 (archive):** `package` is byte-reproducible; import aborts at the first entry that is a symlink, has `..`, exceeds a cap, or pushes a total over its cap, and removes `.incoming/`; a valid archive adds with no network and `/org` shows `source: archive`.
5. **AC-5 (store):** a `bundle` record whose tree matches `files.json` and `bundle_hash` adds; one changed byte is reported as tampering; a record signed by a different person than `sig.json` is refused; endorsed shows `endorsed`, unendorsed `member-signed`, both at $L_2$; a new `rev` appears as "update available"; the clone runs with hooks, filters, LFS and submodules off.
6. **AC-6 (marketplace):** `specs/025` AC-8 from this end: the same `add` code path.
7. **AC-7 (privacy):** one planted canary per scan class in a knowledge file, a goal, a skill body and a pointer label each fails or warns as §6.2 says; `validate --report` lists every hit in the template's table; a `distill` output contains no text from `events.jsonl` (canary); a changed byte between preview and send aborts `publish`.
8. **AC-8 (retrieval gate):** `kb eval --bundle` prints the numbers; `package` below the frozen thresholds warns, records `eval.json`, and still packages; the add summary shows the recorded numbers.
9. **AC-9 (bench and pre-registration):** the runner accepts `A3b`/`T8` and a `--pilot <tasks>` mode that refuses to run a task also present in the confirmatory set; `validate` step 14 refuses a pre-registration file missing any required heading; the runner refuses a confirmatory run whose `tasks/` or rubric blake3 differs from the registered one; results JSONL rows carry arm, bundle hash, prereg hash and `bundle.cited` count; the self-test covers all of it with fake arms.
10. **AC-10 (telemetry):** `bundle.cited` is emitted only when a bundle chunk was in context **and** the output was accepted; `bundle report` prints aggregates only (canary); no telemetry value gates any command.
11. **AC-11 (vector key match):** a cache whose four key parts equal `kb_meta` is applied; one differing in any part is ignored whole with the note; `--no-vectors` ignores all.
12. **AC-12 (vector verification):** a cache with a planted entry for text not in the bundle has it ignored; with 2% such entries `validate` fails; a cache with one vector perturbed below the cosine floor is discarded whole, `bundle.vectors_rejected` names the key, local embedding runs, and the cache is not retried for that hash.
13. **AC-13 (lexical first, semantic later):** a keyword query answers immediately after `add` while vectors are pending; the breadcrumb says `(lexical only…)`; progress events arrive; after completion a paraphrase query answers and the note is gone.
14. **AC-14 (incremental and resumable):** updating a fixture that changes 3 of 100 chunks embeds exactly 3; killing the daemon mid-embed and restarting resumes with no re-embedding of finished chunks; `state.db` deleted and re-added reproduces identical vectors.
15. **AC-15 (skill):** `skills/org-bundle-authoring/SKILL.md` passes `delphi lint` (schema, 200-char description, template exists, no unknown check kind); the writer manifest lists it; a recorded run follows its steps and stops at the redaction-report approval.

## 9. Verification Plan
Unit tests in `delphi-bundle` (archive at every limit; reproducibility; `sign` refusals; `index.json` parsing), daemon tests in `delphi-engine` (`distill` allow-list via the path guard; stamping; staging-only writes; the vector key match, re-keying, spot check and fallback with a fixture `DLPHVEC1` built by the existing writer; incremental embed counts; restart resumption; `bundle.cited` emission conditions), xenia tests in `delphi-xenia` (the `bundle` kind, added to the `specs/012` §10 kind tests when Phase 2 is specified), `dev/bench/bench.py` self-test extended for `A3b`/`T8`, `--pilot`, the pre-registration checks, and `delphi kb eval --bundle` run by hand on `dev/bundles/acme-platform` with results recorded in the PR. Thresholds in §7.2 are frozen only by the pre-registration file and its hash pin.

## 10. Build Plan (slices B3, B8, B10, B11 of `specs/022` §6)
| Slice | Touches | Tests and fixtures | Needs | Effort |
| :-- | :--- | :--- | :--- | ---: |
| **B3 Vector pipeline** (AC-11..14; `specs/023` AC-10) | `crates/delphi-engine/src/kb.rs` (`apply_seed_vectors` generalized to load a verified external cache; `embed_pending` progress callback already exists; `Kb::refresh` batch upsert for zone `bundles`; `kb stats` readiness); `crates/delphi-engine/src/embed/` (`DLPHVEC1` reader/writer shared with `kb seed-export`; `i8` minor revision, reader-first); `crates/delphi-engine/src/vecext.rs` (no change expected; `kb_vec` rows as today); `crates/delphi-engine/src/bundle_vectors.rs` (new: key match against `kb_meta`, re-keying, seeded 2% spot check, `verified.json`, rejection and fallback); `crates/delphi-core/src/event.rs` (`bundle.vectors_progress|vectors_rejected`); `crates/delphi/src/bundle_cmd.rs` (`package --vectors` = scratch index + export; `add --no-vectors`) | fixture caches built with the real writer from the golden bundle (matching, foreign key, planted entry, perturbed vector); a 100-chunk two-version fixture for the incremental count; a kill-and-restart test using `testkit.rs`; the `kb eval` numbers before and after shipped vectors recorded in the PR | B2 | 14 h |
| **B8 Xenia `bundle` record** (AC-5) | `crates/delphi-xenia/src/` (kind module for `bundle` beside the X2 record kinds; `Domain::bundle` from B6; tree verification against `files.json`); `crates/delphi/src/xenia_cmd.rs` (`publish --bundle`, `add <id>`, `sync` listing bundle updates); the store clone hardening from X4a; `crates/delphi-engine/src/service.rs` (`bundle.update` from a store source) | the `specs/012` §10 kind tests extended; the scratch-store drill of X10 with one bundle record (publish, endorse, add, update, roll back); tamper fixture (one changed byte in the tree) | B6; xenia X7 and X8 merged; the `specs/011` §11 Phase 2 re-review | 16 h |
| **B10 Authoring** (AC-1, AC-2, AC-7, AC-15) | `crates/delphi/src/bundle_cmd.rs` (`init`, `distill`, `validate --report`); `crates/delphi-engine/src/engine.rs` (`distill` dispatches the writer role with a span-scoped read allow-list) and `crates/delphi-core/src/pathguard.rs` (allow-list mode for one span); `crates/delphi-core/src/redact.rs` (handle and `[privacy] identifiers` classes as warnings; report rows); `skills/org-bundle-authoring/{SKILL.md,templates/redaction-report.md}` (this PR); `agents/writer/AGENT.md` (`authorized_skills`); `skills/CATALOG.md` (this PR) | fixture workspace with the four allowed source kinds plus canaries in every forbidden place; report golden; the skill's lint in CI; a recorded run that stops at the approval step | B1, B3, B6 | 16 h |
| **B11 Evaluation** (AC-8..10) | `crates/delphi-engine/src/kb.rs` (`evaluate` over a bundle's question file with `--bundle`); `crates/delphi/src/bundle_cmd.rs` (`report`; `package` writes `eval.json`); `crates/delphi-engine/src/engine.rs` (`bundle.cited` at span close when a bundle chunk was in context and the outcome event is an acceptance); `crates/delphi-engine/src/align.rs` counters (from B9); `dev/bench/bench.py` + `dev/bench/README.md` (arm `A3b`, task set `T8` from a bundle's `evals/delphi/tasks/`, `--pilot`, pre-registration hash checks, JSONL columns); `crates/delphi-engine/src/checks.rs` (`prereg` headings check used by `validate` step 14); `missions/2026-10-23-foundational-design/bench/` (the first pre-registration file, written after the pilot) | runner self-test with fake arms for pilot/confirmatory separation and hash refusal; `bundle.cited` emission matrix (in context × accepted); `report` canary; `prereg` heading fixtures | B2, B9; the bench runner's open items (`evaluation-plan` §7) | 16 h |

Merge sequencing: B3 right after B2 (search without vectors is the default state until then and the breadcrumb note covers it); B10 after B6 (it needs `package` and `sign`); B11 after B9 (alignment counters) and may start its runner work earlier; B8 waits on the xenia plan. Each slice is one PR with `Implements-Spec: specs/027-…md` and the AC numbers it closes.

### 10.1 As built (slice B3)
`Implements-Spec: specs/027-bundle-authoring-distribution-and-evaluation.md` (AC-11..14; `specs/023` AC-10). The new module is `crates/delphi-engine/src/bundle_vectors.rs`; `delphi bundle add` calls it right after the lexical index is written. Where the build differs from, or settles, the text above:
- **One chunk key, one function.** `kb::embed_keys(rel_path, zone, text)` returns each chunk's cache key and the texts the model reads; the bundle checks and the carry of vectors across versions use it, and it makes the same pieces the embedder and `kb seed-export` do (they compute them inline from stored rows). The key includes the logical path `bundles/<org>/<name>/knowledge/…` (the model is given the path), so a cache is reachable only for a bundle with the same org and name as the one it was built for. `Host::chunk_keys` therefore gained `org` and `name` parameters, and the real host in `bundle_cmd.rs` calls `bundle_vectors::chunk_keys`. `validate` step 12 now really checks reachability (1% unreachable fails, any unreachable warns); the "cannot chunk" note remains only for hosts that do not implement it.
- **Key parts.** The four parts are compared as strings against the install's embedder and constants: `model_id`, `dim`, `kb::EMBED_FORMAT` (`"1"`) and `kb::CHUNKER_VERSION` (`"2"`). The `w1`/`c3` spellings in the examples of `specs/023` §8 are illustrative; a publisher writes whatever its build reports. A cache differing in any part is skipped, and when none matches the add prints `vectors: no matching cache for <model id>; embedding locally`.
- **Verification as specified, with these concrete choices.** Re-keying keeps only entries whose key the bundle's text makes here and whose window count matches (the rest are noted and ignored). The sample is 2% of the reachable chunks, at least 20 and at most 500, chosen by sorting `blake3(bundle hash || key)`; every window of each sampled chunk is re-embedded and must reach cosine 0.98. **Any** failure discards the whole cache. The verdict is written to `$DELPHI_HOME/bundles/<org>/<name>/<hash>.vectors.json`, a sibling of the cache tree, so nothing a bundle ships can name it (`bundle_vectors::verdicts_path` is the one place that knows the path; `gc` must delete it with the tree) (`verified` with the numbers, or `rejected` with the failing keys); a rejected file is never tried again for that hash, a verified one is not sampled again when the index is rebuilt (the keys are re-derived each time). A verification that could not run (the model errored) is neither: it is not used and is retried at the next add.
- **Without a model nothing is trusted.** With no installed embedder there is nothing to compare the four key parts with and nothing to sample with, so the add says the caches are unverified and unused, and the bundle is searched by its words until a model is installed. `--no-vectors` ignores the caches whatever the model.
- **Only `f32` is read.** An `i8` cache (the "DLPHVEC1 minor revision") is ignored with a note, as is a cache over 256 MiB or 100,000 chunks; `validate` already warned about both. The reader-first `i8` revision and `package --vectors` (the publisher side) belong to B6/B10 with `package`; this slice has the adopter side only. No fixture cache was built with a real model: the tests use a small word-count embedder through the same `Embedder` trait the kb tests use, and the golden bundle ships no cache.
- **Incremental across versions needed one change in the index.** A bundle's file is always rewritten at an add (its label may have changed), and rewriting dropped the document's vectors. `upsert_with` now carries the vectors of a bundle document's chunks whose text did not change into the new rows (matched by cache key, re-scoped and re-trusted from the new row), so a version that changes 3 of 150 chunks leaves exactly 3 to embed. Documents that are not a bundle's behave as before.
- **Background embedding is `delphi kb index` in a detached process**, started by `bundle add` when a model is installed and chunks are still without a vector (`kb::embed_in_background` is a thread of the calling process and would die with the short `add`). It is `Kb::embed_pending`, so it is resumable by construction: a chunk without a `kb_chunk_vec` row is the next one, and there is no job state. The daemon's own `kb.search`/`kb.index` paths continue to start `embed_in_background` when chunks lack vectors, which also resumes a pass after a restart. Two passes at once are safe (a chunk is claimed with `INSERT OR IGNORE`) but waste work; there is no cross-process lock yet.
- **Readiness has one source.** `bundle_vectors::statuses` counts a bundle's chunks with and without a `kb_chunk_vec` row and reads the sidecar. It feeds `delphi bundle list` (`vectors 100% (shipped, verified)`, `100% (local)` or `42%`), `kb stats` (one line per bundle), `delphi doctor` check 12 (informational; a discarded cache is a warning), the `/org` column (`org.view`'s `vectors` field, previously null) and the events. Search results from a bundle whose vectors are incomplete carry `note: "(lexical only: vectors N% ready)"` (`Hit.note`; printed after the pointer by `kb search`, and a `note` field in `kb.search` and the HTTP API); the note disappears when the bundle is complete. It is per bundle, not per result: a result from another source never has it.
- **Events are reported when a task starts**, like the other `bundle.*` events (`bundle_events.rs`): `bundle.vectors_progress {org, name, hash, done, total, percent, shipped}` when the numbers differ from the log's last, and `bundle.vectors_rejected {org, name, hash, file, failing, sampled}` once per rejected cache. The CLI and the background pass change the index outside any mission, so a long embedding appears in a mission's log as the counts it sees at each task, not as a per-batch stream. The Narrator shows both as one line.
- **Deferred:** `i8` and `package --vectors` (above); a per-batch progress stream and an ETA in `kb stats`; a cross-process lock for the background pass; `kb eval` numbers before and after shipped vectors (no model in the build environment); the spot-check floor is the proposed 0.98 and is not yet calibrated against a real model, which gives the same vector for the same text so the observed range should be near 1.0.
- **Residual risk, stated:** the spot check samples 2% of the reachable chunks, so a cache with a few poisoned vectors outside the sample is used. Vectors only move ranking (§5.3 "Scope of trust") and the reranker still reads the text; a larger sample is a one-constant change.

### 10.2 As built (slice B10)
`Implements-Spec: specs/027-bundle-authoring-distribution-and-evaluation.md` (AC-1, AC-2, AC-7, AC-15). Where the build differs from, or settles, the text above:
- **`init`** (`crates/delphi/src/bundle_init.rs`) writes `bundle.toml` (review_by a year out, `compat.delphi` = this build), `README.md`, `CHANGELOG.md`, `knowledge/index.md` listing one example document, and `behavior/goals.md` and `perspectives.md` with frontmatter and a commented example each (valid and empty: `validate` accepts them). It does **not** write `evals/delphi/kb-eval.json`: step 14 needs at least ten questions, so a file with the two controls would fail the first `validate`; `[sections]` omits `evals` and the next-steps text says to add it. It refuses a non-empty directory, a bad `--org`/`--name`, and a place inside a **registered project** (any workspace under `--home`; it does not refuse every git working tree, because an organization's bundle often lives in its own repository). AC-1's "a goal over 600 characters fails" is `validate` step 10, which the deck's check is; tested on the skeleton.
- **`distill` is a daemon task, not a CLI call to a model.** `delphi bundle distill [paths]` runs `/distill` through the same path as `delphi run`; the console has `/distill`; both call the new RPC `bundle.distill` (console scope only, never a relay). The engine (`crates/delphi-engine/src/distill.rs`) resolves the allow-list **deterministically** (`ALLOW`: `context/**`, `specs/**`, `missions/*/decisions.md`, `missions/*/councils/*-verdict.md` as logical paths, i.e. `team/context` and `team/specs`), then starts a **writer span** whose handoff carries the sources as context pointers and a new field `distill` (the staging directory). Before its first turn the span is narrowed: reads limited to the allow-list (the guard, so `..`, symlinks, `operator/`, `memory/agents/`, `events` and `local/` stay refused), **one tool** (`write_file`, reusing the bundle-agent tool filter), writes only under `missions/<id>/distill/<date>-<task>/**`, no `execute`, no worktree, no subscription-CLI route (that CLI has tools the guard does not govern, so a distill span with only harness endpoints escalates rather than runs). Each source is **masked** (the redactor plus the step 8 classes: secret, path, e-mail, the workspace's identifiers and handles) before the model reads it. A path named inside a source is text and is never resolved. The span's ceiling starts at L3 and falls with the sources' tiers; `write_file` stamps provenance (author `writer`, `internal_generated`) and the tier, so the stamp is the daemon's. When the span ends the staged files are scanned again and the findings are in the task's closing note, with every refused input and its reason. An `isolated` workspace (`[workspace] isolated = true`) is refused outright.
- **Staging is in the mission, not in the bundle.** The text says `<bundle>/.distill/<date>/`. A directory inside the bundle would be hashed into `files.json` and packaged, and an unreviewed draft or report must never ship, so the drafts live in the mission's own folder (`<ws>/local/missions/<id>/distill/<date>-<task>/knowledge/`), and `validate` step 8 now **fails** any `.distill` path inside a bundle. The redaction report likewise goes to a path the author names outside the bundle; `--report` refuses a path inside it, including through a symlinked parent. The skill says so.
- **Not done in `distill`:** the endorsed ($L_2$) xenia records and the previous bundle version as inputs (the first waits for the store, the second would make the daemon read an arbitrary directory; the author can copy the previous version's files into `team/context`), the nearest-existing-file dedupe preview, and the per-file diff against an existing bundle file. Both consoles have `/distill` (`ui/src/commands.json`, `ui/src/run.ts`). No recorded model-driven run exists; the tests drive the span with the scripted model, which proves the narrowing, the masking, the write scope and the closing scan, not the quality of what a model writes.
- **`validate --report <file>`** writes the template's report (`crates/delphi-bundle/src/redaction.rs`): every hit as `file:line`, class and a **masked** description (a character count, a domain, a path prefix; handles and identifiers are shown, as they are not secrets), with the action and reason columns blank for the author and the approval block for the human. It is made from the same `scan::scan_text` that step 8 fails on and that `distill` masks with, so the three cannot disagree. Warnings (`person-name`, `identifier`, `transcript`) are marked in the class cell. `--workspace <name>` (or `DELPHI_WORKSPACE`) loads that workspace's `[privacy] identifiers` and `handles` from its `config.toml` as warnings.
- **Roster handles.** There is no roster in the build (xenia X3 is in flight), so the handle class reads `[privacy] handles = [...]` from the workspace config; it moves to the roster when X3 lands. The operator's profile is prose and is not scanned for names.
- **Publish-time extensions** beyond step 8 as written: a `transcript` warning for text that names a mission's event log or record (`events.jsonl`, `local/missions/`, a workspace's `local/`, a logged `"type":"task.…"` event) per §6.4; the `.distill` refusal above; and `package` and `sign` now print every warning (the identifiers and names), with a pointer to `--report`. `package` still needs no approval (nothing leaves the machine, §6.5); the human's approval of the report is the skill's step 9 and is not enforced by a command.
- **`package --vectors`** (`bundle_pack::add_vectors`, `bundle_vectors::build_cache`): embeds the bundle's `knowledge/` chunks with the installed embedder in batches of 16 chunks, using the same `bundle_chunks`/`embed_keys` the adopter's check uses, so the keys equal `chunk_keys` by construction (tested against it, and a built cache is applied and verified by `adopt_with`). Writes `vectors/<model>-<dim>-<format>-<chunker>.dlphvec` (name sanitized to the tree's name rule), merges its entry into `vectors/index.json` (other caches are kept; the same file is replaced), and adds `vectors = true` under `[sections]`. Refuses with a message when no embedder is installed or the model fails (it says how far it got), over 100,000 chunks or 256 MiB (§8), or an empty `knowledge/`; warns when the model is not `bge-base-en-v1.5`. `f32` only. Skill `references/` are not embedded (B3 indexes `knowledge/` only). Run it before `sign`: it changes `bundle.toml` and the tree, which invalidates an earlier signature. Caches for another model that a previous run left in `vectors/` are not touched; if the text changed they are stale and step 12 fails the package with the count.
- **The skill** (`skills/org-bundle-authoring`) is authorized for the writer role (`agents/writer/AGENT.md` `authorized_skills`, its own commit as a ratified manifest change). Steps 2 and 5 now describe the staging directory and the report path; the catalog row no longer says the commands are unbuilt. The writer is not in the default `[agents] enabled` list, so `distill` says to enable it.
- **Residual risks, stated:** the masks are pattern-based, so a secret in a shape the redactor does not know reaches the model (the closing scan and `validate` use the same patterns, so they miss it too); a person's name that is not a configured handle is not found (`customer-data` and `person-name` beyond handles cannot be scanned for); the draft's quality and its faithfulness to the sources are the model's, which is why a person reads each file; the writer's own mission folder and the staged drafts are readable by the operator's other roles through the usual `missions/**` scope.

## 11. Open Questions
B-4, B-5 in `specs/022` §7. The authoring, vector and evaluation questions are decided (OD-6, OD-7, OD-8, OD-9, B-16).
