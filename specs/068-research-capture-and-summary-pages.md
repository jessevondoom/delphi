---
id: spec-068-research-capture-and-summary-pages
title: "068: Research Capture and Imported Summary Pages"
author: architect
timestamp: 2026-11-02T00:00:00Z
status: approved
approved_by: operator (2026-11-02)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec, knowledge, mcp, library, import, summary, encyclopedia, staleness, validation]
---

# Spec 068: Research Capture and Imported Summary Pages

> **Status: Approved by the operator on 2026-11-02.** Born from Council t_0001 and t_0002 (2026-11-02) and the operator's rulings of the same day. Builds on [`specs/055`](055-the-central-library-sources-claims-and-pages.md), [`specs/061`](061-encyclopedia-search-pages.md), [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md) and [`specs/066`](066-mcp-research-endpoint-and-role-identity.md). Where it overlaps them it reuses their records; it adds no second store.

## 1. Problem

Research done through Delphi (TUI or a host agent over MCP) is accurate and valuable, often from internal sources, yet nothing lands in the knowledge base. The operator saves it as markdown outside Delphi. Delphi should build the library from that work with the least friction, leaving most of it *not validated* until reviewed.

## 2. Operator decisions (2026-11-02)

- **D-1.** Summaries are **searchable by default**, labelled, and kept usable.
- **D-2.** A summary page is the same object as a spec 061 encyclopedia page (same skill, layout, `valid_until`), and must carry **citations and disambiguation** to be accepted.
- **D-3.** A summary page **never verifies anything**. Key facts may be *extracted* from it as candidate claims for the operator, or the validator under supervision, to validate.
- **D-4.** Staleness is a dated flag set at summary time (`valid_until`). A stale page is **refreshed on access, on reference, or on demand, never purged**. Pages are removed only when refuted or found problematic.
- **D-5.** A summary may be marked `external_unvalidated` and still appear in search, even as a default result, **provided it links to other knowledge-base resources**, making it a conceptual index.
- **D-7.** Reading a summary taints the span, so each page carries a factual-metadata **bibliography** that is always read instead of the page (§6.7); every page write updates it, and a page with no sources says so explicitly.
- **D-6 (council, 2026-11-02, 5 of 5 approve-with-changes).** Import never blocks and asks nothing: missing frontmatter, citations or disambiguation produce a receipt warning and a review-queue entry, never a refusal. Human-edit preservation on refresh (`keep` regions, three-way merge) is out of scope.

## 3. Doors (one inbox, one schema)

| Door | Who | Result | Phase |
| :-- | :-- | :-- | :-- |
| `propose_kb_entry` (MCP, staging only) | a host agent | L0 inbox entry, `inclusion: staged`, not searchable until the operator ratifies | 1 |
| `delphi kb import <file.md>` (TUI drag-in calls the same code) | the operator | `inclusion: ratified`, `validation: none`; searchable at once | 1 |
| `propose_kb_entry` with `kind: summary` | a host agent | staged as above | 2 |
Import grants **inclusion, never validity**. It asks no confirmation and ends with a short **receipt** (mandatory output):

```
Imported "Vault rotation" → page:vault-rotation
  7 sources found · 5 links to KB pages · searchable now
  Heads-up: 2 sources unresolved, title overlaps "Rotation (keys)"
  Bibliography: 7 sources, 2 internal · page-id.bib.md
  Try: delphi kb search vault
```

Gains first, warnings last, one next action. Import accepts a file, a directory (one summary line: "41 imported, 6 need citations, 3 duplicates") and stdin (`kb import -`). The TUI offers `/save` for the last research answer and drag-in; all call the same code. Re-import of identical bytes reports "already in library"; changed bytes write a new version under the same id. No watched folder exists, and none may be added that ratifies.

**Normalization is the daemon's job, not the operator's.** A file with no frontmatter imports: `kind` defaults to `summary`, `title` from the first `#` then the file name, `retrieved` from the file's mtime, citations inferred from URLs, footnotes and `path:line` mentions, links from exact title or id matches. The body bytes are never rewritten on import (`original_sha256` kept); structure is added on refresh.

## 4. Entry schema

One frontmatter for both doors. Fields marked *daemon* are always computed or overwritten; values in a file or tool call are ignored.

- `kind`: `claim` or `summary`.
- `title`, `retrieved`, `sources[]` (library source ids, paths or URLs), and for summaries `links[]`, the ids of other KB resources the page points to.
- `valid_until` (date, set at summary time by the writer, per 061 volatility rules), `source_hashes[]` (*daemon*), `bibliography` (path of the sidecar, §6.7, *daemon*).
- `author`, `taint`, `inclusion` (`staged` or `ratified`), `validation` (`none`, `validated`, `disproved`) and `origin` (*daemon*).
- `status` label shown to readers, derived: `external_unvalidated` when `validation: none` and the origin is outside the operator's own validated material.

Entries are content-addressed and deduplicated by source hash (055). Backlinks are computed at index time; atomic entries are never edited.

## 5. Staged entries from agents (`propose_kb_entry`)

- Gated by computed taint (GOVERNANCE §4.3). Any web or internal-document input lands at the lowest trust tier; the agent cannot raise it.
- The agent supplies title, claim or body, source pointers, retrieval date and stated confidence. The daemon sets author, mission and taint from the span.
- Caps (bytes, lines, entries per mission), secret scrubbing (§5.3), dedupe, and expiry of untouched staged items. Workspace scope by default.
- Staged entries are **excluded from `kb_search`** until ratified. A host sees only its own proposals through its handoff.
- Not an overload of `propose_note` (lessons) or `record_session_summary`.

## 6. Summary pages

### 6.1 Acceptance
Import always succeeds. Citations (each resolvable to a source, path, URL or KB id) and, for a title that collides with an existing page, a disambiguation hatnote are **advisory**: a page lacking them gets a receipt warning, the label "sources not yet linked", a review-queue entry, and the writer adds the hatnote and structure on first refresh (061 §3). A title collision offers merge-as-new-version, disambiguate or rename, defaulting to disambiguate.

### 6.2 Conceptual index
A summary is **index-worthy**, and so a default search result while `external_unvalidated`, when at least one of its `links[]` resolves. One pure daemon function decides:
- Only links to **ratified, non-summary** KB entries count (up to 3 counted). Self-links and links to other unvalidated summaries do not.
- Index-worthiness grants **visibility only**. It never raises rank above a validated atomic claim, never changes the label, and never changes taint.
- Links are re-resolved on import and refresh; unresolved links are listed, not hidden.
- Pages that are not index-worthy are found with `kb search --all`. A per-origin daily cap is deferred until abuse is seen.

### 6.3 Search presentation
- Returned in a labelled `summary_page` block, never inside a `<fact>` element, ranked below validated atomic claims, with links shown as "see also".
- **Two renderings of one fact.** Data and MCP output carry the exact machine value: `[summary · operator-imported · external_unvalidated · valid until DATE]`. The console shows plain words, `Vault rotation · imported summary · not yet checked · 5 links · fresh to Jan 12`, with a tooltip naming the machine value. A softened label never reaches a model's context.
- Chunked and size-capped; the body is data, rendered as quoted data in a delimited block.
- **A model that opens a `summary_page` body has its span's computed taint set to the page's taint** (GOVERNANCE §4.3; "taint" restricts what the model may do next: mutating and outbound tools are stripped). A test must prove this; it is a ship condition. Search results do not open the body: they return the bibliography (§6.7), so a reader reaches a page's facts without paying the taint.

### 6.4 Taint
Taint comes from the recorded origin and is never raised by import. An agent-generated summary stays at `internal_generated`; one built from web or federated input stays at the lowest of those. An import with no recorded origin defaults to `external_untrusted`. Operator ratification may raise it explicitly, per entry.

### 6.5 No verification from a summary
- A summary cannot supply evidence for `validated`, cannot be cited as support for a claim, and cannot be promoted into heuristics, REVIEW, lessons or skills.
- **Extract key facts:** on request, `delphi kb extract <id>` proposes candidate claims from the summary. Hard rules:
  - Each candidate carries `anchor{source_id, quote, source_hash}`. The daemon rejects any candidate whose quote is not a **verbatim substring** of the stored source snapshot. A candidate supported only by summary text is rejected.
  - Candidates are kind `attributed`, status `noted` (052). A `from_summary` pointer is provenance only and never counts as evidence.
  - The validator sees only the bare claim and its anchor, never the summary body.
  - At most 7 candidates per page, ranked by need (040 §5), deduplicated against validated and existing candidates. Aged `noted` claims leave the queue but are never deleted. `delphi needs` shows a count.
  - Nothing is validated automatically.

### 6.6 Validating internal sources
The validator cannot corroborate private documents on the open web and never marks an internal-source claim `would_ratify`. The operator gets an **attestation card**: the claim beside its verbatim quote and source pointer, grouped by source, one key each (`y` confirm, `n` reject, `s` skip). The daemon has already confirmed the quote exists in the snapshot, so the operator judges only meaning and truth. A confirm records `validated · attested by operator · internal source` (L5, 040 §9), labelled differently from `web_corroborated`.

### 6.7 The bibliography sidecar (D-7, operator, 2026-11-02)
Every summary page has a sibling **bibliography file** (`<page-id>.bib.md`, always written by the daemon; its path is the `bibliography` field in §4) holding **factual metadata only**, one record per cited source: `locator` (a URL, a path, or an opaque internal id such as a Slack permalink or wiki page id, so internal sources need no public URL), `title`, `author`, `publication`, `published`, `retrieved`, `source_id` and `source_hash` (when in the library), and `kind` from a closed list (`web`, `paper`, `repo`, `private-doc`, `slack`, `wiki`, `email`, `other`). It holds no prose, no summary text and no quotes. A bibliography is never evidence of validity (§6.5).

- **Found by content, read by bibliography.** Search matches against the page body, so a page is found by what it says. The result returns the title, the bibliography, the `links[]` ids and a daemon-built topic line of schema-checked keywords, and no page prose. Discoverability and the conceptual index (§6.2) are unchanged; only what a reading model receives changes.

- **Read the bibliography, not the page.** When a bibliography exists, a search result, an MCP read or a mission pointer for the page resolves to the **bibliography**. The page body is opened only by an explicit operator request or an explicit agent "open the page" call, and that read taints the span (§6.3). The agent then follows the bibliography to the real sources and researches from them, so one read of a summary no longer costs a tainted span, and no data is lost.
- **Always present.** A page with no citations still gets a bibliography whose body states `sources: none` and `links: none`. The file exists so a reader learns, without opening the page, that there is nothing further to follow.
- **Written on every page write.** Import, refresh and any later edit create or update it in the same operation. A page without its bibliography is not indexed as complete; the review queue lists it.
- **Extracted by the agent, enforced by the daemon.** The agent (the `writer` or importing host) extracts citations; the daemon, never the agent, validates them against a closed schema: fixed field names, URL and ISO-date syntax, `title` at most 120 characters, `author` at most 80, plain characters only (no markdown or control characters), and every `locator` must appear verbatim in the page body or a stored source snapshot. Anything failing is dropped from the file and listed as a receipt warning. A bibliography that fails the schema keeps the page's taint; one that passes is treated as data and no longer taints the span.
- **Human context stays.** The page prose is untouched and still readable by the operator; the bibliography is an additive view.
- **Links are separate.** KB links (`links[]`) are listed in the bibliography as ids, so the conceptual index (§6.2) stays machine-readable without reading the page.
- Over time, reads move from the page to the bibliography and to primary sources, which lowers taint without losing the page or its facts.

## 7. Staleness and refresh

- At summary time the writer sets `valid_until` by content volatility (061 §4). The daemon stores `source_hashes` for every cited source.
- A page is **stale** when `today > valid_until`, a cited source's hash changed, or a cited claim was superseded.
- A stale page is served at once, labelled "last checked DATE" (never "expired"). The reader never waits.
- **Refresh trigger.** Access or reference by a mission only **enqueues** a refresh, and only when source hashes changed or `valid_until` passed. A 24 h per-page cooldown and a per-mission cap apply; MCP access can only enqueue. **The operator set the cap as a time limit: one pass works for at most four minutes** (`delphi kb refresh --stale`), and no model is used, so there is no token spend. A pass renews a stale page only when every library source it cites (linked in the bibliography as `source_id` and `source_hash`) is unchanged; a changed source leaves it stale, marks its key facts for recheck and asks for a rewrite; without it, refresh is on demand only (`delphi kb refresh <id>`, `r` in the console).
- **Refresh safety.** The refreshing `writer` is read-only. The new version's taint is the minimum of the prior version's taint and every source read; refresh never raises taint. Ratification applies to a version hash, and a new version needs fresh ratification only if its taint dropped. Output is scrubbed as on import.
- Refresh keeps the id and writes a new version; the prior copy stays readable and the receipt says "page rewritten, prior copy kept". Refresh rewrites the page only: a changed source hash re-queues anchored claims as `needs_recheck`, and no claim is invalidated or deleted.
- Refresh never deletes. Removal happens only when the page is **refuted** (a cited claim is `disproved` and the page rests on it) or **found problematic** (secret, licence, injection). The page is then quarantined at once, the operator is notified, and the operator confirms, fixes or deletes it.

## 8. Authority and safety

- No new path may act outside the sandbox, approve itself, or let content raise its own trust. MCP stays read-only apart from staging at L0.
- Operator import is the only act that sets `inclusion: ratified`; it never sets `validation`.
- The daemon overwrites `author`, `taint`, `inclusion`, `validation` and `origin` from file frontmatter.
- Import warns when a page cites unvalidated sources, so one-step friction does not hide the trust level.

## 9. Slices (one per pull request)

Slice 1 is the MVP and does not wait on the rest. It reuses `Library::add_file` and the 055/061 page records; `delphi kb` today has only `index`, `reindex`, `stats`, `search` and `serve`, so the `CMDS` table, `docs/reference/` and `llms.txt` change with it.

If slice 1 runs long, the bibliography sidecar ships as its first sub-step with no change to import behavior.

1. **Import and see it.** `kb import` (file, dir, stdin, TUI `/save`), normalization, receipt, daemon-owned fields, taint, scrub, caps, dedupe, versions; `summary_page` search block, the index-worthy function, both label renderings, the span-taint rule; the bibliography sidecar (closed schema, `sources: none` case, read-instead-of-page resolution); a bare import counter.
2. **Agent capture.** `propose_kb_entry` (MCP, staging only) with the L0 inbox and a batch keep-or-discard review (`delphi kb review`); the tool replies with a receipt pointing there.
3. **Staleness.** Stored hashes, "last checked" label, manual `kb refresh`, then background refresh once the spend cap exists; quarantine on refute.
4. **Extract and attestation.** `kb extract` with the anchor guard (the guard lands before any attestation card), the validator queue, internal-source cards.
5. **Deferred.** Agent-submitted `kind: summary`, quarantine UX polish, capture-rate metrics beyond the counter, per-origin caps, human-edit preservation.

   Built: agent-submitted summaries ride `propose_kb_entry` (slice 2) and become pages only on the operator's keep; quarantine listing and a quarantined count in `kb stats`; `kb review stats` (proposed, waiting, decided, pages); a per-workspace cap of 2000 pages in place of per-origin caps (one origin exists today); a warning, with the edits kept under `.versions/`, when a new version replaces a page edited after import; `needs_recheck` on claims whose source changed at refresh; open key facts counted in `delphi needs`. Not built: a console attest card, validator-assisted queueing, ranking candidates by need.

## 10. Acceptance criteria

- Importing a bare markdown file (no frontmatter) makes it findable in one command with a three-line receipt, labelled `external_unvalidated`, with taint at its recorded origin.
- A model that reads a `summary_page` loses mutating and outbound tools for that span (tested).
- A fabricated quote is rejected by `extract`; a bibliography record is never counted as evidence; a title field carrying an injected instruction is dropped; an internal-only page (private doc, Slack, wiki) yields a valid bibliography with opaque locators; refreshing a page leaves its claims' status unchanged except `needs_recheck` on a changed source hash.
- Every import and refresh writes or updates the bibliography; a page with no citations gets one that says `sources: none`.
- A search, MCP read or mission pointer for a page that has a bibliography returns the bibliography, and the reading span is not tainted by the page body.
- A bibliography field carrying prose, markdown, control characters or an instruction is dropped and reported, never written.
- A refresh never raises taint, and a refresh from a poisoned source cannot give the page tools or trust.
- Self-links and links to other unvalidated summaries do not make a page index-worthy.
- A file that sets `taint: user_ratified` or `validation: validated` in frontmatter gets neither.
- A staged MCP proposal is absent from `kb_search` until ratified.
- A stale page is still returned, labelled, and refreshed on next access; it is deleted only after refutation or a problem finding.
- No code path counts a summary as validation evidence; `extract` anchors to sources.
- Flooding, spoofed frontmatter and an injected instruction inside a page are covered by tests.

## 11. Simpler alternative considered

Ship only `kb import` plus `/save`, and no MCP staging door, until the capture rate shows the need. Rejected as the end state because MCP research is where the operator's pain began; kept as the order of work: slice 1 first.

## 12. Known limits

- A summary labelled unvalidated can still read as authoritative; the label is mandatory in every renderer.
- Refresh on access adds model cost; refresh is rate-capped per mission.
- Link resolution depends on the target still existing; dangling links are reported, not repaired.
- Bibliography extraction costs one model call per page write; the import caps bound it.
