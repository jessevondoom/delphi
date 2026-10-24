---
id: mission-001-decisions
title: "Operator Decision Register (Council 001)"
author: orchestrator
timestamp: 2026-10-23T12:00:00Z
source_type: internal_generated
confidence: medium
status: approved
source_mission: 2026-10-23-foundational-design
tags: [decisions, gates, scope]
---

# Operator Decision Register

> ✅ **All 16 decisions ratified by the operator on 2026-10-23** ("your recommendations all seem reasonable"). Every one was applied to the specs, config template, roadmap and heuristics. To revisit a decision, change its Status line and list the affected files.
>
> 🔁 **Gemini review (Council 001, Round 2):** four decisions were revised by the operator: D-03, D-05, D-11 and D-14. D-04 was amended. See the council file's Gemini section.

These are the decisions that belong to **the operator (the operator)**: value trade-offs, scope, and risk appetite that agents shouldn't settle on their own. Everything Council 001 could fix without such a choice has already been applied to the specs (see [`councils/001-spec-suite-red-team.md`](councils/001-spec-suite-red-team.md)).

**How to use this file:**
- Mark a decision `RATIFIED: <option>` (or write your own option) and add the date.
- Agents may not build anything that depends on a decision still marked `OPEN`.
- The **Blocks** line lists what waits on each decision.

**Priority:**
- 🔴 Decide before Phase 1: D-03, D-05, D-06, D-12.
- 🟠 Decide before the phase that needs it: D-01, D-02, D-04, D-07, D-08, D-10, D-11.
- 🟡 Can wait: D-09, D-13, D-14, D-15, D-16.

---

## 🔴 D-03 — Workspace layout: where do Delphi's files live?
**Context:** The specs never say what `delphi init` does inside an existing project repo. If `memory/` (Zone 3, private) sits in the project root, it gets committed and pushed to the team, which is exactly the leak spec 006 says it prevents.
- **A. Everything under `.delphi/` in the project, gitignored except a team-shared subfolder.** Zone 1 lives in the binary plus `.delphi/base/`. Zone 2 lives in `.delphi/team/` (committed). Zone 3 lives in `.delphi/local/` (gitignored). The project root stays clean.
- **B. Zone 3 outside the repo** in `~/.delphi/workspaces/<hash>/`, with Zone 2 in `.delphi/` in the repo. Nothing private can ever be committed, but private state is harder to find and back up.
- **C. Delphi-as-its-own-repo** (today's layout). Delphi is a separate directory that points at project repos. This is simple for this repo, but awkward for using Delphi day to day on other code.

**Recommendation:** **A.** It's discoverable, one `.gitignore` line protects private state, and `doctor` can verify it.
**Blocks:** Phase 1 (`delphi init`, `doctor`), every path in specs 004–007.
**Status:** 🔁 REVISED 2026-10-23 after the Gemini review, operator's own option: **Delphi is an installed tool.** All of its state (all three zones, worktrees, DB, config) lives together in `$DELPHI_HOME/workspaces/<name>/`, and every project repo stays clean. This repo is the source the install and update process is built from (spec 006 §3.1). Supersedes A.

## 🔴 D-05 — v1 runtime topology: daemon or single process?
**Context:** Spec 005 rejected the simpler design. Council 001 found the daemon brings spawn races, version skew after updates, orphaned approvals, and backpressure handling (all now specified, but all still to be built and tested). Meanwhile, Phases 2–4 need a runtime before the Phase 5 daemon exists, so the plan as written means building a runtime and then rebuilding it.
- **A. Single process for v1.** the operator runs the runtime in-process and writes `events.jsonl`; `delphi narrator` is a read-only tail. Move to the daemon later; the event schema is the same, so the change is additive.
- **B. Minimal daemon in Phase 2.** Event log plus JSON-RPC, no TUI polish. This is the target architecture from day 1, built earlier.
- **C. Keep the roadmap as written.** The daemon arrives in Phase 5, and Phases 2–4 run on a temporary host.

**Recommendation:** **A.** What you lose (pausing from Narrator, builds surviving a closed terminal) are conveniences. HITL only needs the operator.
**Blocks:** Phase 2 onward.
**Status:** 🔁 REVISED 2026-10-23 after the Gemini review: **B — minimal daemon in Step 2.** Single process broke live Narrator streaming and meant building the runtime twice.

## 🔴 D-06 — v1 scope cuts
**Context:** Eight specs, nine agents, four provider dialects, and five terminal integrations, all before the first end-to-end loop runs. Each item below is independent; tick the ones you accept.
- [x] Defer **spec 007 Planes 1–2** (A2A delegation, `delphi://` tokens). Keep Plane 3 (Git plus ratified rule proposals).
- [x] Defer **spec 008 §5 DPO export** (see D-14).
- [x] Defer the **OpenAI and local/DeepSeek drivers**. v1 ships Claude and Gemini only.
- [x] Defer the **headless CLI harness drivers** (`claude`/`gemini`/`codex`). They bypass the IPR and have uneven permission bridges (spec 002 §5).
- [x] Defer **Gemini explicit `cachedContents`**; implicit caching only.
- [x] Reduce **multiplexer auto-spawn** to `tmux` plus manual attach.
- [x] Simplify **`delphi update` merges** for v1: overwrite unmodified files; for modified files, write `.upstream` next to them and prompt. Full diff3 comes later.

**Recommendation:** accept all seven. Each one is cut from the critical path, not abandoned; the amended specs stay as the design of record.
**Blocks:** roadmap re-sequencing (D-12).
**Status:** ✅ RATIFIED 2026-10-23: all seven cuts accepted. **Partly reversed by D-19:** CLI harness drivers and the OpenAI-compatible dialect pass are back in v1.

## 🔴 D-12 — Adopt the revised critical path?
**Context:** The current roadmap builds retrieval before any missions exist and coaching before there's anything to learn, and only reaches a usable loop at Phase 4–5.
- **A. Revised path** (assumes D-05 A and D-06):
  1. Schema validator, `doctor`, and the path guard.
  2. Claude driver with a single agent, `events.jsonl`, and HITL in the operator.
  3. Gemini driver and Builder in a sandboxed worktree.
  4. Council: in-memory barrier, both providers, deterministic Minority Report.
  5. Read-only Narrator tail and FTS5 knowledge-base search.
  6. *(Gated on real usage)* Daemon, vectors, coaching automation, `update`/`sync`.
  7. *(Deferred)* 007 Planes 1–2 and 008 §5.
- **B. Keep the current 6-phase roadmap.**

**Recommendation:** **A.** Something usable lands at step 2, and every later step is driven by real usage.
**Blocks:** the start of implementation.
**Status:** ✅ RATIFIED 2026-10-23: A — revised critical path, with a go/no-go review after Step 2

---

## 🟠 D-01 — Gate 1: Local embedding model for retrieval
**Context:** Vectors help only once there's a corpus where wording shifts between documents. BM25 with identifier-aware tokenization already handles exact rule-ID lookups. An ONNX model download breaks both the "100% offline init" claim and the ~10 MB static binary.
- **A. FTS5 BM25-only in v1.** Add vectors later behind an `Embedder` trait; the schema already records `embedding_model_id`.
- **B. Bundle a local model** (`bge-small` via `candle`, ~30–80 MB download).
- **C. API embeddings**, opt-in.

**Recommendation:** **A.** Reopen when there are more than 50 missions *and* there's a logged search miss on a terminology-shift query.
**Blocks:** spec 004 Part B, and the ExpeL deduplicator's similarity signal.
**Status:** ✅ RATIFIED 2026-10-23: A — FTS5 BM25-only; reopen at >50 missions **and** a logged terminology-shift miss

## 🟠 D-02 — Gate 2: Where deterministic helpers live
**Context:** The barrier, provenance gate, base-branch rubric loader, path guard, and DAG validator are what keep agents in bounds. As Python scripts in `skills/*/scripts/`, they could be edited by the very agents and syncs they constrain, and they'd make Python a runtime dependency.
- **A. Compile them into `delphi-core` as state-machine code**, not agent-invocable subcommands. User-authored scripts stay in `skills/custom/*/scripts/`, run in the sandbox, at the Amber tier.
- **B. `delphi internal <cmd>` subcommands** that agents call.
- **C. Keep the Python scripts.**

**Recommendation:** **A.** A barrier that an agent decides whether to call is not a barrier.
**Blocks:** Phase 1 (path guard, validator), Phase 4 (barrier).
**Status:** ✅ RATIFIED 2026-10-23: A — compiled into the runtime as state-machine code

## 🟠 D-04 — Execution sandbox mechanism
**Context:** A worktree is a working directory, not a sandbox: tests run with the operator's full privileges. Tier 0 "Green" execution is now conditional on an OS sandbox.
- **A. Native:** `sandbox-exec` profiles on macOS and `bubblewrap` on Linux. No network; writes allowed only to the worktree and a temp dir.
- **B. Containers** (Docker or Podman). Stronger isolation, but adds a heavy dependency and slows startup.
- **C. No sandbox in v1.** All execution is Amber (approved once per task).

**Recommendation:** **A**, with C as the automatic fallback when the sandbox is unavailable. Apple has deprecated `sandbox-exec` but it still works. Revisit if it's removed.
**Blocks:** Phase 3 of the revised path (Builder execution).
**Status:** ✅ RATIFIED 2026-10-23: A — `sandbox-exec` / `bubblewrap`, falling back to Amber-tier execution. **Amended after the Gemini review:** sandboxes deny writes to `<repo>/.git/**`, git runs only in `delphi-core`, and diffs touching host-executed files (`build.rs`, `.cargo/config*`, `.envrc`, …) get a Red badge at merge (spec 008 §4).

## 🟠 D-07 — v1 agent roster
**Context:** Nine agent manifests and 42 skills exist before one agent loop runs. Manifests are data, so adding agents later costs very little.
- **A. Four agents in v1:** Orchestrator, Architect, Builder, Red-Team. Strategist's scope audit becomes an Red-Team/Orchestrator rubric item until Strategist is enabled.
- **B. Five agents:** A plus Strategist, so Councils keep a dedicated scope critic.
- **C. All nine.**

**Recommendation:** **B.** Strategist's value only shows up in Councils, and Councils are a v1 feature. Scout, Investigator, Writer and Integrator wait until a mission needs them.
**Blocks:** Phase 2–4 test fixtures.
**Status:** ✅ RATIFIED 2026-10-23: B — Orchestrator, Architect, Builder, Red-Team, Strategist

## 🟠 D-08 — Taint tier for unverified agent output
**Context:** The lattice has no correct label for something an agent wrote without test evidence. `internal_empirical` overstates it (that label means "verified by a test"), and `federated_*` mislabels where it came from. Today the roadmap claims `internal_empirical`/`verified` for an unbuilt plan. As a stopgap, the specs stamp such content `federated_verified` (L2).
- **A. Add `internal_generated` as a sixth tier** between `federated_verified` and `internal_empirical`. It can be used by tools but is not promotable without a test or ratification.
- **B. Keep five tiers** and accept the L2 label.
- **C. Treat it as L0.** Very strict: this would block a lot of normal workflow.

**Recommendation:** **A.** Labels should say where content came from.
**Blocks:** Phase 1 provenance schema.
**Status:** ✅ RATIFIED 2026-10-23: A — add `internal_generated` (L3); the lattice now has six tiers

## 🟠 D-10 — Cost and concurrency defaults
**Context:** No spec had a spending cap, so the retry ladder could escalate indefinitely. The mechanism is now specified (a daemon-enforced `mission_budget_usd`, plus `max_parallel_agents` and per-watch wake caps); the values are yours to set.
- Per-mission budget default: **$__** (suggested: $5 for L1–L3 work, $20 for a Council).
- `max_parallel_agents` default: **4**.
- Behavior when the budget is hit: **halt and escalate** (specified), or **downscale to fast tiers and continue**.

**Recommendation:** $5/$20, 4 agents, halt and escalate.
**Blocks:** Phase 2 (the cost ledger enforces it from the first model call).
**Status:** ✅ RATIFIED 2026-10-23: $5 per mission, $20 per Council, 4 parallel agents, halt and escalate

## 🟠 D-11 — How autonomous is the learning loop?
**Context:** Council 001 changed coaching to stage proposals for changes to `heuristics.md`, `REVIEW.md`, and promoted scripts; you ratify them in one batch at `/retro`. Per-agent `lessons.md` is the remaining question.
- **A. `lessons.md` applies directly** when the lesson's computed taint is ≥ L3; otherwise it's staged.
- **B. Everything is staged**, including `lessons.md`. Safest, but more review work.
- **C. `lessons.md` always applies directly.**

**Recommendation:** **A.** It is what the specs now say.
**Blocks:** spec 004 coaching (late).
**Status:** 🔁 REVISED 2026-10-23 after the Gemini review: **B — stage everything, including `lessons.md`.** Also removes the ≥L3 off-by-one introduced when D-08 added a tier.

---

## 🟡 D-09 — Heuristic size caps
**Context:** `context/heuristics.md` already holds 7 rules in 5,203 bytes (about 740 bytes per rule). The 6 KB cap would stop it at around 8 rules, not 25, and every current rule exceeds the 350-character limit. `doctor` will fail until this is resolved.
- **A. Keep the 6 KB and 350-character caps and rewrite** the 7 rules tersely. Rationale moves to `context/domains/`.
- **B. Raise the byte cap to 10 KB** and the per-rule limit to 400 characters.
- **C. Cap tokens instead of bytes** (~2,500 tokens).

**Recommendation:** **A.** The cap exists to stop prose from inflating, and the current rules show that inflation already happening.
**Status:** ✅ RATIFIED 2026-10-23: A — keep the caps; the 7 rules were rewritten (2.5 KB, ≤241 characters each)

## 🟡 D-13 — Personal and life-planning work in v1?
**Context:** The README markets life planning, but worktrees, PRs and spec gates all assume a git code repo.
- **A. v1 is code-only;** life planning comes later.
- **B. Support non-git workspaces now:** no worktrees, and only Markdown writes to `missions/`.

**Recommendation:** **A.** Then update the README scope line.
**Status:** ✅ RATIFIED 2026-10-23: A — v1 is code-only (git repos)

## 🟡 D-14 — DPO trajectory export
**Context:** The pairs as originally designed were invalid (they compared answers to different prompts). Training other models on Claude or Gemini outputs, especially reasoning traces, also needs checking against each provider's terms.
- **A. Remove it from the specs.**
- **B. Defer it** using the same-prompt pair design now in spec 008 §5, after a terms review.

**Recommendation:** **B**, and treat it as low priority.
**Status:** 🔁 REVISED 2026-10-23 after the Gemini review: **A — removed** from spec 008 (now `specs/008-ambient-watch-and-rewind.md`).

## 🟡 D-15 — Platform support
**Context:** Unix sockets, `flock`, and the sandbox are POSIX-specific.
- **A. macOS and Linux only,** documented (Windows users can use WSL2).
- **B. Plan for native Windows support.**

**Recommendation:** **A.** It is now stated in SPEC.md and spec 006, pending ratification.
**Status:** ✅ RATIFIED 2026-10-23: A — macOS and Linux only

## 🟡 D-16 — Keep reasoning text in `events.jsonl`?
**Context:** Persisted thinking blocks help with replay, Narrator reconnects, and debugging. They also grow the logs and can capture sensitive content. (Some providers return only summarized or encrypted reasoning anyway.)
- **A. Persist completed thinking blocks** (redacted), with a retention limit (e.g. 30 days, then compacted).
- **B. Never persist thinking text;** keep only signatures and metadata.
- **C. Persist everything indefinitely.**

**Recommendation:** **A.**
**Status:** ✅ RATIFIED 2026-10-23: A — redacted thinking blocks kept 30 days, then compacted

## 🟢 D-17 — Canonical role names and per-install theme packs
**Context:** Governance, specs and manifests mixed behavior with mythological naming. One theme name meant both an agent and a runtime layer, and a theme choice would have meant rewriting every file.
**Decision:**
- Agents are referred to only by canonical role ID: `orchestrator`, `builder`, `architect`, `investigator`, `scout`, `strategist`, `red-team`, `writer`, `integrator`, plus `operator` for the human.
- The layers are `console`, `narrator`, `governance`, `adapter`, plus the federation `gateway`.
- Display names, epithets and voice come from a per-install theme pack, Greek mythology by default (`themes/greek/`). New themes are researched by Scout, mapped by Architect, checked by Red-Team, and ratified by the operator (`specs/001` §6).
**Status:** ✅ RATIFIED 2026-10-23 by the operator ("sounds good. go"); applied across all governance, specs, manifests, memory seeds, config and the knowledge base. Council 001 records keep their original names as history.

## 🟢 D-18 — Epic view, agents pane and usage ledger
**Context:** The narrator's first view was under-specified, and the "cost ledger" that budget enforcement relies on was never defined. Token efficiency will become a major focus over time, even if it is minor in v1.
**Decision:**
- Narrator view 1 is the **Epic**: the mission's task DAG as a graph of stories and tasks.
- A persistent **agents pane** shows each role's status, current task, provider, tier and resolved model, tokens by type (including cache reads), and spend against budget.
- A **usage ledger** (`specs/005` §2.4) records one row per model call, derived from `agent.usage` events. It feeds budget enforcement, the pane, `/usage`, and future efficiency analysis.
- The ledger and `/usage` ship in Step 2; the pane and the Epic view ship in Step 5.

**Status:** ✅ RATIFIED 2026-10-23 by the operator.

## 🟢 D-19 — Model access paths and authentication (all required in v1)
**Context:** The specs assumed pay-per-token API keys only. The operator noted these are the *least* common real configuration.
**Decision:** v1 supports four access paths (spec 009):
- **Direct API.**
- **Cloud platforms:** AWS Bedrock and Google Vertex AI.
- **Enterprise gateways:** native-format passthrough, plus an OpenAI-compatible compat mode with reduced fidelity.
- **Subscription CLIs** (`claude`, `gemini`, `codex`) in sandboxed harness mode.

Supporting rules:
- **Credentials:** credential sources are delegated wherever possible (AWS SDK chain, Google ADC, OAuth device-code SSO, exec credential helpers, the CLI's own login), and Delphi never stores or reuses another tool's credentials.
- **Endpoint policy:** endpoints are configured per model family, with ordered failover and an `allowed_endpoints` compliance pin.

**Effect on D-06:** reverses the deferral of CLI harness drivers (now v1, Step 3, sandboxed, with spec 009 §7 conditions) and pulls the OpenAI-compatible dialect pass into v1 (for gateway compat and the `codex` harness). The direct OpenAI API driver stays deferred.
**Status:** ✅ RATIFIED 2026-10-23 by the operator ("we need to include support for all 3 missing setups from v1, they aren't optional"). Spec 009 is `draft` and needs its own review.

## 🟢 D-20 — Per-operator memory, customization and two-level evolution
**Context:** The operator asked that agents evolve both Delphi's central processes and each operator's own customizations.
**Decision:** Four memory scopes (spec 004 §5):
- **release**, **team** and **workspace**, as before;
- a new private, cross-workspace **operator** scope (profile, personal heuristics and lessons, personal overrides).

Supporting rules:
- **Assembly:** each snapshot loads the scopes broadest first; more specific scopes win on *how to work*, and no scope can relax governance.
- **Routing:** each `/retro` proposal has a scope, defaulting to the narrowest; it broadens on evidence.
- **Upstream:** central improvements flow to Delphi's source repo as scrubbed, previewed contribution bundles (Red-tier outbound), reviewed through Delphi's own Council process, and return through ratified updates that merge rather than overwrite customizations.
- **Evidence:** `rule_stats` provides correlational effectiveness signals.

**Status:** ✅ RATIFIED 2026-10-23 by the operator. Spec text is `draft` (spec 004 §5, pending amendments 001-A1 and 006-A1) → Council 003. One open question goes to Council 003: whether operators may widen role permissions by explicit grant.

## 🟢 D-21 — Operator permission widening
**Context:** D-20 left open whether an operator may widen a role's permissions in their own overrides.
**Council 003 recommendation (unanimous after Round 2):** keep `default_access_scope` **narrow-only, with no grants**, but let overrides **attach** ratified custom or operator skills whose `required_permissions` fit within the role's built-in scope (001-A1). The Builder seat first proposed grants with limits, then conceded. Widening a non-builder role would let code change outside `IsolatedBuild` and the merge gates, and an operator-scope waiver would apply silently in every workspace. Code-changing needs are met through `@builder` and skills.
- **A. Narrow-only plus within-scope skill attachment** (recommended)
- **B. Allow a Red-tier `delphi customize grant`** with hard limits (Builder's original Round 1 position)

**Status:** ✅ RATIFIED 2026-10-23: **A**, narrow-only plus within-scope skill attachment.

## ✅ D-22 — Integrations and connectors through CLI harnesses
**Context:** the operator expects Delphi to use integrations, for example running `scout` through `claude` with the operator's Google Drive connector. Approved spec 009 §7 currently disables every non-Delphi MCP server and connector in harness mode. A connector runs inside the harness loop that Delphi can't see, which raises unseen untrusted content, exfiltration and credential-ownership risks.
- **A. Ratified per-workspace, per-role connector allowlist; read-only in v1; connector spans stamped $L_0$** (recommended; drafted as 009-A2)
- **B. Keep connectors disabled until Delphi has native connectors**

**Status:** ✅ resolved 2026-10-24 by Council 005 (`councils/005-verdict.md`): **Option B**, connectors through harnesses stay disabled in v1. Amendment 009-A2 was rejected. A native Delphi connector layer is the post-v1 path.

## ✅ Approvals
- **2026-10-23:** the operator approved `specs/001-governance-and-manifests.md` and `specs/006-installer-and-updater.md` after Council 002. Roadmap Step 1 may begin. Pending amendments 001-A1 and 006-A1 are *not* covered by this approval.
- **2026-10-23:** after Council 003, the operator approved:
  - `specs/009-model-access-and-auth.md` (now `approved`);
  - spec 005 §1.1, §2.1 socket auth, §2.4, §2.5 and §4;
  - spec 004 §5, plus its §2.1 and §3 amendments;
  - amendments 001-A1 and 006-A1 (now part of the approved specs 001 and 006);
  - D-21 (option A).
- **2026-10-23:** the operator ratified heuristic `[H-008]` (no author-family self-review), with the condition that it **never blocks** a Council when only one model family is available. It degrades to a `homogeneous` verdict instead, and as best practice its critics use a *different model within the same family* (another tier or version) than the author's.
- **2026-10-24:** under the operator's pre-authorization ("follow council recommendations and approve them"), Council 004's fixes were applied. Approved:
  - spec 005 §2.1–2.3 and §3;
  - spec 002 §1.2 and §2–4;
  - spec 003 §1.2, §2 and §3;
  - spec 004 §2 and §4.1;
  - amendments 001-A2 (`@project/**` read root, per-scope heuristics, protected approval fields) and 006-A2 (scope files, ratified specs).

  With these, every section roadmap Step 2 depends on is approved. Step 2's scope is narrowed to L1 and read-only L2.
- **2026-10-23:** the operator approved:
  - amendment **006-A3**: release signing moves from Step 1 to Step 5; dev builds are unsigned and `doctor` warns;
  - an editorial change to approved spec 005 §4 (the agents-pane example no longer names a theme character), required by the stricter canonical-name check;
  - drafting amendment 009-A2 (D-22) for Council review.
- **2026-10-24:** implementing Step 2 milestone 2 found that spec 005 §2.4 keyed `usage_ledger` on `seq` alone, but `seq` is per mission, so two missions would collide. Clarified in the spec: primary key `(mission_id, seq)`, plus an `applied` position table; `agent.usage` gains `provider` and `reservation_id`, and the `budget.reserve` / `budget.release` payloads are defined. **Ratified by the operator 2026-10-24** (an edit to approved §2.4; no behavior change beyond the fix).
- **2026-10-24:** the operator ratified D-26 (option A) and spec 002 amendment 002-A1 (§4.1 and §3.1 rule 5): roles ask for capabilities, a ranked model pool promotes the next eligible model when the primary is exhausted, a target model is replaced by a similar one automatically and by a weaker one only after confirmation, and exhausted models are re-probed after 15 minutes, doubling to 4 hours.

---

## ✅ D-23 — Native connectors through MCP (Slack, Google Drive, Google Calendar, Gmail, Zoom)
Follows D-22 (Council 005 rejected harness connectors). The operator asked for high-quality access to these five services, with Claude and Gemini first.

- **A. Native MCP connector layer in the daemon, ratified catalog, browser OAuth login, one tool pipeline** (recommended; drafted as `specs/010`)
- **B. Defer all connectors to post-v1**

**Status:** ✅ APPROVED 2026-10-24 by the operator: A, `specs/010` (after Council 006 rounds 1 and 2; provider facts in §7 stay unverified until probed, §9). Phasing as in the spec: Step 3 builds the prerequisites (oauth2, token store, data-driven tool registry: done), slice 1 is the host, client, login and Google (Drive, Calendar, Gmail), then Slack, then Zoom.

---

## ✅ D-24 — The OS sandbox is optional; approvals are the default control (Claude Code model)
**Context:** D-04 made Tier 0 "Green" execution conditional on an OS sandbox and spec 009 §7 made it mandatory for every harness. The operator's view (2026-10-24): running commands, reading and writing files, and even running the harnesses without sandbox protection is now the norm among coding agents, and the majority path is Claude Code's: approvals, not confinement. Sandboxing should be evaluated later, not required now.
- **A. Sandbox optional, off by default; approval cards and rules are the control** (chosen)
- **B. Sandbox required** (the old D-04 A / spec 009 §7.2)

**Decision (operator, 2026-10-24): A.** `[exec] sandbox = "off" | "prefer" | "required"`, default `off`. The sandbox code stays and is tested; `prefer` uses it when present (sandboxed commands are Green), `required` refuses to run anything without it. This is D-04's option C as the default, with A as an opt-in.
- **Commands (off):** every `run_command` asks (Amber, never auto-approved by a permission mode) unless a **ratified** `allow` rule matches, the built-in read-only list covers it (like Claude Code's: `ls`, `cat`, `git status` and friends, with every path inside the worktree and no writing flags), or "allow similar" was granted for the task. `deny` rules refuse outright and win over everything. Wrappers (`sh`, `env`, `sudo`, interpreters) never match an allow rule.
- **Things the sandbox used to guarantee and what replaces them:** `.git` hooks and config are snapshotted around every unsandboxed command and put back if changed (reported to the model and the operator); the command environment is an allowlist (no `AWS_*` or tokens); resource limits, own process group and timeout kill are applied either way; taint-gated execution (no commands from a span at L0 or L1) is unchanged; read-only commands that print vendored (`untrusted_paths`) files taint the span like `read_file`.
- **Accepted residual risk (same as Claude Code):** an approved command runs with the operator's privileges and can touch anything the operator can, including `$DELPHI_HOME` and the ratification lock files. The approval card is the boundary. Spec 005 §2.1 already defends the daemon socket against same-UID processes (token plus TTY challenge); tamper-evidence for `ratified.lock` against a same-UID writer (a keychain-held MAC) is a candidate hardening if we want it.
- **Harnesses:** see spec 009 §7B. The Claude CLI runs as the operator's own process with its own login and Delphi's approval bridge; this also removes the Keychain obstacle that blocked sandboxed harness runs.
- **Supersedes:** D-04's "Green only inside a sandbox" default and the "always sandboxed" rule in spec 009 §7 item 2. Amends specs 005 §3.1 (`/permissions` wording), 008 §4, 009 §7 and 002 §5 note.

## ✅ D-25 — The Narrator is a separate, read-only process (Council 007)
**Context:** Spec 005 §4 puts the Narrator in its own window; the question (Council 007, 2026-10-24) was whether it should be a separate `delphi narrator` process attached over the daemon's Unix socket, or a pane embedded in the console process. It must never be able to start work or approve anything.
- **A. Separate process, read-only by daemon enforcement** (chosen; unanimous among the Architect, Strategist and Red-Team seats)
- **B. A pane embedded in the console process** (no seat defended it: the guarantee would rest on code discipline in a process that holds the capability to approve)

**Decision (operator, 2026-10-24): A.** The Narrator (`delphi observe`, also `delphi narrator`) is its own process with a narrator-scope token; the daemon rejects every mutating call on that scope.

**Follow-ups the Council made conditions (done 2026-10-24: items 1 to 3 in `delphi-core`'s `server.rs` and the engine's `core_main.rs`, items 4 and 5 in spec 005 §2.1; item 3 was already the hub's behavior and is now tested on the narrator socket):**
1. **Role fixed at accept time, not by a client claim.** Today the role comes from the token presented in `hello`. Move to a dedicated narrator socket (mode 0600, in the 0700 run directory) whose listener fixes the scope, with no role in the handshake.
2. **Deny by construction.** The narrator socket's dispatcher should contain only read handlers (subscribe, snapshot, usage, observe, kb search), so a new mutating verb cannot be reachable by omission. A test must try every mutating verb and expect refusal.
3. **Bounded per-client queues with resync.** A stalled Narrator must not starve the daemon: a bounded queue, and on overflow drop the subscriber and force a snapshot resync (the Narrator already handles `resync_required`).
4. **Say what this does and does not stop.** It stops UI-level and accidental escalation, not a hostile process running as the same user (same-UID processes can reach both sockets). Spec 005 §2.1 should state it plainly.
5. **Redaction before the socket.** State in spec 005 that the Narrator sees what the console sees, with secrets redacted by the daemon before events leave it.

## ✅ D-26 — Roles ask for capabilities; the model pool promotes the next model when the primary is exhausted
**Context (operator, 2026-10-24):** Delphi should not be overly prescriptive about which model does what. If the main model runs out of tokens and a second is available, the second should become primary so the work continues. Today `context/model-capabilities.md` names models per role (Builder on Sonnet, Council on "Opus + Gemini Pro", fixed upscale hops), `routing_directive` carries a static `fallback_provider`, and spec 002 §3.1 rule 4 fails over only on `429`/`529`. Nothing says what happens when a model's quota, subscription window or budget is *exhausted* rather than briefly rate-limited. Council 005 and the Step 2 audit both ran with the Claude seat out of quota, so this already happens in practice.
- **A. Capability requests and a ranked, health-tracked model pool; exhaustion promotes the next eligible model** (recommended; drafted as spec 002 amendment 002-A1, §4.1)
- **B. Keep per-role model names and a static fallback hint** (the current docs)

**Decision (operator, 2026-10-24): A.**
1. **Roles request capabilities, not models.** A dispatch asks for a tier plus needs (tool-use reliability, long context, write tools with the approval bridge, a family other than the author's). Model names in docs and examples are defaults and illustrations; config decides.
2. **The pool is ranked and has runtime state.** `[routing.pool]` lists configured models in preference order. Each has a state: `available`, `cooling_down` (rate limit, with a retry time) or `exhausted` (quota, subscription window or budget used up, until a known reset or a probe succeeds). The primary is the first `available` eligible model for the request; "primary" is a runtime role, not a property of a model.
3. **Exhaustion promotes.** On exhaustion the model is marked, the next eligible model takes over at the next turn boundary, and the work continues. State carries over through the canonical `ContentBlock[]`, the worktree and the scratchpad (spec 002 §3.1 rules 2 and 3), never through the dead model's signed reasoning. Pending tool calls are completed or abandoned first, as the spec already requires.
4. **Targets, similar models, and confirmation instead of stopping (operator, 2026-10-24).** Each role has a *target model* (from `[routing.targets]` or the pool order) and Delphi tries to satisfy it. When the target is unavailable it substitutes the best **similar** model: one that meets every hard requirement (write tools and the approval bridge for a Builder, so a read-only harness such as `agy` is never promoted into a Builder, spec 009 §7B; enough context; a priced catalog entry) *and* is at least the target's tier. That substitution is automatic and only logged. If the only models left are **weaker** than the target (lower tier, shorter context), Delphi does not stop: it raises a warning card naming the target, the substitute and what is lost, and runs on the substitute only after the operator confirms (or waits for the target's reset if they decline). If no model meets the hard requirements, the task parks as `blocked: no_model` with reset times shown and resumes when one returns, since nothing can safely run.
5. **Preferences degrade, constraints do not.** `[H-007]`, `[H-008]` and "≥2 families" are preferences: with one family left they degrade to a different tier of that family, or a fresh-context pass of the same model, and the verdict carries `homogeneous` (already ratified for Councils). Safety rules (taint, approvals, budgets, the `[access]` endpoint policy) are never relaxed by promotion, and promotion can never select an endpoint outside `allowed_endpoints`.
6. **Visible and bounded.** Every promotion is an event (`model.exhausted`, `model.promoted`, and `model.degraded_confirm` for the warning card) shown in the console and Narrator and recorded in the usage ledger. Promotion is not a verification attempt in the retry ladder. A confirmed weaker substitute is flagged in the mission report; a promotion to a pricier tier still goes through budget reservation. Unattended runs (no operator to confirm) wait for the target's reset or the operator rather than assuming consent.
7. **Handoff cost.** Cross-provider promotion loses signed reasoning (downgraded to `<prior_reasoning_note>`), which can cost quality. Promotion therefore prefers a model whose reasoning is replayable from the failed one, then a different family.

**Operator answers (2026-10-24):** (1) no hard pins: track target models and try to satisfy them, but issue a warning and ask for confirmation instead of stopping when no model of similar capability is available (item 4); (2) the re-probe policy for an `exhausted` model with no known reset time is accepted: a capped probe after 15 minutes, doubling to 4 hours.

**Status:** ✅ APPROVED 2026-10-24 by the operator: A, with the answers above. Touches spec 002 §3.1 rule 4 and §4 (new §4.1, amendment 002-A1, unratified), `context/model-capabilities.md` §3 and §4, spec 005 (new event kinds, to add when implemented) and `config.example.toml`.

## ✅ D-27 — The MCP client is the official `rmcp` SDK, not an in-tree client
**Context (operator, 2026-10-24):** spec 010 §1 item 6 (Council 006) chose a minimal in-tree MCP client behind an `McpTransport` trait and scheduled a spike against a library. A survey of Rust agent crates found that every agent framework (Rig, swarms-rs, ADK-Rust, AutoAgents, goose) owns the agent loop, where Delphi's governance lives, so none is adoptable; the plumbing crates are. The operator prefers existing open source wherever Delphi is not the differentiator.
- **A. Adopt `rmcp` (the official SDK) behind a small `McpSession` trait; Delphi keeps the host list, redirect policy, error mapping and all policy code** (recommended)
- **B. Keep the in-tree client** (the previous decision)

**Decision (operator, 2026-10-24): A**, and the operator accepts the lack of a size cap on a plain JSON response (only the 60 s request timeout bounds it).
1. `delphi-adapter::mcp_rmcp::RmcpClient` replaces `McpClient`/`HttpTransport`/`McpTransport`; `ConnectorHost` takes a `SessionFactory` that returns a connected `McpSession`. The scripted test server became a scripted session, so the engine tests did not change.
2. Same contract cases pass against it: happy path over JSON and SSE, typed errors with no token in them, an old protocol refused, a host outside the list refused including through a redirect, and tool listing capped at 10 pages. It also initialized, listed tools and called a tool on two real public servers (DeepWiki, Cloudflare docs).
3. **Accepted differences:** no cap on a plain JSON response; `Retry-After` dropped; strict parsing (a server without `serverInfo.version`, or an image block without `mimeType`, fails the call). **Open:** the Slack, Google Drive, Calendar, Gmail and Zoom servers have not been run through the `live_server_smoke` test (it needs their credentials); do that before ratifying each connector.
4. **Follow-ups, not done:** report or fix upstream the JSON body cap and `Retry-After`; `cargo audit`/`cargo deny` in CI (rmcp adds about a dozen crates); consider a seccomp filter blocking `io_uring` in the Linux sandbox (from codex-rs); ACP stays parked until `gemini --acp` can sign in (spec 009 §7).

**Status:** ✅ APPROVED 2026-10-24 by the operator (amendment 010-A1, spec 010 §1 item 6).
