# Delphi Repository Review Rubric (`REVIEW.md`)
> **Security Invariant:** Critics (**Red-Team**, **Strategist**) and automated CI pipelines MUST load this file strictly from the target base branch (`origin/main`), NEVER from an untrusted PR head branch.  
> **Budget Invariant:** This file must remain under **300 lines**. Any rule violated $\ge 2$ times must be promoted to a deterministic script in `skills/*/scripts/` or CI and pruned from this file.

---

## 1. Review Philosophy & Severity Calibration

Evaluate every Pull Request or Spec Amendment against the linked Specification (`specs/*.md`), [GOVERNANCE.md](GOVERNANCE.md), and the invariants below. Prioritize high-consequence defects, security boundary violations, and architectural drift over stylistic opinions.

- **🔴 BLOCKING (`P0`):** Security vulnerabilities, prompt-injection or memory-poisoning vectors, secret exposure, violation of [GOVERNANCE.md](GOVERNANCE.md), broken provider thinking-signature round-trips, or unapproved deviation from the linked `specs/*.md` contract.
- **🟠 MAJOR (`P1`):** Logic bugs, concurrency/worktree races, unhandled provider timeouts/fallbacks, destructive overwrites in `delphi update`, or missing regression tests for core paths.
- **🟡 MINOR (`P2`):** Non-blocking edge cases, suboptimal context token usage (>500 lines in a `SKILL.md` or `AGENT.md`), or missing YAML provenance frontmatter.

---

## 2. Security & Sovereignty Invariants (`Red-Team` Audit Scope)

- **`[REV-SEC-01]` Untrusted Input Isolation & Taint Tracking:** Any code or skill ingesting external web pages, GitHub issues, or third-party repositories MUST wrap content in untrusted data delimiters, strip executable control tokens, and tag outputs with `source_type: external_untrusted`. Untrusted findings must never auto-promote into `context/heuristics.md`, `REVIEW.md`, or `memory/agents/*/lessons.md`.
- **`[REV-SEC-02]` Least-Privilege Tool Enforcement:** Verify that non-builder specialists (`Scout`, `Investigator`, `Architect`, `Strategist`, `Red-Team`) are never granted source-code write or unrestricted shell execution permissions in `agents/*/AGENT.md`. Their write globs stay limited to Markdown in their declared logical zones (`specs/**`, `missions/**`, `context/domains/**`). Only Builder (and Writer, for `@worktree/README.md` and `@worktree/docs/**`) may write `@worktree/**`. Manifests contain only logical paths (`specs/001` §2.2): reject any physical path, any glob that could match a `.git` file or directory at any depth, any glob reaching built-in `skills/<builtin>/**`, and any write glob over CI configuration (`.github/**`).
- **`[REV-SEC-03]` Three-Zone Updater Safety:** Any changes touching `delphi update` or workspace initialization MUST guarantee zero destructive overwrites of user-owned paths (`memory/`, `context/`, `missions/`, `specs/`, `agents/custom/`, `skills/custom/`).
- **`[REV-SEC-04]` Ratified Rubric Loading:** Verify that audits load `REVIEW.md` and `GOVERNANCE.md` from the workspace's **ratified copies** (`local/ratified/`, verified against `ratified.lock`), never from a worktree or PR branch. When the audited repo itself tracks those files (as Delphi's own source repo does), audits *also* enforce the base-branch copy via `git show origin/main:...`, or the local base ref with no remote. If neither source is available, the audit fails closed.
- **`[REV-SEC-05]` Credential Hygiene:** Reject any diff containing hardcoded API keys, tokens, local user home paths (`/Users/...`), or unredacted session traces.
- **`[REV-SEC-06]` Computed Taint & Tool Gating:** Reject any code path that lets an agent set or raise `source_type`, or that exposes mutating, outbound, or dispatch tools to a span tainted $L_1$ or lower (`specs/001` §5.1–5.2).
- **`[REV-SEC-07]` Execution Containment:** Reject any code that executes agent-requested commands outside the OS sandbox without an approval, lets an agent write the shared `.git/` directory, or auto-approves a Red-tier action in any `/permissions` mode or on timeout.
- **`[REV-SEC-08]` Provisional-Decision Containment:** Reject any code path that lets a provisional decision (`specs/015`) apply to an action that is not an Amber worktree edit (eligibility must stay an allow-list that defaults to no), that lets an agent write or omit a `decision.provisional` entry, that lets an optimistic task merge, open a pull request or reach the operator's checkout while any allowed decision is unreviewed (gate 6, including the re-check when the merge is approved), or that starts an optimistic task without the OS sandbox.
- **`[REV-SEC-09]` Relay Narrowness:** Reject any change that widens what a relay may do (`specs/014`): a method or event kind added to the relay lists without a spec change and a test that pins the list, a path by which a relay approves a Red card or "allows similar", a reply accepted without its single-use code, an authenticated allowlisted sender, or a first-line verdict, a reply parsed for anything beyond a verb and a code, or a `[relay]` setting read from anything but the ratified copy.
- **`[REV-SEC-10]` Hooks Only Tighten:** Reject any code path by which a hook's output or exit status approves, allows, edits or skips an approval or a permission check (a hook may only add a refusal; "no objection" must change nothing), any failure of a deny hook (non-zero exit, timeout, not starting) that lets the action proceed, any hook that runs with an inherited environment or through a shell the operator did not write, and any `[hooks]` setting read from anything but the ratified copy.

---

## 3. Architectural & Spec Alignment Rules (`Strategist` Audit Scope)

- **`[REV-ARCH-01]` Spec-First Linkage:** Every non-trivial implementation PR MUST include `Implements-Spec: specs/NNN-<slug>.md` pointing to a specification with `status: approved`. Flag any out-of-spec features or gold-plating (The scope audit).
- **`[REV-ARCH-02]` Provider Dialect Compilation (No Raw String Flattening):** Reject any model adapter or skill that flattens conversation turns to plain strings or hardcodes single-vendor prompt assumptions instead of routing through the **Integrator** Intermediate Prompt Representation (`ContentBlock` arrays preserving `signature` / `thoughtSignature` / `encrypted_content`).
- **`[REV-ARCH-03]` Progressive Disclosure in Agents & Skills:** `agents/*/AGENT.md` and `skills/*/SKILL.md` files must remain concise (<500 lines) with valid YAML frontmatter. Complex parsing, Git operations, or validation logic must live in deterministic `scripts/` helpers.
- **`[REV-ARCH-04]` Worktree Isolation:** Any task mutating source code or running git bisects must operate inside an isolated worktree (`.delphi/worktrees/<task-id>/`), never mutating the root workspace index concurrently.
- **`[REV-ARCH-05]` Canonical Roles Only:** Governance, specs, manifests, config, frontmatter and code refer to agents only by canonical role ID (`specs/001` §6.1). Reject any theme name or persona text outside `themes/`, and any rule or check that depends on the active theme.

---

## 4. Known Anti-Patterns & Regression Traps (Distilled via `Orchestrator` Coaching)

- **`[REV-TRAP-01]` Council Round 1 Leakage:** Council Round 1 execution must use isolated parallel calls without shared scratchpad state; flag any code path that leaks Round 1 peer responses before Round 2 cross-examination.
- **`[REV-TRAP-02]` Atomic Knowledge Base Writes:** Writes to `context/*.md` and `memory/agents/*/*.md` must use atomic write-and-rename with validated YAML provenance frontmatter (`author`, `timestamp`, `source_type`, `confidence`, `source_mission`).
- **`[REV-TRAP-03]` 60Hz TUI Delta Coalescing:** Streaming token deltas over the Unix Domain Socket (`delphi-core` $\rightarrow$ `Console`/`Narrator`) must be frame-coalesced (16ms) rather than flushing per-token unbuffered writes or writing individual token deltas to SQLite.
- **`[REV-TRAP-04]` Compare-and-Swap Memory Writes:** Atomic rename alone does not prevent lost updates. Writers of `heuristics.md` and `lessons.md` must hold the file lock and verify the read hash before writing (`specs/004` §2.1).

---

## 5. False-Positive Suppression (`DO NOT FLAG`)

- **`[SUPPRESS-01]` Deterministic CI Deferral:** Do NOT comment on code formatting, whitespace, import sorting, or static type errors already enforced by deterministic CI linters/compilers.
- **`[SUPPRESS-02]` Historical Mission Archives:** Do NOT flag outdated assumptions inside closed mission logs (`missions/YYYY-MM-DD-*/`) unless the PR actively modifies them.
- **`[SUPPRESS-03]` Accepted Minority Reports:** Do NOT re-litigate trade-offs that are explicitly documented under the `Accepted Risks / Minority Report` section of the linked approved Specification. This applies only when the spec's `status: approved` was set by the **operator** and the risk entry predates the PR under review. It never suppresses a `P0`.

---

## 6. Required Review Output Contract

Format every review finding as:
```markdown
- **[P0-BLOCKING | P1-MAJOR | P2-MINOR]** `path/to/file#L10-L20` — `[Rule ID]`
  - **Failure Mode:** Concrete scenario showing how this fails or violates the spec/governance.
  - **Remediation:** Exact patch or structural adjustment required.
```
