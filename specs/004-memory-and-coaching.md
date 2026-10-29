---
id: spec-004-memory-and-coaching
title: "004: Dual-Tier Memory, Zero-Server Hybrid RAG & Coaching Loop"
author: architect
timestamp: 2026-10-23T07:53:00Z
status: approved   # by the operator, 2026-10-24 ("ratify all"); whole document. Amendments 004-A1 (§3.3-§3.5, §4.2, §6 tests 16-20), 004-A2 (§3.1, §3.6, §6 tests 21-24) and 004-A3 (§3.3, §3.4, §3.7, §6 tests 25-31) approved by the operator, 2026-10-25
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium
source_mission: 2026-10-23-foundational-design
tags: [spec]
---

# Spec 004: Dual-Tier Memory, Zero-Server Hybrid RAG & Coaching Loop

> **Outside amendments:** §3.3, §3.6 zone `bundles`, bundle `vectors/` cache, `[privacy] identifiers` in `bundle validate` (ratified 2026-10-25). Live in `specs/022` §5, `specs/024` §3.5, `specs/027` §5.

## 1. Scope & Objectives
This specification defines Delphi's memory, local retrieval (Hybrid RAG), and self-improvement engine:
1. **Dual-Tier Memory Architecture:** Isolated **Agent-Private Episodic Memory** (`memory/agents/<role>/lessons.md`) vs. the **Central Knowledge Base** (`context/` & `missions/`).
2. **Frozen Boot-Snapshot Cache Hygiene:** Preventing mid-session prompt-cache invalidation when memory files are updated.
3. **The Two-Gear Retrieval Protocol & Zero-Server Hybrid RAG (`.delphi/state.db`):** Combining **Agentic Progressive Disclosure (`context/index.md` + `ripgrep`/`ast-grep`)** for live code and curated docs with **Deterministic Breadcrumb AST Chunking**, **SQLite FTS5 (BM25 with identifier tokenization)**, **`sqlite-vec` (local embeddings)**, **Reciprocal Rank Fusion (RRF)** and a local **cross-encoder reranker** for deep archival recall, with vectors for the shipped documents made at release time (Amendments 004-A1 to 004-A3).
4. **Orchestrator's Coaching, Two-Strike Circuit Breaker & 4-Channel `REVIEW.md` Loop:** Governed by the **ExpeL Rule of 25**, a **6 KB hard byte cap**, and the **6-Tier Security Taint Lattice (`[H-004]`)**.

### 1.1 Simpler Alternative Considered
- **Alternative 1 (Pure Vector DB / External Server):** Index all files into Qdrant/Chroma using 500-token sliding windows.
  - *Why Rejected:* Pure dense embeddings compress exact technical identifiers (`[H-004]`, `REV-SEC-04`, `Part.thoughtSignature`) into generic semantic neighborhoods, sever Markdown section headers from child paragraphs, ignore YAML provenance taint tags, and require an external server process.
- **Alternative 2 (Grep-Only):** Rely solely on `ripgrep` without BM25 or semantic indexing.
  - *Why Rejected:* While `ripgrep` + `context/index.md` is superior for live source code (`src/`), searching across 500+ historical missions (`missions/YYYY-MM-DD-*/`) for conceptually similar bugs or architectural debates fails when terminology shifts (*"subprocess hang"* vs. *"pipe buffer deadlock"*). Embedding **SQLite FTS5 + `sqlite-vec` + RRF** inside `.delphi/state.db` gives zero-server, single-file ACID hybrid retrieval in about 20 ms per query (measured on an M1 laptop, with the query embedded on its GPU), or about half a second when the reranking pass of §3.7 reads the best results as well; both are local, and the quick path is always available (`--no-rerank`).

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

### 2.0 Phase Mapping (§§2–4)
| Roadmap step | Delivers |
| :--- | :--- |
| **2** | §2 ratified four-scope snapshot with per-scope budgets (§5.2), oldest-first trimming, scoped CAS locks; §3.4 role-filtered retrieval rule |
| **3** | §4.1 context circuit breaker (it needs Builder and the retry ladder) |
| **5** | §3 FTS5 knowledge-base search |
| **6** | §4.2 `/retro` distillation, `rule_stats` |
| **Amendment 004-A1** | §3.5 local embeddings: the vector leg of §3.4 ships in the binary (D-01 reopened by the operator, 2026-10-24) |
| **Amendment 004-A2** | §3.6 preloaded vectors for the shipped documents, and `agents/` and `skills/` in the index (approved by the operator, 2026-10-25) |
| **Amendment 004-A3** | §3.7 a refresh of the index on every search, a cross-encoder reranker with a calibrated score floor, and frontmatter summaries for agents and skills (approved by the operator, 2026-10-25) |

§5.8 covers the §5 rows.

### 2.1 The Frozen Boot-Snapshot Invariant (Prompt Cache Hygiene)
Inspired by Nous Research's `hermes-agent` snapshot pattern:
- **What loads (§5.2 order).** When a task starts, `delphi-core` reads these files **only from their ratified copies** and assembles them into an **immutable in-memory prefix snapshot** (IPR blocks 3–4, `specs/002` §2):

  | Scope | Heuristics | Lessons (this role) | Ratified copy |
  | :--- | :--- | :--- | :--- |
  | Release | `context/heuristics.md` (`[H-…]`) | — | `base/context/heuristics.md`, verified against `base/manifest.lock` |
  | Team | `context/team-heuristics.md` (`[T-…]`) | — | `local/ratified/context/team-heuristics.md` |
  | Operator | `heuristics.md` (`[O-…]`), plus `profile.md` | `memory/agents/<role>/lessons.md` | `$DELPHI_HOME/operator/ratified/…` |
  | Workspace | `heuristics.md` (`[W-…]`) | `memory/agents/<role>/lessons.md` | `local/ratified/heuristics.md`, `local/ratified/memory/agents/<role>/lessons.md` |

  Live files that differ from their ratified copy are pending proposals (`specs/006` §3.1) and are never loaded. Caching is the provider's automatic prefix caching plus the Anthropic breakpoints of `specs/002` §2. Explicit Gemini `cachedContents` is deferred (D-06).
- **Nothing is written mid-session.** Agents and Orchestrator never write heuristics or lessons directly. Every change is a proposal ratified at `/retro` (D-11, §4.2, §5.4). Ratification updates the live file, its ratified copy and `ratified.lock` atomically, and the new snapshot applies from the **next** task. An active task's prefix is never changed mid-turn, which keeps the provider's cache valid.
- **No lost updates (CAS).** Every change to a heuristics, lessons or `REVIEW.md` file (from the daemon applying a ratified proposal, `delphi sync` or `delphi update`) carries the `expected_blake3` of the version it was derived from. The writer:
  - takes an advisory `flock` on the lock file **of the scope that owns the file**: `.delphi/locks/<path>.lock` for workspace and team files, `$DELPHI_HOME/operator/locks/<path>.lock` for operator files;
  - checks `blake3(disk) == expected_blake3`;
  - then, still holding the lock, writes the live file, the ratified copy and the `ratified.lock` entry, each by temp file and rename.

  On a mismatch it may rebase **only** when the change touches rule IDs disjoint from everything that changed meanwhile *and* the result still passes the §2.2 caps and the §5.2 no-relaxation linter. Otherwise it reports a conflict for the operator. It never overwrites blindly, and a rebase never changes ratified text the operator didn't see.

### 2.2 Hard Dual-Cap Invariant: The Rule of 25 + 6 KB Byte Ceiling
To prevent long-rule prose inflation:
- **Every** heuristics file (release, team, operator, workspace) and every lessons file (operator, workspace) is capped at:
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
   - `.delphi/state.db` indexes **only Markdown artifacts** (`context/**`, `specs/**`, `missions/**`, `memory/**`, and from Amendment 004-A2 the shipped and team `agents/**` and `skills/**`) using content-addressed `blake3` incremental hashes. Operator-scope files are **never indexed**; they reach agents only through the snapshot (§5.2).
   - **Staging (D-01, ratified 2026-10-23; reopened by the operator 2026-10-24, Amendment 004-A1):** the first releases shipped **lexical-only** (FTS5 BM25). The vector leg (`kb_vec`, RRF) now ships behind the `Embedder` trait, with the model compiled into the binary (§3.5). `kb_meta` records `schema_version`, `embedding_model_id`, `embedding_dim`, `embedding_format` and `chunker_version`; changing the model or the embedding format forces a full re-embed, and changing the chunker version makes every document be read once more (§3.7). A build or install with no model keeps working lexically.
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
    zone            TEXT NOT NULL, -- 'core' | 'context' | 'missions' | 'memory' | 'specs' | 'agents' | 'skills'
    content_blake3  TEXT NOT NULL,
    author          TEXT,
    source_type     TEXT NOT NULL DEFAULT 'external_untrusted'
                    CHECK (source_type IN ('user_ratified', 'internal_empirical', 'internal_generated', 'federated_verified', 'federated_peer', 'external_untrusted')),
    confidence      TEXT NOT NULL DEFAULT 'low',
    status          TEXT NOT NULL DEFAULT 'active',
    updated_at      TEXT NOT NULL,
    mtime_ns        INTEGER NOT NULL DEFAULT 0,  -- modification time and size when last read (004-A3): a file with the
    size            INTEGER NOT NULL DEFAULT 0   -- same pair is not read again by a sync; 0 means "read it"
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

-- 4. sqlite-vec Vector Index (Amendment 004-A1). Created when an embedding model is attached, sized to that
--    model (`dim` below), and dropped and refilled when the model changes. A chunk is embedded in one or more
--    windows (§3.5); window w of chunk c has rowid c * 16 + w. `trusted` (status active and a trusted source
--    tier) and `scope` ('' shared, '<role>' for memory/agents/<role>/, '~' other memory) are metadata columns
--    so the nearest-neighbour search skips what could never be returned; the authoritative visibility check
--    is the same SQL filter the lexical rankings use (§3.4).
CREATE VIRTUAL TABLE kb_vec USING vec0(
    trusted INTEGER,
    scope TEXT,
    embedding float[dim] distance_metric=cosine
);

-- 5. Which chunks have vectors, and how many windows each has (a chunk without a row is embedded next).
CREATE TABLE IF NOT EXISTS kb_chunk_vec (
    chunk_id        INTEGER PRIMARY KEY REFERENCES kb_chunks(chunk_id) ON DELETE CASCADE,
    windows         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS kb_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- rows: schema_version, embedding_model_id, embedding_dim, embedding_format, chunker_version
```

### 3.4 Reciprocal Rank Fusion (RRF) & Pointer Output Contract
Queries from an agent return `zone = 'memory'` rows **only for the calling role's own `memory/agents/<role>/`**, so no role can search another role's private lessons (GOVERNANCE §4.1). Queries also pre-filter out `status != 'active'` and untrusted taint tiers (`external_untrusted`, `federated_peer`) unless explicitly requested. The lexical rankings are `kb_fts_ident` (weights `2.0, 5.0, 10.0`), `kb_fts_prose`, and a double-weight ranking of chunks that *define* a queried identifier. **`sqlite-vec` cosine distance** joins the same RRF ($k=60$) as one more ranking (Amendment 004-A1):
$$\text{Score}_{\text{RRF}}(d) = \sum_{\text{lexical } i} \frac{w_i}{60 + r_i(d)} + \frac{w_{\text{vec}}}{60 + r_{\text{vec}}(d)}$$
A vector match counts only at or above the model's similarity floor (§3.5), so a question the documents do not answer still returns "no results". With no model attached the vector term is absent and the result is the lexical result.
When a reranking model is installed, the best 10 fused results are then read together with the question and reordered, and if even the best of them scores below the reranker's calibrated floor nothing is returned (§3.7). Results return **at most 10 `path#Lstart-Lend` pointers + 3-line previews** (`<150` lines total, enforcing `[H-002]`). Each pointer carries the chunk's `heading_path` and `blake3`. When a file has been edited since indexing, the resolver re-anchors by heading. If the heading is gone, it reports the pointer as stale rather than returning the wrong lines.

### 3.5 Local Embeddings (Amendment 004-A1)
**Directed by the operator on 2026-10-24**, reopening D-01: use the most accurate embedding model that runs well on a recent Mac laptop, up to 150 MB added to the binary, run with `candle`, vectors in `sqlite-vec`. Approved by the operator on 2026-10-25, with the matching edits to `006` and `013`.

**Model.** `BAAI/bge-base-en-v1.5` (110M parameters, 768 dimensions, MIT), stored as int8 with one scale per row: a 111 MB package, one file, compiled into release binaries. Four candidates were run on this repository's own documents (112 files, 910 chunks, 40 hand-written questions that are worded differently from the answers or are identifiers, plus 8 questions the documents do not answer; `dev/kb-eval.json`, run with `delphi kb eval`):

| Ranking (top 10) | R@1 | R@5 | R@10 | MRR | off-topic answered |
| :--- | :--- | :--- | :--- | :--- | :--- |
| lexical only (FTS5, the previous behaviour) | 0.25 | 0.55 | 0.60 | 0.37 | 5 of 8 |
| vector only (`bge-base`, int8) | 0.42 | 0.78 | 0.80 | 0.57 | 0 of 8 |
| **hybrid (shipped)** | **0.42** | **0.82** | **0.88** | **0.58** | **0 of 8** |

Dense-only recall@10 on the paraphrase subset (30 questions): `bge-small` 0.77, **`bge-base` 0.83**, `snowflake-arctic-embed-m-v1.5` 0.73. `gte-modernbert-base` tied `bge-base` once fused (R@10 0.88, MRR 0.58) but is 153.5 MB as int8, over the limit, so it was not taken. A 40-question set is small: treat differences of one question as noise, and grow the set from confirmed misses (`delphi kb miss`).

**Package.** One file, `DLPHMDL1`: the tokenizer, the model config and the weights (int8 with per-row scales for matrices of 4096 or more values, f32 otherwise), with a header that records the model id, kind, pooling, prefixes, token limit, similarity floor and source repository and revision. `delphi kb model pack <hf dir> <out> --id … --kind … --dim …` makes one; `delphi kb model info` reports the installed one. It is found, in order, at `DELPHI_EMBED_MODEL` (`off` switches vectors off), `$DELPHI_HOME/models/embed.dlphmdl`, then inside the binary (built with `--features embedded-model` and `DELPHI_EMBED_MODEL_FILE`, which the release workflow sets from `dev/fetch-model.sh`, a pinned revision with checked SHA-256 sums). The model id names the weights, the quantization and the prefixes: it changes whenever the vectors would.

**Runtime.** `candle` (pure Rust, no ONNX Runtime). On a Mac the matrix work runs on the GPU through Metal when one is present (`DELPHI_EMBED_DEVICE=cpu` forces the CPU), otherwise on the CPU, with Accelerate on macOS. Always f32: f16 on Metal gave wrong vectors in testing. Candle's Metal backend panics, rather than returning an error, on a macOS that lacks a system class it asks for (found by the release dry run on a macOS 14 runner: `class MTLResidencySetDescriptor could not be found`); the device is therefore created once, under `catch_unwind`, and the CPU is used when that fails, as it is on a machine with no GPU. Weights are widened to f32 when the model loads (about 0.5 GB resident).

**What is embedded.** Each chunk is cut into windows of about 1,600 characters at blank lines outside code fences (at most 16), so a long section is not cut off at the model's 512-token window. A window's text is `<path> > <heading path>` and a newline, then the window. A question is prefixed as the model expects. Similarity is cosine. A vector match counts only at or above the package's floor (0.54 for `bge-base`: off-topic questions scored 0.42 to 0.50, answerable ones 0.55 to 0.82). `delphi kb eval` prints both ranges so the floor can be re-set when the model changes. `kb_meta` also records `embedding_format`; changing the windowing or the text around each window bumps it and every vector is made again.

**Fusion.** The vector ranking joins the lexical ones with weight 3.0. Question words with no topic (`how`, `the`, `is`, …) are left out of the lexical query unless nothing else is left. When no chunk holds every query word and the any-word pass is used, the lexical rankings weigh 0.15, and when a model is installed and no vector clears the floor they are dropped, so a question about something the documents do not cover returns nothing instead of chunks that share a stray word. Exact identifiers (`[REV-SEC-04]`) match every word and keep their full lexical weight.

**Indexing.** Indexing the words is unchanged and quick. Vectors are made separately, a few chunks at a time with no write lock held while the model runs, and a chunk without a vector is simply not in the vector ranking yet. `delphi kb index` embeds with progress; the daemon's `kb.search` and `kb.index` start one background thread (at most one per process) and use the vectors that exist. Only new or changed chunks are embedded after the first time. Measured on an M1 laptop, the first pass over these 910 chunks took 4 to 5 minutes on the CPU with the machine otherwise idle, and was faster on the GPU; timings varied a lot with machine load.

**Safety.** Visibility is decided by the same SQL filter as the lexical rankings (§3.4): the vector table's `trusted` and `scope` columns only skip rows that could never be returned. Nothing leaves the machine: the model runs locally and vectors are derived state in `state.db`. A model that is installed but cannot be loaded is reported once and search carries on lexically; `delphi doctor` check 6 reports a stale or incomplete vector index.

The reranker that was the next lever here is §3.7.

### 3.6 Preloaded Vectors and Agent Skills in the Index (Amendment 004-A2)
**Approved by the operator on 2026-10-25.** Two changes so that retrieval is accurate from the first query, not after the first indexing pass.

**Agents and skills are indexed.** `agents/<role>/AGENT.md` and `skills/**/*.md` (the shipped copy under `base/`, and the team's own under `team/`, the team copy winning for a path) join the index as zones `agents` and `skills`, so an agent can find a skill or a role description by meaning. They are shared files, so they carry no per-role visibility rule. A shipped file has no `source_type` in its frontmatter, so a file that names none is `user_ratified` when it comes from `base/` (the release ratified it, §2.1 of `specs/006`) and `internal_generated` when it comes from `team/`; a file that names its own tier keeps it, and one that says `external_untrusted` stays out of default results like any untrusted document. Every other zone still defaults to `external_untrusted`.

**Preloaded vectors.** The release embeds the documents Delphi ships (a fresh workspace's `base/`, `team/` and memory templates) with the shipped model and compiles the vectors into the executable (`DLPHVEC1`, about 0.6 MB). It is a cache from the hash of *exactly the text given to the model for a chunk* (every window, in order, each length-prefixed) to the vectors the model produced. It is not a copy of `state.db`: nothing in it depends on paths, positions or schema, so a document the operator or team edited has a different text, misses, and is embedded by the model as usual. A cache made for another model id, dimension or `embedding_format` is ignored whole, as is one that is damaged or absent (`DELPHI_SEED_VECTORS=off` switches it off; `DELPHI_SEED_VECTORS=<file>` replaces it).
- `embed_pending` asks the cache before the model. `apply_seed_vectors` does only that, so it is cheap enough to run before a search: the daemon's `kb.search` and `kb.index` and the `delphi kb search` command apply it first, which gives a brand-new workspace semantic search on its first query. Chunks the cache does not cover wait for the background pass (§3.5).
- `delphi kb index` reports how many chunks came from the preloaded vectors and how many from the model. `delphi kb seed-export <out>` writes the cache from a workspace whose chunks all have vectors.
- `dev/build-seed-vectors.sh` makes the cache in a fresh seeded workspace and then checks it: a second fresh workspace is indexed with the cache and the model must read nothing (`0 by the model`), otherwise the script fails and the release stops. The release workflow runs it in the `model` job beside the model fetch. Anything that changes the shipped agents, skills, context or memory templates, the windowing, or the text around a window is covered by that check at release; the cache never needs to be committed.
- Measured on a fresh workspace (34 documents, 170 chunks): 85 s for the model to embed them, 0 s and no model reads with the cache.


### 3.7 A Fresh Index, Reranking and Query-Aware Fusion (Amendment 004-A3)
**Approved by the operator on 2026-10-25.** The operator asked for the data to stay fresh and for retrieval to be as accurate as it can be. Four changes, each measured with `delphi kb eval` on this repository's own documents (125 files, 998 chunks) against two question sets: `dev/kb-eval.json`, which the settings were tuned on, and `dev/kb-eval-holdout.json`, written before any reranker was tried and never tuned on. A third, small set of six questions about facts that live only in agent and skill frontmatter was written after the summaries (below) existed, to test them.

**The index stays fresh.** Every search first brings the index up to date (`Kb::refresh`): the daemon's `kb.search` and `kb.index` and the CLI's `kb search` all do it. A file whose modification time and size match what was recorded when it was indexed (`kb_documents.mtime_ns`, `size`) is not read, so checking the whole tree costs a stat per file, and a note written a minute ago is found. Changed files are re-chunked, new ones added, vanished ones removed; the preloaded vectors are applied (§3.6) and anything else is embedded by the background pass (§3.5). The fast path trusts the stamp: `delphi kb reindex` reads everything regardless. An index made before the columns existed gains them on open and is read once. `kb_meta.chunker_version` does the same for a change in how documents are chunked: when it differs, every document is read once, because an unchanged file would otherwise keep the chunks the old rules made.

**Agents and skills are searchable by what their frontmatter says.** The frontmatter of a shipped agent or skill is not chunked like the body, but it holds what they are looked up by. For zones `agents` and `skills` a first chunk, headed `Summary (frontmatter)`, is built from `title`, `role`, `name`, `description`, `authorized_skills` and `authorized_agents`, with its line range pointing at the frontmatter itself (so `resolve` re-anchors it like any other section).

**Reranking.** A cross-encoder, `cross-encoder/ms-marco-MiniLM-L12-v2` as int8 (34.6 MB, 12 layers; the model card says Apache-2.0, and it was trained on MS MARCO, whose data terms are for research use, which the operator should weigh before distributing commercially), reads the question together with each of the best 10 fused results and scores how well each answers it. It reads the section's heading and its first window of about 600 characters (longer passages did no better and cost more). Its ranking is added to the fusion as one more reciprocal-rank term with weight 6.0 (the result is flat between 3 and 30; 6 was best on the held-out set). The score floor is part of the model package, as the similarity floor is for the embedder (`Meta.min_similarity`, here -9): if even the best result scores below it, nothing is returned. The floor is what stops an off-topic question that happens to resemble some document (similarity up to 0.57) from getting results; floors of -8 and above lost answerable questions. `delphi kb eval` prints the score ranges to re-set it. Where it runs: on the CPU, on several threads, even on a Mac with a GPU (the embedder uses the GPU): a model this small is many short kernels, and Metal made a search about four times slower (2.2 s against 0.5 s for ten passages in testing). `DELPHI_RERANK_DEVICE=metal` overrides. It is found like the embedder: `DELPHI_RERANK_MODEL` (`off` switches it off), `$DELPHI_HOME/models/rerank.dlphmdl`, or compiled in (`DELPHI_RERANK_MODEL_FILE`); `dev/fetch-model.sh rerank` makes the package from a pinned revision with checked SHA-256 sums. It can be left out per call (`--no-rerank`, `"rerank": false`), and if it fails the search carries on in the order it had and says so once on stderr.

**Query-aware fusion.** Keyword matching is what finds `[REV-SEC-04]`; on a sentence it mostly matches filler and common words. Measured, vector search alone beat the hybrid on every sentence-style set (held-out MRR 0.644 against 0.629; frontmatter questions 0.861 against 0.468). A query is treated as a sentence (`is_natural_question`) when it contains no identifier-like word (a digit, `_ . / : @ #`, or a capital after the first letter) and either has five or more words or three or four words with at least one filler word. For a sentence the keyword rankings are weighted 0.1 (the sweep: 1.0, 0.5, 0.25, 0.1, 0 gave frontmatter MRR 0.64, 0.75, 0.83, 0.92, 0.83 and held-out 0.677, 0.677, 0.686, 0.701, 0.699). Keyword and identifier queries are unchanged.

**Results** (shipped settings; recall in the top 10, off-topic questions answered):

| Set | Ranking | R@1 | R@10 | MRR | Off-topic answered |
| :--- | :--- | :--- | :--- | :--- | :--- |
| dev (40 + 8) | keywords only | 0.25 | 0.57 | 0.361 | 5 of 8 |
| | vectors only | 0.45 | 0.80 | 0.584 | 0 of 8 |
| | hybrid | 0.50 | 0.85 | 0.611 | 0 of 8 |
| | **hybrid + reranker** | **0.53** | 0.82 | **0.628** | **0 of 8** |
| held-out (36 + 8) | keywords only | 0.33 | 0.61 | 0.403 | 7 of 8 |
| | vectors only | 0.58 | 0.78 | 0.644 | 2 of 8 |
| | hybrid | 0.56 | 0.83 | 0.643 | 2 of 8 |
| | **hybrid + reranker** | **0.61** | **0.83** | **0.701** | **0 of 8** |
| frontmatter facts (6) | hybrid | 0.50 | 1.00 | 0.667 | |
| | **hybrid + reranker** | **0.83** | 1.00 | **0.917** | |

Before this amendment (the §3.5 hybrid) the held-out set scored R@1 0.53, MRR 0.634 and answered 2 of 8 off-topic questions. The dev set gives up one answerable question from the top 10 (0.85 to 0.82 for the full pipeline), the price of the score floor and the lower keyword weight on sentences.

**Which reranker.** Under the same settings (weight 3, no floor, before query-aware fusion): MiniLM-L12 MRR 0.657 dev and 0.686 held-out at 35 MB and about 0.6 s; MiniLM-L6 0.597 and 0.651 at 24 MB and 0.27 s; `mxbai-rerank-xsmall` 0.665 and 0.645 at 80 MB and 0.95 s (better on dev, worse than L12 on held-out, and over the size budget); `bge-reranker-base` gained least and took 5 to 6 s a query at 297 MB. MiniLM-L12 was the only one that helped on both sets. With the 111 MB embedding model it brings the models to 146 MB, inside the 150 MB the operator set.

**Speed.** Without the reranker a search takes about 20 to 30 ms in the daemon. With it, the reranker adds about 0.5 s for ten passages on an M1, and it was 1.5 to 2 s in the runs above while the machine was loaded (load average 19 to 75, so the figures are pessimistic). A `delphi kb search` from the command line, which loads the models each time, took about 0.4 s without reranking and about 1 s with it under that load. The Linux CPU speed is not measured.

**Limits.** The sets are small (82 questions in all): a difference of one question is noise, and the settings above were chosen on them. Two held-out agent-role questions ("who splits a big task and hands the parts out", "the role that lands changes and cuts releases") still miss because other documents (`specs/003`, the handoff specs) answer them too and the labels accept only the agent file; they were left as written. Grow the sets from the misses the operator confirms with `delphi kb miss`, and re-run `delphi kb eval` before changing any weight, floor or model.

---

## 4. Orchestrator's Coaching, Two-Strike Circuit Breaker & 4-Channel Learning Loop

### 4.1 The Two-Strike Context Circuit Breaker (Mid-Mission Coaching)
When **Builder** fails the same test or build verification twice in a row inside a worktree session, the failed reasoning trace itself poisons subsequent turns. This breaker is step 3 of the unified retry ladder ([`specs/003`](003-collaboration-and-council.md) §3.2); it is not a separate policy. If the reset attempt also fails, the node escalates to the **operator**.
- **Invariant:** On Strike 2, `delphi-core` (not Orchestrator's judgment) trips the **Context Circuit Breaker**:
  1. Builds a **deterministic** diagnostic (no model call): the failing check's command, its exit code, and the first 3 error lines of its output, at most 500 bytes in total. It is tainted at the tier of that output.
  2. Clears **Builder's** conversation history.
  3. Respawns a **fresh Builder context** on a different model family. With a single family, it uses a different model of the same family (`homogeneous_retry: true`, `[H-008]`; `specs/003` §3.2). The diagnostic is injected into the `HandoffEnvelope`.
  4. **Carries the taint ceiling forward.** The fresh span starts with the `external_ceiling` of attempts 1–2, because the worktree still contains their output. Resetting the conversation must not reset trust.

### 4.2 Post-Mission ExpeL Contrastive Distillation
At mission close, **Orchestrator** executes `review-md-distillation` and `lesson-promotion`:
- Candidate lessons are checked against the **6-Tier IFC Taint Lattice (`[H-004]`)**: only `user_ratified` ($L_5$) and `internal_empirical` ($L_4$) lessons may enter `context/heuristics.md`, `REVIEW.md`, `lessons.md`, or `skills/`.
- Using rule-ID match + BM25 overlap (plus embedding cosine similarity between the candidate and an existing rule, with the threshold set from the model's measured scores, not a fixed $0.82$ (Amendment 004-A1)) via `lesson-deduper`, **Orchestrator** proposes ExpeL operations (`ADD`, `MERGE`, `DEMOTE`, or `PROMOTE` 2-strike rules to deterministic scripts in `skills/*/scripts/`).
- Proposals are submitted through the daemon's `propose` tool, stored in daemon-only `local/proposals/<mission-id>/`, and applied by `delphi-core` only after the **operator** ratifies them at `/retro` (GOVERNANCE §5.7, §7). This includes `lessons.md` (D-11, revised 2026-10-23): nothing enters an always-loaded prompt without ratification.

## 5. Memory Scopes, Operator Customization & Evolution (D-20)

Delphi learns at two levels at once. **Each operator's Delphi adapts to that person**: their preferences, tools and recurring corrections, across all their projects. **Delphi itself improves centrally**: lessons that prove generally useful flow back into the built-in processes every install receives. Neither may leak into the other without an explicit, ratified step.

### 5.1 Four Scopes
| Scope | Holds | Lives in | Visible to | Changes through |
| :--- | :--- | :--- | :--- | :--- |
| **Release** | Built-in governance, role manifests, skills, seed heuristics (`[H-…]`), default theme | Binary → `base/` (`specs/006` §3.1) | Everyone on that release | Delphi's own development (§5.5), delivered by `delphi update` with ratification |
| **Team** | `REVIEW.md`, team heuristics (`[T-…]`), domain docs, team agent and skill overrides | Workspace `team/` (optionally a team-registry git clone) | The team | Ratified proposals; `delphi sync` (`specs/007` §4) |
| **Operator** | Personal profile (preferences, tools, style), personal heuristics (`[O-…]`), per-role personal lessons, personal agent and skill customizations | `$DELPHI_HOME/operator/`, shared by all of this operator's workspaces | **Only this operator** | Ratified proposals; `delphi customize` |
| **Workspace** | Project-specific heuristics (`[W-…]`) and per-role lessons, missions, history | Workspace `local/` | Only this operator, in this workspace | Ratified proposals |

The rule-ID prefix shows a rule's scope everywhere it appears. Role isolation (GOVERNANCE §4.1) applies inside every scope: a role loads only its own lessons.

### 5.2 Assembly into the Frozen Snapshot
For each task, IPR blocks 3–4 (`specs/002` §2) are built from all four scopes, **broadest first**:
- heuristics: release → team → operator → workspace;
- the role's lessons: operator → workspace;
- then the operator profile, summarized to ≤ 1 KB.

More specific scopes come later in the prompt, closer to the task.
- **Caps:** each file keeps its Rule-of-25, 6 KB and 350-character caps (§2.2). The combined always-loaded memory for one agent is capped at **12 KB**, split into per-scope budgets:

  | Scope | Budget |
  | :--- | :--- |
  | Release | 4 KB |
  | Team | 3 KB |
  | Operator (rules, lessons and profile) | 3 KB |
  | Workspace | 2 KB |

  Unused budget passes to the next more specific scope.
- **Trimming within a scope:** if a scope exceeds its budget, it trims its own rules: lowest rule score first once `rule_stats` exists (§5.6, Step 6), and before that **oldest ratification first**. Rules tagged `security` are never trimmed. Every trim is logged, and Orchestrator proposes merges at the next `/retro`.
- **Conflicts:** when two rules conflict on *how* to work (style, tools, conventions), the more specific scope wins. **No rule in any scope can relax governance**: approvals, taint, sandboxing, `REVIEW.md` `P0` rules or role scopes. The rule linter rejects rules that reference those controls, using the same deterministic check as theme packs (`specs/001` §6.4). It applies equally to rule text, `operator/profile.md`, and the instruction bodies of every agent override (`core/`, `operator/core/`, `team/agents/custom/`). Red-Team reviews every operator-scope, team-scope and override change before it is offered for ratification. The IPR's governance block (`specs/002` §2, block 1) states explicitly that memory and override content (blocks 2–4) cannot override it.

### 5.3 Operator Customization
- **Profile (`operator/profile.md`):** stated preferences such as languages, package managers, code style, review strictness, explanation depth, working hours and notification channel. It is edited directly with `delphi customize profile` (operator-authored, so $L_5$), or grown from proposals.
- **Personal overrides** of built-in roles and skills live in `operator/core/agents/<role>/` and `operator/skills/`, and apply in every workspace. Precedence: workspace `core/` > operator `core/` > team `custom/` > release.
  - Overrides may change instructions freely, but under `specs/001` §2.3 they may only **narrow** permissions.
  - **Permission widening (D-21, ratified 2026-10-23):** `default_access_scope` stays **narrow-only, with no grants**. Widening a non-builder role would let code change outside `IsolatedBuild` and the merge gates, and an operator-scope waiver would apply silently in every workspace. Instead, overrides may **attach** ratified custom or operator skills whose `required_permissions` fit within the role's built-in scope (`specs/001` §2.3).
- **Survives updates:** operator overrides are 3-way merged against the release ancestor on `delphi update`, exactly like workspace `core/` (`specs/006` §4). Conflicts are staged, never overwritten.
- **Pinned:** operator-scope files are hashed in `operator/ratified.lock` and loaded only from ratified copies, under the same rule as `specs/006` §3.1.

### 5.4 Learning Routing at `/retro`
Every proposal Orchestrator stages (§4.2) carries a **proposed scope** with a one-line rationale. The operator can change the scope when ratifying.
- **Default is the narrowest** plausible scope (workspace), because learning broadens only on evidence.
- **Workspace → Operator:** suggested automatically when an equivalent lesson has been ratified in at least 2 workspaces, or when it's about the operator's own preferences rather than the project. Equivalence is checked against normalized rule hashes recorded in `$DELPHI_HOME/operator/` at ratification; no daemon ever reads another workspace's `local/`.
  - **Project-agnostic only:** operator-scope proposals are rejected by a deterministic linter if they contain identifiers from the source workspace: repo names, hosts, paths, and terms listed in that workspace's `config.toml` `[privacy] identifiers`. Orchestrator generalizes the wording before proposing.
  - **Isolated workspaces:** `[workspace] isolated = true`, for example confidential client work, means nothing is ever promoted out of that workspace.
  - **No leakage into team files:** writes to `team/**` are rejected if they contain `[O-…]` rule IDs or `operator/` references.
- **Operator → Team:** offered only as an explicit, Red-tier proposal through `delphi sync`. Personal memory is never shared implicitly.
- **Any → Release:** a lesson that is general, has evidence ($L_4$ or better, measured improvement per §5.6), and isn't project-specific is marked an **upstream candidate** (§5.5).

### 5.5 Evolving Delphi's Central Processes (upstream contribution)
Improvements to Delphi's own process (built-in role manifests, skills, heuristics, `REVIEW.md` rules, governance) go back to Delphi's source repository, the install source (`specs/006` §3.1), through Delphi's own spec-first process:
1. **`delphi contribute <proposal>`** builds a **contribution bundle**:
   - the proposed change as a diff against the release file;
   - its rationale;
   - its evidence: ledger metrics before and after (first-pass success, retries, tokens per verified task), test results, and how many missions and workspaces it was observed in.
2. **Privacy scrub, applied by `delphi-core` (deterministic):**
   - **The diff:** the single target rule or section applied to a clean `base/` copy, so other customizations in the same file never leak.
   - **The evidence:** numeric aggregates only (counts, rates, deltas), never raw test output, stack traces or generated rationale text.
   - **The scan:** the whole bundle is checked by the `[REV-SEC-05]` path and secret scan and the workspace identifier filter.
   - The operator previews the **exact bytes** that will be sent.
3. **Submission is outbound, so it's Red-tier:** after the Red approval, `delphi-core` itself submits exactly the previewed bytes as an issue or PR against the Delphi source repo, or exports them as a file. No model turn happens between preview and send, so nothing unpreviewed can be added.
4. **Upstream review:** maintainers review contributions as Architect-authored spec amendments or REVIEW/heuristic changes, through a Council, as in this repo's own history.
5. **Delivery:** accepted changes ship in a release and reach every install through `delphi update`. They arrive as ratification proposals, and each operator's customizations are merged, not overwritten (§5.3).

### 5.6 Evidence: Is a Rule Actually Helping?
- **`rule_stats`:** the daemon keeps a `rule_stats` table in `state.db`, derived from events:
  - for each rule, when it was ratified and in which scope;
  - how many tasks ran with it loaded;
  - violations caught by deterministic checks (feeding the two-strike promotion, `[H-005]`);
  - first-pass success, retries and tokens per verified task for its task types, compared with the window before it was introduced (from the usage ledger, `specs/005` §2.4).
- **Schema and source:** `rule_stats` is derived from `rule.ratified`, `rule.trimmed` and `rule.violation` events plus the usage ledger, so it is rebuildable like the rest of `state.db`:
  ```sql
  CREATE TABLE IF NOT EXISTS rule_stats (
      rule_id TEXT PRIMARY KEY, scope TEXT NOT NULL, tags TEXT NOT NULL,
      ratified_seq INTEGER NOT NULL, tasks_since INTEGER NOT NULL DEFAULT 0,
      violations_caught INTEGER NOT NULL DEFAULT 0,
      fps_before REAL, fps_after REAL,                 -- first-pass success rate, matched task types
      retries_before REAL, retries_after REAL,
      tokens_per_verified_before REAL, tokens_per_verified_after REAL
  );
  ```
- **Rule score (deterministic):** `0.5·Δfps + 0.3·(−Δretries / max(retries_before, 1)) + 0.2·(−Δtokens / max(tokens_before, 1))`, using windows of at most 20 tasks of the rule's task types on each side. Rules with fewer than 5 tasks on either side get no score, and keep their position.
- **What the score does:** it combines those signals. It ranks rules for Rule-of-25 eviction and §5.2 cap trimming, flags upstream candidates, and drives `ttl_missions` decay. A rule with no measurable effect after its TTL gets a `DEMOTE` proposal; it is never deleted silently.
- **Not proof:** every rule of a role is loaded in every task of that role, so there is no exposure variation. Only before/after comparisons are possible, and rules ratified together can't be told apart. The `/retro` view labels these as correlational signals. They inform the operator's decision, not replace it.

### 5.7 Privacy & Portability
- Operator scope never leaves the machine. It isn't synced, contributed or federated, except by an explicit `delphi operator export`, which produces an encrypted bundle for moving to a new machine (`delphi operator import`).
- Teammates never see operator lessons, profile or customizations. Shared artifacts record only role IDs and team-scope rule IDs.

### 5.8 Phase Mapping
| Step | Delivers |
| :--- | :--- |
| **1** | `delphi init` creates `$DELPHI_HOME/operator/` (empty profile and heuristics, `ratified.lock`, `ratified/`); `doctor` and `lint` validate it (006-A1) |
| **2** | Snapshot assembly across scopes with per-scope budgets and oldest-first trimming; profile; scoped CAS locks; role-filtered retrieval |
| **6 (usage-gated)** | `/retro` scope routing and the workspace→operator suggestion; team scope via `delphi sync`; `rule_stats` and rule scores |
| **Post-v1** | `delphi contribute` bundles and upstream submission (test 11); `delphi operator export/import` |

---

## 6. Verification Plan
1. **Identifier Tokenization Test:** Verify FTS5 `unicode61 tokenchars '_-.:/@#'` indexes `[H-004]`, `REV-SEC-04`, and `Part.thoughtSignature` as single exact tokens and ranks them #1 on exact queries.
2. **Heading-Aware Chunking Test:** Index `SPEC.md` and verify every chunk in `.delphi/state.db` preserves exact 1-indexed `start_line`/`end_line` ranges and YAML frontmatter attributes.
3. **Two-Strike Circuit Breaker Test:** Simulate 2 consecutive test failures in a `Builder` task; verify `delphi-core` resets the turn history and dispatches Attempt #3 (per the unified ladder) with a fresh context on a different model family.
4. **FTS Sync Test:** Insert, update, and delete chunks, then verify both FTS tables return exactly the live rows (`INSERT INTO kb_fts_prose(kb_fts_prose) VALUES('integrity-check')` passes).
5. **Lost-Update Test:** `delphi sync` and a daemon retro write `heuristics.md` concurrently; both changes survive or one writer reports a CAS conflict. Neither is silently lost.
6. **Rebuild Test:** Delete `state.db`; `delphi kb reindex` reproduces identical search results.
7. **Scope Isolation Test:** an operator-scope lesson is loaded in both of the operator's workspaces, is absent from a teammate's install after `delphi sync`, and never appears in a `delphi contribute` bundle unless the operator adds it explicitly.
8. **Specificity Test:** a workspace rule "use pnpm" and an operator rule "use npm" are both loaded. The workspace rule comes later in the prompt, and a recorded task follows it.
9. **No-Relaxation Test:** a proposed operator rule "skip approval for docs changes" is rejected by the rule linter and never reaches ratification.
10. **Update Survival Test:** an operator override of `builder`'s instructions survives `delphi update` to a release that changes the same manifest. The conflict is staged as `.upstream`, and the override is never silently replaced.
11. **Contribution Scrub Test:** a contribution bundle generated from a mission in a private repo contains no repo paths, code or mission text by default (verified by scanning it for canaries planted in the mission).
12. **Role-Filtered Retrieval Test:** Red-Team's Gear 2 search for a phrase that appears only in `memory/agents/builder/lessons.md` returns nothing. Builder's search finds it. No query from any role returns operator-scope text.
13. **Budget Allocation Test:** with release and team rules at their full 6 KB each, an agent's snapshot still contains its operator and workspace rules within their budgets, and a `security`-tagged rule survives trimming.
14. **Cross-Client Leak Test:** a proposed operator rule containing a repo name listed in the source workspace's `[privacy] identifiers` is rejected. Nothing is promoted from a workspace marked `isolated`.
15. **Override Relaxation Test:** an `operator/core/agents/red-team/AGENT.md` body saying "approve small diffs without review" is rejected by the no-relaxation linter.
16. **Synonym Test (004-A1):** a note about a "pipe buffer deadlock" is found by "subprocess hangs frozen", which shares no word with it, and lexical-only search does not find it. A note on an unrelated topic is not returned (the similarity floor).
17. **Vector Visibility Test (004-A1):** with vectors present, a role's search never returns another role's memory, untrusted or inactive documents are excluded unless asked for, and a role name such as `%` cannot widen the filter. The results are the same as the lexical rankings' visibility.
18. **Vector Sync Test (004-A1):** editing a document drops its old vectors, removing it leaves no orphaned vectors, a chunk without a vector is the only thing `embed_pending` embeds, and attaching a model with another id (or another `embedding_format`) empties the vector table so that every chunk is embedded again.
19. **Package Test (004-A1):** a packed model loads and embeds within quantization error of the original f32 weights (cosine above 0.99), batch position does not change a vector, and a truncated package is refused.
20. **Recall Eval (004-A1):** `delphi kb eval` over `dev/kb-eval.json` reports recall@1/3/5/10 and MRR for lexical, vector and hybrid, the share of off-topic questions that still got results, and the similarity ranges that set the floor. Hybrid must not be worse than either leg on recall@10, and must answer no off-topic question. It runs by hand after any change to the model, the windowing or the fusion weights, because it needs the model.
21. **Preloaded Vector Test (004-A2):** vectors exported from one index and loaded as the cache of a fresh index mean the model reads no text for unchanged documents (`seeded()` equals the chunk count), search returns the same results, an edited document is the only one the model reads, and `apply_seed_vectors` fills what the cache covers without calling the model and leaves the rest pending without stalling.
22. **Cache Integrity Test (004-A2):** a vector cache round-trips; keys differ when the same characters are split into windows differently; a cache for another model id, dimension or embedding format does not match; a truncated or foreign file is refused.
23. **Agents and Skills Test (004-A2):** shipped and team `agents/` and `skills/` files are indexed with the tiers in §3.6, the team copy replaces the shipped one for the same path, and a file that declares itself untrusted is excluded from default results.
24. **Seed Coverage Check (004-A2):** `dev/build-seed-vectors.sh` fails unless a second fresh workspace is fully covered by the cache it just made. It needs the shipped model, so it runs in the release workflow and by hand, not in CI.
25. **Freshness Test (004-A3):** a search sees an edited, a new and a removed file without a reindex; an unchanged file is not read (a file given the same size and modification time keeps its old chunks until `reindex`); an index without the stamp columns gains them and is read once; an index with another `chunker_version` re-chunks every document once.
26. **Reranker Fusion Test (004-A3):** with a reranker attached the passage it prefers moves to the top; weight 0 leaves it out and it is not asked; it reads only `rerank_top` results; a best score below the floor returns nothing and one above does not; a reranker that fails is skipped and the order stands.
27. **Cross-Encoder Package Test (004-A3):** a packed cross-encoder scores passages like the original weights to within quantization error, gives the same score alone or in a batch, takes its score floor from the package (none when uncalibrated), and refuses a package of another kind.
28. **Query Type Test (004-A3):** sentences are told from keyword queries and identifiers (`is_natural_question`).
29. **Summary Chunk Test (004-A3):** an agent's role is found through its frontmatter summary, whose pointer covers the frontmatter lines and stays valid.
30. **Held-Out Evaluation (004-A3):** `delphi kb eval --file dev/kb-eval-holdout.json` is run after any change to the models, the windowing or the fusion; hybrid with the reranker must not do worse than hybrid without it on that set, and must answer none of its off-topic questions. The set is never used to choose a setting.
31. **Release Dry Run (004-A3):** running the `release` workflow by hand fetches both models, makes and checks the preloaded vectors, builds all four executables with the models compiled in, and on the targets that can run on their runner checks that both models load, that the preloaded vectors match, and that a fresh workspace answers a question; it publishes nothing. Run it before tagging.
