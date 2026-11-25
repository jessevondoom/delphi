---
id: spec-141-labels-enforced-at-the-sinks-max-label-inheritance-and-private-research-on-local-pages
title: "141: Labels enforced at the sinks: max-label inheritance, and private research on local pages"
author: architect
timestamp: 2026-11-25T21:00:00Z
status: draft
reviewed_by: []   # no Council or Panel has read this text. A daemon Council of 2026-11-25 ruled on the plan (two seats heard, the strategist seat abstained) and escalated two P0 findings to the operator, who answered them the same day (section 1). That is a review of the plan, not of this spec, and is not cited as one. This spec has an Authority section and should go to a Council or a witnessed Panel with a red-team seat before approval.
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors read on 2026-11-25 at origin/main 52bdd3b8 (0.6.7). Nothing was run. The operator's rulings are written from the coordinating session's relay of them, not from the operator's typed words.
source_mission: 2026-11-25-knowledge-v2
tags: [spec, labels, handling, private, egress, sinks, pages, export, bundle, authority]
---

# Spec 141: Labels enforced at the sinks

> **Status: draft**, revised 2026-11-25 with the operator's decisions of that day: the design now holds **no model-side and no seat-side filter**. One of five specs from the knowledge audit of 2026-11-25 (139 to 143). `specs/139` leaves private records off pages until this spec's guards pass; `specs/142` defines the `text_held` mark's source and carries a first guard for it.
> **Proposed build order (not yet confirmed by the operator):** the single-turn harness bridge (`specs/143`), the source text cache (`specs/142`), seeded pages (`specs/139`), the golden set (`specs/140`), **this spec**, then the one knowledge record and the later steps. Because the only claims on the operator's workspace are on private records (step 0), this order puts private research on pages last; `specs/139` OD-6 asks whether that wait is accepted.
> **Designs** the deferred slice S4 of [`specs/121`](121-data-sensitivity-private-internal-and-confidential.md) §10 (exit channels) and builds the page part of its §8. **Amends:** [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md) §7.2 (the foreign-family clause), [`specs/117`](117-the-encyclopedia-in-the-desktop-app-topic-pages-navigation-and-a-view-of-agent-memory.md) §2 (what a topic page may carry), [`specs/112`](112-one-research-record-a-single-runner-topics-that-persist-and-claims-that-reach-the-library.md) §3 (private research in the index), 121 §5 item 1, §6 item 8, §9 item 2 and §16 question 3. **Builds on:** [`specs/055`](055-the-central-library-sources-claims-and-pages.md) §10.1 and 098 §12 item 16 (the one egress check), [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md) §4 (the private-read mark, unchanged), [`specs/119`](119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) §15 (queries under the mark).
> **Not legal advice.** Where an employer's policy or a licence bears on what may leave, that is a question for counsel. This spec states a mechanism and the operator's rulings.

---

## 1. Scope and objectives

**The operator's rulings, 2026-11-25** (as relayed by the coordinating session; they narrow the two P0 findings the Council escalated):

1. Every model route configured on this machine is licensed and governed by the operator. Sending private material to any configured model, a Council seat of another family and a writer model included, **is not a leak to guard against**.
2. Web searches are considered safe, and research gathering is not risky.
3. So the guards apply **only to sinks that leave the operator's control**: export or publication of pages and bundles, the app, relay and sharing paths, and anything written to a public repository or sent to an outside person. Model dispatch and web search stay unblocked.
4. Private and internal research appears on local pages **once the guards pass their tests**. A page's label is the maximum of the labels of what it cites, computed by the daemon and never declared by the writer.
5. **Model access (decision 1).** Private and internal research may go to every configured model route, and Delphi's agents get the whole knowledge base. On this workspace `private_to_models` is `all`.
6. **Council seats (decision 2).** That includes seats of other model families; 121 §6 item 8 is lifted. The operator's reasoning, as relayed: third-party models are restricted on company accounts, most of the research was gathered by models in the first place, and withholding it from models is "security theater at best". Labels therefore guard only the sinks that leave the operator's control, and every model-side and seat-side filter is removed from this design.
7. **`personal` items (decision 3)** appear on local topic pages without the ask of 121 §5.
8. **Web queries (decision 4)** get an advisory `planned_with` notice and are never refused (§2.5).

| # | Observation (origin/main 52bdd3b8) | Where |
| :-- | :-- | :-- |
| O1 | One deny-by-default check guards every exit, with a registry and a test that fails when a path does not call it. It reads **one bit**: is this source library-private. | `egress.rs:12-17`, `:84-100`, `:186`; `library.rs:203-216` |
| O2 | The three handling traits of 121 are assigned and carried to derived records (slice S1, built). Nothing at an exit reads them: 121 §10 is deferred, and its §8 (labels on output) is not built. | `handling.rs:553`, `:999-1011`; tracker `121/S1-left.md` |
| O3 | A topic page is built at the external scope, so no private record reaches it; a private research record makes no topic and is not in the index; an `internal` hit is not gathered. | `library_topics.rs:44`, `:449`, `:580-584`; `research_index.rs:64-66`; `topic_gather.rs:77` |
| O4 | A library-private entry never goes to a foreign-family seat, whatever the setting; on the host's own family the setting decides, and its default in code is `deny` although 098 §7.2 says `allow`. | `egress.rs:21-29`; `config.rs:1146` |
| O5 | A page has no label of its own. The app writes a page to a file and prints it with no check. | `library_topics.rs:707`; `app/src-tauri/src/lib.rs:154`, `:165` |
| O6 | Two paths can carry text out and are not in the registry: the vectors written by `delphi kb seed-export`, and `delphi kb serve --allow-remote`. | `kb.rs:3390`; `kbserve.rs:623` |

Objective: every source, chunk, record and page carries a label the daemon computes; a thing made from other things carries the maximum of theirs; each sink that leaves the operator's control checks the label in one place, with a test for each sink; and when those tests pass, private and internal research may appear on local pages.

### 1.1 Non-goals

- **No block on model dispatch or on web search** (rulings 1 and 2). §2.5 adds a notice, never a refusal.
- **No change to the private-read mark** of 103: what sets it, what it holds. Rule (b) of the audit is kept.
- **No change to taint.** A label is not a `source_type`, counts for no promotion and unlocks no tool (121 §2 item 4, `handling_tests.rs:245`).
- **No model-side or seat-side filter.** This spec adds none and removes the one that exists (§2.4 item 2). It builds neither the reader grant for confidential (121 §6, slice S3) nor the placeholder-and-ask (121 §5, slice S2); whether those are still wanted after rulings 5 and 6 is OD-7.
- **No enforcement inside a host's own tools** (121 §9 item 1). §2.4 item 6 gives the host a check it can run.
- **No access control between people.**

### 1.2 Simpler alternative considered

- **Alternative:** keep the one bit. Mark every page that used a private record `private: true` and let today's `egress::check` refuse it.
- **Why rejected:** one bit cannot say "internal: may go to a colleague, labelled" against "private: never", so it would either refuse everything internal or let everything through. It also cannot express licence-held text, which may leave as short quotations and not whole (rule (a)). And the traits of 121 already exist as a set; a second, coarser flag beside them would be a fifth meaning of "private" (121 §1, E-1).

### 1.3 Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | one computed label type; the existing egress check takes it; each sink has a policy row and a test | for every sink and every mark, a labelled fixture is sent through the sink's real entry point and the outcome is compared with the row |
| B | constraint removed: "private material may be read anywhere inside" | filter at retrieval: an agent, a seat or a writer never receives a labelled passage unless its route is local | a foreign seat's prompt holds no labelled passage. This is what the Council asked for and what rulings 1 and 3 decline |
| C | inversion: label the destinations, not the data | each sink declares the highest label it accepts; data flows anywhere below it | the same matrix as A, read by column |

Chosen: **A**. B is ruled out by the operator. C is A's table turned on its side and needs an order between marks, which 121 §2 item 3 denies. Flagged by the guard (a new layer of abstraction with no test of its own): none; `Label` is tested by the matrix of AC-4.

## 2. Design

### 2.1 The label

1. A **label** is a set of marks. Five marks exist:

| Mark | Shown as | Set by (already built unless said) |
| :-- | :-- | :-- |
| `personal` | private | 121 §3 (`handling.rs`) |
| `internal` | internal | 121 §3 |
| `confidential` | confidential | 121 §3 |
| `library_private` | private (library) | a source's `private: true` or licence `private` (`library.rs:203`); a research record's `private` (`research_record.rs:336`, `:583`); a claim that rests on such a source (`library_claims.rs:1391`) |
| `text_held` | text kept for local reading | a source whose whole text is kept though no licence allows copying it (`kept_privately` today, `library.rs:809`; `specs/142` §2.5) |

2. **Maximum means union.** The marks are not a ladder (121 §2 item 3), so the least label that is at least as strict as each input is the union of the inputs. `Label::join` is that union. An empty label is unlabelled.
3. **Computed, never declared.** `Label` values are made only by the daemon, from its own records: the handling record (`handling.rs:316`), the source's `meta.json`, the research record. A label written in a file's frontmatter, a tool argument, a writer's draft or a host's report is a view or a note and is never read as the label. An agent or a host may still **add** a handling trait (121 §3 item 4); nothing lowers a label but the operator (121 §3 item 3) or, for `library_private` and `text_held`, the operator's own source commands.
4. **Unreadable is strictest.** A record that cannot be read has every mark, at every sink (as `egress.rs:13-15` does for the one bit).
5. **What a label covers.** The item's text and everything derived from it by code: title, slug, anchors, excerpts, vectors, chunk headers. A sink that refuses an item sends none of them.

### 2.2 Where the label is kept

| Thing | Label | Kept in |
| :-- | :-- | :-- |
| source | its handling set, `library_private`, `text_held` | as today; no new store |
| record (claim, finding, note, research record, summary) | the join of what it used (121 §4 item 1, `handling.rs:553`) and of its sources' library marks | as today, plus the research record's `label` |
| chunk | the label of its document | a new column `marks` of `kb_documents`, written at sync from the daemon's records (`handling::item_of`, `handling.rs:146`), never from the document's frontmatter; a hit carries it |
| page | the join over **everything in its material**: every seed span, claim, source and record gathered for the build, cited or not | the sidecar's `label` (`marks`, and for each mark the ids it came from); `handling:` in the page's frontmatter as a view; the visible line of 121 §8 items 3 and 4 at the top |

A page's label is taken from its material and not only from its markers, because a writer can restate an uncited record (121 §9 item 3). Material contains everything cited: a marker that names something outside it already fails the page check. The label is recomputed at each build (121 §8 item 6) and when a trait of a material item changes (the page is then due).

### 2.3 One check

1. `egress::check_label(label, sink) -> Decision`, beside `egress::check` (`egress.rs:12`), with `Decision` one of `Send`, `SendLabelled` (the output carries the visible line and the operator confirms the send) or `Refuse(reason)`. `egress::check` stays and is implemented by it.
2. The registry `PATHS` (`egress.rs:84`) becomes the table of §2.4: each sink, the file that sends, and its policy for each mark. The enumeration test (`egress.rs:186`) requires each wired sender to call `check_label` with its own sink.
3. A refusal names the marks and the items they came from to the operator, and names no text.

### 2.4 The sinks and their policy

Only sinks that leave the operator's control (ruling 3). `R` refuse; `L` send labelled, after the operator confirms; `S` send; `Q` whole text refused, quotations of at most 300 characters per citation pass (119 §3 item 2, `library_private.rs:20`).

| Sink | Sender | `library_private` | `personal` | `confidential` | `internal` | `text_held` |
| :-- | :-- | :-: | :-: | :-: | :-: | :-: |
| a page or document written out or printed: the app's export and print, `delphi pages` output to a file | `service.rs` (`pages_get` gives `label` and `may_export`), `app/src-tauri/src/lib.rs:154`, `:165` | R | R | R | L | Q |
| a bundle: sources copied in, distill, package, publish, export | `library_export.rs:41`, `delphi-bundle` | R | R | R | L | Q |
| a bibliography export | `library_bib.rs:67` | R | R | R | L | S (a citation is not the text) |
| a Xenia publish | `service_xenia.rs:108`, `xenia_cmd.rs:380` | R | R | R | L | Q |
| vectors written out (`kb seed-export`) | `kb.rs:3390` | R | R | R | R | R |
| `kb serve` on a non-loopback address | `kbserve.rs:623` | R | R | R | R | R |
| a public repository | `worktree.rs:525` (the daemon's push) and `delphi handling check` (item 6) | R | R | R | L only to a remote on `[handling] internal_remotes`, else R | Q |
| mail, relay, connector and other outbound sends | `private_read.rs`, `connector_run.rs:160`, `harness_run.rs:508` | held by the mark, as today | the Red card names the marks (item 5) | the same | the same | Q |

1. **Not sinks under this spec:** a model prompt on any configured route (ruling 1), a web search query (ruling 2; §2.5), the operator's console, and the paired app reading the daemon on this machine (the operator's own view, 115 §11.3). The debug bundle, logs and telemetry carry no entry text and keep their proofs (`egress.rs:89-95`).
2. **No filter on the way to a model.** `check_for_seat` (`egress.rs:21-29`) no longer refuses: a labelled entry may enter the prompt of any configured route, a foreign-family seat included (rulings 5 and 6). `[library] private_to_models` gains the value **`all`**, which becomes the shipped default (OD-1); `deny` and `allow` are still read, so an operator elsewhere can set the older behaviour, and this spec designs nothing further on that side. On this workspace the value is `all`. Retrieval, start passages, a writer's brief and a seat's evidence are filtered by no mark.
3. `SendLabelled` at a command asks at the terminal; at the app it is a confirmation in the app; with no person to ask it is refused, never sent.
4. A labelled page handed to a sink is refused or sent **whole**: the daemon never strips the labelled parts and sends the rest, because what is left may still carry their substance.
5. **Outbound sends.** Every outbound action is already Red and waits for the operator. The card gains one line: the marks of the items the sending task was given (`handling.rs:255`), with counts. Handling traits still never set the 103 mark (121 §2 item 4); the line adds what the operator is told and removes nothing.
6. **A public repository.** One check, `delphi handling check <path>... | --staged | --range <a>..<b> [--remote <url>]`: it reads each file's label (the page's or document's sidecar and the daemon's record, never the file's own frontmatter alone), scans for a run of more than 300 characters of any `text_held` or `library_private` source (`library_private::shares_run`, `library_private.rs:66`), applies the row above and exits non-zero on a refusal. It runs in two places. **The daemon's own push** (a task finished in `merge = "pr"` mode, `worktree.rs:504-525`, after the operator's card, `finalize.rs:317-321`) runs it on the branch's range before pushing; a refusal stops the push and the card names the files and marks. An agent's own `git push` is refused by the command policy (`exec.rs:536`, and the test at `:1035`). **A host** commits and pushes with its own tools, which Delphi cannot stop (121 §9 item 1): `delphi connect` offers the check as a pre-push hook, and a hook can only refuse (GOVERNANCE §5 item 11).

### 2.5 Web queries: a notice, never a refusal

1. **Chosen: an advisory notice.** When a query is planned by a span that was given labelled items, the planned-query reply to a host (119 §13.1), the search log line and the research result carry `planned_with: [marks]`, beside today's `planned_under_mark` (`research_record.rs:548`). `delphi handling status` counts such queries. This builds 121 §9 item 2. Nothing is withheld for it.
2. **Why a notice.** The alternatives were a refusal, which ruling 2 excludes, and a card for every such query, which stops research for a question the daemon cannot answer: it knows the span was *given* a labelled item, not whether the query *contains* its substance (121 §9 item 2). A person can tell. In a host session the host runs the search with its own tools, so the notice arrives where a person or the host's own policy can act on it.
3. **An operator-side policy, noted here, not a Delphi block.** The operator's employer has an AI policy that asks a person to confirm a web query that would contain confidential data. Delphi does not enforce that policy and does not claim to satisfy it. The notice gives the person, or a host that carries the policy in its own instructions, the fact they need: this query was planned with private, internal or confidential material in view.
4. What already limits a query is unchanged: the cleaning of 066, and the identifier guard for the words the operator configured (`query_guard.rs`, 119 §2 item 7), which still withholds a query that names one.

### 2.6 The lift: private and internal research on local pages

Built last, in its own slice, after the sink tests of §6 pass on main.

1. A private research record makes a topic (`library_topics.rs:449`) and its claims join the page's material (`:580-584`).
2. Topic pages are built at the internal scope (`SCOPE`, `library_topics.rs:44`). A seed (139 §2.1 item 4) takes spans from private records, from hits flagged `internal`, and from ingested project research files, and the writer's prompt receives them, whatever family the writer's model is.
3. A private research record and its answer are listed in the index with their marks (`research_index.rs:64-66`), filed apart as claims on private sources are today (`research_index.rs:105-109`), so a session that is handed one is marked as today (115 §11.1).
4. **Nothing is left out for a mark.** A `confidential` item may be in a page's material like any other; the page then carries `CONFIDENTIAL` and no sink sends it. (121 §6 item 1 is not built, so no reader is filtered today either; OD-7.)
5. **`personal` on a local page (ruling 7).** 121 §5 item 1 counts a page as work output and asks for a placeholder and an ask. A topic page in the library is the operator's local view: a `personal` item appears on it, labelled, with no ask. The page can never leave (§2.4). Documents, decks and exports stay under 121 §5.
6. An operator who sets `private_to_models = "deny"` or `"allow"` gets the older behaviour for written pages on the routes that value excludes: the seed page, which no model writes, still carries private material, and `delphi kb health` says in one line that written pages are leaving it out and which key changes that.

## 3. Authority

- **What this widens.** (1) A labelled entry, library-private and confidential included, may enter the prompt of any configured model route, a foreign-family seat included. 098 §7.2 forbade that for a foreign family "whatever the setting", and 121 §6 item 8 for confidential. This is the operator's ruling (5 and 6), and with OD-1 it becomes the shipped default. (2) Private research becomes visible on local pages and in the index of the operator's own agents. Neither lets any agent do a new thing; both widen what is read and what is sent to a model provider.
- **What this narrows.** The sinks of §2.4 now refuse or ask on five marks where they read one bit; two unregistered exits (vectors, remote serve) are closed to labelled material.
- **Content cannot lower its own label.** §2.1 item 3; the test of AC-2.
- **Not enforced, said plainly.** A host that writes or pushes with its own tools (121 §9 item 1); a paraphrase a model writes into an unlabelled reply (121 §9 item 3); software running as the operator (121 §9 item 6). A model on a configured route that misuses what it was sent is the operator's accepted risk under ruling 1.
- **The Council's findings.** P0 "leak paths before sink enforcement": this spec is that enforcement, and the lift waits for its tests. P0 "foreign-host exfiltration by handoff" and P1 "foreign seats and cloud writers": answered by ruling 1, recorded in §1. P1 "metadata and embedding side channels": §2.1 item 5 and the vector and serve rows; inside the machine they are not sinks.

## 4. Slices

| Slice | Content | Criteria | Files | Hours | Depends on |
| :-- | :-- | :-- | :-- | --: | :-- |
| S0 | this spec; the operator's answers to §8; a Council or Panel with a red-team seat | (the spec) | this file | 2-3 | none |
| A | `Label`: the type, the join, of a source, a record, a chunk (`marks` column, the hit) and a page (sidecar, frontmatter view, the visible line) | AC-1, AC-2, AC-3 | `label.rs` (new), `handling.rs`, `kb.rs`, `library_topics.rs` | 8-10 | S0 |
| B | `check_label`, the sink table in the registry; bundle, bibliography and Xenia wired | AC-4, AC-5 | `egress.rs`, `library_export.rs`, `library_bib.rs`, `service_xenia.rs`, `delphi-bundle` | 6-8 | A |
| C | pages written out: `may_export` in the page reply, the app's export and print, `kb seed-export`, remote `kb serve` | AC-6, AC-7 | `service.rs`, `app/src-tauri/src/lib.rs`, `kb.rs`, `kbserve.rs` | 5-6 | B |
| D | `delphi handling check` and the hook; the marks line on outbound cards | AC-8, AC-9 | the `handling` command, `worktree.rs`, `finalize.rs`, `private_read.rs`, `harness_run.rs` | 6-7 | B |
| E | the query notice: `planned_with`, the log line, the status count | AC-10 | `research_run.rs`, `search_log.rs`, `research_record.rs` | 3-4 | A |
| F | the lift of §2.6; `private_to_models = "all"` and its default; `check_for_seat` no longer refuses; the health line | AC-11, AC-12, AC-13 | `library_topics.rs`, `topic_gather.rs`, `research_index.rs`, `egress.rs`, `config.rs` | 4-5 | B, C, D green on main; 139 A and B |
| | **Total** | | | **34-43** | |

F is refused by its own test (AC-13) while any sink row has no passing test.

## 5. Acceptance criteria

1. **AC-1:** the join of two labels is their union; the label of a claim anchored to a library-private source and written from a `personal` email holds both marks; an unreadable record has every mark.
2. **AC-2:** a page whose draft, frontmatter or closing comment declares a label, or declares none, is stored with the label computed from its material; a tool argument naming a lower label changes nothing; an agent's call cannot remove a mark.
3. **AC-3:** a page built from one internal and one library-private record carries both marks in its sidecar with the ids they came from, `handling:` in its frontmatter, and the headline line; when the internal trait is cleared by the operator the page is due and its next build drops the mark.
4. **AC-4:** for every row and column of §2.4, a fixture with that one mark sent through the sink's real entry point yields the decision in the table.
5. **AC-5:** a bundle package that includes a page with `text_held` material carries no run of more than 300 characters of that source's text; a refusal message names marks and item ids and no passage.
6. **AC-6:** the app's export and print of a `personal` or `library_private` page are refused with the marks shown; of an `internal` page they ask once and the file begins with the visible line; of an unlabelled page nothing changes.
7. **AC-7:** `kb seed-export` writes no vector of a labelled chunk and says how many were left out; `kb serve --allow-remote` returns no labelled document; on loopback its results are unchanged.
8. **AC-8:** `delphi handling check --staged` exits non-zero for a staged file that is a labelled page (and the daemon's push of a branch that holds one is stopped, with the file named on the card), and for an unlabelled file that holds 301 characters of a `text_held` source; it passes an `internal` file only with `--remote` on the configured list.
9. **AC-9:** the Red card of an outbound send by a task that was given two `personal` items shows `private (2)`; the same task's private-read mark is unchanged by it.
10. **AC-10:** a query planned by a span that was given a `confidential` item is sent as it would have been, and the reply, the search log and the result carry `planned_with: ["confidential", "internal"]`; no query is withheld for a mark.
11. **AC-11:** after the lift, a private research record has a topic and a seed page labelled `library_private`; with the default (`all`) a foreign-family writer's prompt and a foreign-family Council seat's evidence hold its text; with `deny` set, the writer's prompt holds none of it and the seed page still does.
12. **AC-12:** a `confidential` item gathered for a page is in the writer's prompt and on the page, the page's headline is `CONFIDENTIAL (internal)`, and every sink of §2.4 refuses the page.
13. **AC-13:** the lift's guard test fails when any sink of §2.4 lacks its named test or when that test is ignored.

## 6. Verification plan

| Criterion | How it is verified |
| :-- | :-- |
| AC-1 to AC-3 | unit tests in `label.rs`; library tests with a scripted writer that declares a false label |
| AC-4 | the matrix test below, one named test per sink |
| AC-5 | a bundle test that scans the archive (the scan of 119 AC-4, extended) |
| AC-6 | daemon tests for `may_export`; the app's own tests for export and print |
| AC-7 | CLI tests on a fixture index |
| AC-8 | CLI tests on a temporary repository |
| AC-9, AC-10 | daemon tests; the separation test of 121 (`handling_tests.rs:245`) unchanged and still passing |
| AC-11 to AC-13 | engine tests with pools of one and two families |

**Fitness functions: one per sink** (owner `red-team`; built by `builder`; all to be written unless said):

| Sink | Test that fails when the sink stops checking |
| :-- | :-- |
| page export and print | `egress::tests::sink_page_export_follows_its_row` and the app test `export_refuses_a_private_page` |
| bundle | `egress::tests::sink_bundle_follows_its_row` |
| bibliography | `egress::tests::sink_bibliography_follows_its_row` |
| Xenia | `egress::tests::sink_xenia_follows_its_row` |
| vectors | `egress::tests::sink_seed_export_writes_no_labelled_vector` |
| remote serve | `egress::tests::sink_remote_serve_returns_no_labelled_document` |
| public repository | `egress::tests::sink_repository_check_follows_its_row` |
| outbound | `egress::tests::sink_outbound_card_names_the_marks`; the mark's own tests (`private_read.rs`), exists (read) |

| Invariant | Check that would fail | State |
| :-- | :-- | :-- |
| every sender is registered and calls the check | `egress::tests::every_registered_egress_path_calls_the_one_check` (`egress.rs:186`), extended to `check_label`; `NETWORK_FILES` (`egress.rs:312`) | exists (read), to be extended |
| a label is computed, never declared | `label::tests::no_writer_tool_or_frontmatter_lowers_a_label` | to be written |
| a label decides no trust | `handling_tests::no_trust_decision_reads_handling` (`handling_tests.rs:245`), extended to `label.rs` | exists (read), to be extended |
| the lift waits for the sinks | `label::tests::the_lift_requires_every_sink_test` (AC-13) | to be written |
| model dispatch and web search are not blocked by a mark | `label::tests::no_mark_withholds_a_query_or_a_prompt_under_all` | to be written |

### 6.1 Premortem (three months after release): how this most likely fails

| # | Cause, as it happened | Likely | Damage | Early sign | Response in this spec |
| :-- | :-- | :-- | :-- | :-- | :-- |
| 1 | A sink nobody listed carried a labelled page out: a new export command, or a document tool that embedded a page. The registry only knows the senders it was told about. | medium | high | a file outside the home that holds a page's visible line | the registry test fails for a network sender that is not listed (`egress.rs:312`); writing a page's text to a path outside the home goes through `may_export` (slice C); AC-13 |
| 2 | Everything became labelled: one private record in a broad topic made the whole page private, so no page could be exported and the operator stopped trusting labels. | high | medium | the share of pages with a refusing mark in `delphi kb health` | the sidecar says which items gave each mark, so the operator can clear or remove one; whole-page refusal is kept on purpose (§2.4 item 4); OD-5 |
| 3 | With no filter before a model, a confidential document the operator had forgotten was in the workspace went into a foreign-family seat's prompt during a Council on something else. | medium | medium to high, by what the document was | a seat receipt for a foreign family in a mission whose given list holds a `confidential` item | accepted by rulings 5 and 6, and said in §3; `delphi handling status` counts labelled items by mark so the operator can see what the knowledge base holds; OD-7 keeps the question of a confidential grant open |
| 4 | A host pushed a labelled file to a public repository with its own git. | medium | high | a commit that holds a visible line | stated limit (§3); the hook is offered at `delphi connect`; the visible line makes it findable afterwards |
| 5 | The notice was noise: every query in a workspace full of internal notes carried `planned_with`, and nobody read it. | high | low | the count in `handling status` equals the count of all queries | accepted: the notice is cheap; if it proves useless it is removed by amendment |
| 6 | The label was computed from stale records: a trait cleared by the operator still refused an export a day later. | medium | low | an export refused for a mark `handling status` no longer shows | the label is recomputed at the sink from the daemon's records, not read from the sidecar, when the sidecar is older than the handling record |

Not acted on: 4 beyond the hook (enforcing inside a host's tools is outside what Delphi can do), 5.

## 7. What is not verified

- **The operator's rulings are relayed**, not quoted. The operator confirms §1 by approving this spec.
- That every path that can write a page's text outside the home is found. Slice C begins by listing them; the list goes into the registry.
- That the handling "given" list is fine enough for the outbound card: it is kept per mission and role, not per span (tracker `121/S1-left.md`), so the card may name marks the sending span never saw.
- That `delphi-bundle`'s package and export paths can be given a label without that crate reading the engine's records. If not, the check runs in the engine before the bundle crate is called.
- That whole-page refusal leaves enough pages exportable to be useful.
- Whether the employer's policy is met by a notice. That is the operator's and counsel's to judge (§2.5 item 3).

## 8. Decisions for the operator

| OD | Question | Recommendation | Reversible? (operator decides) | Blocks |
| :-- | :-- | :-- | :-- | :-- |
| OD-1 | `private_to_models`: this workspace is `all` (**decided, operator, 2026-11-25, as relayed**). Should the **shipped** default also become `all`? | yes: the operator's reasons are not particular to this machine, and a `deny` default hides the knowledge base from an operator's own agents; `deny` and `allow` stay as values another operator can set. The alternative keeps `deny` as shipped and sets `all` here only | proposed: yes | F |
| OD-2 | Web queries planned with labelled material in view | **Decided (operator, 2026-11-25, as relayed): an advisory `planned_with` notice, never a refusal**, as §2.5 | decided | none |
| OD-3 | Does the model-route ruling lift 121 §6 item 8? | **Decided (operator, 2026-11-25, as relayed): yes**, seats of other families included | decided | none |
| OD-4 | May a `personal` item appear on a local topic page without the ask of 121 §5? | **Decided (operator, 2026-11-25, as relayed): yes** | decided | none |
| OD-5 | Export of a page with a refusing mark: refuse outright, or let the operator override at a terminal for one page? | refuse outright in the first cut; an override would be a new operator act and its own amendment | proposed: yes | C |
| OD-6 | Printing from the app: the same rule as export? | yes | proposed: yes | C |
| OD-7 | After rulings 5 and 6, are 121's unbuilt slices still wanted: the reader grant for confidential (§6 item 1, S3) and the placeholder-and-ask for documents and decks (§5, S2)? "Agents get the whole knowledge base" reads as withdrawing the grant. | withdraw S3 by an amendment to 121; keep S2 for documents and decks, which do leave | proposed: yes | none (121) |
| OD-8 | Project-history records have their own model-side setting (`[history] to_models`, spec 102, `history_record.rs:745`). Does ruling 5 reach them? | leave it as it is until asked; it is another spec's rule and a different kind of record | proposed: yes | none |
