---
name: adr-authoring
description: "Record one architecture decision: context, options, the choice, accepted drawbacks, a one-way or two-way tag and a reversal trigger. Use when a choice will be costly to undo or to rediscover."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["decision"]
  properties:
    decision:
      type: string
      description: "The decision to record, in one sentence"
    spec:
      type: string
      description: "The spec the decision belongs to"
output_schema:
  type: object
  required: ["record"]
  properties:
    record:
      type: string
      description: "The decision record, as a section of the spec or a file beside it"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# ADR Authoring

A decision record keeps the reason for a choice next to the choice, so that the next person neither undoes it by accident nor defends it after its reason has gone. One record holds one decision.

The architecture decision record (ADR) is a widely described practice. What is written here was taken from secondary summaries, not from its original description. Until the operator ratifies primary sources (spec 124, OD-3), cite no author as an authority for the format.

A record is advice to readers. It approves nothing: a decision takes effect only through an approved spec, and approval is the operator's.

## When to use
- A design choice that is costly to reverse, or that a reader will ask "why not the other way?" about.
- A choice between options from `divergent-options` or a `trade-off-matrix` that should outlive the discussion.
- Not for a choice the spec's own text already explains in a sentence, and not for a preference with no consequence.

## Steps
1. **Decision.** State it in one sentence, in the present tense: "The daemon owns the queue file."
2. **Context.** The forces at the time: the requirement, the constraints, and the facts relied on, each with where it was read. Mark what was assumed and not checked.
3. **Options.** The ones really considered, including the conventional one and doing nothing. Take them from the spec's "Options considered" section where it exists; do not invent options afterwards to make the choice look contested.
4. **Choice.** Which option, and the reason that decided it.
5. **Accepted drawbacks.** What this choice makes worse or rules out. A record with no drawbacks has not been thought through.
6. **Reversibility.** Tag it `one-way` (undoing it means migrating data, breaking a published format or losing trust already given) or `two-way` (it can be undone by a change of similar size). Say in a phrase why.
7. **Reversal trigger.** The observable event that should reopen the decision: a measure crossing a number, a dependency changing, a second use case arriving. A one-way decision needs a trigger that can be seen before the cost is paid.
8. **Guard.** Where a check exists that would fail if the decision eroded, name it (see `fitness-functions`). Where none exists, say so.
9. Put the record in the spec as its own section, or in a file beside the spec, in the layout below. Keep the status line true: `proposed` until the spec is approved, then `accepted`; `superseded by <record>` later. Never edit an accepted record to change its choice: write a new one that supersedes it.

## Output format
```markdown
### Decision record: <short title>
- **Status:** proposed | accepted | superseded by <record>
- **Decision:** <one sentence>
- **Context:** <forces, facts with where they were read, assumptions marked>
- **Options:** <A conventional; B ...; C ...; doing nothing>
- **Choice:** <option> because <reason>
- **Accepted drawbacks:** <what gets worse>
- **Reversibility:** one-way | two-way, because <phrase>
- **Reversal trigger:** <observable event>
- **Guard:** <the check that would fail, or "none yet">
```

## Done when
The record holds one decision, every field of the layout is filled, the drawbacks are real, the reversibility tag has a reason, and the reversal trigger is something a person or a check could observe.

## Pitfalls
- A record that only restates the choice. The value is in the context, the drawbacks and the trigger.
- "Two-way" as the default tag. Check what stored data, published formats and granted trust would have to change.
- A reversal trigger such as "if it stops working well". Name the observation.
- Rewriting history: editing an accepted record, or adding options that were never considered.
- Treating the record as approval. It records a decision; the operator approves the spec.
