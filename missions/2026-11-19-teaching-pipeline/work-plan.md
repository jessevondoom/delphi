---
id: work-plan-teaching-pipeline-2026-11-20
title: "Work plan: the teaching pipeline (spec 129)"
author: architect
timestamp: 2026-11-20T00:00:00Z
source_type: internal_generated
confidence: low
status: draft
source_mission: 2026-11-19-teaching-pipeline
tags: [plan, learning, lessons, app, spec-129]
---

# Work plan: the teaching pipeline (spec 129)

Status (2026-11-21): **S0 to S3 merged; G1 is the prioritized next item.** Spec 129 is `status: approved` (operator, 2026-11-20); S0 #793, S1 #807, S2 #810 and S3 #813 are on `main`. No Council has read the spec's text yet: the operator marked G1 as prioritized future work (2026-11-21), to run before anything else of this plan resumes. Open for the operator: S3's live proof, OD-4 (section 6), S4a. Follow-ups found while building are in section 9. This file is the one place that records what was decided, what changed in the spec, what is open and in what order work goes. Written 2026-11-20 with the operator; reviewed against the code on `main` (43f58275) later that day, which added the estimates, the order and section 8. It is a plain note, not a governed file. Hours are estimates, not measurements.

## 1. The aim in one paragraph

Agents improve by carrying better standing instructions with provenance, chosen with the operator: context, not weights. One pipe (note or request, then a card, then accept, then a lesson on probation, then kept or unteached) with three ways in (the operator asks to teach, a role asks to be taught, a recurring host note) and two places to accept (a terminal, or the app). A host's chat can draft a card and say where to accept it; it can never accept one.

## 2. Decisions, in order

| Date | Decision | Where it lands in the spec |
| :-- | :-- | :-- |
| 2026-11-19 | Build nothing yet; plan first; paste laundering into the app is an accepted risk and ignored | section 13 |
| 2026-11-19 | Revoking an app key must stop running work | section 10.2 |
| 2026-11-19 | Fix the app socket's denial of service | section 10.1 |
| 2026-11-19 | A host-chat confirm with credits, a daily ceiling and terminal top-ups was designed, then dropped | section 14 |
| 2026-11-20 | Probes before the spec (done, section 3 of the spec); `!` in Claude Code has no tty (operator ran it) | sections 3, 14 |
| 2026-11-20 | **Accept at a terminal or in the app only**; no host-chat confirm, no top-ups | sections 1.1, 6 |
| 2026-11-20 | **OD-7: option (a) at full operator scope**: the app may accept operator-scope lessons, bounded as in section 6 item 8, with the red-team pass before S4 | section 6, OD-1, OD-7 |
| 2026-11-20 | **OD-8:** make the one-click card accept in the app safe; read as: the accept is in the first cut and secured, the typed composer is not (operator to correct if wrong) | section 6 items 8 and 9, OD-8 |
| 2026-11-20 | Look into the Gemini seat failures before work starts; the operator said yes to three short test calls | section 5 below |
| 2026-11-20 | **OD-9: the recommendation.** Terminal `yes` arms `review` in v1; the daemon-shown macOS confirmation is probed in S4a and preferred if it holds; operator-scope accepts need an arming no older than five minutes | section 6 items 8 and 9, OD-9 |
| 2026-11-20 | "merge 793": PR #793 to be merged as a draft spec so a Council can read it from the main checkout | the merge itself is the operator's act, run with the operator's words |
| 2026-11-20 | The merge flow refused a draft spec spanning more than its bounded exemption; the operator then said **"approve 129 and add the trailer"**: spec 129 is `status: approved`, `approved_by: operator (2026-11-20)`, commits carry `Implements-Spec: specs/129-*.md`. No Council has read the text; a Council and the spec 120 §5 red-team pass remain required before S4; S1 to S3 may start | frontmatter, sections 12 and 15 |
| 2026-11-20 | #793 merged (05:33 UTC). Spec 128 merged with S1 to S3 built (#792). Spec 131, the Gemini seat fixes, approved by the operator ("131 is approved") and merged as a spec (#794); none of its slices is built | section 5 below |

## 3. What changed in the spec since the first draft

1. Accept binds to the card id and a hash of the daemon-rendered text; the checks run again at the accept.
2. Recurrence is labelled host-supplied and capped; it can only make a card, never load a lesson.
3. The lockout fix keeps cost per peer, per key name and for the socket as a whole, so a same-user process cannot rotate around it.
4. Revoke has a race test and a named event.
5. Measure first; terminal accept before app accept; `review` last.
6. The app may accept operator-scope lessons (OD-1, OD-7).
7. The one-click accept is secured in layers: daemon-rendered text, per-connection arming of `review`, a limit, a digest line, one-click unteach (section 6 items 8 and 9).
8. The typed composer `app.lesson_teach` is out of the first cut.
9. A new slice S4a probes whether a daemon-shown macOS confirmation can arm `review` without a terminal.
10. 2026-11-20, after the merge: three places still listed the composer as part of S5 (section 5.1 item 3, the app bullet of section 7, the S5 row of section 12). They now say what OD-8 and section 10.3 already said: no composer in the first cut. The composer was added to the spec's deferred list. No decision changed.

## 4. Slices, order and gates

### 4.1 The table

Order is the order of starting. "Agent" means an agent can build it and open the pull request; every merge is the operator's (spec 094).

| Order | Slice | Depends on, and gate | Estimate | Exit check | The operator does |
| :-- | :-- | :-- | :-- | :-- | :-- |
| done | S0 the spec and the amendment text | none | done | #793 merged 2026-11-20 | nothing more |
| **next (prioritized, 2026-11-21)** | G1 a Council that reads the spec's text | spec on `main`; context under about 150 KB while a Gemini seat sits (spec 129 is 41 KB, spec 120 is 33 KB) | 2-3 h | a ruling in which every seat gave a readable Round 1 and Round 2; findings applied or answered in the spec | asks for the Council; reads the ruling |
| done (#813, 2026-11-21) | S3 `G:`, `delphi teach`, `learn lessons`, `learn unteach`, the card view in `learn list`, the goal on `learn keep` | S0 | 14-18 h | AC-1, AC-2, AC-5 and AC-6 at a terminal, AC-7; then the live proof (**not yet run**) | runs the live proof (teach, accept, `pack_hash` changed, the next `get_agent` carries it, unteach); merges |
| done (#807, 2026-11-21) | S1 the lockout replaced | S0 | 8-10 h | AC-8: a valid key connects during a flood of refusals; a pairing that waits for its typed code is not counted against its peer | merges |
| 1 | S4a the macOS confirmation probe (no product code) | S0 | 2-3 h | a yes or no, written into the spec's section 6 item 9 and the tracker | runs it at the Mac: the answer depends on that machine's privacy permissions |
| 2 | S6 the `teach` and `ask` labels on `propose_note`, the skill lines, the brief's count line | S3 | 5-8 h | AC-3, AC-10; invariants 9.1, 9.2 and 9.9; at most two `ask` notes a session | merges; cuts the release that carries S3 and S6 (the two-week window starts there) |
| done (#810, 2026-11-21) | S2 revoke cancels running work | S0; after S1, because both change `server.rs` and `app_socket.rs` | 8-12 h | AC-9: the race test; `requests_cancelled` and the ids recorded; a rebuild either stops or is named as running to its end | merges |
| 3 | S7 `lesson.served` and the counts | S3; best after S6 | 8-12 h | a serve is counted from each place a pack is given (4.4); `learn lessons` shows serves, last serve and repeats; "worth a look" lists | merges |
| 3 | G2 the red-team pass spec 120 §5 names, on the `review` grant and arming | G1's ruling; S1 and S2 merged or in review | 2-5 h | a readable red-team answer on section 6 items 8 and 9 and section 10.3, with fixtures named for S4 | asks for it; decides on its findings |
| 4 | S4 the keep logic into the engine; the `review` methods with arming; the limit; the digest line; the gate file | S1, S2, S3 merged; G1 and G2 done; the numbers of the window read | 20-26 h | AC-4, AC-5 and AC-6 through the daemon; invariants 9.3 to 9.6 and 9.11; the gate-file test | reads the numbers and says go or stop; merges **at a terminal** (`crates/delphi/tests/app_grants.toml` is a gate path); runs `delphi app grant <name> review` |
| 5 | S5 the app: inbox, per-role lessons, unteach, the card actions. No composer | S4 | 14-20 h | app tests; then one accept and one unteach in the real app against the real daemon | does the accept and the unteach in an app built from that code; merges |

Total: 83 to 117 hours of building and gate work, plus a two-week window that no amount of building shortens.

Deferred, in likely order: signals other than a recurring host note; a mirror of counted evidence and a legibility card; the typed composer in the app; teaching a skill; the app as the operator's working place (its own spec); an in-chat accept only on measured need.

### 4.2 Order, and why

- **Wave 1, now, three tracks.** S3; S1; G1. S4a whenever the operator is at the Mac. S3 touches the `delphi` crate and `customize.rs`; S1 touches `server.rs`; they do not meet.
- **Wave 2, as soon as its parent merges.** S6 straight after S3; S2 straight after S1.
- **Wave 3, inside the window.** S7, G2, and whatever of S2 is left.
- **Wave 4.** S4, after the window and the gates. **Wave 5.** S5.

What differs from the first reviewer's order ({S1, S2, S3, S4a} now; {S6, S7, Council and red-team} in the window; S4; S5), and why:

1. **S3 is the critical path, not one of four.** The window is two weeks and starts when the operator can use S3. If S3 and S6 are in a release by about 2026-11-24, S4 can start about 2026-12-08. Every day S3 slips moves S4 and S5 by a day; nothing else in wave 1 does.
2. **The window starts at a release, not at a merge.** The operator works with the installed `delphi`; a merged S3 that is not released collects no numbers.
3. **S6 goes to the start of the window, not into it.** Until S6 exists `propose_note` has no label, so no host can make a `teach` or an `ask` card and the window would measure only `delphi teach` and notes of the old kind. The numbers S4 waits for (cards kept at a terminal, host notes that recur) need the host side.
4. **S2 follows S1, not beside it.** Both rewrite parts of `crates/delphi-core/src/server.rs` and `crates/delphi-engine/tests/app_socket.rs`, and both edit spec 120's file. `server.rs` also changed in three commits on 2026-11-20 (pairing by code #797, the update grant #800 and its red-team fixes). In parallel they cost a rebase each; in sequence they cost nothing, and neither is on the critical path.
5. **The Council read (G1) moves to wave 1.** It reads text that S1 to S3 are about to build, so a finding is cheapest now. It does not need spec 131 built: the spec and spec 120 together are 74 KB, under the interim 150 KB rule, and spec 128's Round 2 fix is merged. If the Gemini red-team seat still gives no readable answer, G1 and G2 wait for spec 131's S2 to S4 and that is said, not passed over.
6. **G2 stays in the window,** after G1, because it should read the arming design S4 will build (4.4, S4) and S4a's answer.

### 4.3 Estimates, each with its reason

| Slice | First reviewer | Here | Reason |
| :-- | :-- | :-- | :-- |
| S1 | 8-10 h | 8-10 h, kept | The peer's process id and a ceiling on unproven connections (`max_unauth`) exist already. The work is the per-peer and per-name wait, removing `locked_until`, the aggregated log line, replacing the test `five_refusals_close_the_socket…`, and the words in `app_cmd.rs`, `app_audit.rs`, the app's `client.rs` and `bridge.rs`, and spec 120 §3 item 4. |
| S2 | 6-10 h | 8-12 h, raised | Revoke is in `delphi-core` and the running work is in `delphi-engine`, so it needs a new method on the `Extension` trait. `Tally.running` holds the work without its request id. A rebuild is a spawned task with no handle. Taking a grant away (line 1127) needs the same cancel. An existing test asserts the opposite (`…_is_not_cut_by_a_revocation`). |
| S3 | 12-16 h | 14-18 h, raised | `Provenance` requires the staged note a lesson was kept from, and `delphi teach` has none. No function removes a lesson. `learn.unlearned` needs a log the command can write while the daemon runs. Four command surfaces mean new help, a regenerated CLI reference and man page. The goal adds bytes to a file capped at 6,144. |
| S4a | 2-3 h | 2-3 h, kept | A probe with a written answer. It can end early with "no". |
| S6 | 3-5 h | 5-8 h, raised | It is not only skill text. `propose_note` takes `role`, `text`, `evidence` and `model` and refuses any other field, so the label is a tool-schema change, a daemon mapping from label to card kind, and the cap of two `ask` notes. Worker files and the catalog are regenerated, and hosts need a plugin update to see the new skill text. |
| S7 | 6-10 h | 8-12 h, raised | There is no one serve point (4.4). The host's `get_agent` is answered in the MCP host process, not in the daemon, so the serve has to be reported to the daemon. |
| S4 | 20-26 h | 20-26 h, kept | The move is smaller than the spec implies: `parse_lesson`, `lesson_preview` and `apply_lesson` are already in the engine, and what moves is about 300 lines of planning and applying plus their tests. That saving is spent on arming, which lives in `delphi-core`'s per-connection state, on routing six new methods through the extension, on keeping a staged-only note through the daemon's own log, and on the digest line in the command dispatch. |
| S5 | 12-18 h | 14-20 h, raised | The composer was one form; removing it saves little. Added since the spec's first estimate: the armed and not-armed states, edit and accept, the 26th-lesson and conflict actions. Each method is added in three places (the Rust client's `Method` list, the bridge, the sample-data bridge). No screen in the app writes memory today. |
| G1, G2 | 4-8 h | 4-8 h, kept | Two short runs and the time to apply findings. More if a seat is unreadable and the run repeats. |

### 4.4 Notes per slice (what the code says the slice must settle)

**S1.**
- Keep `max_unauth` as the socket-wide ceiling and add the per-peer one beside it.
- A connection that sent `app.pair_request` holds an unproven slot for as long as the operator takes to type its code (`pair_request_ttl`). The per-peer limit must leave room for it, and a wrong code must not add to the wait of a valid key.
- The flood test must run with refusals from the same process id, from rotating process ids and under one claimed key name, since the spec's reason for three counters is that a process can rotate its id.
- Edit spec 120 §3 item 4 in this pull request and run `delphi specs compile`.

**S2.**
- The cancel for research exists (`Jobs::cancel(id, session)`, `research_jobs.rs:458`). The watcher that follows a run already writes `cancelled`; it needs the reason `revoked`.
- For a rebuild, prefer a flag the rebuild reads over ending its task: `topic_rebuild` journals the tokens it spent after the build returns, and a task ended from outside would skip that line. The rebuild already asks a gate before the writer's model call (`library_topics.rs`, `gate()?`); the flag can be read there and once more before the page file is written. A rebuild past its last check runs to its end and its `app.request` event says so. This is the choice the spec leaves to S2.
- The second check of the grant after admission goes in `app_request_checked`, after `tally.admit`.
- An update in flight (`app.update_apply`, merged in #800 after the spec was written) is also work a key started. See question 3 in section 6; until it is answered S2 leaves an update alone and says so in the event.

**S3.**
- `delphi teach` writes with the pinned write `learn keep` uses, so it works while the daemon runs. Say so in its help.
- Decide where `learn.unlearned` and the teach record are written when the daemon holds the mission's log. `delphi customize note` already tries each mission's log until one opens (`customize_cmd.rs`); that or a call to the running daemon. The slice states which, and tests it with a daemon running.
- `learn lessons` in S3 shows provenance and age only. Its counts arrive with S7; the column says "not counted yet" until then.
- With `[memory] provisional = false` a host note is only staged and `learn keep` needs the daemon stopped. The default is true. The card view prints the right command for each case.
- Tests the acceptance criteria do not name and S3 should add: a `G:` inside the text of `T:` or `D:` is not read as a goal; a goal that would take the file over 6,144 bytes is refused; a secret shape in a goal is redacted (invariant 9.10); `--goal` and a `G:` in the text both given.
- Live proof, added step: after the teach, a worker file that inlines the role's pack still holds the old `pack_hash`. Check that the worker's `get_agent` call with `if_hash` returns the new pack, and record it.

**S4a.**
- State what was tried: a dialog the daemon starts, answered by a second process of the same user by scripting the interface and by sending key events, with and without the Accessibility and Automation permissions.
- Also record what happens when the dialog's own process is ended by another process: it must read as "not armed".

**S6.**
- AC-3 belongs here, not to S3: until the label exists every host card is kind `other` or `recurring`.
- The label is a closed list (`teach`, `ask`). It sets the card's kind and nothing else: origin and trust still come from the channel (invariant 9.2). Test a host call that sends an operator origin.
- Regenerate worker files and `docs/reference/catalog.md`.

**S7.** A role's lessons reach a model from three places, and the premise check is to name which are counted:
1. a host's `get_agent` (`mcp_host.rs:1073`), answered inside the MCP host process. It already reports what it read to the daemon on a short detached call (`host.reads_seen`); a serve can travel the same way. Such a report can be lost, so the count is a floor and is labelled so.
2. a role the daemon runs itself (`role_call.rs`, from the snapshot). Counted directly.
3. a `get_agent` answered `unchanged` because the host holds the pack. Count it as a serve: the host is using the pack.
- The event carries the role, the `pack_hash` and the ids of the lessons in the pack, never their text.
- The spec has no acceptance criterion for S7. The slice writes its own in its pull request: one test per serve point, and one that an unteached lesson is no longer counted.

**S4.**
- **Which connection does the terminal's yes arm?** `delphi app arm <name>` names a key, and arming is per connection. A process that copied the key file can open a second connection under the same name and ask to be armed at the same moment. The pairing flow already solves this shape: the app shows a short code and the operator types it at the terminal (#797). See question 4 in section 6. G2 must read whatever is chosen.
- A staged-only note is closed by an event in its session's log, and the daemon holds that log. The daemon-side keep writes through the daemon's own log writer; the command-line keep keeps its present path.
- The limit of 20 accepts an hour, the digest line and the five-minute rule for the operator scope have no acceptance criterion of their own. Add a test for each (the five-minute rule is invariant 9.11).
- This is likely over the size a pull request stays reviewable at (about 800 changed lines). A split is proposed in question 5 of section 6.
- Keep the pull request free of every other gate path. `app_keys::GRANTS` and `app_grants.toml` must change together; `crates/delphi/tests/app_grants.rs` fails otherwise.
- The numbers read before S4 starts go on one page in this folder: cards made, kept, expired; host notes that recurred; lessons taught at a terminal; whether any real model called `propose_note` with a label.

**S5.**
- "The real app" means an app built from the S5 code. The released app is built only in the release workflow, so the exit check is run on a local build or after a release; say which in the pull request.
- The sample-data bridge needs cards and lessons, or the screens cannot be tested without a daemon.
- Anything that moves obeys reduce motion.

### 4.5 Risks

1. **The window measures nothing.** If S3 is not released, or S6 lands late, two weeks pass with no host cards. Mitigation: 4.2 items 2 and 3.
2. **The slice is not used.** The spec says a slice the operator does not use is stopped, not extended (section 17). S4 and S5 are 34 to 46 hours that depend on that reading. The reading is the operator's (question 6).
3. **A Gemini red-team seat is unreadable again.** Then G1 and G2 are not met and S4 waits for spec 131's slices. S1 to S3, S6 and S7 do not wait.
4. **Other sessions build the same slice.** It has happened (2026-10-28). Before each slice: `gh pr list`, recent `main`, the tracker.
5. **`server.rs` is busy.** Spec 120 work (pairing by code, the update grant) changed it three times on 2026-11-20. S1 and S2 rebase on `main` before review.
6. **The line anchors drift.** The spec's and this plan's line numbers were read on 2026-11-20; a builder reads the function, not the number.
7. **S4a says no.** Then v1 stays with the terminal for arming, which is what the spec builds anyway. No slice depends on a yes.

### 4.6 What only the operator can do

1. Ask for the Council (G1) and the red-team pass (G2), and decide on their findings.
2. Run S3's live proof.
3. Run S4a at the Mac.
4. Cut the release that starts the window.
5. Read the numbers after two weeks and say go or stop for S4.
6. Merge every pull request; S4's at a terminal, because it changes a gate path.
7. Give a key the grant: `delphi app grant <name> review`.
8. Do the one accept and the one unteach in the real app (S5).
9. Answer section 6.

## 5. The Gemini seat problem (checked 2026-11-20; spec 128 built and merged, spec 131 approved and not built)

**What happened.** In three Councils (`t_0002` to `t_0004`) the Gemini seats (carrier `agy`, model `gemini-3.8-flash-medium`) produced nothing usable: red-team `t_0002` was discarded because its run changed files in the project checkout; red-team `t_0003` and `t_0004` "finished without the schema-validated answer"; the strategist abstained in both. Only the Claude architect has ever really sat, so "two families met" has been nominal.

**What the daemon's own log shows.**
- The gate was reachable at least some of the time: the daemon logged policy denials (a shell command `pwd && ls -la`, and reads of the CLI's own transcript file under `~/.gemini/antigravity-cli/brain/`). Those are correct denials, and they show the seat was looking for something it did not have.
- The red-team's last message in `t_0004` says its prompt was cut by 76,525 bytes and the task was gone from the end, and that every tool call was denied with "the gate could not be reached". That literal text is produced by the hook when it cannot connect to the span's socket (`crates/delphi-engine/src/bridge.rs`, lines 542 and 549); a connection that never reaches the daemon cannot appear in the daemon's log, so the log can neither confirm nor rule it out.
- Delphi has no prompt cap of its own (searched). The seven or eight context files given to the Councils add up to about 255 KB; the Claude seat took them (about 119k tokens), the Gemini carrier appears to have cut the end of its prompt, and the task and output contract sit at the end ([H-006] sandwich prompting). With the task cut off, a seat improvises: in `t_0003` the red-team audited spec 120 and nothing else.
- Known earlier (memory notes): a seat given no source inline tries a shell command, headless mode auto-denies it and the run ends empty; "red team"/"attacker" wording trips Gemini's safety filter; Flash marks nearly every finding verified, including false ones.

**Hypotheses, most likely first.**
1. **Oversized prompt:** the carrier cuts the tail and the seat loses its task. Test: the same one-line question with no files, with one small file, and with the seven large files.
2. **A tool-connection failure** once the seat starts calling tools: the hook cannot reach the span's socket (possibly a changed carrier sandbox or the bridge ending early). Test: a seat told to call one allowed tool, then read the daemon's gate log and the hook's stderr.
3. **The seat writes files** (the `request_research` tool made files in the checkout in `t_0002`): the daemon discards its output. Fix is policy: no research tool for Council seats, or a scratch checkout.

**Test run 2026-11-20 with the operator's yes (three `ask_family` calls, Gemini Flash, no tools): hypothesis 1 is confirmed.**

| Call | Context given | Result |
| :-- | :-- | :-- |
| `t_0005` | none | answered (20k tokens) |
| `t_0006` | one file, spec 120 (18 KB) | answered correctly: title and what section 3 item 4 says (24k tokens) |
| `t_0007` | the seven large files the Councils used (about 255 KB) | the carrier cut **69,110 bytes** from the tail, mid-sentence in spec 120 §12 (the fifth of seven files); the seat reported that the task, objective and output contract were missing (144k tokens) |

- So the carrier delivers about 186 KB of a prompt (105, 102, 108, 098 and most of 120 fit) and drops the rest, **including the task, which sits at the end**. That explains `t_0003` and `t_0004`: no task, so the red-team improvised an audit of spec 120 or reported a "block", and the strategist returned an empty answer.
- A seat with no tools works (`ask_family` offers none); the Councils' seats are offered 33 tools with a gate. Hypothesis 2 (the gate or hook failing) is **not shown**: the truncation alone explains the symptoms, and the "gate could not be reached" line may be the lost seat's own wording or a real connect failure; the daemon cannot see the latter. It stays open until a Gemini Council seat is run with a small context and its tool calls read.
- `t_0002` is a different cause, and **hypothesis 3 as first written here was wrong**: spec 128 shows the files under `research/topics/` were written by Delphi's own research writer for a research run the host (this session's drafter) had started earlier, not by the seat; the checkout guard compared `git status` before and after the seat and blamed it. The fix is spec 128's ledger of Delphi's own writes, not a limit on the seat's tools.

**Where the fixes are, as of 2026-11-20.**

| Fix | Specified in | State |
| :-- | :-- | :-- |
| A checkout guard that knows Delphi's own writes; Round 2 without tools; one retry for an empty finish | spec 128 | approved 2026-11-19; S1 to S3 built and merged (#792); verified live 2026-11-20 (tracker #801, merged) |
| 1. A per-carrier input budget, 150,000 bytes by default for an unmeasured carrier, counted before the seat starts; over it, excerpts and a note in the receipt | spec 131 S1, S2 | approved 2026-11-20 (#794 merged); not built |
| 2. The task at the top and the bottom of the prompt for a budgeted carrier | spec 131 S3 | not built |
| 3. A seat that says it was cut counts as not heard; `require_families` counts seats that were heard, not seats that were started | spec 131 S4 | not built |
| 4. Reading tools only for a Council seat | spec 131 S5 | not built |

Spec 131 notes that the 186 KB figure is file text only, so the whole-prompt limit is probably higher, and that truncation does not by itself explain the "gate could not be reached" line, which stays open.

**Until spec 131's S2 to S4 exist, a Council that includes a Gemini seat is given under about 150 KB of context files in total.** G1 and G2 of section 4 fit inside that.

## 6. Open questions for the operator

Open:

1. **OD-8:** was "one-click cards" meant to include the typed composer? Read as no, and the spec, the tracker and this plan now all say no. *Recommendation:* leave it out; it is first on the deferred list after the evidence mirror.
2. **OD-2 to OD-6** take the spec's recommendations unless the operator says otherwise. *Recommendation:* let them stand.
3. **Does revoking a key stop an update that key started?** `app.update_apply` (#800) merged after the spec was written, and section 10.2 says "every request the key started". An install ended halfway can leave a broken program. *Recommendation:* no. An update in flight runs to its end, and the `app.revoked` event names it as left running. S2 builds that unless told otherwise.
4. **How does the terminal's yes pick the connection it arms?** OD-9 decided "terminal `yes` arms `review`". Two connections can hold the same key name, and only the app's should be armed. *Recommendation:* the app shows a short code when it asks to be armed and the operator types that code at `delphi app arm`, as pairing does. It is one more thing to type, about once per app session. The alternative, arming the oldest open connection of that name, is weaker and is not recommended.
5. **May the part of S4 that only moves code start before the red-team pass?** S4 as one pull request is likely too large to review well. *Recommendation:* two pull requests. S4-1 moves the keep logic into the engine with no new method, no grant and no change in behaviour; it may be built in the window and merged like any other. S4-2 adds the `review` methods, arming and the gate file; it waits for G1, G2 and the numbers, and the operator merges it at a terminal. The spec's gate ("before S4") then applies to S4-2, which is where the new authority is.
6. **What do the numbers have to show for S4 to start?** The spec says they are read again and that an unused slice is stopped; it gives no threshold. *Recommendation:* no fixed number. The operator reads the one-page count (4.4, S4) and says go or stop; a window with no lesson taught and no card kept is a stop unless the operator says the terminal step itself was the obstacle.
7. **Does the two-week window start at the release that carries S3 and S6?** *Recommendation:* yes, and that release is cut as soon as both are merged.
8. **A second red-team read, of S4-2's code before it merges?** Spec 120's own grants were handled that way (K1, K4, and the update grant in #800). The spec requires only the pass before S4. *Recommendation:* yes; it is an hour or two, and the first pass can only read a design.

Answered:

- 2026-11-20: OD-9 (the recommendation); the Gemini test calls (yes, run); the merge of #793 (yes, "merge 793"); the approval of spec 129 ("approve 129 and add the trailer").
- 2026-11-20: where the Gemini fixes are specified. In a spec of their own, spec 131 (#794, approved and merged); the checkout guard in spec 128 (#792, merged).

## 7. Measured so far (section 3 of the spec)

182 live staged notes, one correction; 12 correction-cue turns and 25 praise words in 1,228 human turns; the harness labels every turn (`human typed`, `human queued`, `task-notification`, `peer`, `sdk`, `scheduled`); the prompt hook also fires for notifications; the transcript line is absent when the hook fires; `!` has no tty; a hung-up session still fires `SessionEnd`. All read-only or in a throwaway directory; one operator; Claude Code 2.1.329.

## 8. Checked against the code (main at 43f58275, 2026-11-20)

Read, not run. Line numbers are of that commit.

| Claim | Found |
| :-- | :-- |
| The app socket locks after five refusals in a minute | True. `crates/delphi-core/src/server.rs`: `APP_REFUSALS = 5` (line 56), `app_refused` (676) sets `locked_until`, and the handshake (699) drops new connections without a word while it is set. One counter for the whole socket |
| The server reads the peer's credentials | True. `peer_cred` at line 471; the process id is kept (`peer_pid`, 475) and already used for the terminal check |
| A ceiling on unproven connections | Exists already for the whole socket (`max_unauth`); none per peer |
| Revoke closes sessions and leaves work running | True. `close_sessions` (928); `app.revoke` (1074 to 1089) records `sessions_closed` only; taking a grant away does the same (1127). The comment above `app_request` (1139 to 1142) says a request in flight runs to its own end |
| Running app requests are tracked | True. `Tally.running` in `crates/delphi-engine/src/app_requests.rs` (line 69), by key, as `Work::Research(run id)` or `Work::Rebuild(slug)`, without the request's id |
| The research queue has a cancel | True, with one correction: line 347 of `research_jobs.rs` is the job's `cancel` field; the function is `Jobs::cancel(id, session)` at line 458 |
| **A page rebuild has no cancel handle** | **True.** `app_rebuild` (`app_requests.rs:357`) spawns the task and keeps no handle; `page_run.rs` and `library_topics.rs` hold no cancel of any kind; `going` counts a rebuild as running until its slot is dropped. S2 has to add one (4.4) |
| The keep logic is in the `delphi` crate | True for the command (`crates/delphi/src/learn_keep.rs`, 587 lines, about half of them tests). The parsing, the preview and the pinned write are already in the engine (`customize.rs`: `parse_lesson` 201, `lesson_preview` 347, `apply_lesson` 356) |
| `G:` and a goal in provenance are not built | True. `parse_lesson` reads `T`, `D` and `F`; `Provenance` has no goal and requires `ratified_from` |
| `delphi teach`, `learn lessons`, `learn unteach` are not built | True. No such command, and no function that removes a lesson |
| **`lesson.served` does not exist** | **True.** No event of that or any similar name. The premise that it "can be written at the pack's serve point" holds only in part: there are three serve points and the host's is outside the daemon (4.4, S7) |
| The `review` grant and the app's inbox are not built | True. `app_grants.toml` has `read`, `request` and `update`; it is a gate path (`crates/delphi/src/merge/gates.rs`) |
| The app's Memory screen is read-only | True. `app/src/screens/Memory.tsx` calls `memory.roles` and `memory.role` and nothing that writes |
| `propose_note` can carry a `teach` or `ask` label | **False today.** Its schema is `role`, `text`, `evidence`, `model`, with no other field allowed (`mcp_host.rs:420`). The spec's sections 5.1 and 5.2 assume a label and no slice named the change; this plan puts it in S6 |
| Spec 120's own text is amended | Not yet. Section 3 item 4 still says five refusals; the spec's section 12 says S1 and S2 edit that file |

## 9. Follow-ups found while building (2026-11-21)

Recorded so they are not lost; none is scheduled, none is part of a slice of this plan unless the operator says so.

1. **An app update's child can outlive the daemon (found by the S2 builder; it predates S2).** `update_shutdown` (`crates/delphi-engine/src/app_update.rs`, about line 443) ends only the child whose process id is already recorded in the update state (`child`, set at about line 425 once the child has started). An apply admitted earlier sets `running_for` (about line 705) before that. If the daemon stops in the window between the apply starting and the child's id being recorded, shutdown ends nothing and the child keeps running after the daemon is gone. The S2 test race (fixed in `e8aeb63f` by waiting for the child before shutting down) showed the window is real on a loaded machine. A likely fix: let shutdown mark the apply as stopping so the start path ends the child itself if it records it after shutdown began, with a test that stops the daemon inside that window. Spec 120 or 122 owns the update path; the fix needs a slice under one of them.
2. **Two intermittent tests seen on this plan's pull requests**, neither caused by them:
   - `delphi` `train_run::tests::a_hot_five_hour_window_makes_a_claude_run_wait_and_a_hot_weekly_window_stops_the_study` on Linux: `Text file busy (os error 26)` when a fake script is run right after the test writes it (another forked thread still holds the write handle). The usual fix is to write the script, close it, and start it through a copy or with a retry on `ETXTBSY`.
   - the granted-claude seat tests in `harness_run.rs` on macOS: the 5-second `--version` probe misses its bound under load (already recorded as a known flake).
3. **OD-4 as built.** S3 writes `learn.taught` and `learn.unlearned` to a lessons log of its own (`<workspace>/local/lessons-log/`, `<home>/operator/lessons-log/`), not a mission log, because the daemon holds the mission log it works in and a fresh workspace has none. The spec's OD-4 says "in the mission log". Recommendation: accept and amend the spec's text to match.
