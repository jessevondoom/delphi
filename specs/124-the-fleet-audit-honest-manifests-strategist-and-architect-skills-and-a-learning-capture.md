---
id: spec-124-the-fleet-audit-honest-manifests-strategist-and-architect-skills-and-a-learning-capture
title: "124: The fleet audit: honest manifests, strategist and architect skills, and a learning capture"
author: architect
timestamp: 2026-11-17T00:00:00Z
status: approved   # by the operator, 2026-11-17, in a session, after seeing the build plan and this draft being written: "i saw your drafting of the steps for 124 so i'll approve it when ready. if it's based on this conversation consider it approved and run hard at it". Earlier the same day they answered the three questions: "1. repo-wide. include the first set of recommendations too. 2. go with your pick. 3. no. now go". The text was written from that conversation; the operator has not read it line by line. Open questions OD-1 to OD-4 of section 12 take their recommendations unless the operator says otherwise. It takes effect when the operator merges the pull request that carries it.
approved_by: operator (2026-11-17)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from a read-only audit of the 13 manifests and a web search made on 2026-11-17. The audit's findings about code were read by a subagent and are NOT re-checked here: the first step of each slice below is to check its own premise. The outside facts are from secondary web pages and are NOT VALIDATED (the scout returned no claim; see section 1, O-2).
source_mission: 2026-11-17-fleet-audit
tags: [spec, roles, skills, strategist, architect, learning, audit, lint]
---

# Spec 124: The fleet audit: honest manifests, strategist and architect skills, and a learning capture

> **Status: approved by the operator, 2026-11-17** (see the frontmatter), on the conversation this text is written from. The operator asked for a read-only audit of every role, then for the strategist and the architect to be made as capable as possible.
> **Amends, from its approval:** nothing by text. It uses [`specs/031`](031-layered-learning-and-training-mode.md) (capture, `/retro`, training), [`specs/063`](063-agent-fleet-behaviors-and-dynamic-orchestration.md) and [`specs/067`](067-fleet-expansion-administrator-assistant-advocate.md) (the roles), [`specs/026`](026-team-alignment-and-strategist-integration.md) §5 (`check_alignment`) and [`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) (findings) as they stand.

---

## 1. Scope and objectives

Three jobs, in this order of risk:

1. **Make every role's text true.** A manifest names tools in `execute` and charters promise work on top of them; where no tool exists the model is told it holds something it does not.
2. **Raise the strategist and the architect** with skills built on how experts in the two fields work, including a deterministic check that a strategy leads to actions, and a default step that produces unconventional options beside the conventional one.
3. **Make both roles feed the learning loop**, which today they do not.

Observations. "Checked" means read in a file on 2026-11-17 by the audit; "premise" means the first task of its slice is to check it again before changing anything.

| # | Observation | Status |
| :-- | :-- | :-- |
| O-1 | `execute` is a free string array in `schemas/agent-manifest.schema.json`. About 14 names appear to have no implementation: `scope-analyzer`, `critical-path-calc`, `prose-linter`, `example-verifier`, `doc-link-checker`, `fuzz-harness-scoped`, `trust-boundary-scanner`, `mermaid-validator`, `schema-validator`, `base-review-loader`, `webhook-dispatcher`, `external-api-client`, `gh-cli-scoped`, `mcp-bridge-inspector`. The strategist lists the first two and the architect the last two of the validators. Most real names appear only in the host capability map (`crates/delphi-engine/src/mcp_packs.rs`). | premise |
| O-2 | The scout returned no claim on 16 of 16 research requests across two sessions (daemon runner, then host-fed). In the host-fed runs the search log shows 18 of 20 results kept as leads and the 2 trusted-list pages "fetch_failed". Cause not established. | checked (log); cause open |
| O-3 | The messenger has no tool for `gh`, webhooks or MCP inspection, no shell scope, and `host_servable: false`. `specs/007` calls it the Gateway. The assistant promises calendar and triage with only `org-context`. The administrator holds three jobs, two without tools, and "resolve procedural deadlocks" contradicts the rule that deadlock is decided deterministically. | premise |
| O-4 | The three roles of spec 067 (administrator, assistant, advocate) are in none of the role tests in `crates/delphi-engine/src/workspace.rs` (tier, search default, workspace seed). | premise |
| O-5 | The validator's text says "validated / not validated / disproved" and "advisory only"; spec 100 adds "substantiated" and "vetted" and a `ratify` mode, which exist in code and in no `AGENT.md` or `SKILL.md`. | premise |
| O-6 | The red-team's §4 says "run the code" without a shell scope. The writer says every snippet is "tested or checked" without one, and nothing says `docs/reference/` is generated. The builder's never-merge rule is in a skill, not its manifest. | premise |
| O-7 | The architect still does "discovery until Scout and Investigator are enabled" (both ship) and nothing tells it to run `delphi specs compile` after a spec edit. The strategist has no search path and no source ladder, so a SWOT comes from model memory. | premise |
| O-8 | Five skills disagree with the manifests on `authorized_agents` (`fact-validation`, `critical-path-sequencing`, `work-breakdown-structures`, `tui-design`, `reading-labels`); 22 of 68 skills have no "Done when" section. | premise |
| O-9 | `memory/agents/strategist/lessons.md` has 0 active rules and `memory/agents/architect/lessons.md` has 1. Only the orchestrator proposes lessons (at `/retro`). The scout and investigator end with `record_finding`; the strategist and architect have no capture step. | checked |
| O-10 | `oblique-strategies` runs only on a deadlock; nothing makes divergent options a default step of `spec-authoring`. `competitive-swot-analysis` is on both the strategist's and the investigator's lists, and fits neither well. | checked |
| O-11 | Outside (NOT VALIDATED): strategy practice shares one move, diagnose, choose, then check the actions cohere (Rumelt's kernel; the Playing to Win cascade). Architecture practice has ADRs, quality-attribute scenarios, ATAM's utility tree, C4, fitness functions and bounded contexts. TRIZ resolves a contradiction with inventive principles. Two benchmarks (SPIN-Bench, STAR) report LLMs degrading as horizons and adversarial pressure grow; none tested business strategy. The full record is in the knowledge base under the topics `how-do-expert-business-strategists-think-core-532f3bc1`, `tried-and-true-software-and-systems-architecture-4582aecf` and `unconventional-and-creative-design-methods-for-6feb7dde`. | secondary sources |

Goals:

- **G-1** No `execute` name in a manifest names nothing; a lint keeps it so.
- **G-2** A role's charter promises only what its tools, scope and skills can do, or says plainly that it drafts and a person acts.
- **G-3** The strategist reasons from a diagnosis through a guiding policy to actions, and a script can say when an action leads to no policy.
- **G-4** The architect proposes, by default, one conventional design and at least one unconventional design, with the method named, and keeps the simpler-alternative rule.
- **G-5** Both roles stage a note when the operator corrects them, and date what they predict so `/retro` can check it.
- **G-6** Each new or changed skill has a trigger test, and the two flagship checks have deterministic training cases.

Non-goals: a new tool that acts outside the sandbox; a shell for any role (section 11); outbound messaging tools (section 11); changing a role's tier, turn budget or write scope except where a slice says so; theme changes; any change to what a gate decides.

## 2. Terms

- **Real name:** an `execute` name that a tool, a daemon capability or a host pack grants, found in code. A **host-only name** is granted by a host's capability map and not by the daemon; it stays but is marked so.
- **Kernel:** a strategy's diagnosis (what is in the way and why), guiding policy (the approach chosen) and coherent actions (steps that carry the policy out).
- **Trace:** the links action → policy → diagnosis. An **orphan action** has no policy; an **unfounded policy** has no diagnosis; a diagnosis with no **falsifier** (what would show it wrong) is flagged.
- **Unconventional option:** a design that differs from the conventional one by a named method (inversion, constraint removal, a TRIZ principle, morphological recombination, an analogy) and that the spec says how to test.
- **Capture:** staging a note for `/retro` (spec 031). It never loads into a prompt until the operator ratifies it.

## 3. Honest manifests (slice S1)

1. **Inventory.** For each name in every manifest's `execute`, find what grants it and write the table (name, role(s), granted by: daemon / host-only / nothing) to `docs/reference/execute-names.md`, generated by a command, not hand-written.
2. **Fix.** A name granted by nothing is removed, or replaced by the real tool that does the job, and the charter text that leaned on it is edited in the same slice (the role's own slice, section 4, carries the larger rewrites).
3. **Lint.** `delphi lint` fails a manifest whose `execute` has a name not in a known list kept in one place beside the capability map. Adding a name needs the tool in the same change.
4. **Do not widen.** Removing a name never grants anything. Where a host pack listed `shell` or `fetch` as an expected capability only because of a fictional name (the audit suspects `fuzz-harness-scoped`, `gh-cli-scoped`, `webhook-dispatcher`), that expectation goes too.

Acceptance: AC-1.

## 4. The roles, one by one (slices S2 and S3)

Slice S2 covers the roles never audited; S3 the rest. Each item is an edit to the role's `AGENT.md`, and its skills where named, plus catalog regeneration. Each item's first task is to check its premise.

| Role | Change |
| :-- | :-- |
| messenger | Shrink the charter to what exists: drafting external messages to `missions/**/comms/**`, for a person to send, behind the existing Red-tier card. Mark `gh`, webhook and MCP-inspection duties as not available. Real outbound tools are a separate spec (section 11). |
| assistant | Cut to duties it can do. `transcript-action-extraction` requires, for each action item, the verbatim line and the speaker. Add "Done when" and "Pitfalls" to its two skills. |
| administrator | Narrow to workspace administration (`inspect_system`, `propose_admin_action`). Sequencing moves to the strategist, which already owns `roadmap-sequencing`. Remove "resolve procedural deadlocks"; the parliamentarian either has a seat in `council.rs` or the duty is dropped (an open question, section 12). |
| advocate | Label every finding "simulated, not observed". Add an accessibility skill (reduce motion is a repo rule). Merge `persona-simulation` and `cognitive-walkthrough`. Each finding points at a screen or a snapshot. |
| administrator, assistant, advocate | Add them to the role tests of `workspace.rs` (tier, search default, seed). They also get the source-ladder section the others carry. |
| validator | Align the charter and `fact-validation` with spec 100's words and modes. Add a rewrite-to-stand-alone step and a search cap per fact, if spec 100 allows. |
| red-team | Findings carry a confidence field (low / medium / high) beside severity. "Run the code" becomes "ask the builder to run it" (the shell question is section 11). |
| writer | A generated-files rule: `docs/reference/` is regenerated, never edited. Example checking is handed to a role with a shell. A report is outlined before it is drafted. |
| builder | The never-merge rule enters the manifest. Whether an edit is syntax-checked automatically is checked, and if not, section 11 applies. |
| investigator | Drop `competitive-swot-analysis`. Reconcile `cognitive_profile: fast-triage` with its tier and turns. |
| orchestrator | Trim, and define "Rule of 25" in the manifest (a light pass: no responsibility is removed without the operator's say). |
| scout | Check the host `fetch` capability (the audit saw `web-search` without `daemon-fetch`). |
| architect | Remove the stale discovery duty. Add: run `delphi specs compile` after editing a spec. |
| strategist | Add the source ladder and a path to the scout, so a claim about the outside carries a source. |

Acceptance: AC-2.

## 5. A capture step for both roles (slice S4)

1. **Strategist.** Every forecast it writes carries a date, a tripwire (an observable that would show it wrong) and a label (`forecast`). When the operator corrects it or a Council overrules it, it stages a note with `propose_note` (spec 031 capture) saying what was wrong. At `/retro` the deck lists forecasts past their date so the operator can mark held, failed or open.
2. **Architect.** Every decision it records (an ADR, section 8) carries a reversal trigger and, where one exists, the check that guards it. Corrections and overrides stage a note the same way.
3. **Nothing loads unratified.** A note is staged only; ratification stays the operator's (spec 031 §2). Nothing here lets either role write to `lessons.md`, `context/heuristics.md` or any ratified file.
4. **Premise.** The first task is to read `propose_note`'s actual semantics, its taint rules and its limits (spec 031 L1/L2) and fit the wording to them.

The operator's answer, 2026-11-17: no auto-ratification of captured notes.

Acceptance: AC-3.

## 6. Strategy that leads to action (slice S5)

1. **Skills.** `strategy-kernel` (diagnosis, guiding policy, coherent actions on one page; a bad-strategy checklist: goal lists, buzzwords, restating the obvious; a falsifier for the diagnosis) and `strategy-cascade` (the choices from aspiration to management systems, then tactics, then actions, each action with an owner, a measure and a "stop doing"). Both authorize the strategist.
2. **Check.** A read-only tool `strategy_trace`, beside `check_alignment`, reads a strategy document in a fixed layout and reports orphan actions, unfounded policies, a diagnosis with no falsifier, and policies with no action. It decides nothing and blocks nothing; its findings are advice, like `check_alignment`'s (spec 026 §5). It makes no model call.
3. **Use.** `roadmap-sequencing` consumes a trace: a milestone with no action in it is flagged.
4. **Sources.** The skill text cites primary works (Rumelt, Lafley and Martin, Kim and Mauborgne, Helmer, Wardley) only after the operator has ratified those sources; until then it says they were taken from secondary summaries.

**Amendment, 2026-11-21 (review fixes V1, written by the builder, not read by the operator):** the fixed layout of item 2 is read more widely, and the two skills say so: a section heading may carry a number or more words and may be level 3 under a heading outside the layout; an item line may follow a list marker, a number or a task box, write its references after the colon, separate its text with a dash, write an id with leading zeros or a range of ids (`P1–P3`); a field counts only for the item it is written under. A line that starts with an id of its section and cannot be read is reported as `TRACE-DOC-02` (advice, like every trace finding), and a `text` wrapped whole in one code fence is read inside it. A labelled forecast (§5 item 1) may be a numbered item or a heading with a date or a colon, may wrap onto the next line, and keeps its `Tripwire:` across one blank line.

Acceptance: AC-4.

## 7. Divergent options by default (slice S6)

1. **Skill.** `divergent-options`: after the requirement is stated and before a design is chosen, produce at least three options: one conventional, one that removes a named constraint, and one by a named method from the set above (inversion, a TRIZ principle with the contradiction stated, morphological recombination, an analogy from another field). For each, one sentence of how it would be tested.
2. **`spec-authoring`.** Gains an "Options considered" section with the same three, kept beside "Simpler Alternative Considered" (the simpler-alternative rule is unchanged and still survives review by the builder, the strategist and the red-team).
3. **Lint.** A new spec (frontmatter `status: draft` or later, number above this spec's) without an "Options considered" section is a lint warning for one release, then an error. Older specs are exempt.
4. **`oblique-strategies`** stays for deadlocks and links to `divergent-options`.
5. **Guard.** An unconventional option that adds a layer of abstraction without a stated test is flagged by the checklist (the Labyrinth failure in the architect's own charter).

Acceptance: AC-5.

## 8. Architect rigor (slice S7)

`quality-scenarios`: a six-part scenario (source, stimulus, artifact, environment, response, measure) and a utility tree that ranks them, plus a short list of sensitivity points (one attribute) and tradeoff points (several) in a design. `adr-authoring`: the decision, its context, the options, the choice, the accepted drawbacks, a reversibility tag and a reversal trigger. Both authorize the architect and carry "Done when" and "Pitfalls".

Acceptance: AC-6.

## 9. Strategist breadth (slice S8)

`positioning-and-product-strategy` (positioning from competitive alternatives, unique attributes, value and the best-fit customer, before messaging; brand architecture; a vision to strategy to roadmap review that flags a roadmap that is a list of features); `advantage-analysis` (sources of durable advantage with evidence required for each; evolution of the components; eliminate, reduce, raise, create), which **replaces `competitive-swot-analysis`** (operator, 2026-11-17: "go with your pick"); `scenario-and-premortem` (at least three scenarios, each with a dated tripwire; a premortem on the chosen strategy). The retired skill leaves every manifest, the catalog and its tests.

Acceptance: AC-7.

## 10. Architect safeguards (slice S9)

`fitness-functions` (each invariant a spec states names the automated check that would fail when it erodes, and which role owns it); `spec-premortem` (before ratification, list how the design most likely fails); `boundary-design` (name bounded contexts, with a guard against applying it to a small, data-shaped domain).

Acceptance: AC-8.

## 11. Not built here, and why

| Item | Why not |
| :-- | :-- |
| A shell for the red-team (to run what it reviews) | A new capability; the operator's decision, not an agent's. |
| An automatic syntax or lint check on the builder's edits | Needs its own look at the write path and probably its own spec; S3 only checks whether one exists. |
| Outbound tools for the messenger (`gh`, webhooks, MCP inspection) | A new authority surface; its own spec, with Red-tier cards. |
| Model-graded judgement of strategy quality | No evidence it measures anything; the checks here are deterministic. |
| Any claim that the new skills raise quality | Unmeasured until S12 has been run and read by a person. |

## 12. Open questions for the operator

Answered 2026-11-17 in their own words: **(a)** the inventory is repo-wide and includes the first set of recommendations; **(b)** `competitive-swot-analysis` is replaced by `advantage-analysis`; **(c)** captured notes are never auto-ratified. Still open:

| # | Question | Recommendation |
| :-- | :-- | :-- |
| OD-1 | Does the administrator keep the parliamentarian duty (wire a seat into `council.rs`) or drop it? | Drop it; the orchestrator moderates and deadlock is deterministic. |
| OD-2 | The messenger: shrink to a drafting role now, or hold the charter until outbound tools are specified? | Shrink now. |
| OD-3 | Which primary works may the strategy and architecture skills cite once checked (Rumelt, Lafley and Martin, Helmer, Wardley, the SEI ATAM report)? | The operator ratifies a short list after the scout's pages are read. |
| OD-4 | Should the "Options considered" lint start as a warning for one release? | Yes. |

## 13. Authority and trust

- No slice adds a path that acts outside the sandbox, approves its own action or raises trust. `strategy_trace` and every skill are read-only advice; nothing they say authorizes anything.
- Removing an `execute` name never grants a capability. S1's lint can only refuse.
- A captured note is staged, tainted by its sources, and loads only after the operator ratifies it. The strategist and architect gain no write path to a ratified file.
- Edits to `agents/**`, `skills/**`, `schemas/`, `specs/` and the lint are governed or executable: each slice carries `Implements-Spec: specs/124-*.md` and a tracker update. No `.github/`, charter or rubric path is touched.
- Outside facts in skills are labelled as taken from secondary sources until OD-3 is answered.

## 14. Slices and pull requests

| Slice | What | Depends on |
| :-- | :-- | :-- |
| S1 | execute inventory, generated table, lint, removal of fictional names (section 3) | approval |
| S2 | the roles never audited, and the role tests (section 4, first four rows) | S1 |
| S3 | the other roles' manifests and skills (section 4, remaining rows) | S1 |
| S4 | capture step for both roles (section 5) | S2 |
| S5 | `strategy-kernel`, `strategy-cascade`, `strategy_trace` (section 6) | S1 |
| S6 | `divergent-options`, `spec-authoring`, the options lint (section 7) | S1 |
| S7 | `quality-scenarios`, `adr-authoring` (section 8) | S1 |
| S8 | the strategist's breadth skills, SWOT retired (section 9) | S1 |
| S9 | `fitness-functions`, `spec-premortem`, `boundary-design` (section 10) | S1 |
| S10 | skill lint: `authorized_agents` match manifests; "Done when" required for new skills (O-8) | S1 |
| S11 | the research result says why it is empty; read leads or widen the trusted list (O-2) | approval |
| S12 | trigger tests for every new skill; `delphi train` cases: orphan action caught, three distinct options produced, correction staged; a baseline-versus-candidate report a person reads | S4, S5, S6 |

Pull requests, same-spec trial permitting: A = S1 + S2 + S3; B = S4; C = S5; D = S6 + S7 + S9; E = S8 + S10; F = S11; G = S12. Each slice is its own commit with its own trailer and tracker line.

## 15. Acceptance

- **AC-1** The generated table lists every `execute` name; the lint fails a manifest with an unknown name and passes on main.
- **AC-2** For each role in section 4, a test or a catalog check holds the changed text; the three fleet roles are in the `workspace.rs` tests; `cargo test -p delphi --test docs` passes after regeneration.
- **AC-3** A scripted correction stages a note for each role, no note loads unratified, and a forecast without a date or tripwire is flagged by the skill's check.
- **AC-4** `strategy_trace` fails a planted orphan action, an unfounded policy and a diagnosis with no falsifier, and passes a clean trace, with no model call.
- **AC-5** A new spec without "Options considered" produces the lint warning; `spec-authoring`'s example carries three options.
- **AC-6, AC-7, AC-8** Each new skill has the frontmatter validated by `schemas/`, a trigger test that retrieves it from the task it was written for, and "Done when" and "Pitfalls".
- **AC-9** The retired `competitive-swot-analysis` is in no manifest, catalog or test.
- **AC-10** The S12 report exists and says what was and was not shown; nothing in the repo claims the skills improve quality before it does.
- **AC-11** All gates pass for every slice: `cargo test --workspace --locked`, clippy with `-D warnings`, and `delphi lint --repo .`.

## 16. Amendment (operator, 2026-11-17 and 2026-11-18): the answers to the open decisions

The operator was shown eleven open decisions after S1-S3 merged and answered four of them, in their own words: "2: your list is good. go. 3: follow your recommendation. 4: yes stabilize. 5: give the strategist access to research tools directly. follow your suggested build plan". The amendment takes effect when the operator merges the pull request that carries it. The text below was written after the words and has not been read by the operator.

1. **Primary sources (the answer to 2; OD-3).** The operator ratifies this list as the works the strategy and architecture skills may cite: Rumelt, *Good Strategy/Bad Strategy*; Lafley and Martin, *Playing to Win*; Helmer, *7 Powers*; Wardley, *Wardley Maps*; Ford, Parsons and Kua, *Building Evolutionary Architectures*; and the SEI report *ATAM: Method for Architecture Evaluation* (CMU/SEI-2000-TR-004). The list was suggested by the model from general knowledge and is NOT VALIDATED: the first task of slice S14 is to confirm, for each work, its author, title, year and a public address with the scout (`request_research`), and to drop a work it cannot confirm. A skill cites a work only for a claim the work makes and the builder has read in the work or in a source the scout returned; until then its "Sources" section stays as written. Citations are text in skills; nothing about trust changes.
2. **A path for the strategist's strategy files (the answer to 3).** The strategist's `write` scope gains `missions/**/strategy/**` and nothing else. It is a drafting path in the mission folder, beside `roadmap.md`: no ratified file, no `specs/`, no code. `strategy_trace` (spec 124 §6) keeps reading a path under the same guarded read as `check_alignment`; the skills say where a strategy file lives.
3. **Stabilize the timing-sensitive tests (the answer to 4).** Spec 125 carries this: which tests, why they fail under load, and the slices that fix them. It is not part of this spec.
4. **Research tools for the strategist (the answer to 5).** The strategist may ask for research and read it back: `request_research`, `research_status` and `research_result`, in a host and where the daemon offers them to a role. This amends [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md) §4 by adding one role to the roles that hold those tools; it edits none of 113's text. It is the least that gives the strategist a source ladder it can use: it does NOT grant `record_research`, `propose_kb_entry` or `record_session_summary` (the strategist hands facts to the scout, which records them), and it grants no fetch, search or write. The worker's tool list (`mcp_hosts::worker_tools`), its committed worker file and the ecosystem docs follow from the charter by the existing test (113 §4 item 2). What comes back is untrusted data (specs 062 and 119): a strategy claim built on it names its source and is marked not validated, and a result raises no one's trust.

### 16.1 Slices

| Slice | What | Depends on |
| :-- | :-- | :-- |
| S13 | the strategist's write path (item 2) and research tools (item 4): manifest, the strategy skills' "where a strategy file lives" line, `worker_tools`, the worker file and ecosystem docs regenerated, the tests | S8 merged (it edits the same manifest) |
| S14 | confirm the six works with the scout, then add citations to the strategy and architecture skills' "Sources" sections for the claims the works make; a work that cannot be confirmed is dropped | S8, S13 |

### 16.2 Acceptance

- **AC-12** A strategist worker's tool list holds exactly `request_research`, `research_status` and `research_result` beyond the read tools, and the charter-and-worker test passes; no other role's list changes.
- **AC-13** The strategist's `write` scope is `missions/**/roadmap.md`, `missions/**/councils/**` and `missions/**/strategy/**`; an edit outside them is still refused.
- **AC-14** Every work cited in a skill is on the list in item 1 and was confirmed by a scout result recorded in the pull request; a skill that cites none still carries the secondary-sources statement.

## 17. Amendment (written 2026-11-18, from S12's evidence): a daemon seat is shown its role's skills

This section is a proposal until the operator merges the pull request that carries it. The operator asked for the question below to be traced before going on ("run the one hour test before proceeding"), and the text that follows was written from the trace.

### 17.1 What S12 and the trace found (O-12)

Slice S12 (pull request #783) ran the architect on a small model: it wrote the three kinds of option **0 of 3** times unless the prompt told it to load the skill `divergent-options`, and **3 of 3** times when it did. A read-only trace of the code found why. Paths are under `crates/delphi-engine/src/` unless noted; none was run live, and the line numbers are those of main at e4014a32.

| Path by which a built-in skill's text can reach a daemon seat | Reaches it? |
| :-- | :-- |
| System prompt, `bundle_skills` slot (`engine.rs:1173`, `workspace.rs:336`) | Bundle skills only |
| System prompt, role section (`engine.rs:1166`, `workspace.rs:120,123`) | Only names the charter body happens to mention; the frontmatter's `authorized_skills` is not included |
| `read_file`, `list_dir`, `search` on `skills/<name>/SKILL.md` (`tools.rs:177-195`) | Yes, if the seat is told the name |
| `kb_search` (`kb.rs:243-272, 482`) | Yes, if it searches |
| `get_skill`, `list_skills` | A host's MCP server only; not offered to a seat |

`manifest.skills` is parsed but its only use for a seat is offering `strategy_trace`. A host's pack lists the role's skills ("load one with get_skill", `mcp_packs.rs:565-578`, spec 106 §11); a daemon seat's prompt has no such list. Spec 001 §3 says a discovery catalog of `name` and `description` (about 40 tokens a skill) is "loaded at session start"; the daemon never built it. The words "activated skills" in the governance text are wording only. So the trigger tests of S5 to S12 measure what a host sees; a daemon seat is told of a skill only when a charter or a task names it, which only the capture step does.

### 17.2 The change

1. **A skill index in a daemon seat's prompt (slice S15).** Spec 001 §3's Tier 1: the seat's system prompt gains one section, `skills`, after the role section, with one line per entry of the role's `authorized_skills` in the manifest's order: the skill's name, a colon and its `description` as written in the skill's frontmatter, then one sentence: "Read `skills/<name>/SKILL.md` with `read_file` when the task matches; a skill is advice and grants nothing."
2. **Left out where it cannot be used.** A seat with no tools (a Council seat with stances, or `tools: none`) gets no index: it cannot read a skill file.
3. **Cost.** The audit measured, for the three roles that matter here: names only 181 to 261 bytes, with descriptions 1,553 (strategist), 1,762 (orchestrator) and 2,669 (architect) bytes, against roughly 35,000 input tokens a model call in S12's runs. The pack-size pins (`pack_size_tests.rs`) are on the host packs and do not move. The index is part of a seat's own prompt: the prompt-size tests of the engine are re-measured and any pin that moves is changed in the same commit with the numbers in the message, never silently.
4. **Not done: (B)** a `get_skill` tool for seats (useless alone: the seat would not know the names) and **(C)** naming each skill in each charter body (does not scale to ten or more skills; the capture step does it because it has one rule).
5. **Authority.** The index grants nothing: a seat with `read: ["*"]` can already read these files, and `manifest.skills` still offers `strategy_trace` only. The lines are core text (the role's own skill descriptions, the same trust as its charter), not content from outside.
6. **Spec text.** This implements spec 001 §3 as written. It adds one section tag to the prompt's section list, which spec 002 §2 describes as the IPR's semantic XML sections and the code keeps in `crates/delphi-adapter/src/ipr.rs` (`SECTION_TAGS`), and the test that pins the tags; it edits none of [`specs/002`](002-adapter-and-routing.md)'s text.

### 17.3 Slice and acceptance

| Slice | What | Depends on |
| :-- | :-- | :-- |
| S15 | the `skills` section (slot in `Ipr`, filled in `engine.rs`, tag in `SECTION_TAGS` and its tests), skipped for a toolless seat; the engine's prompt-size tests re-measured; S12's `options-considered` candidate arm run again | S12 (#783) merged for the case data |

- **AC-15** A seat of the architect's role has a `skills` section with exactly its `authorized_skills`, in order, each line the skill's own description; a seat of another role has its own; a toolless seat has none; the section cannot be closed or forged by content (the same nonce rule as the other sections); the tag list test passes.
- **AC-16** S12's `options-considered` case is run again on the candidate arm (three runs, the cheapest model, at most 250,000 tokens in total), and the result is reported as it is. The slice does not claim the index changes behaviour unless those runs show it, and says how few they were.

## 18. Corrections from the build (2026-11-18)

Written from the reports of slices S4, S8, S10 and S12 after they merged. This section corrects the spec's text to what was built; it asks the operator for nothing new and takes effect when its pull request merges.

1. **S4 stages a note with the daemon's `note` tool, not `propose_note` (corrects §5 item 1).** `propose_note` is a host tool offered to the scout's and the investigator's workers only, and two existing tests (`host_guidance.rs` and `every_delphi_tool_a_charter_names_is_on_its_workers_list`) refuse a strategist or architect charter that names it. The charters name the in-daemon `note` tool (280 characters, `kind: pitfall`, three kept per task, never above trust level L3). A role that holds no staging tool (a Council seat without tools, a host worker) ends its answer with `Note to stage: …`. Pull request #778.
2. **"Nothing loads unratified" is true of lessons, heuristics and the system prompt, and not of retrieval (corrects §5 item 3).** Under [`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) an untainted staged note can be shown to a later span of the same role as a passage labelled `unreviewed note by <role>` for about 30 days. The charters say so and a test asserts both halves. The operator has not been asked to change spec 116; this spec does not.
3. **The `/retro` listing of forecasts past their date was not built (§5 item 1).** A forecast is a line in a document: nothing records it, and nothing lets the operator mark it held, failed or open. It needs a record format and an operator write path, so it needs its own design. Until then a forecast carries its date and tripwire (`TRACE-FOR-01` flags one without) and a person checks it.
4. **The list of skills a manifest holds changed by S8 and S10 (corrects §9 and §14).** `competitive-swot-analysis` is deleted; the strategist holds ten skills. S10 narrowed `fact-validation` (the validator only), `tui-design` and `competitive-swot-analysis` rather than widening a manifest, and allowlisted three skills no manifest lists (`reading-labels`, `work-breakdown-structures`, `critical-path-sequencing`) as on-demand. The "Done when" check is a warning for built-in skills, with a list of older skills exempt that only shrinks.
5. **A model arm was run once (corrects §11).** S12 ran 27 `delphi train run` runs on the cheapest tier (about 1.75 million tokens in): the capture step was followed 6 of 6 times with the change and 0 of 6 without; the architect wrote the three kinds of option 0 of 3 times unless told to load the skill. The cases are the model's own drafts (spec 031 §4), not curated. `dev/train/spec124/EVIDENCE.md` is the report and says what was not shown.

## 19. Amendment (2026-11-21, from the Stage 0 review of S11): the `empty` words tell the truth, and the two planner changes

S11 (commit 928d7185) also changed the query planner, which §14 did not say; the review found both changes and four places where `empty` said something untrue. This section records what was built to fix them; it asks the operator for nothing new and takes effect when its pull request merges. **The `empty` words:** three are added, making 21: `claims_not_stored` (claims were drawn and the library kept no record of them, so none can be cited; `empty` is now also present then), `search_failed` (every engine failed for a reason other than its key: an outage, a limit, an answer that is not results; it was reported as `no_results`, with the advice to ask a shorter question), and `no_search_run` (nothing was sent and nothing failed; it was also reported as `no_results`). When a feed was leads only because the session is under the private-read mark, `leads_unread` says the mark, not that the addresses are off the trusted list. When the answer is written from claims held before the run, `next` says so instead of advising a new question. `search_failed` is also a word of the coverage report's `reason` of [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §7.5 item 3 and of a search log line's `failed`; 098's list is otherwise unchanged.

**The two planner changes of S11** (`crates/delphi-engine/src/research_plan.rs`), now in the text: (1) a sentence-ending full stop is trimmed before a word is checked as a private name, so the last word of a sentence stays in the query (the privacy rules that this trim weakened are [`specs/066`](066-mcp-research-endpoint-and-role-identity.md) §5-6 and are fixed apart); (2) a question over 200 characters is cut where a sentence ends only when that keeps at least half of what fits **and the kept part holds a `?`**, so a cut never keeps the background and drops the ask; otherwise it is cut at the last word that fits. An abbreviation (`vs.`, `Dr.`, `etc.` and a short fixed list) ends no sentence.

**Smaller corrections in the same pull request:** a fetch refused for want of https is `not_https` in `why` (it was `other`); `leads_unread` counts every address not fetched, past the 40 that `host_leads` lists; `budget` also counts the check's own budget running out, and at `deep` its `next` asks for a narrower question instead of a deeper run; a page-reading reply that is not a claim list counts as `scout_call_failed`, not `no_claims_on_pages`; and the plain words no longer say pages were fetched when they were read from the library, that the question was "searched as one query" (the fixed plan sends up to three), or that sources were read when none were.
