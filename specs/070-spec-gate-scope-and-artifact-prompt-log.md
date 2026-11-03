---
id: spec-070-spec-gate-scope-and-artifact-prompt-log
title: "070: Scope of No Spec, No Build, and the Artifact Prompt Log"
author: architect
timestamp: 2026-11-02T23:58:00Z
status: approved   # by the operator, 2026-11-03 ("approve it and build"); Council t_0001 unanimous approve-with-amendments, applied below; GOVERNANCE §1.3 ratified by the operator 2026-11-03
approved_by: operator (2026-11-03)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, governance, no-spec-no-build, artifacts, prompt-log, audit]
---

# Spec 070: Scope of No Spec, No Build, and the Artifact Prompt Log

> **Status: approved (operator, 2026-11-03).** Operator direction (2026-11-02): *No Spec, No Build* is kept in full for software and is credited for the quality of Delphi; it is not meant to gate documents, decks, images or media. Those get a prompt log instead. Everything else the system writes within its permissions stays allowed.

---

## 1. Problem & Intent

[`GOVERNANCE.md`](../GOVERNANCE.md) §1.3 exempts Markdown notes and `scratch/` files, but says nothing about scaffolds, non-Markdown artifacts (decks, images, media), or coding projects outside Delphi. Without a stated scope, the rule is either read too narrowly (blocking ordinary work) or loosely (eroding it for code).

This spec fixes the scope in three classes, and adds a minimal audit record for artifacts.

## 2. The Three Classes

| Class | What it covers | Rule |
| :-- | :-- | :-- |
| **Software** | Building Delphi itself; writing code, tests or helper scripts; guiding a coding project outside Delphi (advice that results in code is gated at the build, not in chat) | **No Spec, No Build, unchanged and mandatory.** An `approved` spec and the `Implements-Spec:` trailer. For an external project the authorizing spec lives in Delphi's governed space, `specs/external/<project>/` (project registered under [`specs/058`](058-project-scopes-scaffolding-and-promotion.md)). An in-repo mirror is allowed and non-authoritative; a PR or branch of the external repo can never supply its own authorizing spec. |
| **Artifacts** | Docs, decks, images, media and any other work output the operator asked for | **No spec.** Every create or edit is recorded in the prompt log (§3). |
| **System operation** | Writing Markdown (notes, plans, reports), creating project scaffolds, `scratch/` files, and other writes that belong to regular operation or an operator-approved prompt | **Always allowed** within the role's permissions, the sandbox and the approval tiers. No spec, no log. |

### 2.1 Limits that do not move

1. **Governed paths keep ratification:** `specs/`, `memory/`, `agents/`, `skills/`, `themes/` and the other governed paths. Writing there is never "system operation" under this spec.
2. **Authority is not widened.** Red actions still always ask. No class lets an agent approve its own action, act outside the sandbox, or raise a content's trust.
3. **Fail closed, closed allow-list.** An Artifact is defined by what the file does. Only types on a closed allow-list are Artifacts (plain documents, images, audio and video, non-scripted decks and PDFs). Anything unlisted is **Software**. Anything executable, parsed as configuration, or installed or loaded is Software: scripts, CI files, hooks, notebooks, SQL, themes, skills, prompts, and HTML, SVG, decks or office files with scripts or macros. Class comes from the daemon's type and content check, never from an agent's claim.
4. **A scaffold's code is code.** A scaffold is system operation only if it holds no executable content; executable files in it are Software.
5. **Markdown, scaffolds and scratch stay inside permissions** and never touch the governed paths of item 1.
6. **Class is derived, not declared.** A file cannot name its own class (the same principle as [`specs/052`](052-knowledge-status-capture-on-contact-and-the-digest.md) for status).

## 3. The Artifact Prompt Log

1. **Content: the operator's prompt only.** Verbatim text of the operator prompt that caused the create or edit. No surrounding context, no system or tool messages, no model reply.
2. **Entry fields:** UTC timestamp; artifact path; operation (`create` or `edit`); the prompt; optional content hash of the artifact after the write.
3. **Location:** the daemon-only `local/` area of the workspace (for example `local/artifact-log/<mission>/`), outside git and agent worktrees, with local-only permissions and excluded from exports by default. A pointer from the mission names the log. One line per entry.
4. **Writer and integrity:** the daemon, not the agent. Append-only and hash-chained; agents have no write or truncate access. Each entry also carries the task id and the artifact hash.
5. **No `(system)` entries.** Work started by a schedule or another agent is recorded only in the daemon audit log, linked by task id and artifact hash. This keeps the prompt log operator-only and blocks taint laundering.
6. **Handling:** the log is data, never instructions, and never feeds lessons or heuristics. Prompts may hold pasted secrets: access controls and a retention rule apply, and the secret scrubber question is open (§8).
7. **Limits stated:** the log records the prompt only, not the context the agent acted on, and does not capture edits made outside Delphi.
8. **Multi-file runs:** one prompt that produces several artifacts appears once per artifact, each pointing to the same prompt text.

## 4. Changes Required (after approval)

1. **GOVERNANCE §1.3** reworded to the three classes. Staged as a constitution proposal; applied only by operator `/ratify`.
2. An entry in `context/invariant-enforcement.md` mapping the rule to a daemon check, and a Red-tier ratification (GOVERNANCE §5.7).
3. Matching text in `AGENTS.md`, `CONTRIBUTING.md`, `agents/builder/AGENT.md`, the `rule` string and test in `crates/delphi-engine/src/mcp_project.rs`, and the `invariant-enforcement.md` row.
4. Daemon: the write-path classifier and the prompt-log writer, with events for each entry.
5. Schema `schemas/prompt-log.schema.json` and a section in `docs/reference/`.
6. Spec tracker row.

## 5. Slices

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| PL1 | Path classifier (Software / Artifact / System), fail-closed, with tests for each extension and location class | 003, 058 |
| PL2 | Prompt-log writer, schema, redaction, `(system)` entries | PL1 |
| PL3 | GOVERNANCE §1.3 and the matching text, by ratified proposal | PL1, PL2 |

## 6. Acceptance Criteria

- **AC-1:** source, test, script, CI and manifest files classify as Software; an unknown extension classifies as Software.
- **AC-2:** a Markdown note and a `scratch/` file write with no spec and no log entry.
- **AC-3:** an image, deck or document create and edit each append one entry with the verbatim prompt and nothing else.
- **AC-4:** the log contains no model reply, context or tool output (test against a fixture run).
- **AC-5:** a write to `specs/`, `memory/`, `agents/`, `skills/` or `themes/` is still refused without ratification.
- **AC-6:** an agent cannot write, truncate or edit the log through its file tools; a broken hash chain is detected.
- **AC-7:** with no spec, a Builder run on Software is still refused.
- **AC-8:** an HTML or SVG file with script, a notebook and an unlisted type classify as Software.
- **AC-9:** a spec supplied by an external repo's own branch does not authorize a build.

## 7. Simpler Alternative Considered

Add one sentence to GOVERNANCE §1.3 ("documents and media are exempt") with no log. Rejected: it gives no audit trail for artifacts, leaves "code" undefined so the exemption could widen, and leaves the external-project case open. A single sentence plus the §3 log would be enough only if the classifier of §2.1 were still specified.

## 7a. Proposed GOVERNANCE §1.3 text (for operator `/ratify`)

> 3. **No Spec, No Build (software):** Builder may not implement non-trivial software (building Delphi, writing code, tests or scripts, or guiding a coding project outside Delphi) without an `approved` spec authored by Architect, reviewed by Strategist and Red-Team, and linked to the task (`Implements-Spec: specs/NNN-*.md`); for an external project the spec lives in `specs/external/<project>/`. Artifacts (documents, decks, images and media on the closed allow-list of `specs/070`) need no spec; each create or edit is recorded in the daemon-written operator-prompt log. Markdown notes, scaffolds without executable content and plain files in `scratch/` are regular operation and need neither. Unlisted, executable or loaded types are software, and the governed paths (`specs/`, `memory/`, `agents/`, `skills/`, `themes/`) stay under ratification.

## 8. Council t_0001 Outcome and Open Decisions

Architect, red-team and strategist all voted approve with amendments; the amendments are applied above. Answers adopted: **D-1** daemon-only `local/` log; **D-2** `specs/external/<project>/`; **D-3** no `(system)` entries in the prompt log. Seats could not read this file when they ruled, so the amended text needs a re-review.

**OD-1 (resolved as recommended, pending operator approval of the spec):**
- **Now (in PL2):** secret scrubbing with the existing scrubber, applied before an entry is written; fixture tests.
- **Not needed:** SHA pinning. External specs live in `specs/external/<project>/` (D-2), so there is nothing in an external repo to pin.
- **Deferred to a follow-up spec:** active-content stripping (scripted HTML/SVG, decks, macros), hash-chain verification tooling, retention policy and MIME sniffing. Until then, scripted or unlisted files are Software under §2.1.3, so deferral does not weaken the gate.
