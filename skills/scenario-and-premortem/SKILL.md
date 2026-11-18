---
name: scenario-and-premortem
description: "Write at least three futures, each with a dated tripwire, then a premortem on the chosen strategy: suppose it failed and say why. Use when asked what could change or how a strategy could fail."
authorized_agents:
  - strategist
input_schema:
  type: object
  required: ["strategy"]
  properties:
    strategy:
      type: string
      description: "The chosen strategy, as text or as a path"
    horizon:
      type: string
      description: "How far ahead to look, as a date"
    uncertainties:
      type: array
      items: {type: string}
      description: "Things outside our control that the operator already worries about"
output_schema:
  type: object
  required: ["scenarios", "premortem"]
  properties:
    scenarios:
      type: string
      description: "Three or more scenarios in the fixed layout, each with a dated tripwire"
    premortem:
      type: string
      description: "The causes of failure, their early signs and what the strategy does about each"
required_permissions:
  - read:all
  - write:missions
---
# Scenario and Premortem

A strategy is chosen for one expected future. This skill writes down other futures that could happen, how we would notice each in time, and how the chosen strategy would most likely fail. A scenario is not a forecast: it is a future worth being ready for.

## When to use
- A strategy has been chosen (a `strategy-kernel` page, a cascade, a roadmap) and is about to be relied on.
- The operator asks "what could change?", "what if we are wrong?" or "how does this fail?".
- A forecast is being written and needs the observation that would show it wrong.

Not for a spec's design: the architect's `spec-premortem` does that.

## Steps
1. **Read the chosen strategy** and write its expected future in two sentences: what it assumes stays true until the horizon. If no horizon was given, propose a date and say so.
2. **List the uncertainties:** things outside our control that would change the answer. Keep the two or three that matter most and could go either way. Something certain is not an uncertainty; put it in every scenario.
3. **Write at least three scenarios.** One is the expected future. The others differ from it on a named uncertainty, and at least one is unwelcome. Each has:
   - **What happens**, in a few sentences, as a story that could be true.
   - **What it does to the strategy:** which policy or action stops making sense.
   - **Tripwire:** one observation, with **a date by which it will be checked** and where it will be looked for, that would show this scenario is arriving. "If the market shifts" is not a tripwire; "fewer than 40 paid seats on 2027-03-31, from the billing export" is.
   - **Response:** what we would do if the tripwire fires.
4. **Premortem on the chosen strategy.** Suppose the horizon date has come and the strategy failed. Write the most likely causes, most likely first, each with its early sign and what the strategy does about it today: a change to an action, a tripwire, or an accepted risk with the reason. At least one cause is inside our control (we did not do what we said) and one is outside.
5. **Mark every prediction** with the label `forecast`, its date and its tripwire, so a later review can say held, failed or open.
6. **Report** the scenarios, the premortem, the tripwires in date order, and which cause of failure nothing in the strategy answers yet.

## Layout
```markdown
# Scenarios: <strategy>, to <horizon date>

## S1: <name> (expected)
What happens: ...
Effect on the strategy: ...
Tripwire: <observation>, checked on <date>, from <where>
Response: ...

## S2: <name>
...

## S3: <name>
...

## Premortem
| Cause | Early sign | What the strategy does about it |
| :-- | :-- | :-- |

## Tripwires by date
- <date>: <observation> (S2)
```

## Done when
There are at least three scenarios, one of them unwelcome; each has a tripwire that names an observation, a calendar date and where to look; each says which part of the strategy it affects and what we would do; the premortem lists causes with early signs, at least one inside and one outside our control, each answered or marked as an accepted risk with a reason; and every prediction carries `forecast` and a date.

## Pitfalls
- Best case, base case, worst case. Those are one future at three volumes; vary a named uncertainty instead.
- A tripwire with no date, or one nobody can observe. It will never fire.
- Scenarios that all leave the strategy unchanged. Then they were not about this strategy.
- A premortem that blames only the outside. Most plans fail on things their owners controlled.
- Giving the scenarios probabilities you cannot support. Say which is expected and leave it there.
- Treating this as a review by someone else. The author of a strategy finds the failures the author can imagine; it does not replace the red-team.

## Sources
Writing several futures around named uncertainties is the common account of scenario planning, and the premortem (suppose the plan failed, then explain why) is commonly attributed to Gary Klein. This text was written from secondary summaries, not from the primary works, and has not been checked against them. It cites no primary work until the operator ratifies the sources (`specs/124` §12, OD-3).
