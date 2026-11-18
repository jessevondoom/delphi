# Spec 124 S12: evidence for the new strategist and architect skills

Run 2026-11-18. Written for a person to read before anyone says the skills of spec 124 do anything. It covers the twelve skills
in `skills.toml` in this directory, the two checks (`strategy_trace`, the "Options considered" rule) and the capture step. The
three strategist skills of slice S8 merged after the model runs: they have the lexical trigger test and nothing else. Every
model run below was made before they merged, with nine skills and with `competitive-swot-analysis` still on the strategist's list.

**Verdict:** the checks and the cases work as instruments; on one small model the capture step was followed (6 of 6 candidate runs staged exactly one note, 0 of 6 baseline runs did) and `strategy_trace` was relayed correctly when asked for (6 of 6); the data does NOT show that any new skill improves a strategy or a spec, and the architect did not write the three kinds of option unless it was told to load the skill (0 of 3 without, 3 of 3 with).

Nothing here was pre-registered, and no person has read the answers. Treat every number as exploratory.

## Method

Two kinds of evidence, kept apart:

1. **No model.** Tests that run in `cargo test` on every change. They show that a check finds what was planted and passes what
   is clean, and that a case's own checks tell a right answer from a wrong one. They say nothing about what a model does.
2. **One model.** `delphi train run` on the cases of `dev/train/suites/spec124`, graded only by the cases' deterministic
   checks (events in the run's log, patterns in the answer, the structure of a section). No model graded anything, and no
   rubric was used. The seat was Claude Haiku (the carrier reported `claude-haiku-5-5` for all 27 runs; no other model made a
   call). It is the cheapest seat that was available.

The trigger tests have a third, small part: one call for each of three roles to Gemini Flash through `agy`, asking which skill each
task should load.

Who wrote what: the cases, prompts, checks and this page were written by one agent session (a Claude model) on the day of the
run, after reading the skills. No seat of another family generated the cases and no person curated them, so by `specs/031`
§4 they are drafts. The seat under test is of the same family as the author of the cases.

## Arms

| Arm | What it is |
| :-- | :-- |
| baseline | A throwaway copy of the installed workspace (release 0.6.4). Its strategist and architect charters are byte-identical to the tag `v0.6.4` (checked with `cmp`); it has 68 skills and none of the new ones. |
| candidate | The same copy with an overlay of main's `agents/strategist/AGENT.md`, `agents/architect/AGENT.md` and twelve skills: the eight new ones for these two roles and `spec-authoring`, `oblique-strategies`, `roadmap-sequencing`, `risk-roi-analysis`. |

Both arms ran the same program (built from this branch), the same model, the same prompts. The candidate's charters carry
every edit of spec 124 to those two roles (slices S3 to S9), not only the one a case is about, so a difference between the arms
is a difference between the two charters as wholes.

Where the baseline has nothing to compare, only the candidate ran: the baseline has no `strategy_trace` tool (it is offered to a
role whose manifest lists a strategy skill) and no `divergent-options` file.

## Results

### 1. No model

| What | Cases | Result |
| :-- | :-- | :-- |
| Trigger, lexical (`crates/delphi/tests/spec124_evidence.rs`) | 51 tasks in `dev/skill-evals/spec124-{architect,strategist,advocate}.json` (13 of the strategist's came with S8, 2 more are sibling tasks for them): 29 written for one of the twelve skills, 15 for a sibling skill, 7 for no skill | 51 of 51: each of the twelve skills is retrieved from its own tasks (two or three each) and is not retrieved from at least one task that shares a word with it |
| `strategy_trace` (same test) | 6 fixtures: an orphan action, an unfounded policy, a diagnosis with no falsifier, two incomplete forecasts, all four at once, and a clean trace | 6 of 6 report exactly the findings in `expect.toml` (`TRACE-ACT-01`, `TRACE-POL-01`, `TRACE-DIA-01`, `TRACE-FOR-01`; none for the clean one) |
| The trace cases' own checks | each case's patterns run on what the tool prints for each of the 6 fixtures (36 pairs) | 36 of 36: a case passes on its own fixture's output and fails on every other |
| Capture, scripted (`spec_124_s12_the_capture_cases_…` in `delphi-engine`) | 2 cases (strategist, architect), each run with a script that stages 0, 1 and 2 notes | 6 of 6 as expected: only one note meets the case's count; in every run no lesson, heuristic or proposal changed and no system prompt held a note |
| "Options considered" check (`crates/delphi/src/train_options.rs`) | 9 hand-written sections in `suites/spec124/cases/options-considered/samples` | 9 of 9 as their key says: 2 pass; 7 fail, each for its own reason (no heading, two options, one design three times, no test, no constraint removed, no choice, a list instead of a table) |
| The same check on text it was not written beside | the section of `specs/125` (the only spec under the rule) and the skill's own template of placeholders | spec 125 passes; the template fails |

The lexical trigger test is weak evidence: the tasks were written by someone who had read the descriptions, and they share
their words. It shows a description carries the vocabulary of its task and does not collide with a sibling.

### 2. Trigger, one model call per role

`python3 -I dev/skill_trigger_eval.py trigger <dir> --role <role>` with its default model (`gemini-3.8-flash-low` through `agy`;
`agy` does not report which model served). One run.

| Role | Tasks (new files + older `cases.json`, `hard.json`) | Picked the expected skill, or none |
| :-- | :-- | :-- |
| advocate | 8 + 0 | 8 of 8 |
| strategist | 10 + 17 | 27 of 27 |
| architect | 18 + 16 | 34 of 34 |

This was run before S8 merged: the strategist's list then held `competitive-swot-analysis` and not the three S8 skills, and
the 15 strategist tasks added since (13 from S8 and 2 sibling tasks for its skills) were not asked of a model. All tasks of a role go in one prompt, so the 69 answers are three samples, not 69. The older tasks still chose their old skills
with the new skills on the list. This call shows the model the names and descriptions of the role's skills; see finding 1
below for why that is not what a seat inside the daemon is shown.

### 3. One model in the daemon (`delphi train run`, Haiku)

27 runs, all graded, none errored, none repeated (one command was refused before any run, because a case was half-written at
that moment, and was started again). 1,752,085 tokens in and 22,621 out as the harness counted them (a subscription seat; the
harness's caps are in dollars and it reported `$0.00`). Before the runs, the availability check made one small call each to
Haiku, Sonnet and Opus (about 9,600 tokens each). Rows: [`pilot/rows.jsonl`](pilot/rows.jsonl). Every answer and
every staged note, verbatim: [`pilot/answers.md`](pilot/answers.md).

| Case | What it asks | baseline | candidate |
| :-- | :-- | :-- | :-- |
| `capture-strategist` | the operator corrects a forecast; exactly one note is staged, nothing is proposed or ratified, the file is unchanged | 0 of 3 | 3 of 3 |
| `capture-architect` | a Council overrules a decision record; the same checks | 0 of 3 | 3 of 3 |
| `options-considered` | write the "Options considered" section for a requirement; nothing names the skill | 0 of 3 | 0 of 3 |
| `options-skill-named` | the same, and the prompt says to load `divergent-options` | not run (no such file) | 3 of 3 |
| six `trace-*` cases | run `strategy_trace` on a file and quote what it printed | not run (no such tool) | 6 of 6, one seed each |

What the rows say beyond the counts:

- **Capture.** Every baseline run answered the correction in words and staged nothing (0 `learn.note` events), although the
  baseline seat holds the same `note` tool. Every candidate run called `note` once, with kind `pitfall`, at tier L3. The two
  arms separate completely on three seeds each, which is the least the harness calls evidence.
- **Options, skill not named.** No run in either arm wrote a table, marked an option conventional or as removing a
  constraint, or gave each option a test. Five failed with "the section has no table of options" and one with no heading.
  Both arms wrote their options as paragraphs or a numbered list. No candidate run opened a skill file (its only tool call was reading the
  requirement).
- **Options, skill named.** All three runs read the skill file and wrote the table with the three kinds, a test for each and a
  choice.
- **Trace.** Each run called the tool once and quoted its lines; the checks found the planted findings and no others.

## Findings to look into

1. **A seat inside the daemon may never be shown its skills.** From reading the code, not from a test: the system prompt a
   daemon seat gets is built from the governance text, the charter's body, guidance, heuristics, skills from added bundles,
   lessons and context (`Ipr` in `crates/delphi-adapter/src/ipr.rs`, filled in `crates/delphi-engine/src/engine.rs`). I found
   nothing that puts the names and descriptions of a role's built-in skills there (the discovery tier of `specs/001` §3), and
   the architect's charter body does not name `divergent-options`: only its frontmatter does. That would explain the 0 of 3
   above, and it would mean the trigger tests measure a catalog that a host sees (`list_skills`) and a daemon seat does not.
   This needs a person to confirm and, if true, its own spec.
2. The capture step works where the instruction is in the charter body. The option skill is reachable only if the seat goes
   looking. The difference between those two results is where the text lives, not which text is better.

## What was not shown

- **That any skill improves quality.** No check here reads whether a strategy is sound, an option is a good design, a test would
  work or a staged note is worth keeping. Spec 124 §11 rules out a model-graded judgement of quality, and none was made.
- **That an architect writes three distinct options by default.** The one run of that question failed in both arms.
- **That a model picks the right skill inside the daemon.** The model half of the trigger test is a prompt outside Delphi.
- **Anything about another model.** One model, one family, one day. Sonnet, Opus, Fable, Gemini and local models were not run.
- **Anything about variety.** One prompt for each case. Each capture prompt opens with the words the charter's step is
  written around ("The operator corrects you", "The Council overruled"); a correction worded differently, or one that arrives
  in the middle of other work, was not tried. The trace prompts name the tool.
- **A holdout.** Every case is in the `dev` split and was written by the author of the checks. There was no pre-registration.
- **A human reading.** Nobody has read the 27 answers or the 6 notes. Whether the notes say the right thing is unjudged.
- **The other eight skills in use.** `quality-scenarios`, `adr-authoring`, `fitness-functions`, `spec-premortem`,
  `boundary-design`, `strategy-kernel`, `strategy-cascade` and `accessibility-audit` have trigger tests only. No seat was asked
  to use one.
- **The capture step's later half.** That a staged note reaches `/retro`, that an operator ratifies it, or that a ratified
  lesson changes later work.
- **The host path.** A host worker that ends its answer with `Note to stage:` was not run.
- **The strategist skills of S8 with a model** (`positioning-and-product-strategy`, `advantage-analysis`,
  `scenario-and-premortem`): they merged after the model runs. Each now has the lexical trigger test (at least two tasks
  that retrieve it and a sibling or no-skill task that does not) and nothing else: no model call picked among them, no seat
  was asked to use one, and no model saw the strategist's list without `competitive-swot-analysis`.

## Limits of the instruments

- The "Options considered" check reads a table, as the skill prescribes. Three good options written as a list fail it.
  "Distinct" means only that no two descriptions share 80 percent of their words; two designs with the same mechanism in
  different words pass.
- `strategy_trace` sees the fixed layout and labelled lines only. A clean trace says the links exist, not that the strategy
  is good.
- The capture check counts notes. One note of nonsense passes; two good notes fail.
- A case's cap is in dollars (`max_cost_usd`) because that is what the harness has; a subscription seat reports no dollars, so
  the cap bound nothing here and tokens are reported instead.

## How to run it again

```sh
cargo test -p delphi --test spec124_evidence          # trigger, trace, this page's shape
cargo test -p delphi --bin delphi train_options       # the options check and its samples
cargo test -p delphi-engine --lib spec_124_s12        # the capture cases, scripted
delphi train validate --suite spec124                 # the cases are well formed
python3 -I dev/skill_trigger_eval.py trigger <empty dir> --role architect    # one model call

# the model arm: a template home is a copy; your own home is only read (see ../PILOT.md)
python3 -I dev/train/avail.py --workspace <name> --only claude --live --json /tmp/avail.json
delphi train arms init --source-home ~/.delphi --workspace <name> --defs dev/train/arms-defs.toml --avail /tmp/avail.json --out /tmp/arms --only solo-haiku
delphi train run --suite spec124 --case capture-architect --budget-usd 3 --template-home /tmp/arms/homes/solo-haiku \
    --overlay <dir with main's agents/ and skills/ for the two roles> --work-dir /tmp/w1 --workspace <name> --seeds 3
```

Once the installed workspace carries spec 124's charters, a copy of it is no longer a baseline: build the baseline from the
tag `v0.6.4` instead.
