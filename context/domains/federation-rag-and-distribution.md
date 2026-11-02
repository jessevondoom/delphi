---
id: kb-federation-rag-and-distribution
title: "Federated Multi-Instance Mesh, Zero-Server Hybrid RAG & Multi-Operator Distribution"
author: scout
timestamp: 2026-10-23T07:54:00Z
source_type: internal_empirical
confidence: high   # 'verified' requires a linked test artifact (specs/001 §5)
status: active
source_mission: "seed-central-knowledge-base"
tags: [federation, a2a, ucan, biscuit, hybrid-rag, sqlite-vec, fts5, rrf, three-zone-updater]
---

# Federated Context Sharing, Local Hybrid RAG & Multi-Operator Distribution

## 1. Federated Multi-Instance Context Sharing (`specs/007`)

To allow multiple independent Delphi installations (run by different operators or across different machines) to collaborate without exposing private `memory/agents/` or requiring a centralized cloud database, Delphi uses a **Three-Plane Federation Architecture** mediated by **Messenger**:

1. **Plane 1 — Agent-Team-to-Agent-Team Delegation (Google A2A Profile):**
   - Each `delphi-core` instance exposes a signed Agent Card (`/.well-known/delphi-card.json`) with its Ed25519 DID (`did:key:z6Mk...`) and high-level team skills.
   - Peer instances delegate bounded tasks via A2A JSON-RPC 2.0 + SSE over **Tailscale (`tsnet`)**, **SSH Multiplexed Unix Sockets**, or **LAN mDNS**.
2. **Plane 2 — Scoped `xenia://cap/` URI Pointers & Capability Tokens (`DelphiCap`):**
   - Instead of copying massive files across instances (violating `[H-002]`), instances exchange compact pointers:
     `xenia://cap/bob.tailnet/payments/context/domains/stripe.md?rev=a8f91c2&cap=<token>#L40-L120`
   - Access is governed by **Ed25519 Object-Capability (OCap) tokens** (UCAN/Biscuit profile) that cryptographically restrict the holder to an exact file path, Git commit SHA, line range (`#L40-L120`), byte limit, and expiration timestamp.
   - **Hard Chroot Allowlist:** `delphi-core` enforces a compile-time block preventing `memory/**`, `.delphi/config.toml`, `.delphi/state.db`, or `.env*` from ever being served over `xenia://cap/`.
3. **Plane 3 — The 6-Tier Security Taint Lattice (`[H-004]` Extension):**
   - $L_5$ (`user_ratified`) $>$ $L_4$ (`internal_empirical`) $>$ $L_3$ (`internal_generated`) $>$ $L_2$ (`federated_verified`) $>$ $L_1$ (`federated_peer`) $>$ $L_0$ (`external_untrusted`).
   - **Transitive Anti-Laundering Rule:** $\text{EffectiveTaint} = \min(\text{PayloadTaint}, \text{PeerTrustCeiling})$. If a teammate ($L_2$) forwards web-scraped data ($L_0$), it remains $L_0$ (`external_untrusted`) and can never auto-promote into `heuristics.md`, `REVIEW.md`, or `lessons.md`.

---

## 2. Local Zero-Server Hybrid RAG (`specs/004`)

### 2.1 Why Pure Vector Search Fails on Technical Markdown
Pure dense embeddings compress exact identifiers (`[H-004]`, `REV-SEC-04`, `Part.thoughtSignature`) into generic semantic neighborhoods, sever Markdown headings (`# H1 > ## H2`) from child paragraphs, and ignore YAML provenance/taint metadata.

### 2.2 The Two-Gear Retrieval Protocol (`.delphi/state.db`)
- **Gear 1 (Hot Code & Curated Docs):** Live source code (`src/**`, `.delphi/worktrees/**`) is **never** vector-indexed (avoiding index staleness during TDD). Agents navigate via `context/index.md` + `ripgrep` / `ast-grep`.
- **Gear 2 (Archival Missions & Domain Docs):** `.delphi/state.db` indexes Markdown files (`context/`, `specs/`, `missions/`, `memory/`, and the agents and skills) using:
  1. **Deterministic AST Breadcrumb Chunking:** Splits on `# H1`/`## H2`/`### H3` while keeping code blocks and tables intact, recording exact `start_line`..`end_line` numbers and prepending `[File | Lines | Heading Path | Source | Status]`.
  2. **SQLite FTS5 (BM25):** Configured with `tokenize="unicode61 tokenchars '_-.:/@#'"` so technical IDs (`H-004`, `REV-SEC-04`) are tokenized intact.
  3. **`sqlite-vec` Vector Search:** local nearest-neighbour search inside the same SQLite ACID file, over vectors from a BERT-family model that runs in the binary on `candle` (the dimension follows the model; `specs/004` §3.5 names it), with preloaded vectors for the shipped documents (§3.6). Built in Amendments 004-A1 and 004-A2; before that this document described it as planned.
  4. **Reciprocal Rank Fusion (RRF, $k=60$):** Pre-filtered by `status = 'active'` and taint tier, returning at most 10 `file:///path#Lstart-Lend` pointers (`<150` lines total).
  5. **Cross-encoder reranking:** a small model reads the question together with each of the best few results and puts the best answer first; a best score below a calibrated floor returns nothing (`specs/004` §3.7, Amendment 004-A3). This is the answer to §2.1's concern: exact identifiers are matched by the keyword rankings, meaning by the vectors, and the final order by a model that reads both texts.

---

## 3. Multi-Operator Three-Zone Distribution (`specs/006`)

- **Zone 1 (Core Upstream — `GOVERNANCE.md`, `SPEC.md`, `agents/`, built-in `skills/`):** Embedded inside the signed `delphi` binary alongside `.delphi/manifest.lock` (`blake3` ancestor hashes). Updated via deterministic **Three-Way Merge (`diff3`)** during `delphi update`.
- **Zone 2 (Team Shared — `REVIEW.md`, `context/`, `specs/`, custom `skills/`):** Synchronized across operators via `delphi sync` using AST Rule-ID union merges and provenance gates.
- **Zone 3 (Operator Private — `memory/agents/`, `missions/`, `.delphi/worktrees/`, `.delphi/state.db`):** Mathematically blocked from ever being overwritten by `delphi update` or `delphi sync`.
