---
name: fitness-functions
description: "For each invariant a spec states, name the automated check that would fail when it erodes and the role that owns it. Use when a spec says something must always or never hold."
authorized_agents:
  - architect
input_schema:
  type: object
  required: ["spec"]
  properties:
    spec:
      type: string
      description: "Path of the spec whose invariants are to be guarded"
output_schema:
  type: object
  required: ["guards"]
  properties:
    guards:
      type: string
      description: "A table: invariant, the check that would fail, where it runs, the owning role, and its state (exists, to be written, not automatable)"
required_permissions:
  - read:all
  - write:worktree
  - write:missions
---
# Fitness Functions

An invariant written only in a spec erodes: a later change breaks it and nothing says so. A fitness function is an automated check that fails when a stated property of the design stops holding. This skill makes each invariant name its check, or say plainly that it has none.

The idea and its name are taken from Neal Ford, Rebecca Parsons and Patrick Kua, *Building Evolutionary Architectures* (O'Reilly, 2017; a second edition of 2022 adds Pramod Sadalage). This text was taken from secondary summaries, not from the book, and has not been checked against it. The operator ratified the book for citation on 2026-11-17 (`specs/124` §16, OD-3 answered); that it exists was confirmed on 2026-11-18 from web search results and recorded in Delphi's knowledge base, not validated. It is cited as a bibliographic reference only: no passage is quoted or relied on.

The architect names checks; it does not write them. A check is code, built by Builder against an approved spec. Nothing in this skill changes what a gate decides.

## When to use
- A spec says "always", "never", "only", "at most", "byte for byte", or lists invariants.
- A decision record (`adr-authoring`) needs its guard named.
- Not for behaviour that an ordinary acceptance test of the feature already proves once and that cannot erode.

## Steps
1. List the spec's invariants, one sentence each, in the form "X always / never Y". Split a sentence that holds two.
2. For each, look for an existing check before proposing a new one. Delphi's real checks, and what each is good for:
   - **A Rust test** run by `cargo test --workspace --locked`: behaviour of code, including the tests that read the repository's own files.
   - **`delphi lint --repo .`**: rules about files (manifests, skills, frontmatter, links, spec numbers, role IDs outside `themes/`).
   - **The schema tests** (`cargo test -p delphi --test docs`): every manifest, skill and example file satisfies its schema in `schemas/`.
   - **The docs drift test** (the same test file): `docs/reference/` matches what the program generates.
   - **The reference test** (`cargo test -p delphi --test spec_refs`): a cited spec section still exists.
   - **Clippy** with warnings denied: code-level rules.
   - **The console tests** under `ui/test`: golden screens, and that every animation obeys reduce motion.
3. Search for the check (`ast-code-search`) and read it. Name it exactly: the file and the test function, or the lint check's name. A check you have not read is recorded as "believed, not read".
4. Ask of each named check: **would it fail if the invariant broke?** Describe the smallest change that breaks the invariant and say whether the check would catch it. If not, the invariant is unguarded, whatever the test is called.
5. For an unguarded invariant, propose the check: what it reads, what it asserts, and which of the kinds in step 2 it is. Add it to the spec's verification plan as work for Builder.
6. Name the **owning role** for each check: the role that fixes it when it fails and is asked before it is weakened (Builder for a code test, Architect for a rule about specs, Writer for generated docs, Red-Team for an authority rule). An owner is a role ID, never a display name.
7. Mark what cannot be automated ("the text is clear") as "not automatable: reviewed by <role>", and do not pretend a check exists.

## Output format
```markdown
| Invariant | Check that would fail | Kind | Owner | State |
| :-- | :-- | :-- | :-- | :-- |
| <X never Y> | `<file>::<test>` or lint `<check>` | Rust test / lint / schema / docs drift / reference / console | <role id> | exists (read) / believed, not read / to be written / not automatable |
```

## Done when
Every invariant in the spec has a row, every "exists" row names a check that was read and would fail on the smallest breaking change, every missing check is in the verification plan, and every row has an owning role.

## Pitfalls
- Naming a test by its title. A test called after the invariant may assert something narrower.
- A check that passes on an empty input: it guards nothing when the thing it reads is deleted.
- One broad check claimed for five invariants. Say which assertion guards which.
- A fitness function for everything. Guard the properties whose loss would be costly and quiet.
- Proposing a check that would loosen or replace an existing gate. That is a gate change and needs the operator.
