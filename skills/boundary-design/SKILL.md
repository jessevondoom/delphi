---
name: boundary-design
description: "Name the bounded contexts of a domain, the terms each owns and the contract at each boundary, after checking the domain is large enough to need them. Use when one model strains to serve several areas."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["domain"]
  properties:
    domain:
      type: string
      description: "The domain or system area to divide, and the modules or specs that make it up"
output_schema:
  type: object
  required: ["boundaries"]
  properties:
    boundaries:
      type: string
      description: "Either the contexts with their terms, owners and boundary contracts, or the finding that the domain should not be divided, with the reason"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Boundary Design

A bounded context is an area inside which each term has one meaning and one model. Boundaries are useful where the same word means different things to different parts of a system, and harmful where they split something that was simple.

Bounded contexts come from domain-driven design (DDD). What is written here was taken from secondary summaries, not from the primary works. Until the operator ratifies primary sources (spec 124, OD-3), cite no author as an authority for it.

## When to use
- One word (record, session, source, trust) is used with two meanings, and bugs or long arguments follow from it.
- A module must change whenever any of several unrelated areas changes.
- A new spec has to say which existing part owns a piece of data.
- Not before the guard in step 1 has been passed.

## Steps
1. **Guard: is the domain large enough?** Answer each question from the code and specs, not from expectation. If the answers are mostly "no", stop: report that the domain is small and data-shaped, recommend plain data structures and functions in one module, and do not draw boundaries.
   - [ ] Does some term have two meanings in use today, with a place in the code or specs for each?
   - [ ] Are there rules (invariants, state changes), and not only fields that are stored and read back?
   - [ ] Do different parts change for different reasons and at different times?
   - [ ] Would more than one role or team work in it at once?
   - [ ] Is it more than a few record types and the operations that create, read, update and delete them?
2. Collect the terms: read the specs and the code (`repo-cartography`, `ast-code-search`) and list each domain word with what it means where it is used. Mark the words with more than one meaning.
3. Group the terms into contexts so that inside each context every word has one meaning. Name each context after what it is for. Prefer few: two or three well-placed boundaries are worth more than seven.
4. For each context say what it owns: its data, its invariants, and the one module, crate or file format that is the authority for them.
5. For each pair of contexts that touch, write the **boundary contract**: what crosses (a named record or event, with its schema where one exists in `schemas/`), in which direction, and who translates when the two sides use a word differently. Say which side may change without asking the other.
6. Check the boundaries against authority: no boundary lets content from outside raise its own trust, and nothing crosses a trust boundary without the existing checks. A boundary that would need a new path around the sandbox or a gate is not drawn: it is reported to the operator.
7. Test each boundary with a change: pick two changes that are likely, and say how many contexts each touches. A boundary that every likely change crosses is in the wrong place.
8. Write the result as a context map in the spec (a table, and a diagram from `diagram-generation` when there are more than three contexts). A boundary that is costly to move gets a record through `adr-authoring`.

## Output format
```markdown
Guard: <passed: which questions were yes, with evidence | not passed: keep one module, because ...>

| Context | Purpose | Owns (data, invariants) | Authority (module / format) | Terms with their meaning here |
| :-- | :-- | :-- | :-- | :-- |

| Boundary | What crosses | Direction | Translation | May change alone |
| :-- | :-- | :-- | :-- | :-- |

Change test: <change> touches <n> contexts; <change> touches <n> contexts.
```

## Done when
The guard's questions are answered with evidence, and either the report says not to divide the domain, or every context has a purpose, an owner and one meaning per term, every touching pair has a contract, and two likely changes have been traced across the map.

## Pitfalls
- Contexts drawn along the existing folders. A boundary follows meaning, and the folders may be the problem.
- Applying the method to a small domain of records because it is the respected method. That adds layers and translations with nothing to separate.
- A shared "common" context that every other context depends on: it is one model again under another name.
- A boundary with no contract. If nothing says what crosses, the boundary is a wish.
- Splitting what must change together. Two contexts that always change in the same commit are one.
