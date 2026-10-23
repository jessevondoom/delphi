---
id: council-003-round1-red-team
title: "Council 003 Round 1: red-team seat (blind, verbatim)"
author: red-team
timestamp: 2026-10-23T20:30:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-003, verbatim]
model: "gemini-4-argon-high via agy"
---

### 1. Findings

- **[P0]** [`specs/009-model-access-and-auth.md#L154-L163`](../../../../specs/009-model-access-and-auth.md#L154-L163) — `[REV-SEC-02 / REV-SEC-06 / REV-SEC-07]` Harness mode bypasses read denylists, intra-loop taint accounting, `.git` write-deny, and non-builder restrictions.
  - **Failure mode:**
    1. *Unsandboxed non-code tasks & unbridged host-exec/gitfile writes:* `#L155` disables harness mode without a sandbox only for "code-writing tasks," permitting a Council seat (`#L163`) or discovery task to run an unsandboxed third-party CLI loop. When sandboxed, `#L155` permits writes anywhere in the worktree (omitting the [`specs/001-governance-and-manifests.md#L77`](../../../../specs/001-governance-and-manifests.md#L77) / [`specs/008-ambient-watch-and-rewind.md#L91`](../../../../specs/008-ambient-watch-and-rewind.md#L91) write-deny on `@worktree/.git` and nested `.git`), and `#L158` accepts unbridged worktree diffs at Amber tier (auto-approved under `/permissions mission-auto`, [`specs/005-console-and-narrator-tui.md#L195`](../../../../specs/005-console-and-narrator-tui.md#L195)) without escalating `host_exec_paths` ([`specs/008-ambient-watch-and-rewind.md#L98`](../../../../specs/008-ambient-watch-and-rewind.md#L98)) to Red.
    2. *Read-denylist & secret exfiltration:* Because the harness runs its own file/shell tools and has live network access to its provider (`#L155`), it bypasses `delphi-core`'s file-tool path guard ([`specs/001-governance-and-manifests.md#L78-L80`](../../../../specs/001-governance-and-manifests.md#L78-L80)). On macOS (`sandbox-exec` has no mount namespaces and defaults to allowing reads), or via inherited environment variables and global CLI hooks/MCP configs (`~/.claude/`, `~/.gemini/`), the harness can read `$DELPHI_HOME` (`state.db`, `events.jsonl`, peers' `lessons.md`, `$DELPHI_HOME/operator/`), `$HOME/.ssh`, and `$HOME/.aws`, exfiltrating them to the provider API.
    3. *Intra-loop taint laundering:* A harness span starting at $L_4$/$L_5$ (`#L161`) can read an $L_0$ file inside the worktree (`vendor/**`, [`specs/001-governance-and-manifests.md#L221`](../../../../specs/001-governance-and-manifests.md#L221), or an $L_0$ provenance-indexed file) using its internal tools without `delphi-core` lowering `external_ceiling`; `#L161` stamps its output $L_3$, which rises to $L_4$ once a test passes ([`specs/001-governance-and-manifests.md#L214`](../../../../specs/001-governance-and-manifests.md#L214)), laundering $L_0$ into $L_4$.
  - **Remediation:**
    1. Require an active OS sandbox for **all** harness runs (never only code-writing tasks); for non-builder roles and Council seats (`#L163`), enforce read-only worktrees and disable all CLI write/shell tools.
    2. In the harness sandbox profile, deny `file-read*` and `file-write*` on `$DELPHI_HOME` (except the task's worktree and ratified skill inputs) and `$HOME` (except the single CLI credential file required by that binary); deny `file-write*` on any `.git` segment inside the worktree; scrub inherited `env` to an explicit allowlist; and pass CLI flags disabling user-global hooks, plugins, and non-Delphi MCP servers.
    3. Before launching a harness, clamp the span's `external_ceiling` to the **minimum provenance tier of every file in the worktree** (refusing launch if $\le L_1$), and run the `specs/008` §4 `host_exec_paths` and `.git` classifier on all `#L158` unbridged diffs (escalating matches to Red).

- **[P0]** [`specs/005-console-and-narrator-tui.md#L66-L67`](../../../../specs/005-console-and-narrator-tui.md#L66-L67), [`#L99`](../../../../specs/005-console-and-narrator-tui.md#L99), [`#L198-L200`](../../../../specs/005-console-and-narrator-tui.md#L198-L200) — `[REV-SEC-07 / REV-TRAP-01]` Same-UID UDS socket lets subprocesses self-approve Red modals and read Round 1 Council streams.
  - **Failure mode:** Agent subprocesses (CLI harnesses, custom scripts, unsandboxed Amber test runs under D-04 fallback, or macOS `sandbox-exec` runs where [`specs/008-ambient-watch-and-rewind.md#L91`](../../../../specs/008-ambient-watch-and-rewind.md#L91) only denies `file-write*`) run as the operator's UID. Because `~/.delphi/run/<workspace-hash>.sock` checks only UID via `SO_PEERCRED`/`getpeereid` (`#L67`) and does not separate `Console` from `Narrator` or subprocesses, a malicious test or prompt-injected harness can connect to the UDS socket, read live Council Round 1 streams (`#L99`), raise `/budget` (`#L196`), or send the first `approval_id` answer (`#L199`) to **self-approve its own Red-tier action**.
  - **Remediation:**
    1. In `specs/008` §4 and `specs/009` §7.2, explicitly deny `file-read*`, `file-write*`, and `network-outbound (remote unix-socket)` on `$DELPHI_HOME/run/**` in all sandbox profiles.
    2. In `specs/005` §2.1 and §3.1, require a cryptographic `Console` session token (passed via pipe on spawn or interactive TTY attach, never readable by agents) for privileged RPCs (`approval.resolve`, `/budget`, `/permissions`), and make `Narrator` connections strictly read-only through Step 4. While any unsandboxed Amber command is executing, reject new `Console` attachments unless confirmed by an already-attached `Console`.

- **[P0]** [`specs/005-console-and-narrator-tui.md#L128-L135`](../../../../specs/005-console-and-narrator-tui.md#L128-L135) vs [`specs/009-model-access-and-auth.md#L162`](../../../../specs/009-model-access-and-auth.md#L162), [`#L168-L170`](../../../../specs/009-model-access-and-auth.md#L168-L170) — `[REV-ARCH-01]` Contradictory usage-estimation rules break harness execution and disable `subscription_budget_tokens`.
  - **Failure mode:** `specs/005#L135` states *"Token counts come from the provider's usage report, never from an estimate. Calls with no usage report are recorded with `outcome = error` and the reservation as their cost,"* directly contradicting `specs/005#L128` (`usage_source: reported | estimated`) and `specs/009#L162`. For a subscription harness that omits usage reports (`cost_usd = 0`, `reserved_usd = 0`), recording `outcome = error` with `0` tokens means every such harness call fails verification OR leaves cumulative tokens at `0` so `subscription_budget_tokens` (`specs/009#L170`) never triggers. Moreover, because a harness runs a multi-turn loop inside one subprocess, checking budget only at process exit allows a single invocation to loop unboundedly.
  - **Remediation:** Amend `specs/005#L135` so metered API/cloud/native-gateway calls require a provider report (recording `outcome = error`, `cost_usd = reserved_usd`, and byte-estimated tokens on missing report), while harness and `gateway-compat` calls without a usage report record `usage_source = 'estimated'` (`bytes / 4`) without forcing `outcome = error`. In `specs/009#L170`, define default `subscription_budget_tokens` ceilings (e.g., `500_000` per mission, `2_000_000` per Council), reserve worst-case tokens pre-dispatch, and enforce both token and `max_turn_budget` limits live on the harness JSONL stdout stream ([`specs/002-adapter-and-routing.md#L138`](../../../../specs/002-adapter-and-routing.md#L138)), killing the subprocess on breach.

- **[P0]** [`specs/001-governance-and-manifests.md#L70-L88`](../../../../specs/001-governance-and-manifests.md#L70-L88), [`#L270-L274`](../../../../specs/001-governance-and-manifests.md#L270-L274), [`specs/006-installer-and-updater.md#L121-L133`](../../../../specs/006-installer-and-updater.md#L121-L133), and [`specs/004-memory-and-coaching.md#L65`](../../../../specs/004-memory-and-coaching.md#L65), [`#L99`](../../../../specs/004-memory-and-coaching.md#L99), [`#L165`](../../../../specs/004-memory-and-coaching.md#L165) — `[REV-SEC-02 / REV-TRAP-04]` Incomplete operator-scope isolation, FTS5 cross-role memory leak, and cross-workspace lock race.
  - **Failure mode:**
    1. *Read-denylist & FTS5 cross-role leak:* `001-A1` (`specs/001#L272`) adds only `memory/operator/<self>/**` and `operator/profile.md` to §2.2, omitting `memory/operator/<other>/**`, `operator/proposals/`, and `operator/ratified*` from the `specs/001#L78` read denylist, and omitting `operator/heuristics.md`, `operator/core/**`, and `operator/skills/**` from the §2.2 logical mapping table. Worse, `specs/004#L99` indexes all roles' `memory/**` into `state.db`, and `#L165` filters only by `status` and taint tier—allowing any agent calling Gear 2 search to read other roles' private `lessons.md`!
    2. *Unpinned operator skill scripts:* `006-A1` (`specs/006#L128`) adds `operator/skills/`, but `001-A1` (`specs/001#L273`) does not extend `specs/001#L85` to pin `operator/skills/**/scripts/**` in `operator/ratified.lock`.
    3. *Cross-workspace CAS lock race:* `specs/004#L65` locks `.delphi/locks/<path>.lock` (workspace-local per [`specs/006-installer-and-updater.md#L111`](../../../../specs/006-installer-and-updater.md#L111)), so two workspace daemons concurrently updating `$DELPHI_HOME/operator/` lock different files and race on `operator/heuristics.md`, `operator/lessons.md`, and `operator/ratified.lock`.
  - **Remediation:**
    1. In `specs/001#L272-L273` (`001-A1`), map `operator/heuristics.md`, `operator/core/**`, and `operator/skills/**` in §2.2; add `memory/operator/<other>/**`, `operator/proposals/`, and `operator/ratified*` to the §2.2 read denylist; and require `operator/skills/**/scripts/**` hashes in `operator/ratified.lock`.
    2. In `specs/004#L165`, require Gear 2 queries from an agent to filter `zone = 'memory'` rows to `rel_path` matching the caller's canonical role ID.
    3. In `specs/004#L65` and `specs/006#L129` (`006-A1`), place operator-scope locks at `$DELPHI_HOME/operator/locks/<path>.lock`.

- **[P1]** [`specs/005-console-and-narrator-tui.md#L150-L151`](../../../../specs/005-console-and-narrator-tui.md#L150-L151), [`#L160`](../../../../specs/005-console-and-narrator-tui.md#L160), [`#L167-L175`](../../../../specs/005-console-and-narrator-tui.md#L167-L175) — Crash recovery races: post-approval/pre-intent gap, unvalidated worktree keeps, and infinite crash-retry billing loops.
  - **Failure mode:**
    1. If `delphi-core` crashes after fsyncing a granted Red-tier `approval.resolved` (`#L150`) but before writing `action.intent` (`#L151`), the node matches neither `#L169` nor `#L171`, either executing a stale destructive approval on restart or hanging. Also, when `#L171`'s idempotency check succeeds, it does not append `action.result`, so a later `/rewind` ([`specs/008-ambient-watch-and-rewind.md#L75`](../../../../specs/008-ambient-watch-and-rewind.md#L75)) misses the side effect.
    2. Keeping post-checkpoint worktree changes (`#L168`, `#L173`) stamps them with "the span's tier" (potentially $L_4$/$L_5$ `external_ceiling` without passing verification) and skips the `specs/001` §2.2 path guard (`.git`, symlinks, `st_nlink > 1`).
    3. Releasing reservations on mid-stream crashes with zero strikes and zero recorded cost (`#L167`, `#L174`) means a repeatable mid-stream crash (e.g., SSE parser panic) causes `[r]esume all` (`#L160`) to retry infinitely while billing the provider and logging `$0`.
  - **Remediation:** Expire any granted Red-tier `approval.resolved` lacking an `action.intent` on recovery and re-raise `approval.requested`; append and fsync a synthetic `action.result` whenever an `#L171` idempotency check confirms execution; run the `specs/001` §2.2 path guard on kept `#L168`/`#L173` files, stamp them `min(L3, external_ceiling)`, and write a `worktree.checkpoint`; and cap consecutive mid-stream crash retries at 2 per turn before charging `reserved_usd` (`outcome = 'error'`) and escalating.

- **[P1]** [`specs/009-model-access-and-auth.md#L61`](../../../../specs/009-model-access-and-auth.md#L61), [`#L85-L90`](../../../../specs/009-model-access-and-auth.md#L85-L90), [`#L111-L113`](../../../../specs/009-model-access-and-auth.md#L111-L113), [`#L125-L147`](../../../../specs/009-model-access-and-auth.md#L125-L147) and [`specs/005-console-and-narrator-tui.md#L100`](../../../../specs/005-console-and-narrator-tui.md#L100) — `[REV-SEC-05]` Residency pin bypass via probing/overrides/redirects and dynamic credential leakage.
  - **Failure mode:**
    1. `allowed_endpoints` is applied only under Failover (`#L130`), while `delphi endpoints test` (run by `doctor` and `init`, `#L137`) probes every configured endpoint, per-role overrides (`#L132`) are not explicitly intersected with `allowed_endpoints`, and gateway HTTP redirects (`302`/`307` on `base_url`, `#L111`) can redirect requests and auth headers outside the residency pin.
    2. `#L131` lets §6 single-endpoint probes relax foreign-signature downgrading across endpoint pairs without testing cross-endpoint replay ($E_1 \to E_2$), risking `400 Invalid signature` failures.
    3. OAuth2, AWS STS, Google ADC, `command:<argv>` tokens, and custom `headers` (`#L113`) do not match static format regexes in `specs/005#L100` and will leak if echoed in error responses or helper stderr.
  - **Remediation:** Enforce `allowed_endpoints` on primary selection (`#L129`), failover (`#L130`), per-role overrides (`#L132`), and capability probes (`#L137`); disable cross-origin HTTP redirects; require an explicit $E_1 \to E_2$ replay probe before sharing signatures across endpoints (`#L131`); and register all runtime-resolved credential values (`keychain`, `env`, `aws`, `gcloud-adc`, `oauth2`, `command`, secret `headers`) into the redactor's literal-match set while suppressing raw helper stderr.

- **[P1]** [`specs/004-memory-and-coaching.md#L201-L214`](../../../../specs/004-memory-and-coaching.md#L201-L214), [`#L237-L242`](../../../../specs/004-memory-and-coaching.md#L237-L242) — Governance relaxation via `profile.md`/`operator/core/` and 12 KB cap eviction of security rules.
  - **Failure mode:**
    1. `operator/profile.md` (`#L211`, which includes "review strictness") and `operator/core/agents/<role>/AGENT.md` body overrides (`#L213`, "may change instructions freely") bypass the `#L208` rule linter and Red-Team review when edited via `delphi customize`, allowing an override or poisoned proposal to soften prompt-only critic invariants ([`context/invariant-enforcement.md#L70-L76`](../../../../context/invariant-enforcement.md#L70-L76)), reinforced by `#L208` ("more specific scope wins") at higher-recency prompt positions.
    2. Release (`6 KB`) + team (`6 KB`) + profile (`1 KB`) alone reach `13 KB > 12 KB` (`#L207`), causing 100% of operator and workspace rules to be evicted and still overflowing 12 KB; under `#L241-L242`, newly ratified or low-frequency security rules score lowest in `rule_stats` and are evicted first.
  - **Remediation:** Subject `operator/profile.md`, `operator/core/agents/**`, and `team/agents/custom/**` body changes to the §5.2 no-relaxation linter and Red-Team review (forbidding `profile.md` from modifying `REVIEW.md` §1 severity rules), state in the IPR Block 1 preamble that Blocks 3–4 cannot override Block 1–2 invariants, assign per-scope byte sub-caps that sum to $\le 12\text{ KB}$ (or raise the combined cap), and exempt `security`-tagged rules from automatic score eviction.

- **[P1]** [`specs/004-memory-and-coaching.md#L221`](../../../../specs/004-memory-and-coaching.md#L221), [`#L227-L233`](../../../../specs/004-memory-and-coaching.md#L227-L233), [`#L247`](../../../../specs/004-memory-and-coaching.md#L247) — Privacy leaks in `delphi contribute` bundles, cross-workspace dedup, and shared `team/` artifacts.
  - **Failure mode:**
    1. In `#L228-L232`, diffing a customized `core/` or `operator/core/` file against `base/` leaks unrelated operator customizations in the same file; LLM-generated `/retro` rationales and raw "test results" (`#L229-L230`) contain internal repo paths, symbol names, and stack traces; and delegating submission to the `Integrator` LLM agent (`#L232`) *after* operator preview allows Integrator to attach unpreviewed workspace context to the outbound PR/issue.
    2. Nothing enforces `#L247` when agents write `[O-…]` rule text or `operator/**` pointers into `team/specs/**` or `team/context/**`, and `#L221` cross-workspace dedup across isolated `state.db` files leaks private workspace tokens if one workspace's daemon reads another's `local/` memory.
  - **Remediation:** Build `delphi contribute` diffs by applying only the single target AST rule/section against clean `base/`; include only numeric counter aggregates from `usage_ledger`/`rule_stats` (never raw test output); scan the bundle with the `REV-SEC-05` path/secret and repo-identifier filter; submit the exact previewed bundle bytes deterministically from `delphi-core` at the Red gate without an post-preview `Integrator` LLM turn; block `[O-…]` and `operator/**` references on writes to `team/**`; and compute `#L221` dedup using only normalized hashes recorded in `$DELPHI_HOME/operator/` at ratification.

---

### 2. Open-Question Recommendation (Item 5)

**Recommendation:** **Keep `default_access_scope` (`read`, `write`, `execute`) strictly narrow-only (no `delphi customize grant` waivers), while amending [`specs/001-governance-and-manifests.md#L87`](../../../../specs/001-governance-and-manifests.md#L87) so `authorized_skills` may add ratified custom/operator skills whose `required_permissions` (`specs/001` §3.1) are a subset of the built-in role's `default_access_scope`.**

- **Failure modes if `default_access_scope` widening is allowed:**
  1. *Bypasses `No Spec, No Build` and the 5 Merge Gates:* `delphi-core`'s DAG guard ([`specs/003-collaboration-and-council.md#L69`](../../../../specs/003-collaboration-and-council.md#L69)) gates only `Builder`'s transition to `IsolatedBuild`. Widening `orchestrator`, `architect`, or `red-team` to `write: ["@worktree/**"]` or shell execution lets non-builder roles mutate code outside `IsolatedBuild` and breaks critic independence (`GOVERNANCE.md` §2, `[REV-SEC-02]`).
  2. *Cross-workspace privilege bleed:* A waiver recorded in `operator/ratified.lock` (`001-A1`) silently widens that role across **every** workspace on the operator's machine, including high-security repos.
- **Failure mode if `authorized_skills` stays a static name-subset of `base/` (`specs/001#L87` today):**
  No built-in manifest lists user-created `skills/custom/<name>` or `operator/skills/<name>`, so `delphi lint` currently rejects attaching *any* custom skill to any role. Allowing `authorized_skills` to include ratified custom/operator skills whose `required_permissions` stay within the built-in role's `default_access_scope` gives operators safe extensibility without ever widening a role's access ceiling.

---

### 3. Checked and Sound

1. **Delegated credential storage (`specs/009` §4):** No third-party CLI session files, cookies, or SDK caches are copied or reused; OAuth refresh tokens and static keys live only in the OS keychain.
2. **Model family diversity across endpoints (`specs/009` §2, §10 Test 6):** Reaching the same training lineage through multiple endpoints (e.g., Anthropic API + Bedrock) counts as one family for `[H-001]`/`[H-007]`.
3. **Append-only durability & frame coalescing (`specs/005` §2.1, §2.3):** 16ms frame coalescing with `seq_from..seq_to` and `events.jsonl` write-ahead `fsync` before SQLite indexing prevent lost sequence numbers and torn state.
4. **Synchronous approval timeout & `/permissions` Red-tier invariance (`specs/005` §3.1):** Unanswered approvals resolve deterministically to deny, and no `/permissions` mode auto-approves Red-tier actions.
5. **Operator scope ratifications (`specs/004` §5.3–5.4, `006-A1`):** Operator-scope lessons and overrides are staged as proposals and loaded only from `operator/ratified/` copies matching `operator/ratified.lock`.
6. **Three-way merge survival on `delphi update` (`specs/004` §5.3, `006-A1`):** `operator/core/` overrides merge against `base/` ancestors and stage conflicts as `.upstream` rather than overwriting customizations.

---

### 4. Per-Item Decisions

1. **`specs/009-model-access-and-auth.md`:** `approve-with-fixes` (requires fixing **P0-1**, **P0-3**, **P1-6**).
2. **`specs/005-console-and-narrator-tui.md` (§2.4, §2.5, §4):** `approve-with-fixes` (requires fixing **P0-2**, **P0-3**, **P1-5**).
3. **`specs/004-memory-and-coaching.md` (§5):** `approve-with-fixes` (requires fixing **P0-4**, **P1-7**, **P1-8**).
4. **Pending amendments `001-A1` (`specs/001` §6A) and `006-A1` (`specs/006` §3.2):** `approve-with-fixes` (requires fixing **P0-4** and applying the **Item 5** narrow-only rule).

---

### 5. Concurrence

`concur: false`
