---
id: spec-075-kb-search-operator-presentation-and-summary-filtering
title: "075: Knowledge Base Search Operator Presentation and Summary Filtering"
author: architect
timestamp: 2026-11-04T16:55:00Z
status: approved
approved_by: operator (2026-11-04)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, kb, search, summaries, tui, cli, bibliography, taint]
---

# Spec 075: Knowledge Base Search Operator Presentation and Summary Filtering

> **Status: Approved by the operator on 2026-11-04.**  
> **Extends:** [`specs/068`](068-research-capture-and-summary-pages.md) (Research capture and summary pages), [`specs/004`](004-memory-and-coaching.md) (Memory and coaching), [`specs/005e`](005e-console-ux-review-and-redesign.md) / [`specs/005f`](005f-console-redesign-build-plan.md) (Console & TUI), and [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md) (Knowledge retrieval for agents).

---

## 1. Scope & Problem Statement

Spec 068 established the bibliography sidecar pattern (§6.7, Decision D-7): every imported summary page (`summaries/<id>.md`) has a daemon-generated sibling bibliography file (`summaries/<id>.bib.md`) containing factual metadata only (URLs, authors, publication dates, and link IDs).

In the initial implementation of Spec 068, `delphi_engine::kb::Kb::search` unconditionally intercepts hits in the `summaries` zone and substitutes the sibling `.bib.md` chunk for the result. While this was designed to protect autonomous AI agents from incurring `Tier::L0 external_untrusted` span taint when reading search results, it creates a severe usability problem for human operators:

1. **CLI searches (`delphi kb search`)** return only `.bib.md` pointers and YAML metadata previews, leaving the operator with zero prose visibility into the summaries Delphi actually holds.
2. **TUI search panel in the console (`/` search in `app.tsx`)** displays metadata lists instead of the substantive researched markdown summaries.
3. **No selective control:** The operator has no CLI flag to filter or toggle between viewing summary markdown pages versus their bibliography sidecars.

This specification defines a dual-mode presentation architecture for knowledge base search:
1. **Operator CLI Search Filters:** Adds explicit filtering flags (`--summaries` / `--bibs`) to `delphi kb search`, defaulting CLI terminal runs to returning the actual summary `.md` pages and markdown previews.
2. **TUI Default Presentation & Configuration:** Adds a configurable setting (`hide_bibliographies = true`, enabled by default) in `config.toml` and wires the daemon `kb.search` RPC so the console search panel displays summary `.md` pages and rich previews.
3. **Autonomous Agent Guard:** Preserves the `.bib.md` sidecar substitution strictly for autonomous agent tool invocations (`kb_search` in `tools.rs`), ensuring agent spans are not involuntarily downgraded to `L0` taint.

---

## 2. Background & Taint Invariants

### 2.1 The Agent Taint Boundary (Spec 068 §6.3, GOVERNANCE §4.3)
Imported summary pages carry untrusted external prose (`Tier::L0 external_untrusted`). When an AI agent reads the raw markdown body of an imported summary, Delphi's governance automatically drops its execution span to `Tier::L0`, stripping outbound and mutating tools to prevent prompt injection and unvalidated influence.

The `.bib.md` sidecar exists specifically to allow **agents** to discover citations and primary sources without taking on span taint.

### 2.2 Human Operator Exception
A human operator using the terminal CLI or navigating the TUI console is not an AI model:
- The human operator cannot be "prompt-injected" into executing unauthorized host tools.
- The human operator does not possess an execution span whose ceiling can be degraded.
- The human operator specifically imported the summaries to read and reference their substantive findings.

Therefore, suppressing `.bib.md` files and displaying full `.md` summary pages to the human operator violates no security invariant and significantly enhances knowledge utility.

---

## 3. Design Specification

### 3.1 Search Options Extension (`SearchOpts`)
In `crates/delphi-engine/src/kb.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SummaryTarget {
    #[default]
    Bibliography, // Answer with .bib.md (safest for agent tools)
    SummaryPage,  // Answer with .md summary page body and prose
    Both,         // Allow hits on either without substitution
}
```

`SearchOpts` gains a field:
```rust
pub summary_target: SummaryTarget,
```

When `summary_target` is `SummaryTarget::SummaryPage`:
- Hits matching `summaries/*.md` are retained as `summaries/<id>.md`.
- Preview extraction pulls lines from the summary markdown prose (skipping YAML frontmatter).
- Pointers format to `summaries/<id>.md#L<start>-L<end>`.
- Hits matching direct `.bib.md` files are suppressed or filtered out.

When `summary_target` is `SummaryTarget::Bibliography`:
- Hits on `summaries/*.md` are substituted with `summaries/<id>.bib.md` (preserving Spec 068 §6.7).

### 3.2 CLI Flags (`delphi kb search`)
The CLI command `delphi kb search <query>` supports:
- `--summaries` (synonym: `--pages`): Force returning summary `.md` files and previews.
- `--bibs` (synonym: `--bibliographies`): Force returning `.bib.md` metadata sidecars.
- **Default for CLI:** When neither flag is passed in a terminal CLI invocation, `delphi kb search` defaults to `SummaryTarget::SummaryPage`.

### 3.3 TUI & Daemon Configuration
Under `[tui]` in `config.toml`:
```toml
[tui]
# When searching the knowledge base in the console, display full summary markdown pages
# instead of bibliography sidecars (default: true).
hide_bibliographies = true
```

The daemon RPC `kb.search` in `crates/delphi-engine/src/service.rs` accepts an optional parameter:
```json
{
  "query": "...",
  "target": "summaries" | "bibliographies" | "default"
}
```
The Ink console (`ui/src/app.tsx`) requests `target: "summaries"` when `hide_bibliographies` is active, ensuring the TUI preview deck renders the summary's formatted markdown lines.

### 3.4 Autonomous Agent Preservation
The `kb_search` tool implementation in `crates/delphi-engine/src/tools.rs` continues to execute with `summary_target: SummaryTarget::Bibliography`. An agent calling `kb_search` receives only schema-checked bibliography records and validated fact elements unless explicitly authorized by a separate operator directive.

---

## 4. Simpler Alternative Considered

### 4.1 Banning `.bib.md` Sidecars Entirely
A simpler approach would be removing the `.bib.md` sidecar generator from `kb_import.rs` and indexing only summary `.md` files.

**Why rejected:** This would break Spec 068 §6.7, Operator Decision D-7, and taint isolation. Autonomous agents using `kb_search` would ingest unvalidated markdown prose, immediately demoting their span ceiling to `Tier::L0` and losing tool privileges. The sidecar must be preserved for agent safety; only presentation to the operator should change.

### 4.2 Requiring CLI Users to Run Separate Subcommands
Introducing `delphi kb pages <query>` separate from `delphi kb search <query>`.

**Why rejected:** Unnecessary command fragmentation. `delphi kb search` is the canonical entry point for all knowledge discovery. Using CLI flags and configuration maintains a single cohesive mental model.

---

## 5. Verification Plan

The implementation must pass all standard Delphi gates:
```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo run -q --locked -p delphi -- lint --repo .
```

Specific test suites:
1. **Engine Search Unit Tests (`crates/delphi-engine/src/kb.rs`):**
   - Verify searching with `SummaryTarget::SummaryPage` returns `summaries/<id>.md` with prose preview and no `.bib.md` pointer.
   - Verify searching with `SummaryTarget::Bibliography` returns `summaries/<id>.bib.md` with metadata preview.
2. **CLI Integration Tests (`crates/delphi/src/main.rs`):**
   - Test `delphi kb search <query> --summaries` and `--bibs` output formatting and synopsis validation.
3. **Agent Seat Safety Test (`crates/delphi-engine/src/tools.rs`):**
   - Verify that calling `kb_search` from an agent span returns `.bib.md` and does not taint the span or leak summary body text.
4. **TUI Search Panel Verification (`ui/test/`):**
   - Golden screen snapshot test verifying the search preview pane renders summary markdown content when `hide_bibliographies = true`.

---

## 6. Implementation Slices & Time Estimates

In accordance with Delphi's **One Slice Per Pull Request** rule:

| Slice | Focus | Deliverables | Est. Hours |
| :--- | :--- | :--- | --: |
| **S0** | **Spec Approval & Spec Tracker** | Ratify `specs/075-kb-search-operator-presentation-and-summary-filtering.md` and update `missions/2026-10-23-foundational-design/spec-tracker.md`. | 1 h |
| **S1** | **Engine Search Options & Presentation** | Add `SummaryTarget` to `crates/delphi-engine/src/kb.rs`. Support returning `.md` summary pointers, lines, and prose previews while filtering `.bib.md`. Add unit tests in `kb.rs`. | 4 h |
| **S2** | **CLI Flags & Synopsis** | Add `--summaries` and `--bibs` to `delphi kb search`. Update command synopsis in `main.rs`. Regenerate CLI reference documentation (`docs/reference/cli.md` and `docs/reference/delphi.1`). | 3 h |
| **S3** | **TUI Config & Console Integration** | Add `[tui] hide_bibliographies` to `config.example.toml`, `config.rs`, `config.schema.json`, and `config.md`. Wire daemon RPC `kb.search` in `service.rs` and update `ui/src/app.tsx` search panel. Add TUI test. | 4 h |

**Total Estimated Effort:** ~12 engineering hours across 4 PR slices.
