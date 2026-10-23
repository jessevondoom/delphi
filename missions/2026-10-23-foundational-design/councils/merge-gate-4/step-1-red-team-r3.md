---
id: review-step-1-red-team-r3
title: "Step 1 Merge-Gate-4 Audit, Round 3 (Red-Team, verbatim)"
author: red-team
timestamp: 2026-10-24T02:30:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [review, step-1, merge-gate-4, verbatim]
model: "gemini-4-argon-high via agy"
audited_commit: abba754
---

### Red-Team Implementation Audit — Merge Gate 4 ([`specs/003`](../../../../specs/003-collaboration-and-council.md#L52-L80) §3), Round 3

---

### 1. Status of Round-2 Findings

#### Round-2 Section 2 Findings (`NEW-1` to `NEW-5`)

1. **`NEW-1` (`[P0-BLOCKING]` — Fail-open lockfiles, unratified files in `ratified/`, and silent `config.toml` corruption in `doctor`):** **RESOLVED**
   - `home::read_lock` (`crates/delphi/src/home.rs#L68-L75` @ audited commit) now fails closed (`with_context`) if `"files"` is missing, not a JSON object, or contains any non-string hash value.
   - `doctor::check_ratified` (`crates/delphi/src/doctor.rs#L202-L207` @ audited commit) and `#L237-L242` (`crates/delphi/src/doctor.rs#L237-L242` @ audited commit) verify that all baseline seed entries from `required_workspace_entries` (`crates/delphi/src/doctor.rs#L265-L269` @ audited commit) and `home::operator_required_entries` (`crates/delphi/src/home.rs#L308-L312` @ audited commit) are present in `local/ratified.lock` and `operator/ratified.lock`.
   - `doctor::verify_copies` (`crates/delphi/src/doctor.rs#L273-L296` @ audited commit) walks `local/ratified/` and `operator/ratified/` (`#L275-L281`) and emits `Level::Fail` on any planted file absent from `lock`.
   - `doctor::run` (`crates/delphi/src/doctor.rs#L71-L74` @ audited commit) fails Check 3 (`"3 taint-audit"`) when `config.toml` is missing, unreadable, or invalid TOML.

2. **`NEW-2` (`[P0-BLOCKING]` — Override skill-permission escalation in `doctor::check_overrides` and `lint::attachable_skill`):** **RESOLVED** *(see **R3-1** in §2 for a new regression when `lint::check_frontmatter` runs on `operator/core/` and `operator/skills/`)*
   - `doctor::check_overrides` (`crates/delphi/src/doctor.rs#L300-L320` @ audited commit) now delegates to `lint::manifest_findings` (`crates/delphi/src/lint.rs#L172-L212` @ audited commit) against `lint::builtin_fm` (`crates/delphi/src/lint.rs#L164-L167` @ audited commit), enforcing schema validation, the 500-line body cap, `authorized_agents` back-checks across `skill_package` (`crates/delphi/src/lint.rs#L215-L219` @ audited commit) (`operator/skills/`, `skills/custom/`, `skills/`), and `lint::attachable_skill` (`crates/delphi/src/lint.rs#L227-L246` @ audited commit).
   - `lint::attachable_skill` (`crates/delphi/src/lint.rs#L233-L245` @ audited commit) requires `required_permissions` to be a valid string sequence via `string_seq` (`crates/delphi/src/lint.rs#L250-L252` @ audited commit) (also enforced in `skill_errors` (`crates/delphi/src/lint.rs#L315-L324` @ audited commit)) and checks exact scope grants (`"*"`, `"@worktree/**"`, `"missions/**"`, `"shell-worktree-scoped"`).
   - `doctor::overlay` (`crates/delphi/src/doctor.rs#L23-L24` @ audited commit) snapshots `base/` into `Tree::builtin` (`crates/delphi/src/lint.rs#L29` @ audited commit) before layering `core/`, so `team/agents/custom/<id>/AGENT.md` always compares against `base/`.

3. **`NEW-3` (`[P0-BLOCKING]` — `pathguard` zone-mapping regressions and unratified `skills/custom/` read/write):** **PARTIALLY RESOLVED**
   - **What is fixed:** `is_ratified_kind` (`crates/delphi/src/pathguard.rs#L75-L85` @ audited commit) now includes `skills/**/scripts/**` (`#L82`) and `themes/*/theme.md` (`#L83`), and `Guard::zone_of` (`crates/delphi/src/pathguard.rs#L198-L210` @ audited commit) serves `is_ratified_kind` reads from `self.zones.ratified`, maps `key == "spec.md"` to `self.zones.base` (`#L209-L210`), and maps `memory/agents/` to `self.zones.local` (`#L206-L208`).
   - **What remains broken:** `never_writable` (`crates/delphi/src/pathguard.rs#L127-L129` @ audited commit) checks `!(key.starts_with("skills/custom/") && key.split('/').any(|s| s == "scripts"))`. Because `**` in `glob_match` (`crates/delphi/src/pathguard.rs#L174` @ audited commit) matches zero segments, Builder's built-in write glob `"skills/custom/**/scripts/**"` (`agents/builder/AGENT.md` (`agents/builder/AGENT.md#L8` @ audited commit)) matches `"skills/custom/scripts/SKILL.md"`, `"skills/custom/<s>/scripts/SKILL.md"`, and `"skills/custom/<s>/scripts"` (a file named `scripts`), and all three pass `never_writable` (see **R3-2**).

4. **`NEW-4` (`[P1-MAJOR]` — Wildcard glob bypasses in `write_glob_error`/`read_glob_error` and silent non-string filtering in `seq_strs`):** **PARTIALLY RESOLVED**
   - **What is fixed:** `names_segment` (`crates/delphi/src/lint.rs#L492-L494` @ audited commit) and `logical_glob_error` (`crates/delphi/src/lint.rs#L496-L516` @ audited commit) use `glob_match` to reject wildcard variants of `.git`, `state.db`, `config.toml`, `ratified*`, `provenance*`, `events.jsonl`, `events`, and `.barrier` in both `read_glob_error` (`crates/delphi/src/lint.rs#L519-L524` @ audited commit) and `write_glob_error` (`crates/delphi/src/lint.rs#L528-L544` @ audited commit). `manifest_errors` (`crates/delphi/src/lint.rs#L467-L486` @ audited commit) and `skill_errors` (`crates/delphi/src/lint.rs#L315-L331` @ audited commit) validate string list elements via `string_seq` (`crates/delphi/src/lint.rs#L250-L252` @ audited commit) and check `deterministic_scripts` sequence typing.
   - **What remains broken:** `write_glob_error` (`crates/delphi/src/lint.rs#L556-L561` @ audited commit) checks `if !segs.contains(&"scripts")`, which still permits `skills/custom/scripts/**` (covering `SKILL.md` when the skill is named `scripts`), `skills/custom/scripts/SKILL.md`, and `skills/custom/<s>/scripts` (see **R3-2**).

5. **`NEW-5` (`[P1-MAJOR]` — Incomplete `HandoffEnvelope` pointer check, exact-match `override_widening`, and minor `operator/base` / worktree hygiene gaps):** **RESOLVED**
   - `envelope_errors` (`crates/delphi/src/lint.rs#L394-L418` @ audited commit) requires `context_pointers` to be a list, validates `#L\d+(-L\d+)?` anchors, and requires non-empty `heading`, `blake3`, and `reason`.
   - `override_widening` (`crates/delphi/src/lint.rs#L587-L598` @ audited commit) allows narrowing sub-globs via `within(b, &g)` (`#L589`).
   - `home::init_operator_home` (`crates/delphi/src/home.rs#L298-L303` @ audited commit) writes `operator/base/manifest.lock`, and `doctor::run` (`crates/delphi/src/doctor.rs#L70` @ audited commit) verifies it via `check_manifest_lock(&home.join("operator/base"), "operator/base", &mut r)`.
   - `doctor::check_hygiene` (`crates/delphi/src/doctor.rs#L341-L349` @ audited commit) resolves `gitdir` via `e.path().join(gitdir.trim()).exists()`, handling both relative and absolute paths.

#### Round-2 Section 1 Partial Items Summary
- **R1-1:** Partially resolved (same remaining issue as **NEW-3** / **R3-2**).
- **R1-3:** Resolved (`NEW-2` and `NEW-5`).
- **R1-4:** Resolved (`NEW-1`; see **R3-3** for a new overlay/symlink bypass in `doctor`).
- **R1-5:** Partially resolved (same remaining issue as **NEW-4** / **R3-2**).
- **R1-9:** Resolved (`NEW-5`).

---

### 2. New and Remaining Defects in Round 3

- **[P1-MAJOR]** `crates/delphi/src/lint.rs#L35` (`crates/delphi/src/lint.rs#L35` @ audited commit), `crates/delphi/src/lint.rs#L113-L130` (`crates/delphi/src/lint.rs#L113-L130` @ audited commit), `crates/delphi/src/doctor.rs#L30-L42` (`crates/delphi/src/doctor.rs#L30-L42` @ audited commit) — `[REV-ARCH-01]` (**R3-1: `check_frontmatter` fails valid `operator/core/` overrides and `operator/skills/` packages, and skips `local/heuristics.md`**)
  - **Failure Mode:**
    1. `doctor::overlay` (`crates/delphi/src/doctor.rs#L40-L42` @ audited commit) inserts operator agent overrides and skills into `tree.files` under `"operator/core/agents/<id>/AGENT.md"` and `"operator/skills/<s>/SKILL.md"`. Because `GOVERNED` (`crates/delphi/src/lint.rs#L35` @ audited commit) includes the broad prefix `"operator/"`, `check_frontmatter` (`crates/delphi/src/lint.rs#L113-L130` @ audited commit) runs `frontmatter::check_provenance` (`crates/delphi/src/frontmatter.rs#L73-L115` @ audited commit) (§5 provenance schema) on every `operator/core/agents/<id>/AGENT.md` (§2.1 manifest schema) and `operator/skills/<s>/SKILL.md` (§3.1 skill schema). Any workspace with a valid, ratified operator override or operator skill ([`specs/001` §6A](../../../../specs/001-governance-and-manifests.md#L270-L275), [`specs/006` §3.2](../../../../specs/006-installer-and-updater.md#L121-L137)) unconditionally fails `delphi doctor` with `"3 frontmatter"` errors (`missing required field author`, `timestamp`, `source_type`, `confidence`, `status`, `source_mission`, `tags`).
    2. Conversely, `doctor::overlay` (`crates/delphi/src/doctor.rs#L30-L33` @ audited commit) inserts `local/heuristics.md` at logical key `"heuristics.md"`, which does not match any prefix in `GOVERNED` (`crates/delphi/src/lint.rs#L35` @ audited commit) (`["context/", "missions/", "specs/", "memory/agents/", "themes/", "operator/"]`). Unlike `check_caps` (`crates/delphi/src/lint.rs#L655` @ audited commit) (which explicitly checks `k.as_str() == "heuristics.md"`), `check_frontmatter` (`crates/delphi/src/lint.rs#L115` @ audited commit) skips `"heuristics.md"`, allowing invalid §5 provenance fields in `local/heuristics.md` to pass `delphi doctor`.
    3. Similarly, `"themes/"` in `GOVERNED` (`crates/delphi/src/lint.rs#L35` @ audited commit) forces user theme packs (`$DELPHI_HOME/themes/<name>/theme.md`) to carry §5 provenance fields (`source_mission`, `confidence`, `source_type`, `tags`), whereas [`specs/001` §6.2](../../../../specs/001-governance-and-manifests.md#L247) and `theme::parse` (`crates/delphi/src/theme.rs#L32-L34` @ audited commit) only require `theme_schema: 1`.
  - **Remediation:** In `check_frontmatter` (`crates/delphi/src/lint.rs#L113-L130` @ audited commit), check `path == "heuristics.md"` in addition to `GOVERNED`, exclude `"operator/core/"` and `"operator/skills/"` (or narrow `"operator/"` in `GOVERNED` to `"operator/profile.md"`, `"operator/heuristics.md"`, `"operator/memory/"`), and remove `"themes/"` from `GOVERNED` (since `theme_names` (`crates/delphi/src/lint.rs#L93-L109` @ audited commit) already validates `themes/*/theme.md` frontmatter).

- **[P1-MAJOR]** `crates/delphi/src/pathguard.rs#L82` (`crates/delphi/src/pathguard.rs#L82` @ audited commit), `crates/delphi/src/pathguard.rs#L127-L129` (`crates/delphi/src/pathguard.rs#L127-L129` @ audited commit), `crates/delphi/src/lint.rs#L556-L561` (`crates/delphi/src/lint.rs#L556-L561` @ audited commit) — `[REV-SEC-02]` (**R3-2: `skills/custom/` segment-membership check allows writing `skills/custom/scripts/SKILL.md` and unratified `skills/custom/<s>/scripts` files**)
  - **Failure Mode:**
    1. In `never_writable` (`crates/delphi/src/pathguard.rs#L127-L129` @ audited commit), the exemption is `key.starts_with("skills/custom/") && key.split('/').any(|s| s == "scripts")`. Because `**` in `glob_match` (`crates/delphi/src/pathguard.rs#L174` @ audited commit) matches zero segments (`go(&g[1..], p)`), Builder's default write glob `"skills/custom/**/scripts/**"` (`agents/builder/AGENT.md` (`agents/builder/AGENT.md#L8` @ audited commit)) matches:
       - `"skills/custom/scripts/SKILL.md"` (first `**` matches 0 segments, `"scripts"` matches the `<skill>` directory name, second `**` matches `"SKILL.md"`), allowing Builder to write `team/skills/custom/scripts/SKILL.md` directly via `Guard::check("skills/custom/scripts/SKILL.md", Access::Write)`;
       - `"skills/custom/<s>/scripts/SKILL.md"`; and
       - `"skills/custom/<s>/scripts"` (second `**` matches 0 segments, so `"scripts"` is a regular file under `skills/custom/<s>/`, which also bypasses `key.contains("/scripts/")` in `is_ratified_kind` (`crates/delphi/src/pathguard.rs#L82` @ audited commit) and `home::is_protected` (`crates/delphi/src/home.rs#L19` @ audited commit), allowing unratified reads and writes).
    2. In `write_glob_error` (`crates/delphi/src/lint.rs#L556-L561` @ audited commit), `if !segs.contains(&"scripts")` similarly accepts `write: ["skills/custom/scripts/**"]`, `write: ["skills/custom/scripts/SKILL.md"]`, and `write: ["skills/custom/x/scripts"]`.
  - **Remediation:** Define a helper `is_custom_skill_script(key: &str) -> bool` requiring `segs.len() >= 5 && segs[0] == "skills" && segs[1] == "custom" && !segs[2].is_empty() && segs[3..segs.len() - 1].contains(&"scripts") && !key.ends_with("/skill.md")`. Use it in `never_writable` (`crates/delphi/src/pathguard.rs#L127-L129` @ audited commit), and require the same segment structure (plus `!names_segment(&lower, "skill.md")`) in `write_glob_error` (`crates/delphi/src/lint.rs#L556-L561` @ audited commit).

- **[P1-MAJOR]** `crates/delphi/src/doctor.rs#L15` (`crates/delphi/src/doctor.rs#L15` @ audited commit), `crates/delphi/src/doctor.rs#L26-L27` (`crates/delphi/src/doctor.rs#L26-L27` @ audited commit), `crates/delphi/src/doctor.rs#L184-L200` (`crates/delphi/src/doctor.rs#L184-L200` @ audited commit), `crates/delphi/src/lint.rs#L55` (`crates/delphi/src/lint.rs#L55` @ audited commit) — `[REV-SEC-03]` (**R3-3: `doctor::overlay` allows `team/` to shadow Zone 1 paths without override narrowing checks, and ignores symlinks and deleted live files**)
  - **Failure Mode:**
    1. In `doctor::overlay` (`crates/delphi/src/doctor.rs#L26-L27` @ audited commit), `add(ws.dir.join("team"), "", &mut files)` runs *after* `base/`, `themes/`, and `core/` with prefix `""` and no zone filter. If `team/` contains a file at a built-in Zone 1 path such as `team/agents/scout/AGENT.md` (instead of `team/agents/custom/scout/AGENT.md`), it overwrites `tree.files["agents/scout/AGENT.md"]`. Because `lint::check_manifests` (`crates/delphi/src/lint.rs#L151` @ audited commit) only sets `builtin = Some(...)` when `dir` starts with `"custom/"`, and `doctor::check_overrides` (`crates/delphi/src/doctor.rs#L302` @ audited commit) only inspects `core/agents` and `operator/core/agents`, a widened manifest at `team/agents/scout/AGENT.md` is validated with `builtin = None` (bypassing `override_widening` (`crates/delphi/src/lint.rs#L579-L606` @ audited commit)) and `delphi doctor` exits `0`.
    2. `doctor::overlay` (`crates/delphi/src/doctor.rs#L15` @ audited commit) and `Tree::from_repo` (`crates/delphi/src/lint.rs#L55` @ audited commit) filter `WalkDir` entries with `e.file_type().is_file()`, which is `false` for symlinks when `follow_links` is disabled. Furthermore, `check_ratified` (`crates/delphi/src/doctor.rs#L184-L200` @ audited commit) iterates over `&tree.files` rather than `&lock` (unlike the `op_lock` loop at `#L243-L249` (`crates/delphi/src/doctor.rs#L243-L249` @ audited commit)). Consequently, if a symlink is placed in `core/`, `team/`, `local/`, or `operator/`—or if a live protected file with no `base/` counterpart (`team/REVIEW.md`, `team/context/team-heuristics.md`, `local/heuristics.md`, `local/memory/agents/*/lessons.md`) is deleted or replaced by a symlink—it is omitted from `tree.files` and silently skipped by `doctor` and `lint`.
  - **Remediation:** In `doctor::check_ratified` (`crates/delphi/src/doctor.rs#L184-L200` @ audited commit), also verify that every non-`#` key in `lock` exists in `tree.files` (emitting a drift warning if missing) and reject any file in `team/` that encroaches on a Zone 1 path (`GOVERNANCE.md`, `SPEC.md`, `context/heuristics.md`, `themes/**`, non-`custom/` `agents/**` and `skills/**`). In `doctor::overlay` (`crates/delphi/src/doctor.rs#L15` @ audited commit) and `Tree::from_repo` (`crates/delphi/src/lint.rs#L55` @ audited commit), include `!e.file_type().is_dir()` (or explicitly flag `e.file_type().is_symlink()` as a failure in `doctor`/`lint`).

---

### Verdict
block
