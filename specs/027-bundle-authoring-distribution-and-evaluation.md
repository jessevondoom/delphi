---
id: spec-027-bundle-authoring-distribution-and-evaluation
title: "027: Bundle Authoring, Distribution Transports & Evaluation"
author: architect
timestamp: 2026-10-25T21:00:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the transports reuse built or specified seams; the authoring and evaluation workflows are proposals with guessed thresholds
source_mission: xenia-org-memory
tags: [spec, xenia, bundles, distribution, evaluation, authoring, post-v1]
---

# Spec 027: Bundle Authoring, Distribution Transports & Evaluation

> **Status: draft, proposed.** Part of the sharing-layer set indexed by [`specs/022`](022-xenia-sharing-overview.md). Depends on `specs/023` (format), `specs/024` (adoption) and, for the store transport, on Xenia Phase 1 (`specs/011`, `specs/012`; milestones X1–X8 of `xenia-phase-1-plan.md`, of which X1 is built).

## 1. Scope & Objectives
1. **Authoring** (§2): how an organization produces a bundle: scaffold, write, distill from what the team already knows, check, sign.
2. **Packing** (§3): the archive format for a bundle that must travel as one file, including air-gapped.
3. **Transports** (§4): directory, archive, the xenia store (a `bundle` record), a static HTTP mirror, a Claude Code marketplace, and peer sync; which to build first and why.
4. **Privacy when sharing** (§5): what the publish path scans, scrubs and refuses.
5. **Evaluation** (§6): how to tell whether a bundle improves a team's results, at three levels.

### 1.1 Non-Goals
- A hosted registry or service. Every transport here is files or git.
- Live peer transport (`specs/007` Planes 1–2, deferred by D-06). Listed in §4.6 only to say how a bundle would ride on it.
- Automatic publishing. Every outbound step is a Red card with exact-bytes preview (`specs/011` §7.1).

### 1.2 Simpler Alternative Considered
- **Alternative: the bundle is a git repository; "distribution" is `git clone`; "evaluation" is whether people keep using it.** This is most of the answer for a single team and the store transport (§4.3) is exactly a git repository. Rejected as the *whole* answer because (a) a clone has no verified identity (anything the host serves is accepted) and no continuity across publisher keys, which `specs/011` §3.1 and `specs/024` §2.3 require; (b) an air-gapped machine and a Claude Code user both need one file or one marketplace entry, not a clone with Delphi-specific checks; (c) "people keep using it" cannot distinguish a bundle that helps from one that is merely present, and the operator's pre-registered evaluation discipline (`evaluation-plan-2026-10-25.md`, D-30) should apply to bundles as it does to the team itself.

---

## 2. Authoring Workflow
### 2.1 `delphi bundle init <dir> --org <org> --name <name>`
Scaffolds the §2 layout of `specs/023` with a filled `bundle.toml`, an empty `knowledge/index.md`, a `README.md` that explains the sections to a human, `behavior/goals.md` and `perspectives.md` with one commented example each, `evals/delphi/kb-eval.json` with the two required controls, and a `.gitignore` that excludes nothing (bundles are meant to be committed whole). It never writes into a registered project repository (D-03); the operator chooses a directory, normally its own repository.

### 2.2 Writing by hand
Most of a good bundle is written by people: the architecture overview, the decisions, the goals, the perspectives. `delphi bundle check` (`specs/023` §9) runs on save in the console's deck or from the shell, so an author sees caps and warnings while writing rather than at publish.

### 2.3 `delphi bundle distill` (agent-assisted; proposals only)
The organization already has knowledge in Delphi: ratified team heuristics, `context/domains/`, decision records in missions, Council verdicts, `/retro` outputs, endorsed xenia `memory` records. `distill` asks the **writer** role (synthesis, "truth before harmony", GOVERNANCE §2) to propose bundle files from them:
- **Inputs it may read:** `team/context/**`, `team/specs/**`, ratified `[T-…]` rules, the `councils/*-verdict.md` and `decisions.md` files of missions, endorsed ($L_2$) xenia records, and knowledge files of the previous bundle version. **Never:** operator scope, `memory/agents/**`, `events.jsonl`, raw mission transcripts, connector payloads, worktrees, an `isolated` workspace (`specs/004` §5.4, §5.7).
- **Outputs:** proposed files under a staging directory the operator names (`<bundle>/.distill/<date>/`), each with provenance frontmatter stamped by the daemon at the span's tier (normally $L_3$ `internal_generated`; a span that read an $L_0$ source stamps $L_0$ and `check` refuses such a file for `behavior/`), a `summary`, `pointers` to the sources it summarized (the xenia pointer format: a `git` pointer to the mission file at its commit, or `xenia` to a record), and a **diff view** against the current bundle file if one exists.
- **It proposes; the operator moves files into the bundle.** Nothing in `distill` writes under `knowledge/` or `behavior/` directly. For `behavior/` files the operator's move is the first review; ratification on adoption is the second. For knowledge, the publish scan (§5) is the second.
- **Dedupe.** Before proposing a knowledge file, `distill` runs the hybrid search over the current bundle for the candidate's summary and shows the nearest existing files, the same deterministic step the xenia publish preview uses (`xenia-phase-1-plan.md` §5.1 item 6). The librarian, when built, owns this duty (`specs/013` §2); until then it is plain code.
- **What it will not do:** invent goals or perspectives. It may *quote* a goal it finds stated in a ratified document and propose it as a `[G-…]` entry with the pointer; it does not infer intent. Goals are a human statement by construction (`specs/023` §6.1).

### 2.4 `delphi bundle sign <dir>`
Writes `.delphi-bundle/files.json` (if stale) and `.delphi-bundle/sig.json` with the install's xenia identity (`delphi xenia whoami`, X1). It is a console action with a TTY challenge like ratification and shows the file list with hashes before signing; the key never signs a manifest the operator has not seen (`specs/024` §1.3 item 3). `publisher.did` must equal the key's DID or the command refuses. There is no `--yes`.

### 2.5 Versioning discipline
`delphi bundle check --against <previous>` names the smallest honest semver bump (`specs/023` §8). `sign` refuses a version lower than that when `--against` is given, and `publish` (§4.3) always runs it against the last published version in the store.

---

## 3. The Archive: `<name>-<version>.xbundle`
One file for mail, a USB stick, an artifact store or an air-gapped import.
- **Format:** a POSIX ustar tar of the bundle tree (including `.delphi-bundle/files.json` and `sig.json`, and `.claude-plugin/plugin.json` if present), **uncompressed** (the tree is text; compression would hide the size check behind a decompressor, and zip bombs are a documented threat in `specs/014` §6), entries sorted bytewise, regular files only, mode `0644`, `mtime` fixed to the `signed_at` second, no uid/gid names, no extended headers. Two archives of the same tree are byte-identical.
- **Caps on import:** the §2 limits of `specs/023` checked **while reading**: an entry that is not a regular file, a path that is absolute or contains `..`, a size over 1 MiB, a total over 64 MiB, or more than 2,000 entries aborts extraction at that entry. Extraction goes to a temp directory in `$DELPHI_HOME/bundles/.incoming/<random>/`, is verified by `specs/023` §9 in full, and only then moved into the cache under its hash (`specs/024` §4.1). The incoming directory is removed on any failure.
- **Identity:** the archive's identity is the bundle hash inside it, not the archive's own hash; `delphi bundle adopt file.xbundle` reports both, and a xenia record (§4.3) that pins the same bundle hash is the same bundle.
- **Air-gapped flow:** `delphi bundle pack` on a connected machine; carry the file; `delphi bundle adopt <file>` on the isolated machine, which needs no network and verifies the signature against a publisher key the operator pins by fingerprint on first adoption (`specs/024` §2.3). A store's freshness and endorsement do not travel in the archive, so an archive-adopted bundle is $L_2$ by the operator's adoption only (B-2), and `/org` shows `source: archive` so nobody mistakes it for an endorsed one.

---

## 4. Transports
### 4.1 Directory (slice B2; first)
`delphi bundle adopt <dir>`. For development and for a team that keeps its bundle in a repository it already clones. No network. Unsigned allowed with the `UNSIGNED` rule (`specs/024` §2.4). **This is the minimal first transport**: it needs nothing beyond the format and the adoption card, and every other transport ends by handing a verified directory to the same code.

### 4.2 Archive (slice B5)
§3. Adds signing and the import hardening. Still no network, still no infrastructure. With §4.1 it covers an organization that distributes by git repository, mail, or artifact store.

### 4.3 The xenia store: kind `bundle` (slice B7; Xenia Phase 2)
`specs/012` §11 reserved a provisional `skill` kind pointing at a pinned git tree. This spec proposes **kind `bundle` in its place** (B-4): a skill is a bundle with one `skills/` entry and nothing else, so one mechanism serves both and the adoption rules of `specs/011` §11.1 (hash-verified tree, `delphi lint`, recomputed `requires` shown as a diff, adoption is ratification, install under a namespaced path, re-adopt on update) are the bundle rules of `specs/024` restated. Record fields, under the common envelope of `specs/012` §2:
```yaml
kind: bundle
bundle_name: acme-platform                 # ^[a-z0-9][a-z0-9-]{0,47}$ (specs/023 §3)
bundle_version: "1.4.0"                    # semver; equals bundle.toml
bundle_hash: "blake3:…"                    # the bundle hash (specs/023 §3.4): the identity across transports
tree:                                      # where the bytes are: a git pointer pinned by tree (specs/012 §4) or an archive pointer
  scheme: git
  locator: "https://git.acme.example/platform/bundle.git#."
  pin: { kind: git-tree, value: "<tree sha>" }
touches: { roles: [strategist, architect, builder, red-team], connectors: [slack] }   # copied from bundle.toml for the card
compat: { delphi: ">=0.3.0", bundle_schema: 1 }
```
- The record is signed by the publisher (a roster member); the bundle's own `sig.json` must be by the **same person** (same DID or successor chain), checked at adoption. The record's `summary` is the bundle's `description`.
- **Endorsement lifts the bundle's knowledge to $L_2$ by standing, not only by adoption:** a steward of a different person endorses `bundle_hash`, and the adoption card shows `endorsed by <handle>`; without endorsement the card shows `member-signed, not endorsed` and knowledge adopts at $L_1$ **until** the operator overrides on the card (the same quarantine-read discipline as `memory`). This is the difference between the store and an archive: the store gives the organization a say, the archive gives only the operator one.
- **Fetching the tree** is the one place a pointer is followed: `delphi xenia adopt <id>` clones the `tree` locator with the xenia store hardening (R2: hooks, filters, LFS, submodules, fsmonitor off; `protocol.allow` list; object size cap), checks out the pinned tree sha, verifies that its contents' hashes equal `files.json` and that `blake3(files.json) == bundle_hash`, and then runs the ordinary adoption. A mismatch is tampering and is reported. No other pointer is ever fetched (`specs/011` §6).
- **Updates** are new `rev`s of the same record (same `id`, same signer or successor), so `specs/011` §3.4 ownership and the history walk cover them; `delphi xenia sync` lists "bundle update available" and `delphi bundle update` adopts through the card. Rollback is local (`specs/024` §4.4).
- **Requests for the bundle's knowledge** (Phase 3 `request`/`claim`) are unchanged by this spec.

### 4.4 Static HTTP mirror (later; optional)
A read-only export of a xenia store or of a set of archives: `index.json` listing `{name, version, bundle_hash, sha256 of the .xbundle, url}` plus the files, served from any static host. Delphi reads it as a locator for archives (§4.2), verifying the archive's inner signature; the mirror confers no trust (`specs/011` §6 item 2). It exists for organizations whose git host is not reachable from where Delphi runs. Not scheduled; listed so a later slice does not invent a different manifest.

### 4.5 Claude Code marketplace (slice B6)
`specs/025` §7.3: a `marketplace.json` with `sha`-pinned sources is how a Claude Code user installs the same bundle, and `delphi bundle adopt --from-marketplace` reads one as a locator. Hosting is the same git repository as the bundle itself.

### 4.6 Peer sync (deferred; `specs/007` Planes 1–2)
If live federation is built, a bundle crosses it as its **`bundle` record plus its archive bytes**, not as files: the receiving gateway verifies the archive exactly as §3 and the record exactly as §4.3, and `specs/007` §3.1's $\min$ with the peer ceiling applies on top. No bundle-specific protocol is needed, which is the point of carrying identity in the hash.

### 4.7 Recommendation
Build **directory → archive → store** in that order. Directory needs nothing and unblocks authoring and the strategist work (`specs/026`); archive adds signing and air-gap; the store adds organizational standing and depends on Xenia Phase 1 milestones X7 (publish) and X8 (admin) which are not yet built. Marketplace export is a small addition to archive; the mirror and peer sync wait for evidence of need (`delphi needs`).

---

## 5. Privacy and Redaction When Sharing
Publishing a bundle is outbound, so it follows the publish gate of `specs/011` §7.1 and the contribution scrub of `specs/004` §5.5, applied to a directory:
1. **Source scope.** `distill` reads only promoted team and workspace content (§2.3); operator scope never enters a bundle; an `isolated` workspace cannot publish or `distill`.
2. **Scan.** `delphi bundle check` step 8: `[REV-SEC-05]` path and secret classes, local user paths, e-mail addresses other than `publisher.contact`, the workspace's `[privacy] identifiers`, and (new) **person names from the roster's `handle` list and the operator's own profile** are flagged (warning, since a decision record may legitimately name its author by handle; the operator decides). Canary tests plant one value per class (`specs/023` AC-6).
3. **Mission text.** A knowledge file derived from a mission carries a pointer to the mission file at its commit, never the mission's text beyond the summary the writer role produced; `events.jsonl` is unreadable to any role (`specs/001` §2.2) so it cannot leak by construction.
4. **Exact-bytes preview.** `delphi bundle publish` (store) and `pack` (archive) show the file list with hashes and the scan report; the operator approves (Red); the daemon signs and sends **exactly** those bytes; no model turn after the preview.
5. **Un-leak is rotation.** A secret that reached a store or an archive must be rotated; tombstoning the record hides it, it does not recall it (`specs/011` §7.1 item 5). The adoption card of a later version says "supersedes a withdrawn version" so recipients know to drop the old cache (`gc` removes it).

---

## 6. Evaluation: Does a Bundle Improve Team Results?
Three levels, cheapest first. Thresholds are proposals to be frozen before the first measured run, as D-30 froze the team evaluation.

### 6.1 Retrieval (per bundle, deterministic, minutes)
`delphi kb eval --bundle <name>` runs the bundle's own `evals/delphi/kb-eval.json` (`specs/023` §7) against an index that contains the bundle, reporting recall at 1/5/10, MRR and the share of off-topic controls answered, exactly as `dev/kb-eval.json` is run today (`specs/004` §3.5–3.7). **Proposed gate for `publish`:** recall@10 ≥ 0.80 and 0 controls answered, on the publisher's machine with the shipped models; `check` warns below it and `publish` refuses unless `--allow-weak-retrieval` is given and the reason is typed (recorded in the record's `note`). A bundle whose own questions cannot be found is not ready. Because the publisher writes the questions, this measures *findability*, not usefulness; §6.2 measures that.

### 6.2 Task outcome (per bundle, paid, pre-registered)
The bench runner (`evaluation-plan-2026-10-25.md` §7, built as a skeleton in `dev/bench/`) gains one arm and one task set:
- **Arm A3b:** A3 (Delphi, full pipeline) with the bundle adopted; compared paired against A3 without it on the same tasks.
- **Task set T8 (org tasks):** the bundle's `evals/delphi/tasks/*.md`: tasks whose correct answer depends on the organization's knowledge (a runbook step, a decision's constraint, a convention) with a **hidden rubric** the arm never sees. At least 15 tasks per bundle; the organization writes them before tuning, and they live in the bundle so the measure travels with it.
- **Metrics:** rubric pass rate (primary); cards per task; the share of tasks in which a bundle chunk was in the span's context and cited in the result (`bundle.cited`, §6.3); cost per accepted task.
- **Proposed success criterion:** rubric pass rate at least **10 points** higher with the bundle (the same margin D-30 fixed for the team), at a median cost per accepted task at most **1.3×**. If A3b does not beat A3 on T8, the honest conclusion is that the knowledge is not reaching the work, and §6.3 says where it stops.
- Statistics as in the evaluation plan: paired, 3 runs, McNemar for pass/fail, bootstrap for cost; graders blind to the arm and from a different model family for free-text rubric items.

### 6.3 Usage telemetry (per install, free, continuous)
Derived from events already written or added by this set, never from a model's say-so:
- `bundle.cited`: a span's context included a chunk from bundle B (zone `bundles` search result or `@bundle/` read) **and** the span's output was accepted (spec approved, PR merged, card answered yes). Counted per bundle and per knowledge file.
- `align.finding` counts by goal id and level (`specs/026` §5): which goals specs serve, which they conflict with.
- `org_context.built` trims: how often the slice overflowed (a signal that the bundle's goals are too many or too long).
- Staleness: files past `review_by`; disputes open; superseded decisions still cited (the pre-pass).
- Endorsement latency and $L_2$ share for store-distributed bundles, the same two numbers the xenia dogfood gate records (`specs/011` §1.4 gate 8, O-7).

`/org` shows them; `delphi bundle report <name>` prints them for a publisher (aggregates only, never the span text, following the contribution-scrub rule of `specs/004` §5.5). A knowledge file that is never cited in 50 accepted tasks is a candidate for the publisher to shorten or drop, as an un-scored rule is a candidate for `DEMOTE` (`specs/004` §5.6). These are correlational signals and the report says so.

### 6.4 What is deliberately not measured
Whether a team "feels aligned". The console shows goals and findings; whether people agree with the goals is a conversation, not a metric. And the model's own claim that it "used the organization's context" is never counted; only the deterministic `bundle.cited` is.

---

## 7. Acceptance Criteria
1. **AC-1 (init/check loop):** `init` produces a tree that passes `check` with warnings only for the empty index; editing a goal over 600 characters produces the cap failure within the deck's check.
2. **AC-2 (distill):** with a fixture workspace holding a ratified `[T-…]` rule, a Council verdict and two endorsed xenia records, `distill` proposes files only under the staging directory, each with daemon-stamped provenance, a `summary` and source pointers; nothing is written under `knowledge/` or `behavior/`; a run whose span read an $L_0$ file produces files `check` refuses for `behavior/`; operator scope, `memory/`, `events.jsonl` and an `isolated` workspace are never read (path-guard denial recorded).
3. **AC-3 (sign):** `sign` shows the file list and hashes, refuses when `publisher.did` differs from the key, refuses a version below `check --against`, writes a `sig.json` that verifies under `specs/024` §2.1, and has no non-interactive mode.
4. **AC-4 (archive):** `pack` is byte-reproducible for the same tree; import aborts at the first entry that is a symlink, has `..`, exceeds 1 MiB, or pushes the total over 64 MiB, and removes `.incoming/`; a valid archive adopts with no network (denied-network test run) and `/org` shows `source: archive`.
5. **AC-5 (store):** a `bundle` record whose `tree` checks out to files matching `files.json` with `blake3(files.json) == bundle_hash` adopts; a tree with one changed byte is reported as tampering; a record signed by a different person than the bundle's `sig.json` is refused; endorsed adopts at $L_2$, unendorsed at $L_1$ with the override shown; a new `rev` appears as "update available" and the clone runs with hooks, filters, LFS and submodules disabled.
6. **AC-6 (marketplace):** `specs/025` AC-7 holds from this end as well: `adopt --from-marketplace` lands in the same adoption code path as a directory.
7. **AC-7 (privacy):** one planted canary per scan class in a knowledge file, a goal, a skill body and a pointer label each fails or warns as §5.2 says; a `distill` output contains no text from `events.jsonl` (canary planted there); a changed byte between preview and send aborts `publish`.
8. **AC-8 (retrieval eval):** `kb eval --bundle` runs the bundle's questions and prints the four numbers; `publish` refuses below the gate without the typed reason and records the reason in the record.
9. **AC-9 (bench):** the runner accepts arm `A3b` and task set `T8` from a bundle's `evals/delphi/tasks/`, applies the hidden rubric after the arm finishes, and writes one JSONL row per run with the arm, bundle hash and `bundle.cited` count; the runner self-test covers it with fake arms.
10. **AC-10 (telemetry):** `bundle.cited` is emitted only when a bundle chunk was in the span's context **and** the output was accepted; `bundle report` prints aggregates only (no span text; canary test).

## 8. Verification Plan
Unit tests in `delphi-bundle` (archive writer/reader at every limit; reproducibility; `sign` refusals), daemon tests in `delphi-engine` (`distill` input allow-list through the path guard; stamping; staging-only writes; `bundle.cited` emission conditions), xenia tests in `delphi-xenia` (the `bundle` kind's schema and tree verification, added to the kind tests of `specs/012` §10 when Phase 2 is specified), `dev/bench/bench.py` self-test extended for `A3b`/`T8` with fake arms, and `delphi kb eval --bundle` run by hand on the `dev/bundles/acme-platform` fixture with its results recorded in the PR. The store path's live test is the scratch-store drill of X10 extended with one `bundle` record. Thresholds in §6.1–6.2 are frozen by a dated operator decision before the first paid run (as D-30).

## 9. Open Questions
B-4 (`bundle` kind supersedes `skill`), B-5 (transport order), B-14 (distill sources and whether an agent-run distill is acceptable at all), B-15 (evaluation thresholds) in `specs/022` §7.
