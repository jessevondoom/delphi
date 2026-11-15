---
id: spec-115-research-recall-across-projects
title: "115: Research recall across projects"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: approved   # by the operator, 2026-11-14, in a host session: "115: your suggestions are approved" (the default `workspace` and the private flag of section 8). Open question 2 was settled later the same day ("1. follow your direction"): claims from private and internal sources are shared under the three guards of section 2.3. Section 10 (2026-11-15) is an amendment written from the operator's words of that day; it takes effect when the operator merges its pull request.
approved_by: operator (2026-11-14)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and four live searches against an installed 0.6.1 daemon
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, projects, retrieval, scopes, privacy]
---

# Spec 115: Research recall across projects

> **Status: approved by the operator, 2026-11-14** (see the frontmatter). Fourth of eight specs (112 to 119) from the knowledge-loop audit.
> **Amends:** [`specs/058`](058-project-scopes-scaffolding-and-promotion.md) (project scopes: a project's files are visible to that project only). **Extends:** [`specs/109`](109-one-retrieval-path-for-host-search-a-fresh-index-and-passages-at-adoption.md), [`specs/064`](064-knowledge-retrieval-for-agents-and-mcp.md).

---

## 1. Scope and objectives

| # | Observation (2026-11-14) |
| :-- | :-- |
| E-1 | The index shows a project's files only to a session with that project's id (`kb.rs`, the project filter). Tests assert that another project gets nothing. |
| E-2 | A research run inside a project writes to that project and never to the shared index (`research.rs`, the destination rule). |
| E-3 | Project knowledge reaches the rest of the workspace only by promotion, one item at a time, on the operator's accept. |
| E-4 | On a live workspace with eight projects, a search from one project for subjects researched in two others returned nothing about them. |

Objective: an agent working in any project of a workspace can find research done in any other, labelled with where it came from, unless the operator has marked a project private.

### 1.1 Non-goals

- Only research is shared: topic records, answers, research files, and the library claims and sources that came from them. A project's specs, plans, context, work output, memory and staged notes stay inside it.
- Nothing crosses workspaces, bundles or the federation.
- No trust is raised: a shared item keeps its label and reads as untrusted project text from another project.

### 1.2 Simpler alternative considered

*Keep isolation and promote research automatically.* Rejected: promotion copies, so the copy goes stale when the research is continued, and it puts unreviewed text in the layer every role reads as the workspace's own. Reading in place with a label keeps one copy and keeps its origin visible.

---

## 2. The setting

1. Workspace config: `[projects] research_sharing = "project" | "workspace"`. `project` is the behaviour before this spec. The default is `workspace` (operator, 2026-11-14); see §8.
2. A project is private when its entry in the workspace's project list has `private = true` (`delphi project private <name> on|off`). A private project shares nothing and its sessions can still read shared research from others.
3. Claims that rest on private or internal sources are shared too (operator, 2026-11-14), under three guards:
   - **Label and weight.** The result reads `another project's research, from an internal source, host-reported`. Such a claim never counts as an origin for corroboration, in any project.
   - **The reader is marked.** A session that is given such a claim, by a search or at mission start, takes the private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) from that moment, as it would on reading a private source itself.
   - **The exits stay.** A private project shares none of them, and the workspace's privacy identifiers withhold any that contain one (§3.6).
   What crosses is the claim: its sentence, its quote and a pointer. The private source's own record and any kept text stay readable only inside its project.
4. Changing the setting or a project's flag takes effect at the next search; nothing is copied or deleted.

## 3. What a search returns

1. With sharing on, the retrieval filter admits, besides the session's own project, the research paths of every non-private project of the same workspace: `research/**` files and the `research` zone of [`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md).
2. Every such result carries `project: <display name>` and `shared: true`, and its label reads `another project's research, not checked` unless its claim status says more.
3. Results from the session's own project rank ahead of shared ones at equal score (a fixed small boost, in config).
4. Library claims with a project scope are visible across projects on the same rule; the claim pull at mission start may select them and says which project they came from.
5. `search_kb` and `kb_retrieve` take `scope: "project" | "workspace"`; the default follows the setting. `search_project` stays the session's project only. `search_memory` is unchanged.
6. The workspace's privacy identifiers are applied to shared results as they are to operator-scope rules: a result containing one is withheld and counted.
7. The vector search applies the project filter before it takes its nearest candidates, so a large project cannot crowd a small one's research out of the candidate set.

## 4. What the operator sees

1. The console's knowledge deck and the desktop app show the project on each research result and can narrow to one project.
2. `delphi kb search --project <name>` and `--all-projects`.
3. `delphi doctor` prints the setting and the private projects.

## 5. Acceptance criteria

1. **AC-1:** with `research_sharing = "workspace"`, a search from project B returns a research file of project A, with A's name and the shared label.
2. **AC-2:** with A private, the same search returns nothing of A's. With the setting `project`, likewise.
3. **AC-3:** a spec, a plan, a memory file and a staged note of project A are not returned to project B under any setting.
4. **AC-4:** a claim resting on a private source in A is returned to B with the internal-source label; B's session then carries the private-read mark; the claim adds nothing to any origin count; and the source's record and text are not readable from B.
5. **AC-5:** a result containing a privacy identifier is withheld from B and counted in the reply.
6. **AC-6:** with ten thousand passages in A and ten in B, a question answered only in B's research returns B's passage from the vector leg.

## 6. Verification plan

- Index tests on a fixture workspace with three projects, one private (AC-1 to AC-5). The existing isolation tests stay, run under the `project` setting; copies run under `workspace` with the new expectations.
- A recall test for AC-6.
- Red-team review of the filter before merge: the change is to who may read what.
- One live check on a real workspace after upgrade, recorded in the tracker.

## 7. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| X0 | this spec | 1-2 |
| X1 | the setting, the private flag, `delphi project private`, the doctor line | 3-4 |
| X2 | the retrieval filter and labels; internal-source claims labelled and the reader marked; identifiers withheld; own-project boost | 9-12 |
| X3 | the vector leg filters before it cuts | 4-6 |
| X4 | `scope` on `search_kb` and `kb_retrieve`; shared claims in the claim pull | 4-6 |
| X5 | the project on results in the console and the app; the CLI options | 3-4 |
| | **Total** | **24-34** |

X2 and X3 need no other spec. X4's claim part needs 112 R2.

## 8. Authority and trust: the decision the operator made

- This widens what a session may **read**. It gives no session a new action. The widening is the point of the spec and the operator approved it on 2026-11-14; an agent cannot change it, and the setting is among the config keys an agent may not write.
- A project folder may hold material the operator would not want read from another project's sessions (a host session in project B can show what it reads to its own model and tools). That is why the private flag exists, why a private source's record and text stay inside, and why a session that is given an internal-source claim is marked.
- The default is `workspace`, because all projects of one workspace belong to one operator (decided 2026-11-14). `project` remains as a setting for an operator who wants the earlier isolation.

## 9. Open questions (the operator decides)

1. *Decided 2026-11-14:* the default is `workspace`.
2. *Decided 2026-11-14:* claims from private and internal sources ([`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2.4) are shared, under the guards of §2.3.
3. Should any project be private from the start? Until the operator says so, none is.

## 10. Amendment (operator, 2026-11-15): a session with no project is given shared research

The operator's words, in a host session on 2026-11-15: "a session with no project should get full shared research." Until then, as built in X2, only a session inside a registered project was given another project's research; the spec did not require that, and the builder had asked before widening it. The operator approves this section by merging the pull request that adds it.

1. A session that is in no registered project (a host session outside every project root, a console session, or a mission in the workspace's default project) is given the research of every non-private project of the workspace, on the same rule as a project session (§3 items 1 to 6), wherever research is shared: a host's `search_kb` and `kb_retrieve`, an agent's `kb_search`, start passages and the claim pull at mission start.
2. The guards of §2 item 3 apply unchanged: the label and weight, the private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) taken at the moment such a session is given a claim that rests on a private or internal source, and the exits (a private project shares nothing; the workspace's privacy identifiers withhold).
3. With no own project there is no own-project boost (§3 item 3): every shared result ranks on its score alone.
4. `research_sharing = "project"` (§2 item 1) still turns sharing off for every session, with or without a project.
5. Built in X4, with a test for each entry point of item 1 and the mark.
