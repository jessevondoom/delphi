---
id: council-011-haiku
title: "Council 011, Round 1: Claude Haiku (claude -p --model haiku, no tools) (verbatim)"
author: red-team
timestamp: 2026-10-27T19:50:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, heterogeneous, bundles, routing]
---

> Verbatim Round 1 output (blind) of the seat above, answering [the brief](brief.md).

## Q1. Failure modes, ranked

**P0: The premise is unmeasured.** The stated problem is that an organization will exceed 20 dispatchable agents. Nothing in the brief shows that any team is near that limit. The current failure is also narrower than the proposal implies: the 20-entry cap truncates silently, so an agent past the cap is undispatchable with no error. That is a bug to fix now, and it does not need a pyramid. Build the measurement (how often real tasks pick the wrong or no agent, and how many listing tokens the orchestrator spends per turn) before committing to the architecture.

**P1: Silent routing misses.** If the right agent is outside top-k, the orchestrator never sees it and does not know it is missing. The failure looks like "no agent was needed," so it goes unnoticed.

**P1: Domain leads add a lossy hop.** Each lead is a second cognitive firewall that compresses leaf output. Evidence gets dropped, disagreement gets averaged out, and retries, cost, and accountability are split across two levels.

**P1: Extensions grow the builder's context.** A role has a bounded window. Relevance-loaded skills, a 2 KiB+ guidance block, and team sub-agents all compete for it. Adding skills from several bundles can make the built-in role worse, not better, and nothing in the proposal measures that.

**P1: Conflicts resolved by ordering.** "Precedence follows bundle order" means the later bundle silently wins on overlapping guidance. Reporting conflicts helps only if installation fails closed until someone resolves them.

**P2:** The examples (art-collective sculpture craft, the Hephaestus theme name) suggest the use case is drifting away from coding. Theme names do not belong in the design.

## Q2. Authority and trust paths, with invariants

1. **Registry text as prompt injection.** Descriptions and tags render into the orchestrator's context. **Invariant:** render registry entries as quoted data with a length cap, lint them with the existing guidance linter, and state in governance that descriptions are never instructions.
2. **Router gaming.** A bundle stuffs its description with "production, deploy, security" to win routing. Routing grants no authority, since the host's scope is fixed, so the harm is misdirection. **Invariant:** routing is advisory only; the selected agent's tools stay intersected with the host's; the router's score is logged, never used for approval.
3. **Lead dispatching outside its subtree.** A lead's prompt cannot be the control. **Invariant:** `dispatch_agent` checks the caller's authenticated dispatch identity against the target's ancestry in code. A lead's subtree is a daemon-enforced set, not a prompt instruction.
4. **Self-approval through the chain.** A lead or team member approving a leaf's action. **Invariant:** a span's approval tier equals its host's, and red-tier actions always go to the human regardless of who dispatched them.
5. **Trust laundering through summaries.** A lead's envelope summarizes L2 bundle content and comes back looking like orchestrator-grade input. **Invariant:** an envelope's taint is the maximum taint of its inputs, and trust never rises through a summary.
6. **Operator trust raise propagating.** Raising one bundle's trust silently raises its team members and extension skills. **Invariant:** the raise applies to the named bundle and each extension explicitly, not inherited by its sub-agents.
7. **Skill blast radius.** A skill can fit the scope on paper while instructing the role to use its existing write or execute capabilities in new ways. **Invariant:** the guidance linter applies to skills and extensions, and no skill may reference approval, execute, or network paths beyond the role's declared set.
8. **Two bundles composing.** Bundle A's skill hands off to bundle B's team member. Each individually fits; the chain may not. **Invariant:** handoffs are checked against the host's scope at each hop, so composition cannot exceed the host's ceiling.

## Q3. Deterministic routing

Deterministic shortlisting is enough for a first slice, with one correction: the "no model call" claim does not hold for the hybrid vector half, because query embeddings come from a model. Be explicit about that. Models belong at ingestion (generating or reviewing tags and descriptions, reviewed once), not at route time.

When top-k is wrong or empty: return top-k plus the domain index. The orchestrator may re-route with a refined query, capped at two tries. If that still fails, it falls back to the canonical roles and logs a miss. Never fall back silently to the full list.

## Q4. Domain leads

Cut them from the first slice. Registry plus router, with the orchestrator dispatching leaves directly, handles the stated goal. Add a lead only if measurements show the orchestrator is overloaded by within-domain fan-out. Leads break handoff fidelity, dissent (a Council needs raw disagreement, not a smoothed summary), retry accounting, and cost attribution.

## Q5. Extension unit and limits

- **Unit:** skills plus team membership plus an optional guidance block. Do not invent a new object type.
- **Team members** are existing bundle agents with an optional `host` field. This is an evolution of bundle agents, not a new section.
- **Guidance** stays in the existing role-guidance slot, with the cap raised only for relevance-loaded blocks. Do not add a parallel "extends" section that duplicates the linter path.
- **Limits:** at most 5 skills and 5 team members per bundle per role; team depth of 1 under the role (team members cannot have teams); a token-share ceiling for total loaded skills per role; guidance at most 8 KiB, relevance-loaded.
- **Conflicts:** duplicate agent or skill names across bundles fail installation until the operator picks one. Guidance is concatenated in bundle order with a conflict report, never silently merged.

## Q6. First slice and measurement

Ship registry, router, and the orchestrator domain index, with no leads and no extension guidance.

- **Routing recall@5** on a labeled set of 100–200 real task descriptions from past runs. Pass bar: at least 0.9.
- **Silent truncation:** zero. Every agent is either listed or reported as unlisted.
- **Orchestrator listing tokens per turn:** fixed and within budget as the registry grows.
- **Outcome parity:** an A/B on the same tasks against the flat-list baseline, comparing rework rate and escalations. Pass bar: no regression.
- **Re-route rate:** if the orchestrator re-routes often, the router is failing and the slice does not pass.

## Q7. Omissions

- **Add:** a kill switch for the router and for each bundle's extensions; per-extension A/B evaluation before default-on, since an extension can degrade its role; deprecation and unload for bundle agents; a clear rule on who may install extensions (operator only, or org admin); and diff-level audit of registry changes.
- **Cut:** the theme names and the art-collective example, and the separate extension section if team members and skills can be expressed as bundle agents and skills.

## Recommendation

**BUILD WITH CHANGES.** The core idea is sound: a deterministic router that keeps the orchestrator's context small is a reasonable answer to scaling, and it fits the existing hard rules. But the proposal is premised on an unmeasured problem and bundles several new trust surfaces at once. The changes:

1. Fix the silent 20-entry truncation now, and measure real listing cost and routing misses.
2. Ship only registry plus router plus domain index, with the measurement bar in Q6.
3. Drop domain leads from the first slice.
4. Enforce subtree checks, approval-tier inheritance, taint as max-of-inputs, and no trust inheritance in code, not in prompts.
5. Express extensions as skills plus team bundle agents (`host` field), with guidance deferred to a second slice.
6. Set the explicit limits in Q5, and fail installation closed on name conflicts.

Extend only after the first slice shows better routing recall at equal outcome quality.
