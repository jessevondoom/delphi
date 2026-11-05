# E1: does a contract-complete brief change outcomes? (pre-registration)

`specs/082` section 7. Written and committed **before the first run**. The block text, the cases, the seat, the counts and the
decision rule below are fixed; changing any of them after a run starts voids the experiment (start a new one and say so).
Registered 2026-11-05.

## Question

For a cheap model seat, does a brief that states a deliverable, a done criterion, a stop condition, a scope bound and a tool
dependency produce different deterministic outcomes than the same brief without them?

## Design

- **One seat:** `solo-gemini-flash` (Gemini 3.8 Flash, medium, on every role), through the `agy` subscription seat. Chosen over Haiku
  so the study does not draw on the Claude window the operator shares with their own work. `expect_models` is declared, so a run
  in which any other model made a call is not graded.
- **Two variants of each case**, run in one invocation so they interleave in time:
  - `<id>-t` (terse): the research case's prompt exactly as shipped.
  - `<id>-c` (contract): the same prompt, a blank line, then this block, **identical for every case**:

    ```
    Deliverable: one written answer to the request above.
    Done when: every part of the request is answered, or marked as something you could not answer.
    Stop when: the request depends on something you cannot find or verify. Say what you could not verify and stop there, instead of filling the gap.
    Scope: the request above only.
    Tools: use only the tools you have in this session, and say so if one you needed was missing.
    ```
- **Cases: the five research cases** (`lions-mane-claim`, `livewire-strategy`, `motogp-2027-regs`, `mycorrhizal-networks`,
  `phantom-paper`), whose truth packs are operator-verified (2026-10-26) and copied byte for byte. **3 seeds each.** 5 cases x 2
  variants x 3 seeds = **30 runs**, 15 per variant.
- Built by `dev/train/e1/make-scratch.py` into a scratch repository outside the codebase; run with `delphi train run`; analysed by
  `dev/train/e1/analyze.py`. No model judges anything: **no jury is used.**

### Deviations from spec 082 section 7 as written

1. Section 7 said 10 cases. The 037 pack has only **five** cases that are short and checked deterministically. The three code
   cases take about 16 minutes and a million tokens each and do not turn on how a brief is worded; the orchestration and
   role-behavior cases test Delphi's own behavior, not a brief. So the experiment is five cases, which makes it **less able to
   detect a small effect**. This is stated in the decision rule, not hidden.
2. Section 7 said "a Haiku or Flash seat". Flash was chosen (above).

## Outcomes

- **Primary:** a run passes when the harness's own deterministic checks all pass (`fact_recall` at the case's minimum, `no_traps`,
  `text_matches` where a case has one). Nothing is re-graded by hand.
- **Secondary, reported and never deciding:** mean fact recall; median tokens (in + out) per variant and per case.

## Decision rule (mechanical; `analyze.py` prints it)

Let `d` = (contract passes) - (terse passes) over the 15 + 15 graded runs, and let `same` = the number of the five cases whose
own pass-count difference has the same sign as `d`.

- **Effect:** `|d| >= 4` **and** `same >= 3`.
  - Effect with `d > 0`: the contract helps. The follow-ups of section 8 may be *drafted* (still each needs approval).
  - Effect with `d < 0`: the contract hurts. Reconsider S1c and S2 before anything further; build nothing from section 8.
- **Otherwise: NO EVIDENCE of an effect on passes.** This is not evidence of no effect. Nothing in section 8 is built. S1c and S2
  stay, because they were approved as cheap shape checks and not on this result.
- **Ceiling:** if the terse variant passes 12 or more of 15, an improvement of 4 cannot be shown; only a decline can. The
  analysis says so.
- **Incomplete:** if any case has fewer than 3 graded runs in a variant, no verdict is given until the missing runs are re-run
  (`--resume`).

### What this can and cannot show

With 15 runs per variant, a difference of 4 passes (27 points) is the smallest this rule acts on, and even that is not
statistically significant by itself (11 of 15 against 7 of 15 is p of about 0.26, two-sided, by Fisher's exact test). The rule is a
decision threshold, chosen so that a small or noisy difference does not trigger more building; it is not a significance test.
Do not read a "NO EVIDENCE" as "the contract does nothing".

## Exclusions and run handling

- A run is **graded** if it has a verdict (`passed` is true or false), no `error`, and no staffing note (a model the arm did not
  declare made a call). Anything else is **excluded**, listed in the report, and re-run once with `--resume`. A run is never
  dropped for having a result the experimenter dislikes.
- No run is repeated to replace a graded result. No seed is chosen. No case is added or removed after the first run.
- The experimenter looks at the table only after all 30 runs are graded.

## Known biases, written down now

1. The `text_matches` and fact patterns were tuned on terse-prompt answers from earlier pilots (PILOT.md says so: "a fit and not an
   independent test"). Answers to the contract variant may phrase the same facts differently and be under-credited. That biases
   against the contract. Fact recall is reported beside the pass count so a drop in passes with steady recall is visible.
2. The block's stop condition ("say what you could not verify") is relevant to the false-premise cases (`phantom-paper`,
   `lions-mane-claim`). An effect there is partly by design, not a surprise.
3. Earlier pilots had Flash fail `livewire-strategy` on the checks regardless of variant, which can leave that case without
   headroom. The per-case table shows it.
4. One seat, one provider family, five cases, one date. Any result is about this, not about models in general.

## How it is run

```sh
python3 -I dev/train/e1/make-scratch.py --repo . --out /tmp/e1scratch
python3 -I dev/train/avail.py --workspace default --live --json /tmp/avail.json
delphi train arms init --source-home ~/.delphi --workspace default --defs dev/train/arms-defs.toml --avail /tmp/avail.json --out /tmp/e1arms
# arms-flash.toml: the single `solo-gemini-flash` arm from /tmp/e1arms/arms.toml
delphi train run --suite e1 --arms-file /tmp/e1arms/arms-flash.toml --work-dir /tmp/e1run --workspace default \
    --no-session-cap --seeds 3 --budget-usd 200 --repo /tmp/e1scratch
python3 -I dev/train/e1/analyze.py /tmp/e1run/rows.jsonl
```

Subscription seats report `$0.00`; tokens are the cost measure. Expected time: 30 to 95 s per run (PILOT.md), so roughly 15 to
50 minutes.
