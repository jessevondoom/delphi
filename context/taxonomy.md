---
id: kb-taxonomy
title: "Tag Taxonomy (audit of current use)"
author: architect
timestamp: 2026-10-24T18:00:00Z
source_type: internal_empirical   # counts below come from a script over the repo's frontmatter, 2026-10-24
confidence: medium
status: draft
source_mission: 2026-10-23-foundational-design
tags: [taxonomy, index, knowledge-base]
---

# Tag Taxonomy

Audit of the `tags:` field across `context/`, `specs/`, `memory/` and `missions/` (first `tags:` line of each Markdown file). It is the starting vocabulary for the librarian role ([`specs/013`](../specs/013-librarian-and-internal-retrieval.md) §5). Nothing here is enforced yet; `delphi lint` checks that `tags` exists, not what it holds.

## Finding
Tags currently mix three different things. Only the first helps retrieval by subject; the other two describe where a file came from.

| Facet | Meaning | Seen today | Proposed rule |
| :-- | :-- | :-- | :-- |
| **Subject** | what the file is about | `tui`, `ratatui`, `mcp`, `auth`, `connectors`, `model-routing`, `security`, `ucan`, `sqlite-vec`, `thinking-budgets`, `unix-domain-sockets` | keep as `tags`; draw from a controlled list |
| **Kind** | what sort of document | `spec`, `council`, `verbatim`, `review`, `verdict`, `lessons`, `plan`, `roadmap`, `milestones` | keep as `tags`; one kind per file |
| **Provenance / process** | which mission step or council produced it | `council-002`…`council-006`, `step-1`…`step-7`, `round1`, `round2`, `merge-gate-4`, `checkpoint` | move out of `tags`; `source_mission` and the file path already carry this |

## Counts (top of the distribution)
`council` 36 · `verbatim` 22 · `spec` 15 · `lessons` 9 · `council-002/003/004` 8 each · `review` 6 · `verdict` 5 · `ui`, `tui`, `narrator`, `council-006` 5 each. Most other tags occur once or twice, which is the sign of a vocabulary that grew by accident.

## Near-duplicates to merge when the vocabulary is fixed
- `tui`, `ui`, `console`, `ratatui`, `wireframes` (subject: the terminal UI)
- `spec`, `specs`, `specs-010`, `spec-005c` (kind: spec; the number is already in `id`)
- `taxonomy`, `skills-taxonomy` (subject)
- `scope`, `scoping` (kind/subject unclear)

## Next steps (not done here)
1. Operator or architect picks the controlled **subject** list from the rows above.
2. Add the optional `domain`/`summary` fields proposed in `specs/013` §5 via a `specs/001` amendment.
3. Extend `delphi lint` with a warning (not a failure) for tags outside the controlled list.
