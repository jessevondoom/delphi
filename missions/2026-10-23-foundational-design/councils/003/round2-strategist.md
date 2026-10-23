---
id: council-003-round2-strategist
title: "Council 003 Round 2: strategist seat (verbatim)"
author: strategist
timestamp: 2026-10-23T21:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-003, verbatim]
model: "gemini-4-argon-medium via agy"
---

## 1. Cross-Examination of Peer Findings

### Builder (`round1-builder.md`)
1. **[P1] `009#L155` (OS sandboxes can't do hostname allowlists; use local CONNECT proxy):** `agree` — `sandbox-exec` and `bubblewrap` lack DNS/SNI filtering, so a daemon-run local `HTTPS_PROXY` is the only deterministic control.
2. **[P1] `009#L155, #L139` (Harness needs read/write to its own CLI state dir):** `agree` — headless CLIs fail without session/token state dirs; scoping to the exact per-harness directory preserves [009 §4](../../../../specs/009-model-access-and-auth.md#L88-L93) Invariant 2.
3. **[P1] `009#L160` (Per-harness prompt injection table & `info/exclude`):** `agree` — prevents accidental git commits of injected system-prompt files and removes Step 3 build ambiguity.
4. **[P1] `009 §4` (OAuth2 config missing `issuer`, `client_id`, `scopes`, `audience`):** `agree` — enterprise IdPs require per-org OAuth client schemas.
5. **[P2] `009#L106` vs `002 §4` (Precedence between endpoint `models` and global `[models]`):** `agree` — endpoint-specific model IDs (e.g., Bedrock ARNs) must override global aliases.
6. **[P2] `009 §6` (Probing costs money/quota; cache TTL and `doctor --probe`):** `agree` — keeps [`delphi doctor`](../../../../specs/006-installer-and-updater.md#L155-L166) fast, offline, and free by default.
7. **[P1] `005#L167` ("Retry from last completed block" breaks reasoning signatures; retry whole turn):** `agree` — providers reject partial assistant turns with thinking blocks enabled.
8. **[P1] `005#L167` (Providers can't report usage for interrupted calls; charge full reservation):** `agree` — reconciles with Red-Team's retry cap (see §2c below).
9. **[P1] `004#L207` (Memory budget math overflows 12 KB; allocate per-scope sub-caps):** `agree` — prevents `release` + `team` (6 KB + 6 KB) from starving `operator` and `workspace` scopes.
10. **[P1] `004 §5.4` (Workspace→Operator promotion leaks client details; add linter & `isolated = true`):** `agree` — high-ROI privacy control for multi-client operators.
11. **[P2] `001 §6A` (Put `$DELPHI_HOME/operator/**` on the never-readable file-tool list):** `adjust severity to P1` — subsumed by Red-Team P0-4; snapshot-only delivery eliminates file-tool exfiltration of personal state.
12. **[P2] `004 §5.6` (`rule_stats` is before/after only, no within-role variation):** `agree` — accurately scopes expectations for Step 6 telemetry.

### Red-Team (`round1-red-team.md`)
1. **[P0] `009#L154-L163` (Harness bypasses read denylist, intra-loop taint, `.git` write-deny, non-builder limits):** `agree` — critical containment gap; must require sandbox for all harness runs, read-only/no-tools for Council seats, pre-launch worktree taint clamping, and `host_exec_paths`/`.git` checks on unbridged diffs.
2. **[P0] `005#L66-L67, #L99, #L198-L200` (Same-UID UDS socket lets subprocesses self-approve Red modals / read Round 1):** `agree` — blocking `$DELPHI_HOME/run/**` in sandboxes plus a Console session token prevents trivial privilege escalation.
3. **[P0] `005#L128-L135` vs `009#L162, #L168-L170` (Contradictory usage-estimation rules break harnesses and token budgets):** `agree` — matches Strategist P1-6; `005#L135` must allow `usage_source = 'estimated'` for harness/compat paths and enforce `subscription_budget_tokens` live on JSONL streams.
4. **[P0] `001-A1`, `006-A1`, `004` (Operator-scope read-denylist omissions, FTS5 cross-role memory leak, workspace-local lock race):** `agree` — Gear 2 search leaking peer `lessons.md` and cross-workspace lock races on `$DELPHI_HOME/operator/` are real P0 flaws.
5. **[P1] `005#L150-L151, #L167-L175` (Post-approval/pre-intent crash gap, unvalidated kept worktree files, infinite crash-retry loop):** `agree` — closes state-machine edge cases in §2.5 recovery.
6. **[P1] `009 §5–§6`, `005#L100` (Residency pin bypass via probes/overrides/redirects; dynamic token redaction; $E_1 \to E_2$ signature probe):** `agree` — `allowed_endpoints` must gate all outbound model traffic including probes and redirects.
7. **[P1] `004 §5.2–§5.3` (`profile.md` / `operator/core/` bypass no-relaxation linter; 12 KB eviction drops security rules):** `agree` — all prompt-injected overrides must pass the deterministic no-relaxation linter, and `security`-tagged rules must be exempt from score eviction.
8. **[P1] `004 §5.4–§5.7` (`delphi contribute` diff leaks, post-preview `Integrator` turn, cross-workspace dedup leaks):** `adjust severity to P2` — valid fixes to write into `004 §5.5` now, but implementation is already Post-v1 (`004 §5.8`).

---

## 2. Explicit Conflict Resolutions

### (a) Operator Permission-Widening (`delphi customize grant`)
- **Decision:** **Keep `default_access_scope` (`read`, `write`, `execute`) strictly narrow-only (no `delphi customize grant`), AND adopt Red-Team's amendment to [`specs/001#L87`](../../../../specs/001-governance-and-manifests.md#L87) allowing `authorized_skills` to add ratified custom/operator skills whose `required_permissions` are a subset of the built-in role's `default_access_scope`.**
- **Justification:** Builder's proposal breaks structural separation of powers ([`specs/003#L69`](../../../../specs/003-collaboration-and-council.md#L69) gates only `builder` into `IsolatedBuild`; widening `architect`, `red-team`, or `strategist` in `operator/core/` silently applies across *every* workspace on the machine and bypasses the 5 Merge Gates). Conversely, Red-Team identified a real extensibility bug in [`specs/001#L87`](../../../../specs/001-governance-and-manifests.md#L87): because built-in manifests never list user-created skill names, pure subset checking on `authorized_skills` blocks attaching *any* custom/operator skill. Allowing custom skills whose `required_permissions` fit within the role's built-in `default_access_scope` delivers operator sovereignty without privilege escalation.

### (b) D-19 Sequencing Across the v1 Critical Path
- **Decision:** **Keep all four access paths in v1 (honoring ratified [D-19](../../../../missions/2026-10-23-foundational-design/decisions.md#L217-L230)), split across Step 2 and Step 3, and update [`roadmap.md`](../../../../missions/2026-10-23-foundational-design/roadmap.md#L21-L59) so Steps 2–3 match `specs/009` §9:**
  - **Step 2 (Single-Agent Loop, pre-Go/No-Go):** Endpoint config/selection (`access.order`, `allowed_endpoints`), `keychain` + `env` + `command` + `aws` + `gcloud-adc` credentials, Claude Direct + Bedrock/Vertex + `gateway-native`, and `usage_ledger` columns. *(Gives every enterprise/cloud operator a working Step 2 loop.)*
  - **Step 3 (Builder & Harnesses):** Move `oauth2` device-code flow (`delphi auth login`) and live capability probing (`delphi endpoints test`) from Step 2 to **Step 3** alongside Gemini Direct/Vertex, `gateway-compat` (`/v1/chat/completions` pass), and sandboxed CLI harnesses (`claude`, `gemini`, `codex`) + local CONNECT proxy. *(Step 2 already supports corporate SSO via `command:<argv>`, `aws`, and `gcloud-adc`, so moving interactive OAuth2 and probing to Step 3 protects the Step 2 Go/No-Go timeline while shipping 100% of D-19 by Step 3.)*

### (c) Mid-Stream Crash Retry Accounting (`specs/005` §2.5)
- **Decision:** **Charge every interrupted call with unknown usage its full `reserved_usd` immediately (`outcome = 'cancelled'`, `usage_source = 'estimated'`, Builder's rule), AND allow at most 2 automatic crash-recovery retries of that turn (`attempt` unchanged, zero verification strikes) before marking the node `Escalated` (Red-Team's cap).**
- **Justification:** As Builder noted, providers have no post-hoc API to query tokens consumed by a dropped stream; releasing the reservation (`005#L167`) allows silent budget overrun. Charging `reserved_usd` per interrupted stream while capping consecutive mid-stream crash retries at 2 prevents both uncounted spend and infinite crash-loop billing on `[r]esume all`.

---

## 3. Missed Blocking Issues

None. All blocking gaps are covered by the combined Round 1 findings.

---

## 4. Final Deduplicated MUST-FIX List & Phase Deferrals

### `specs/009` (plus `roadmap.md` & `specs/002` co-alignment)
- **MUST-FIX before approval:**
  1. **Harness Sandbox & Proxy (Red-Team P0-1, Builder P1-1/P1-2/P1-3, Strategist P1-3):** Require OS sandbox for *all* harness tasks; route network exclusively through a daemon local `HTTPS_PROXY` allowlisting provider hosts; deny reads/writes to `$DELPHI_HOME` and `$HOME` except the worktree (minus `.git`) and the single per-harness state dir; scrub `env` and disable third-party hooks/plugins; clamp worktree `external_ceiling` pre-launch; run `host_exec_paths`/`.git` checks on unbridged diffs; specify per-harness CLI flags, read-only Council mode, and `info/exclude` system-prompt injection table.
  2. **Budget & Estimation (Red-Team P0-3, Strategist P1-3/P1-6):** Define byte estimation as `ceil(utf8_bytes / 4)`, set default `subscription_budget_tokens`, and enforce token/turn budgets live on harness JSONL streams.
  3. **Auth, Residency & Probing (Builder P1-4/P2-5/P2-6, Red-Team P1-6, Strategist P1-4/P2-8):** Define `[auth.<name>]` OAuth2 schema; enforce `allowed_endpoints` across primary selection, overrides, probes, and HTTP redirects; register dynamic tokens in redactor; require $E_1 \to E_2$ signature probe; specify `/v1/chat/completions` `gateway-compat` mapping; cache probe results in Zone 3 (`local/`) with TTL (`doctor --probe`); set endpoint `models` precedence over global `[models]`.
  4. **Roadmap Sync (Strategist P0-1):** Move `oauth2` device-code and active probing to Step 3 in `009 §9`, and update `roadmap.md` Steps 2, 3, and 7 to reflect D-19.

### `specs/005` (§2.4, §2.5, §4)
- **MUST-FIX before approval:**
  1. **UDS Socket Auth & Isolation (Red-Team P0-2):** Deny `$DELPHI_HOME/run/**` in all sandbox profiles; require a Console session token for privileged RPCs (`approval.resolve`, `/budget`, `/permissions`); keep `Narrator` read-only through Step 4.
  2. **Ledger Estimation Rule (Red-Team P0-3, Strategist P1-6):** Amend `005#L135` so `harness` and `gateway-compat` calls without provider usage reports record `usage_source = 'estimated'` and `outcome = 'ok'`.
  3. **Crash Recovery Hardening (Builder P1-7/P1-8, Red-Team P1-5, Strategist P1-5):** Retry whole interrupted turns (never partial blocks); charge `reserved_usd` (`outcome = 'cancelled'`, `usage_source = 'estimated'`) on interrupted calls and cap consecutive crash retries at 2; expire pre-intent Red approvals on restart; write `action.result` on idempotency-check success; run §2.2 path guard and stamp `min(L3, external_ceiling)` on kept worktree diffs; add §1.2 `Phase Mapping` table (splitting §2.5 across Steps 2, 3, and 4).

### `specs/004` (§5)
- **MUST-FIX before approval:**
  1. **FTS5 Role Isolation & Operator Locks (Red-Team P0-4):** Filter Gear 2 `zone = 'memory'` queries by caller role ID; use `$DELPHI_HOME/operator/locks/` for operator-scope CAS locks.
  2. **12 KB Sub-Caps & Eviction (Builder P1-9, Red-Team P1-7, Strategist P1-7):** Assign per-scope byte sub-caps (4 KB release, 3 KB team, 3 KB operator, 2 KB workspace) with unused carryover; define Step 2 deterministic eviction fallback (before Step 6 `rule_stats` exists) and exempt `security`-tagged rules; add `rule_stats` DDL, rebuild events, and score formula; note before/after correlational nature (Builder P2-12).
  3. **Linter & Privacy Boundaries (Builder P1-10, Red-Team P1-7/P1-8):** Run §5.2 no-relaxation linter and Red-Team review on `profile.md` and `core/` body overrides; add workspace `isolated = true` and identifier scrubbing on Workspace→Operator promotion; tighten `delphi contribute` diff/submission rules and move Test 11 to Post-v1.

### `001-A1` (`specs/001` §6A) & `006-A1` (`specs/006` §3.2)
- **MUST-FIX before approval:**
  1. **Step 1 Bootstrap & Phase Mapping (Strategist P0-2):** Bootstrap `$DELPHI_HOME/operator/` in Step 1 `delphi init` so Step 1 `doctor`/`lint` checks pass; align `004 §5.8`, `001 §1.2`, and `006 §1.1`.
  2. **Never-Readable Operator Dir & Pinning (Red-Team P0-4, Builder P2-11):** Put `$DELPHI_HOME/operator/**` on the §2.2 file-tool never-readable and never-writable lists (delivered via snapshot only); pin `operator/skills/**/scripts/**` in `operator/ratified.lock`.
  3. **Narrow-Only + Custom Skill Rule (Conflict 2a):** Lock `default_access_scope` as strictly narrow-only; allow `authorized_skills` to add ratified custom/operator skills whose `required_permissions` are a subset of the built-in role's `default_access_scope`.

### Deferrable via Phase Mapping
- **Step 3:** `oauth2` device-code CLI flow, active capability probing, Gemini Direct/Vertex, `gateway-compat`, CLI harnesses + local `HTTPS_PROXY`, and worktree/action-intent crash recovery.
- **Step 4:** Council Round 1 crash recovery.
- **Step 5:** `Narrator` agents pane and 4 views.
- **Step 6 (usage-gated):** `rule_stats` table, `/retro` scope routing, `delphi sync`.
- **Post-v1:** `delphi contribute` implementation + Test 11, `delphi operator export/import`.

---

## 5. Concurrence

`concur_after_fixes: true`
