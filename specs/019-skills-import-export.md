---
id: spec-019-skills-import-export
title: "019: Skills Import and Export (agentskills.io)"
author: architect
timestamp: 2026-10-26T09:00:00Z
status: approved   # by the operator, 2026-10-25, by merging the pull request that added it (commit 91eb352)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, skills, portability, security]
---

# Spec 019: Skills Import and Export

## 1. Scope & Objectives
A skill written for another tool should be usable in Delphi, and a Delphi skill usable elsewhere, without Delphi giving up the rule that a skill is code-like text an agent will follow and so loads only after the operator has ratified it (`specs/001` §2.3, GOVERNANCE §5.7).

Origin: the comparison in [`gap-closure-plan-2026-10-25.md`](../missions/2026-10-23-foundational-design/gap-closure-plan-2026-10-25.md) §3.1. All 43 built-in skills were checked against the [agentskills.io](https://agentskills.io/specification) rules: they conform, and differ in four extra top-level fields and a `templates/` directory.

### 1.1 Non-Goals
- Importing scripts. Never (§3).
- Keeping a skill up to date with its source. An import is a copy, once.

### 1.2 Simpler Alternative Considered
**Tell the operator to copy files by hand into `team/skills/custom/`.** That works today and is how a skill is made at all. It leaves every check to the operator: the name rules, the 200-character description, the schemas, which roles may use it, and whether a `scripts/` directory came along. The command does the mechanical checks and refuses the unsafe parts; the operator still reads and ratifies.

## 2. The format difference
The standard needs `name` and `description`, and allows `license`, `compatibility`, `metadata` (a map of strings to strings) and `allowed-tools`. Delphi also requires `authorized_agents`, `input_schema`, `output_schema` and `required_permissions` (`specs/001` §3.1), and caps the description at 200 characters (the standard allows 1024).

## 3. `delphi skills`
- **`check <dir>`** says whether a directory is a valid standard skill, whether it already carries Delphi's fields, and which files an import would skip. It needs no workspace.
- **`export <name> [--out <dir>]`** writes a skill (looked up in `team/skills/custom`, then `core/skills`, then `base/skills`) as a standard skill. Delphi's four fields go into `metadata` as JSON strings under `delphi.*` keys, since `metadata` holds strings only; nothing else changes (the body is untouched, `templates/` keeps its name). It refuses to overwrite.
- **`import <dir> --agents <role,role> [--description "…"]`** brings a skill in:
  - **The operator names the roles** (`--agents`). A foreign skill never decides who may use it; an export's own list is shown only as a hint in the error.
  - Missing fields get the **least** that passes: `input_schema` and `output_schema` as a bare `{type: object}`, `required_permissions` as `read:all`. The command says so.
  - **Scripts are never imported:** a `scripts/` directory is skipped and listed, declared `deterministic_scripts` are dropped, and so are symbolic links and hidden files. `allowed-tools` is ignored with a note, because it names another tool's tools.
  - A description over 200 characters is **refused** with a request for `--description`, never cut for the operator. A body over the cap, a bad name or an unknown role is refused with the reason.
  - The package is limited to 100 files, 512 KiB each and 2 MiB in all.
  - It writes to `team/skills/custom/<name>/` and **never overwrites** anything, including a built-in of the same name.
  - The result must pass the same checks as any custom skill (`lint::skill_errors`).
- **Ratification is the existing mechanism.** A file under `skills/` is a protected path (GOVERNANCE §5.7), so the new skill appears as a pending Red proposal and is not loaded until the operator reads it and runs `/ratify`. The command ends by saying so, and that imported text is put in front of an agent as instructions.

## 4. Verification Plan
Unit (`skills_cmd.rs`): the standard's name, description, `metadata` and `compatibility` rules are checked; an import without roles is refused and gets least-privilege defaults when given them; a skill's own claim about its roles cannot decide them; declared scripts are dropped; a long description, an oversize body, no frontmatter and an unclosed frontmatter are refused; **every built-in skill exports as a valid standard skill, keeps its body byte for byte, and imports back with all four fields unchanged**; a package skips scripts, links and hidden files and refuses an oversize file; an import writes under the protected directory, a second import and a name that shadows a built-in are refused and nothing is overwritten; export then import through a workspace restores a built-in; the command-line arguments are picked apart correctly.

Not covered: the reference validator `skills-ref` (not run; this checks the rules the specification states); importing from a URL.
