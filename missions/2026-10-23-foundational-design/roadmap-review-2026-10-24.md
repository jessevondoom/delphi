---
id: roadmap-review-2026-10-24
title: "Roadmap review and plan: where the build stands, what is unproven, what comes next"
author: builder
timestamp: 2026-10-24T08:30:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [roadmap, review, plan, step-6, step-7]
---

# Roadmap review and plan (2026-10-24)

You asked for items 1 to 3, then 5, 4, 6, and a review of 7 with a plan. This is that review. It separates what is built **and shown working** from what is built **but not proven**, because the two are easy to blur in a long run of green tests.

## 1. Status against the roadmap

| Item | State | How far it is proven |
| :-- | :-- | :-- |
| 1. v0.1.0 and dogfood | Released; installer, checksums, attestation and `self-update --check` verified from the real release. | `brew install` has **not** been run. The Step 2 go/no-go ("3 real tasks by an operator on a real project") has **not** been done: the live builds so far were scripted by me. This is yours to do. |
| 2. Step 3 cleanup | Red-team sweep triaged and its verified findings fixed; flaky-test rerun; PR merge mode; Linux `bwrap` CI job. | The `bwrap` job caught a real bug (merged-`/usr` links missing inside the sandbox) and now passes on Ubuntu. Sweep fixes each have a test. **Live Gemini OAuth is unverified** (needs your OAuth client). |
| 3. Step 4 Council | `/council`; blind barrier held in memory, tested by checking the log during Round 1; heterogeneous seats; strict Round 2 format; deterministic escalation; daemon-written Minority Report with a lint rule. | One **real** Council ran across Claude and Gemini ([Council 007](councils/007-narrator-process-or-pane.md)). The Orchestrator does not yet frame the proposition or pick seats, and the Builder buildability check is not wired. |
| 5. Step 5 Observe and recall | `delphi kb` (FTS5; `[REV-SEC-04]` ranks `REVIEW.md` first on this repo), `delphi update`/`rollback` (.upstream mode), and the Narrator (separate read-only process, four full-window views, snapshot-then-tail). | The Narrator is tested by rendering into a buffer and by a real attach through a pty with `--once`. **Nobody has watched it run interactively**: key handling, resize and how it looks with a live Council are untested by a human. Signed releases (Ed25519, `specs/006` §4.1) are replaced by SHA-256 plus GitHub build attestations; that is a deviation, not an equivalent. |
| 4. Step 3b slice 1 connectors | Catalog, in-tree MCP client, pins through `/ratify`, account pinning, the read/draft/write pipeline, payload store, `delphi connectors`, doctor check. | Tested only against a scripted MCP server. **Never run against Google, Slack or Zoom.** The spec's server URLs are "reported", not confirmed. |
| 6. Step 6 usage-gated | Not built, on purpose: the roadmap says each item starts only on a recorded need. What I built instead is the **recording**: `delphi needs` shows the rule, the evidence and whether it is met; empty searches are logged as candidate misses (`delphi kb miss` confirms one); customized files an update set aside are logged. | Today every rule reads "not yet". That is the correct answer, not a gap. |
| 7. Deferred | Reviewed below. | |

## 2. Things I would not trust yet

1. **The connectors, end to end.** The pipeline's policy (taint, approvals, write-once, pins) is solid against a mock. Everything provider-specific is unknown: real tool names and result shapes, whether Google's remote MCP servers exist at those URLs, OAuth scope and refresh-token behavior. Expect the first live run to find work.
2. **The Narrator as an interface.** It is the least exercised piece relative to its size.
3. **Findings from the red-team sweep that I did not fix.** Verified-real but still open, roughly by severity:
   - `oauth2.rs`: device-code polling never sends the client secret and aborts on one transient error; discovery is not bound to the configured issuer; a rotated refresh token that cannot be stored throws away a good sign-in.
   - `credential.rs`: the gcloud token's expiry is invented (now + 50 min); a transient refresh failure discards a still-valid token; no single-flight on concurrent refresh.
   - `harness.rs`: a non-UTF-8 line ends the reader and can hang `Session::next`; the 8 MiB line cap applies after buffering; `agy` tool states other than ACTIVE are all reported as completed; `Tools::None` is silently ignored for `agy`; the system prompt travels in argv.
   - `finalize.rs`/`build.rs`: gates 1 to 4 can go stale while a merge card waits (head and sha are re-checked, gates are not); an audit that could not run is fed back to the Builder as a defect.
   - `setup.rs`: a late error discards earlier edits after secrets were stored; a blank price silently disables budget reservation; `exec` defaults are written without review.
   - `install.sh`: attestation verification is skipped when `gh` is absent (a warning, not a failure).
4. **Spec 005b and the wireframes are not in the repo.** They are untracked in your working tree, and they fail the repo's own lint (theme names outside `themes/`, absolute home paths, missing frontmatter on `ui-wireframes-and-blueprints.md`). Committing them as they are would turn CI red. The Narrator follows 005b §4; the Console decks, Decision Inbox, header badge state machine and chromeless viewer from the same spec are **not built**.

## 3. Item 7, the deferred list

| Item | Recommendation | Why |
| :-- | :-- | :-- |
| Federation planes 1 and 2 (`specs/007`) | Keep deferred; revisit only with a second operator or machine. | Large surface (peer trust, sync) with no user today. `delphi needs` watches the one measurable signal (a team repo with a remote). |
| Direct OpenAI API driver | Keep deferred; build when you want a third family. | The dialect layer already speaks OpenAI-compatible chat; the missing parts are the family's capability probes and a catalog. Roughly a day, once there is a key to test with. |
| Harness session resumption | Keep deferred. | A harness span is not resumable by design; Delphi resumes the *task*. Worth doing only if harness spans get long enough to hurt. |
| Gemini Builder | Keep blocked on `gemini --acp`. | `agy` has no approval bridge and cannot disable tools, so a Builder on it is refused (by design, tested). |
| Vectors, `/retro`, diff3, `sync`, watches, `/rewind` | Gated by `delphi needs`. | See §1, item 6. |

## 4. Proposed order from here

**You, first (nothing I can do substitutes):**
1. Run the Step 2 go/no-go: three real tasks through the console on a real project, and say if any felt wrong.
2. Decide Council 007 (separate Narrator process: the seats were unanimous).
3. Create the Google OAuth client, so that `delphi auth login google`, `delphi connectors test`, and the live Gemini check can be run.
4. Tidy and commit `specs/005b` and `ui-wireframes-and-blueprints.md`, or tell me to.
5. Try `delphi narrator` next to a running `/council` or `/build`, and tell me what is wrong with it.

**Me, in this order, once the above gives me ground truth:**
1. First live connector run (Google Drive read) and fix what it shows; then the formatters, the promotion card (`specs/010` §5.3) and the harness bridge.
2. The open sweep findings above, starting with the `oauth2.rs` and `harness.rs` ones.
3. Whatever the Narrator trial turns up, then the group-chat Council view from `ideas.md`.
4. The Console half of 005b (decks, inbox, header states), as its own step with its own tests.
5. Slack and Zoom when their admin steps are done.
6. Step 6 items only as `delphi needs` says they are met.

## 5. Decisions this run made on its own

Each is cheap to reverse; tell me if one is wrong.
- The Narrator keeps a one-line agents strip in its header and puts the full agents table in Archives & Spend, reconciling `specs/005` (a persistent pane) with `specs/005b` (no tiled panes).
- Council "unresolved" means every P0 or P1 a seat lists, since seats cannot yet mark an item as answered; that errs toward escalating.
- Connector tool wire names are `<connector>__<tool>`, because some model APIs reject a dot.
- `delphi update` surfaces refreshed release files as ordinary ratification proposals, which required `base/` to count as a source of proposals.
- Thresholds in `delphi needs` (10 finished tasks, 3 set-aside files) are my defaults; the vector rule (50 missions and a confirmed miss) is D-01's.

## 6. Update, 2026-10-25: what changed after this review

This review is a snapshot and its body is left as written. Two of its statements no longer hold:
- **"Vectors" is no longer Step 6 or gated.** The operator reopened D-01 on 2026-10-24 and directed the work that followed. Knowledge search now has local embeddings, a reranker, preloaded vectors for the shipped documents, the agents and skills in the index, and a refresh on every search (`specs/004` Amendments 004-A1 to 004-A3). The "50 missions and a confirmed miss" rule in the last decisions line above is spent; `delphi needs` no longer lists vectors.
- **`delphi kb` is no longer FTS5 only** (the Step 5 row in §1). `delphi kb eval` measures it, and `dev/kb-eval.json` with `dev/kb-eval-holdout.json` are the question sets; the second is never tuned on.

Still true: Step 6's other items (coaching automation, diff3, sync, watches, rewind) are gated by `delphi needs`.
