# E1 result (run 2026-11-05)

Pre-registration: [`PREREG.md`](PREREG.md), committed as `4ac6bb0` before the first run. Raw rows: [`rows.jsonl`](rows.jsonl). The analysis below is
[`analysis.txt`](analysis.txt), produced by `python3 -I dev/train/e1/analyze.py dev/train/e1/rows.jsonl --arm solo-gemini-flash`.

## Verdict (the pre-registered rule, unchanged)

**NO EVIDENCE of an effect on passes.** Contract 6 of 15, terse 7 of 15: a difference of -1 against a threshold of 4, with 1 of 5
cases pointing the same way as the total. This is not evidence that the contract does nothing. By the rule, **nothing in spec 082
section 8 is built.** S1c and S2 stay, because they were approved as shape checks and not on this result.

## What ran

30 runs, all graded: 5 research cases x 2 variants x 3 seeds, one seat (`solo-gemini-flash`, Gemini 3.8 Flash medium). Only that
model made calls in any run; no run errored; none was excluded; none was repeated. Median tokens per case and variant ranged from about 150k
to 320k; the seat reports `$0.00`.

| case | terse | contract | d | median tokens terse -> contract | fact recall terse -> contract |
| :-- | --: | --: | --: | :-- | :-- |
| lions-mane-claim | 3/3 | 0/3 | -3 | 215,129 -> 171,576 | 67% -> 70% |
| livewire-strategy | 0/3 | 0/3 | 0 | 280,319 -> 319,660 | 28% -> 22% |
| motogp-2027-regs | 3/3 | 3/3 | 0 | 284,519 -> 281,509 | 76% -> 88% |
| mycorrhizal-networks | 0/3 | 0/3 | 0 | 233,378 -> 149,811 | 50% -> 30% |
| phantom-paper | 1/3 | 3/3 | +2 | 233,553 -> 175,685 | 91% -> 100% |
| **total** | **7/15** | **6/15** | **-1** | | **62% -> 62%** |

Secondary (never decides): tokens were within 25% in 4 of 5 cases and lower in one. Mean fact recall was 62% in both variants.

## Exploratory observations (after the fact; 3 runs per cell; they decide nothing)

The total hides two opposite movements, each in a different place:

1. **Fabrication fell where the request rested on a false premise.** In `phantom-paper` (an invented citation), 2 of 3 terse runs
   fell into the trap "invented numbers about it"; none of the 3 contract runs did. This is the stop condition doing what it is for
   ("say what you could not verify and stop there"), and the pre-registration said an effect here is partly by design.
2. **Citations fell.** Failures of each case's check that wants a source in the answer (a URL, DOI, PMID or filing, depending on
   the case) were 2 of 15 terse runs and 7 of 15 contract runs. `lions-mane-claim` went from 3/3 to 0/3 **with fact recall at 70%**: it failed only that citation check.
   `mycorrhizal-networks` missed the citation pattern in 3 of 3 contract runs against 1 of 3 terse. A plausible reading is that
   "one written answer" and "the request above only" read as a reason to drop sources; that reading is a guess, not a finding.

The pre-registered bias about patterns tuned on terse answers (PREREG, known bias 1) does not explain item 2: the citation check is a
real requirement of those cases, and the contract variant did not meet it.

Two cases (`livewire-strategy`, `mycorrhizal-networks`) failed on recall in both variants and had no headroom, as bias 3 expected.

## What this means for spec 082

- The block is not harmless. It helped on one failure mode and hurt on another, and five cases with three seeds cannot say
  which effect would win on other tasks.
- That the **wording of each clause matters** is the one thing this run supports. If the question is worth another experiment, it
  needs a **new pre-registration** with the clauses separated (for example: stop condition alone; stop condition plus deliverable
  that names sources; the full block), more cases, and the citation requirement stated in the scope clause. Nothing here authorizes
  building it; that is the operator's call.
- `check brief` reports presence only and does not recommend any wording, so nothing in S2 contradicts this result.
- The two exploratory movements are hypotheses. They would need their own pre-registered test before any rule, lint or
  skill text relied on them.

## Limits

One seat, one model family, five cases, one afternoon, 15 runs per variant. A difference of 4 passes was the smallest the rule acted
on, and even that would not have been significant on its own (p about 0.26, two-sided). The verdict is about this setup only.
