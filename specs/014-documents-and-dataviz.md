---
id: spec-014-documents-and-dataviz
title: "014: Document Reading, Document Creation & Data Visualization (dependency-free)"
author: architect
timestamp: 2026-10-25T12:00:00Z
status: approved   # by the operator, 2026-10-25 ("your plan sounds solid. go"); whole document, the five decisions in §10 as recommended
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # format coverage and limits are reasoned, not yet measured on real documents
source_mission: 2026-10-25-documents-and-dataviz
tags: [spec, tools, documents, pdf, office, archives, html, charts, dataviz]
---

# Spec 014: Document Reading, Document Creation & Data Visualization

## 1. Scope and Non-Goals
Agents today read and write plain text only (`read_file`, `write_file`, 256 KB caps, `crates/delphi-engine/src/tools.rs`). This spec gives them the common document formats, in two phases: **read first, create second**, with charting and data visualization as a first-class part of creation.

- **In scope:** a `delphi-docs` crate of format readers (§3, §4), how reads enter the existing tool, taint and path-guard pipeline (§5), creation tools that emit HTML, SVG, PNG, CSV, XLSX and zip (§7), the chart engine (§8), and the phased build (§10).
- **Not in scope:** OCR, audio/video transcription, legacy binary Office formats (`.doc`, `.xls`, `.ppt`), editing a document in place (round-tripping a docx or pptx), password-protected files, and any network access. These are named in §11 as deliberate gaps.

### 1.1 The dependency rule (operator, 2026-10-25)
**No new third-party crates.** The workspace already has what the work needs:

| Need | Already in the tree |
| :-- | :-- |
| inflate/deflate (zip members, PDF `FlateDecode`, PNG `IDAT`) | `flate2` (`crates/delphi`) |
| CRC-32 (zip, PNG chunks) | `crc32fast` (`delphi-adapter`) |
| tar | `tar` (`crates/delphi`) |
| SQLite | `rusqlite` (bundled; `delphi-core`, `delphi-engine`) |
| JSON / YAML / TOML | `serde_json`, `serde_yaml_ng`, `toml` |
| regex, hashing, directory walking | `regex`, `blake3`, `walkdir` |

Everything else is written in-tree: a zip reader and writer, a minimal XML tokenizer, a PDF object/stream/text extractor, an SVG builder, a PNG encoder. This costs code, and §11 says plainly where it costs coverage. `flate2` moves to the new crate; `crates/delphi` depends on `delphi-docs` for it (a `Cargo.toml` change, not a new dependency).

### 1.2 Simpler Alternative Considered
Shell out to `pdftotext`, `pandoc` or LibreOffice when installed. Rejected as the primary path: results would depend on the operator's machine, it would run an unsandboxed process on untrusted input, and it would make "works" mean "works here". A later amendment may add an *optional* external renderer for the one thing that cannot be done in-tree (§11, pixel-accurate rendering).

## 2. Principles
1. **Converters, not per-format tools.** The agent calls `read_file` and gets text. Format detection is internal. One new read tool (`read_doc`, §5) exists only for what `read_file` cannot express: paging and members.
2. **Untrusted by construction.** Every parser treats its input as hostile: bounded memory, bounded output, bounded recursion, no panics, no I/O except through the path guard. A parser that cannot finish within its bounds returns a partial result *and says so*.
3. **Extraction never raises trust.** The tier of extracted text is the tier of the file it came from (`specs/001` §5). An archive is as trusted as the archive, and a member is never more trusted than its container.
4. **Text out, always.** A reader's output is Markdown or plain text with page, sheet, slide or member markers the agent can cite and page through. Never a binary blob, never base64.
5. **Say what was lost.** Every result ends with a one-line loss report: "12 pages, 2 images skipped, tables flattened". The agent must not believe it read more than it did.
6. **Deterministic.** The same bytes produce the same output, so results can be cached by content hash and cited stably.

## 3. The `delphi-docs` crate
A new leaf crate with no dependency on the engine, so it can be fuzzed and tested alone.

```text
crates/delphi-docs/
  src/
    lib.rs        detect(bytes, name) -> Format;  extract(Format, bytes, Request, &Limits) -> Extract
    limits.rs     Limits { max_input, max_output, max_members, max_expanded, max_depth, max_nodes }
    sniff.rs      magic-byte detection; extension is a hint only
    zipfile.rs    central-directory reader, stored+deflate members, zip64, traversal-safe names; writer
    xml.rs        pull tokenizer: elements, attributes, text, entities, CDATA; no DTD, no external entities
    pdf.rs        objects, xref (table + stream + rebuild-by-scan), Flate/ASCII85/ASCIIHex/RunLength, text operators, ToUnicode CMaps
    ooxml.rs      docx / xlsx / pptx on top of zipfile + xml
    odf.rs        odt / ods / odp (content.xml)
    sheet.rs      csv / tsv parsing, cell typing, range slicing
    textual.rs    html→text, json/yaml/toml outline, ipynb, eml, ics, rtf
    tarfile.rs    tar / tar.gz listing and member read
    sqlite.rs     read-only schema and query (uses rusqlite)
    svg.rs  png.rs  html.rs  chart.rs   (creation; §7, §8)
  fuzz/  tests/fixtures/
```

`Extract { text, pages_total, unit: Page|Sheet|Slide|Member|Line, truncated, loss: Vec<String>, members: Option<Vec<Member>> }`.

### 3.1 Limits (all enforced, all tested at the boundary)
| Limit | Default | Why |
| :-- | :-- | :-- |
| input size | 64 MB | `Guard::read_file` is 256 KB today; documents need a larger, separate cap (§5.1) |
| expanded size | 256 MB and ratio ≤ 100:1 | zip bombs |
| members per archive | 5,000 | listing cost |
| nesting (archive in archive, form XObject in form) | 3 | recursion |
| XML nodes / PDF objects | 5,000,000 / 1,000,000 | parser blow-up |
| output per call | 256 KB (the existing `MAX_READ_BYTES`) | context budget |

A parser works on `&[u8]` with indexes checked, never `unsafe`, and every public entry point is wrapped so a panic becomes an error result (`catch_unwind`), because a hostile PDF must not take down the daemon.

## 4. Formats (read)
Detection is by magic bytes; the extension only breaks ties (a `.xlsx` that is really a zip of a docx is read as what it is).

| Format | Output | What is read | Loss, stated in the result |
| :-- | :-- | :-- | :-- |
| **PDF** | text per page, `## Page N` | text-layer content streams; Flate, ASCII85, ASCIIHex, RunLength; `ToUnicode` and standard encodings; xref tables, xref streams and object streams; damaged xref rebuilt by scanning | scanned pages ("page 4: no text layer, image only"), encrypted files (refused), exotic encodings, images, form fields |
| **docx** | Markdown: headings, lists, tables, links, footnotes, comments | `word/document.xml`, `styles.xml` (heading levels), `footnotes.xml`, `comments.xml`, tracked changes shown as `{+ins+}` / `{-del-}` | images, text boxes, equations |
| **xlsx** | per sheet: dimensions, header row, typed sample, then a range slice | `workbook.xml`, `sharedStrings.xml`, sheet XML, number formats enough to tell date from number; **cached formula values and the formula text** | charts, pivot tables, conditional formats, macros |
| **pptx** | per slide: title, body, tables, **speaker notes**, alt text | `ppt/slides/*.xml` in presentation order, `notesSlides` | animations, layouts, images, SmartArt (text only) |
| **odt / ods / odp** | as docx / xlsx / pptx | `content.xml` | as above |
| **csv / tsv** | same shape as xlsx, one sheet | delimiter and encoding sniffing, quoted fields, BOM | none beyond typing guesses |
| **zip / jar / epub** | member listing (path, size, compressed size), then `member` to read one | central directory; each member read through the matching converter | nothing is extracted to disk (§5.2) |
| **tar / tgz** | as zip | headers, then member | as zip |
| **html / mhtml** | readable Markdown | headings, lists, tables, links, `alt`; `<script>` and `<style>` dropped | layout; the dropped scripts are counted in the loss line |
| **json / yaml / toml / xml** | size-aware outline when large, full text when small | key paths, array lengths, a sample element | none when small |
| **sqlite** | schema, row counts, then `query` (SELECT only) | read-only open, `PRAGMA query_only`, statement allow-list, row and time caps | blobs |
| **ipynb** | cells in order, code and Markdown, outputs trimmed | JSON | image outputs |
| **eml / mbox / ics** | headers, body (text part preferred), attachment list | MIME multipart, quoted-printable, base64 | attachments are listed; read them as members |
| **rtf** | plain text | control words, `\uN` | formatting |
| **images** | metadata only: format, dimensions, byte size | PNG/JPEG/GIF/WebP headers | **content**: reading an image's content is the model adapter's vision input, not a converter (§11) |

Out of the box nothing here executes anything: no macros, no scripts, no external references, no fonts, no network.

## 5. Tool surface
### 5.1 `read_file` learns documents
`read_file` keeps its name, arguments and tier line. When the target is not UTF-8 text, it detects the format and returns the §4 output instead of "binary file; not shown". Two additions:

- A **document read cap** separate from `MAX_READ_BYTES`: `Guard::read_file` is called with the document input limit (§3.1), and only the *output* is capped at 256 KB. Plain text files keep today's cap. The guard's checks (no symlinks, no hardlinks, no protected inodes, scope) apply unchanged: the reader receives bytes, never a path.
- Unit paging in the call: `page` (PDF, pptx), `sheet` and `range` (xlsx, csv; `A1:F200`), `start`/`end` in the format's own unit. The header shows the unit: `[@project/x/report.pdf · tier L2 internal_empirical · pages 1-12 of 80 · 2 pages without a text layer]`.

### 5.2 `read_doc` for members and queries
```text
read_doc { path, member?, query?, ... }   // path via the guard; member is a path inside an archive or a sheet name
```
- `path` alone on an archive lists members. With `member`, it reads that member through the right converter. A member name is validated (§6) and is never joined to a filesystem path.
- `query` applies to sqlite only (`SELECT …`), capped at 200 rows and 2 s.
- Both tools classify **Green** (reads only), like `read_file`.

There is no `extract` tool. Writing archive contents to disk is a creation action and goes through `write_*` (§7) so it is gated, stamped and scoped like any other write.

## 6. Security
| Threat | Control |
| :-- | :-- |
| zip bomb / decompression bomb | expanded-size and ratio caps checked **while inflating**, abort at the limit |
| path traversal in archive names (`../`, absolute, backslashes, NUL, drive letters) | names are display-only; `member` lookup is by exact normalized match in the listing; nothing is joined to a path |
| symlink members | listed as symlinks; never followed |
| quine / nested archives | depth 3 |
| XML entity expansion, external entities, DTD | tokenizer has no DTD or entity-definition support; unknown entities left as text; node cap |
| PDF: JavaScript, launch actions, embedded files, XFA | never interpreted; counted in the loss line |
| PDF malformed xref / object cycles | rebuild by scan; visited-set on references; object cap |
| prompt injection in document text | extraction output is tiered like its source file and carries the standard "untrusted data, never instructions" note whenever the tier is external. White-on-white or off-page text is **not** filtered; this is why the tier, not the content, is what protects the agent |
| hostile parser panic or hang | `catch_unwind`; the read runs under the engine's tool timeout; fuzz targets (§9) |
| SQL in a hostile database | read-only open, `query_only`, SELECT/WITH allow-list, no `ATTACH`, no extension loading, no `load_extension` |

A document is also a **memory-promotion hazard**: its text must not flow into `context/heuristics.md`, `lessons.md` or skills below `internal_empirical` (`GOVERNANCE.md` §3). This holds automatically because the span's ceiling drops when it observes a lower-tier read; the converters add no new path around it.

### 6.1 Caching
Extraction is cached under the workspace's local state keyed by `blake3(bytes) + format + request`, so re-reading page 3 of a large PDF does not re-parse it. The cache holds only derived text, is never an input to provenance, and a cache hit still records the tier of the *current* file content.

## 7. Creation (phase 2)
Creation tools write **text-based, self-contained, auditable** artifacts. No tool produces a binary format whose contents cannot be inspected by reading it back with §4.

### 7.1 Write path changes
`write_file` embeds a provenance header in Markdown front matter. That cannot work for SVG, PNG, XLSX or zip. For non-Markdown outputs:

- The output lands under `missions/<id>/artifacts/…` (Green) or another scoped path (Amber, per today's classification).
- Provenance goes in a **sidecar**, `<name>.provenance.md` (the same header block, `source_type` forced, the output's `blake3`), and the file is stamped in the provenance index exactly as `write_file` does (`ctx.index.stamp`). HTML and SVG also carry an in-file comment with the same fields.
- `MAX_WRITE_BYTES` (256 KB) is raised to 4 MB for artifacts only, and the card shows the size and the blake3. PNG output is capped at 2 MB.

### 7.2 Tools
| Tool | Input | Output |
| :-- | :-- | :-- |
| `write_html` | `{path, title, body_md?, sections?, theme?, script?}` | one self-contained `.html`: inline CSS, optional inline JS, no external URLs |
| `write_deck` | `{path, title, slides: [{title, body_md, notes?, chart?}], theme?}` | a self-contained HTML slide deck (§7.3) |
| `write_chart` | `{path, spec}` (§8) | `.svg`, and a `.png` when `format: "png"` |
| `write_sheet` | `{path, sheets: [{name, columns, rows, formulas?, widths?}]}` | `.xlsx` (stored-or-deflate zip of spec-conformant XML) or `.csv` |
| `write_report` | `{path, title, sections}` | HTML report with embedded charts and tables; print CSS so the operator can "Save as PDF" |
| `write_archive` | `{path, members: [logical paths]}` | `.zip` of files that the *role could already read*; the manifest (names, sizes, blake3) is in the card |

All six classify as the existing artifact kinds (Green under `missions/`, Amber elsewhere) and go through the same `precheck_write`: path scope, provenance gate, size limit, header rules. A deck or page written from a span that read untrusted content is stamped at the span's lowered ceiling, so it cannot be silently used as a trusted template later.

### 7.3 HTML safety (the generated page is code)
The generated HTML is a program that the operator will open in a browser, so it is checked **before** the write, by a linter in `delphi-docs`:

1. **No network.** No `http(s)://`, `//`, `ws:`, `data:` URLs in `src`, `href` (except in-page `#anchors`), `import`, `fetch(`, `XMLHttpRequest`, `WebSocket`, `sendBeacon`, `<link>`, `<iframe>`, `<object>`, `<embed>`, `@import`, `url(` pointing outward. The agent may not smuggle data out by a page the operator opens.
2. **No dynamic code.** No `eval`, `new Function`, `setTimeout("string")`, `document.write`, `innerHTML` with non-literal input, `javascript:` URLs.
3. **Template, not blank page.** Interactivity comes from a small **audited runtime** shipped in the binary (`include_dir`, already a dependency): deck navigation, sortable tables, filter boxes, tabs, tooltips, copy buttons. The agent selects behaviours by name and supplies data as JSON; free-form `script` is allowed but passes rules 1 and 2 and is shown in full on the approval card.
4. **Content-Security-Policy meta tag** is injected into every page: `default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data:`. The browser enforces what the linter checks, so a linter bypass still cannot reach the network.
5. **Escaping by construction.** Text from the agent goes in as text, not markup. Markdown is rendered by the in-tree converter with raw HTML disabled.
6. **Accessible and responsive by default**: semantic headings, `prefers-color-scheme` themes, phone-width layout, keyboard navigation in decks, print stylesheet.

The deck (`write_deck`) is one page with N `<section>` slides, arrow-key and swipe navigation, a slide index, speaker notes toggled by `n`, fragment-free (no animation), and `@media print` that lays one slide per page so "Save as PDF" gives a clean handout.

## 8. Charts and data visualization
### 8.1 Spec, not code
The agent describes a chart as data; Delphi draws it. The agent never writes SVG path math.

```json
{
  "type": "line",                       // bar | hbar | line | area | scatter | hist | box | heatmap | pie(donut) | stacked-bar | sparkline | table
  "title": "Median latency by release",
  "subtitle": "ms, lower is better",
  "data": { "columns": ["release","p50","p95"], "rows": [["0.2.1",120,340], ["0.2.2",118,310]] },
  "x": "release", "y": ["p50","p95"], "series_by": null,
  "scale": { "y": "linear|log", "y_zero": true },
  "annotations": [{ "x": "0.2.3", "label": "daemon rewrite" }],
  "format": { "y": "0,0 ms" },
  "size": "wide|square|tall",
  "theme": "auto|light|dark",
  "source": "missions/…/latency.csv"     // cited under the chart
}
```
Data may also be `{ "from": "<logical path to csv/xlsx>", "sheet", "range" }`, read through the §5 path (so its tier is its source's tier and is printed under the chart).

### 8.2 Rendering rules (what makes the output good by default)
- **Form follows the question.** `type` is validated against the data shape and rejected with a reason when wrong (a pie of 14 slices, a line chart over unordered categories, a dual axis). The error message names a better type. This is the main quality lever.
- **A validated default palette.** Categorical colours chosen for contrast and for colour-vision deficiency; sequential and diverging ramps for continuous values; series are also distinguished by marker shape and direct labels, never by hue alone. The palette lives in one file (`themes/` convention) so it can be swapped.
- **Light and dark** from CSS custom properties inside the SVG, driven by `prefers-color-scheme`, so a chart pasted into a dark page still reads.
- **Direct labelling and a title that states the point.** Legends only when direct labels do not fit. Axis tick selection ("nice numbers"), thousands separators, units from `format`, rotated labels avoided by switching to `hbar`.
- **Honest axes.** Bars start at zero (rejected otherwise unless `y_zero: false` is explicit and shown on the chart). Log scale labelled as such. Truncated or dual axes are refused.
- **Accessible.** `<title>` and `<desc>` generated from the spec and a summary of the data (min, max, trend); optional data table alongside; sufficient contrast checked in tests.
- **Bounded.** ≤ 5,000 marks per chart; larger data is binned (histogram) or downsampled (line, with the note "downsampled from N points" on the chart).

### 8.3 Interactivity (HTML output only)
A chart embedded by `write_html` / `write_report` / `write_deck` can opt into the audited runtime: hover tooltips, series toggle by clicking the legend, brush-to-zoom on line and scatter, a "show data" table toggle. The static SVG is always the fallback and is the thing written when `format` is `svg`.

### 8.4 PNG
`format: "png"` rasterizes the same scene with an in-tree scanline rasterizer (anti-aliased lines, filled polygons, circles, rectangles; text from a built-in bitmap font at 2x and 3x; no font files) and encodes with `flate2` + `crc32fast`. PNG exists for the places SVG does not go: messages, slides that become pictures, Markdown rendered elsewhere. Text quality is deliberately plain; **SVG is the primary output.** A chart whose text would be unreadable in PNG (long labels, > 12 categories) is refused for PNG with a pointer to SVG.

### 8.5 Diagrams
`write_diagram { type: "flow" | "sequence" | "tree" | "timeline" | "matrix", ... }` from a small node/edge spec, laid out by a deterministic in-tree layered layout (no force simulation, so the same input gives the same picture). Out of scope for the first creation slice (§10, slice C5) and cut first if time is short.

### 8.6 Dashboards
`write_report` accepts `charts` plus `tables` plus `stats` (a labelled number, an optional delta and sparkline) in a CSS grid, with the same linter and CSP. This is what turns a CSV into a one-page readout.

## 9. Verification
| Layer | What |
| :-- | :-- |
| Unit | each reader against hand-made minimal files in `tests/fixtures/` (one per feature: xref stream, object stream, ToUnicode, shared strings, tracked changes, quoted CSV, BOM, zip64) |
| Golden | extraction output stored as text; a diff is the review |
| Hostile | a corpus of zip bombs, traversal names, billion-laughs XML, cyclic PDF references, truncated and bit-flipped files; each must return an error or partial result inside the limits |
| Fuzz | `cargo fuzz` targets per reader (zip, xml, pdf, csv, tar, rtf); run in CI for a fixed budget, longer on a schedule |
| Round trip | files we write are read back by our own readers (`write_sheet` → xlsx reader; `write_deck` → html reader). External validation (open in Excel, LibreOffice, a browser) is an **operator-gated** manual check recorded in the PR, not claimed from CI |
| Chart | SVG golden files; PNG decoded by the test and compared on a pixel grid; contrast and palette tests; every rejected chart type has a test for its message |
| HTML linter | a table of forbidden constructs, each with a page that must be rejected, plus accepted pages the runtime ships |
| Engine | classification, scoping, tier and stamp tests for each new tool, like the existing `tools.rs` tests; a hostile document read must lower the span ceiling and block a later write to a gated path |
| Real-world | a private set of 20 documents per format from the operator's actual work, run as an **ignored** test with a pass-rate and loss-rate report. Format claims in this spec are not final until that report exists |

## 10. Build plan
Each slice is one PR, ends green on `cargo test` + `delphi lint`, and states in its description what is proven and what is operator-gated. Slices within a phase are ordered by value and risk; later ones can be dropped without breaking earlier ones.

### Phase 1: Read
| Slice | Content | Done when |
| :-- | :-- | :-- |
| **R0** | `delphi-docs` crate skeleton, `Limits`, `sniff`, `Extract`, `catch_unwind` wrapper, fixtures dir, CI fuzz job. `read_file` gets the document read cap and a "format X not supported yet" result for known formats | sniffing tests pass; a binary file reports its detected format instead of "binary file" |
| **R1** | `zipfile` reader + tar; `read_doc` listing and `member`; all §6 archive controls | bomb, traversal and nesting corpus passes; listing is stable |
| **R2** | `xml` tokenizer + OOXML: **docx**, **xlsx**, **pptx**; csv/tsv; paging by sheet, range, slide | golden outputs for each feature fixture; hostile XML corpus passes |
| **R3** | **PDF** text extraction (Flate; xref table, stream and rebuild; ToUnicode); per-page paging and the "no text layer" report | extracts text from a corpus of generated and real PDFs; encrypted PDFs refused clearly; fuzz target runs clean for the CI budget |
| **R4** | html→Markdown, json/yaml/toml outline, ipynb, eml/mbox/ics, rtf, odf, epub (zip + html), image metadata | per-format goldens |
| **R5** | sqlite read-only `query`; extraction cache (§6.1); loss-report polish; the 20-document real-world report | report reviewed by the operator; claims in §4 adjusted to match |

### Phase 2: Create
| Slice | Content | Done when |
| :-- | :-- | :-- |
| **C0** | Artifact write path: sidecar provenance, `missions/<id>/artifacts/`, raised artifact cap, card with size + blake3; `zipfile` writer; `write_archive` | write → read-back round trip; stamps recorded; gate and scope tests |
| **C1** | HTML linter + CSP + escaping + audited runtime v0 (tables, tabs, copy); `write_html` | linter rejection table passes; one handwritten page per accepted construct |
| **C2** | Chart engine: spec validation, scales and ticks, palette, SVG for bar/hbar/line/area/scatter/hist; `write_chart` (svg) | goldens; rejection messages; contrast tests |
| **C3** | `write_deck` + `write_report` (grid, stats, tables, embedded charts); runtime: deck navigation, print CSS | deck opens, navigates and prints one slide per page; **operator checks in two browsers** |
| **C4** | `write_sheet` (xlsx + csv), the rest of the chart types (box, heatmap, donut, stacked, sparkline), chart interactivity | xlsx opens in Excel and LibreOffice (**operator-gated**); runtime tooltips and zoom |
| **C5** | PNG rasterizer + bitmap font; `write_diagram` | PNG decode tests; diagram layout determinism |

### Decisions ratified before R0 (operator, 2026-10-25)
1. **Crate and tool names**: `delphi-docs`, `read_doc`, `write_html` / `write_deck` / `write_chart` / `write_sheet` / `write_report` / `write_archive` / `write_diagram`. Alternative: a single `write_artifact { kind: … }`. This spec prefers separate tools because each has its own schema, card and classification.
2. **Artifact write path** (§7.1): sidecar provenance and a 4 MB artifact cap. This amends `specs/003` §1.2 (the tool set) and the write-tool description in `specs/001` §5.
3. **Document read cap** (§5.1): 64 MB input, 256 KB output. This amends the current 256 KB read limit for non-text formats only.
4. **Roles**: which roles see the write tools. Proposal: all roles with a write scope get `write_chart` and `write_html`; `write_sheet`, `write_deck`, `write_report` and `write_archive` for roles that already write mission artifacts.
5. **Phase gate**: Phase 2 does not start until the R5 real-world report is accepted.

## 11. Known gaps (decided, not forgotten)
- **Scanned PDFs and images** have no text without OCR. The result says so. Reading what is *in* an image is vision input to the model, handled by the adapter, not a converter; a follow-up spec decides how an image file is offered to an adapter that supports it.
- **PDF fidelity**: reading order in multi-column layouts is heuristic; tables become text runs; exotic font encodings without a `ToUnicode` map produce gaps, reported per page.
- **No PDF creation.** In-tree layout and font embedding is a large amount of work for a format that "Save as PDF" from `write_report` already covers. Revisit on demand.
- **No pptx or docx creation.** The self-contained HTML deck and report are the supported outputs. OOXML creation can be added after xlsx proves the zip writer, if the operator wants it.
- **No in-place editing** of existing documents.
- **Legacy `.doc` / `.xls` / `.ppt`**, password-protected files, DRM: not supported; named in the result.
- **PNG text is plain**; SVG is the quality output.
- **Optional external helpers** (`pdftotext`, a headless browser to render a page to PNG for preview) could improve fidelity and give the agent a way to *look at* its own output. They are excluded by the dependency rule and can be proposed as a separate amendment.
