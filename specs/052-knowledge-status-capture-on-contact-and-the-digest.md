---
id: spec-052-knowledge-status-capture-on-contact-and-the-digest
title: "052: Knowledge status, capture on contact and the daily digest: one vocabulary for how much a claim can be relied on, and growth that costs the operator nothing"
author: architect
timestamp: 2026-10-28T21:00:00Z
status: approved   # by the operator, 2026-10-28 ("merge and approve all, follow the rest of your recommendations"); the provisional answers in §8 stand; nothing is built
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a synthesis of approved and draft specs plus a code read; nothing here has been tried with an operator
source_mission: 2026-10-23-foundational-design
tags: [spec, knowledge, memory, validator, trust, capture, digest, ux, draft]
---

# Spec 052: Knowledge status, capture on contact and the daily digest

> Approved 2026-10-28. It adds no store and no authority. It joins four things that already exist or are approved: the memory entry fields of [`specs/050`](050-memory-lifecycle-types-supersession-and-trace-mining.md) ML-1..ML-4, the operator-facing labels of [`specs/040`](040-validator-role-and-fact-ratification.md) §18.2, the staged-note capture of [`specs/031`](031-layered-learning-and-training-mode.md) §3, and the decisions pane of [`specs/044`](044-decisions-pane.md). Thesis: [`docs/thesis.md`](../docs/thesis.md) (trust, compounding, honest uncertainty).

## 1. Why
Delphi's knowledge side has two halves that do not meet.
- **Protection is strong.** A six-level taint lattice computed by the daemon (`delphi-core/src/provenance.rs`), ratification by compare-and-swap, Xenia endorsement and freshness.
- **Growth is gated.** A useful answer, a Council verdict or a decision becomes durable knowledge only through a failure trigger, `/retro`, a validator path that is not built, or a deliberate act. The operator's normal work adds almost nothing, and what they read does not say how far to rely on it.
- **The vocabulary disagrees with itself.** Web research is indexed at `federated_verified` (L2, `research.rs`) with no validation and no steward; validated facts are L3 (040 §7); bundle knowledge is L2 (024 §3); Xenia memory needs an endorsement for L2. "Verified" means three different things.

The goal is that every operator interaction can add to the library with no extra step, the library always says how far each claim can be relied on, and nothing is promoted by being popular, recent or repeated.

### 1.1 Non-Goals
- Changing the six-tier lattice, taint, any gate, any tool or approval rule. The lattice stays the security mechanism; this spec is about what the operator reads and what is captured (040 §18.2 draws the same line).
- Building the validator. Its slices stay in 040. This spec only fixes where its output shows.
- Sending anything off the machine. Capture is local; sharing stays the opt-in, previewed contribution of 031 and 040 §17.3.

### 1.2 Simpler Alternative Considered
Keep adding capture triggers one by one (more staged-note triggers in 031 §3.2). Rejected as the whole answer: it grows the store but leaves the operator with no way to see the status of what is stored, and each trigger needs its own review surface.

## 2. Principles
1. **Capture is free; promotion is earned.** Anything the operator touches may be stored as *noted* at no cost and no prompt. A higher status needs evidence or a person, never repetition.
2. **Status is derived, not declared.** It is computed by the daemon from the provenance index, validation records, endorsements and dates. A file cannot name its own status.
3. **Show the rung, always.** Every retrieval result, answer citation and detail view carries its status and age. Disagreement and gaps are shown, not smoothed (026 §1.3 principle 5).
4. **Proposals, never silent changes** (050 §2). Capture writes only to the staged tier; nothing reaches an always-loaded store without ratification.
5. **Private by default.** Conversation text is not stored as a claim; a claim holds the assertion and its sources, not the operator's wording (040 §17.1).

## 3. The additions
### 3.1 One status ladder (KS-1)
The operator-facing status of a stored claim, derived from existing state. The labels of 040 §18.2 are kept; two rungs refine "not validated" so that a new capture and a checked-and-failed one do not look alike.

| Status | Glyph (spec 053) | Derived from |
| :-- | :-- | :-- |
| **noted** | `○` dim | captured by the daemon or written by an agent; no check has run (any tier below L4) |
| **checking** | `◐` | queued or in progress with the validator (040 §18.3), or awaiting the digest |
| **validated** | `●` | validator-ratified (L3 `web_corroborated` or audited), L4, or L5 (040 §18.2) |
| **contested** | `◆` warm | a newer claim, a Council dissent or a contradiction search disagrees (KS-4) |
| **disproved** | `✕` | a `refuted` verdict (040 §18.4 D-5) |
| **retired** | `·` struck | superseded or expired (050 ML-2, ML-3); kept in history, never loaded |

Federated and bundle claims show the same ladder plus "endorsed by <steward>" or "from <bundle>" in the detail (040 §18.2). The mapping from tier and record to status is one table in code (`delphi-core`), so UI, CLI, search and checkpoints cannot disagree. The research writer's L2 tier (`research.rs`) is left as it is for security decisions; its status is **noted** until a validation record exists (OD-1).

### 3.2 Capture on contact (KS-2)
The daemon, from events it already sees, stages a candidate claim when the operator:
- accepts or acts on an answer (continues from it, `/pin`s it, exports it);
- receives a Council verdict (the verdict, its Minority Report and the claim each seat cited, as separate candidates);
- resolves a decision in the pane, including the reason typed with it;
- or finishes a research run.

Candidates are written to the `staged` zone with ML-1 fields (`kind`, `created`, `last_confirmed`, optional `valid_until`), the source pointers the span read, and status **noted**. They are capped per mission, deduplicated by subject key (050 ML-2), searchable at once with their label, and excluded from every always-loaded prompt. A candidate whose span was tainted is stamped below `internal_empirical` (050 ML-6). Capture adds no model call on the hot path: the claim text is the assistant's own sentence, selected by the daemon, or a one-line field the operator already typed.

### 3.3 The daily digest (KS-3)
One review surface: a new decision `kind: learn` in the decisions pane (044 §4.3), listed beside the cards the operator already owes, with the same one-by-one brief and a recommendation. A digest item is a candidate with its source, its status and one default action:
- **keep** (stays noted and searchable; the default for nothing-to-say items);
- **check** (queues it for the validator, 040 §5);
- **promote** (operator-ratified, L5; the existing `/retro` path of 031 L2 for rules);
- **dismiss** (retired; the dismissal is a signal that tunes capture, not a deletion).

Items are tickable for "keep" and "dismiss" only. Nothing in the digest blocks work, times out into a decision or counts as approval. `/retro` remains the path for rules, lessons and skills; the digest is the path for facts.

### 3.4 Contradiction and decay (KS-4)
- A new claim whose subject key or retrieval similarity matches a stored one, with a different assertion, raises a **contested** pair: both shown, side by side, in the digest, with the Council's Minority Report pattern (what each says, from where, how old). The daemon never picks a winner; the operator or the validator does.
- `valid_until` and the 90-day `last_confirmed` window (050 ML-3, OD-3) move a claim to a visible "review due" state, not to retired. Staleness shows on the result.

## 4. Where each lives
| Addition | Code and files |
| :-- | :-- |
| KS-1 | `delphi-core` status table beside `provenance.rs`; `kb.rs` result type; `delphi kb`, the knowledge window (028), checkpoint summaries (040 §18.2 list) |
| KS-2 | event handlers in `delphi-engine` on `approval`, `council` and `decision` events; the staged-note writer of 031 L1; ML-6 write-path audit as a precondition |
| KS-3 | `decision_model.rs` (a `learn` kind), the pane of 044, `/decisions` |
| KS-4 | the staged writer and `kb.rs` similarity (050 ML-2), 040's contradiction search when it exists |

## 5. Invariants and tests
- **Status is derived:** a test edits a file's frontmatter to claim `validated` and checks the status stays **noted**.
- **No promotion by use:** retrieving, citing or re-capturing a claim 100 times does not change its status.
- **Capture is inert:** a captured candidate is fenced as data in any prompt; no always-loaded store changes without ratification (ML-6 passes for every new write site).
- **Tainted capture stays low:** a claim captured from a tainted span is stamped below `internal_empirical`.
- **One table:** a test lists every surface that prints a status and checks each calls the shared table.
- **Digest never approves:** no `learn` item can resolve a card or satisfy a gate.
- **Privacy:** a candidate contains no operator message text beyond the claim sentence and typed reason; the scrubber of 040 §17.1 runs before write.

## 6. Acceptance criteria
1. One accepted answer, one Council verdict and one resolved decision each produce a searchable **noted** claim with no extra operator action.
2. Every result in `delphi kb search`, the knowledge window and the chat reader shows its status and age.
3. The digest lists new candidates, contested pairs and review-due items; a person clears ten items in under two minutes with the keyboard alone.
4. A contradicting claim produces a **contested** pair and nothing is overwritten.
5. Lattice, gates and approvals are unchanged; the existing provenance and ratification tests pass untouched.

## 7. Build plan
| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| KS1 | the status table and labels everywhere (display only); the "untrusted" wording test of 040 §18.2 | 040 V1 labels |
| KS2 | capture on contact: answer, verdict, decision, research | 050 ML1 (write-path audit), ML2 |
| KS3 | `learn` kind in the decisions pane and its replies | KS2, 044 D1-D7 (merged) |
| KS4 | contested pairs and review-due | 050 ML3, ML4; 040 contradiction search for the strong form |

One slice per pull request; each updates the tracker.

## 8. Open decisions (provisional answers, change on request)
- **OD-1** The research writer stores at L2. Provisional: leave the tier (a security input, 043) and show status **noted** until validated; revisit with the operator when 040 V1 exists.
- **OD-2** Two refined rungs (`noted`, `checking`) beyond the operator's `validated` / `not validated` / `disproved`. Provisional: yes, `contested` and `retired` too; the three original labels stay the headline.
- **OD-3** Which events capture in the first slice. Provisional: answer accepted, Council verdict, decision resolved; research run last.
- **OD-4** Whether the digest is a pane kind or its own pane. Provisional: a `learn` kind (one place for everything owed).

## 9. Non-goals
Fine-tuning; automatic promotion; deleting captured claims without a ratified proposal; an always-on background crawler (040 §17.1).

## 10. Known limits
Validation checks that sources support a claim, not that the sources are right (040 §7.2). The status ladder reports what was checked; it does not turn a claim into truth.
