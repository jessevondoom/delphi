---
id: spec-100-confidence-ladder-substantiated-vetted-and-an-audit-that-rolls-back
title: "100: The confidence ladder: substantiated and vetted tiers, an accept-all workflow, and an audit that rolls a tier back"
author: architect
timestamp: 2026-11-10T00:00:00Z
status: approved   # by the operator, 2026-11-10 ("approve spec 099 and spec 100"): the text as written, including that 040 section 15.4's audit is a roll-back trigger and not a promotion gate. OD-1 to OD-6 of section 14 take their recommendations. The tier numbers are hypotheses to calibrate, not pre-registered thresholds. The text takes effect when this pull request merges.
approved_by: operator (2026-11-10)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the tier numbers are hypotheses, nothing here has run, and the audit that is meant to calibrate them has not been run once
source_mission: 2026-11-10-session
tags: [spec, knowledge, validator, confidence, audit, rollback, draft]
---

# Spec 100: The confidence ladder

> **Status: approved by the operator, 2026-11-10.** It depends on **spec 098** (approved, merged in pull request #612; its OD-7 is open and does not affect this spec), sections 7.5 and 7.6, which define the `corroborated` status, the `trusted_sources` flag and the tags this spec reads. It amends [`specs/040`](040-validator-role-and-fact-ratification.md) §15.4 and §18.2, [`specs/052`](052-knowledge-status-capture-on-contact-and-the-digest.md) §3.1 and [`specs/066`](066-mcp-research-endpoint-and-role-identity.md) §5; each amendment is listed in §9 and takes effect when this pull request merges. **It widens no authority**: no tier is a taint level, and none unlocks promotion, tool access or approval.

## 1. Scope and objectives

The operator's aim (2026-11-10): **no learning is absolute, but a high degree of confidence is the goal.** Research should find more, keep what it finds, and say how well each claim is supported. As the knowledge base grows with accepted facts, that knowledge helps judge new learnings without expanding trust. Neither a human nor an agent is likely to do better than five independent sources with two trusted ones, so the strongest tier should be cheap for the operator to accept in bulk, and the operator should be able to find out afterwards whether that was a mistake.

Spec 098 ships the evidence: every claim carries its count of independent origins, its `trusted_sources` count, its contradiction state and its sources' track record, and a claim with three or more independent origins reads **corroborated**. This spec adds the tiers above that, a fast lane to the validator, an accept-all workflow, and the way back if they prove unreliable.

| Tier | Rule (defaults; hypotheses, §3.2) | What the operator reads |
| :-- | :-- | :-- |
| noted | fewer than 3 independent origins | `not validated` |
| corroborated | 3 or more independent, daemon-refetched origins (spec 098, section 7.6) | `not validated · corroborated (3 origins)` |
| **substantiated** | 3 or more origins, at least 1 a trusted source | `not validated · substantiated (3 origins, 1 trusted source)` |
| **vetted** | 5 or more origins, at least 2 trusted sources, **and** the validator's own checks pass | `vetted (5 origins, 2 trusted sources; awaiting your accept)` |
| validated | the operator accepts (one by one, or all vetted at once), or the validator ratifies in `ratify` mode | `validated` (unchanged, 040 §18.2) |

**What exists, so it is not rebuilt.** The validator's three modes `off`, `advisory` and `ratify`, whose `advisory` mode runs the whole fact path as a dry run and records a `would_ratify` outcome (040 §15.1). The blind operator audit, the latch, `retract` and `unquarantine` (040 §15.2, §15.3), and the commands `delphi validation audit`, `report`, `latch`, `retract`, `unquarantine`, `queue` and `score`. The status ladder of 052 §3.1 and the digest of 052 §3.3.

### 1.1 Non-goals

- **No new taint level and no promotion.** A tier is a statement of reliance for a reader (GOVERNANCE §4; 040 §7.1 and the 040 decision D-1). It never lets a claim into `REVIEW.md`, a rule, a lesson or a skill.
- **No pre-registered thresholds.** The numbers 3, 1, 5 and 2 are defaults for the operator to calibrate (§3.2, §8). 040 §15.4 refused to fix numbers before labelled data existed; this spec records the data and leaves the numbers movable.
- **No change to `[validator] mode = "ratify"`.** The validator ratifying by itself stays gated by PROMOTE as 040 §15.4 says (OD-1). The ladder does not use `ratify` mode at all: a vetted claim waits for the operator.
- **No change to what flows in.** Nothing is held back from the library because of its tier.

### 1.2 Simpler alternative considered

Ship only what spec 098 ships: the `corroborated` status plus the tags, and let the operator read the counts. Rejected as the whole answer, because the operator wants a cheap bulk path for the strongest claims and a fast lane for the next-strongest before release. It is, however, **exactly the state the roll-back returns to** (§7): if an audit shows a tier is unreliable, that tier switches off and the library reads as spec 098 leaves it.

## 2. Principles

1. **Confidence, not truth.** Every tier says what was counted and that it is weaker than `validated`.
2. **Tags first, tiers derived.** A tier is computed from stored tags by one table in code. No model assigns a tier.
3. **The queue is not the status.** The fast lane changes the order work is done in, never what a claim is.
4. **The audit is the safety, not the glance.** Showing the operator a few items before an accept-all is a convenience. The error bound comes from the blind audit.
5. **A way back that costs one command.** A tier that fails its audit is switched off by the daemon state, visibly, and nothing is silently deleted.
6. **Measure before believing.** The per-tier calibration report says how often each tier was later contested or disproved, with its sample size.

## 3. The ladder

### 3.1 The tags (from spec 098)

A claim stores: `origins` (distinct origin keys of daemon-refetched, quote-verified citations; origins by 040 §6.2), `trusted_sources` (below), `contradiction` (none, contested, or disproved), and `track_record` (055 §3.8: how many of an origin's earlier claims stood). The tags are recorded from the first day, whatever tiers are switched on.

**`trusted_sources: k`** counts the counted origins that are on the operator's trusted-source list within the topic scope the operator set for that entry (066 §5), or in a trusted source class, Primary or Reputable (040 §6.1). A Weak or Hostile class vetoes a list entry. The young-domain and burst rules of spec 098, section 7.6 apply. A source must have been on the list, or in the class table, before the claim was captured.

### 3.2 The table, and why the numbers are only defaults

The tier is the highest row whose condition holds and whose gates pass:

- **corroborated:** `origins >= corroborated_origins` (default 3).
- **substantiated:** corroborated, and `trusted_sources >= substantiated_trusted` (default 1).
- **vetted:** `origins >= vetted_origins` (default 5), `trusted_sources >= vetted_trusted` (default 2), **and** the validator has recorded `would_ratify` for the claim (§4).
- **Gates for every tier above `noted`:** the claim is a factual claim, not an attribution or a perspective (040 §19.3); it is not in a protected class (040 §9.2); no credible source contradicts it, and it does not contradict a validated fact (§6). Any of these makes it `contested` or leaves it `noted`; none lets it rise.

The four defaults live under `[research] tiers`, a ratified setting. They are a first guess. Nothing in the evidence yet says that five is safer than three or that two trusted sources beat one. The calibration report (§8) and the audit (§7) are how they are changed, and changing them is the operator's act.

## 4. Vetted is the validator's would-ratify, made visible

`vetted` is not a new check. It is 040's `advisory`-mode outcome shown to the operator:

1. A claim that reaches **substantiated** and has `origins >= vetted_origins` and `trusted_sources >= vetted_trusted` is queued for the validator (the fast lane, §5).
2. The validator runs the full fact path of 040 §7.2 as a dry run: the daemon fetches and stores the sources, the entailment judge (a different model from the claim's author) reads only the claim and the passage, the daemon runs the contradiction search, and the protected-class check applies. The result is recorded as `would_ratify`, with the same record as 040 §8.
3. The claim then reads **vetted**. It is still `not validated` underneath. Nothing is written to `context/facts/**`.
4. The operator's accept (§6) is the ratification, as an L5 act (040 §9.3).

In `off` mode there is no validator network access (040 §15.1), so no claim can be vetted and the top tier is substantiated. In `ratify` mode, if the operator has raised it through PROMOTE, the validator ratifies as 040 says and the ladder only feeds its queue.

## 5. The fast lane

A substantiated claim is queued for the validator ahead of a corroborated one, and a corroborated one ahead of a noted one, inside the background budget of 040 §18.3 and §20. **This changes queue order and nothing else.** It never changes a claim's status, taint, tier, promotion or tool access, and a test asserts it (extending spec 098, section 12 item 17 to every tier and to the fast lane). The caps of 040 §18.3 item 5 still apply: the per-mission card cap, the daily fetch cap, the pause while the audit backlog exceeds ten.

## 6. Accept all vetted

A decision of kind `learn` in the decisions pane (052 §3.3; 044 §4.3), grouped by topic, one row per claim, showing the claim, its origins and their domains, its `trusted_sources`, and the validator's `would_ratify` record.

- **Actions:** accept all, accept some (tickable), reject (disproved), skip. Nothing blocks work or times out into an acceptance (052 §3.3).
- **Excluded from accept-all:** protected classes (040 §9.2 items go to the operator as cards with the evidence attached), claims whose sources changed since the check (re-checked first), and any claim that is `contested`.
- **Marked `batch-accepted`.** An item accepted through accept-all carries that mark and the audit batch it belongs to, so a later disproof or a failed audit can trace it. An item accepted one at a time does not.
- **Reversible.** `delphi validation retract` and retirement work on a batch-accepted fact as on any other.
- **The glance is not the bound.** The workflow shows the operator three items with their quotes before an accept-all. That is a convenience. It bounds nothing, and the workflow says so.

## 7. The audit is a roll-back trigger, not a promotion gate

**Decided by the operator, 2026-11-10.** Council `t_0006` (on an earlier form of this ladder) advised the opposite: no tiers before PROMOTE, that is 60 clean blind audits from ten missions with no origin over 20% and an empty backlog (040 §15.4). **That advice is declined at the operator's direction.** The ladder ships live. The operator runs the audit by hand, once there is enough data to study, and a tier that fails is rolled back.

### 7.1 Running the audit

The audit uses the blind-audit machinery that exists (040 §15.3): the card shows the claim first, records the operator's own `true`, `false` or `unsure`, and only then reveals the sources and the verdict; `unsure` counts for nothing. What is new is the sample and what the result does:

- `delphi validation tier-audit [--tier substantiated|vetted|batch]` draws a random sample from the claims that reached that tier (or were batch-accepted), stratified by source class, and serves them as ordinary audit cards. It is operator-run, needs a terminal, and refuses inside an agent seat, like the other `delphi validation` commands.
- `delphi validation tier-report` prints, per tier: the sample size, the errors the operator found, the rule-of-three bound (3 divided by the sample, when there were no errors) or the observed rate with its confidence interval, the number of claims not yet audited, and whether the sample floor (§7.4) is met. It reads only daemon-written audit events, never text a span could influence (040 §15.3).

### 7.2 What triggers a roll-back

Any one of these, for the tier in question:

1. **The error rate.** After the sample floor, the upper bound of the tier's observed error rate is above the threshold the operator set. **The threshold has no default.** The operator sets it when first running the audit, with 040 §15.4's own figures shown as reference (zero errors in 60 audits bounds the error rate to about 5%).
2. **A hard failure.** One audited claim that was both `false` and reached **vetted** or was batch-accepted, **or** one protected-class claim that reached vetted, or one poisoned page that passed. These trip the tier at once, as a false audit trips the latch in 040 §15.2.
3. **The operator's say-so.** `delphi validation tier rollback <tier> --reason "<text>"`, logged as an operator act.

### 7.3 What a roll-back does

It uses the same pattern as the 040 latch: the daemon has no write path to the ratified `config.toml`, so a **tier latch** lives in daemon state and the effective tier set is the configured set minus the latched tiers.

- **The tier's label and fast lane switch off.** A latched `vetted` reads as `substantiated` (or `corroborated` if that is also latched); a latched `substantiated` reads as `corroborated`. The claims keep all their tags. Nothing is deleted or re-labelled in storage, so the tier can return.
- **Accept-all is disabled** while `vetted` is latched.
- **Items batch-accepted since the last good audit are flagged `re-audit due`.** They stay `validated`, because the operator accepted them, and stay searchable with the flag. They are **not** silently retired. The operator may audit them, or `retract` the batch (040 §15.2).
- **The roll-back is an event,** `tier.rollback {tier, trigger, sample, errors, bound, audit_batch}`, written by the daemon, shown in `delphi needs` and in the next `tier-report`.
- **Only the operator raises a tier again:** `delphi validation tier restore <tier> --reason "<text>"`, which starts a new audit batch. An agent cannot run it.

### 7.4 What the audit cannot show

- **A small sample bounds nothing.** Zero errors in ten audits is compatible with an error rate of about 26%. The report therefore always prints the sample size and the bound, and **refuses to call a tier "clean" below the sample floor** (default 30 audited claims per tier, which bounds a clean result to about 10%; 60 bounds it to about 5%; OD-2). It also states how many claims it did not audit.
- **It measures whether the claim is true, as the operator judged it,** not whether the sources agree. A wrong consensus passes the count and fails the audit, which is the point.
- **The sample is the operator's attention.** An operator who stops auditing leaves the ladder unchecked. The report prints the age of the last audit and of the oldest unaudited batch-accepted item, and `delphi needs` shows it.
- **Roll-forward is never automatic.** A good audit changes nothing by itself; the operator decides.

## 8. Calibration

`delphi validation tier-report` also prints, per tier, the share of its claims later `contested` or `disproved` (055 §3.8's counts, joined by tier at the time of capture), with the sample size. It is informational until there is volume; below the floor it prints the numbers without a verdict. This is how the defaults of §3.2 are changed on evidence.

## 9. Knowledge-base tags and the amendments

### 9.1 KB-prior tags

- **Support from the knowledge base counts only through the supporting facts' origins,** deduplicated by origin key, so a claim cannot gain weight by citing the facts it was derived from. A citation-graph **cycle check** applies: a claim derived from a corroborated or vetted claim adds no origin to itself.
- **A claim that contradicts a validated fact is `contested` and cannot rise.** Overturning a validated fact needs the operator.
- **Source track record is a tag.** It becomes a trusted source **only from operator-accepted history**, never from auto-vetted history, so a few auto-vetted facts cannot raise a source's standing and then vet more of its claims. This closes the circular-trust loop the Council raised.

### 9.2 Amendments (they take effect on approval)

1. **040 §18.2 (labels).** `vetted` is a new top-level operator label. `corroborated` and `substantiated` are shown as detail on `not validated`. Each tier's detail says it is weaker than `validated`. The 040 §23.1 table gains the rows.
2. **052 §3.1 (the ladder).** Gains `corroborated` (spec 098), `substantiated` and `vetted` as derived statuses between `noted` and `validated`, with their glyphs chosen under spec 053 in slice L1.
3. **066 §5.** Trusted-list membership may set the displayed `trusted_sources` flag within its topic scope (spec 098 makes the first part of this change) and, through this spec, may place a claim in the fast lane. It still never changes a claim's status or taint.
4. **040 §15.4.** The blind audit gains a second use: a roll-back trigger for the ladder (§7). PROMOTE keeps its role as the gate on `[validator] mode = "ratify"`.

## 10. Authority invariants (what the tests prove)

1. No tier, flag or fast-lane position is read as a criterion for promotion, trust, tool access or approval (a test over the code that reads them).
2. The fast lane changes queue order only; the same claim has the same status with or without it.
3. A tier is computed from stored tags by one table; no tool argument, host field or model output sets a tier.
4. Accept-all writes nothing except through the operator's act, marks every item `batch-accepted`, and skips protected classes, contested claims and claims whose sources changed.
5. A latched tier cannot be raised by an agent or by a config edit alone, and the latch survives a daemon restart (as 040 §15.6 item 4 requires of the validator latch).
6. A roll-back deletes nothing: after it, the tags and the claims are unchanged and the labels fall back one tier.
7. Every `delphi validation tier-*` command refuses inside an agent seat.
8. The audit report reads only daemon-written audit events and counts only `true` or `false` answers.

## 11. Slices (one pull request each; none starts before approval)

| Slice | Content | Hours | Depends |
| :-- | :-- | --: | :-- |
| (spec) | review, Council, amendments | 8-12 | spec 098, section 7.6 |
| L1 | the tier table in `delphi-core`, labels, glyphs, docs, the `[research] tiers` setting | 10-16 | spec 098 S1b |
| L2 | the fast lane (queue order only) and `would_ratify` shown as `vetted` | 12-18 | L1 |
| L6 | the roll-back: `tier-audit`, `tier-report`, `tier rollback` and `restore`, the tier latch and the event | 12-18 | L2 |
| L3 | the accept-all workflow (digest kind, `batch-accepted` mark) | 16-24 | L6 |
| L4 | the per-tier calibration report | 10-16 | L1 |
| L5 | KB-prior tags: support through origins, the cycle check, the contradiction block, track record from operator-accepted history | 16-24 | L1 |

Total build about 76 to 116 hours, plus the spec. **L6 comes before L3 on purpose:** accept-all does not go live until the way back exists. The hours are my estimates, not measurements.

## 12. Acceptance criteria

1. A claim with the tags of §3.2 reads the tier the table gives, for each row, from fixtures; a claim that fails a gate (attribution, protected class, contradiction) never rises.
2. A claim reaches **vetted** only after a `would_ratify` record, and in `off` mode none does.
3. The fast lane reorders the validator queue and a test shows the claim's status is the same with it removed.
4. Accept-all lists vetted claims by topic, marks accepted items `batch-accepted`, and skips protected, contested and changed-source claims.
5. `tier-audit` serves cards for a stratified sample; `tier-report` prints the sample size and bound and refuses a "clean" verdict below the floor.
6. A hard failure, or an error rate whose bound is above the operator's threshold, latches the tier: the label falls back, accept-all is disabled, batch-accepted items since the last good audit read `re-audit due`, a `tier.rollback` event is written, and nothing is deleted.
7. The latch survives a restart; an agent cannot raise it; `restore` needs a reason.
8. A claim that cites facts derived from itself gains no origin; a claim contradicting a validated fact is contested; a source becomes a trusted source only from operator-accepted history.
9. The calibration report prints per-tier contest and disproof rates with sample sizes and no verdict below the floor.

## 13. Verification Plan

Every item is verified without a network or a model, plus the repository gates of `AGENTS.md`.

| Item | How |
| :-- | :-- |
| Tier table (criterion 1) | Unit tests in `delphi-core` over a fixture table of tags, including each gate. |
| `vetted` and `off` mode (2) | An engine test with the validator fake: `would_ratify` recorded gives `vetted`; mode `off` gives none. |
| Fast lane (3) | A queue-order test and a same-status test with the lane removed; the "grants nothing" test of §10 item 1. |
| Accept-all (4) | A decisions-pane test over a fixture batch with a protected, a contested and a changed-source claim. |
| Audit and report (5) | Tests over synthetic audit events: sample size, rule-of-three bound, refusal below the floor. |
| Roll-back (6, 7) | A drill: seed an audit with a hard failure; assert label fallback, accept-all disabled, flags, the event, a restart, and that an agent seat is refused. |
| KB tags (8) | A cycle fixture; a contradiction fixture; a track-record fixture with auto-vetted history that must not count. |
| Calibration (9) | A report golden over synthetic outcomes. |
| Gates | `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`, and `delphi check spec` on this file. |

Nothing here is verified by a live run.

## 14. Decisions and open questions

**Decided by the operator, 2026-11-10:**

- The ladder: corroborated, substantiated, vetted, with `trusted_sources` as the extra tag and `vetted` meaning the validator's checks pass.
- The audit of 040 §15.4 is a **roll-back trigger, not a promotion gate**; the operator runs it by hand once there is data to study.
- Vetted claims are accepted in bulk by the operator ("accept all vetted"), and called *vetted*, not *trusted*.
- The tier numbers are to be calibrated, not pre-registered.
- The ladder is a separate spec from 098 and is built before release.

**Open (recommendation in brackets):**

- **OD-1.** Should the audit also replace PROMOTE as the gate on `[validator] mode = "ratify"`? [No. The ladder never uses `ratify` mode; leave that gate as 040 §15.4 has it.]
- **OD-2.** The audit sample floor per tier. [30 audited claims for a clean bound near 10%, 60 for about 5%; the report prints the bound either way.]
- **OD-3.** Is accept-all on from the first day, before any audit? [Yes, as decided; the tier report prints "unaudited" prominently until the first audit.]
- **OD-4.** The default roll-back threshold. [None; the operator sets it at the first audit with 040 §15.4's numbers as reference.]
- **OD-5.** The default numbers 3, 1, 5 and 2 (§3.2). [Keep as defaults; change on calibration.]
- **OD-6.** What a latched `vetted` falls back to. [`substantiated`, one tier at a time.]

## 15. Council record

Council `t_0006` (2026-11-10) reviewed an earlier form of this ladder and advised shipping only `corroborated` plus the `trusted_sources` flag, with the rest behind PROMOTE. Its record is `missions/host-a4cc691a99d9ea4e/councils/005-…` in the Delphi home. How this draft answers it:

| Finding | Answer |
| :-- | :-- |
| Batch accept samples three items and bypasses 040 §15.4 PROMOTE (2 P0s) | **Declined at the operator's direction.** The audit is a roll-back trigger (§7). The sampling objection is accepted and answered: the glance is not the error bound (§6), the audit is, and a failed audit rolls the tier back. |
| Trusted list as an anchor contradicts 066 §5 and can self-reinforce (3 P0s) | §3.1 and §9.1: topic-scoped, Weak/Hostile veto, a source must predate the claim, track record counts only from operator-accepted history; 066 §5 is amended explicitly (§9.2). |
| Four tiers are scope creep in a ten-slice spec (P0) | This is its own spec, built in its own slices. |
| 3 and 5+2 are invented numbers (3 P1s) | They are defaults, recorded from day one and changed on calibration (§3.2, §8). |
| Circular trust through KB support (P1) | §9.1: support through origins only, with a cycle check. |
| Extend the "grants nothing" test to every tier and the fast lane (P1) | §10 items 1 and 2. |
| Tier names read as "true" (P2s) | §9.2 item 1: every tier's detail says it is weaker than `validated`. |
| The calibration report measures nothing at low volume (P2) | §8: no verdict below the floor. |

## 16. Known limits and what is not verified

- **No live run.** Nothing here has run against a real library or a real audit.
- **The numbers are untested.** Whether three, one, five and two are the right defaults is exactly what the audit and the calibration report are for.
- **The audit is only as good as the operator's attention and the sample size** (§7.4).
- **The judge's cost.** Whether the entailment judge's token cost is acceptable at the volume the fast lane creates is not measured; the caps of 040 §18.3 and §20 apply.
- **A wrong consensus can pass every count.** Several independent sources can repeat one error; that is why the tiers read "weaker than validated" and why the audit exists.
- **Spec 098 is still a draft.** This spec's tags and `corroborated` status are defined there; if section 7.6 of that spec changes, this one follows.
