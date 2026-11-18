---
name: strategy-kernel
description: "Write a strategy on one page: a diagnosis with a falsifier, guiding policies and the steps that carry them out, then trace the links. Use when asked for a strategy or to judge one."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["challenge"]
  properties:
    challenge:
      type: string
      description: "The situation or goal the strategy is for, in the operator's words"
    evidence:
      type: array
      items: {type: string}
      description: "Paths or sources the diagnosis may rest on"
output_schema:
  type: object
  required: ["strategy"]
  properties:
    strategy:
      type: string
      description: "The strategy document in the fixed layout, as text or as the path it was written to"
    trace:
      type: string
      description: "What `strategy_trace` reported on it"
required_permissions:
  - read:all
  - write:missions
---
# Strategy Kernel

A strategy says what is in the way, what approach is chosen, and what will be done. A list of goals is not one.

## When to use
- The operator asks for a strategy, a plan of attack or "what should we do about X".
- A document called a strategy needs judging: find its kernel or show that it has none.

## The three parts
- **Diagnosis:** what is in the way and why, as claims that could be wrong. Each claim has a **falsifier**: an observation that would show it wrong.
- **Guiding policy:** the approach chosen to deal with the diagnosis. It rules things out. It is not a goal and not a list of steps.
- **Coherent actions:** steps that carry a policy out and do not work against each other.

## Layout
`strategy_trace` reads exactly this. Ids are `D`, `P` and `A` with a number. A policy names the diagnosis claims it answers in parentheses; an action names the policies it carries out.

```markdown
# Strategy: faster checkout

## Diagnosis
- D1: Checkout is slow because every page load calls the tax service.
  Falsifier: p99 stays above 2 s with the tax call cached.

## Guiding policies
- P1 (D1): Take the tax call off the page-load path.

## Actions
- A1 (P1): Cache tax rates per region.
  Owner: payments
  Measure: p99 under 1 s by 2027-01-31
  Stop doing: per-request tax lookups
```

Other sections may come before, between or after these three; they are not traced. Keep the three on one page.

A statement about what will be true at a later date is a **forecast**. Write it on its own line, in any section, with the label, its date in parentheses and a tripwire: an observable that would show it wrong.

```markdown
- Forecast (2027-03-31): checkout is the fastest step of the purchase in both regions.
  Tripwire: p99 above 1.5 s at the 2027-02-15 review.
```

`strategy_trace` reports a line labelled `Forecast` that has no date (`YYYY-MM-DD`) or no `Tripwire:` (`TRACE-FOR-01`). It cannot see a forecast that was not labelled.

## Steps
1. Write the diagnosis first, from evidence you can point at. A claim about the outside carries its source, or is marked as an assumption.
2. Give every diagnosis claim a falsifier. If nothing could show it wrong, it is a restatement, not a diagnosis: rewrite it.
3. Write one to three guiding policies. For each, say what it rules out. Name the diagnosis claims it answers.
4. Write the actions. Each names its policies, an owner, a measure, and what stops being done to make room.
5. Run the bad-strategy checklist below and rewrite what it catches.
6. Run `strategy_trace` on the document: pass `path` when it is in a file you may read, or `text` when it is in your reply. A strategy file lives at `missions/<id>/strategy/<name>.md`, and `strategy_trace` can be passed that path. Fix each finding or say why it stands. Its findings are advice; they decide nothing.
7. Report the strategy, the trace result, and the one diagnosis claim you are least sure of.

## Bad-strategy checklist
- **Goal list:** "grow revenue, improve quality, delight users" with nothing about how or what is in the way.
- **Buzzwords:** a sentence that keeps its meaning when the nouns are swapped for others. Say the plain thing.
- **Restating the obvious:** a diagnosis that repeats the challenge ("we are behind because competitors are ahead").
- **No choice made:** a policy that rules nothing out, or policies that cannot all be followed with the people available.
- **Actions that pull apart:** two actions that spend the same people on opposite ends.

## Done when
The document has the three sections in the layout, `strategy_trace` reports a complete trace or each remaining finding is answered in a sentence, the checklist catches nothing, every diagnosis claim has a falsifier that names an observation, and every forecast carries its label, a date and a tripwire.

## Pitfalls
- A clean trace is not a good strategy. The tool checks that the links exist, not that the diagnosis is true.
- A falsifier nobody could observe ("if the market changes").
- Adding a policy afterwards to adopt an orphan action. Cut the action, or admit the policy and trace it to a diagnosis.
- Writing the actions first and the diagnosis to fit them.
- A forecast written as a fact, with no label. No check can see it, and nobody can later say whether it held.

## Sources
The kernel (diagnosis, guiding policy, coherent actions) and the bad-strategy signs are commonly attributed to Richard Rumelt. This text was written from secondary summaries, not from the primary work, and has not been checked against it. It cites no primary work until the operator ratifies the sources (`specs/124` §12, OD-3).
