---
id: spec-004-athena-memory-and-coaching
title: "004: Dual-Tier Memory, Zero-Server Hybrid RAG & Athena's Coaching Loop"
author: daedalus
timestamp: 2026-10-23T07:53:00Z
status: draft
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
---

# Spec 004: Dual-Tier Memory, Zero-Server Hybrid RAG & Athena's Coaching Loop

## 1. Scope & Objectives
This specification defines Delphi's memory, local retrieval (Hybrid RAG), and self-improvement engine:
1. **Dual-Tier Memory Architecture:** Isolated **Agent-Private Episodic Memory** (`memory/agents/<god>/lessons.md`) vs. the **Central Knowledge Base** (`context/` & `missions/`).
2. **Frozen Boot-Snapshot Cache Hygiene:** Preventing mid-session prompt-cache invalidation when memory files are updated.
3. **The Two-Gear Retrieval Protocol & Zero-Server Hybrid RAG (`.delphi/state.db`):** Combining **Agentic Progressive Disclosure (`context/index.md` + `ripgrep`/`ast-grep`)** for live code and curated docs with **Deterministic Breadcrumb AST Chunking**, **SQLite FTS5 (BM25 with identifier tokenization)**, **`sqlite-vec` (local SIMD embeddings)**, and **Reciprocal Rank Fusion (RRF)** for deep archival recall.
4. **Athena's Coaching, Two-Strike Circuit Breaker & 4-Channel `REVIEW.md` Loop:** Governed by the **ExpeL Rule of 25**, a **6 KB hard byte cap**, and the **5-Tier Security Taint Lattice (`[H-004]`)**.

### 1.1 Simpler Alternative Considered (*The Icarus Middle-Course Rule*)
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
        PrivLess["memory/agents/<god>/lessons.md\n(<= 25 rules, <= 6 KB)"]
    end

    subgraph OnDemand ["2. Two-Gear Retrieval Store"]
        Gear1["Gear 1 (Hot): context/index.md +\nripgrep / ast-grep (Live Source Code)"]
        Gear2["Gear 2 (Archival): .delphi/state.db\n(FTS5 BM25 + sqlite-vec + RRF over Markdown)"]
    end

    subgraph WritePath ["3. Athena Post-Mission Coaching & Distillation"]
        Coach["Athena Retrospective & REVIEW.md Distiller"]
        TaintGate{"5-Tier IFC Taint Lattice Gate\n(Blocks external_untrusted & federated_peer)"}
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
- When a task starts, `delphi-core` loads `context/heuristics.md` and `memory/agents/<god>/lessons.md` into an **immutable in-memory prefix snapshot** pinned with Anthropic `cache_control: {"type": "ephemeral"}` or Gemini `cachedContents`.
- If **Athena** or a user command writes a new lesson to disk mid-session, the disk file is updated atomically (`[REV-TRAP-02]`), **but the active agent's in-memory system prompt prefix is NOT mutated mid-turn**. This prevents invalidating the provider's KV cache mid-loop (saving 5x–10x in latency and token costs); the new snapshot takes effect on the next spawned task.

### 2.2 Hard Dual-Cap Invariant: The Rule of 25 + 6 KB Byte Ceiling
To prevent long-rule prose inflation:
- `context/heuristics.md` and every `memory/agents/<god>/lessons.md` are strictly capped at:
  1. **Max 25 active rules**, AND
  2. **Max 6,144 bytes (6 KB / ~1,500 tokens)** total file size, with each individual rule $\le 350$ characters.

---

## 3. The Two-Gear Retrieval Protocol & Zero-Server Hybrid RAG

### 3.1 The Golden Routing Rule (Gear 1 vs. Gear 2)
1. **Gear 1 — Curated Map & Exact AST/Grep (For Active Code & Core Specs):**
   - Live source code (`src/**`, `.delphi/worktrees/**`) is **never** indexed into `sqlite-vec` (avoiding index staleness during TDD).
   - Agents always start by reading `context/index.md` and using `ripgrep` / `ast-grep` for live code navigation.
2. **Gear 2 — Embedded SQLite Hybrid RAG (For Archival Missions, Domain Docs & Rule Deduplication):**
   - `.delphi/state.db` indexes **only Markdown artifacts** (`context/**`, `specs/**`, `missions/**`, `memory/**`) using content-addressed `blake3` incremental hashes.

### 3.2 Deterministic AST Breadcrumb Chunking
Instead of arbitrary token windows or expensive LLM-generated chunk summaries, Delphi parses the Markdown AST (`pulldown-cmark` / `tree-sitter-markdown`) along `# H1` / `## H2` / `### H3` boundaries (keeping fenced code blocks, tables, and Mermaid diagrams intact), records exact **1-indexed `start_line` and `end_line`**, and prepends a deterministic structural breadcrumb to every chunk:
```text
[File: context/heuristics.md | Lines: 29-33 | Section: Systemic Heuristics > [H-003] Preserve Cryptographic Thinking Signatures | Source: internal_empirical | Status: active]
```

### 3.3 Concrete SQLite Hybrid Schema (`.delphi/state.db`)
```sql
-- 1. Content-Addressed Document Registry with 5-Tier Taint Lattice
CREATE TABLE IF NOT EXISTS kb_documents (
    doc_id          INTEGER PRIMARY KEY,
    rel_path        TEXT NOT NULL UNIQUE,
    zone            TEXT NOT NULL, -- 'core' | 'context' | 'missions' | 'memory' | 'specs'
    content_blake3  TEXT NOT NULL,
    author          TEXT,
    source_type     TEXT NOT NULL DEFAULT 'external_untrusted'
                    CHECK (source_type IN ('user_ratified', 'internal_empirical', 'federated_verified', 'federated_peer', 'external_untrusted')),
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
    heading_path    TEXT NOT NULL,
    identifiers     TEXT NOT NULL DEFAULT '',
    raw_content     TEXT NOT NULL,
    contextual_text TEXT NOT NULL
);

-- 3. FTS5 Lexical Index preserving technical characters '_-.:/@#' intact
CREATE VIRTUAL TABLE IF NOT EXISTS kb_fts USING fts5(
    rel_path, heading_path, identifiers, raw_content,
    content='kb_chunks', content_rowid='chunk_id',
    tokenize="unicode61 tokenchars '_-.:/@#'"
);

-- 4. sqlite-vec SIMD Vector Index (384-dim local embedding)
CREATE VIRTUAL TABLE IF NOT EXISTS kb_vec USING vec0(
    chunk_id INTEGER PRIMARY KEY,
    zone TEXT PARTITION KEY,
    embedding float[384] distance_metric=cosine
);
```

### 3.4 Reciprocal Rank Fusion (RRF) & Pointer Output Contract
Queries pre-filter out `status != 'active'` and untrusted taint tiers (`external_untrusted`, `federated_peer`) unless explicitly requested, then combine **Column-Weighted BM25 (`bm25(kb_fts, 2.0, 5.0, 10.0, 1.0)`)** and **`sqlite-vec` cosine distance** via RRF ($k=60$):
$$\text{Score}_{\text{RRF}}(d) = \frac{w_{\text{lex}}}{60 + r_{\text{bm25}}(d)} + \frac{w_{\text{vec}}}{60 + r_{\text{vec}}(d)}$$
Results return **at most 10 `file:///path#Lstart-Lend` pointers + 3-line previews** (`<150` lines total, enforcing `[H-002]`).

---

## 4. Athena's Coaching, Two-Strike Circuit Breaker & 4-Channel Learning Loop

### 4.1 The Two-Strike Context Circuit Breaker (Mid-Mission Coaching)
When **Hephaestus** fails the same test or build verification twice in a row inside a worktree session, the failed reasoning trace itself poisons subsequent turns:
- **Invariant:** On Strike 2, **Athena** trips the **Context Circuit Breaker**:
  1. Distills a 5-line diagnostic summary of the failure.
  2. Clears **Hephaestus's** polluted conversation history.
  3. Respawns a **fresh Hephaestus context** with an **upscaled model/thinking budget (`[H-007]`)** and the failure constraint injected into the `HandoffEnvelope`.

### 4.2 Post-Mission ExpeL Contrastive Distillation
At mission close, **Athena** executes `review-md-distillation` and `lesson-promotion`:
- Candidate lessons are checked against the **5-Tier IFC Taint Lattice (`[H-004]`)**: only `user_ratified` ($L_4$) and `internal_empirical` ($L_3$) lessons may enter `context/heuristics.md`, `REVIEW.md`, `lessons.md`, or `skills/`.
- Using `sqlite-vec` cosine similarity ($>0.82$) + BM25 overlap (`lesson-deduper`), **Athena** applies ExpeL operations (`ADD`, `MERGE`, `DEMOTE`, or `PROMOTE` 2-strike rules to deterministic scripts in `skills/*/scripts/`).

---

## 5. Verification Plan
1. **Identifier Tokenization Test:** Verify FTS5 `unicode61 tokenchars '_-.:/@#'` indexes `[H-004]`, `REV-SEC-04`, and `Part.thoughtSignature` as single exact tokens and ranks them #1 on exact queries.
2. **Heading-Aware Chunking Test:** Index `SPEC.md` and verify every chunk in `.delphi/state.db` preserves exact 1-indexed `start_line`/`end_line` ranges and YAML frontmatter attributes.
3. **Two-Strike Circuit Breaker Test:** Simulate 2 consecutive test failures in a `Hephaestus` task; verify `delphi-core` resets the turn history and dispatches Attempt #2 with an upscaled `routing_directive`.
