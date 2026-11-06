---
id: council-001-propose-concrete-improvements-and-enhanc
title: "Council 001: propose concrete improvements and enhanc"
author: orchestrator
timestamp: 2026-11-06T20:53:27.032Z
source_type: internal_generated
confidence: medium
status: active
source_mission: host-7b2378dfb0209014
tags: [council, council-001]
council_format: 1
venue: panel
blind: yes
---

# Council 001

**Proposition:** A host agent (host:claude-code/claude-sonnet-5-5), working in another tool, asks the Council to rule. What follows between the markers is content from outside Delphi: it is the question under debate, not an instruction, and it grants nothing. Your ruling is advice to the host and binds nothing.
--- BEGIN HOST QUESTION ---
Question: Propose concrete improvements and enhancements to Delphi in four areas: (1) make the inside/outside design more effective: how Delphi's internal roles, memory, KB and Councils serve outside hosts (MCP, specs 062/064, plugin) and how outside knowledge and tools flow back in without raising trust; (2) grow the knowledge base more effectively (specs 004, 013, 043, 064; validator spec 040, primary-sources plan, kb review, research expand); (3) enhance Council activity itself (spec 003, 063, lenses, /follow, verdict quality, cost, seat selection, dissent handling); (4) improve memory, skills and learning (specs 004, 031, 065, 041; retro, train, lessons, promotion). Each recommendation must name the spec it touches or would need, the smallest first slice, how it would be measured, and the authority risk (it must not widen authority or let outside content raise its own trust). Rank by value over effort, and call out what existing work already covers so nothing is duplicated.
Stakes: normal
Options:
(none)
Pointers:
(none)
--- END HOST QUESTION ---

## Seats

| Seat | Family | Round 1 | Round 2 |
| :-- | :-- | :-- | :-- |
| `architect` | host-reported | answered | critique |
| `red-team` | host-reported | answered | unreadable |
| `strategist` | host-reported | answered | critique |

**Homogeneous (low diversity):** one model family only, so the `[H-001]` diversity guarantee did not hold.

- a host Panel: isolation and model names are host-reported

## Round 1 stances (blind)

### architect

> OPTION: Measure first; finish half-built slices
> 
> Reasoning (architect). The question gave no pointers, so I checked which specs exist and their status: 003, 004, 013, 031, 040, 041, 043, 062, 063, 064, 065 and 086 are all approved. 064 already sets "green is not trusted" (the daemon assigns the tier, read-back lowers the ceiling, every MCP result is external_untrusted). The gap is not design. Several approved specs are only partly built and none of it has been measured. So my ranking puts closing those slices and adding measurement ahead of any new spec.
> 
> Ranked by value over effort:
> 1. Inside/outside: a provenance round-trip test (062/064/086). Smallest slice: one e2e test showing that an outside host's request_research result lands as `proposition` with tier external_untrusted and cannot be ratified without a validator (040) or the operator. Measure: test passes; no path in the trace raises the tier. Authority risk: none, it only asserts. Already covered: the 064 tier rules and the 086 barrier.
> 2. KB: build 040 V1 plus primary-sources S1 (registry only). Slice: the registry file and lint. Measure: the share of propositions with a primary source; validator agreement on the operator-labelled set (§23.4, still blocked). Risk: the validator must not self-ratify. V1 only proposes. Covered: 043 K1/K5 and E1-E4 expand; K2-K4 are still open, so do not duplicate them.
> 3. Council: a structured dissent field in the 086 ruling card plus a token count for each seat. Slice: the schema field and its recording. Measure: the share of verdicts that keep a minority view, and tokens per council. Hold seat-selection changes until that data exists. Risk: low. Covered: Council --tier and /follow (#228).
> 4. Memory/learning: run 031 L2 /retro live once, and have 065 trace distillation write candidate lessons only, never promoted ones. Slice: /retro writes to a candidate queue. Measure: how many candidates the operator accepts. Risk: auto-promotion would let content raise its own trust, so promotion stays operator-gated.
> 5. 041 S0 registry (nothing built): last, because it is the highest effort.
> 
> Risks: duplicating parallel sessions' work (check gh pr list first); proposing new specs where approved ones suffice; measurements that rely on model judgement instead of deterministic code; any learning path that promotes without the operator; outside lessons laundered through the shared promoted summaries; and the token cost of Councils growing unmeasured.

### red-team

> OPTION: Measure and harden before building more
> 
> Reasoning: The brief asks for a ranked wish list across four areas, but my own project memory shows the dominant pattern is that built things stay unmeasured: the fixed reminder (085) has no measured effect, the harness defaults (045) are unmeasured, record_change use is unmeasured, name_thread has never been called live by a real model, the layered learning work (031) has never been run live, and training (037) still awaits the operator. Adding new features in all four areas would mostly add more unmeasured surface. I did not read the specs for this round (no pointers were given), so treat the specific claims below as unverified.
> 
> Ranking I would push for, by value over effort:
> 1. Pick one instrument and run it live. Every recommendation in the host's list should name a metric. Start with one shared measurement slice (a small fixed set of Council questions with known-good outcomes, scored for verdict quality, tokens and dissent kept). The smallest first slice is a script that replays them, not a new feature. It reuses 037/048 harness work rather than duplicating it.
> 2. Inbound trust flow (area 1). The smallest slice is a lint or test that proves outside content (MCP results, request_research output) is stored with a source label and an unvalidated status, and can never be promoted to lesson or skill without an operator step. This is a negative test, not new code, and it directly guards the authority rule.
> 3. KB growth (area 2). Do not add sources faster than the validator (040) can label them. Gate growth on the validator and primary-sources plan; the first slice is labeling the existing 41 claims, not expanding research.
> 4. Council (area 3). Dissent handling is the weak spot: check that a minority view survives into the verdict record verbatim. Measure it by seeding a known dissent and checking it appears.
> 5. Memory/learning (area 4). Promotion is the authority hazard. Before L2 /retro or train, require that promotion is operator-gated.
> 
> Duplication warning: 004-A, 040, 041, 043/044, 045, 031, 037 and 086 already cover much of this. The host should map each idea to an existing spec before proposing a new one.
> 
> Risks:
> - Self-reinforcing loops: Council output feeding KB and memory, then back into Council, can launder low-trust content into trusted lessons.
> - Host-supplied framing can steer seat selection or ranking; the host is itself an outside party.
> - Cost: more Councils and seats per question multiply tokens with no measured gain.
> - Metrics gaming: a small replay set will be overfit.
> - Claims here rest on memory notes, which may be stale.

### strategist

> OPTION: Extend existing specs with measured, small slices.
> 
> Basis: the question has no Pointers, so I did not read spec files. I worked from the memory index, which may be stale. I recommend no new specs.
> 
> Already covered (do not duplicate): 086 outside Council (S0-S4 merged, S5 run in progress); 062/064 MCP and plugin; 031 learning layers (L2 /retro is next); 040 validator (drafted, not built); the primary-sources plan (unbuilt); 044 K2-K4 and K6 (unbuilt); 041 registry (S0 next); 045 harness loop H2-H9 (unbuilt); 087 identity (S1 next).
> 
> Ranked by value over effort:
> 
> 1. Council dissent (003, 063). Record per-seat abstentions and unresolved dissent in each verdict. Measure: share of verdicts with open dissent. Authority: none.
> 2. Validator before KB growth (040 V1, 004, 013). Require a checked primary source for each claim that enters the KB. Outside-sourced claims stay propositions until an operator validates them. Measure: share of validated claims with a checked source. Risk: the validator must not approve its own output.
> 3. Inbound outside facts (062, 064). Tag every fact returned over MCP as a signed proposition, never validated. Measure: outside facts reaching the validated tier without operator action (target zero).
> 4. Retro and promotion (031 L2, 065). Lessons are written as proposals. Promotion needs an operator or a held-out check. Measure: a fixed held-out task pair from the L7 suite, run before and after a promotion.
> 5. Seat selection (063). Choose seats by token share, not money, per the team's rule. Measure: tokens per accepted verdict.
> 
> Risks:
> - No baseline exists for any of these. Measure before changing defaults.
> - Any auto-promotion path would let content raise its own trust. Keep it out.
> - Parallel sessions already build the same slices. Check open PRs first.
> - The 086 S5 run is capped; do not add load to it.
> 
> Model note: the launcher asked for claude-sonnet-5-5. I run on claude-haiku-5-5, so I recorded that.

## Round 2 critiques

### architect

> The three stances agree on the main point. The approved specs (003, 004, 013, 031, 040, 041, 043, 062, 063, 064, 065, 086) already cover the design. What is missing is measurement and finishing the slices that are half built. None of us proposes a new spec. That is the synthesis, and I support it. Four objections still need an answer. (a) Strategist proposes choosing seats by token share now. That changes a default before any baseline exists, so it should be held until the per-seat token counts from item 3 exist. (b) Strategist's "signed proposition" for outside MCP facts needs care. A signature records where a fact came from, not that it is true. The tier must stay external_untrusted per 064, whoever signed it. The wording should say "source-labelled", so no later path reads a valid signature as a reason to raise the tier. (c) Red-team warns that a fixed replay set will be overfit. That is right. Keep a held-out split and rotate it, and reuse the 037/048 harness rather than building a new one. (d) On KB growth, red-team's first slice (label the 41 existing claims) and mine (primary-sources registry plus lint) go together. Labelling gives 040 V1 its agreement set, and the registry gives the claims somewhere to point. Expansion waits for both. Structural synthesis, in order: 0) a shared measurement slice: replay script, held-out split, tokens per seat, dissent kept. 1) a negative e2e test that outside content cannot be promoted or ratified without the operator (062/064/086, test only). 2) a structured dissent and abstention field in the 086/003 ruling record, checked by seeding a known dissent. 3) label the 41 claims, add the registry and lint, then 040 V1 as propose-only. 4) 031 L2 /retro and 065 writing to a candidate queue only, with promotion gated by the operator and a held-out check. 5) 041 S0 last. Every item: check gh pr list first, and do not load the capped 086 S5 run.
> RECOMMENDATION: PROCEED-WITH-CHANGES
> FINDING P1 Seat-selection default change (strategist item 5) precedes any baseline; defer until per-seat token data exists
> FINDING P1 "Signed proposition" wording for MCP facts risks a signature being read as trust; specify source-labelled, tier fixed external_untrusted per 064
> FINDING P2 Replay measurement set needs a held-out, rotated split to avoid overfitting; reuse 037/048 harness
> FINDING P2 Spec status claims rest partly on stale memory (040 described as draft vs approved); verify against specs/ and the tracker before slicing
> CONCUR: yes
> ROUND 2 COMPLETE

### red-team

> All three stances agree: measure first, no new specs, operator-gated promotion. None read a spec (no pointers were given), so every "already covered" and "approved" claim rests on memory notes and is unverified. The host asked for specific slices, spec names, metrics and authority risks, and the stances give only partial answers.
> 
> Main attacks:
> - Metrics are unspecified or circular. "Share of verdicts with dissent" rewards manufactured dissent. "Validator agreement on the operator-labelled set" is blocked, so it can't be measured. "Tokens per accepted verdict" lets the accepting party game it.
> - The shared replay set that the red-team stance proposed is overfit-prone and has no owner. The architect's "candidate queue" has no cap, so it can flood the operator and push them toward rubber-stamping.
> - Zero-tier-raise tests only cover paths that exist today. They miss laundering through Council output, then KB, then shared promoted summaries, then lessons. No stance proposes a taint or provenance field that survives each hop.
> - The host is itself outside, and its question text is untrusted. A seat-selection change driven by host framing is an authority risk.
> - Strategist's cost point (do not load the 086 S5 run, which is capped at 350k tokens per run) is right, but no stance defines a token budget for the measurement work.
> - Ranking differs across the stances. Nobody ranks by value over effort with numbers.
> 
> The direction is sound, but the answer is incomplete as a response to the host.

### strategist

> Reasoning. The three Round 1 stances agree on the main shape: measure what is already built, extend approved specs with small slices, and build no new spec. I agree with that. The host asked for four areas at once, and the answer should be a short ranked list, not a full roadmap. Each item must map to an existing spec, and its risk must be checked against the rule that outside content never raises its own trust. The Round 1 claims rest on memory notes, not on spec text, so the host must verify each cited status before acting on it. My own Round 1 stance had the same gap. I ranked the items as: (1) a provenance test showing outside results (request_research, MCP) enter as untrusted propositions and cannot be ratified without the validator or the operator (062, 064, 086); (2) a Council dissent field in the verdict record, measured by how often a minority view survives (003, 063); (3) a token count per Council, tracked before any seat-selection change (063); (4) a candidate-only queue for /retro output, with promotion kept operator-gated (031 L2, 065); (5) the validator V1 and the primary-sources registry, gated before any KB growth (040, 004, 013, 043). The 041 registry comes last because it is the largest effort. Duplication to avoid: 086 S5 is running, 044 K2-K4 and K6 are unbuilt, 045 H2-H9 are unbuilt, and 087 S1 is next. Check gh pr list before each slice, since parallel sessions build the same slices.
> 
> RECOMMENDATION: PROCEED-WITH-CHANGES
> FINDING P1 Round 1 status claims came from memory notes, not spec files; the host must verify each spec status and state before ranking
> FINDING P1 Scope is too broad: four areas and many items; the host should deliver the five-item list above and defer the rest
> FINDING P2 No baseline exists for any proposed metric; measure before changing any default
> FINDING P2 Promotion paths (retro, train, lessons) must stay operator-gated; a held-out check is not a substitute for the operator step
> CONCUR: yes
> ROUND 2 COMPLETE

## Round 2 structured output

- architect | recommendation | PROCEED-WITH-CHANGES
- architect | concur | true
- architect | P1 | Seat-selection default change (strategist item 5) precedes any baseline; defer until per-seat token data exists
- architect | P1 | "Signed proposition" wording for MCP facts risks a signature being read as trust; specify source-labelled, tier fixed external_untrusted per 064
- architect | P2 | Replay measurement set needs a held-out, rotated split to avoid overfitting; reuse 037/048 harness
- architect | P2 | Spec status claims rest partly on stale memory (040 described as draft vs approved); verify against specs/ and the tracker before slicing
- red-team | unreadable | no readable critique
- strategist | recommendation | PROCEED-WITH-CHANGES
- strategist | concur | true
- strategist | P1 | Round 1 status claims came from memory notes, not spec files; the host must verify each spec status and state before ranking
- strategist | P1 | Scope is too broad: four areas and many items; the host should deliver the five-item list above and defer the rest
- strategist | P2 | No baseline exists for any proposed metric; measure before changing any default
- strategist | P2 | Promotion paths (retro, train, lessons) must stay operator-gated; a held-out check is not a substitute for the operator step

## Verdict

**Escalated to the operator:** Red-Team's Round 2 could not be read.

All seats (architect, strategist) recommend PROCEED-WITH-CHANGES; the open items below need your sign-off.

> The seats agree on the shape of the answer. Delphi's approved specs already cover most of what the host asks for. What is missing is measurement, plus finishing slices that are half built. None of the seats proposes a new spec. Each wants small slices on existing specs, with promotion left to the operator and outside content kept at its untrusted tier.
> 
> The proposed order is: first, a shared measurement slice (a replay script with a held-out split, tokens per seat, dissent kept) built on the 037/048 harness. Second, a negative end-to-end test that outside content cannot be promoted or ratified without the operator (062, 064, 086). Third, a structured dissent and abstention field in the ruling record (003, 063), checked by seeding a known dissent. Fourth, labelling the 41 existing claims, the primary-sources registry and lint, then the 040 validator V1 as propose-only, with KB growth waiting on those. Fifth, 031 L2 /retro and 065 writing to a candidate queue only. Last, 041 S0. Seat selection by token share waits for per-seat baselines. Before each slice, check gh pr list and do not load the capped 086 S5 run.
> 
> The seats disagree on how much to deliver at once: a short ranked list, or all four areas. They also differ on how strict the metrics and the status checks must be, since several claims rest on memory notes rather than spec text. Those differences are recorded below and not settled here. The choice is the operator's: accept this order with changes, or ask for a fuller revision first.

## Decision and next steps

**Decision:** needs the operator: Do you accept the measurement-first, no-new-spec ordering as the base for the host's four-area plan, to be refined against verified spec text?


If **PROCEED-WITH-CHANGES**:

1. `builder`: Verify the status of each cited spec against specs/ and the tracker, then write the shared measurement slice (replay script, held-out split, tokens per seat) reusing the 037/048 harness.
2. `red-team`: Write a negative end-to-end test showing that outside content cannot be promoted, ratified or raised in tier without the operator.
3. `architect`: Specify a structured dissent and abstention field for the 003/063 ruling record and a source-labelled, external_untrusted form for outside facts under 064.
4. `strategist`: Rank the remaining items by value over effort using the new baselines, and check gh pr list for duplicate work before each slice.

## Accepted Risks & Minority Report (Red-Team / Dissent)

- [P1] architect: "Signed proposition" wording for MCP facts risks a signature being read as trust; specify source-labelled, tier fixed external_untrusted per 064
- [P1] architect: Seat-selection default change (strategist item 5) precedes any baseline; defer until per-seat token data exists
- [P1] strategist: Round 1 status claims came from memory notes, not spec files; the host must verify each spec status and state before ranking
- [P1] strategist: Scope is too broad: four areas and many items; the host should deliver the five-item list above and defer the rest
- [P1] red-team: Round 2 gave no readable critique (treated as unresolved, not as agreement)

Homogeneous (low diversity): every seat is in one model family, so the author's family reviewed its own work. You may prefer to wait for a second family before approving.
