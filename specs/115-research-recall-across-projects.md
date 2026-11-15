---
id: spec-115-research-recall-across-projects
title: "115: Research recall across projects"
author: architect
timestamp: 2026-11-14T00:00:00Z
status: draft   # written at the operator's request on 2026-11-14 ("make specs covering all 8, show me LOE and slices"), after an audit of the scout, the investigator and the knowledge base. Not approved. Nothing is built from it until the operator approves it. It reverses a default of spec 058, which only the operator can decide.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (7db7ba36) and four live searches against an installed 0.6.1 daemon
source_mission: 2026-11-14-knowledge-loop
tags: [spec, research, projects, retrieval, scopes, privacy]
---

# Spec 115: Research recall across projects

> **Status: draft, 2026-11-14.** Fourth of eight specs (112 to 119) from the knowledge-loop audit.
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

1. Workspace config: `[projects] research_sharing = "project" | "workspace"`. `project` is today's behaviour. The proposed default is `workspace`; see §8.
2. A project is private when its entry in the workspace's project list has `private = true` (`delphi project private <name> on|off`). A private project shares nothing and its sessions can still read shared research from others.
3. A source or claim already marked private (a private source, a claim resting on one, anything recorded under the private-read mark of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md)) is not shared, whatever the setting, unless the operator settles open question 2 otherwise.
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
4. **AC-4:** a claim resting on a private source in A is not returned to B.
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
| X2 | the retrieval filter and labels; private sources and identifiers withheld; own-project boost | 8-10 |
| X3 | the vector leg filters before it cuts | 4-6 |
| X4 | `scope` on `search_kb` and `kb_retrieve`; shared claims in the claim pull | 4-6 |
| X5 | the project on results in the console and the app; the CLI options | 3-4 |
| | **Total** | **23-32** |

X2 and X3 need no other spec. X4's claim part needs 112 R2.

## 8. Authority and trust: the decision this spec asks for

- This widens what a session may **read**. It gives no session a new action. The widening is the point of the spec and is the operator's to approve; an agent cannot switch it on, and the setting is among the config keys an agent may not write.
- A project folder may hold material the operator would not want read from another project's sessions (a host session in project B can show what it reads to its own model and tools). That is why the private flag exists and why private sources stay inside.
- The proposed default is `workspace`, because all projects of one workspace belong to one operator. The conservative choice is to ship `project` and let the operator turn sharing on. The operator chooses.

## 9. Open questions (the operator decides)

1. Default: `workspace` (proposed) or `project`?
2. Should claims from private and internal sources ([`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §2.4) be shared across the operator's own projects after all? Much of the existing research rests on internal sources, so leaving them out shares little of it.
3. Should any project be private from the start?
