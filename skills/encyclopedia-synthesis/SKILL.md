---
name: encyclopedia-synthesis
description: "Write an encyclopedia-style summary page in Markdown from gathered records: lead, infobox, contents, inline figures, citations, hatnotes, and a cache lifetime."
authorized_agents:
  - writer
input_schema:
  type: object
  required: ["query", "records"]
  properties:
    query:
      type: string
      description: "The search words the page answers"
    depth:
      type: string
      enum: ["summary", "expanded"]
      description: "summary is short (about 150 to 400 words); expanded adds sections and figures"
    records:
      type: array
      items: {type: string}
      description: "Claims, source excerpts and image assets the daemon gathered, each with its id"
output_schema:
  type: object
  required: ["page"]
  properties:
    page:
      type: string
      description: "The page body in Markdown, ending with one `<!-- page: ... -->` line that names its kind and lifetime"
required_permissions:
  - read:all
---
# Encyclopedia Synthesis

A reader should know what the subject is after the first sentence, and be able to check every fact after the last.

## When to use
- A search needs a page that answers the query from the library's claims, sources and images.
- An existing page is being refreshed or expanded with new records.

## Prose
1. Plain, direct words. Short sentences. Say a thing once, in the order a curious stranger would ask about it.
2. The first sentence names the subject in bold and says what it is: `**Subject** is ...`. No "this page", no "based on the records", no hedging openers.
3. Neutral voice. A validated claim is stated as fact with its marker. A noted claim is attributed ("According to ...") . A contested pair is shown side by side, each with its marker. Never hide a disagreement and never invent a fact, a link, an image or an id.
4. Short is fine. A page of two paragraphs that answers the query beats six paragraphs of filler.

## Layout (in this order; leave out what the material does not support)
1. **Hatnote**, only when the records show a second meaning of the query: one italic blockquote line before the title:
   ```markdown
   > *This page is about X. For Y, see [Title](page:slug).*
   ```
   where `slug` is the lowercase hyphenated query for the other topic.
2. **Title**: `# Title`.
3. **Lead**: one to three paragraphs before the first `##`. It stands alone as a summary.
4. **Infobox**: a two-column table, `| Attribute | Summary |`, three to six rows of key facts. A table needs a caption line with a citation next to it.
5. **Contents**: `## Contents` as a numbered list of anchor links, only when there are two or more `##` sections. The slug is the heading in lowercase with spaces as hyphens:
   ```markdown
   1. [Section](#section-slug)
   ```
6. **Sections**: `##` and `###` headings. Put a figure where it helps: the image on its own line:
   ```markdown
   ![what it shows](lib:<source-id>/<file>)
   *Figure 1: what to notice [s-id].*
   ```
   Use only assets listed in the records, and cite the source the asset belongs to.
7. **Citations**: `[c-id]` for a claim and `[s-id]` for a source, after the sentence or paragraph they support, using only ids you were given.
8. Do not write the banner or a bibliography table. The daemon adds them; its table is the page's References.

## Disambiguation page
When the records split evenly across unrelated meanings and none dominates, write a disambiguation page instead: a one-sentence lead (`**Query** may refer to:`), then a bulleted list, one line per meaning, each linking with a cited one-line description:
```markdown
* [Meaning](page:slug) - short summary [s-id].
```
End with `kind=disambiguation` on the closing line.

## Cache lifetime
End the page with one line the daemon reads and removes:

`<!-- page: kind=article; volatility=low; days=90; reason=one short reason -->`

Pick the volatility from how fast the subject changes:
- `high`, 1 to 7 days: live incidents, current work, fast-moving numbers, pages that lean on unconfirmed notes.
- `medium`, 14 to 30 days: evolving software, active specs, tool documentation.
- `low`, 60 to 180 days: ratified specifications, stable interfaces, settled procedure.
- `timeless`, 180 to 365 days: history, mathematics, science, archived records.

The daemon clamps `days` to its configured bounds and writes the expiry. If you leave the line out, the page is treated as `medium`, 30 days.

## Expanded depth
When `depth` is `expanded`, the records include material fetched from reputable outside sources. Add sections for context, history or open questions, place the new figures, and keep every added statement cited. Do not drop what a summary page already said unless a newer record contradicts it.
