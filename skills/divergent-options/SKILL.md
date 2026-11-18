---
name: divergent-options
description: "Produce at least three design options before one is chosen: a conventional one, one that removes a named constraint and one by a named method. Use after the requirement is stated."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["requirement"]
  properties:
    requirement:
      type: string
      description: "The requirement the design must meet, stated before any design"
    constraints:
      type: array
      items: {type: string}
      description: "The constraints the conventional design assumes, stated or unstated"
output_schema:
  type: object
  required: ["options"]
  properties:
    options:
      type: string
      description: "The 'Options considered' section: three or more options, each with its method and how it would be tested"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Divergent Options

The first design that comes to mind is usually the conventional one. This skill makes two more appear beside it before a choice is made, so the choice is between designs and not between one design and nothing. It is a default step of `spec-authoring`, not a tool for emergencies; for a deadlock use `oblique-strategies`.

The methods named here (inversion, TRIZ, morphological recombination, analogy) were taken from secondary summaries, not from the primary works. Until the operator ratifies primary sources (spec 124, OD-3), cite none of them as an authority and treat each as a prompt for ideas, not as proof that an idea is good.

## When to use
- After the requirement and the non-goals are written, and before the design section of a spec.
- Not for a typo, a one-line fix, or a change whose design another approved spec already fixes.

## Steps
1. Restate the requirement in one sentence that names no solution. If you cannot, the requirement is not understood yet: stop and ask.
2. List the constraints the obvious design assumes, including the unstated ones (it must be a daemon, it must be synchronous, it must keep the current file format).
3. **Option A, conventional.** The design a careful engineer would expect. Say so plainly.
4. **Option B, a constraint removed.** Pick one constraint from step 2, name it, and design as if it did not hold. Say what is lost if the constraint turns out to be real.
5. **Option C, by a named method.** Use exactly one of these and name it:
   - **Inversion:** reverse who acts, when, or in which direction data flows (the client resolves instead of the server; append instead of mutate).
   - **A TRIZ principle:** first state the contradiction ("improving X makes Y worse"), then name the principle used to escape it (for example segmentation, doing it in advance, taking out the harmful part).
   - **Morphological recombination:** list the design's independent dimensions, list two or three values for each, and pick a combination nobody proposed.
   - **An analogy from another field:** name the field and the mechanism borrowed, and say where the analogy stops holding.
6. For each option write **one sentence on how it would be tested**: the observation that would show it works or fails. An option nobody can test is not an option.
7. Run the guard below on options B and C.
8. Write the result as the spec's **Options considered** section, then choose. The choice may be the conventional option; say why the others lost. `Simpler Alternative Considered` is a separate section with its own rule and is still required.

## The guard: no untested abstraction
An unconventional option often arrives as a new layer: a registry, a plugin point, a general engine for one case. For options B and C, answer each question. A "yes" to the first with a "no" to the second means the option is flagged in the section, in those words, and is not chosen until it has a test.

- [ ] Does the option add a layer of abstraction, an indirection or a new general mechanism?
- [ ] If so, does its one-sentence test exercise that layer itself, and not only the feature behind it?
- [ ] Is there a second real use for the layer today, not a predicted one?
- [ ] Is the option different from option A in mechanism, not only in naming?

## Output format
```markdown
### Options considered
| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | ... | ... |
| B | constraint removed: <the constraint> | ... | ... |
| C | <inversion / TRIZ: contradiction and principle / recombination / analogy from <field>> | ... | ... |

Chosen: <A, B or C>, because <reason>. Flagged by the guard: <none, or the option and why>.
```

## Done when
The section holds at least three options, the three kinds are all present, options B and C each name their constraint or method, every option has a test sentence, the guard has been answered for B and C, and one option is chosen with a reason.

## Pitfalls
- Three variants of the same design. If B and C share A's mechanism they are not options.
- A TRIZ principle named with no contradiction stated: the name alone explains nothing.
- Choosing the unconventional option because it is interesting. Novelty is not a criterion.
- Treating this section as the simpler alternative. The simpler-alternative rule stands on its own.
- Citing a method's author or book as support. These methods are unratified secondary knowledge here.
