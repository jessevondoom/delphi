---
id: spec-113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture
title: "113: Host-led research: ingest, a record tool, and workers that can capture"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: draft   # written at the operator's request on 2026-11-14 ("make specs covering all 8, show me LOE and slices"), after an audit of the scout, the investigator and the knowledge base. Not approved. Nothing is built from it until the operator approves it.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and a read-only look at one live workspace; nothing was run
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, hosts, mcp, scout, investigator, library]
---

# Spec 113: Host-led research

> **Status: draft, 2026-11-14.** Second of eight specs (112 to 119) from the knowledge-loop audit.
> **Extends:** [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) (agents run in the host first), [`specs/068`](068-research-capture-and-summary-pages.md) (capture and summary pages), [`specs/106`](106-compact-packs-and-role-subagents-a-charter-excerpt-a-hash-check-and-a-worker-file-for-every-role.md) (worker files), [`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md) (the research record).

---

## 1. Scope and objectives

| # | Observation (2026-11-14) |
| :-- | :-- |
| E-1 | In the audited workspace, the research that exists was done by host sessions acting as the scout with the host's own tools (web, and connectors to internal systems). It was saved as Markdown in project `research/` folders (about fifty files across eight projects) and as 58 imported summaries. Delphi's own runner had produced no claim. |
| E-2 | Those files are indexed as untrusted project text. They are not sources, they yield no claims, and a page cannot gather them. 48 of the 58 summaries list no source. |
| E-3 | The scout's and the investigator's charters tell a host agent to use `propose_kb_entry` and `record_session_summary`, and the scout's source ladder includes Delphi's research endpoint. The worker files (`ecosystem/claude-code/delphi/agents/scout-worker.md`, `investigator-worker.md`) list neither those tools nor `request_research`. A worker can read and cannot record. |
| E-4 | `propose_kb_entry` takes single claims. There is no call by which a host hands over a finished piece of research with its question, its sources and what it searched. |

Objective: research done in a host is recorded as fully as research done by the daemon: a topic record, sources, claims with quotes, and a log of what was searched.

### 1.1 Non-goals

- Delphi does not gain connectors. What a host read through its own connector is recorded as the host reports it and is never fetched by the daemon.
- No trust is raised. Everything a host hands over is `noted` and untrusted until the rules of [`specs/100`](100-confidence-ladder-substantiated-vetted-and-an-audit-that-rolls-back.md) say otherwise.
- Sharing across projects is [`specs/115`](115-research-recall-across-projects.md).

### 1.2 Simpler alternative considered

*Tell hosts to call `propose_kb_entry` once per claim.* Rejected: the question, the search trail and the link between claims are lost, the per-session caps are reached by one report, and files already on disk stay outside. Kept: `record_research` stores each claim by the same path `propose_kb_entry` uses.

---

## 2. `record_research`: a host hands over a piece of research

1. A new MCP tool. Input: `question`; `answer` (Markdown, at most 24 KiB); `sources[]`, each with a `locator` (a URL, or a description for a source that has no public address), `title`, `retrieved`, `kind` (`web`, `internal`, `file`, `person`), and `private: true|false`; `claims[]`, each with `text`, `quote`, and the index of its source; `searches[]`, each with where it was run and the words used; `gaps[]`; optional `topic_id` to add to an existing topic.
2. The daemon writes a research record (112 §2) with `runner: "host"` and the host and model as `requested_by`.
3. For a source with a public URL and `private: false`, the daemon fetches the page itself, under the trusted-list and egress rules that apply to its own runner, and checks each quote against the text. A quote it finds is an anchor; a quote it does not find is kept as `host-reported, not found`. This is the rule `propose_kb_entry` already follows.
4. A source marked `private`, or of kind `internal`, `file` or `person`, is stored as a private source record: locator, title and date only. It is never fetched. Its claims are stored with scope private to the project (or to the workspace when there is no project) and carry `host-reported`. They never count as an origin for corroboration.
5. A session under the private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) may call the tool; the daemon then fetches nothing for that call and marks every source private.
6. Limits: 40 claims and 40 sources per call, 10 calls per session per hour. Past a limit the call is refused with the limit named.
7. The reply gives the topic id, how many quotes were found, and the line: "not validated".

## 3. Ingest: files that already exist

1. `delphi research ingest <path>...` reads Markdown research files and imported summaries and writes, for each, a topic record with `runner: "ingest"`, the file as a private source (kind `file`, with its hash), and the file's body as `answer.md` by reference, not by copy.
2. It extracts claims from the file with the extractor `delphi kb extract` already uses, at most 20 per file, each anchored to a line range of the file.
3. URLs listed in the file become source pointers. With `--fetch`, public ones are captured and quotes that appear on the fetched page gain a second anchor there. Without it, nothing is fetched.
4. A file whose content hash is already recorded is skipped. A changed file adds a run to its topic.
5. The command prints what it would do and writes nothing without `--apply`. `delphi research ingest --all` walks every registered project's `research/` folder and the workspace's summaries.
6. A summary that lists no source is ingested and flagged `no sources listed` in its record and in search results.

## 4. Workers that can capture

1. The scout and investigator worker files gain `request_research`, `research_status`, `research_result`, `record_research`, `propose_kb_entry`, `propose_note` and `record_session_summary`.
2. A test reads every role's charter and its worker file and fails when the charter names a Delphi tool the worker's list lacks.
3. The scout's charter says, in one line each: search what Delphi holds first; do the research with the host's tools; hand it over with `record_research` before answering.
4. The session-end hook of [`specs/105`](105-session-hooks-for-hosts-a-brief-at-start-mention-routing-at-the-prompt-and-a-marker-at-the-end.md) notes when a session wrote a file under a project's `research/` folder and made no `record_research` call, and the next session's brief says so with the ingest command.

## 5. Acceptance criteria

1. **AC-1:** a `record_research` call with two public sources and four claims leaves a topic record, two library sources and four library claims; quotes present on the pages are anchors.
2. **AC-2:** a source marked private is not fetched (the test transport sees no request) and its claims do not raise an origin count.
3. **AC-3:** under the private-read mark the call makes no network request.
4. **AC-4:** `delphi research ingest <file>` without `--apply` changes nothing; with it, the file is a topic with claims anchored to line ranges; a second run on the unchanged file does nothing.
5. **AC-5:** the charter-and-worker test fails on today's worker files and passes after §4.1.
6. **AC-6:** the 41st claim in one call is refused with the limit named.

## 6. Verification plan

- Tool tests against the fake web for AC-1 to AC-3 and AC-6; a schema for the tool's input, checked by the docs test.
- Ingest tests on a fixture project (AC-4), including a summary with no sources.
- The worker test of §4.2 (AC-5).
- One live check: a host session researches a small public topic and records it; the topic is found by `search_kb` in a second session. Recorded in the tracker as observed or not.

## 7. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| H0 | this spec | 1-2 |
| H1 | `record_research`: the tool, the record, public quotes checked, limits | 10-12 |
| H2 | private and internal sources, the private-read mark | 4-6 |
| H3 | `delphi research ingest`: topic, private source, hash skip, dry run | 8-10 |
| H4 | claim extraction with line anchors; `--fetch`; the no-sources flag | 8-10 |
| H5 | worker files, the charter-and-worker test, the charter lines | 3-4 |
| H6 | the session-end note and the next brief | 2-3 |
| | **Total** | **36-47** |

Depends on 112 R1 and R2.

## 8. Authority and trust

- The tool stores text; it does not act. Everything it stores is labelled as coming from a host and not validated.
- The daemon fetches only public addresses the caller named, under its own rules. A host cannot use the tool to make the daemon reach an address the daemon's runner could not reach.
- Internal material is never copied in bulk: a private source is a pointer, and a claim is at most a sentence with a quote. The privacy identifiers of the workspace config are applied to what is stored.

## 9. Open questions (the operator decides)

1. Should `delphi research ingest --all --apply` be run once over the existing projects as part of this work?
2. Should a private claim's text be searchable from other projects ([`specs/115`](115-research-recall-across-projects.md)), or stay inside its project even when research is shared?
