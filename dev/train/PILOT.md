# Running the pilot (`specs/037` phase 0)

The commands, in order, with what each costs. Details of every command are in `README.md`; what the study is for is in
`specs/037`. Numbers marked *measured* come from real runs on 2026-10-26; the rest are extrapolations and say so.

## Before anything runs: your part

1. **Review the five research packs** (`README.md`, "Reviewing a truth pack"): open `suites/research/cases/<id>/truth/review-sheet.md`
   for `motogp-2027-regs`, `mycorrhizal-networks`, `phantom-paper`, `lions-mane-claim`, `livewire-strategy`. Check facts
   against their sources, then write your own `operator_view` for each judgement question in `truth/insights.toml`
   (`livewire-strategy` leans on this most). Stamp each with `delphi train verify --case research/<id> --by "<name>"`.
   `--allow-unverified` runs a draft, and the report then says so; it is for rehearsal, not for the study.
2. **Accept or tighten the build policy.** Code cases run the Builder with `--permissions accept-edits` and a default card
   policy that lets it run `cargo`, `git`, `timeout` and a few read-only programs, unsandboxed as you, in a throwaway
   worktree of a repository with no remote. Pass `--policy <file>` for something tighter.
3. **Keep the machine open, plugged in and awake.** `train run` and `train judge` hold off idle sleep themselves; a lid
   closed on battery still sleeps, and a run that slept is refused rather than graded (`--resume` retries it).

## Set up (about 2 minutes, no model cost beyond one-token probes)

```sh
python3 -I dev/train/avail.py --workspace <name> --live --json /tmp/avail.json    # who answers right now; ~40 s
delphi train arms init --source-home ~/.delphi --workspace <name> --defs dev/train/arms-defs.toml \
    --avail /tmp/avail.json --out /tmp/pilot-arms                                  # arms.toml and judges.toml
```

An arm whose model is exhausted is skipped with its reset time (on 2026-10-26: `solo-gemini-argon` until 10-30 06:50Z).
Check that `judges.toml` has two judges of different families; with one, that judge's own arms go unjudged.

## Phase 0, research cases first (cheap, fast)

```sh
delphi train run --suite research --arms-file /tmp/pilot-arms/arms.toml --work-dir /tmp/p0 \
    --workspace <name> --no-session-cap --seeds 1 --pace-window 0.85
```

*Measured, one seat each:* a research case takes **30 to 55 s on Haiku** (42k to 153k tokens in, 1k to 4k out) and **30 to
95 s on Gemini Flash** (42k to 279k in, 3k to 10k out). Sonnet, Opus, Fable and the team and cheap-first arms have not been
timed on research cases; a team run does several seat calls, so expect minutes. A guess for 9 arms x 5 cases is **1 to 3
hours**, serial. Subscription seats report `$0.00`; tokens are the cost measure here.

`--work-dir` must be short (at most 59 characters). Results are appended to `/tmp/p0/rows.jsonl` as each run ends; if it
stops for any reason, run the same command with `--resume`.

What to look at first: `notes` on each row (staffing: a run in which a model you did not declare made a call is not graded),
`error`, and runs graded `passed: null`. Note that a run only fails the deterministic checks if it misses facts or falls
into a trap; both cheap models pass most research cases on those checks alone (Haiku 4 of 5 and Flash 4 of 5, both failing only
LiveWire; but the patterns were tuned on those same ten answers, so that is a fit and not an independent test), so **the jury
is what separates arms on research**. The first real new answers are the check on the calibration: if a correct answer is
marked missing, say so, and the pattern is widened in the same way.

## Judge, then you

```sh
delphi train judge --work-dir /tmp/p0 --judges-file /tmp/pilot-arms/judges.toml --no-session-cap
delphi train review --work-dir /tmp/p0        # you, blind, before seeing the jury; about 10% of pairs come back flipped
delphi train disputes --work-dir /tmp/p0      # where you and the jury differ
delphi train resolve --work-dir /tmp/p0 --id <id> --cause rubric_gap --note "..."
delphi train report --work-dir /tmp/p0 --judges-file /tmp/pilot-arms/judges.toml
```

Pairs are judged in both orders by every judge that does not share a model with either answer. Judge-call time and tokens
have **not been measured** at scale (a handful of calls, each about as long as a research run). Expect your own review of the
pairs to take the 5 to 8 hours the spec set aside, not minutes.

## Phase 0, code cases (the long part)

```sh
delphi train check-hidden --case code/agy-builder-ready      # once per case: fails on the base commit, passes 3x on the reference
delphi train run --suite code --arms-file /tmp/pilot-arms/arms.toml --work-dir /tmp/p0code \
    --workspace <name> --no-session-cap --seeds 1 --pace-window 0.85
```

*Measured, C1 (`agy-builder-ready`) once each:* **Haiku 973 s, 1 build attempt, 993k tokens in, 22k out; Sonnet 1,032 s, 3
attempts, 1.13M in, 7.8k out.** Both passed. Most of the 16 minutes is the project's own compile and test gates, not the
model. C2 and C3 have not been run live (C3 has the most hidden tests). A guess for 9 arms x 3 cases is **7 to 10 hours**,
serial, and it crosses at least one Claude five-hour window.

Claude's windows are shared with your own work. `--pace-window 0.85` checks the CLI's own utilization report before each
Claude run: at 85% of the five-hour window it waits for the reset (up to 6 hours, `--pace-max-wait-hours`), and at 90% of the
seven-day window it stops the study and lists what was left, because a weekly reset is too far to wait for. On 2026-10-26 the
seven-day window stood at 63% used (reset 10-30 13:00Z), so a full Claude code pass on a busy week may need splitting. Gemini
and Ollama arms are never paced; run them while Claude is waiting by making a second `--arms-file` with only those arms.

## If something looks wrong

| you see | it means |
| :-- | :-- |
| `the machine slept for about N s` | the run is not graded; `--resume` retries it |
| `daemon exited before it was ready ... SUN_LEN` | `--work-dir` is too long; use `/tmp/p0` |
| a row with `passed: null` and an `error` | an infrastructure fault, not a model result; read `rNNN/stderr.txt`, then `--resume` |
| `pace: stopping: the seven-day window is N% used` | the study stopped on purpose; the skipped runs are listed |
| `already holds results; add --resume` | that work dir has rows; resume it or use a new one |
