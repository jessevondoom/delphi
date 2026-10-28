---
id: spec-049-context-management-recoverable-compaction-and-handles
title: "049: Context management: recoverable compaction, argument trimming, offload previews, structured digests and handle relay (draft)"
author: architect
timestamp: 2026-10-28T18:10:00Z
status: draft   # a proposal, not permission to build (No Spec, No Build). Amends 045 T5/T6 and 030 §4.3 if approved; open decisions in §8.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # the techniques are from the model's recollection of LangChain's Deep Agents context handling (offload to files, argument truncation, summarization with the full history saved, sub-agent isolation); not re-checked, not yet measured in Delphi
source_mission: 2026-10-23-foundational-design
tags: [spec, context, compaction, handoff, payload-store, draft]
---

# Spec 049: Context management

> **Status: draft.** Sibling of `specs/048` (benchmarking) and `specs/050` (memory). It refines slices H2 and H3 of `specs/045` and the model-written summary of `specs/030` §4.3 before they are built. Nothing here changes what a role may touch.

## 1. Why

A long span fills its window with old tool results, large tool-call arguments and its own history. LangChain's published approach has four parts we can compare with ours: write large results to files and keep a preview; truncate old large tool-call arguments before summarizing; summarize late, but save the full pre-summary history so it can be read back; and isolate sub-agents so they return a result, not their whole context.

**What exists, so this is not rebuilt:** prompt-cache blocks and frozen boot snapshots (002 §3, 004 §2.1); `@payload/<id>` handles for connector results (045 T5, planned for large tool results); compaction with a flush and a deterministic check, `replayable: false` (045 T6, not built, H3); the session digest and an optional model-written summary of at most 400 tokens, one paragraph (030 §4); `/context` (030).

**What is missing** is listed in §3. All of it is deterministic code around a model call; none adds a model call.

## 2. Principles

1. **Additive, never widening** (045 §2.1). A handle names content already in the mission's payload store; it grants no access to anything else. `read_file` on a handle is subject to the same sandbox as any read.
2. **Lossy summaries, lossless store.** Whatever a compaction or trim removes from the prompt stays recoverable from the payload store for the life of the mission.
3. **Taint travels.** A handle, a preview and a summary carry the lowest taint of the content they stand for. A preview of untrusted text is untrusted text (fenced as such) and never raises a trust tier (spec 001 §5, 030 §5).
4. **Cache-aware.** Trimming and clearing happen in batches so the cached prefix is invalidated rarely and never mid-batch (045 T5); each change shows up in cache accounting (045 T9).
5. **Off until measured.** Every addition has a switch under `[loop]` (or `[session]` for CX-5), default `off`, and moves only on the 048 bar.

## 3. The additions

| # | Addition | Behavior |
| :-- | :-- | :-- |
| CX-1 | **Argument trimming** | Before any tool result is cleared (045 T5) and long before a compaction, replace *old, large tool-call arguments* (a written file body, a patch, a long command) with a stub: `[argument trimmed: 4.1 KB, @payload/…]`. The content is first stored in the payload store, so the stub is lossless. Keeps the last N turns intact. Deterministic, no tokens. |
| CX-2 | **Offload previews** | A handle for an offloaded result carries a bounded preview (first and last lines, size, content type, at most 400 bytes, control sequences removed, fenced as tool output) so the model can tell whether to read it. |
| CX-3 | **Recoverable compaction** | A compaction (045 T6) stores the dropped turns, whole, in the payload store and puts `@payload/<id>` in the summary. The compaction check (every open task and every written file named) is unchanged; the handle is what makes a miss recoverable instead of silent. A missing or unreadable handle is a failed compaction, not applied. |
| CX-4 | **Threshold as an experiment** | The 045 T6 high-water mark (70 %) is a guess. Run it as arms at 45, 60, 70 and 85 % on the 048 suites (BM-7 context retention, plus the 037 tasks) and record the result; the default stays `off` until then. |
| CX-5 | **Structured session digest** | The optional model-written summary (030 §4.3) changes from one paragraph of 600 characters to fixed fields: asked, done, decisions, open items, files touched, constraints. Each field is capped; the whole stays within the 400-token cap. The deterministic digest is unchanged. Every 030 rule stays: off by default, never for a session that read untrusted content, sanitised, redacted, not charged to the mission. A field-level recall probe (048 BM-7) decides whether the fields beat the paragraph. |
| CX-6 | **Result summary plus handles** | A seat's result to the Orchestrator is a short summary plus handles to its raw outputs (artifacts, diffs, tool results, test output). The Orchestrator relays the **handles** onward and quotes the raw output where the operator needs it, so exact items survive the hop. This resolves the tension between context isolation (return little) and fidelity (do not paraphrase): return little, but point to the exact. The handoff envelope's 2,000-token limit is unchanged; handles are short. Touches `crates/delphi-engine/src/handoff.rs`; design confirmed in slice CX1. |
| CX-7 | **Context composition in accounting** | `/context` and the span report show how the window is spent: system blocks, memory, slices, history, tool results, trimmed and cleared amounts, handles outstanding. Read-only; the console takes its data from the daemon as now. |

## 4. Where each lives

| Addition | Code |
| :-- | :-- |
| CX-1, CX-2 | `Engine::drive` history handling in `crates/delphi-engine/src/engine.rs`; the payload store (as connector results use it); batching with 045 T5 |
| CX-3 | the 045 T6 compaction code (H3), the payload store, the compaction check |
| CX-4 | the 048 harness arms |
| CX-5 | `summary.rs` and its hook in `EngineService::begin_session`; `sessions.jsonl` summary line gets the fields |
| CX-6 | `crates/delphi-engine/src/handoff.rs`, `bridge.rs`, role instructions for the Orchestrator and seats; schema for the handoff result under `schemas/` |
| CX-7 | `session.context`, the span report, `delphi observe`; `ui/src` only displays (no private clocks; reduce motion unaffected) |

## 5. Invariants and tests

- **No loss:** a test trims, clears and compacts a scripted history, then reads every handle and rebuilds the original turns byte-for-byte.
- **Taint:** a handle from a tainted span stays tainted; a preview of hostile text containing instructions is fenced and does not alter the span's ceiling.
- **Cache:** a trim batch invalidates the prefix once; accounting names the cause.
- **Check still bites:** a compaction that omits an open task or written file is refused, with and without a handle.
- **Handle scope:** a handle from another mission is refused; `read_file` of a handle outside the sandbox is refused.
- **Relay:** with CX-6 on, a scripted chain passes 048 BM-5 at 1.0.

## 6. Acceptance criteria

- **AC-1** CX-1 reduces history size on a scripted long span and the original arguments are recoverable.
- **AC-2** CX-2 previews are at most 400 bytes and carry the source's taint.
- **AC-3** CX-3: after a compaction, 048 BM-7 recovery is higher with the handle than without, on the same scripted model.
- **AC-4** CX-5 fields fit the 400-token cap and stay redacted and sanitised; a tainted session produces none.
- **AC-5** CX-6: the final answer to the operator contains the seeded exact items from the seat's raw output.
- **AC-6** Defaults are `off`; a switch turns each on independently; the docs and `schemas/config.schema.json` are regenerated (`tests/docs.rs`).

## 7. Build plan

| Slice | Contents | Depends on |
| :-- | :-- | :-- |
| CX1 | design confirmation for CX-6 against `handoff.rs`; CX-1 argument trimming and CX-2 previews, folded into 045 H2 | 045 H1; 048 BM1 for measuring |
| CX2 | CX-3 recoverable compaction, folded into 045 H3 | CX1, 045 H2 |
| CX3 | CX-7 context composition in accounting | CX1 |
| CX4 | CX-6 result summary plus handles, Orchestrator and seat instructions, schema | CX1, 048 BM2 |
| CX5 | CX-5 structured digest | 048 BM3 |
| CX6 | CX-4 threshold arms and default decisions | CX2, 048 BM3 |

Where a slice is "folded into" a 045 slice, the 045 slice's pull request carries it and the tracker lists both specs.

## 8. Open decisions (provisional answers, change on request)

- **OD-1** Keep the dropped turns for the life of the mission (provisional), or age them out by size? Provisional: for the mission, bounded by the payload store's existing limits; revisit if it grows.
- **OD-2** How many recent turns stay untrimmed (CX-1). Provisional: 6.
- **OD-3** Whether CX-6 handles replace pasted raw output in the operator-facing answer, or only travel between seats. Provisional: between seats only; the operator sees the exact text quoted where it matters.
- **OD-4** Verify the LangChain description (OD-1 of spec 048) before approval.

## 9. Non-goals

Compacting a CLI seat's own context (the CLI manages it, 045 T5/T6). Vector storage of history. Letting the model choose what to keep without the deterministic check. Raising the envelope limit.

## 10. Known limits

Handles add a read when the model needs the content. A preview can mislead about what follows it. A structured digest costs more to write than a paragraph. All defaults are unmeasured.
