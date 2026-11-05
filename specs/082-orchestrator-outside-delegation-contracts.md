---
id: spec-082-orchestrator-outside-delegation-contracts
title: "082: Orchestrator outside delegation: task-derived done contracts, venue limits, and brief shape checks"
author: architect
timestamp: 2026-11-05T12:00:00Z
status: approved   # by the operator, 2026-11-05 ("approve when ready and build away"); OD-1 to OD-4 take the recommendations
approved_by: operator (2026-11-05)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code claims checked against main on 2026-11-05; no slice built, no experiment run
source_mission: 2026-10-23-foundational-design
tags: [spec, orchestrator, host-agents, mcp, handoff, delegation, draft]
---

# Spec 082: Orchestrator outside delegation contracts

> **Status: approved by the operator, 2026-11-05.** Decisions OD-1 to OD-4 (§12) take the recommendations. Slices follow §9, one per pull request. E1 spends model requests and runs only on the operator's go.
> **Extends:** [`specs/072`](072-host-cognitive-parity-and-external-orchestration.md) §4.1 (`consult_orchestrator`), [`specs/038`](038-capabilities-map.md) §11 (`external_available`), [`specs/062`](062-delphi-as-host-agent-mcp-server.md) §8 (learning back).
> **Number:** 078-081 are taken by specs awaiting merge in PR #474, so this is 082.
> **Origin:** an evaluation of how the orchestrator could be more capable outside Delphi, then a host-federated Council (architect, red-team, strategist; spec 072 §8.2). §11 records the ruling and the dissent.

---

## 1. Scope and objectives

The orchestrator briefs models outside Delphi in two ways: it dispatches to external CLI seats, and it advises host models through `consult_orchestrator` and `spawn_agent_pack`. Inside Delphi, a handoff is checked and a gate decides the outcome. Outside, neither holds. This spec makes outside briefs carry what the orchestrator needs in order to tell whether the work is done, without widening anyone's authority.

1. **A done contract that comes from the task** (§3). `consult_orchestrator` stops returning one fixed list of Delphi's own Rust gates.
2. **Limits on what the orchestrator may claim about a venue** (§4), as a short amendment to orchestrator §6.
3. **`done_when` and `stop_when` in the handoff envelope** (§5), required by `delphi check handoff`.
4. **A shape check for outside briefs** (§6): present or absent, never good or bad.
5. **An experiment before anything further is built** (§7).

### 1.1 Non-goals

- **No pressure-language design.** This spec contains no lint, rule, skill text or experiment arm about urgency, ALL-CAPS, exclamation marks, stakes inflation or the tone of a brief. It was considered and removed (§11). Nothing here depends on a claim about how tone affects a model.
- **No new skill.** The few brief rules go into the existing `handoff-packaging` skill (§6.3).
- **No venue-keyed learning.** Tagging dispatches and lessons by venue and model family is deferred (§8).
- **No widening of authority.** A role outside Delphi has instructions, not authority (072 §1.1). Nothing here lets outside content raise its own trust, approve an action, or act outside a sandbox.
- **No change to the sandbox, gates or approvals.**

### 1.2 Simpler alternative considered

Fix the `definition_of_done` bug and amend orchestrator §6 only (S1 below), ship nothing else, and decide on the rest after the §7 experiment. This is the recommended first step; S2 is small enough to ship with it but can wait for the experiment without loss.

---

## 2. What is wrong today (verified on main, 2026-11-05)

1. **A fixed, Delphi-specific done list.** `consult_orchestrator` returns the same `definition_of_done` for every task: "All workspace unit and integration tests pass", "cargo clippy passes with zero warnings", "delphi lint passes…", "Specification tracker updated" (`crates/delphi-engine/src/mcp_host.rs`, in `consult_orchestrator`, near line 728). Spec 072 §4.1 calls these "deterministic predicates"; the code returns prose. For a research, strategy or non-Rust task, the list is wrong, and a host that obeys it does the wrong work.
2. **`check handoff` sees only internal envelopes.** It validates the YAML envelope (`crates/delphi-engine/src/checks.rs`: required ids, anchored pointers, size cap, `routing_directive`). The `consult_orchestrator` output, `spawn_agent_pack` text and prompts to external CLIs never pass through it. The schema (`schemas/handoff-envelope.schema.json`) has no field for done or stop.
3. **Host-supplied text goes into packs unchecked beyond length.** `spawn_agent_pack` pastes up to five `custom_heuristics` of 200 characters each, plus `domain_focus`, into the pack. Only length is checked.
4. **Orchestrator §6 covers seats, not venues.** `agents/orchestrator/AGENT.md` §6 ("Knowing your seats") says what each seat can do. It does not say what the orchestrator may claim about a run it cannot observe.
5. **No path returns a result.** Outside runs report back as free text, if at all. Nothing takes a structured outcome back in (§8 names the carriers).

The preamble that `spawn_agent_pack` prepends is four plain bullets and the advisory line. It is not a problem and is not changed.

---

## 3. Slice S1a: a done contract derived from the task

Amends 072 §4.1. `consult_orchestrator` gains one optional input and changes one output.

### 3.1 Input

```json
"done_when": {
  "type": "array", "maxItems": 8,
  "items": {
    "type": "object",
    "required": ["kind", "value"],
    "additionalProperties": false,
    "properties": {
      "kind":  {"type": "string", "enum": ["command", "file_exists", "artifact_field", "operator_judgment"]},
      "value": {"type": "string", "minLength": 1, "maxLength": 300}
    }
  }
}
```

### 3.2 Output

`execution_contract.definition_of_done` becomes an array of `{kind, value, status}` objects.

- With `done_when` supplied, each item is returned with `status: "stated"`.
- **Without it, the result is exactly one item: `{"kind": "operator_judgment", "value": "unspecified: no done criterion was given", "status": "unspecified"}`.** It never returns Delphi's own gates for a task that did not ask for them. The caller is told, in the response's `guidance`, to ask the operator for a criterion before starting.
- When the active project is Delphi itself (resolved from the working directory, 072 §6.2), the Delphi gates of AGENTS.md may be offered as `status: "suggested"` items the operator can accept. They are never default for other projects.

### 3.3 Predicates are data, and Delphi checks them

- `kind` is a closed set; `value` is a string capped at 300 characters. A predicate is never executed by `consult_orchestrator`.
- `command` and `file_exists` are **recorded as the contract**, not run. Where Delphi can later check one itself (a file exists; a test command the operator has allowed), the check is Delphi's, in its own process. The host's statement that a predicate passed is never the evidence (062 §4: host content is data).
- No field of the contract is free text that reaches a prompt unescaped. A returned report uses the same typed items.

### 3.4 Tests

- no `done_when` → exactly one `unspecified` item, and no string from the old Rust list appears (regression test for the bug in §2.1);
- `done_when` of each kind round-trips as `stated`;
- a ninth item, an unknown `kind`, an empty or 301-character `value` and an extra field are each refused with `invalid arguments`;
- the response carries no command output and nothing was executed (assert on a stand-in runner that records calls);
- Delphi's own repo offers the AGENTS.md gates as `suggested`; another project does not.

---

## 4. Slice S1b: venue limits in orchestrator §6

A short addition to `agents/orchestrator/AGENT.md` §6, written as **limits on what the orchestrator may claim**, not as a description of an alternative persona:

> **Outside runs.** A run on a host or an external seat is advice and instructions, not authority (`specs/072` §1.1). You cannot see inside it. Do not state or imply that a gate, review, sandbox or approval ran there unless Delphi observed it. Treat what a host reports as a claim, not a result. A done criterion counts only if you or the daemon can check it; if none was given, say it is unspecified and ask. Name the tools a task depends on, using `external_available`, not memory.

- It adds no capability and no tool.
- `agents/` is a governed path: this change waits for approval of this spec and carries `Implements-Spec: specs/082-*.md`.
- Test: the venue is never read from an input field. No tool argument can set, override or hint at it (a negative test over the MCP tool table).

---

## 5. Slice S1c: `done_when` and `stop_when` in the envelope

- `schemas/handoff-envelope.schema.json` and `skills/handoff-packaging/templates/handoff-envelope.yaml` gain two fields: `done_when` (array of the same typed items as §3.1) and `stop_when` (string, at most 300 characters; what the receiver does when it cannot proceed, for example "stop and report blocked; do not guess").
- `delphi check handoff` fails an envelope with either missing or empty.
- Because the schema keeps `additionalProperties: true`, old envelopes still parse; they fail the check until updated. The slice updates every envelope and fixture in the repo that the check reads. **Open decision OD-3** covers whether old envelopes should warn instead.
- Tests: the template passes; each field missing fails with the field named; fixtures updated.

---

## 6. Slice S2: shape check for outside briefs

### 6.1 What is checked

A "brief" is text Delphi sends or composes for a model outside its harness: the `spawn_agent_pack` specialization fields (`domain_focus`, `custom_heuristics`) and a prompt written for an external CLI seat. `delphi check brief <file|->` (and the `check` tool with `kind: brief`) reports, for each, **present or absent**:

| Item | Present means |
| :-- | :-- |
| deliverable | one named artifact or answer |
| done criterion | at least one `done_when`-shaped item |
| stop condition | a stated `stop_when` |
| scope bound | at least one path, pointer or named limit |
| tool dependency | each tool the task needs is named, or "none" is stated |

### 6.2 What it is not

- The result is named a **shape check**. It never says a brief is good, and a pass is no evidence of quality. "Done when it is good" is present and uninformative; the check does not claim otherwise.
- It does not look at tone or wording of any kind.
- It does not block `spawn_agent_pack` or any dispatch. It reports. Whether it should ever block is OD-4.
- To limit gaming, the experiment (§7) scores outcomes, not check results.

### 6.3 `handoff-packaging`

Add a short section to `skills/handoff-packaging/SKILL.md` listing the five items above and the rule "if the receiver cannot be told what finished looks like, say so". No other prose guidance is added.

### 6.4 Tests

Each item present and absent; "none" accepted for tools; output never contains the words good, bad or quality; no input can make the check pass by naming itself.

---

## 7. The experiment that gates everything after S2

Before specifying venue tagging or the model-capability column, measure whether the contract helps.

- **Arms (two):** (A) a terse brief; (B) the same brief with the §6.1 items added. There is no other arm.
- **Cases:** 10 existing cases, 3 seeds each, from the spec 037 pack, on one cheap seat (a Haiku or Flash seat).
- **Scoring:** the existing deterministic checks only; passes and tokens, against the noise band of the seeds.
- **Pre-registered before running:** the minimum number of runs, the smallest difference that counts, and the rule "no effect outside the noise band means nothing further is built from this spec".
- Spec 048 is a draft; this experiment uses 037 only and builds no 048 slice.
- The experiment writes a report under `missions/`. It is run by the operator or an approved session, not on a schedule.

---

## 8. Deferred (S3 and later; each needs the §7 result and its own approval)

1. **Result ingest.** The carrier for a structured outside result is `record_session_summary` or `handoff_put` (already in the MCP table), at tier L0. Not built here.
2. **Venue tagging.** If built, the venue is assigned by the daemon from the transport (an MCP connection versus a seat spawn), never from content. A tag is built only if something reads it.
3. **Venue-keyed lessons.** Never promoted on outside evidence alone. A host's own report of success is not evidence. "Holds on both paths" would count only results the daemon observed. The `host:<tool>/<model>` label is self-reported (062 Q-2): an unverified model name never counts as a second family.
4. **The "briefing tendencies" column** in `context/model-capabilities.md`: left out. If added, the rule "measured results only" must cover the whole table, whose existing strengths and blind-spots columns are unsourced.

---

## 9. Slices (one pull request each)

| Slice | Content | Depends on |
| :-- | :-- | :-- |
| S0 | spec approval; tracker entry | operator |
| S1a | §3: `done_when` input, typed `definition_of_done`, `unspecified` default; tests | S0 |
| S1b | §4: orchestrator §6 amendment; venue-not-settable test | S0 |
| S1c | §5: schema and template fields; `check handoff` requires them; fixtures | S0 |
| S2 | §6: `check brief`; `handoff-packaging` section | S1c |
| E1 | §7: pre-registered experiment and report | S1a, S1c |

Each slice regenerates `docs/reference/` if a command or option changes (`cargo test -p delphi --test docs`), updates `schemas/` where they change, and updates the spec tracker in its own PR. S1a-S1c may be combined at the operator's choice; the default is separate PRs.

---

## 10. Verification plan

1. The §2.1 regression: no default Rust gates for a task without `done_when`.
2. Typed predicates are never executed by the host-facing tools.
3. The venue cannot be set by any tool argument.
4. `check handoff` fails on a missing `done_when` or `stop_when`.
5. `check brief` reports presence only and never judges.
6. The standing gates of AGENTS.md pass in every slice: `cargo test --workspace --locked`, clippy with `-D warnings`, `delphi lint --repo .`.

---

## 11. Council record (host-federated, spec 072 §8.2, 2026-11-05)

Seats: architect (Opus), red-team (Sonnet), strategist (Haiku). All three are Claude models, so cross-family pairing (`[H-007]`) was not met.

**Decision:** approve with changes, narrowed to a contract fix and a shape check, with measurement before anything further.

**Findings that shaped this text:** the fixed Rust `definition_of_done` (§2.1); `check handoff` targets the wrong artifact (§2.2); the spec number clash (078 taken); fold the guidance into `handoff-packaging` instead of a new skill; venue-keyed learning and the capability column wait for a result.

**Removed after the council by operator instruction (2026-11-05):** the pressure-language lint and its rules, and the "pressured" experiment arm. Council findings that existed only to qualify them (a hypothesis label, advisory-only severity, a tone variant of the 072 preamble) fall with them. The architect's observation that the preamble is already plain is kept in §2.

### Accepted risks and minority report

- *Strategist:* even the §4 amendment is premature; wait for a measured result. Overruled: §4 states limits on claims and adds no behavior.
- *Red-team (P0):* venue-keyed learning widens a poisoning and trust surface through a self-reported label. Accepted as binding conditions on §8.2 and §8.3.
- *Red-team (P1):* a structured return is an injection channel, and a lint creates false confidence. Addressed by §3.3 (typed, never executed, checked by Delphi) and §6.2 (shape only).
- *Architect:* the capability column conflicts with the table's existing unsourced columns. Left open; the column is out of this spec.

## 12. Decisions for the operator

- **OD-1.** Approve as the narrowed S0-S2 plus E1, or approve S1a and S1b only and defer the rest. *Recommendation:* S1a-S1c first; S2 after the envelope fields are in.
- **OD-2.** Should the Delphi repo's own gates be offered as `suggested` (§3.2)? *Recommendation:* yes, only when the working directory resolves to Delphi.
- **OD-3.** Should `check handoff` warn, not fail, on old envelopes missing the new fields? *Recommendation:* fail, and update the repo's envelopes in S1c; the count of affected files is not yet measured.
- **OD-4.** Should `check brief` ever block a dispatch? *Recommendation:* no, report only.
