---
id: council-002-round1-strategist
title: "Council 002 round1 strategist (verbatim)"
author: strategist
timestamp: 2026-10-23T19:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-002, verbatim]
model: "gemini-4-argon-medium via agy"
---

> Verbatim seat output. Line numbers refer to the files as they stood when this round ran (commit `6bb4cb5` plus uncommitted edits).

# Council 002 — Round 1 (Blind): `strategist` Review of `specs/001` & `specs/006` for Step 1

## 1. Findings

- **[P0]** `specs/006-installer-and-updater.md#L44-L48` & `specs/006-installer-and-updater.md#L126-L135` — `delphi doctor` Checks 6–7 and `delphi init` wizard make Step 1's Definition of Done impossible to pass
  - **Failure mode:** Step 1 DoD (`missions/2026-10-23-foundational-design/roadmap.md#L51`) requires "`doctor` exits `0` on a fresh init", and `specs/006-installer-and-updater.md#L34` requires `delphi init` to work "100% offline with zero network calls". However:
    1. `doctor` Check 6 (`specs/006-installer-and-updater.md#L132`) requires verifying `.delphi/state.db` WAL integrity, `fts5`, and `sqlite-vec` tables. Neither `state.db` (Step 2, `roadmap.md#L52`) nor FTS5 (Step 5, `roadmap.md#L55`) is created by `delphi init`, and `sqlite-vec` is deferred past v1 by ratified D-01 (`missions/2026-10-23-foundational-design/decisions.md#L93`). Check 6 will deterministically fail on a fresh `init`.
    2. `doctor` Check 7 (`specs/006-installer-and-updater.md#L133`) verifies Claude/Gemini credentials and CLI harnesses, and `delphi init` (`#L44-L48`) is specified only as an interactive wizard that prompts for API keys and "runs an initial capability verification pass" (a network call). In headless CI or an offline Step 1 environment without provider keys, `init` blocks on stdin or fails network verification, and `doctor` exits `1`.
  - **Remediation:**
    1. In `specs/006-installer-and-updater.md#L44-L48`, add `delphi init --non-interactive [--workspace <name>] [--project <path>]` (skipping key prompts and network capability checks); make the capability check optional/skippable when offline.
    2. In `specs/006-installer-and-updater.md#L132` (Check 6), specify that if `state.db` does not exist yet (pre-Step 2 / fresh init), Check 6 reports `not initialized (skipped)` and exits `0`; remove `sqlite-vec` from v1 Check 6 per D-01 (check `sqlite-vec` only when `config.toml` `[rag] engine = "sqlite-hybrid"`).
    3. In `specs/006-installer-and-updater.md#L133` (Check 7), report missing API keys as `warning (no credentials configured)` on `doctor` unless `--require-providers` is passed (or gate hard failure to Step 2+ daemon launch).

- **[P0]** `specs/001-governance-and-manifests.md#L163-L178` & `specs/001-governance-and-manifests.md#L234` — Step 1 validator and `doctor` checks will fail on this repository's own files (blocking Step 1 DoD)
  - **Failure mode:** Step 1 DoD (`missions/2026-10-23-foundational-design/roadmap.md#L51`) requires that "this repo passes the validator, link and absolute-path checks". Running the `specs/001` validator and `specs/006` Check 5 against commit `6bb4cb5` will fail on four concrete mismatches:
    1. **Non-canonical role names (`[REV-ARCH-05]`, `specs/001-governance-and-manifests.md#L169`, `#L207`, `#L234`):** `context/index.md#L4` has `author: mnemosyne` (not a canonical role ID); `themes/greek/research.md#L4` has `author: argos` (theme name, not `scout`); and specs still contain mythological identifiers outside `themes/`: `specs/001-governance-and-manifests.md#L114` (`sp_athena_01`), `specs/003-collaboration-and-council.md#L107` (`Select 3-5 Gods`), `specs/005-console-and-narrator-tui.md#L36-L37` & `#L79-L80` (`CHAOS`, `HOMER`, `sp_ares_r2`, `sp_athena_council_01`), and `specs/007-federated-mesh-and-sync.md#L67` (`ask-argos-cartography`).
    2. **Frontmatter schema vs repo files (`specs/001-governance-and-manifests.md#L163-L178`):** §5 lists 11 fields (`id`, `title`, `author`, `timestamp`, `source_type`, `confidence`, `status`, `superseded_by`, `source_mission`, `ttl_missions`, `tags`) without marking which are optional. Zero files in the repo have `superseded_by` or `ttl_missions`. Even for core fields, all 9 `memory/agents/*/lessons.md#L1-L11`, `context/index.md#L1-L9`, and `context/heuristics.md#L1-L13` omit `source_mission` and `tags`; `missions/2026-10-23-foundational-design/mission.md#L1-L10` and `roadmap.md#L1-L10` omit `source_mission` (and `roadmap.md` omits `tags`); `skills/CATALOG.md#L1-L7` omits `source_type`, `confidence`, `source_mission`, and `tags`.
    3. **Missing `skills/*/SKILL.md` files (`specs/001-governance-and-manifests.md#L67-L103`, `#L234`):** Test 1 (`#L234`) validates "all `skills/*/SKILL.md` files" and `agents/*/AGENT.md` declare `authorized_skills`, but zero `skills/<name>/SKILL.md` files exist in the repo (only `skills/CATALOG.md`).
    4. **Illustrative pointers in fenced blocks (`specs/006-installer-and-updater.md#L131`):** `doctor` Check 5 validates "all `path#Lstart-Lend` pointers and Markdown links across `context/`, `specs/`, `SPEC.md`, `GOVERNANCE.md`, and `REVIEW.md`". Unless fenced code blocks are excluded, fictional example URIs in `specs/001-governance-and-manifests.md#L127-L135` (`missions/2026-10-23-auth-refactor/...`), `specs/004-memory-and-coaching.md#L90`, and `specs/007-federated-mesh-and-sync.md#L48,L87` fail Check 5.
  - **Remediation:**
    1. In `specs/001-governance-and-manifests.md#L165-L178`, mark `superseded_by` (required only when `status == superseded`), `ttl_missions`, `source_mission`, and `tags` as optional (or add `source_mission` and `tags` to `context/index.md`, `context/heuristics.md`, `memory/agents/*/lessons.md`, `mission.md`, `roadmap.md`, and `skills/CATALOG.md`).
    2. Fix `author: mnemosyne` $\rightarrow$ `orchestrator` in `context/index.md#L4`, `author: argos` $\rightarrow$ `scout` in `themes/greek/research.md#L4`, and replace the remaining mythological tokens in `specs/001#L114`, `specs/003#L107`, `specs/005#L36-L37,L79-L80`, and `specs/007#L67` (and explicitly exempt `missions/**/councils/**` historical records per D-17).
    3. In `specs/001-governance-and-manifests.md#L49` & `#L234`, specify that in Step 1 `authorized_skills` validates against skill IDs declared in `skills/CATALOG.md` when `skills/<name>/SKILL.md` is not yet present.
    4. In `specs/006-installer-and-updater.md#L131`, specify that Check 5 parses Markdown AST links and inline pointers outside fenced code blocks (` ``` `).

- **[P1]** `specs/001-governance-and-manifests.md#L18-L23` & `specs/006-installer-and-updater.md#L18-L22` — Later-phase subsystems in `001` and `006` lack phase tags, and `delphi init` includes deferred Step 7 federation keygen (`[REV-ARCH-01]` scope audit)
  - **Failure mode:** Unlike `specs/002`, `004`, `005`, and `007`, neither `specs/001` nor `specs/006` demarcates which sections ship in Step 1 vs Steps 2–7. Under `GOVERNANCE.md#L24-L25` (*No Spec, No Build* and *Drift Remediation*), approving `001` and `006` as written forces the Step 1 implementer either to build later-step subsystems prematurely or to halt for spec drift:
    1. `specs/006-installer-and-updater.md#L46` requires `delphi init` to generate an "Ed25519 federation identity keypair (stored in macOS Keychain / Linux Secret Service; public DID written to `.delphi/identity.json`)". Federation (Spec 007 Planes 1–2) is deferred to Step 7 (`decisions.md#L55`, `roadmap.md#L57`). Building OS Keychain / Secret Service integration and DID keygen in Step 1 is unrequested scope creep that breaks headless Linux/CI `init`.
    2. `specs/001-governance-and-manifests.md#L183-L201` (§5.1–5.2 runtime span taint accounting, `(worktree, path)` taint map, provenance write gate, taint-gated tool exposure) and `#L235-L238,L241` (Tests 2–5, 8) require the Step 2 daemon (`roadmap.md#L52`).
    3. `specs/001-governance-and-manifests.md#L227-L231` (§6.5 `delphi theme new <name>` and `/theme review`) dispatches `scout`, `architect`, and `red-team` and requires the console TUI, plus `scout` is excluded from the v1 roster by D-07 (`decisions.md#L123`). Test 10 (`#L243`) requires mission replay (Step 2+).
    4. `specs/006-installer-and-updater.md#L38-L42` (§2.2 bootstrap/Homebrew), `#L107-L121` (§4 `delphi update` + SQLite migration), and `#L138-L143` (§6 Tests 1–5) belong to Step 5 (`.upstream` update + packaging, `roadmap.md#L55`) and Step 6 (diff3 merge + `sync`, `roadmap.md#L56`).
  - **Remediation:** Add a `### 1.3 Phased Implementation Mapping` table to both `specs/001` and `specs/006`:
    - **Step 1 (`001`):** §2, §3.1, §4, §5 frontmatter/envelope schemas (static validation), §2.2 logical path guard, §6.1–6.4 static theme schema + voice linter + `ratified.lock` check, §7 Tests 1, 6, 7, 9.
    - **Step 2+ (`001`):** §5.1–5.2 runtime taint tracking/gating + §7 Tests 2–5, 8 (Step 2); §6.5 `delphi theme new` + Test 10 (post-Step 4 / post-v1 when `scout` is enabled).
    - **Step 1 (`006`):** §2.1 embedded seed (`include_dir!`, `core-manifest.json`, `base/`, `manifest.lock`), §2.2.3 `delphi init` (without federation DID keygen), §3.1 workspace layout + `ratified.lock`, §5 `doctor` checks 1–5 & 8.
    - **Step 5/6/7 (`006`):** §2.2.1–2.2.2 packaging & §4 `delphi update` `.upstream` mode + §6 Tests 1, 3, 4, 5 (Step 5); §4 in-memory diff3 + §6 Test 2 + `delphi sync` (Step 6); §2.2.3 Ed25519 federation DID keypair (Step 7).

- **[P1]** `specs/001-governance-and-manifests.md#L37,L58-L60,L99-L103,L234` & `specs/006-installer-and-updater.md#L35,L84-L100` — Unresolved path-guard and layout ambiguities between `001` and `006`, plus leftover Python helper references contradicting D-02
  - **Failure mode:** An implementer building Step 1's path guard (`specs/001` §2.2) and `delphi init` (`specs/006` §3.1) faces four conflicting or incomplete specifications:
    1. **Unmapped logical globs (`specs/001-governance-and-manifests.md#L58` vs `agents/builder/AGENT.md#L8`):** `specs/001#L58` requires the daemon to map every logical glob "to exactly one zone root", listing `missions/**`, `memory/**` $\rightarrow$ `local/`; `context/**`, `specs/**`, `REVIEW.md` $\rightarrow$ `team/`; `@worktree/**` $\rightarrow$ worktree. It omits `skills/custom/**` (used in `agents/builder/AGENT.md#L8`), `agents/custom/**` (`specs/006#L77`), and Zone 1 read paths (`GOVERNANCE.md`, `SPEC.md`, `agents/<role>/AGENT.md`, `skills/<builtin>/**`). And `specs/001#L37` still says `.delphi/agents/custom/<id>/AGENT.md` instead of `team/agents/custom/<id>/AGENT.md`.
    2. **Binary-embedded Zone 1 reads vs `openat` (`specs/006-installer-and-updater.md#L35,L99` vs `specs/001-governance-and-manifests.md#L58`):** `specs/006#L99` says unmodified Zone 1 files do not exist in `core/` and "are read straight from the binary", while `#L35` says `delphi init` writes all Zone 1 files to `base/`. `specs/001#L58` requires resolving agent file reads on disk via `openat` + `O_NOFOLLOW` inside a zone root. It is unspecified whether agent file-read tools resolve unmodified Zone 1 paths against `base/` on disk (with `core/` taking precedence when customized).
    3. **Missing `manifest.lock` path (`specs/006-installer-and-updater.md#L35,L84-L96`):** `#L35` and `#L128` require `manifest.lock` for `doctor` Check 2, and `#L100` requires `local/ratified/` to store copies of ratified constitutional files, but neither `manifest.lock` nor `local/ratified/` appears in the §3.1 directory tree (`#L84-L96`).
    4. **D-02 contradiction (`specs/001-governance-and-manifests.md#L99-L103,L234`):** D-02 (`decisions.md#L97-L103`) ratified compiling all built-in deterministic helpers into `delphi-core` and restricting scripts to `skills/custom/*/scripts/`. Yet `specs/001#L234` still names `lint_manifests.py` and `#L99-L103` still specifies `scripts/run_tdd_gate.py` on built-in skills.
  - **Remediation:**
    1. In `specs/001-governance-and-manifests.md#L58` and `specs/006-installer-and-updater.md#L98`, state the complete logical-to-zone mapping: `missions/**`, `memory/**` $\rightarrow$ `local/`; `context/**`, `specs/**`, `REVIEW.md`, `agents/custom/**`, `skills/custom/**` $\rightarrow$ `team/`; `GOVERNANCE.md`, `SPEC.md`, `agents/<builtin>/**`, `skills/<builtin>/**` $\rightarrow$ `core/` if overridden, else `base/` (for read-only `openat` resolution); `@worktree/**` $\rightarrow$ `worktrees/<task-id>/`. Fix `#L37` to `team/agents/custom/<id>/AGENT.md`.
    2. In `specs/006-installer-and-updater.md#L88-L95`, add `base/manifest.lock` (or `core-manifest.json`) and `local/ratified/` to the physical layout tree.
    3. In `specs/001-governance-and-manifests.md#L99-L103` & `#L234`, note per D-02 that built-in deterministic checks (including `lint_manifests`) are compiled into `delphi` / `delphi-core`, while `deterministic_scripts` in `SKILL.md` applies to `skills/custom/*`.

- **[P1]** `specs/006-installer-and-updater.md#L124-L143` — `specs/006` §6 has zero verification tests for Step 1 (`init`, `doctor`, `ratified.lock`, source-repo lint)
  - **Failure mode:** All 5 tests in `specs/006-installer-and-updater.md#L138-L143` test `delphi update` (Step 5/6). None test Step 1 deliverables (`delphi init`, `delphi doctor`, `ratified.lock` tamper detection, clean registered project repo, or running `doctor`/validator against this source repository where no `$DELPHI_HOME/workspaces/<name>/{base,team,local}` wrapper exists).
  - **Remediation:** Add Step 1 verification tests to `specs/006-installer-and-updater.md#L138-L143`:
    - **Test 6 (Fresh Offline Init & Clean Project Repo):** Run `delphi init --non-interactive --project <repo>` with network disabled; verify `base/`, `team/`, `local/ratified.lock`, `local/ratified/`, and `config.toml` are created, `delphi doctor` exits `0`, and `<repo>` has zero untracked or modified files.
    - **Test 7 (Constitution Drift Detection in `doctor`):** Mutate `team/REVIEW.md` or `core/GOVERNANCE.md` without updating `local/ratified.lock`; verify `delphi doctor` Check 8 exits `1`.
    - **Test 8 (Source-Repo Lint Mode):** Specify `delphi doctor --repo <path>` (or `delphi lint`) to run schema, canonical-role, taint-tier, Rule-of-25, link, and absolute-path checks against the flat source repository layout, exiting `0` on this repo.

- **[P2]** `specs/006-installer-and-updater.md#L130` — `doctor` Check 4 omits the 350-character per-rule cap and rule-counting grammar from D-09 / `GOVERNANCE.md` §6.2
  - **Failure mode:** `GOVERNANCE.md#L92`, `decisions.md#L159-L164` (D-09), and `specs/004-memory-and-coaching.md#L71` enforce three caps on `context/heuristics.md` and `memory/agents/*/lessons.md`: $\le 25$ active rules, $\le 6,144$ bytes total, and $\le 350$ characters per rule body (`T`/`D`/`F` lines). `specs/006-installer-and-updater.md#L130` checks only $\le 25$ rules and $\le 6,144$ bytes, omitting the 350-char per-rule cap and how rule boundaries are parsed.
  - **Remediation:** Update `specs/006-installer-and-updater.md#L130` to specify that each `### ` section in `heuristics.md` and `lessons.md` counts as one rule, must contain `- **T:**`, `- **D:**`, and `- **F:**` lines totaling $\le 350$ Unicode scalar values (chars), with $\le 25$ rules and $\le 6,144$ bytes total per file, matching `active_rules` in frontmatter.

---

## 2. Things Checked and Found Sound

1. **Logical-first path guard (`specs/001-governance-and-manifests.md#L57-L62`):** Matching logical globs first and opening zone roots with `openat` + `O_NOFOLLOW` + case-insensitive rejection cleanly prevents symlink/hardlink/traversal escapes (`Tests 6–7`).
2. **Agent manifests compliance (`agents/*/AGENT.md`):** All 9 manifests are under 50 lines (well below the 500-line cap), include `max_turn_budget`, use canonical role IDs, and enforce least-privilege write globs matching `[REV-SEC-02]`.
3. **Seeded heuristics compliance (`context/heuristics.md#L1-L52`):** All 7 rules follow the `T`/`D`/`F` format, are $\le 241$ chars each ($\le 350$ limit), total ~2.5 KB ($\le 6,144$ bytes), and carry `source_type: user_ratified` ($L_5$).
4. **Default Greek theme pack (`themes/greek/theme.md#L1-L51`):** Maps all 9 canonical roles and 3 labels, keeps all `persona` strings $\le 108$ chars ($\le 400$ limit) with zero directive/rule-override language, and carries `source_type: user_ratified`.
5. **External workspace isolation (`specs/006-installer-and-updater.md#L80-L103`):** Placing all 3 zones and worktrees under `$DELPHI_HOME/workspaces/<name>/` satisfies D-03 and keeps registered project repos completely clean.
6. **Constitutional pinning via `ratified.lock` (`specs/006-installer-and-updater.md#L100`):** Pinning `blake3` hashes of `GOVERNANCE.md`, `REVIEW.md`, and `context/heuristics.md` in `local/ratified.lock` with fallback copies in `local/ratified/` closes raw `git pull` or local edit bypasses.
7. **Daemon-less Step 1 `doctor` check 1 (`specs/006-installer-and-updater.md#L127`):** Skipping Check 1 with `not running` when no UDS socket is active aligns with D-05 (daemon arrives in Step 2).

---

## 3. Recommendation for Each Spec

- **`specs/001-governance-and-manifests.md`:** `approve-with-fixes` — must land **Finding 2** (frontmatter optionality/backfill, canonical role fixes, `authorized_skills` catalog check), **Finding 3** (§1.3 Step 1 vs Step 2+ phase table), and **Finding 4** (complete logical-to-zone mapping + D-02 compiled-in linter note).
- **`specs/006-installer-and-updater.md`:** `approve-with-fixes` — must land **Finding 1** (`init --non-interactive` + `doctor` Checks 6–7 pre-Step-2/D-01 behavior), **Finding 2** (Check 5 fenced-block exclusion), **Finding 3** (§1.3 Step 1 vs Step 5/6/7 phase table + deferring federation DID keygen from Step 1 `init`), **Finding 4** (`manifest.lock` and `local/ratified/` in §3.1 tree), **Finding 5** (Step 1 verification tests + source-repo lint mode), and **Finding 6** (350-char per-rule check in Check 4).

## 4. Concurrence

`concur: false`
