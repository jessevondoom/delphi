---
id: spec-006-installer-and-updater-history
title: "006 history: amendment layers removed by the fold (non-normative)"
author: architect
timestamp: 2026-10-30T00:00:00Z
status: draft   # history only; never normative, never approved
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: user_ratified
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, history]
---

# History of spec 006 (non-normative): amendment preambles and superseded text removed by the 2026-10-30 fold; the normative text is `specs/006-installer-and-updater.md`.

## Original frontmatter status comment
status: approved   # by the operator, 2026-10-23, after Council 002. The embedded-model note in §2.1 and doctor check 6 in §5 (Amendment 004-A1) approved by the operator, 2026-10-25; Amendment 006-A5 (development channel) approved by the operator, 2026-10-25; Amendment 006-A6 (running an update ratifies what the release ships) approved by the operator, 2026-10-25

## Amendment headings and preambles (verbatim)

### 3.2 Amendment 006-A1: Operator Home (D-20; approved by the operator 2026-10-23 after Council 003)

### 3.3 Amendment 006-A2: Scope Files & Ratified Specs (Council 004; approved under the operator's pre-authorization 2026-10-23)
> This amendment is part of the approved spec.

### 3.4 Amendment 006-A3: Release Signing Moves to Step 5 (approved by the operator 2026-10-23)
> This amendment is part of the approved spec. It resolves drift found while building Step 1 (Council 004, Builder seat).

### 3.5 Amendment 006-A4: One-Command Onboarding (approved by the operator 2026-10-24)
> This amendment is part of the approved spec. It replaces the "run `init`, then `setup`, then `doctor`, then `chat`" sequence with a single command, and supersedes the "offered `delphi init`" and "`--workspace` is required" clauses of workspace resolution (§3.1).

### 3.6 Amendment 006-A5: The Development Channel (approved by the operator 2026-10-25)
> This amendment is part of the approved spec. It adds a second update channel that follows `main`, beside the stable releases. Nothing about the stable channel changes.
> - **One command (supersedes the `self-update` and `rollback` commands named in this spec).**

### 3.7 Amendment 006-A6: Running an Update Ratifies the Release (approved by the operator 2026-10-25)
> This amendment is part of the approved spec. It supersedes the "one Red-tier approval" in §4 step 3 and the "stages the difference as a Red-tier proposal" for a new release in §3.1; every other source of change is unaffected.

## Superseded text

The pre-fold spec (every superseded clause, verbatim) is `git show origin/main:specs/006-installer-and-updater.md` as of the fold commit's parent.
