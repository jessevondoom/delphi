---
id: spec-116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context
title: "116: Memory that grows: a provisional tier, findings, consolidation, and a larger start context"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: draft   # written at the operator's request on 2026-11-14 ("make specs covering all 8, show me LOE and slices"), after an audit of the scout, the investigator and the knowledge base. Not approved. Nothing is built from it until the operator approves it.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and a read-only look at one live workspace; nothing was run
source_mission: 2026-11-14-knowledge-loop
tags: [spec, memory, lessons, investigator, retrieval, learning]
---

# Spec 116: Memory that grows

> **Status: draft, 2026-11-14.** Fifth of eight specs (112 to 119) from the knowledge-loop audit.
> **Extends:** [`specs/004`](004-memory-and-coaching.md) (memory and the Rule of 25), [`specs/050`](050-memory-lifecycle-types-supersession-and-trace-mining.md) (lifecycle and supersession), [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md) (retrieval for agents; R1 partly built), [`specs/065`](065-episodic-traces-and-agent-long-term-memory.md) (episodes), [`specs/108`](108-closing-the-learning-loop-for-host-sessions.md) (the loop for host sessions), [`specs/080`](080-kb-read-count-report.md) (read counts, not built).

---

## 1. Scope and objectives

| # | Observation (2026-11-14) |
| :-- | :-- |
| E-1 | Every path into memory an agent reads without asking (lessons, heuristics) ends at the operator: `delphi learn keep` at a terminal with the daemon stopped, `/retro`, or the digest at ten items a day. Capture allows thirty notes a mission. On a live workspace: 62 notes staged, 51 dropped, none promoted; eight of ten roles' lessons files hold no rule. |
| E-2 | `delphi learn keep --as lesson` accepts host notes only. A note from an agent inside the daemon cannot become a lesson by any command. |
| E-3 | A staged note is never loaded into a prompt and is not searched by inside agents. |
| E-4 | An investigation leaves a transcript, a mechanical episode (tools, files, the first error; at most three a mission), and a note in the mission folder only if the model chose to write one. That note defaults to `status: draft`, which the inside search leaves out. A later investigation cannot reliably find an earlier root cause. |
| E-5 | At the start of a root span an agent is given at most three passages, 1,500 characters in all. That retrieval is skipped whenever the handoff already holds a pointer, and the claim pull just before it adds pointers (`engine.rs`, the span-start condition), so one word-overlap claim removes the passages. Child spans get none. |
| E-6 | The inside `kb_search` returns pointers, not text, except for validated facts; 064 §4.1 asks for passages. |
| E-7 | Nothing merges, de-duplicates or reconciles memory. Episodes are written with no expiry and supersede nothing. |
| E-8 | Use changes nothing: read counts are logged and not reported (080), agent searches that find nothing are not logged, and no agent can say "this was wrong". Stored `confidence` is not read by ranking. |
| E-9 | The investigator's skills (bisect, benchmarks, code search by script) need a shell scope its manifest lacks; inside the daemon and as a host worker it can only read. While it is adopted, host search defaults to validated text only, which hides the unvalidated research its own charter tells it to read first. |

Objective: what agents learn is kept without waiting for the operator, clearly marked as unreviewed, found again when it is relevant, and tidied over time; the operator's review promotes and corrects instead of gating.

### 1.1 Non-goals

- Ratified lessons and heuristics, their caps and the Rule of 25 are unchanged. Nothing in this spec writes them without the operator.
- No graph database and no new embedder.
- Sharing between projects is [`specs/115`](115-research-recall-across-projects.md); the operator's view of memory is [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md).

### 1.2 Simpler alternative considered

*Raise the digest's daily limit and let `learn keep` run with the daemon up.* Worth doing (it is in G2) and not enough: the operator stays the only writer, so memory grows at the speed of review, and on the evidence of E-1 that speed is zero. The provisional tier exists so that an unreviewed note is still of use, at a trust level that cannot hurt.

---

## 2. A provisional tier

1. A kept agent note is written to `local/memory/provisional/<role>/<id>.md` with frontmatter: role, mission, project, the taint of the span that wrote it, `kind` (`practice`, `pitfall`, `fact`, `preference`), `created`, `valid_until` (30 days by default), `uses: 0`.
2. Sources: the inside `note` tool, `propose_note` and `record_session_summary` from hosts, capture on contact, and the run note of [`specs/114`](114-a-methodical-scout-known-first-a-plan-from-the-gaps-the-role-in-every-call-and-a-written-synthesis.md) §6. The per-task and per-mission caps of today stay. The eviction of staged proposals past twenty stops: a provisional note leaves by expiry, not by crowding.
3. A provisional note is **never** part of the system prompt, the lessons block or the heuristics. It reaches an agent only as a retrieved passage (§4), inside the untrusted-data frame, labelled `unreviewed note by <role>, <date>`.
4. A note written by a span that had read untrusted content is stored with that taint and is not retrieved automatically. It is found only by an explicit search, and then with the label `from untrusted content`.
5. A role retrieves its own provisional notes and those of kind `fact`. It never retrieves another role's `practice`, `pitfall` or `preference` notes, as with lessons today.
6. Each time a note is retrieved and the span ends without an error or a failed gate, `uses` rises by one. A note with three uses lives another 90 days. A note never used expires at `valid_until` and moves to `provisional/expired/`, which is not indexed. Nothing is deleted.
7. `[memory] provisional = true|false` turns the tier off; with it off, notes stage as today.

## 3. Review promotes

1. The digest and `delphi learn list` order provisional notes by `uses`, then by age. The operator's choices are: keep as a lesson for the role, keep as a workspace heuristic, mark wrong, or leave.
2. `delphi learn keep` works with the daemon running, by the compare-and-swap the memory writers already use, and accepts notes from inside agents as well as hosts.
3. `mark wrong` moves the note to `provisional/rejected/` and remembers its fingerprint for 180 days, so the same note is not kept again.
4. The digest's daily limit becomes a config key (default 10).

## 4. A larger, steadier start context

1. Span-start retrieval no longer requires an empty pointer list. It runs for root spans and for a child span's first turn.
2. Budget: six passages and 4,000 characters by default (`[retrieval] start_passages`, `start_chars`), at most two per document.
3. Zones: those of today, plus the `research` zone of [`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md), findings (§5), and the role's provisional notes. At most two of the six may be provisional.
4. Each passage keeps its label; untrusted and unreviewed passages are framed as data, as retrieved text is today.
5. `kb_search` (inside) returns the passage text for each hit, within a per-call character budget, as 064 §4.1 says. `recall_memory` is unchanged.
6. The inside search refreshes the index when a source is newer than it, bounded as 109 §3 bounds the host's.

## 5. Findings: what an investigation concluded

1. A new tool, `record_finding`, for the investigator and the scout, and an MCP tool of the same name for hosts acting in those roles. Input: `question`; `conclusion`; `status` (`confirmed`, `likely`, `ruled_out`, `open`); `evidence[]`, each a pointer (file and lines, a commit, a command and its output's hash, a source id) with one line of what it shows; `ruled_out[]`, hypotheses rejected with the evidence against each; `how_verified`; optional `supersedes` (an earlier finding's id).
2. The daemon writes `missions/<id>/findings/<slug>.md` (or the project's `research/findings/`), `status: active`, `source_type: internal_generated`, kind `finding`, label `agent finding, not validated`. Evidence pointers into files are checked to resolve; one that does not is refused with the pointer named.
3. Findings are indexed in a zone `findings`, visible to every role of the project, and to other projects on the rule of 115.
4. A finding that names `supersedes` retires the earlier one from default search; the earlier file stays and links forward.
5. The investigator's and the scout's charters say: search findings first; end an investigation with `record_finding`, including when the answer is "not found" (`open`, with what was ruled out).
6. The tenacity retry of today is unchanged.

## 6. Consolidation

1. A pass that runs when the daemon has been idle five minutes, at most once a day, within a token cap (default 30,000), off by default (`[memory] consolidate = false`).
2. It reads a role's provisional notes, findings and episodes and **proposes**, never applies: merge these near-duplicates into one note; these two contradict; this note is covered by lesson L; this finding is superseded by that one.
3. Proposals go to the digest. A merge the operator accepts writes the merged note and retires its parts with links.
4. Without a model (`consolidate = "mechanical"`), the pass merges only exact and near-exact duplicates by fingerprint and proposes nothing else.
5. Harvested episodes get `valid_until` (180 days) and are superseded by a later episode with the same error fingerprint and a resolution.

## 7. Use feeds back

1. The read-count report of 080 is built, counting inside and host reads.
2. A tool `flag_wrong {pointer, why}` for every role and for hosts: it records a dispute against a passage, a claim, a finding or a note. A disputed item keeps its place in search and gains the label `disputed by <role>`; three disputes or one by the operator drop it from automatic retrieval until reviewed. Nothing is deleted or edited by a flag.
3. Ranking gains two small, bounded terms, both in config and both off at zero: a boost for items retrieved in spans that ended well, and a penalty for disputed items. Stored `confidence` joins as a third.
4. Agent and host searches that return nothing are logged with the console's.

## 8. The investigator can investigate

1. Host search while the investigator or the validator is adopted returns unvalidated text for the first rung of its ladder (`explore`), clearly labelled, and validated text only when it asks to verify.
2. Whether the investigator's manifest gains the read-only shell scope its three skills declare is the operator's decision (§12). Without it the skills are marked in the catalog as host-only.

## 9. Acceptance criteria

1. **AC-1:** a note made with the `note` tool in one mission is among the passages given to the same role at the start of a later mission on the same subject, labelled unreviewed, and is not in the system prompt.
2. **AC-2:** a note written by a span with untrusted taint is not retrieved automatically.
3. **AC-3:** role A's `practice` note is never retrieved for role B.
4. **AC-4:** a note retrieved three times in spans that ended well has its expiry extended; an unused note is not in the index after `valid_until`.
5. **AC-5:** a mission with a claim pointer in its handoff still receives start passages.
6. **AC-6:** a `record_finding` call writes an active, indexed file; a second investigation's `kb_search` on the same question returns it with its text; a pointer to a missing file is refused.
7. **AC-7:** `delphi learn keep` succeeds with the daemon running, for a note from an inside agent.
8. **AC-8:** the consolidation pass changes no file outside its proposals.
9. **AC-9:** an item flagged three times is absent from start passages and present in an explicit search with its label.

## 10. Verification plan

- Engine tests on the snapshot and the retrieved slice (AC-1 to AC-5), with a check that the lessons block is byte-identical with the tier on and off.
- Tool tests (AC-6, AC-9); CLI tests (AC-7); a file-watch test for AC-8.
- Red-team review of §2 and §4: a prompt-injection fixture in which a web page tells the agent to write a note, and the note must not be retrieved automatically (AC-2).
- An evaluation, not a gate: the retention probes of 050 ML6 on five tasks, before and after.

## 11. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| G0 | this spec | 1-2 |
| G1 | the provisional tier: files, sources, taint rule, role rule, expiry, uses | 10-12 |
| G2 | review: ordering by use, `learn keep` with the daemon up and for inside notes, mark wrong, the digest limit | 6-8 |
| G3 | start context: runs with pointers present and for child spans, the budget, the new zones | 6-8 |
| G4 | `kb_search` returns passages; the inside index refresh | 4-6 |
| G5 | `record_finding`, the findings zone, supersession, the charters | 8-10 |
| G6 | consolidation: the mechanical pass, then the model pass as proposals; episode expiry | 10-12 |
| G7 | read counts (080), `flag_wrong`, the ranking terms, misses logged | 8-10 |
| G8 | the investigator's search default; the catalog mark | 2-3 |
| | **Total** | **55-71** |

G3, G4 and G8 stand alone. G5 stands alone. G1 comes before G2, G6 and G7.

## 12. Authority and trust

- The tier lets agent-written text reach a later agent without review. The guards are: it is never instruction (§2.3), tainted notes are not retrieved automatically (§2.4), it is capped at two passages (§4.3), it expires, and one config key turns it off. The red-team reviews G1 and G3.
- No agent writes lessons, heuristics or governance. `flag_wrong` and consolidation change labels and make proposals; they delete and edit nothing.
- A shell scope for the investigator would widen what a role can do. This spec does not grant it.

## 13. Open questions (the operator decides)

1. Is the provisional tier on by default?
2. Should the investigator have a read-only shell scope inside the daemon, or stay read-only and investigate through hosts?
3. Start budget: six passages and 4,000 characters, or more?
