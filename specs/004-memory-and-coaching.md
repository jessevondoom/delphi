---
id: spec-004-memory-and-coaching
title: "004: Dual-Tier Memory, Zero-Server Hybrid RAG & Coaching Loop"
author: architect
timestamp: 2026-10-23T07:53:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec]
---

# Spec 004: Dual-Tier Memory, Zero-Server Hybrid RAG & Coaching Loop

## 1. Scope & Objectives
This specification defines Delphi's memory, local retrieval (Hybrid RAG), and self-improvement engine:
1. **Dual-Tier Memory Architecture:** Isolated **Agent-Private Episodic Memory** (`memory/agents/<role>/lessons.md`) vs. the **Central Knowledge Base** (`context/` & `missions/`).
2. **Frozen Boot-Snapshot Cache Hygiene:** Preventing mid-session prompt-cache invalidation when memory files are updated.
3. **The Two-Gear Retrieval Protocol & Zero-Server Hybrid RAG (`.delphi/state.db`):** Combining **Agentic Progressive Disclosure (`context/index.md` + `ripgrep`/`ast-grep`)** for live code and curated docs with **Deterministic Breadcrumb AST Chunking**, **SQLite FTS5 (BM25 with identifier tokenization)**, **`sqlite-vec` (local SIMD embeddings)**, and **Reciprocal Rank Fusion (RRF)** for deep archival recall.
4. **Orchestrator's Coaching, Two-Strike Circuit Breaker & 4-Channel `REVIEW.md` Loop:** Governed by the **ExpeL Rule of 25**, a **6 KB hard byte cap**, and the **6-Tier Security Taint Lattice (`[H-004]`)**.

### 1.1 Simpler Alternative Considered
- **Alternative 1 (Pure Vector DB / External Server):** Index all files into Qdrant/Chroma using 500-token sliding windows.
  - *Why Rejected:* Pure dense embeddings compress exact technical identifiers (`[H-004]`, `REV-SEC-04`, `Part.thoughtSignature`) into generic semantic neighborhoods, sever Markdown section headers from child paragraphs, ignore YAML provenance taint tags, and require an external server process.
- **Alternative 2 (Grep-Only):** Rely solely on `ripgrep` without BM25 or semantic indexing.
  - *Why Rejected:* While `ripgrep` + `context/index.md` is superior for live source code (`src/`), searching across 500+ historical missions (`missions/YYYY-MM-DD-*/`) for conceptually similar bugs or architectural debates fails when terminology shifts (*"subprocess hang"* vs. *"pipe buffer deadlock"*). Embedding **SQLite FTS5 + `sqlite-vec` + RRF** inside `.delphi/state.db` gives zero-server, single-file ACID hybrid retrieval in `<8ms`.

---

## 2. Dual-Tier Memory & Frozen Prefix Cache Hygiene

```mermaid
flowchart TD
    subgraph Boot ["1. Frozen Session Prefix Snapshot (KV Cache Pinned)"]
        GovSnap["GOVERNANCE.md + Agent Manifest"]
        SysHeur["context/heuristics.md\n(<= 25 rules, <= 6 KB)"]
        PrivLess["memory/agents/<role>/lessons.md\n(<= 25 rules, <= 6 KB)"]
    end

    subgraph OnDemand ["2. Two-Gear Retrieval Store"]
        Gear1["Gear 1 (Hot): context/index.md +\nripgrep / ast-grep (Live Source Code)"]
        Gear2["Gear 2 (Archival): .delphi/state.db\n(FTS5 BM25 + sqlite-vec + RRF over Markdown)"]
    end

    subgraph WritePath ["3. Orchestrator Post-Mission Coaching & Distillation"]
        Coach["Orchestrator Retrospective & REVIEW.md Distiller"]
        TaintGate{"6-Tier IFC Taint Lattice Gate\n(Blocks external_untrusted & federated_peer)"}
        ExpeL{"ExpeL Rule-of-25 & 6KB Gate\n(ADD / MERGE / DEMOTE / PROMOTE)"}
    end

    Boot -->|Pinned in Provider Prompt Cache| ActiveTurn["Active Agent Execution"]
    OnDemand -->|Paged via Pointer Slices (<300 lines)| ActiveTurn
    ActiveTurn --> Coach
    Coach --> TaintGate --> ExpeL
    ExpeL -->|Next Session Snapshot| Boot
    ExpeL -->|2-Strike Rules -> Scripts| Skills["skills/*/scripts/"]
```

### 2.1 The Frozen Boot-Snapshot Invariant (Prompt Cache Hygiene)
Inspired by Nous Research's `hermes-agent` snapshot pattern:
- When a task starts, `delphi-core` loads `context/heuristics.md` and `memory/agents/<role>/lessons.md` into an **immutable in-memory prefix snapshot** pinned with Anthropic `cache_control: {"type": "ephemeral"}` or Gemini `cachedContents`.
- If **Orchestrator** or a user command writes a new lesson to disk mid-session, the disk file is updated atomically (`[REV-TRAP-02]`), **but the active agent's in-memory system prompt prefix is NOT mutated mid-turn**. This prevents invalidating the provider's KV cache mid-loop (saving 5x–10x in latency and token costs); the new snapshot takes effect on the next spawned task.
- **No lost updates:** every change to `heuristics.md`, `lessons.md` or `REVIEW.md` (from the daemon, `delphi sync`, `delphi update`, or the user's `$EDITOR` via `delphi`) carries the `expected_blake3` of the version it was **derived from**, captured when the proposal or edit was first prepared. The writer takes an advisory `flock` on `.delphi/locks/<path>.lock`, checks `blake3(disk) == expected_blake3`, writes a temp file, and renames it. On a mismatch it rebases (re-derives the change against the current file) or reports a conflict; it never overwrites blindly.

### 2.2 Hard Dual-Cap Invariant: The Rule of 25 + 6 KB Byte Ceiling
To prevent long-rule prose inflation:
- `context/heuristics.md` and every `memory/agents/<role>/lessons.md` are strictly capped at:
  1. **Max 25 active rules**, AND
  2. **Max 6,144 bytes (6 KB / ~1,500 tokens)** total file size, with each individual rule $\le 350$ characters.
- **Caps kept (D-09, ratified 2026-10-23):** rule bodies hold only Trigger / Directive / Falsification lines. Rationale and citations live in `context/domains/`. The 7 seed rules were rewritten to this format (2.5 KB total, ≤241 characters each).

---

## 3. The Two-Gear Retrieval Protocol & Zero-Server Hybrid RAG

### 3.1 The Golden Routing Rule (Gear 1 vs. Gear 2)
1. **Gear 1 — Curated Map & Exact AST/Grep (For Active Code & Core Specs):**
   - Live source code (`src/**`, `.delphi/worktrees/**`) is **never** indexed into `sqlite-vec` (avoiding index staleness during TDD).
   - Agents always start by reading `context/index.md` and using `ripgrep` / `ast-grep` for live code navigation.
2. **Gear 2 — Embedded SQLite Hybrid RAG (For Archival Missions, Domain Docs & Rule Deduplication):**
   - `.delphi/state.db` indexes **only Markdown artifacts** (`context/**`, `specs/**`, `missions/**`, `memory/**`) using content-addressed `blake3` incremental hashes.
   - **Staging (D-01, ratified 2026-10-23):** v1 ships **lexical-only** (FTS5 BM25). The vector leg (`kb_vec`, RRF) is added later as a schema migration behind an `Embedder` trait. `kb_meta` records `schema_version` and `embedding_model_id`; changing the model forces a full re-embed.
   - `state.db` is a **derived index**: deleting it and running `delphi kb reindex` must rebuild it completely from Markdown and `events.jsonl`.

### 3.2 Deterministic AST Breadcrumb Chunking
Instead of arbitrary token windows or expensive LLM-generated chunk summaries, Delphi parses the Markdown AST (`pulldown-cmark` / `tree-sitter-markdown`) along `# H1` / `## H2` / `### H3` boundaries (keeping fenced code blocks, tables, and Mermaid diagrams intact), records exact **1-indexed `start_line` and `end_line`**, and prepends a deterministic structural breadcrumb to every chunk:
```text
[File: context/heuristics.md | Lines: 29-33 | Section: Systemic Heuristics > [H-003] Preserve Cryptographic Thinking Signatures | Source: internal_empirical | Status: active]
```

### 3.3 Concrete SQLite Hybrid Schema (`.delphi/state.db`)
```sql
-- 1. Content-Addressed Document Registry with 6-Tier Taint Lattice
CREATE TABLE IF NOT EXISTS kb_documents (
    doc_id          INTEGER PRIMARY KEY,
    rel_path        TEXT NOT NULL UNIQUE,
    zone            TEXT NOT NULL, -- 'core' | 'context' | 'missions' | 'memory' | 'specs'
    content_blake3  TEXT NOT NULL,
    author          TEXT,
    source_type     TEXT NOT NULL DEFAULT 'external_untrusted'
                    CHECK (source_type IN ('user_ratified', 'internal_empirical', 'internal_generated', 'federated_verified', 'federated_peer', 'external_untrusted')),
    confidence      TEXT NOT NULL DEFAULT 'low',
    status          TEXT NOT NULL DEFAULT 'active',
    updated_at      TEXT NOT NULL
);

-- 2. Heading-Aware Chunks with Exact Line Pointers
CREATE TABLE IF NOT EXISTS kb_chunks (
    chunk_id        INTEGER PRIMARY KEY,
    doc_id          INTEGER NOT NULL REFERENCES kb_documents(doc_id) ON DELETE CASCADE,
    chunk_index     INTEGER NOT NULL,
    start_line      INTEGER NOT NULL,
    end_line        INTEGER NOT NULL,
    rel_path        TEXT NOT NULL,   -- denormalized from kb_documents: external-content FTS reads columns from kb_chunks only
    heading_path    TEXT NOT NULL,
    identifiers     TEXT NOT NULL DEFAULT '',
    raw_content     TEXT NOT NULL,
    contextual_text TEXT NOT NULL
);

-- 3a. FTS5 Identifier Index: technical tokens kept whole ('[H-004]' -> 'H-004', 'Part.thoughtSignature')
CREATE VIRTUAL TABLE IF NOT EXISTS kb_fts_ident USING fts5(
    rel_path, heading_path, identifiers,
    content='kb_chunks', content_rowid='chunk_id',
    tokenize="unicode61 tokenchars '_-.:/@#'"
);

-- 3b. FTS5 Prose Index: standard tokenization so 'done.' and 'spec-001' still match 'done' / 'spec'
CREATE VIRTUAL TABLE IF NOT EXISTS kb_fts_prose USING fts5(
    raw_content,
    content='kb_chunks', content_rowid='chunk_id',
    tokenize="porter unicode61"
);

-- External-content FTS tables are NOT kept in sync automatically: these triggers are required.
CREATE TRIGGER IF NOT EXISTS kb_chunks_ai AFTER INSERT ON kb_chunks BEGIN
  INSERT INTO kb_fts_ident(rowid, rel_path, heading_path, identifiers) VALUES (new.chunk_id, new.rel_path, new.heading_path, new.identifiers);
  INSERT INTO kb_fts_prose(rowid, raw_content) VALUES (new.chunk_id, new.raw_content);
END;
CREATE TRIGGER IF NOT EXISTS kb_chunks_ad AFTER DELETE ON kb_chunks BEGIN
  INSERT INTO kb_fts_ident(kb_fts_ident, rowid, rel_path, heading_path, identifiers) VALUES ('delete', old.chunk_id, old.rel_path, old.heading_path, old.identifiers);
  INSERT INTO kb_fts_prose(kb_fts_prose, rowid, raw_content) VALUES ('delete', old.chunk_id, old.raw_content);
END;
-- Updates are implemented as delete + insert of the chunk row.
-- Triggers read only old.*/new.* (never kb_documents), so they stay correct when a
-- document delete cascades to its chunks.

-- 4. sqlite-vec Vector Index (deferred: decision D-01). Filter fields are metadata
--    columns so KNN pre-filters by status/taint instead of post-filtering a top-k.
CREATE VIRTUAL TABLE IF NOT EXISTS kb_vec USING vec0(
    chunk_id INTEGER PRIMARY KEY,
    zone TEXT PARTITION KEY,
    source_type TEXT,
    status TEXT,
    embedding float[384] distance_metric=cosine
);

CREATE TABLE IF NOT EXISTS kb_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- rows: schema_version, embedding_model_id
```

### 3.4 Reciprocal Rank Fusion (RRF) & Pointer Output Contract
Queries pre-filter out `status != 'active'` and untrusted taint tiers (`external_untrusted`, `federated_peer`) unless explicitly requested. In v1, results come from BM25 alone: `kb_fts_ident` (weights `2.0, 5.0, 10.0`) fused by RRF with `kb_fts_prose`. Once D-01 enables vectors, **`sqlite-vec` cosine distance** joins the same RRF ($k=60$):
$$\text{Score}_{\text{RRF}}(d) = \frac{w_{\text{lex}}}{60 + r_{\text{bm25}}(d)} + \frac{w_{\text{vec}}}{60 + r_{\text{vec}}(d)}$$
Results return **at most 10 `path#Lstart-Lend` pointers + 3-line previews** (`<150` lines total, enforcing `[H-002]`). Each pointer carries the chunk's `heading_path` and `blake3`. When a file has been edited since indexing, the resolver re-anchors by heading. If the heading is gone, it reports the pointer as stale rather than returning the wrong lines.

---

## 4. Orchestrator's Coaching, Two-Strike Circuit Breaker & 4-Channel Learning Loop

### 4.1 The Two-Strike Context Circuit Breaker (Mid-Mission Coaching)
When **Builder** fails the same test or build verification twice in a row inside a worktree session, the failed reasoning trace itself poisons subsequent turns. This breaker is step 3 of the unified retry ladder ([`specs/003`](003-collaboration-and-council.md) §3.2); it is not a separate policy. If the reset attempt also fails, the node escalates to the **operator**.
- **Invariant:** On Strike 2, `delphi-core` (not Orchestrator's judgment) trips the **Context Circuit Breaker**:
  1. Distills a 5-line diagnostic summary of the failure.
  2. Clears **Builder's** polluted conversation history.
  3. Respawns a **fresh Builder context** with an **upscaled model/thinking budget (`[H-007]`)** and the failure constraint injected into the `HandoffEnvelope`.

### 4.2 Post-Mission ExpeL Contrastive Distillation
At mission close, **Orchestrator** executes `review-md-distillation` and `lesson-promotion`:
- Candidate lessons are checked against the **6-Tier IFC Taint Lattice (`[H-004]`)**: only `user_ratified` ($L_5$) and `internal_empirical` ($L_4$) lessons may enter `context/heuristics.md`, `REVIEW.md`, `lessons.md`, or `skills/`.
- Using rule-ID match + BM25 overlap (plus `sqlite-vec` cosine similarity $>0.82$ once vectors are enabled (D-01 reopen trigger: >50 missions **and** a logged terminology-shift miss)) via `lesson-deduper`, **Orchestrator** proposes ExpeL operations (`ADD`, `MERGE`, `DEMOTE`, or `PROMOTE` 2-strike rules to deterministic scripts in `skills/*/scripts/`).
- Proposals are written to `missions/<id>/proposals/` and applied by `delphi-core` only after the **operator** ratifies them at `/retro` (GOVERNANCE §5.7, §7). This includes `lessons.md` (D-11, revised 2026-10-23): nothing enters an always-loaded prompt without ratification.

---

## 5. Verification Plan
1. **Identifier Tokenization Test:** Verify FTS5 `unicode61 tokenchars '_-.:/@#'` indexes `[H-004]`, `REV-SEC-04`, and `Part.thoughtSignature` as single exact tokens and ranks them #1 on exact queries.
2. **Heading-Aware Chunking Test:** Index `SPEC.md` and verify every chunk in `.delphi/state.db` preserves exact 1-indexed `start_line`/`end_line` ranges and YAML frontmatter attributes.
3. **Two-Strike Circuit Breaker Test:** Simulate 2 consecutive test failures in a `Builder` task; verify `delphi-core` resets the turn history and dispatches Attempt #3 (per the unified ladder) with a fresh context on the other provider family.
4. **FTS Sync Test:** Insert, update, and delete chunks, then verify both FTS tables return exactly the live rows (`INSERT INTO kb_fts_prose(kb_fts_prose) VALUES('integrity-check')` passes).
5. **Lost-Update Test:** `delphi sync` and a daemon retro write `heuristics.md` concurrently; both changes survive or one writer reports a CAS conflict. Neither is silently lost.
6. **Rebuild Test:** Delete `state.db`; `delphi kb reindex` reproduces identical search results.
