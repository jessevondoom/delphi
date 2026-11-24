---
state: built
date: 2026-10-30
title: "spec 057 §5.1 (057-A1, council 012): gather counters, parity fixtures, baseline measurement"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 377, under `The central library (spec 055, approved: ratified 2026-10-29)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| SK8e | spec 057 §5.1 (057-A1, council 012): gather counters, parity fixtures, baseline measurement | built 2026-10-30 (`GatherStats` on `Gathered`, also in the `pages.results` reply as `gather`; `stored_text_measured`; parity fixtures in `library_tests.rs`; ignored `gather_baseline_report`). Baseline, release build, synthetic library with 2% large documents: 100 sources 15 ms; 1,000 sources 151 ms (meta list 25%, read 13%, hash 9%, tokenize 30%); 3,000 sources 479 ms (same shape). Read, hash and tokenize are about 51% of gather, under the 70% bar |
