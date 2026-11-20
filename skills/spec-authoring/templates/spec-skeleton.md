---
id: spec-NNN-short-name
title: "NNN: Short Name"
author: architect
timestamp: "2026-01-01T00:00:00Z"   # replace with the current UTC time
status: draft
reviewed_by: ""   # the reviews that happened (role, and where: a Council record, a Panel id, a pull-request review); GOVERNANCE.md §1.3
source_type: internal_generated
confidence: medium
source_mission: mission-id
tags: [spec]
---

# Spec NNN: Short Name

## 1. Scope & Objectives
What this spec decides, in a few sentences. Link the specs it builds on instead of restating them.

### 1.1 Non-Goals
What this spec deliberately does not do.

### 1.2 Simpler Alternative Considered
- **Alternative:** the cheapest thing that might work.
- **Why Rejected:** the concrete reason it does not, or, if there is none, build this instead.

### 1.3 Options Considered
At least three, from the `divergent-options` skill. This section does not replace 1.2.

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | the design a careful engineer would expect | the observation that shows it works or fails |
| B | constraint removed: name the constraint | the design if that constraint did not hold | its own test |
| C | a named method: inversion, a TRIZ principle with the contradiction stated, morphological recombination, or an analogy from a named field | the design that method produced | its own test |

Chosen: A, B or C, and why the others lost. Flagged by the guard (a new layer of abstraction with no test of its own): none, or which.

## 2. Design
Interfaces, data shapes, invariants and failure behaviour, at the level a Builder needs.

## 3. Authority
Only when the spec touches authority, trust, a gate or a sandbox; otherwise delete this section. What the change may do, what it may never do, and the test that proves each limit.

## 4. Slices
S0 is the spec itself, or a spike with its kill criterion written here before it runs. About 300 changed lines a slice; say why when one is larger.

| Slice | Criteria closed | Files | Lines (est.) | Depends on | Blocked by |
| :-- | :-- | :-- | --: | :-- | :-- |
| S0 | (the spec) | this file | (docs) | none | none |
| S1 | AC-1, AC-2 | the files it changes | 100-200 | S0 | none, or OD-n |

## 5. Acceptance Criteria
1. **AC-1:** an observable, testable statement.
2. **AC-2:** another one.

## 6. Verification Plan
How each criterion is proven, by number: the test (named with `specs/NNN AC-n`), check or manual step.

| Criterion | How it is verified |
| :-- | :-- |
| AC-1 | the test or check |

## 7. What Is Not Verified
Each claim this spec makes that nobody has observed, and what would show it.

## 8. Decisions for the Operator
The architect proposes whether a decision is reversible; the operator decides. Only a decision marked reversible may take its recommendation by default. One that changes a criterion, an interface, a data shape or authority blocks the slices it names until answered.

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | the question | the recommendation and its alternative | proposed: yes or no | none, or S-n |

<!--
Check with: delphi check spec <this file>
An agent never sets `status: approved`; approval comes from the operator.
A tiny change (about 100 lines or fewer, 3 criteria or fewer) may delete sections 4 and 8 and say so in one line.
-->
