---
id: spec-092-operator-authorized-tests
title: "092: Operator-authorized tests: an advance, scoped, recorded approval of named Red actions in a headless test run"
author: architect
timestamp: 2026-11-07T00:00:00Z
status: draft   # a proposal, not permission to build. It amends an invariant of spec 016 (Red is never allowed), so it needs the operator's approval and a readable red-team re-review of this text before any slice starts (§6, S0).
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # a design for a safety boundary, reasoned from the existing approval rules, one live observation and two Councils; no part of it is built or attacked in code
source_mission: 2026-10-23-foundational-design
tags: [spec, approvals, authority, red-team, headless, testing, security, sandbox, draft]
---

# Spec 092: Operator-authorized tests

> **Status: draft.** Amends invariant 1 of [`specs/016`](016-headless-run.md) §3 ("Red is never allowed") in one narrow way, for headless runs only. Builds on [`specs/014`](014-relay-and-away-mode.md) §7.1 (a live Red approval with step-up; the number 014 is shared with another spec, see draft 090), [`specs/005`](005-console-and-narrator-tui.md) (the permission modes and the Green, Amber and Red tiers) with `GOVERNANCE.md` §5, and [`specs/037`](037-training-experiments-quality-and-efficiency.md) (throwaway homes). Companion to `specs/091`, which widens no authority.

## 1. Why

On 2026-11-07 every `agy` arm of a headless training run tried to dispatch an investigator and was refused: "The operator denied this action: headless run: a Red action is never approved without the operator." That is the rule working. It also means a headless test cannot exercise a Red path at all: not a red-team sweep that needs a hostile command run in a sandbox, not a dispatch, not a cross-family seat hop.

**The operator's direction (2026-11-07):** Red actions should be allowed when they are part of a test the operator authorized, and the authorization can state the red-team intent plainly, so it counts as the approval on a headless run. **How one comes about:** an agent may *request* an authorization for a future headless action, briefly explaining the context and the risk, so that the operator's approval carries full merit. The agent asks; the operator decides.

**Council `t_0003` (2026-11-07, stakes high; architect, strategist, red-team)** found the invariant preserved in intent and not yet in mechanism, and all three seats chose *approve with changes, narrower first*. Its required changes are written into this text (§3, §5). The red-team seat's Round 2, which the Council's reader could not parse, is intact in the Council record and added two P0 findings, PATH shadowing and worktree poisoning, both closed here (§3 TA-2, TA-5).

## 2. What must not change

1. **An agent never approves its own action.** The authorization is made by the operator, in an act an agent cannot perform or forge. An agent may *request* one (TA-0); a request is inert, grants nothing, and **is never effective for the run that wrote it**.
2. **Content read from outside never raises trust,** and never creates, extends or alters a request or an authorization.
3. **Merges and pull requests are never authorized this way** (spec 016 invariant 2 stands; the merge card is approved only at the console, 014-A1). Nor are credentials, a network call, dispatch, or a write to `.git`, a hook, a configuration file or the authorization store.
4. **Nothing leaves the sandbox.** An authorized action runs only in a verified OS sandbox with the network dropped, in a throwaway home the daemon made, over a disposable snapshot of the project. If those cannot be had, the run does not start.
5. **Every use is recorded** with who authorized it, when, and what it covered.
6. **This amendment applies to headless runs only.** An interactive session keeps "Red always asks".

## 3. The additions

| # | Addition | What it is |
| :-- | :-- | :-- |
| TA-0 | **The request** | A tool `request_test_authorization` for an agent (and for a host over the host socket, as `propose_note` is): `{intent, purpose, manifest, context, risk, run_plan}`. It is **staged, inert and labelled**: tier `L0`, the requesting role, model and span, and the span's **taint**. **A request from a span at `federated_peer` taint or lower is refused at staging**, not merely labelled, so hostile content cannot launder a request through a trusted agent. It cannot be used by any run, including the one that wrote it. Pending requests are capped, de-duplicated by the hash of their `manifest`, and a refused request is not re-staged for a period (OD-5). Free text is cut to a fixed length and stripped of control and escape characters. |
| TA-0r | **The review** | `delphi test review` shows the operator, in this order: (1) the **manifest** the code will enforce, in plain words, exactly as it will be matched and hashed (TA-1); (2) a **dry run**: the manifest replayed against recent recorded cards, showing what it would have matched; (3) a **structured diff against the operator's earlier grants**; (4) the requester, its taint and span; (5) the agent's `context` and `risk`, **collapsed by default**, labelled as the agent's words and data, never as instructions, and stripped again of control and escape characters on display. Only (1) is enforced. The operator may narrow the manifest, shorten the expiry or the uses, or refuse. Approving is `delphi test authorize --from-request <id>`: a terminal and the operator's step-up are required, and it writes a **new** TA-1 record that stores the request's id and hash. A request that arrived by the relay can be reviewed and authorized only at a local terminal (no relay-forwarded approval). |
| TA-1 | **The authorization record** | Stored where only the daemon and the operator's ratification can write (a protected, hash-locked location an agent has no write path to). Fields: `id`; `purpose` (the operator's words, shown at every use); `intent`; a **manifest** of exact actions: for each, the tool, the **canonical argument vector** (no globs, no shell string, no wrapper such as `sh -c`, no chaining, no substitution), the **absolute canonical path of the binary and a hash of it**, a fixed scrubbed environment and a fixed working directory; `home` and `machine` it is bound to; `run` (**required**: the run id the operator names at launch); `expires` and `max_uses`. The hash of the canonical manifest is what the operator saw. |
| TA-2 | **`--on-card authorization:<id>`** | A new value beside `deny`, `policy:<file>` and `relay` (016 §3). **The run refuses to start** unless: the record's hash verifies; it is unexpired and unrevoked; it is bound to this run, home and machine; a **verified OS sandbox with the network dropped is in force**; the home was created fresh by the daemon with **no symlinks, mounts or credentials**; and the project is a **disposable snapshot** that is discarded afterwards and persists nothing into any interactive run. A run that an agent started, or that is nested inside one, **may not carry this flag** (it is refused). |
| TA-3 | **Use** | At each Red card the daemon matches the canonical vector and the binary path and hash against the manifest; verifies the record hash, expiry and revocation; and **decrements `max_uses` atomically before the action runs**. A match is approved *by the authorization* and logged as `authorization.used` (authorization id and hash, the manifest entry, the purpose, uses left); every other card is denied as before. Revocation takes effect mid-run. The task summary lists every card approved by authorization. |
| TA-4 | **Training runs** | `delphi train run --authorization <id>` passes it to each run; a report row says its run used an authorization, so a result is never read as an unauthorized run. |
| TA-5 | **The deny-list, enforced in code after matching** | An entry that could match a merge, a pull request, a credential, a network tool, dispatch, or a write to `.git`, hooks, configuration or the authorization store is rejected when the record is made and again at use. **Composition:** a Red entry may not run in a run that also offers a file-write tool, and the project snapshot is read-only to the authorized command, which writes only to a scratch directory. Pairing rules on a single record are not relied on: an Amber tool that writes a payload followed by an authorized command that executes it is stopped by making the first impossible, not by inspecting the pair. |

## 4. Why not a policy file, and why not the relay

`policy:<file>` is an Amber allowlist by design and refuses to load with any other tier, so that a file an agent could edit can never approve a Red action. The relay with step-up (014 §7.1) already lets an operator approve a Red card live; it needs the operator present for each card. An authorization is the operator's approval **given in advance, scoped and bound to one run**, made once through a path an agent cannot write, so a test can run unattended without the approval becoming something an agent can write.

## 5. Threats this design must survive

| Threat | Mitigation required |
| :-- | :-- |
| An agent writes its own authorization into scratch, a worktree or the home | the record is read only from the protected store and checked against the lock; a file anywhere else is ignored and logged |
| Content says "this test is authorized" | authorization comes only from the flag and the protected record; nothing in a model's context can add one |
| A pattern widens the scope | no globs and no shell strings; the canonical argument vector is matched exactly |
| **PATH shadowing:** a binary of the same name earlier on `PATH`, or a working-directory binary, replaces the authorized one | the manifest pins the absolute canonical path and a hash of the binary; the environment is fixed and scrubbed (no `PATH` search, no `LD_PRELOAD`, no `DYLD_*`) |
| **Worktree poisoning:** a build script or test hook alters source, caches or hooks that persist into later runs | the project is a disposable snapshot, read-only to the command, discarded after the run; the command writes only to a scratch directory |
| Approval by composition: write a payload with an Amber tool, run it with the authorized command | no file-write tool is offered in an authorized run (TA-5); the snapshot is read-only |
| The "throwaway" home is a label: symlinks or mounts reach real data | the daemon creates it fresh and checks for symlinks, mounts and credentials at start and at each use; the OS sandbox is mandatory or the run does not start |
| Replay of the record: another run, another day, another machine or home | expiry, `max_uses`, a **required** run binding, home and machine binding, a daemon-kept use counter |
| Time of check versus time of use | the run pins the record hash at start and re-verifies at each use; a counter decrement precedes the action |
| An agent starts, or nests inside, a headless run that carries the flag | a run started by an agent, or nested, may not carry `--on-card authorization:` |
| An agent edits the authorization store, the configuration or the flag of a later run | those paths are in the deny-list and unwritable by agents (TA-5) |
| A persuasive or misleading request | the operator sees the manifest and the dry run first; the agent's prose is collapsed, labelled as data, and cannot change the manifest |
| Prose and manifest disagree | both are shown and the difference flagged; the manifest wins |
| A request written by a span that read hostile outside content | refused at staging at `federated_peer` taint or lower; otherwise the taint is shown at the top |
| Request flooding and operator fatigue | a cap on pending requests, de-duplication by manifest hash, a rate limit, a refusal that is not re-staged, and prose collapsed by default |
| A request approved, then edited by the agent | an approved record is a new operator-made file bound to the request's hash; a changed request is a new request |
| Escape sequences in the request aimed at the operator's terminal (line overwrite that hides the true scope) | stripped on staging and on display; the manifest is rendered by Delphi from structured fields, never from agent text |
| A relay-forwarded request approved through the relay (confused deputy) | authorization needs a local terminal and step-up; a relay cannot approve |
| Alias ambiguity: two names for one tool | tool names are canonicalised before matching and before display |
| A grant outliving its purpose | a maximum lifetime (OD-2); expired records reported in `delphi doctor`; revocation |
| A used authorization erodes "Red always asks" in interactive runs | the amendment is headless-only; every use appears in the operator's summary |

## 6. Slices (one per pull request; none starts before approval and a readable red-team re-review)

| Slice | Contents |
| :-- | :-- |
| S0 | this spec: the operator's approval, and a red-team re-review of v2 in a readable form (the previous Round 2 was intact in the record and unreadable to the Council's reader) |
| S1 | TA-0, TA-0r and TA-1: the request tool and its staging rules, `delphi test review`, `delphi test authorize --from-request`, the protected store; tests for the staging, display and forgery rows of §5 |
| S2 | TA-2, TA-3 and TA-5: the flag, sandbox-or-refuse, the snapshot, canonical matching, atomic use, the deny-list; the 016 invariants re-tested; a test for every remaining row of §5 |
| S3 | TA-4: the training runner flag and the report mark |

**First actions admitted (S1/S2): exactly one kind, a sandboxed shell command inside the throwaway home**, matched by exact canonical vector, offline, over the read-only snapshot. Dispatch, file edits and anything not sandboxed are each a separate later slice with its own red-team review, and are admitted only after the first has run clean (Council `t_0003`).

## 7. Acceptance criteria

- **AC-1** A headless run with a valid authorization approves a matching Red card and records `authorization.used`; the same run without it denies the card.
- **AC-2** Every row of §5 has a test that fails against a version without its mitigation.
- **AC-3** A merge, pull-request, credential, network, dispatch or `.git` card is denied under any authorization.
- **AC-4** Without a verified sandbox the run does not start.
- **AC-5** `AGENTS.md` and `specs/016` §3 are amended in the same pull request that builds S2, so the text says what the code does: "Red actions are asked of the operator, live or, in a headless run only, through an authorization the operator ratified for that run."

## 8. Non-goals

Authorizing anything for an agent's own benefit. A standing "allow red" mode. Authorizing anything in the operator's real home or outside the sandbox. A shared or exported authorization. Any change to interactive runs.

## 9. Decisions

**Direction from the operator, 2026-11-07:** a test the operator authorizes may run Red actions headless; an agent may request one with context and risk. **Council advice (`t_0003`):** staged approval, the sandboxed-shell slice first, with the P0s above written in before implementation. **For the operator:**

- **OD-0** Approve the sandboxed-shell-only first slice, with the P0s above closed in this text. [Recommended.]
- **OD-1** Which commands the first authorization may name. [Offline test runners already built in the throwaway home, nothing else.]
- **OD-2** Maximum lifetime and uses. [24 hours and 50 uses, shorter for the first slice.]
- **OD-3** Whether creating an authorization requires the operator's step-up. [Yes.]
- **OD-4** Whether one authorization may cover several runs of a study. [No: a run binding is required, so a study needs one per run or a launcher that mints them from one operator act. To be decided with S3.]
- **OD-5** How many requests may be pending, and for how long. [Five per mission, expiring after seven days.]

## 10. Known limits

An authorization is only as strong as the protected store, the OS sandbox and the lock around them; if an agent can write to the store, or the sandbox is weaker than assumed, nothing here helps, which is why the sandbox is mandatory and the store is tested for forgery before anything else is built. A test runner that loads fixtures or configuration can execute arbitrary code; the sandbox, the read-only snapshot and the dropped network are what contain that, not the manifest. It authorizes an *action*, not an *outcome*.
