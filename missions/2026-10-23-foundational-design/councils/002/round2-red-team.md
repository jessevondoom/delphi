---
id: council-002-round2-red-team
title: "Council 002 round2 red team (verbatim)"
author: red-team
timestamp: 2026-10-23T19:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-002, verbatim]
model: "gemini-4-argon-high via agy"
---

> Verbatim seat output. Line numbers refer to the files as they stood when this round ran (commit `6bb4cb5` plus uncommitted edits).

### 1. Cross-Examination of Peer Reviews

#### Builder Seat (B-1 … B-13)
- **B-1 `[P0]` ([`001#L184-L191`](../../../../specs/001-governance-and-manifests.md#L184-L191), [`006#L100`](../../../../specs/006-installer-and-updater.md#L100)):** `agree` — unindexed Markdown arriving via raw `git pull` or local edits in `team/` can forge `source_type: user_ratified` ($L_5$) unless `L_4`/`L_5` trust is verified against a daemon provenance index or `ratified.lock`.
- **B-2 `[P0]` ([`006#L77`](../../../../specs/006-installer-and-updater.md#L77), [`#L100`](../../../../specs/006-installer-and-updater.md#L100)):** `agree` — unpinned `AGENT.md`, `SKILL.md`, and `skills/custom/**/scripts/**` allow `git pull` or local edits to override system prompts and widen permissions without ratification (matches Red-Team **F2**).
- **B-3 `[P1]` ([`roadmap.md#L51`](../../../../missions/2026-10-23-foundational-design/roadmap.md#L51), [`001#L163-L178`](../../../../specs/001-governance-and-manifests.md#L163-L178)):** `adjust severity to P0` — Step 1 DoD requires this repository to pass the validator, so missing frontmatter fields and non-canonical authors (`mnemosyne`, `argos`) directly block Step 1 (matches **F5**, **S-2**).
- **B-4 `[P1]` ([`001#L49-L51`](../../../../specs/001-governance-and-manifests.md#L49-L51), [`#L234`](../../../../specs/001-governance-and-manifests.md#L234)):** `agree` — zero `skills/*/SKILL.md` packages exist yet, so `authorized_skills` must validate against [`skills/CATALOG.md`](../../../../skills/CATALOG.md) in Step 1 and cross-check `authorized_agents` when packages exist.
- **B-5 `[P1]` ([`006#L44-L48`](../../../../specs/006-installer-and-updater.md#L44-L48), [`#L80-L103`](../../../../specs/006-installer-and-updater.md#L80-L103)):** `agree` — `delphi doctor` cannot locate `$DELPHI_HOME/workspaces/<name>/` from a project directory unless `cwd`-to-`config.toml` `projects` matching and `--workspace` are specified.
- **B-6 `[P1]` ([`006#L124-L134`](../../../../specs/006-installer-and-updater.md#L124-L134)):** `adjust severity to P0` — "`doctor` exits `0` on a fresh init" is a hard Step 1 DoD gate ([`roadmap.md#L51`](../../../../missions/2026-10-23-foundational-design/roadmap.md#L51)), which Checks 6 and 7 deterministically fail offline or pre-Step 2 (matches **F5.3**, **S-1**).
- **B-7 `[P1]` ([`006#L100`](../../../../specs/006-installer-and-updater.md#L100)):** `agree` — `specs/006` must state that `delphi init` bootstraps `local/ratified.lock` and `local/ratified/` from the signed `core-manifest.json`.
- **B-8 `[P1]` ([`006#L110-L113`](../../../../specs/006-installer-and-updater.md#L110-L113)):** `agree` — `delphi rollback` must restore `state.db.bak-<version>` and `base/` (refusing if newer-schema events exist), scoped to Step 5.
- **B-9 `[P2]` ([`001#L37`](../../../../specs/001-governance-and-manifests.md#L37) vs [`006#L77`](../../../../specs/006-installer-and-updater.md#L77)):** `adjust severity to P1` — `.delphi/agents/custom/` vs `team/agents/custom/<id>/` is part of the Step 1 logical-to-zone mapping ambiguity (**F2**, **S-4**).
- **B-10 `[P2]` ([`001#L235`](../../../../specs/001-governance-and-manifests.md#L235)):** `agree` — Test 2 still requires `file://` pointers instead of workspace-relative `path#Lx-Ly` pointers (**F5.4**).
- **B-11 `[P2]` ([`001#L58`](../../../../specs/001-governance-and-manifests.md#L58)):** `agree` — NFC-normalizing path components before glob/denylist comparison prevents APFS NFD/NFC normalization bypasses.
- **B-12 `[P2]` ([`006#L46`](../../../../specs/006-installer-and-updater.md#L46)):** `adjust severity to P1` — generating an Ed25519 federation keypair during Step 1 `init` violates D-06 (`[REV-ARCH-01]`) and breaks headless CI (**S-3.1**).
- **B-13 `[P2]` ([`006#L19`](../../../../specs/006-installer-and-updater.md#L19)):** `agree` — §1 item 1 omits `templates/`, `themes/`, seed `context/`, and `memory/` listed in [`006#L103`](../../../../specs/006-installer-and-updater.md#L103).

#### Strategist Seat (S-1 … S-6)
- **S-1 `[P0]` ([`006#L44-L48`](../../../../specs/006-installer-and-updater.md#L44-L48), [`#L126-L135`](../../../../specs/006-installer-and-updater.md#L126-L135)):** `agree` — interactive-only `init` and hard-failing Checks 6–7 make Step 1's offline/CI DoD impossible to pass (**F5.3**, **B-6**).
- **S-2 `[P0]` ([`001#L163-L178`](../../../../specs/001-governance-and-manifests.md#L163-L178), [`#L234`](../../../../specs/001-governance-and-manifests.md#L234), [`006#L131`](../../../../specs/006-installer-and-updater.md#L131)):** `agree` — verified against current tree (operator's edit fixed [`003#L107`](../../../../specs/003-collaboration-and-council.md#L107) and `005` headers, but [`context/index.md#L4`](../../../../context/index.md#L4), [`themes/greek/research.md#L4`](../../../../themes/greek/research.md#L4), [`001#L114`](../../../../specs/001-governance-and-manifests.md#L114), [`005#L79-L80`](../../../../specs/005-console-and-narrator-tui.md#L79-L80), [`007#L67`](../../../../specs/007-federated-mesh-and-sync.md#L67), frontmatter gaps, and fenced-block pointers still fail).
- **S-3 `[P1]` ([`001#L18-L23`](../../../../specs/001-governance-and-manifests.md#L18-L23), [`006#L18-L22`](../../../../specs/006-installer-and-updater.md#L18-L22)):** `agree` — both specs need a §1.3 phased implementation table demarcating Step 1 from Steps 2–7 under [`GOVERNANCE.md#L24-L25`](../../../../GOVERNANCE.md#L24-L25).
- **S-4 `[P1]` ([`001#L37,L58-L60,L99-L103,L234`](../../../../specs/001-governance-and-manifests.md#L58), [`006#L35,L84-L100`](../../../../specs/006-installer-and-updater.md#L84-L100)):** `agree` — logical-to-zone mappings omit `agents/custom/**`, `skills/custom/**`, and Zone 1 read resolution (`core/` overriding `base/`), §3.1 omits `base/manifest.lock` and `local/ratified/`, and `.py` helpers contradict D-02.
- **S-5 `[P1]` ([`006#L124-L143`](../../../../specs/006-installer-and-updater.md#L124-L143)):** `agree` — `specs/006` §6 tests only `delphi update` (Step 5/6) and lacks Step 1 tests for `init`, `doctor`, `ratified.lock`, and source-repo linting.
- **S-6 `[P2]` ([`006#L130`](../../../../specs/006-installer-and-updater.md#L130)):** `agree` — Check 4 omits the ratified D-09 $\le 350$-char per-rule cap and `### ` / `T`/`D`/`F` rule grammar.

---

### 2. Explicit Conflict Resolutions

1. **`openat` + `O_NOFOLLOW` vs. Hard Links (`Strategist` Checked Sound #1 vs. `Red-Team` F3.1 on [`specs/001#L58`](../../../../specs/001-governance-and-manifests.md#L58)):**
   - **Red-Team F3.1 is correct; Strategist is wrong.** In POSIX (`openat(2)` on macOS/Linux), `O_NOFOLLOW` rejects only symbolic links (`S_IFLNK`). A hard link to `local/missions/<id>/events.jsonl` or `local/ratified.lock` inside the same `$DELPHI_HOME` filesystem is a regular file (`S_IFREG`, `st_nlink > 1`) and bypasses `O_NOFOLLOW`. The path guard must reject `fstat` `st_nlink > 1` on agent file reads/writes (and the sandbox must block `link`/`linkat` onto protected paths).
2. **`source_mission` and `tags` Required (`Builder` B-3 / `Red-Team` F5.2) vs. Optional (`Strategist` S-2.1):**
   - **B-3 and F5.2 are correct for `source_mission` (keep required); `superseded_by` and `ttl_missions` are optional.** Tier 1 [`GOVERNANCE.md#L69`](../../../../GOVERNANCE.md#L69) and [`REVIEW.md#L42`](../../../../REVIEW.md#L42) (`[REV-TRAP-02]`) mandate `source_mission` on every entry. Keep `id`, `title`, `author`, `timestamp`, `source_type`, `confidence`, `status`, `source_mission`, and `tags` required in [`specs/001#L165-L178`](../../../../specs/001-governance-and-manifests.md#L165-L178), mark `superseded_by` and `ttl_missions` optional, and backfill the repo files.
3. **Custom Agent Override Path (`Builder` B-2/B-9 `team/agents/custom/` vs. `Red-Team` F2 `core/agents/`):**
   - **Resolution:** Resolve [`specs/006#L76-L77,L99`](../../../../specs/006-installer-and-updater.md#L76-L77) by defining precedence `core/agents/<id>/` (operator customization) > `team/agents/custom/<id>/` (team override) > `base/agents/<id>/` (built-in), while requiring **all** `AGENT.md`, `SKILL.md`, and `skills/custom/**/scripts/**` hashes in `local/ratified.lock` and forbidding any override from widening a built-in role's `default_access_scope` ceiling (`[REV-SEC-02]`).
4. **Spec Fixes Now vs. Runtime Implementation in Steps 2–5 (`Red-Team` F1/F4/F6 & `Builder` B-1 vs. `Strategist` S-3):**
   - **Resolution:** Under *No Spec, No Build* ([`GOVERNANCE.md#L24`](../../../../GOVERNANCE.md#L24)), spec text fixes for **B-1, F1, F4, F6** must land in `001` and `006` before approval so we do not ratify broken security invariants, while their runtime code and tests are scheduled for Steps 2–5 in the new §1.3 phase table (**S-3**).

---

### 3. Step 1 Blockers Missed by All Three Seats

1. **M1 (`[P0]`, [`specs/006#L103,L131`](../../../../specs/006-installer-and-updater.md#L103)) — `doctor` Check 5 fails on a fresh `delphi init` because seeded files link across zones and to unseeded files:** [`006#L103`](../../../../specs/006-installer-and-updater.md#L103) states `missions/` and `specs/` (plus `OVERVIEW.md`, `README.md`) are not seeded into `$DELPHI_HOME/workspaces/<name>/`, and splits `GOVERNANCE.md`/`SPEC.md` (`base/`) from `REVIEW.md`/`context/` (`team/`). Yet seeded [`GOVERNANCE.md#L69`](../../../../GOVERNANCE.md#L69), [`SPEC.md#L2,L128,L136-L143`](../../../../SPEC.md#L136-L143), [`context/index.md#L18,L28`](../../../../context/index.md#L18), and [`memory/agents/orchestrator/lessons.md#L14`](../../../../memory/agents/orchestrator/lessons.md#L14) link to `specs/001..008`, `missions/.../decisions.md`, `OVERVIEW.md`, `README.md`, and flat relative paths (`../REVIEW.md`). Check 5 must resolve links across the logical workspace overlay and skip links to unseeded design records in workspace mode (or seed `specs/` into `base/`).
2. **M2 (`[P0]`, [`specs/006#L129`](../../../../specs/006-installer-and-updater.md#L129) vs [`REVIEW.md#L1-L5`](../../../../REVIEW.md#L1-L5), [`specs/001#L82-L103`](../../../../specs/001-governance-and-manifests.md#L82-L103)) — `doctor` Check 3 (Taint Audit) has no `source_type` field on `REVIEW.md` or `skills/`:** Check 3 verifies no entry below $L_4$ exists in `REVIEW.md` or `skills/`, but `REVIEW.md` has no YAML frontmatter and `SKILL.md` (§3.1) omits `source_type`. Add `source_type` frontmatter to `REVIEW.md`, `skills/CATALOG.md`, and `specs/001` §3.1.
3. **M3 (`[P1]`, [`specs/006#L131`](../../../../specs/006-installer-and-updater.md#L131) vs [`REVIEW.md#L23`](../../../../REVIEW.md#L23)) — Check 5's `/Users/` and `/home/` scan self-triggers on `REVIEW.md#L23` and `006#L131`:** Both files contain literal `/Users/...` or `/Users/…` when stating `[REV-SEC-05]`. Specify the pattern as `/(Users|home)/[A-Za-z0-9._-]+` (requiring a username segment).

---

### 4. Final Deduplicated MUST-FIX List & Explicit Deferrals

#### Must Land Before Approving `specs/001` and `specs/006`
1. **Path Guard & Isolation (`F1`, `F3`, `B-9`, `B-11`, `S-4.1`):** Deny `.git` and `**/.git` (file or dir); reject `fstat` `st_nlink > 1`; block agent reads of `local/state.db*`, peer `memory/agents/<other != self>/**`, and Council Round 1 peer outputs; unify proposals to `local/proposals/<mission-id>/`; define the complete logical-to-zone mapping (`core/` > `base/`, `team/`, `local/`, `@worktree`); add NFC normalization.
2. **Manifest/Script Pinning & Bootstrap (`F2`, `B-2`, `B-7`, `S-4.3`):** Pin all `AGENT.md`, `SKILL.md`, and `skills/custom/**/scripts/**` in `local/ratified.lock` (seeded from `core-manifest.json` on `init`), include `base/manifest.lock` and `local/ratified/` in `006` §3.1, and forbid overrides from widening built-in `default_access_scope` ceilings.
3. **Taint Accounting & Provenance Index (`F4`, `B-1`, `M2`):** Fix $L_3 \rightarrow L_4$ promotion (`ext_ceiling = min(inputs <= L2)`); attribute sandboxed worktree file mutations and non-daemon Markdown edits (`team/` $\rightarrow L_2$, local $\rightarrow L_3$, unknown $\rightarrow L_0$) via a persisted `(path, blake3) -> taint` index; add `source_type` to `REVIEW.md`, `skills/CATALOG.md`, and `001` §3.1.
4. **Step 1 Validator, Repo Backfill & `doctor`/`init` DoD (`F5`, `B-3`, `B-4`, `B-5`, `B-6`, `B-10`, `B-13`, `S-1`, `S-2`, `S-4.4`, `S-5`, `S-6`, `M1`, `M3`):** Mark `superseded_by`/`ttl_missions` optional; backfill `source_mission`/`tags` on the 15 repo files; fix non-canonical IDs in `context/index.md#L4`, `themes/greek/research.md#L4`, `001#L114`, `005#L79-L80`, `007#L67` (exempting `councils/001-*`); validate `authorized_skills` against `CATALOG.md`; replace `file://` in Test 2; add `init --non-interactive` and `cwd` workspace resolution; fix `doctor` Checks 4–7 (350-char rule cap, logical link overlay + fenced-block exclusion + username regex, skip uninitialized `state.db`/`sqlite-vec`, `warn` on missing keys); add Step 1 tests to `006` §6.
5. **Updater & Theme Hardening (`F6`, `F7`):** In `006` §4, require `new_version > current_version`, drain all `$DELPHI_HOME/run/*.sock` daemons, and ratify `GOVERNANCE.md`/`AGENT.md`/theme diffs before binary swap; in `001` §6, reject XML tags (`<`, `>`) and bidi/tag codepoints and enforce unique, role-disjoint theme names.
6. **Phased Scope Table (`S-3`, `B-12`):** Add §1.3 Phase Mapping tables to `001` and `006`, moving federation DID keygen out of Step 1 `init`.

#### Explicitly Deferred (Implementation Only, Post-Spec Fix)
- **Step 2:** Runtime span taint engine and `001` Tests 2–5, 8 (**S-3**).
- **Step 3:** OS sandbox profile and worktree `--git-dir`/`--work-tree` runner (**F1** runtime).
- **Step 5/6/7:** `delphi update` / `delphi rollback` implementation (**B-8**, **F6** runtime), `delphi theme new` (**S-3**), and federation DID keygen (**B-12**, **S-3**).

---

### 5. Concurrence
`concur_after_fixes: true`
