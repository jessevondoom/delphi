---
name: strategy-cascade
description: "Cascade a chosen direction through five choices, aspiration to management systems, then tactics and dated steps, each with a measure and a stop-doing. Use when a direction must become who does what."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["direction"]
  properties:
    direction:
      type: string
      description: "The chosen direction or the strategy kernel to cascade"
    constraints:
      type: string
      description: "People, time and money available"
output_schema:
  type: object
  required: ["cascade"]
  properties:
    cascade:
      type: string
      description: "The cascade document, as text or as the path it was written to"
    trace:
      type: string
      description: "What `strategy_trace` reported on it"
required_permissions:
  - read:all
  - write:missions
---
# Strategy Cascade

A direction becomes work only when each choice narrows the next, down to an action somebody owns.

## When to use
- A direction or a kernel (`strategy-kernel`) exists and nobody can yet say who does what.
- A plan lists actions and it is unclear which choice each one serves.

## The choices, in order
1. **Aspiration:** what winning looks like, in terms someone could observe.
2. **Where to play:** the customers, uses, places or products chosen, and those left out.
3. **How to win:** why the chosen ground can be won by this team and not by anyone.
4. **Capabilities:** what the team must be able to do well for that to hold.
5. **Management systems:** the measures, reviews and rules that keep the capabilities in place.

Then **tactics** (the few moves that put a choice into effect), then **actions** (the dated steps).

## Layout
The cascade is a strategy document in the layout of `strategy-kernel`, with the choices written above it and tactics between policies and actions. `strategy_trace` reads only `## Diagnosis`, `## Guiding policies` and `## Actions` (headings and item lines as `strategy-kernel` describes them); the other sections are for the reader.

```markdown
# Strategy: faster checkout

## Aspiration
Checkout is the fastest step of the purchase by 2027-03-31.

## Where to play
Card payments in the two largest regions. Not invoicing, not new regions.

## How to win
We own the tax data, so we can cache what others must look up.

## Capabilities
Regional rate caching; a latency budget per page.

## Management systems
A weekly p99 review owned by payments; a budget check in CI.

## Diagnosis
- D1: Checkout is slow because every page load calls the tax service.
  Falsifier: p99 stays above 2 s with the tax call cached.

## Guiding policies
- P1 (D1): Take the tax call off the page-load path.

## Tactics
- T1 (P1): Serve rates from a regional cache.

## Actions
- A1 (P1): Cache tax rates per region.
  Owner: payments
  Measure: p99 under 1 s by 2027-01-31
  Stop doing: per-request tax lookups
```

## Steps
1. Write the five choices in order. Each must narrow the one before: if "where to play" leaves nothing out, it is not a choice.
2. Write or copy the diagnosis and the guiding policies (`strategy-kernel`). Where to play and how to win are stated again as policies, so the actions can name them.
3. Under each policy write the tactics: a few, not a backlog.
4. Turn each tactic into actions. Every action names its policies, an **owner** (a role or a person, not "the team"), a **measure** (what will be observed, and by when), and a **stop doing** (what is dropped to make room).
5. Run `strategy_trace` (`path` for a file you may read, `text` otherwise). A strategy file lives at `missions/<id>/strategy/<name>.md`, and `strategy_trace` can be passed that path. An action with no policy is cut or its choice is written down; a policy with no action is either given one or removed. The tool notes an action with no owner or measure. It does not read the choices, the tactics or the stop-doing: check those by eye. A statement about what will be true at a later date is a forecast: write `Forecast (YYYY-MM-DD): …` on its own line with a `Tripwire:` line under it (an observable that would show it wrong; layout in `strategy-kernel`). The tool reports a labelled forecast with no date or no tripwire.
6. Report the cascade, the trace result and the choice most likely to be wrong.

## Done when
Each of the five choices is one or two sentences and leaves something out; every action has an owner, a measure and a stop-doing; `strategy_trace` reports a complete trace or each remaining finding is answered in a sentence.

## Pitfalls
- An aspiration that is a slogan. If no observation could show it reached, rewrite it.
- "Where to play: everywhere." No ground was chosen.
- Capabilities listed because they sound good, not because how-to-win needs them.
- Actions with no "stop doing": the plan assumes free time nobody has.
- Treating a complete trace as proof the choices are right. It shows the links exist.

## Sources
The five-choice cascade is taken from A. G. Lafley and Roger Martin, *Playing to Win: How Strategy Really Works* (Harvard Business Review Press, 2013). This text was written from secondary summaries, not from the primary work, and has not been checked against it. The operator ratified this work for citation on 2026-11-17 (`specs/124` §16, OD-3 answered). That it exists was confirmed on 2026-11-18 from web search results and recorded in Delphi's knowledge base, not validated; the scout's own research returned no claim. It is cited as a bibliographic reference only: no passage is quoted or relied on.
