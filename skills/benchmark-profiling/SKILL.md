---
name: benchmark-profiling
description: "Measure performance with warmups, repeated runs and a significance test before claiming a change helps. Use when comparing speed, memory or tokens."
authorized_agents:
  - investigator
input_schema:
  type: object
  required: ["target"]
  properties:
    target:
      type: string
      description: "What to measure"
    baseline:
      type: string
      description: "Revision or variant to compare against"
output_schema:
  type: object
  required: ["report"]
  properties:
    report:
      type: string
      description: "Distributions, comparison and verdict"
required_permissions:
  - read:all
  - shell:scoped_scripts
---
# Benchmark and Profiling

One timing is an anecdote. Measure enough to tell signal from noise, and say what you measured.

## When to use
- A claim about speed, memory, token use or cost needs evidence.

## Steps
1. State the question and the metric (latency percentiles, peak memory, tokens per task).
2. Control the environment: same machine, release build, quiet system, fixed inputs and seeds. Write the setup down.
3. Warm up, then take many samples per variant, interleaving baseline and candidate to cancel drift.
4. Report p50, p95 and p99 (or the metric's equivalent) with the sample count, not a mean alone.
5. Test the difference with a non-parametric test (such as Mann-Whitney U) and report the effect size, not just significance.
6. Profile only after you have a measured difference worth explaining; report the hot spots with `path:line`.
7. State the verdict in terms of the original question, including "no measurable difference".

## Done when
Another person could rerun the setup and get the same ranges, and the verdict follows from the numbers.

## Pitfalls
- Benchmarking a debug build.
- Optimizing what the profile did not show.
