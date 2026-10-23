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
approved_sections: "§2, §4.1, §5, plus the §3.4 role-filter rule (operator: Council 003 + Council 004, 2026-10-23); §3 schema and §4.2 remain draft"
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

### 2.0 Phase Mapping (§§2–4)
| Roadmap step | Delivers |
| :--- | :--- |
| **2** | §2 ratified four-scope snapshot with per-scope budgets (§5.2), oldest-first trimming, scoped CAS locks; §3.4 role-filtered retrieval rule |
| **3** | §4.1 context circuit breaker (it needs Builder and the retry ladder) |
| **5** | §3 FTS5 knowledge-base search |
| **6** | §4.2 `/retro` distillation, `rule_stats`, vectors (once D-01's reopen trigger fires) |

§5.8 covers the §5 rows.

### 2.1 The Frozen Boot-Snapshot Invariant (Prompt Cache Hygiene)
Inspired by Nous Research's `hermes-agent` snapshot pattern:
- **What loads (§5.2 order).** When a task starts, `delphi-core` reads these files **only from their ratified copies** and assembles them into an **immutable in-memory prefix snapshot** (IPR blocks 3–4, `specs/002` §2):

  | Scope | Heuristics | Lessons (this role) | Ratified copy |
  | :--- | :--- | :--- | :--- |
  | Release | `context/heuristics.md` (`[H-…]`) | — | `base/context/heuristics.md`, verified against `base/manifest.lock` |
  | Team | `context/team-heuristics.md` (`[T-…]`) | — | `local/ratified/team/context/team-heuristics.md` |
  | Operator | `heuristics.md` (`[O-…]`), plus `profile.md` | `memory/agents/<role>/lessons.md` | `$DELPHI_HOME/operator/ratified/…` |
  | Workspace | `heuristics.md` (`[W-…]`) | `memory/agents/<role>/lessons.md` | `local/ratified/local/…` |

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
   - `.delphi/state.db` indexes **only Markdown artifacts** (`context/**`, `specs/**`, `missions/**`, `memory/**`) using content-addressed `blake3` incremental hashes. Operator-scope files are **never indexed**; they reach agents only through the snapshot (§5.2).
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
Queries from an agent return `zone = 'memory'` rows **only for the calling role's own `memory/agents/<role>/`**, so no role can search another role's private lessons (GOVERNANCE §4.1). Queries also pre-filter out `status != 'active'` and untrusted taint tiers (`external_untrusted`, `federated_peer`) unless explicitly requested. In v1, results come from BM25 alone: `kb_fts_ident` (weights `2.0, 5.0, 10.0`) fused by RRF with `kb_fts_prose`. Once D-01 enables vectors, **`sqlite-vec` cosine distance** joins the same RRF ($k=60$):
$$\text{Score}_{\text{RRF}}(d) = \frac{w_{\text{lex}}}{60 + r_{\text{bm25}}(d)} + \frac{w_{\text{vec}}}{60 + r_{\text{vec}}(d)}$$
Results return **at most 10 `path#Lstart-Lend` pointers + 3-line previews** (`<150` lines total, enforcing `[H-002]`). Each pointer carries the chunk's `heading_path` and `blake3`. When a file has been edited since indexing, the resolver re-anchors by heading. If the heading is gone, it reports the pointer as stale rather than returning the wrong lines.

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
- Using rule-ID match + BM25 overlap (plus `sqlite-vec` cosine similarity $>0.82$ once vectors are enabled (D-01 reopen trigger: >50 missions **and** a logged terminology-shift miss)) via `lesson-deduper`, **Orchestrator** proposes ExpeL operations (`ADD`, `MERGE`, `DEMOTE`, or `PROMOTE` 2-strike rules to deterministic scripts in `skills/*/scripts/`).
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
