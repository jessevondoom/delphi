---
name: quality-scenarios
description: "Turn a quality goal such as speed or safety into six-part scenarios with a measure, rank them in a utility tree and mark sensitivity and tradeoff points. Use when a design must meet one."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["design"]
  properties:
    design:
      type: string
      description: "The design or spec section under study"
    qualities:
      type: array
      items: {type: string}
      description: "The quality attributes that matter here, such as latency, availability, security, modifiability"
output_schema:
  type: object
  required: ["scenarios"]
  properties:
    scenarios:
      type: string
      description: "The scenarios, the utility tree that ranks them, and the lists of sensitivity points and tradeoff points"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Quality Scenarios

"It must be fast" and "it must be safe" cannot be built or tested. A scenario says who does what to which part, under which conditions, what the system does, and the number that decides whether that was good enough.

The scenario form, the utility tree and the two kinds of point come from the architecture tradeoff analysis method (ATAM). The method is described in Rick Kazman, Mark Klein and Paul Clements, *ATAM: Method for Architecture Evaluation*, CMU/SEI-2000-TR-004 (Software Engineering Institute, August 2000). This text was taken from secondary summaries, not from that report, and has not been checked against it. The operator ratified the report for citation on 2026-11-17 (`specs/124` §16, OD-3 answered); that it exists was confirmed on 2026-11-18 from web search results and recorded in Delphi's knowledge base, not validated. It is cited as a bibliographic reference only. Do not call the result an ATAM evaluation: that method has steps and participants this skill does not have.

## When to use
- A spec states a quality goal (speed, availability, security, ease of change, cost in tokens) without a number.
- Two designs differ mainly in how well they serve such goals.
- Not for a change with no quality goal at stake, and not as a substitute for acceptance criteria about function.

## Steps
1. List the quality attributes that matter for this design. Three to five is usual; more means the goals are not ranked yet.
2. For each attribute write one to three scenarios, each with all six parts:
   - **Source:** who or what causes the event (an operator, a second daemon, an attacker, a timer).
   - **Stimulus:** the event (a request, a crash, a malformed file, a change request).
   - **Artifact:** the part that receives it (a named module, file format, queue or screen).
   - **Environment:** the conditions (normal load, during a restart, offline, disk full).
   - **Response:** what the system does.
   - **Measure:** the number or observable that decides pass or fail (within 200 ms at the 95th percentile; no record lost; one file changed).
3. Build the **utility tree**: the root is "what this design is good for", its branches are the attributes, its leaves are the scenarios. Rate each leaf twice, high, medium or low: how much it matters to the operator's goal, and how hard it is to achieve. Work on the leaves rated high on both first, and say which leaves you are not addressing.
4. Walk the design against the top leaves and list:
   - **Sensitivity points:** a decision that one attribute's measure depends on strongly (the batch size sets latency).
   - **Tradeoff points:** a decision that moves several attributes, at least one for the worse (a cache helps latency and hurts freshness). Each tradeoff point names the attributes on both sides.
5. For each tradeoff point say which side the design takes and why, or hand it to `trade-off-matrix`. A decision made here that is costly to undo goes to `adr-authoring`.
6. Turn each top scenario's measure into an acceptance criterion or a check in the spec's verification plan.

## Output format
```markdown
| # | Attribute | Source | Stimulus | Artifact | Environment | Response | Measure | Matters | Hard |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |

Sensitivity points: <decision> -> <attribute>
Tradeoff points: <decision> -> <attribute helped> against <attribute hurt>; side taken and why
Not addressed: <scenario numbers and the reason>
```

## Done when
Every scenario has six parts and a measure someone could observe, the tree ranks the leaves by importance and difficulty, the top leaves each map to an acceptance criterion or a check, and the sensitivity points and tradeoff points are listed apart.

## Pitfalls
- A measure that is an adjective ("quickly", "securely"). If it has no number or observable it is not a measure.
- Scenarios for the normal environment only. The ones that decide a design are usually the stressed ones.
- Calling a sensitivity point a tradeoff. One attribute is a sensitivity; a tradeoff needs a loser.
- Rating every leaf high. A tree in which everything matters ranks nothing.
- Numbers invented to fill the measure column. Say "not yet known, to be measured by" and name the check.
