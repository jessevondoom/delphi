# M7: is `discovered` assignment no worse than `static`? (pre-registration)

`specs/056` §10.6, slice M7. Written and committed **before any run and before any calibration spend** (§10.6: M5's paid
calibration run does not start until this file is on `main`). Registered 2026-11-22. The definition below is fixed; changing any
of it after a run starts voids the benchmark (register a new one and say so). A FAIL may be re-run once, only under a new
pre-registration with fresh seeds and 6 runs per case, written before the re-run.

## Machine-readable form

- `dev/train/m7/benchmark.toml`: the twelve cases, 3 runs per arm, the token caps (600,000 code, 400,000 research, 150,000
  review), the criterion's numbers (margin 2, ratio 1.25, low contrast below 25 %) and the review seeds. A test
  (`train_gate::tests::the_pre_registered_benchmark_is_the_one_section_10_6_defines`) checks the file against this text and the
  text below against the spec, word for word.
- `dev/train/m7/arms.example.toml`: the two arms, S (`[arm.routing] assignment = "static"`) and D (`"discovered"`), one template
  home, the same `expect_models`.

## Review cases: seeds and the commands that make them

Four medium seeded-defect cases, holdout split, inline, made **after this file is on `main`** and never before. These seeds are
fresh (no earlier experiment used them) and a calibration run (M5) must not use them:

```sh
delphi train defects gen --seed 56071 --difficulty medium --inline --split holdout --out dev/train/suites/review/cases/m7-rev-56071
delphi train defects gen --seed 56072 --difficulty medium --inline --split holdout --out dev/train/suites/review/cases/m7-rev-56072
delphi train defects gen --seed 56073 --difficulty medium --inline --split holdout --out dev/train/suites/review/cases/m7-rev-56073
delphi train defects gen --seed 56074 --difficulty medium --inline --split holdout --out dev/train/suites/review/cases/m7-rev-56074
delphi train validate
```

The defaults of `gen` stand: 10 planted defects and 8 decoys, `review_findings` with `min_recall = 0.5` and
`max_false_positives = 2`.

## The run (operator only; it spends tokens and premium requests)

```sh
delphi train run --bench dev/train/m7/benchmark.toml --arms-file <dir>/arms.toml --no-session-cap --work-dir /tmp/m7 --dry-run
delphi train run --bench dev/train/m7/benchmark.toml --arms-file <dir>/arms.toml --no-session-cap --work-dir /tmp/m7
delphi train gate --bench dev/train/m7/benchmark.toml --work-dir /tmp/m7
```

The first command prints the token ceiling and the premium-request multipliers and starts nothing. The second prints them again
and runs only on a `yes` typed at the terminal. Runs not graded for quota or sleep are re-run with `--resume` (the yes is asked
again). The verdict, the date, the model versions served and the contrast go into the tracker.

## How the runner reads §10.6 (decisions made in building M7)

- **Tokens** are input + cache read + cache write + output of every call (§10.3), without a multiplier; thinking tokens a
  carrier reports apart are not in the unit. Tokens per passing run = an arm's tokens over its graded runs / its passes.
  Premium requests are the `premium_requests` the Copilot seat reported (`harness.cost`).
- **The cap** is read from the run's own logs every 5 seconds while it runs; a run past its cap is stopped and recorded graded,
  not passed. A run that ends between two looks but over its cap is failed the same way.
- **Criteria 2 to 4** are judged only when criterion 1 holds; with runs missing they are "not judged" and the verdict is
  INCOMPLETE unless criterion 5 fails. **Criterion 4** cannot hold when S passed no run (no base): the verdict is then INCOMPLETE.
  When D passed no run and S did, it fails.
- **Criterion 5** counts every D row, first runs and re-runs alike: a `model.assigned` whose class is below the floor it names
  (or `emergency`), a served `(role, model)` over that role's `max_cost_class` in the run's own configuration, or a run ungraded
  for `arm staffing violated`.
- **The contrast** pairs D's dispatches (the first model of each span) with S's for the same case and run, role by role in order.
- **Both arms list every pool model** in `expect_models`; the runner refuses a template whose pool has a model neither lists.

## The definition (copied word for word from `specs/056` §10.6)

**When.** This definition is fixed by this amendment, so it exists before any calibration spend. M7's pre-registration file (under `dev/train/`, committed before any run, as E1 to E3 did) copies it word for word. M5's paid calibration run does not start until that file is on `main`. The gating run happens after M4 and M5 are built and calibration has run. **The default stays `static` until this gate passes and the operator then decides (OD-5).** A pass is necessary, not sufficient.

**Arms.** Two plain arms of `delphi train run` (spec 037 §2), built by `delphi train arms init` from one source home, so they have the same ratified pool, endpoints, tiers, grades, `max_cost_class` and template files. Arm **S** has `[routing] assignment = "static"` and arm **D** has `"discovered"`. Nothing else differs; M7 adds an `[arm.routing]` table, the way harness arms set `[arm.loop]`, so the runner enforces that. Both arms list every pool model in `expect_models`, because D may choose any of them.

**Suite: 12 cases, all existing today or generated by an existing command.**

| Suite | Cases | Role (what discovered changes) | Scored by |
| :-- | :-- | :-- | :-- |
| `code` (037 C1 to C3) | `agy-builder-ready`, `ollama-keyless`, `snapshot-loaded` | `builder`, plan-following (floor `quick`, preferred `steady`) | hidden tests and gates |
| `research` (037 R1 to R3, R5, R6) | `motogp-2027-regs`, `mycorrhizal-networks`, `phantom-paper`, `lions-mane-claim`, `livewire-strategy` | `strategist`, plan-making (preferred `deep`) | the deterministic checks only (`fact_recall`, `no_traps`, `text_matches`); no jury; packs verified 2026-10-26 and re-verified before the run (037 §11) |
| `review` (048 seeded defects) | four cases from `delphi train defects gen --difficulty medium --inline --split holdout`, seeds fixed in the pre-registration and disjoint from every calibration seed | `red-team`, plan-making | the `review_findings` check (defaults: recall at least 0.5, at most 2 false positives) |

Not used: `spec124` (it tests one tool's output, not a routing choice), `orchestration` and `role-behavior` (one case each, testing control flow), `easy` review cases (on 2026-11-08 every arm scored 0.95 to 1.0 on them, a ceiling), and `hard` ones (their prompt leaves the inline form).

**Runs, caps and seeds.** Each case runs **3 times per arm** (the harness calls a repetition a seed; carriers expose no model seed). That is 12 × 2 × 3 = **72 runs**. The harness's caps today are in dollars (`max_cost_usd`, `--budget-usd`), and a subscription seat reports $0.00 (E1, 2026-11-05), so a dollar cap does not bind. **M7 adds a per-run token cap**, enforced by the runner on the run's summed tokens: **600,000** for a code case, **400,000** for a research case, and **150,000** for a review case. For comparison, E1 measured medians of about 150k to 320k tokens per research run, and the code and review figures are estimates the first runs will check. A run stopped at its cap counts as **not passed** in both arms. The worst case is 18 × 600k + 30 × 400k + 24 × 150k ≈ **26.4M tokens**. The command prints that ceiling, and premium requests from the catalog multipliers, before anything runs, and runs only on the operator's yes for that run.

**Metrics.** Per arm: **pass rate** (graded runs that passed every check, over graded runs), and **accepted results per cost**, reported as tokens per passing run and, where a Copilot seat served, premium requests per passing run. Also reported, but not in the criterion: per-case passes, the share of D's dispatches whose model differs from what S chose for the same case and seed (the *contrast*), `model.cost_capped` events, the SEATS SERVED and FAMILY COMPARISONS sections (README), and the EFFORT columns (048 BM-2).

**Pass criterion (all five must hold).**

1. **Complete:** at least 3 graded runs per arm per case (runs not graded for quota or sleep are re-run with `--resume`). Otherwise the verdict is INCOMPLETE, not a pass.
2. **Pass rate no worse:** D passes **at least S's passes minus 2**, out of 36 graded runs each.
3. **No case lost:** there is no case where S passes 3 of 3 and D passes 1 or 0 of 3.
4. **No more than 25 % costlier:** D's tokens per passing run are **at most 1.25 times** S's, and so are its premium requests per passing run when either arm used any.
5. **Safe:** in D, no dispatch below its role's floor (from `model.assigned`), no served model above its cap, and no run left ungraded because an unexpected model served.

The verdict is **PASS** if 1 to 5 hold, **FAIL** if 2, 3, 4 or 5 fails, and INCOMPLETE otherwise. It goes into the tracker with the date, the model versions served and the contrast.

**Why these numbers.**

- *N = 25 %* is the threshold the harness already uses for "costlier, not better" (048 OD-2, `COST_FLAG`), so one instrument has one bar. E1 also found its two variants' tokens within 25 % in 4 of 5 cases, so a tighter bar would sit inside run-to-run spread. Note that 056 §3.2 *intends* D to spend more on plan-making roles and less on plan-following ones, so the bar is on the total, not per role.
- *S = 3 runs per case* is the harness's minimum for any evidence (README: "no evidence" below three). Six would double the cost (about 53M tokens at the ceiling) to cut the noise by only √2.
- *A margin of 2 runs* (about 6 points) is tighter than the noise. With 36 runs per arm and pass rates near one half, one standard error of the difference is about 4 runs.

**What the gate cannot show.**

- It fails by chance about **one time in four** when the arms are truly equal. That is a safe failure, because the default stays `static`. A FAIL may be re-run once, only under a new pre-registration with fresh seeds and 6 runs per case, written before the re-run.
- It passes by chance about **one time in three** when D is truly 4 runs worse (11 points), and about **one time in ten** when D is 8 runs worse (22 points). It catches gross regressions, not small ones, and it never shows that the arms are *equal*.
- Twelve cases from one repository, five research topics and synthetic defects describe these tasks, not Delphi's work in general (037 §11). The deterministic checks are lower bounds. A seat on a carrier with no tools cannot take a code case, and a research arm whose carrier lacks web tools is measuring tooling. The FAMILY COMPARISONS section marks such runs CONFOUNDED, and they are read that way.
- If the contrast is under 25 % (D mostly picks what S picks), the result is labelled **low contrast**. A pass then shows only that D does no harm where it rarely differs.
- Results hold for the model versions served on that date (037 §11). A new tier or model reopens the question (048 §10, "re-arm on each model tier").
