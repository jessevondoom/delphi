---
name: knowledge-compaction
description: "Compact mission logs and scratchpads into a short summary with provenance frontmatter. Use at mission close to keep context lean."
authorized_agents:
  - orchestrator
input_schema:
  type: object
  required: ["mission"]
  properties:
    mission:
      type: string
      description: "Mission id whose notes to compact"
output_schema:
  type: object
  required: ["summary"]
  properties:
    summary:
      type: string
      description: "Path of the compacted summary"
required_permissions:
  - read:all
  - write:missions
---
# Knowledge Compaction

Keep what a future agent needs to continue or avoid repeating the work; drop the rest.

## When to use
- A mission closes, or its notes have grown past what a handoff can point at usefully.

## Steps
1. List the artifacts worth keeping: decisions with reasons, results, open questions, and links to the final files.
2. Write the summary in a few short sections. Link to detail with `path#L-L` pointers instead of copying it.
3. Carry the lowest trust tier forward: a summary containing external or unverified material is itself `external_untrusted` or `internal_generated`, never higher.
4. Add provenance frontmatter: id, title, author, timestamp, source_type, confidence, status, source_mission, tags.
5. Run `delphi check provenance <summary>` and fix what it reports.
6. Leave the originals in place; compaction adds a summary, it does not delete history.

No shell? Call the `check` tool with `kind: provenance` and the summary's path; it returns the same verdict.

## Done when
The check passes and the summary is a fraction of the source size while still answering "what was decided and why".

## Pitfalls
- Raising `confidence` or `source_type` because the summary reads well.
- Summarizing without pointers, so nothing can be verified.
