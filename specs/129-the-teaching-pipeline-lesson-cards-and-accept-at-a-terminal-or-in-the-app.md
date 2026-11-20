---
id: spec-129-the-teaching-pipeline-lesson-cards-and-accept-at-a-terminal-or-in-the-app
title: "129: The teaching pipeline: lesson cards, and accept at a terminal or in the app"
author: architect
timestamp: 2026-11-20T00:00:00Z
status: draft   # a proposal, not permission to build (AGENTS.md). Written with the operator over one session; it has had no Council and no red-team pass of its own.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # P0 measurements (section 3) are real but small; every code anchor below was read on main on 2026-11-20, nothing was built or run beyond read-only probes and four headless Claude Code runs
source_mission: 2026-11-19-teaching-pipeline
tags: [spec, learning, lessons, memory, hosts, app, ratification, cards]
---

# Spec 129: The teaching pipeline: lesson cards, and accept at a terminal or in the app

> **Status: draft.** Nothing here is approved or built. It follows two Council runs on earlier drafts of the idea (`t_0002`, `t_0003`, section 14) and a measured first phase (section 3).
> **Extends:** [`specs/031`](031-layered-learning-and-training-mode.md) (the operator layer, staging, ratification), [`specs/108`](108-closing-the-learning-loop-for-host-sessions.md) (what a host session leaves, `delphi learn keep`), [`specs/116`](116-memory-that-grows-a-provisional-tier-findings-consolidation-and-a-larger-start-context.md) (the provisional tier).
> **Amends, from its approval:** [`specs/120`](120-the-app-scope-a-key-paired-from-a-terminal-and-grants-that-widen-one-at-a-time.md) (section 10 says exactly what), and builds the `review` grant that 120 section 5 foresees.
> **Does not change:** taint rules, the Rule of 25, the no-relaxation linter, or who may ratify.

## 1. Scope and objectives

The operator wants agents to improve with their guidance: to say "remember this" or "learn this" to a particular role, to be asked why when the reason is not plain, to have the agent say what it is unsure of, and to have the system suggest lessons from what happens. All of this changes what a role carries into its next task, with provenance, chosen or confirmed by the operator. It does not change a model's weights.

| # | Observation (code on `main` and measurements, 2026-11-19/20) |
| :-- | :-- |
| E-1 | A role's lessons are written only by `delphi learn keep` (spec 108 M2) and `delphi customize` (spec 031 L5), each at a terminal with a typed `yes`. There is no way to say "teach the builder this" in the place the operator works, and no field for why. |
| E-2 | The notes that wait for review are mostly not the operator's: of 182 live staged notes, 151 are Council by-products (claims, verdicts, minorities, escalations), 14 scout, 8 agent notes and one is a correction (section 3). |
| E-3 | Nothing shows the operator what a role has been taught, whether it was used, or that a note will expire. The app's Memory screen is read-only. |
| E-4 | A host cannot be trusted to say who typed a turn: the prompt hook fires for task notifications too, and the transcript line is not on disk when it fires (section 3). A terminal cannot be reached from a `!` line (it has no tty). |
| E-5 | `learn keep` runs in the `delphi` binary and writes files with a compare-and-swap on the pin; the daemon has no method that does it, so the app cannot ask for it. |
| E-6 | Spec 120's app socket closes to new connections after five refusals in a minute (a local process can lock the operator out), and `delphi app revoke` closes the session without stopping work the key already started (section 10). |

**Objective.** One pipe from a note to a lesson, with three ways in and two places to accept:

```
note or request  ->  card  ->  accept (terminal or app)  ->  lesson on probation  ->  kept, or unteached
 (operator, host, agent)         the daemon renders the text      counts only
```

### 1.1 Non-goals

- **No accept in a host's chat.** A host can draft a card and say where to accept it; it can never accept one, and nothing a host sends can raise a card's trust (section 14 says why).
- **No top-up credits, no per-session or daily limits, no transcript verification on the accept path.** They existed only to make a model-mediated confirm count, which this spec does not allow.
- **No automatic ratification and no automatic retirement.** A lesson is accepted by the operator and retired by the operator.
- **No score** for a lesson or a role (spec 083 non-goal). Counts and ages only.
- **No model grading the operator or itself.** The agent never assesses the operator; evidence is what the daemon counted.
- **No teaching of skills.** The engine does not load skills by name (spec 031 L5), and a skill needs a rehearsal path that does not exist. Deferred.
- **Not a rewrite of the app.** This spec adds the learning surfaces and one grant. The wider plan for the app as the operator's working place is its own spec (OD-5).
- **No cross-operator sharing** beyond spec 031's `delphi contribute`.

### 1.2 Simpler alternative considered

*Change nothing: the operator keeps using `delphi learn keep` and `delphi customize`.* It works, and everything here sits on top of it. Rejected as the whole answer because the gap is in the front of the pipe, not the back. The operator cannot say "teach the builder this" where they work, a lesson has no stated goal, the operator cannot see what a role carries, and the notes that wait are mostly Council by-products. The smallest change that closes it is a card with a goal, a list of what each role carries and an accept that works where the operator already is. This spec adds exactly that and no more.

### 1.3 Options considered

| Option | Kind | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A. Extend the terminal only | conventional | `delphi learn keep` gains a goal; `delphi teach` writes a lesson at a terminal; no cards in an app, no host drafting | S3's live proof alone, then two weeks of use: how many lessons the operator writes this way |
| B. Remove the constraint "a host cannot say who typed" | removes a named constraint | Accept in a host's chat, checked against the harness transcript with a daemon-computed code (designed in full, section 14) | A fixture that appends a forged user record to a transcript and expects the accept to fail; the hook and timing probes of section 3. It failed both in P0 and is not adopted |
| C. Invert who starts | inversion | The role interviews the operator: at a review it asks its two or three most uncertain questions and each answer becomes a card, so the operator is asked and never has to think of what to teach | Count of accepted lessons per question asked and of questions left unanswered over two weeks; stop if most go unanswered |
| D. This spec | recombination of A, C and a card | One card shape, three ways in (operator, a role's ask, a recurring note), accept at a terminal or in the app | The acceptance criteria of section 16 and the measured result per slice (section 17) |

D is chosen because A alone leaves the operator without a view of what a role carries, B depends on evidence P0 did not find, and C needs a role that knows what it does not know, which no measurement here supports yet. C's questions arrive in D as the `ask` card (section 5.2), at most two a session, so the inversion is tried at small cost.

## 2. Principles

1. **One pipe.** Every lesson comes from a card; the three ways in differ only in who started the card and how it is labelled.
2. **The daemon renders what is stored.** What the operator reads at the accept is the text the daemon will write, rendered by the daemon from the card. A model's rendering of a card is advice about where to look.
3. **Trust lives in the channel, not in the words.** The terminal and the app are channels Delphi controls. A card's words may come from anywhere; the operator's accept, at one of those two, is the act that makes it a lesson.
4. **Evidence is counted.** The daemon counts what it observed. No model assesses a lesson.
5. **Measure before building the next slice.** The slices of section 12 each end with a measured result or stop.

## 3. P0: what was measured (Claude Code 2.1.329, 2026-11-19/20)

Read-only analysis of the operator's real data and four headless Claude Code runs in a throwaway directory. The probe files were removed. No text of any turn was printed or kept; counts only.

| Finding | Evidence | Consequence |
| :-- | :-- | :-- |
| The staged-note stream holds almost no operator signal | 182 live notes; `correction` 1; host notes recurring in two sessions 0 | Recurrence-gated suggestions have no input yet (section 5.3) |
| Correction and praise wording is rare | 12 of 1,228 human turns open with a correction cue (9 of 134 sessions); 25 hold a praise word; median human turn 63 characters | Lexical cues are not a v1 signal source |
| The harness labels each turn | 167 transcripts: human typed 1,132; human queued 96; task-notification 1,529; peer 336; sdk 37; scheduled 2. No paste label. Fields exist since 2.1.309 | A transcript can say who a turn came from, but see the next rows |
| The prompt hook cannot tell the operator from a notification | In a headless run it fired for two task notifications; it did not fire for a subagent's prompt | The hook is not evidence of who typed |
| The transcript line is not on disk when the hook fires | Absent after 2 s of polling; `prompt_id` in the hook equals `promptId` in the transcript | Any in-host check must be deferred; this spec does not need one |
| A hung-up session still fires `SessionEnd` | SIGHUP, reason `other`; it cannot fire on SIGKILL | No design here depends on it |
| `!` in Claude Code has no tty | operator ran `test -t 0` and got `no-tty`; `<bash-input>` records carry no origin labels | The terminal means a separate terminal; no in-chat terminal-trust path |

Not measured: interactive paste, slash commands and queued turns against the hook; a live end-to-end `note`, `keep`, reload and use (section 12, slice S3 exit).

## 4. The card

A **card** is a note, a proposal or a request that waits for the operator, shown the same way wherever it is shown. It is a projection over events that exist today, not a new store.

| Field | Content | Set by |
| :-- | :-- | :-- |
| `ref` | `<mission>/<id>`, as `learn list` prints it | daemon |
| `kind` | `teach` (the operator asked), `ask` (a role asks to be taught), `recurring` (a host note reported by two or more sessions), `other` (any other staged or provisional note that can be kept) | daemon, from the note's trigger and label |
| `role` | the role the lesson would be for | the note or request; the operator can change it at accept |
| `text` | a lesson as `T: <when> D: <what to do> [F: <what would refute it>] [G: <goal>]`, at most 350 characters | the drafter; the operator may retype it at accept |
| `goal` | why: the operator's words, or the drafter's guess marked as a guess, or `none given` | the drafter; the operator confirms or retypes |
| `scope` | `workspace` (default) or `operator` | the operator, at accept |
| `origin` | `operator-terminal`, `operator-app`, `host-reported` or `agent-note`, with tool and session where known | daemon, from the channel the card arrived on; never from a field the sender supplies |
| `evidence` | pointers (mission, event ids, session ids), never text from a page or a tool | daemon |
| `recurred` | how many sessions reported it (spec 108 M1) | daemon |
| `expires` | the date a provisional note lapses (spec 116) | daemon |

- `T:`, `D:` and `F:` are the grammar of `customize::parse_lesson`. `G:` is new (S3): optional, one sentence, and it is written into the lesson's `Why:` line with the provenance. A card whose text lacks `T:` or `D:` cannot be accepted as typed; the operator supplies them at accept (the existing `--text` path).
- A card's `origin` is a label of what happened, not a grant of trust: `host-reported` stays `external_untrusted` at L0 exactly as spec 098 §12 item 12 says.
- **A card never loads into a prompt.** Only an accepted lesson does.

## 5. Three ways in

### 5.1 The operator asks to teach

1. **In a host session.** The `delphi` skill tells the host: when the operator says to teach, remember or learn something for a role, call `propose_note` with the lesson as `T: D: [F:] [G:]`, label `teach`, and say that a card now waits, naming its ref and the two places to accept it. The host never says the lesson is learned. If the goal is not plain from the operator's words, the host asks once and writes the answer into `G:`; if the operator gives none, `G: none given`.
2. **At a terminal.** `delphi teach <role> "<text>" [--goal "<why>"] [--scope operator]`. It shows the lesson as it will be written and writes it after the operator types `yes` (there is no `--yes`). The operator's own typing at a terminal is the card and the accept in one act; origin `operator-terminal`.
3. **In the app.** A composer for a role, a lesson and an optional goal (S5). Origin `operator-app`.

### 5.2 A role asks to be taught

The skill tells a host role: when the same ambiguity stops you twice in one session, call `propose_note` with label `ask`, what you could not decide and what rule would have decided it, as `T: D:`. At most two `ask` notes are kept per session (OD-2). An `ask` is the role's request, labelled so; the operator decides.

### 5.3 The system suggests

In v1 exactly one signal creates a suggestion: **a host note reported by two or more sessions** (`recurring`), which spec 108 already marks. A host supplies its own session id (spec 108 `Caller::parse` takes any 8 to 32 characters), so the count is the number of distinct session ids a host reported, and every place that shows it says "reported by N sessions (host-supplied)". It can be manufactured by a compromised host; what that buys is a card to look at, never a loaded lesson, and the daemon caps the cards one host session may stage (the staging cap of spec 031 applies). A count from sessions the daemon itself saw start, where it can (the session-start marker of spec 105), is shown beside it. P0 shows that the other candidate, correction wording, does not occur often enough to rely on (section 3). The signals worth building are a follow-up (OD-3), chosen from what P0 and the first months of use show: the operator overruling a Council recommendation, a merge without changes, a card accepted unedited. None is built here and none may use a model to decide that something is a signal.

## 6. Accepting

1. **At a terminal:** `delphi learn keep <ref> --as lesson --role <role> [--scope workspace|operator] [--text "T: D: [F:] [G:]"]`, as today, plus the card's goal shown and written. It shows the text that will be written, and writes after a typed `yes` at a terminal; it refuses non-terminal input.
2. **In the app:** the card shows the daemon-rendered lesson, its goal, its scope (workspace or operator, the operator's choice, in plain words: the operator scope follows the operator into every workspace; OD-1 and OD-7), its origin and its expiry. **Accept** sends `{ref, code}` and nothing else; the code is the keyed hash the daemon made over the card's id and the text it rendered, so an accept is refused when the stored text no longer hashes to the code (the card changed after it was shown), and the daemon renders again and shows the new text and code. **Edit and accept** sends the operator's typed text; typed text is checked as below and carries no code. **Not this** marks the note wrong (180 days, as `learn wrong`). Each is the `review` grant (section 10.3) and runs only on a connection that armed it (item 8).
3. **Both paths run the same checks, in the daemon's code, at the accept and not only when the note was staged:** the no-relaxation linter, the 350-character rule cap, the 25-rule and 6,144-byte caps, near-duplicate detection against existing lessons, the redaction of secret shapes in stored words (spec 102 §3.1), a role that exists in this workspace.
4. **At the 26th lesson** the accept is refused with the three ways out (merge two lessons, demote one to `context/domains/`, or promote one to a script), as `learn keep` does today. The app shows them as actions.
5. **A conflict is named, not hidden.** If the new lesson overlaps an existing one for the same role by the duplicate test, the accept says so and requires the operator to merge or confirm that both stay.
6. **The lesson written** carries `source_type: user_ratified`, `ratified_from`, the card's `origin` and tool, the date and the goal. It applies from the role's next pack (`pack_hash` changes, spec 108 M2).
7. **Rules and skills are not accepted in the app in v1.** `--as rule` and `delphi customize` stay at a terminal.
8. **The one-click accept, secured in layers** (operator decisions 2026-11-20: OD-7 option (a) at full operator scope; OD-8 read as "make the one-click card accept safe"). Accepting a card in the app is one click. What keeps a stolen or file-read app key from doing the same:
   - *The text is the daemon's* (items 2 and 3): the click carries an id and a code, nothing a page, a tool or an agent wrote.
   - *`review` is armed per connection.* A key that holds `review` can use it only on a connection that armed it. Arming is `app.arm`, answered only after a presence proof (item 9); it lasts until that connection closes or 30 minutes pass, and it applies to that connection alone. A second connection signed with the same key, which is what a process that copied the key file would open, is not armed. A hostile process would have to take over the app's own connection, which is a larger compromise than reading a file. Recommended and open to veto (OD-9): an accept at the operator scope needs an arming no older than five minutes, because it follows the operator into every workspace.
   - *Each accept is bounded and visible.* The lesson is labelled `operator-app`; item 3's checks run; the grant has its own limit (20 accepts an hour per key); the next terminal command prints one line with a count (`N lessons were accepted in the app since you last looked: delphi learn lessons --app`, no card text); and each lesson has a one-click unteach.
   - *What this does not defend against:* software that drives the app's own window (the accessibility interface) or a replaced app. Spec 120 §8's stated limit stands.
9. **The presence proof for arming** (OD-9), in the order of what exists. (i) `delphi app arm [<name>]` at a terminal with a typed `yes`: the terminal is the trust anchor everywhere else, it works now, and it is needed about once per app session. (ii) A native confirmation the daemon itself shows on macOS (its own dialog naming the key, the role and the scope), which a process running as the operator should not be able to answer without the operating system's permission to control the screen. That is a claim about the platform that has not been tested here; slice S4a tests it. If it holds, it replaces (i) for one-click use. (iii) Touch ID through a key held by a signed app, which needs the app to be signed (spec 120 §8) and is later. v1 builds (i).

## 7. What the operator sees

- **Terminal:** `delphi learn list` shows each card with kind, role, goal, origin, recurrence and expiry, and the exact command to accept it. A new `delphi learn lessons [<role>]` lists what each role carries with its provenance and counts. `delphi learn unteach <role> <lesson id>` retires one (section 8).
- **App:** an inbox of waiting cards, a per-role list of lessons with their goal, origin, age and counts, the composer, and unteach. Opening it needs the `review` grant for the actions and `read` for the lists.
- **Host:** at session start the brief (spec 105 §3) gains one line of fixed words and a count, `N lesson cards wait for review (delphi learn list)`, and no text from any card. `recall_sessions` is unchanged.
- **Expiry is visible.** A provisional note shows how many days it has left everywhere it is listed; a card with seven days or fewer is marked.

## 8. Probation, evidence and unteaching

1. **Counts, from daemon events only:** the day accepted; how many times a role's pack carrying the lesson was served and the last time (a new `lesson.served` event, S7; the premise that the event can be written at the pack's serve point is checked first); how many sessions reported the same note again after it was accepted (a repeat shows that the lesson did not stop the problem or was not served).
2. **No score and no automatic retirement.** A lesson with no serves in 60 days and one with repeats after acceptance are listed under "worth a look" in the app and by `delphi learn lessons`; nothing changes without the operator.
3. **Unteach.** `delphi learn unteach` and the app action remove the lesson from the role's file with the same pinned write as an accept and record `learn.unlearned` with the lesson id, the date and the goal it had. The text is kept in the log, so unteach is reversible by teaching it again. An unteached lesson's text is not served.
4. **In a host, a lesson is advisory** (spec 098 §12). Delphi can serve it and count it; it cannot see that the model followed it. This spec does not claim the agent changed, only that the role's pack carries what the operator chose.

## 9. Authority invariants (what the tests prove)

1. No host-scope call can write a lesson, accept or reject a card, or unteach. A host's `propose_note` creates only a note at L0, `external_untrusted`.
2. A card's `origin`, `kind` and trust come from the channel the daemon saw it arrive on, never from a field the sender supplies; a payload claiming `operator-terminal` from the host socket is stored as `host-reported`.
3. An accept without a terminal or without the `review` grant is refused, and the refusal names which.
4. The app's accept of unedited text is refused when the code does not match the text the daemon renders now.
5. A lesson cannot be written that fails the no-relaxation linter, the Rule of 25 or the caps; a refusal writes nothing.
6. Text taken from a page, a tool result or an agent never reaches the app's request: the `review` methods take ids, a code, and text the operator typed.
7. No card, note or ask loads into any prompt; only an accepted lesson does, in the role's own pack.
8. `unteach` removes a lesson with the same pinned write and records what was removed.
9. The brief's line carries a count and fixed words only (the test of spec 105 A1 applies).
10. Stored operator words pass the redactor against the fixed corpus of secret shapes (spec 102 §3.1).
11. A review method on a connection that has not armed `review` is refused; arming applies to one connection and ends with it or after 30 minutes; a second connection with the same key is not armed; an operator-scope accept needs an arming within five minutes if OD-9 keeps that rule.

## 10. Amendments to spec 120 (they take effect on approval; slices S1 and S2 build them)

1. **Section 3 item 4 (the lockout).** Replace "Five refusals in a minute close the socket to new connections for a minute" with: the daemon always sends the challenge. A refusal costs the caller, not the listener: after a connection's first failure the daemon waits before answering the next from the same peer, the wait doubling to a cap of five seconds, and it limits the unproven connections one peer may hold at once and the unproven connections the whole socket may hold. The cost is kept per peer (the connection's process id where the platform gives it, the server already reads the peer's credentials, and its user id otherwise), per key name claimed in the hello, and for the socket as a whole, because a process running as the operator can start new processes and so rotate a process id: the per-peer cost stops accidents and noise, and the global ceiling and the per-name cost stop the rest from locking a valid key out. A valid key's signature is never refused for the failures of others. The `app.refused` log aggregates: after the first twenty in a minute each further one adds to a count written once a minute. *Why:* the proof is an Ed25519 signature and the pairing ticket is single use, so guessing is not the threat; the lockout let any process of the operator's keep the app out, with 5 knocks a minute. A hostile process running as the operator can always interrupt its own daemon; the goal is that no cheap or accidental process can.
2. **Section 2 item 8 and AC-10 (revoke).** `delphi app revoke <name>`, and taking away the `request` grant, also cancel every request the key started that has not ended, through the same cancel the research queue already has, and write `app.request` state `cancelled` with reason `revoked`. `app.revoked` gains `requests_cancelled`, and each cancelled request's `app.request` event names its request id (never text). A request admitted while the revoke runs is refused by a second check of the key's grant after admission, and a test makes a request arrive during a revoke and expects it either refused or cancelled, never left running. *Premise to check first (S2):* a page rebuild has no cancel handle that was found; S2 adds one or says a rebuild runs to its end and is named so.
3. **Section 4 and 5 (the `review` grant).** `review` is built: a named grant in the code table and `crates/delphi/tests/app_grants.toml` with the methods `app.cards_list`, `app.card_show` (renders text and code), `app.card_keep`, `app.card_wrong`, `app.lesson_unteach` and `app.arm` (section 6, item 8); `app.card_keep` takes optional typed text (edit and accept), because it starts from a card. A typed lesson with no card, `app.lesson_teach`, is not in the first cut (OD-8): it is new text with no earlier card to read, and it waits for its own red-team pass. `review` writes memory the operator alone may write, so it needs the red-team pass 120 section 5 names; a key gets it only by `delphi app grant <name> review` at a terminal, and uses it only on an armed connection. The `read` grant gains the read methods for the card and lesson lists. The foreseen row for `review` in 120 section 5 is replaced by this definition; `answer`, `settings` and `curate` are unchanged.
4. **Section 4.2 rule 5.** Unchanged by operator decision (2026-11-19): app-typed text is treated as the operator's. The paste case a red-team seat raised is accepted and not addressed here.
5. **Acceptance criteria** AC-10 becomes: revoke closes the session and cancels the key's running requests; a new AC covers a valid key connecting during a flood of refusals.

## 11. What exists to build on, and what does not

| Needed | State on `main` (2026-11-20) |
| :-- | :-- |
| Notes, host notes, `recurred`, `learn keep`, the lesson file format, the Rule of 25 | built (spec 031, 108) |
| `G:` in the lesson grammar; a `goal` field in provenance | not built (S3) |
| `delphi teach`, `learn lessons`, `learn unteach` | not built (S3) |
| A daemon method that keeps a note | not built: the keep logic lives in the `delphi` crate and is not reachable by the daemon; S4 moves it into the engine |
| `lesson.served` | not found; premise checked in S7 |
| The `review` grant and the app's inbox | not built (S4, S5) |

## 12. Slices (one pull request each; none starts before approval)

| Slice | Content | Depends on | Hours (estimate) |
| :-- | :-- | :-- | --: |
| S0 | this spec, and the amendment text of section 10 applied to spec 120 | Council with a readable red-team Round 2, operator approval | 3 |
| S1 | the lockout replaced (section 10.1), with a flood test | S0 | 6-8 |
| S2 | revoke cancels running work (section 10.2), the page rebuild cancel handle | S0 | 6-10 |
| S3 | `G:` and the goal in provenance; `delphi teach`; `learn lessons`; `unteach`; the card view in `learn list`; the Rule-of-25 and conflict messages | S0 | 10-14 |
| S4a | a probe, no product code: can a process running as the operator answer a dialog the daemon shows on macOS without the screen-control permission? (section 6 item 9 (ii)) | S0 | 2-3 |
| S4 | the keep logic into the engine; daemon methods for the `review` grant, with per-connection arming (`delphi app arm`, `app.arm`), the limit and the digest line; the grant and its gate-file entry (a gate-changing path, so the operator merges it at a terminal) | S1, S2, S3 | 16-22 |
| S5 | the app: inbox, per-role lessons, composer, unteach, actions; the read methods | S4 | 14-20 |
| S6 | the `delphi` skill lines (teach, ask), the brief's count line | S3 | 4-6 |
| S7 | `lesson.served` and the counts of section 8 | S3 | 6-10 |

- **S3's exit is a live proof,** operator-run: teach a lesson, accept it at a terminal, confirm `pack_hash` changed and the next `get_agent` for the role carries it, then unteach it. If a real model has never called `propose_note` with a `teach` label by then, that is recorded. Before S4 starts, the numbers are read again: how many host notes recur and how many cards the operator kept at the terminal in two weeks. The order is terminal accept (S3), then the app's accept (S4, S5), with the `review` grant last of all.
- **S4 and S5 do not start until S1 and S2 are merged.** The app becomes a place that accepts lessons; it must not be lockable or leave work running after a revoke.
- **Deferred, in this order of likelihood:** the signals of section 5.3; a mirror of counted evidence and a role's legibility card; teaching a skill; the app as the operator's working place (OD-5); an in-chat accept, only if measured friction justifies it and a verification that does not depend on a model exists.

## 13. Known limits, and what is not verified

- **A host lesson is advisory** (spec 098 §12). The operator's accept is real; the model's compliance is not enforced.
- **A card's words may be a model's.** The operator reads them at the accept; a plausible but wrong lesson that the operator accepts is the operator's. The linter and the caps bound what it can say, not whether it is wise.
- **Software running as the operator** can drive the terminal or read the app's key file (spec 120 §8). This spec does not defend against it.
- **Paste into the app** is treated as the operator's (section 10.4), by decision.
- **The app is unsigned** (spec 120 §8); a keychain item's per-application protection may not hold.
- **Not verified:** that a real model calls `propose_note` with the labels this spec asks for; interactive paste, slash commands and queued turns against the prompt hook; that `lesson.served` can be written where a pack is served; that a page rebuild can be cancelled; whether peer process ids are available on every supported platform; every claim about prior art in the design discussions (searched on 2026-11-19, unvalidated); the red-team's view of this text.
- **P0 is small and single-operator.** The finding that correction wording is rare is about one operator's 1,228 turns.

## 14. Considered and not adopted

| Idea | Why not |
| :-- | :-- |
| **Accept in a host's chat** (a confirm checked against the harness transcript, with a daemon code, a daily ceiling and terminal top-ups) | Designed, then dropped. A model with a shell can write the transcript line; a code shows the text was shown, not read; the prompt hook fires for task notifications; the transcript line is not on disk when the hook fires; paste has no label. The self-attested rung put unverified text in an always-loaded pack. Council `t_0002` and `t_0003` (one seat each could not be read) leaned the same way. The operator chose terminal and app only (2026-11-20). |
| **`!` in Claude Code as the terminal** | No tty; a non-tty exemption would admit the agent's own Bash tool. |
| **A separate per-agent ledger** | A lesson's `Why:` line and the exchange record (spec 102) already hold the goal and the words. Revisit only if `learn lessons` cannot answer "what did we teach and why". |
| **Suggestions from correction and praise wording** | P0: 12 correction-cue turns and 25 praise words in 1,228 turns. Structural signals are the follow-up. |
| **Automatic retirement of unused lessons** | It acts without the operator; "worth a look" lists and unteach give the same information. |
| **Model self-assessment as reciprocity** ("what could I do better?") | Self-assessment without outside evidence is unreliable and sycophancy-prone; counted evidence and a legibility card are the follow-up. |
| **Teaching a skill from a practiced session** | The engine does not load skills by name; a rehearsal path does not exist. |

## 15. Council record

- **`t_0002`** (on an earlier draft, 2026-11-19): architect and strategist, proceed with changes; the red-team seat abstained. Its P0 (teach must be operator-typed and enforced in code) is section 9 items 1-2; its P1s (a live end-to-end proof, a Rule-of-25 plan, no skills yet) are sections 6.4, 12 and 1.1.
- **`t_0003`** (on the plan with a host-chat confirm, 2026-11-19): the architect answered; the red-team seat audited spec 120 and its Round 2 could not be read; the strategist abstained. It recommended terminal and app accept only, which this draft adopts, and its findings on spec 120 (checked against the text) are section 10.
- **`t_0004`** (on this text, 2026-11-20): **not a review of this text.** The daemon looked for the spec in the main checkout, where this branch's file did not exist, so the architect seat answered from the question and the cited specs and said so; the red-team seat's Gemini carrier finished without a valid answer (its report says the gate could not be reached and the prompt was truncated) and the strategist seat abstained. The architect's conditional findings were applied where they were clear: accept bound to the card id and a hash of the rendered text and the checks re-run at accept (sections 6.2, 6.3), recurrence labelled as host-supplied (5.3), the lockout's peer defined so a same-user process cannot rotate around it (10.1), a revoke race test and a named event (10.2), measure first and terminal before app (12). Two findings are policy choices left to the operator: OD-7 and OD-8.
- **The gate before S0 is still a Council that read this text,** with a red-team seat that answered in a readable Round 2.

## 16. Acceptance criteria

1. **AC-1:** `delphi teach builder "T: ... D: ..." --goal "..."` shows the lesson and, after a typed `yes`, writes it to the builder's lessons with the goal and provenance; the next `get_agent` for `builder` has a different `pack_hash` and carries it.
2. **AC-2:** the same command with stdin that is not a terminal refuses and writes nothing.
3. **AC-3:** a `propose_note` labelled `teach` from the host socket creates a card with origin `host-reported` and trust L0; no host call accepts it.
4. **AC-4:** an app key without `review` is refused each of the review methods; with it, on a connection that armed it, `app.card_keep` of unedited text succeeds only with the code of the text rendered now. On a connection that did not arm it, or a second connection signed with the same key, every review method is refused and says it is not armed.
5. **AC-5:** at the 26th lesson, an accept in the terminal and in the app is refused with the three ways out and writes nothing.
6. **AC-6:** an accept whose text fails the no-relaxation linter, exceeds 350 characters, duplicates a lesson or names a role the workspace lacks is refused with the reason.
7. **AC-7:** `unteach` removes the lesson, records `learn.unlearned`, and teaching the same text again restores it.
8. **AC-8:** a flood of refused connections on the app socket does not stop a valid key from connecting (spec 120, amended).
9. **AC-9:** revoking a key cancels its running requests and records `requests_cancelled` and the cancelled request ids; a request that arrives while the revoke runs is refused or cancelled and never left running (spec 120, amended).
10. **AC-10:** the session brief's card line holds a count and fixed words and no card text.

## 17. Verification Plan

- **Unit and engine tests** for each invariant of section 9 and each acceptance criterion, in the crate that owns the code; the app-socket tests of `crates/delphi-engine/tests/app_socket.rs` extend to the flood and revoke cases; the gate-file test of spec 120 AC-6 covers the `review` methods.
- **Fixtures:** the secret-shape corpus of spec 102 §3.1 for stored goals; a card payload that claims an operator origin from the host socket; a lesson text that tries to relax governance; a 26th lesson.
- **Live, operator-run:** S3's end-to-end proof (section 12); and, after S5, one accept and one unteach in the real app against the real daemon.
- **A measured result per slice** (section 2 principle 5): after S3, how many cards the operator creates and accepts in two weeks and how many expire; after S7, which lessons were served and which problems came back. A slice that the operator does not use is stopped, not extended.

## 18. Decisions and open questions

| # | Question | Recommendation |
| :-- | :-- | :-- |
| OD-1 | May the app accept operator-scope lessons, or only workspace-scope? | **Decided by the operator, 2026-11-20: both** (with OD-7). The operator scope follows the operator across every workspace, so the app says so in plain words at the accept. |
| OD-2 | How many `ask` notes per session? | Two. |
| OD-3 | Which signals come after the recurring host note? | Chosen from the first months of use; candidates in section 5.3. |
| OD-4 | Should `unteach` keep the removed text in the log forever? | Yes, in the mission log, so a re-teach is exact. |
| OD-5 | The app as the operator's working place: its own spec? | Yes, after S5, written from what the learning surfaces show. |
| OD-6 | Does the spec 120 amendment ride with this spec's approval, or take its own pull request? | With this spec (S0), as section 10 is the text; the code stays in S1 and S2. |
| OD-7 | `review` lets any process holding a file-held app key (spec 120 §2 item 4) write always-loaded role lessons, which is stronger than `request` and is not what `learn keep`'s terminal-only rule allows (the architect seat of `t_0004`, a P0). Options: (a) accept the same stated limit as `request` (spec 120 §8: software running as the operator can already do much) and bound it: only keep, wrong and unteach, workspace scope, the linter and caps, an `operator-app` label on each lesson and a visible list of app-accepted lessons with one-click unteach; (b) make `review` ungrantable until the key is in a signed app's keychain, which the unsigned app (§8) means is not soon; (c) drop the app's accept and keep terminal only. | **Decided by the operator, 2026-11-20: (a), at full operator scope** ("go with (a) but at full operator scope"). The bounds of section 6 item 8 apply, and the red-team pass spec 120 §5 names comes before S4. |
| OD-8 | Is the app's `app.lesson_teach` (typed text becomes a new always-loaded lesson) in the first cut? The architect seat says cut it: `app.cards_list`, `app.card_show`, `app.card_keep`, `app.card_wrong` and `app.lesson_unteach` first, because a typed lesson is new text with no earlier card to read. | **Operator's answer, 2026-11-20:** "look for ways to secure the app interaction to make one-click learning cards move from proposal to a taught lesson." Read here as: the one-click card accept is in the first cut, secured as section 6 items 8 and 9 say; the typed `app.lesson_teach` composer stays out of it, behind its own red-team pass. Say so if the operator meant the composer too. |
| OD-9 | How is `review` armed for a connection: a terminal `yes` (works now), a daemon-shown macOS confirmation (needs a probe), Touch ID (needs a signed app)? And does an operator-scope accept need an arming no older than five minutes? | Terminal `yes` in v1; probe the macOS confirmation in S4a and prefer it if it holds; yes to the five-minute rule for the operator scope. Open: the operator has not chosen. |
