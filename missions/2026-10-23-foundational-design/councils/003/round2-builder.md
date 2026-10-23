---
id: council-003-round2-builder
title: "Council 003 Round 2: Builder seat (cross-examination)"
author: builder
timestamp: 2026-10-23T20:45:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-003, verbatim]
model: "Claude (this session)"
---

# Council 003, Round 2: Builder seat (Claude), cross-examination

## Red-Team findings
- **P0-1 (harness bypasses the path guard, taint, `.git` denial and role limits): agree, P0.** It goes further than my Round 1 P1s:
  - The harness's *internal* tools read files Delphi never sees, so taint can't be tracked from inside the loop. Clamping the span's ceiling to the minimum tier of every file the sandbox exposes is the only sound bound.
  - macOS `sandbox-exec` allows reads by default, so the profile must be deny-by-default for reads too.
  - Inherited environment variables and user-global CLI hooks and MCP configs are real channels.
  - The diff classifier (host-exec and `.git` paths escalate to Red) must run on unbridged diffs.
- **P0-2 (any same-UID process can self-approve over the socket): agree, P0.** Verified: `specs/005#L66-L67` authenticates only by UID. Any unsandboxed Amber run, harness or script can connect and answer its own `approval_id`.
  - **Fix in three layers:** sandbox profiles deny `$DELPHI_HOME/run/**`; privileged RPCs (`approval.resolve`, `/budget`, `/permissions`, ratification) require a console capability token held only by the attached TTY client; narrator connections are read-only until Step 5 controls exist, and even then pause and inspect only.
  - Red-Team's extra rule (refuse new console attachments while an unsandboxed command runs) is proportionate. Adopt it.
- **P0-3 (`specs/005#L135` contradicts estimated usage): agree.** That line is stale text of mine from before D-19. Adopt Strategist's replacement wording plus Red-Team's live stream enforcement: kill the harness subprocess when it breaches token or turn limits, mid-run.
- **P0-4 (operator scope incomplete; FTS search leaks other roles' memory; operator lock race): agree, P0.**
  - The FTS leak predates D-20: Gear 2 search (`specs/004` §3) indexes all of `memory/**` and filters only by status and taint. It must filter memory rows to the caller's role, and never index operator scope at all.
  - Operator locks must live under `$DELPHI_HOME/operator/locks/`.
- **P1-5 (recovery gaps: approved-but-no-intent, missing synthetic result, kept files skipping the guard, crash loops): agree.** All four are real.
- **P1-6 (residency pin bypassed by probes, overrides and redirects; dynamic credentials not redacted): agree.** Literal-match redaction of every runtime-resolved credential value is the right mechanism.
- **P1-7 (profile and override bodies skip the no-relaxation check; cap eviction drops security rules): agree.** It also overlaps my scope-budget P1. Combine them: per-scope byte sub-caps, rules tagged `security` never evicted, and no-relaxation linting on profile and override bodies.
- **P1-8 (contribution privacy): agree.** In particular, submission must send the exact bytes previewed, from `delphi-core` deterministically, with no model turn after the preview.

## Strategist findings
- **P0 (D-19 overloads Steps 2–3; roadmap still lists CLI harnesses as deferred): agree, P0.** Verified: `roadmap.md` Step 7 still says "CLI harnesses" despite D-19. Accept the three-stage split (below).
- **P0 (operator scope phase contradiction; `init` never creates `operator/`): agree.** Step 1 `init` must create the operator home, and Step 1 `doctor`/`lint` must validate it.
- **P1 (harness protocols, token estimation and reservation underspecified): agree.** This overlaps my injection-table P1.
- **P1 (no chat-completions compat dialect spec): agree.** Write it as a co-amendment to `specs/002` §2.1.
- **P1 (no phase mapping in 005): agree.** Adopt Strategist's table.
- **P1 (usage contradiction): agree.** Same as Red-Team P0-3.
- **P1 (Step 2 eviction depends on Step 6 `rule_stats`; no DDL or formula): agree.** Use a deterministic fallback order until Step 6 (oldest operator rules, then oldest workspace rules; security-tagged never). Add the DDL and formula, and move test 11 to post-v1.
- **P2 (probes write team-tracked `model-capabilities.md`): agree, adjust to P1.** It breaks `doctor`'s read-only contract and leaks machine-local endpoint names into `team/`. Store probe results in Zone 3.

## Conflicts
- **(a) Permission widening: I concede. Narrow-only stays absolute**, with Red-Team's extension that operators may *attach* ratified custom or operator skills whose `required_permissions` fit within the role's built-in scope.
  - The decisive failure mode is Strategist's: widening a non-builder role lets code change outside `IsolatedBuild` and the five merge gates. An operator-scope waiver would also apply silently across every workspace.
  - Operator sovereignty is still served by `@builder` plus custom skills. The operator decides this, but the council recommendation is now unanimous.
- **(b) Sequencing:**
  - **Step 2:** endpoint layer, `env`/`keychain`/`command` credentials (the `command:` helper already covers corporate SSO), Claude direct API, native gateways.
  - **Step 3:** Gemini direct API, Bedrock and Vertex, the compat dialect.
  - **Step 4** (before the Council work, to use the Step 3 sandbox): the three harnesses, the MCP bridge, subscription budgets, the `oauth2` device-code flow, active probing.

  All four access paths still ship in v1, consistent with D-19.
- **(c) Crash accounting: both rules apply.** Each interrupted call is charged its full reservation (usage unknown, `usage_source = estimated`). After **2** consecutive interruptions on the same turn, the node escalates instead of retrying.

## Missed by all three
- None that block approval. One clarification: the console capability token must survive daemon restarts (re-issued on reattach over the TTY), or recovery would leave re-raised approvals unanswerable.

`concur_after_fixes: true`
