---
state: built
date: 2026-11-27
title: "synthetic cases, a single-call caller of spec 143"
eval_waived: "synthetic cases for the golden set only; no ranking, chunking or index change; the diff base counts knowledge-v2's own retrieval changes"
---

Slice C of spec 140, built 2026-11-27 on `build/140-c` (from `knowledge-v2`).

## What was built

- **`delphi kb eval build [--synthetic <n>]`.** Without `--real-only`, a build adds synthetic cases to the real ones. `n` is the number of chunks asked about: 150 by default, at most 1,000.
- **The sample** (`kb_eval::sample_chunks`). Chunks of the index, stratified by zone and by project (`zone` or `zone/<project>`). The strata take turns in name order. Inside a stratum the chunks go in the order of the hash of their place, so the same index gives the same sample. Passed over: a chunk with fewer than 20 content words, and a document the run's search cannot return (a staged note, for example).
- **The model call** (`Engine::question_key`, `Engine::synthetic_question` in `page_run.rs`). The writer role on the `fast` tier, a route request marked `single_call`. One call per chunk, no tools, the chunk's text as quoted data, an answer of at most 120 tokens. The call states the tier of its one input: the index's tier of the chunk's document (`external_untrusted` when unknown). The answer is redacted.
- **The daemon method** `kb.eval.synthetic {n, plan}` (console scope only). With `plan` it samples and makes no call: chunks, strata, the model, the token estimate, whether through the bridge. Without it, it asks one question per chunk, one call at a time, stops at the first failed call (an empty answer only loses its chunk), and returns the kept cases. It writes nothing.
- **The two filters** (`kb_eval::synthetic_from`), in code:
  - more than half of the question's content words are in the chunk's heading or first line: dropped (`repeats_heading`);
  - the question's top keyword hit is not its chunk and none of its content words is in any chunk: dropped (`about_nothing`).
  - Also dropped: an answer that is not one line ending in a question mark (`unparsed`), and a second copy of a question (`duplicate`).
- **The cost line** (`kb_eval::build_cli`). Printed before any call: the number of calls, the model, the tokens, the bridge if used, and that chunk text, private passages included, goes to that model. Then `send them? [y/N]`. Without `y` or `yes` nothing is sent.
- **Clearly synthetic.** Each case has `kind: synthetic`, an id starting `s-`, and `from: chunk:<path>#<line>`. Its one relevant passage is its chunk, grade 3, with the chunk's tier. A synthetic question that equals a real case's question is left out. The header gains `synthetic`: the model, chunks answered, kept, dropped by reason, tokens, and why the calls stopped. It says when the set has fewer than 150 synthetic cases.
- **Real cases are unchanged.** They are built by the same code as `--real-only`, line for line. An accepted set is never touched: a build after acceptance is a new version.
- **With no model, no daemon, or no `yes`**, the build has the real cases only and says the synthetic ones were skipped, and why.
- `route::SINGLE_CALLERS` already listed `page_run.rs`. `summary::CALL_SITES` now counts 2 calls there. The `page_run` test in `single_call_tests.rs` checks both callers of the file.

## Acceptance criteria and tests

- **AC-4**:
  - `kb_eval::tests::a_synthetic_question_that_repeats_its_chunks_heading_is_dropped`: the heading filter, the about-nothing filter, unparsed and duplicate answers; the kept cases are marked synthetic with their chunk as the one passage.
  - `kb_eval::tests::the_cost_line_is_printed_before_any_model_call_and_nothing_is_sent_without_yes`: the cost line comes before the call; without `yes` no call is made; the real cases are the same lines as a real-only build.
  - `kb_eval::tests::with_no_model_the_build_has_the_real_cases_and_says_the_synthetic_ones_were_skipped`.
  - `kb_eval::tests::the_daemon_plans_without_a_call_then_asks_one_question_per_chunk_and_stops_at_a_failure`: the plan calls no model; one tool-less call per chunk; a failed call stops the rest; another scope than the console is refused.
  - `kb_eval::tests::the_sample_is_stratified_deterministic_and_leaves_out_short_and_unreachable_chunks`.
- **`specs/143` AC-13** (the new caller): `single_call_tests::page_run_makes_one_request_with_no_tools` and `summary::tests::every_call_site_of_a_single_call_is_on_the_list`.

## Builder decisions

- **The model call runs in the daemon**, through a new console-scope method, as `kb summarize` does. The command at the terminal prints the cost and asks first. `kb_eval.rs` has no engine, and the daemon owns routing, the bridge and redaction.
- **The command always asks before it sends**, on every install. §3 asks only "on another install"; asking on the operator's install too is the stricter reading. Nothing is sent without `y`.
- **No bridge job.** The call is the operator's command with no `[bridge.jobs]` job and no `in_job`/`in_command` scope. So it is admitted as a command (window and daily cap), with no weekly job budget and no typed confirmation. A refusal stops the calls, and the build keeps what it got.
- **The writer role on the `fast` tier.** §2.1 item 4 names the tier, not the role.
- **Filter one** counts the heading's whole path and the first line together: the stricter reading of "heading or first line".
- **Filter two** reads "whose words appear in no chunk" as: none of the question's content words is found by a keyword search of the index.
- Content words are lower case, three characters or more, minus a fixed list of common words. A question must end with a question mark.
- One failed call stops the build's calls; an empty answer does not.
- The default `n` is 150, the minimum of §2.1 item 5. Filters drop some, so a default build is usually marked short; ask for more with `--synthetic <n>`.
- The `kb` help line shows `eval build [--synthetic <n>]`; `--real-only` is still taken and is in `kb eval`'s usage (the help page is at its width limit).
- **`kb_eval.rs` is a row of `private_read::READS`** (`specs/103`), as `NotYetMarked`: it reads chunk text into the writer's prompt. Only the operator's command reaches it, after the cost line; the writer has no tools; the question stays under `local/eval`. The architect should confirm the classification.
- `eval_waived` is set: the slice changes no ranking, chunking or index.

## Not verified

- No build on the operator's workspace, and no real model call. The tests use a scripted model.
- Whether a model writes questions that are neither trivial nor unanswerable at a useful rate (§7). The filters are guesses until the first build is sampled.
- The cost estimate is a count of characters over four plus 250 tokens a call; it is not checked against a provider.
- A long build is one call to the daemon. 150 calls at a few seconds each is several minutes with no progress line.
- Through the bridge, a build past the daily cap stops; it has no typed confirmation.
- The operator's acceptance of the set (OD-2) is still due. No accepted case was changed.
