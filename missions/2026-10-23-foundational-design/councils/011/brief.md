---
id: council-011-brief
title: "Council 011: the brief given to every seat"
author: orchestrator
timestamp: 2026-10-27T19:34:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, bundles, routing]
---

# Council brief: agent routing pyramid and extensible agents (proposed Delphi spec)

You are an independent design reviewer (a Council seat) for Delphi, a multi-agent coding/work system. Review the PROPOSAL below for design soundness. Do NOT call any tool; answer directly from this text.

## Facts about the current system (verified in the code)
- Delphi has 9 canonical built-in roles (orchestrator, architect, builder, red-team, strategist, investigator, scout, writer, integrator); 5 enabled by default. Each role has a manifest: a fixed access scope (read/write globs, execute capabilities), authorized skills, memory isolation, taint rules, human-approval tiers. Only the operator and signed releases change a manifest.
- The orchestrator dispatches work with a `dispatch_agent(role, objective, tier, pointers)` tool; `role` is a free string checked against the enabled list. The orchestrator is the only role with the `agent-dispatch` capability. It absorbs verbose traces and passes compact pointer-based handoff envelopes (a "cognitive firewall").
- "Org bundles" (signed or unsigned directory trees, shared across a team by the Xenia sharing layer) can contain: knowledge (trust tier L2), skills (Agent Skills format; a skill names the roles it loads for, and its required permissions must fit within every named role's built-in scope), behavior (goals, perspectives, rules, a tighten-only policy file), and agents.
- Bundle agents today: an `agents/<name>.md` file (Claude Code agent format) with a host role declared in the bundle manifest. It runs as a span of the host role: host scope, memory, taint and approval tiers apply unchanged; the body (max 8 KiB) is appended after the host manifest. Its `tools` list is INTERSECTED with the host's tools; nothing can widen authority. Dispatch target is `bundle:<bundle>/<agent>`.
- Role guidance today: `behavior/roles/<role>.md`, instruction-only, at most 2 KiB, one per role, placed after governance and the manifest. A linter refuses text that relaxes approvals ("skip approval for small diffs"). Org guidance cannot override governance.
- Dispatchable bundle agents are listed one line each in a context slice shown to the orchestrator EVERY turn, capped at 20 entries and silently truncated beyond that. Disabled or undispatched built-in roles cost the orchestrator nothing.
- Hard rules: no new code path may act outside the sandbox, approve its own action, or let content read from outside raise its own trust. Red-tier actions always ask the human. Deterministic work belongs in code, not in a model. Limits are expressed as token shares or counts, never dollars.
- Skills load per role from the role's authorized_skills; a role has a bounded context window and skills are loaded by relevance.

## Proposal
Goal: let an organization scale the agent fleet far beyond 20 without taxing the orchestrator, and let a bundle customize a built-in role.

P1. Routing pyramid. The orchestrator keeps direct reports = the few canonical roles plus a short index of DOMAINS (for example platform, data, security, docs), not a flat list of every agent. A deterministic daemon-side `route_agent` step (tags, domains, descriptions, the existing hybrid lexical+vector search; no model call) takes a task description and returns the top-k matching registry entries; only those enter the orchestrator's context. The registry itself is unbounded and lives outside the prompt. Optionally a domain "lead" (a bundle agent) sits between the orchestrator and the leaf agents in its domain; the lead may dispatch only within its own domain subtree and returns a compact handoff envelope. Depth limit: 2 tiers below the orchestrator.

P2. Extending a built-in role (any one, for example `builder`, whose theme name is Hephaestus). A bundle may ship an EXTENSION for a canonical role, separate from role guidance, containing: (a) more skills for that role (for example a company's C++ stack conventions, or an art collective's sculpture craft), each still subject to the existing rule that a skill's required permissions fit inside the role's built-in scope; (b) a larger guidance block than 2 KiB, possibly loaded by relevance rather than always; (c) "team" members: bundle agents attached to that role as its sub-agents (for example a C++ build-system specialist under builder), reachable through the pyramid, running as spans of that role (so same scope), dispatchable by that role or its domain lead as well as by the orchestrator. An extension can only add or tighten, never widen scope, tools, execute capability, network or approval tier. Several bundles may extend the same role; precedence follows the existing bundle ordering; conflicts are reported, not silently merged.

P3. Xenia integration: extensions travel as bundle sections, so they get the existing signing, publisher-continuity, trust-tier, add/update/rollback/remove and validation lifecycle. Per the existing rule a bundle's contents are "validated sources" at a default trust level; the operator may raise one bundle.

P4. Observability: every routed or extended dispatch is logged with the bundle, the extension, the domain and the router's top-k with scores, so cost and quality are attributable; an eval measures routing accuracy (right agent in top-k) before any router default is turned on.

## Questions for you (answer each, concretely)
Q1. What are the strongest reasons this design is WRONG or will fail in practice? Rank by severity (P0 = blocks the design, P1 = serious, P2 = minor).
Q2. Security and authority: find any path by which an extension, a routed agent, or a domain lead could widen authority, self-approve, launder trust, or smuggle instructions (prompt injection through the registry text, descriptions that game the router, a lead dispatching outside its subtree, two bundles composing into a wider capability, a skill added to a role that changes its effective blast radius). Propose the minimal invariant that closes each.
Q3. Is deterministic routing (no model) good enough, or must a model be in the loop? Where exactly? What does it do when the top-k is wrong or empty?
Q4. Domain leads: worth the extra tier, or should the pyramid be only registry + router with the orchestrator dispatching leaves directly? What breaks with leads (handoff loss, dissent smoothing, retry-ladder, Council)?
Q5. Extending a role with skills and a team: what is the right unit and the right limits (sizes, counts, depth)? How should conflicting extensions from two bundles resolve? Should "extend" be a new section or an evolution of role guidance and bundle agents?
Q6. What is the smallest first slice that proves the value, and how should it be measured (name metrics and a pass bar)?
Q7. Anything the proposal omits that you would add or cut.
End with a one-paragraph recommendation: BUILD AS PROPOSED / BUILD WITH CHANGES (list them) / DO NOT BUILD, and why.

Keep the answer under 1500 words. Do NOT call any tool. Answer directly.
