---
id: spec-123-the-council-and-panel-from-a-host-who-sat-what-it-cost-a-tail-and-the-loop
title: "123: The Council and the Panel from a host: who sat, what it cost, a tail, and the loop"
author: architect
timestamp: 2026-11-16T00:00:00Z
status: approved   # Section 15 (2026-11-17) is an amendment written from the operator's answers to section 12; it takes effect when the operator merges its pull request. By the operator, 2026-11-16, in a host session: "yeah build it in a branch and target fewer independent PRs to make merging easier", said of a ranked list of sixteen changes that was shown to them, after "i like the council/panel split as it stands today. im curious about the experience of both really". The approval was given to the list BEFORE this text was written; the operator has not read the text. It takes effect when the operator merges the pull request that carries it. Items of the list that are not builds (a live check, an experiment, two policy questions) are in section 10, not built.
approved_by: operator (2026-11-16, in advance, on the list)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the code on main (4d299148) and a read-only assessment of it; nothing was run live. Every mechanism below is NOT VALIDATED.
source_mission: 2026-11-16-council-host-experience
tags: [spec, council, panel, host, progress, tokens, receipts, decisions, learning]
---

# Spec 123: The Council and the Panel from a host: who sat, what it cost, a tail, and the loop

> **Status: approved by the operator, 2026-11-16, in advance, on the list it carries out** (see the frontmatter). The split between the daemon Council and the host Panel (specs 062, 086, 104) stays as it is; this spec changes what the operator sees of each and what each leaves behind.
> **Amends, from its merge:** [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §19 (what a status call carries: §3, §4), [`specs/091`](091-seats-families-and-outside-launch.md) (the daemon Council's card names each seat's served family: §3), [`specs/062`](062-delphi-as-host-agent-mcp-server.md) §7 (what `request_council` may ask for: §6) and [`specs/044`](044-decisions-pane.md) §4.4 (a raised decision of kind `council` raised by the daemon: §8). It edits none of their text.

---

## 1. Scope and objectives

The operator asked, on 2026-11-16, for "the council experience in the host environment" to be "the best experience for the operator with things like better tailing to show activity, really getting better output via mixed families responding from delphi, and learning loop or knowledge base integration". A read-only assessment of the code followed; the operator then said "i like the council/panel split as it stands today. im curious about the experience of both really", and, of the ranked list, "build it".

Observations on main (4d299148), each checked in code unless it says otherwise:

| # | Observation |
| :-- | :-- |
| E-1 | A daemon Council's card says only "seats span more than one model family" or "one model family (homogeneous ...)". The per-seat block of served model, family and carrier (`host_rpc::council_view`, `seats_run_by_delphi`) is built for a Panel only (`if panel { ... } else { Vec::new() }`), although every daemon seat writes a `seat.receipt` (`engine.rs`, the span end; spec 091 SR-1). |
| E-2 | A status call (`council_status`) names each seat's role, state, times and calls (`progress::SeatProgress`), and no family, model or token count. Tokens appear only on the final card. |
| E-3 | A Council's ceiling is money: `[budget] council_budget_usd` (20.0), checked after Round 1, before the repair turn and before the synthesis (`council.rs`, `over_budget`). The operator's standing rule is that limits are token shares or counts, never dollars. |
| E-4 | A seat that fails abstains with "no answer within 600s" or the first 160 characters of its error (`Engine::run_seat`); the card does not say which family or endpoint failed, nor where the whole error is. |
| E-5 | `request_council` runs `CouncilOpts::default()` (`service.rs`, `host_request_council`); its schema has no tier, author family or family count. The console's `--tier` is not reachable from a host. A Panel has `require_families`; a daemon Council does not. |
| E-6 | A host Council lives in mission `host-<session>`. There is no way to follow it from a terminal: `delphi logs --follow` is the error log, `delphi sessions` lists finished console sessions, and the console's observe view is per mission. What a host shows of the pushed progress line is not verified (spec 104 R2). |
| E-7 | An escalated host Council answers `needs_operator` and stops; no decision is raised, so nothing reaches the decisions pane (spec 044). `host.council_choose` writes `council.host_choice`, which nothing reads. |
| E-8 | Earlier rulings never reach a later Council. Records are written to `missions/<id>/councils/NNN-slug.md` and indexed as untrusted in zone `missions`, which is not a start zone (`retrieval::START_ZONES`). A seat is excluded from start passages by design, so that Round 1 stays blind (`specs/026` §3.1). |
| E-9 | Measured (spec 089 E3, `dev/train/e3/REPORT.md`): a Panel of the host's own family escalated on 1 of 6 records against the daemon Council's 10 of 12, and said GO on 6 of 6. The arm with a foreign seat is INCOMPLETE. Nothing here changes which venue is the default. |

Objectives: (1) the operator can see who sat, on which family and model, and what it cost, while a Council runs and on its card; (2) the operator can follow a host Council from a second terminal without relying on the host model to relay; (3) a host can ask for the Council the console can ask for; (4) an escalation reaches the decisions pane, and earlier rulings reach the rounds that may see them.

## 2. Terms

- **Council**: the daemon Council (`venue: daemon`), seats run by Delphi.
- **Panel**: the host Panel (`venue: panel`, spec 086), seats run by the host, Delphi holding the barrier; a foreign seat is run by Delphi.
- **Planned family**: the family the plan gave a seat (`council.started`). **Served family** and **served model**: what the seat's receipt says served it (spec 091 SR-1). A served value always wins over a planned one once a receipt exists.

## 3. Who sat: family and model per seat (slice S1)

1. **Status.** Each entry of `seats` in a `council_status` answer gains `family` and `model`. `family` is the served family from the seat's receipts when one exists, otherwise the planned family from `council.started`, with `family_source: "receipt" | "plan" | "host-reported"`. `model` is the served model of the seat's latest `agent.usage` event or receipt (the envelope's `model_id`), or null. Both are checked as plain identifiers (letters, digits, `.`, `-`, `_`, `:`, `/`, at most 64 characters) and are otherwise `other`, so a status call stays fixed-form (098 §12 item 18).
2. **The daemon Council's card** names each seat as the Panel's card does: a block "Seats (from the receipts)" with, per seat, the served model, the served family, the carrier, round 1 parsed, round 2 readable, declined, and its tokens. `council_view` builds `seats_run_by_delphi` for both venues. The header keeps its one-phrase summary and adds the family names: "seats on 2 families: anthropic, google".
3. **One family, said on its own line.** When the families that sat are one (by receipts; by plan while there are none), the card says so on a line of its own directly under the header, for both venues:
   - Council: "**One model family only** (`<family>`): the seats may share blind spots, and the diversity guarantee did not hold."
   - Panel: "**One model family only** (`<family>`): every seat ran on the host's family. Treat a unanimous ruling as a quick check, not an independent review."
   The status answer of a Council gains `homogeneous` from `council.started` while it deliberates, so a host can say so before the card.
4. Nothing here ranks a seat or changes a decision. The card stays built from structured fields only.

## 4. What it cost, in tokens (slice S2)

1. **A token ceiling.** `[budget] council_budget_tokens` (default 300,000; 0 turns it off) is the ceiling of one Council: the input plus output tokens of every `agent.usage` event of the task. It is checked where the money ceiling is checked today (after Round 1, before the repair turn, before the synthesis); reaching it escalates with "the Council token ceiling (N tokens) was reached after <step>; nothing was decided" in place of the money sentence.
2. `council_budget_usd` is kept and still checked, for workspaces that set it; the example config marks it as kept for compatibility and names the token key first. Removing it is not in this spec.
3. **Status.** A `council_status` answer gains `tokens: {spent, ceiling}` (ceiling null when off), and each seat gains `tokens` (its own spans). The progress line ends "; 62k of 300k tokens" when a ceiling is set, "; 62k tokens" otherwise. For a Panel, `spent` counts only the seats Delphi ran, and says so with `tokens.scope: "delphi_seats"`.
4. The card's **Cost** line is unchanged in form and gains the ceiling: "62,000 of 300,000 tokens on Delphi's seats".

## 5. Clearer failures (slice S3)

1. A seat that abstains because of a timeout or an error says which family and endpoint it was on, from its handoff's routing directive and its last receipt: "no answer within 600s (family google, endpoint gemini-pro)". The error text kept on the stance is at most 300 characters, redacted, and ends "(cut; the whole error is in `delphi logs --mission <id>`)" when it was cut.
2. The whole error is written to the error log (spec 039) with the mission, task, role and endpoint, redacted, at `error` level.
3. The card lists abstentions as "`red-team`: abstained: <reason>" in "Who sat", as it does now, with the fuller reason.

## 6. What a host may ask for (slice S4)

`request_council` gains three optional arguments, passed to `CouncilOpts`:

| Argument | Values | Effect |
| :-- | :-- | :-- |
| `tier` | `fast`, `workhorse`, `frontier` | as the console's `--tier`: raises every seat, never lowers (`specs/003` §4.2); a lower tier is refused with the console's sentence. |
| `author_family` | a configured family name | as the console's option: the family that drafted the material; red-team and strategist are kept off it. An unknown family is refused. |
| `require_families` | 1 to 4 | the Council is refused at once, before anything is spent, with `seats_unavailable: only N model famil(y/ies) usable now; require_families asked for M` when fewer families are usable than asked; otherwise the plan must seat at least that many, and the card says "require_families = M: met" or "NOT met" from the receipts. |

Nothing else of `CouncilOpts` (seats, lenses) is opened to a host here. The arguments are checked in the daemon; the MCP schema states them.

## 7. A tail from a terminal (slice S5)

`delphi council watch [<council_id>] [--mission <id>] [--once] [--interval <secs>] [--home <dir>] [--workspace <name>]`:

1. Reads the mission's event log from disk; asks no daemon and calls no model. With no id it picks the latest Council or Panel, still running if any, across the workspace's missions (host missions included); `--mission` narrows it.
2. Prints a header (id, venue, planned seats with planned family and tier, the ceiling), then every new fixed-form line as it happens, in the form of the status tail (`HH:MM:SS role kind name`), and a progress line (phase, tokens) every `--interval` seconds (default 10, 2 to 300) when anything changed. Seat text is never printed.
3. Ends when the Council is decided, needs the operator, is not decided, failed or was cancelled, printing the state and the record's path, and exits 0. `--once` prints the current state and exits.
4. Output is plain lines: no spinner, colour only when the terminal supports it and `NO_COLOR` is not set, nothing redrawn in place (the reduce-motion rule).
5. It is listed in the `CMDS` table, so the CLI reference is regenerated.

## 8. An escalation reaches the decisions pane (slice S6)

1. When a host Council or Panel escalates (`council.verdict` with `escalated`), the daemon raises a decision of kind `council` in the same mission, through `decision_raised::admit`, as `orchestrator` (the role that rules a Council): title "Council <n>: <the escalation reason, fixed text from `decide`>", summary from the escalation and the record path, choices from the ruling's options (ids `opt_1` to `opt_5`, labels as offered; not reversible), no recommendation, `related` the Council's task id. It is Amber (044 §4.3). The ten-open cap applies; when it is full, nothing is raised and the card says so.
2. **A host-reported choice never resolves it.** `council_choose` keeps recording `council.host_choice`; when that Council has a raised decision still open, the choice is shown on the decision as "the host reported the operator chose <label> (host-reported)" (a field `host_reported_choice` on the list entry) and nothing else changes. Only `decision.resolve` from a console resolves it (044 §4.4; the authority invariants of spec 093). The answer to `council_choose` says the decision is still open, and where.
3. The card of an escalated Council ends with "Raised for your decisions pane as RD-00n in mission `<id>`".
4. A daemon Council run from the console is not changed: it already ends in the console's own escalation flow.

## 9. Earlier rulings reach the rounds that may see them (slice S7)

1. **Which.** When a daemon Council starts, the daemon looks for up to two earlier Council records in the workspace (every mission's `councils/`) whose proposition headline shares the most words with this one (lowercase words of four letters or more, stop words removed; at least 3 words in common, and at least half of the shorter headline's words). Ties go to the newer record. No model is asked.
2. **What.** From each, the record's Decision section and its Minority Report, at most 1,500 characters each, as a fenced context slice labelled `council/earlier/<file>` at tier L3 (the tier Round 1 stances have in Round 2).
3. **When.** Only in Round 2, the repair turn and the synthesis. Never in Round 1, which stays blind (`specs/026` §3.1). The record of this Council lists the earlier records it was shown under "Earlier rulings shown", and `council.started` lists their paths.
4. **The Panel.** The moderator's round 3 brief gains the same block; the Round 1 and Round 2 briefs do not.
5. `[council] earlier_rulings = false` turns it off. A caller can also turn it off for one Council or Panel (§17 item 2).

## 10. Not built here, and why

| Item | Why it is not built |
| :-- | :-- |
| Showing Round 1 stances to the host while a Council runs | 098 §12 item 18 keeps a status call fixed-form; the text is already on the decided ruling (`seat_positions`). Changing that needs the red-team's review first. |
| Re-running spec 089 E3's foreign-seat arm | an experiment that spends tokens; it needs the operator's go and a new pre-registration (089 §8). |
| Checking what Claude Code shows of the pushed progress line | a live check by a person at a terminal (104 R2). |
| A foreign red-team seat as the Panel's default | the operator keeps the split as it is. |
| Promoting Council records to a start zone | a trust question for the operator (§12). |
| Councils in the change log (032) and retros (031) | the change log records changes to work and a ruling changes nothing; retros (031 L2) are not built. |
| One decisions view across missions | the decisions pane is per mission; a host mission's decision is seen with the console on that mission. |

## 11. Authority and trust

- Nothing here lets a host resolve a decision, approve an action, raise its own trust or start work. `council_choose` stays advice; §8 item 2 is the rule.
- A status call stays fixed-form: families, models and counts are checked identifiers or numbers, never text a model wrote.
- Earlier rulings are model-written text and enter only as L3 fenced slices, after Round 1, as the Round 1 stances do.
- `require_families` can only refuse or label; it never lowers a tier or spends anything before the refusal.
- `delphi council watch` reads files and prints fixed forms; it writes nothing.

## 12. Open questions for the operator (answered in §15, 2026-11-17)

1. Should a host-reported choice ever resolve a decision (today: never)?
2. Should Council records be promoted to a zone agents retrieve from at span start, and at what tier?
3. Should a single-family daemon Council refuse unless the host opts in, instead of running labelled?
4. Is 300,000 tokens the right default ceiling? (Observed Councils used 100,000 to 135,000.)

## 13. Slices and pull requests

Three pull requests, each one spec's slices (the same-spec trial of `CONTRIBUTING.md`), each slice its own commit with its own trailer and tracker row:

| PR | Slices |
| :-- | :-- |
| 1 | this spec; S1 who sat; S2 tokens; S3 failures |
| 2 | S4 host options; S5 `delphi council watch` |
| 3 | S6 escalation to decisions; S7 earlier rulings |

## 14. Acceptance

- AC-1: a daemon Council's status names each seat's family (plan, then receipt) and model, and its card has a per-seat block from the receipts (test on a fixture log).
- AC-2: a one-family Council and a one-family Panel each print the one-family line (golden card text).
- AC-3: a Council over `council_budget_tokens` escalates with the token sentence and decides nothing; status shows `tokens.spent` and `ceiling`.
- AC-4: a timed-out seat's abstention names its family and endpoint; a long error is cut at 300 characters with the pointer, and the whole error is in the error log.
- AC-5: `request_council` with `tier`, `author_family` and `require_families` reaches `CouncilOpts`; an unmeetable `require_families` is refused before any event of the Council is written.
- AC-6: `delphi council watch --once` on a fixture prints the header, tail and state; with a live log it exits 0 when the Council ends; it prints no seat text (a seat text containing a marker never appears).
- AC-7: an escalated host Council raises one `council` decision with the ruling's options; `council_choose` adds the host-reported choice to it and leaves it open; only `decision.resolve` closes it.
- AC-8: a Council whose headline matches an earlier record's gets that record's Decision and Minority Report in Round 2 and the synthesis, never in Round 1, and lists it in its record.

## 15. Amendment (operator, 2026-11-17): the answers to §12

The operator's words, in the host session, as typed:

> "decisions: host reported choice if it's relayed from a delphi-orchestrated agent can resolve a decision but in a same family panel it should be framed as an operator choice or a recommendation. council records being searchable by agents is smart. accomplish how you feel best. calling up previous decisions when similar circumstances are present is a smart learning step. single-family should continue to run and be labelled. we can't assume access to multiple families. 300K feels like a good cieling but maybe raise it to 400K just in case."

This section replaces §8 item 2, amends §4 item 1, §9 and §11, and closes §12.

1. **§12 question 1, a choice the host relays (replaces §8 item 2).** `council_choose` on a Council whose raised decision is still open:
   - **Delphi-orchestrated** (a daemon Council, or a Panel in which Delphi ran at least one seat on a family other than the host's, by the receipts): the choice **resolves** the decision. `decision.resolved` is written with `by: "host-relayed"`, `surface: "host"` and the host session, so the record says the answer came through the host and not from a console. The effect is still only `record` (044 §4.4): nothing starts.
   - **Same-family Panel** (every seat on the host's family): the choice does **not** resolve the decision. It is put on the decision as the operator's choice as the host reported it, shown as its recommendation ("the host reported the operator chose <label> after a same-family Panel"), and the decision stays open for the operator to confirm in one step in the decisions pane.
   - The answer of `council_choose` says which of the two happened and where the decision is.
   This narrows 044 §4.4 ("only `decision.resolve` from a console") for this one kind and these conditions only; no other raised decision can be resolved from a host.
2. **§12 question 2, records agents can search.** Council records stay untrusted and stay out of the span-start zones (`retrieval::START_ZONES`), so Round 1 stays blind and no span starts on model-written text. They are made findable on purpose: the knowledge search an agent calls (`kb_search`, and `search_kb` for a host) returns Council records with a label naming them as a Council record (venue, number, date, decided or escalated) and their tier. §9 is the automatic half: a later Council is shown earlier rulings that match it.
3. **§12 question 3, a single family.** Unchanged: a Council of one family runs and is labelled (§3 item 3). No refusal is added; a host that wants more asks with `require_families` (§6).
4. **§12 question 4, the ceiling.** The default of `[budget] council_budget_tokens` is **400,000** (amends §4 item 1).

## 16. Corrections from the build (2026-11-17)

What building S1 to S5 showed the text above got wrong or left open. Each is the closest safe reading of the section it corrects; none widens what a host may do.

1. **§6, `require_families`.** A Council from a host always has the three default seats, so at most 3 families can sit: the range is 1 to 3, and 4 is refused with a sentence that says why. Seating M families moves seats onto unused usable families before Round 1; it never puts a critic on the author's family and never lowers a tier. `[seats] require_families` in the config applies to the Panel only, as before.
2. **§5 item 1, the endpoint of a failed seat.** A seat that times out on its first call has no receipt, so the endpoint is taken from the seat's last event in the task that names one (a budget reservation or a harness start included). A seat whose calls all fail comes back escalated with the error as its reason; it is cut and logged as an error is. A timeout writes no error-log entry, since it has no error text.
3. **§3, the orchestrator is not a seat.** The synthesis span writes a receipt too; on the daemon venue only the planned seats' receipts count for the families, the per-seat block and diversity.
4. **§4 item 2, the money ceiling.** `council_budget_usd` was never read from `config.toml` before (the 20.0 default always applied); it is now read, so a workspace that sets it gets the value it set.
5. **§7, what the tail prints at the end.** Only the state and the record's path, never the escalation or error reason, which may be model text.
6. **§9 and §15 item 2, records stay inside their project** (`specs/115`). A Council or Panel is shown earlier rulings, and a host's search returns Council records, only from missions it may see: a record of a home-only mission (no project scope) is shared, as the home layer is; a record of a project's mission is shown only inside that same project. A record of another project is never shown or returned.

## 17. Amendment (2026-11-21): a seat held at the barrier, and earlier rulings off per call

Two defects found in a review of 2026-11-20 (D5, D6). Neither widens what a host may do.

1. **§3, the state of a seat Delphi runs in a Panel (D5).** Before the Round 1 barrier releases, `council_status` reported such a seat as `answered` while its stance was only held. It now reports `waiting_on_barrier` (the word the seat progress list already uses), and `abstained` as soon as the seat abstains: an abstention carries no stance text, so showing it early does not break the blind round.
2. **§9 item 4, earlier rulings off for one call (D6).** `council_open` and `request_council` take an optional `earlier_rulings` (boolean, default `true`). `false` picks no earlier records for that Panel's round 3 brief or that Council's Round 2, repair turn and synthesis, whatever `[council] earlier_rulings` says; the record's front matter gains `earlier_rulings_off: by the caller` and a daemon Council's `council.started` carries `earlier_off: true`. A value that is not a boolean is refused before anything is recorded. Use: an experiment whose runs must not see each other's rulings.
