---
id: spec-058-project-scopes-scaffolding-and-promotion
title: "058: Project scopes, scaffolding, indexes and promotion to shared memory"
author: architect
timestamp: 2026-10-30T00:00:00Z
status: approved   # approved and ratified by the operator on 2026-10-30; this spec is the explicit authorization to amend the project-clean rule in 006
approved_by: operator (2026-10-30)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, project, scopes, memory, context, index, promotion, onboarding]
---

# Spec 058: Project scopes, scaffolding, indexes and promotion to shared memory

> Approved and ratified by the operator on 2026-10-30. This spec refines workspace resolution and the project clean-repo rule in [`specs/006`](006-installer-and-updater.md), extends the scope routing in [`specs/004`](004-memory-and-coaching.md), and defines project-to-home promotion alongside [`specs/031`](031-layered-learning-and-training-mode.md), [`specs/052`](052-knowledge-status-capture-on-contact-and-the-digest.md) and [`specs/055`](055-the-central-library-sources-claims-and-pages.md). It does not change approval tiers, taint, source status, or the rule that content cannot grant itself authority.

## 1. Goal

Delphi is usable from any directory, whether or not it is a Git repository and whether or not it has been registered. A repository is useful for Git operations, but it is not a prerequisite for a project scope.

Every running session has a shared home layer and exactly one active project layer:

1. **Home layer:** the operator's default memory and context under `$DELPHI_HOME/operator/`, available in every project and directory.
2. **Project layer:** project-specific memory and context. A registered directory project is active at its root and every descendant. If no registered project contains the current directory, the persistent default project is active.

The layers remain identifiable in retrieval and writes. Project-specific learning is stored with that project by default. The operator may deliberately promote a reusable summary, with eligible source references, into the home layer. Promotion is proposed and reviewed; it is never automatic.

## 2. Goals and non-goals

### 2.1 Goals

- Resolve a deterministic active scope for every invocation, including outside Git.
- Register arbitrary directory roots centrally in the Delphi home.
- Make project-specific specs, plans, context, research, work output and a navigational `_index.md` visible in the project directory.
- Load home context and memory everywhere, and add the most-specific registered project's context and memory when inside it or a subdirectory.
- Keep runtime state, credentials, event logs, indexes/caches and private operator memory under `$DELPHI_HOME`.
- Provide a reviewed project-to-home promotion flow that can preserve references to trusted original research sources.
- Preserve a safe path for existing workspaces and projects during migration.

### 2.2 Non-goals

- Requiring Git, a remote, or a repository metadata directory to create or use a project.
- Automatically sharing project memory, source captures, or raw research text with home scope.
- Making `_index.md` an authoritative README, a source of claims, an instruction file, or a trust-bearing record.
- Copying source bodies into home memory during promotion. Source inclusion means preserving a reference to an existing library record, not duplicating its full text.
- Replacing 031's `/retro` ratification rules or 055's source and claim status model.

## 3. Scope model and resolution

### 3.1 Project identity and registration

- A project is a registered directory scope, not a Git repository. `delphi project register [path]` registers the selected directory; the command works without Git. Bare `delphi` remains usable without registration.
- Registration assigns an immutable random `project_id`. The central registry under the selected workspace in `$DELPHI_HOME/workspaces/<name>/local/projects.json` maps the ID to a canonical root path, display name, creation time and last-seen time. The operator home contains the shared home layer and is not itself a project.
- The fallback project has the reserved ID `default` and stores its memory/context under `$DELPHI_HOME/workspaces/<name>/local/projects/default/`. It has no filesystem root. It is not listed as a registered directory project and cannot be relinked or cloned.
- A small `.delphi-project` marker at the project root contains only the schema version and `project_id`. It is an identity marker, not a configuration or instruction file. It contains no credentials, memory, event log or cache. If the directory is not writable, registration may keep identity centrally and report that the marker could not be written; path-based resolution still works for that registration.
- A copy of a project directory that carries the marker is not silently treated as the same live scope. On detecting one project ID at two canonical roots, Delphi reports a duplicate and requires `project relink` or `project clone` to create an independent identity. It never merges the two project memories.
- Moving a root is handled by `delphi project relink <project-id> <new-path>`, an operator action that updates the central path after validating the marker. Missing roots are retained in the registry and reported as unavailable; they are not silently deleted or rebound.

### 3.2 Resolution rules

1. Explicit `--workspace`, `DELPHI_WORKSPACE`, and existing workspace selection rules in 006 determine the workspace first.
2. The invocation's current working directory is resolved to a canonical path. The registry is searched for registered roots that contain it.
3. If one or more roots match, the most-specific (longest path-component prefix) root is the project layer. Nested projects are isolated: the child project is active in its subtree; the parent project resumes outside that subtree. Home memory/context is available in either case.
4. If no root matches, the active project layer is the persistent `default` project; the home layer is still available. The current directory may be read as task input under existing path and taint rules. Durable project learning is scoped to the default project, not to an inferred root at the current directory.
5. Symlinked working directories are resolved before matching. A symlink that resolves within a registered root uses that root. A symlink that resolves outside it is outside that scope. Delphi never indexes through a symlink to paths outside the selected root.
6. Ambiguous duplicate registrations, corrupt registry entries, or an inaccessible canonical path produce an explicit diagnostic and fail closed for project-specific access. Delphi remains available with home scope only.

`delphi project scope` (also available as `delphi which-scope`) reports the workspace, home layer, active project ID/root, resolution reason, and whether project-local writes are available. It prints no private memory content.

### 3.3 Default behavior and subdirectories

- On first invocation, Delphi creates or resolves the default workspace/home using 006. It does not require a repository or registration to start the console or answer a question.
- The default project is persistent and centrally stored. It has its own project memory and context in addition to the shared home layer. All directories outside registered roots use this default project, as the fallback scope. Consequently, durable lessons from unrelated unregistered directories can meet in the default project; registering a directory gives it an isolated project-memory layer.
- Bare launch never registers or scaffolds the current directory. An operator may explicitly register/init any directory (with or without Git) to give it a distinct project identity and local artifact scaffold. Existing automatic workspace creation does not imply project registration.
- Once inside a project root, running Delphi from any nested directory resolves to the same project, unless a more-specific nested project root is registered.

## 4. Knowledge and memory routing

### 4.1 Read path

- Home memory and context are eligible in every mission, subject to existing precedence, trust, visibility and token-budget rules.
- When the current directory resolves to a registered project, relevant project context and project memory are added as a distinct source layer. Retrieval records the scope and provenance for every result; project content never silently masquerades as home content.
- Default precedence remains the precedence defined in 004. More-specific project material may specialize a home preference, but neither layer may relax governance, approval requirements, sandbox boundaries or trust rules.
- Reads from project files retain their actual provenance/taint. Registration does not make repository content ratified. Specs, context and `_index.md` are data; none can issue approvals, widen authority or promote their own trust.

### 4.2 Write path

- New durable lessons, preferences and research-derived notes from project work default to that project's memory/library scope when one is active.
- Work outside a registered project defaults to staged default-project learning under 031. A note is not always-loaded merely because it was captured. The default project's durable memory is separate from shared home memory and from every registered project's memory.
- Memory remains staged or proposed until the existing ratification gate applies. Existing taint ceilings and 050 write-path requirements apply to every project and home write.
- Agents cannot choose a broader scope than the caller's active scope. The operator can select a destination explicitly in a proposal, and daemon code validates it.
- Project memory is not automatically copied into source-controlled files. Deliberately shared project context is placed in `context/` by an operator-reviewed action; private agent memory remains under the central home, keyed by project ID.

### 4.3 Home and project isolation

Home memory is intentionally shared among the operator's projects. Project-specific memory is only eligible when its project scope is active, including from subdirectories. A nested project does not inherit its parent's project memory; it receives home memory plus its own project layer. The default project is the fallback layer for every directory outside registered roots; it is not the home layer. Project-to-home transfer uses §7. No implicit transfer between two registered projects exists.

## 5. Project scaffold

`delphi project register` records the root centrally and creates the identity marker, but does not create the artifact folders. `delphi project init` creates the following layout at the selected root. It is idempotent and creates only missing paths:

```text
<project>/
├── .delphi-project          # stable identity marker; no secrets or runtime state
├── _index.md                # generated human-readable project map
├── specs/                   # project specs; status frontmatter governs build eligibility
├── plans/                   # build plans and project roadmaps
├── context/                 # reviewed, project-shared context for agents
├── research/                # research notes with citations and source-status labels
└── work/                    # reports, generated work output and mission deliverables
```

Additional folders are created when a feature first needs them; empty placeholders are not required. The durable project memory store remains in the central home under `local/projects/<project-id>/memory/` and `library/`, indexed under the project scope. The project `context/` directory is the versionable, human-reviewable shared context surface.

Scaffolding rules:

- Before writing, report the exact paths that will be created. `project register` explicitly authorizes creation of the marker only. In an interactive session, `project init` asks for confirmation the first time; non-interactive mode requires explicit `project init --yes`. Bare `delphi` never writes project files merely because it was launched there.
- Existing files and directories are preserved. Existing `specs/`, `plans/`, `context/`, `research/` or `work/` directories are adopted without changing their contents. A colliding non-directory path is an error with a repair instruction.
- `_index.md` is generated only if absent, or replaced only if its recorded generated-content hash matches the current file. If it has hand edits, generation writes a proposed replacement beside it and reports the conflict; it never discards edits.
- `.delphi-project` is created only when absent. A marker with a different ID is not overwritten; explicit relink/clone resolves the conflict.
- Project worktree/build isolation and approval gates remain in force. This scaffold does not authorize direct edits to a project checkout by an agent.

The scaffold amends 006's rule that registered project repositories contain no Delphi files. The allowed footprint is limited to the marker, approved project artifacts, and generated index described here. Runtime state stays outside the project.

## 6. Generated `_index.md`

### 6.1 Purpose and content

`_index.md` is the human-readable front door to a project. The daemon generates it from project files and indexed knowledge; it is a navigational summary, not an independent source of truth.

It contains, in order:

1. A generated notice, project name/ID and concise description when available.
2. **Overview and current focus:** purpose, active approved spec/build plan pointers, and a short project map.
3. **Recent activity:** a bounded, high-level changelog of completed or materially changed work, linked to the source artifacts. It summarizes recorded outcomes, not private reasoning or raw event logs. It is not a duplicate Git log.
4. **Project stats:** deterministic counts of eligible files by type/folder, specs by status, research notes and indexed knowledge entries by status, and last meaningful project update. Volatile process data and private usage data are excluded.
5. **Project contents:** a deterministic hierarchical index of eligible project files, including `specs/`, `plans/`, `context/`, `research/`, `work/` and other eligible project paths.
6. **Knowledge base:** entries scoped to this project, with stable links/IDs, title, type and the 052 status label. Central home entries are listed only when they are relevant and visible to this project, clearly marked `home`; private entries and their titles are excluded.

Every eligible indexed file and knowledge entry must be directly discoverable through `_index.md` or a linked generated continuation index. For large projects, the index may link to deterministic category indexes under `.delphi-index/`; `_index.md` must link every category index and state the indexed-file count. Category indexes are generated views with the same integrity and exclusion rules.

### 6.2 Generation and freshness

- Generation uses stable path ordering, normalized line endings and stable formatting. It omits generation timestamps from content unless the displayed `last meaningful update` changed; generation time and input hashes belong in central metadata.
- Refresh occurs on explicit `delphi project index [--refresh]`, after registration/init, and after project changes are observed by existing indexing flows. Refresh is incremental where possible. It does not run a full tree scan on every prompt.
- Per-project metadata and hashes live centrally under `local/projects/<project-id>/index/`; the index cache is disposable and rebuildable.
- The index includes only files allowed by `.gitignore`, `.delphiignore` and Delphi's default exclusions. Default exclusions include credentials, secret stores, `.git/`, `.delphi/` runtime/private state, caches, build outputs, dependency/vendor trees, and files above configured size/type limits. `.delphiignore` can exclude more paths, never force-include a protected/default-excluded path.
- Hidden files are excluded by default unless explicitly eligible through an allowlisted class; symlinks are not traversed. Paths are relative to the project root; absolute host paths are never emitted.
- Source text, filenames, summaries and knowledge titles are untrusted data. The index is read as tainted content, fenced from instruction following, and cannot become an authority source. No private memory content is materialized into `_index.md`.

### 6.3 Protection and conflicts

- Before writing or refreshing, the daemon applies exclusion rules to both path names and content-derived metadata. Secret detection is a secondary rejection layer, not a substitute for default-deny classes.
- The generator records a hash of its last complete output in central metadata. A changed on-disk `_index.md` with a mismatched hash is treated as operator-edited: preserve it and write a `.delphi-index/proposed-index.md` candidate for review.
- Write protocol (amendment 058-A1, council 012): take an exclusive lock under `local/projects/<project-id>/index/`; refuse if `_index.md` is a symbolic link; rename the current file aside to `.delphi-index/prev.md`; hash it. If it matches the recorded hash, or no hash is recorded and it equals the stub the scaffold writes, write the new file to a temporary path, flush it, and `link` it into place (which fails rather than replace a file an editor has recreated); otherwise link the original back and write `.delphi-index/proposed-index.md`. Record the new hash by temporary file and rename, then release the lock. Ignore rules, symbolic links, secrets and private titles are re-evaluated on every pass.
- The generated notice states that the file is generated, may be incomplete, and does not confer trust or authority on listed content.
- The index is not itself used as input to build another index, a knowledge claim, a generated research page, or a prompt instruction. When agents need its navigation, it is opened in a tainted, read-only span and links are independently resolved against the project root.

## 7. Project-to-home promotion

### 7.1 Surface and proposal

- `/promote` is a discoverable alias for the promotion queue in `/retro`. Both open the same proposal system, filtering to project-to-home promotions when invoked as `/promote`.
- The console and daemon equivalent support list, inspect, propose, accept and reject. Existing `/retro` behavior remains unchanged. Promotion is a distinct proposal kind and requires an explicit operator acceptance; an Amber batch approval cannot accept it implicitly.
- An agent can prepare a proposal from project-scoped lessons, research notes, validated claims and cited source records. The proposal includes a concise reusable summary, proposed home destination/type, originating project ID, source material IDs, uncertainty, known conflicts/duplicates and redaction findings.
- The proposal is data-only. It cannot write into home memory, context or the central library until the operator accepts it. Agent-authored content remains subject to its source taint and the 031/050 ratification rules.

### 7.2 Original trusted source references

The operator may select **include eligible source references** while reviewing a promotion. This option includes references and provenance, not source bodies.

A source reference is eligible only when all are true:

1. It resolves to an existing central-library source record (055), not merely a URL, filename, or citation invented in a research note.
2. Its current derived status is `validated`, or `validated · vouched by <org>` through an active, unchanged bundle trust record (052, 024, 055). `noted`, `checking`, `contested`, `disproved`, `retired`, purged, revoked or stale references are excluded and reported.
3. It is not `private`, and its metadata/citation may be visible at home scope under its licence and privacy classification. Restricted source bodies are never copied by this operation.
4. The reference retains source ID, title, origin, canonical locator, version/hash, status, licence and relevant anchor/locator. The central record remains the source of truth.

The operator may accept the summary without references, or accept the summary with the eligible subset. Ineligible references are shown with reasons and cannot be forced into the trusted-reference set by an agent. Promotion never raises source/claim status, validates the summary, or makes a bundle-vouched source independent of its bundle trust. If that trust is revoked later, the home entry shows the changed source status and is queued for review.

### 7.3 Apply, deduplication and reversal

- Acceptance performs a daemon-side compare-and-swap against the proposal version and target home records. A stale proposal must be reviewed again. The eligibility checks of §7.1 and §7.2 (private flag, library status, bundle revocation, secret scan) run at proposal time and again inside acceptance under that compare-and-swap, so a record that became private or revoked after the proposal cannot be promoted (amendment 058-A1).
- The daemon checks for likely duplicates and contradictions against home memory/claims. It presents matches; it does not silently overwrite, merge or supersede a home record.
- The accepted home entry preserves provenance to the project and original records, plus the operator and acceptance time. It retains the original taint/status constraints; operator acceptance ratifies the *summary as home memory* but does not validate factual claims or raise any cited source's status.
- Rejection records only the proposal outcome and operator-provided reason subject to existing privacy rules; it does not copy proposal text into an always-loaded store.
- The operator may withdraw a promoted entry. Withdrawal marks it inactive/retired with an audit reference; it does not delete source records or rewrite the project original.

### 7.4 Amendment 058-A2 (council 013, D11: promotion hardening)

- **Decision:** the console-only compare-and-swap acceptance stays the control; no Red card is added. It was verified before being kept: (a) `delphi retro show`/`accept` print the full summary that would land in `local/promoted/` and then ask for a typed yes at a terminal, and `retro.promotions` returns the same summary with the `version` that `retro.accept` requires; (b) `retro.accept` is console-scope only (a relay or narrator seat is refused), and acceptance is bound to a hash of the summary, the records' hashes and the home version, so a changed proposal is `stale_proposal`. Had either failed, acceptance would need a Red card whose payload is the summary text.
- **Source floor:** a record whose `source_type` is below `federated_verified` (`external_untrusted`, `federated_peer`) is refused at proposal and again at acceptance with `source_untrusted`. The ceiling of §7.3 is unchanged.
- **Cap:** `local/promoted/` holds at most 200 active entries and 256 KiB of them; a proposal or acceptance past either is refused with `promoted_cap_reached`. A retired entry no longer counts, so withdrawal frees room.
- **Acceptance tests:** `records_below_the_source_floor_are_refused_with_a_reason_code` and `the_promoted_store_is_capped_in_entries_and_bytes_and_a_withdrawal_frees_room` in `crates/delphi-engine/src/promote.rs`; the existing narrator-denied and stale-version cases of `retro.accept` cover (b).

## 8. Commands and user experience

| Command | Behavior |
| :-- | :-- |
| `delphi project register [path]` | Register a directory as a project scope without requiring Git; create only its identity marker and report the scaffold paths available through `project init`. |
| `delphi project init [path] [--yes]` | Register if needed and create only missing artifact scaffold paths under §5. |
| `delphi project list` | List registered roots and availability without exposing memory contents. |
| `delphi project scope` / `delphi which-scope` | Explain active home/project scopes and the resolution reason. |
| `delphi project relink <id> <path>` | Explicitly move the central mapping after marker/identity checks. |
| `delphi project clone <path>` | Give a copied project directory a fresh ID and empty project-memory association; preserve project artifact files. |
| `delphi project index [--refresh]` | Show freshness or regenerate `_index.md` and category indexes. |
| `/retro` | Existing retrospective and learning proposal surface, including promotion proposals. |
| `/promote` | Alias into `/retro`, initially filtered to project-to-home promotion proposals or creation. |

`--home`, workspace selection and noninteractive semantics remain compatible with 006. Bare `delphi` never registers a directory or writes project files without explicit registration/init. In a project, the console scope indicator names both the home layer and active project; outside one, it says `home + default project`.

## 9. Migration and compatibility

- Existing registered Git roots retain their existing project/workspace association. On first use after upgrade, the registry receives stable IDs and a marker is offered/created only under the explicit migration path; no existing project files are rewritten automatically.
- Existing specs in the workspace's `team/specs/` remain workspace/team specs. The operator may move/copy selected specs into a project's `specs/` through a reviewed migration; source hashes and ratification state are preserved or re-approved according to 006's ratified-spec rules.
- Existing home memory remains home memory and becomes available in every project. No project memory is inferred from path, Git history, or agent lessons.
- Existing knowledge records retain their current scope. A one-time migration may assign project scope only after an explicit root-to-project mapping; uncertain records stay at their prior scope.
- The 006 clean-repo invariant is amended: ordinary runtime state still never enters project roots, but the narrowly allowlisted marker, project scaffold and generated indexes in this spec are permitted. `doctor` checks that no credentials, event logs, caches or private memory have entered the project tree.
- Bare invocation from outside a repository continues to work using shared home memory plus the persistent default-project layer. No current-directory registration, index or scaffold is created unless the operator explicitly registers that directory.

## 10. Slices

| Slice | Deliverable | Depends on |
| :-- | :-- | :-- |
| PS1 | Registry schema, stable project IDs, registration/list/scope/relink/clone, canonical resolution including nested roots and subdirectories; amend 006 resolution | 006 |
| PS2 | Home + project memory/context routing, provenance labels, staged write routing and project-isolation checks | PS1; 004, 031, 050, 052 |
| PS3 | Explicit scaffold and project marker, no-Git support, collision handling, migration; amend 006 clean-repo rule | PS1 |
| PS4 | `_index.md` and category index generation, ignore/exclusion rules, deterministic output, secret/privacy protections, hand-edit detection | PS1, PS3; 028, 043, 052 |
| PS5 | Project-to-home promotion proposal kind, summary/provenance/duplicate checks, accept/reject/withdraw | PS2; 031 L2; 055 SK3 |
| PS6 | Eligible trusted-source reference selection and trust-revocation updates in promotion | PS5; 024, 052, 055 |
| PS7 | `/promote` alias, project index and scope UX, docs and tracker; end-to-end migration polish | PS1–PS6; 053, 054 |

One slice per pull request. Each implementation PR cites this approved spec and updates the tracker when a slice state changes.

## 11. Verification

- **Availability:** bare `delphi` works in a registered repo, an unregistered repo, a non-Git directory and a nested directory; absent project scope never prevents home-scope use.
- **Resolution:** root and descendants resolve to the project; sibling paths do not; most-specific nested project wins; symlinks resolving inside/outside behave per §3.2; duplicate IDs fail closed; moved roots require relink.
- **No-Git project:** registering an ordinary directory creates the same project identity and scaffold as a Git directory; Git-only commands explain their limitation without disabling chat, knowledge or memory.
- **Memory layers:** home facts are available in all projects; registered project A memory is unavailable in project B and default project; unregistered directories share the default-project layer; a child project sees home plus child, not parent-project memory; invocation from any subdirectory loads the correct project layer.
- **Taint:** unratified project specs/context and `_index.md` do not gain trust, approve actions, widen tools or alter sandbox scope. Tainted source notes cannot be promoted above their inherited ceiling.
- **Scaffold safety:** init preserves existing files, errors on path collisions, does not run from bare invocation, and records marker identity. A copied ID is detected; clone creates independent memory identity.
- **Index coverage:** every eligible file and visible knowledge record is directly listed or reachable through a linked category index; deterministic inputs yield byte-identical output; ignored, secret, private, oversized and outside-root paths are absent.
- **Index integrity:** generated content is refreshed only when the prior hash matches; hand edits survive and create a proposal; the index is not recursively ingested as authoritative knowledge.
- **Promotion:** no agent can apply a proposal; accept/reject is operator-only and CAS protected; duplicates/conflicts are surfaced; accepted summary retains project provenance; withdrawal is auditable.
- **Trusted references:** only current validated or actively bundle-vouched library records can be included; noted/contested/disproved/retired/private/purged/revoked records are excluded; promotion does not change a source status; revocation updates home display and review queue.
- **Migration:** existing workspace memory and project mappings remain usable; no private memory, event log, credentials or cache is written into the project; doctor detects such leakage.

## 12. Decisions (operator, 2026-10-30)

- **D-1 — Always available:** Delphi runs from any directory and does not require Git or registration.
- **D-2 — Layered scope:** home memory/context is shared into every project; the active registered project adds its own memory/context, including from every subdirectory beneath its root.
- **D-3 — Default project:** outside registered roots, use shared home memory plus the persistent default-project memory/context. The current directory does not become a registered root implicitly; its task context is read under normal rules. Explicit registration creates an isolated project layer and enables project-local scaffolding.
- **D-4 — Git is optional:** any directory can be registered; Git supplies version-control capabilities only.
- **D-5 — Central runtime:** credentials, runtime, event logs, caches/index metadata and private memory remain in `$DELPHI_HOME`; only the specified project marker and human-reviewable artifacts/index may live in the project.
- **D-6 — `_index.md`:** generated human-readable overview, high-level changelog, project stats, and complete navigational index of eligible project files and visible knowledge entries.
- **D-7 — Promotion:** `/promote` aliases `/retro` and routes project-to-home promotion; the operator explicitly accepts each proposal.
- **D-8 — Trusted references:** promotion may include references to existing validated or actively bundle-vouched sources, preserving provenance/status/licence; it never copies source bodies or elevates trust.

## 13. Amendment 058-A1: research routing and trust gating (operator, 2026-10-31)
Research the operator asks for (the expand job, `specs/028` §10) is written where its scope says, and only a trusted source reaches the shared knowledge base on its own. The class of a source is the validator's (`specs/040` Appendix A and its amendments); no model decides it.

1. **Default scope** (no project named). A kept page whose final address is **Primary** or **Reputable secondary** is written under `knowledge/web/` and indexed at once, as before. Any other page (Weak) is written under `local/pending/web/`, which the index never reads, and stays local until a validator pass or the operator ratifies it (`/validate`, `/retro`: slice 4). With `[validator] weak_sources = "off"` a Weak page is not stored at all (`specs/040` §27.5). An Excluded page (shortener, cache, mirror) is never stored.
2. **Project scope** (`research.start` names a registered project). Every kept page is written under the project's `research/web/`, visible in the project, and is **not** indexed into the shared knowledge base, whatever its class. It reaches home scope only through promotion (§7, `/retro`), which the operator accepts. The project's research is readable from home scope by path, as any project file is.
3. **Source-status label.** Each entry's frontmatter gains `source_class` (`primary`, `reputable_secondary`, `weak`), written by code from the same class. A pending or project entry is at the same tier as any other research entry; nothing here raises trust (`specs/001` §5).
4. **One list.** The research job still reads only hosts on its own list (`context/sources/trusted.toml`, the operator's `trusted-sources.toml`); the validator's classes decide only where a page goes. Hosts the research list reads but the validator does not trust (today `upload.wikimedia.org`, `commons.wikimedia.org`) are held, not shared; a test fails if the sets drift.
5. **Not built:** the console passing a project name (slice 6), listing or promoting pending entries (slice 4), the digest (slice 5), and routing of agent-run searches outside the expand job.

## 14. Amendment 058-A2: reviewing held research, `/validate` (operator, 2026-10-31)
1. **What is held.** The default scope's `local/pending/web/` and a registered project's `research/web/` (§13). Neither is indexed.
2. **Console commands.** `/validate` lists them; `/validate ratify <n…>` and `/validate discard <n…>` decide; `project=<name>` selects a project's own research. `/retro research …` is the same command. The daemon methods are `pending.list`, `pending.ratify` and `pending.discard`: console scope only, and absent from the relay.
3. **Ratify** puts the entry (and the pictures it names) under `knowledge/web/`, where the index reads it at the research tier, so it is searchable as **noted, not validated**; its `source_class` still says what the source was. A held page is moved; a project's entry is copied and the project keeps its own. It is the operator's act and nothing raises a tier (`specs/001` §5).
4. **Discard** deletes a held page from the workspace's pending folder. It never deletes a project's files.
5. **Checks by code.** An entry is named `host/slug.md` exactly (validated host, lowercase letters, digits and dashes); a link is never followed; a file the job did not write (no `source_url`) is refused.
6. **Not built:** the automatic validator pass over held entries (the promotion pass of `specs/040`, 040-A2), and a bulk `retro promote` of project research; ratification is per entry.

## 15. Amendment 058-A3: `/projects`, switching project (operator, 2026-10-31)
1. **The picker.** `/projects` in the console lists the home workspace (`default`, always first) and every registered project (§2), in an arrow-key menu; Enter selects. `/projects <name>` selects by name (the directory name, lowercased, spaces as hyphens).
2. **Switching is a restart.** The console writes the chosen directory to the file `delphi chat` named for it and exits; `delphi chat` changes its working directory there and starts the console again with a new session. The scope of the new session is then resolved from that directory as in §3.2; nothing else changes and nothing is carried over.
3. **Checks by code.** `delphi chat` follows the request only when the directory is the home workspace or the root of a registered project; anything else ends the console as before. The request file is removed after it is read.
4. **Not built:** the list is read when the console starts (a project registered later shows after a restart); the console does not yet name its project to `research.start`.
