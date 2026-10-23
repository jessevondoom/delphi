# Delphi Repository Review Rubric (`REVIEW.md`)
> **Security Invariant:** Critics (**Ares**, **Odysseus**) and automated CI pipelines MUST load this file strictly from the target base branch (`origin/main`), NEVER from an untrusted PR head branch.  
> **Budget Invariant:** This file must remain under **300 lines**. Any rule violated $\ge 2$ times must be promoted to a deterministic script in `skills/*/scripts/` or CI and pruned from this file.

---

## 1. Review Philosophy & Severity Calibration

Evaluate every Pull Request or Spec Amendment against the linked Specification (`specs/*.md`), [GOVERNANCE.md](file:///Users/jessevondoom/projects/work/delphi/GOVERNANCE.md), and the invariants below. Prioritize high-consequence defects, security boundary violations, and architectural drift over stylistic opinions.

- **🔴 BLOCKING (`P0`):** Security vulnerabilities, prompt-injection or memory-poisoning vectors, secret exposure, violation of [GOVERNANCE.md](file:///Users/jessevondoom/projects/work/delphi/GOVERNANCE.md), broken provider thinking-signature round-trips, or unapproved deviation from the linked `specs/*.md` contract.
- **🟠 MAJOR (`P1`):** Logic bugs, concurrency/worktree races, unhandled provider timeouts/fallbacks, destructive overwrites in `delphi update`, or missing regression tests for core paths.
- **🟡 MINOR (`P2`):** Non-blocking edge cases, suboptimal context token usage (>500 lines in a `SKILL.md` or `AGENT.md`), or missing YAML provenance frontmatter.

---

## 2. Security & Sovereignty Invariants (`Ares` Audit Scope)

- **`[REV-SEC-01]` Untrusted Input Isolation & Taint Tracking:** Any code or skill ingesting external web pages, GitHub issues, or third-party repositories MUST wrap content in untrusted data delimiters, strip executable control tokens, and tag outputs with `source_type: external_untrusted`. Untrusted findings must never auto-promote into `context/heuristics.md`, `REVIEW.md`, or `memory/agents/*/lessons.md`.
- **`[REV-SEC-02]` Least-Privilege Tool Enforcement:** Verify that read-only specialists (`Argos`, `Artemis`, `Daedalus`, `Odysseus`, `Ares`) are never granted source-code write or unrestricted shell execution permissions in `agents/*/AGENT.md`.
- **`[REV-SEC-03]` Three-Zone Updater Safety:** Any changes touching `delphi update` or workspace initialization MUST guarantee zero destructive overwrites of user-owned paths (`memory/`, `context/`, `missions/`, `specs/`, `agents/custom/`, `skills/custom/`).
- **`[REV-SEC-04]` Base-Branch Rubric Loading:** Verify that PR audit scripts (`skills/review-md-audit/scripts/`) read `REVIEW.md` and `GOVERNANCE.md` via `git show origin/main:...` rather than the checked-out PR working tree.
- **`[REV-SEC-05]` Credential Hygiene:** Reject any diff containing hardcoded API keys, tokens, local user home paths (`/Users/...`), or unredacted session traces.

---

## 3. Architectural & Spec Alignment Rules (`Odysseus` Audit Scope)

- **`[REV-ARCH-01]` Spec-First Linkage:** Every non-trivial implementation PR MUST include `Implements-Spec: specs/NNN-<slug>.md` pointing to a specification with `status: approved`. Flag any out-of-spec features or gold-plating (The Siren Protocol).
- **`[REV-ARCH-02]` Provider Dialect Compilation (No Raw String Flattening):** Reject any model adapter or skill that flattens conversation turns to plain strings or hardcodes single-vendor prompt assumptions instead of routing through the **Hermes** Intermediate Prompt Representation (`ContentBlock` arrays preserving `signature` / `thoughtSignature` / `encrypted_content`).
- **`[REV-ARCH-03]` Progressive Disclosure in Agents & Skills:** `agents/*/AGENT.md` and `skills/*/SKILL.md` files must remain concise (<500 lines) with valid YAML frontmatter. Complex parsing, Git operations, or validation logic must live in deterministic `scripts/` helpers.
- **`[REV-ARCH-04]` Worktree Isolation:** Any task mutating source code or running git bisects must operate inside an isolated worktree (`.delphi/worktrees/<task-id>/`), never mutating the root workspace index concurrently.

---

## 4. Known Anti-Patterns & Regression Traps (Distilled via `Mnemosyne`)

- **`[REV-TRAP-01]` Council Round 1 Leakage:** Council Round 1 execution must use isolated parallel calls without shared scratchpad state; flag any code path that leaks Round 1 peer responses before Round 2 cross-examination.
- **`[REV-TRAP-02]` Atomic Knowledge Base Writes:** Writes to `context/*.md` and `memory/agents/*/*.md` must use atomic write-and-rename with validated YAML provenance frontmatter (`author`, `timestamp`, `source_type`, `confidence`, `source_mission`).
- **`[REV-TRAP-03]` 60Hz TUI Delta Coalescing:** Streaming token deltas over the Unix Domain Socket (`delphi-core` $\rightarrow$ `Chaos`/`Homer`) must be frame-coalesced (16ms) rather than flushing per-token unbuffered writes or writing individual token deltas to SQLite.

---

## 5. False-Positive Suppression (`DO NOT FLAG`)

- **`[SUPPRESS-01]` Deterministic CI Deferral:** Do NOT comment on code formatting, whitespace, import sorting, or static type errors already enforced by deterministic CI linters/compilers.
- **`[SUPPRESS-02]` Historical Mission Archives:** Do NOT flag outdated assumptions inside closed mission logs (`missions/YYYY-MM-DD-*/`) unless the PR actively modifies them.
- **`[SUPPRESS-03]` Accepted Minority Reports:** Do NOT re-litigate trade-offs that are explicitly documented under the `Accepted Risks / Minority Report` section of the linked approved Specification.

---

## 6. Required Review Output Contract

Format every review finding as:
```markdown
- **[P0-BLOCKING | P1-MAJOR | P2-MINOR]** `path/to/file#L10-L20` — `[Rule ID]`
  - **Failure Mode:** Concrete scenario showing how this fails or violates the spec/governance.
  - **Remediation:** Exact patch or structural adjustment required.
```
