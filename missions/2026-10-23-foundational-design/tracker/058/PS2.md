---
state: built
date: 2026-10-30
title: "Home + project memory/context routing, provenance labels, project isolation"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell.

The row (line 410, under `Project scopes, scaffolding, indexes and promotion (spec 058, approved 2026-10-30)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| PS2 | Home + project memory/context routing, provenance labels, project isolation | built in part 2026-10-30 (`delphi-engine/src/scope.rs`: resolution with fail-closed registry, home+project `layered_sources` with layer labels and labelled overrides, `route_staged`; `learn::project_staged` writes a mission's staged notes to its project layer from a `<mission>/scope` file and to no layer when the id is unregistered or spoofed). Built 2026-10-30 (PS2b): `mission.open` carries the client `cwd` and a newly created mission is stamped with the resolved project (`EngineService::mission_at`; no cwd, unresolvable or ambiguous/corrupt registry stamps nothing = home-only), and `scope::prompt_layer` puts the mission's own project layer (own-role memory, knowledge, context; never staged, never another project's) into the span prompt as a labelled `@runtime/project-layer` slice, span tainted to L0. Deferred: moving unscoped missions to the default project, loading the project layer into the `Kb` index (search/`kb::sources` still home-only; project items stay out of search until then), operator-chosen destination in a proposal, project-layer `heuristics`/`lessons` ratification |
