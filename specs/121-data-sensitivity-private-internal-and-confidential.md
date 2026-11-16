---
id: spec-121-data-sensitivity-private-internal-and-confidential
title: "121: Data sensitivity: private, internal and confidential"
author: architect
timestamp: 2026-11-15T00:00:00Z
status: approved   # by the operator, 2026-11-15, in a host session: "approve spec 121 and merge 740 when ready", after the draft, its slices and its five open questions were shown. The open questions of the last section stay open: until the operator answers each, the provisional behaviour the draft names for it is what is built. Written from the operator's words of that day (quoted in section 1) and the ruling of a Panel (council t_0001, PROCEED-WITH-CHANGES) as relayed to the author. Section 17 (2026-11-16) is an amendment written from the operator's words of that day ("4: yes", on the two choices the S1 builder reported); it takes effect when the operator merges its pull request.
approved_by: operator (2026-11-15)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: low   # written from the text of specs 098, 103, 113, 115 and 119 on main (582f4692) and a short look at kb_import.rs; nothing was run. Every mechanism below is NOT VALIDATED.
source_mission: 2026-11-14-knowledge-loop
tags: [spec, privacy, sensitivity, handling, internal, confidential, labelling, knowledge]
---

# Spec 121: Data sensitivity: private, internal and confidential

> **Status: approved by the operator, 2026-11-15** (see the frontmatter). Until each open question of §16 is answered, the provisional behaviour named for it is what is built.
> **Narrows or replaces**, from its approval on 2026-11-15: parts of [`specs/103`](103-the-private-read-mark-closing-the-four-unaudited-egress-channels.md), [`specs/113`](113-host-led-research-ingest-a-record-tool-and-workers-that-can-capture.md), [`specs/115`](115-research-recall-across-projects.md), [`specs/119`](119-research-readiness-setup-private-text-for-retrieval-health-checks-and-a-live-proof.md) and [`specs/098`](098-where-agents-run-host-first-locus-and-the-inside-exceptions.md). §11 lists the exact text. This draft edits none of those specs.
> **Five questions are open for the operator** (§16). The draft names a behaviour for each so that it can be read end to end; none of the five is decided.

---

## 1. Scope and objectives

The operator's words, in a host session on 2026-11-15, as typed:

> "i hate to expand scope but i want to clarify some of the private/internal concepts. priate should be data gathered from an operator's email, DMs, or a meeting with no more than two people. anything private should be visible to the operator and to agents but must not be directly cited in work output like docs or decks. if it's helpful to make the case, an agent should ask the operator how or if to include it and only do so with permission. internal is anything that's internal to an organization or team, marked as internal-only, etc. internal can be private and vice versa — they are not an escalation path to one another they are permission traits of the data. confidential further restricts internal data, meaning it's internal and limited in scope. confidential data should not be available for PRD documents or decks — in limited advisory capacity the strategist should be able to be granted confidential access but explicitly by the operator. internal data should be treated as trusted if it pertains to specifics defined by the organization or team, and always cited properly with tagging, marking, or frontmatter (as available) labeling any work output as internal, confidential, or private to correspond with its status. does this make sense? much of it is in the specs already but there may be additional nuance?"

And, to the Council that reviewed the question: "i'll bias towards inclusion and quality of result over exclusion and security".

A Panel (council t_0001) ruled PROCEED-WITH-CHANGES. This draft is built on its consensus as relayed.

| # | Observation (2026-11-15) |
| :-- | :-- |
| E-1 | "Private" already has four meanings in the specs, each with its own mechanism: a library source the daemon withholds from models and from every exit (098 §7.2, 103 §2); a project that shares none of its research (115 §2 item 2); source text kept for licence reasons (`private_text`, 119 §3); and an exchange record (spec 102). None of them is what the operator described: material from the operator's own mail, direct messages and two-person meetings, which agents may read and work output may not cite. |
| E-2 | "Internal" is today a list of four import kinds (`private-doc`, `slack`, `wiki`, `email`; `kb_import.rs::is_internal`) and a source kind a host may declare (113 §2 item 1). Email is counted as the organisation's shared material, which the operator's words contradict. |
| E-3 | "Confidential" does not exist. |
| E-4 | The only rule about what work output may say is the quoting rule of 103 §7.1 (rows 6 and 7), which is an instruction to agents and has no design (slice M5, not started). |
| E-5 | Nothing carries a label from an item to the records made from it. A claim, a finding, a summary or a handoff written from an email looks like any other. |
| E-6 | No work output is labelled by the sensitivity of what went into it. |

Objective: three named traits that say how a piece of data is to be handled; the daemon assigns them and carries them to whatever is made from the data; they change what work output may contain and who may retrieve confidential material; and every work output says, in its frontmatter and on its face, which of the three it carries. Reading stays as easy as it is today, except for confidential.

### 1.1 Non-goals

- **Not a trust tier and not a ladder.** The traits are not `source_type` values, do not enter the taint lattice of GOVERNANCE §4 item 3, and are not ordered against one another.
- **Not the private-read mark.** No trait sets or clears the mark of spec 103, makes a project private (spec 115), or changes what `private_text` keeps (spec 119).
- **No new exit rule in the first three slices.** Bundles, Xenia, the federation and public remotes are §10, deferred.
- **No enforcement inside a host's own tools.** §9 says what that means.
- **No access control between people.** One operator, one machine. This spec is about what agents retrieve and what output says, not about which human may open a file.

### 1.2 Simpler alternative considered

*Build M5 of spec 103 (the quoting rule) and add a `confidential` flag to the library.* Rejected: the quoting rule covers three import kinds and says nothing of meetings, direct messages or the records derived from them; a flag on sources does not reach claims and summaries, where the substance travels (E-5); and reusing the word "private" for a fifth thing would make the egress mark, the private project or `private_text` fire by accident. Also considered: *one ordered scale (public, internal, private, confidential).* Rejected by the operator's own words: internal and private "are not an escalation path to one another".

---

## 2. Three traits, under their own name

1. The traits are a set on each item, called **handling** in code, files and config. Its values are `personal`, `internal` and `confidential`. An item with an empty set is unlabelled.
2. What the operator sees is the operator's words: `personal` is shown as **private**, the other two as **internal** and **confidential**. The code name differs from the display name on purpose, so that no existing check on the word `private` reads this trait (E-1).

| Trait (code) | Shown as | Means | Effect, in one line |
| :-- | :-- | :-- | :-- |
| `personal` | private | gathered from the operator's own email, direct messages, or a meeting of no more than two people | readable; not cited directly in work output without the operator's permission (§5) |
| `internal` | internal | internal to the organisation or team, or marked internal-only | readable; authoritative for the organisation's own specifics when it arrived by a configured path (§7); output is labelled |
| `confidential` | confidential | internal and limited in scope | retrieved only by a role the operator has granted (§6); refused by PRD and deck kinds; output is labelled |

3. **Independence.** `personal` and `internal` are independent: an item may carry either, both or neither. A one-to-one email between two colleagues is both. `confidential` implies `internal`: the daemon adds `internal` whenever `confidential` is set, and removing `internal` from a confidential item is refused until `confidential` is removed.
4. **What the traits never do.** They never change an item's `source_type`, never count toward the promotion threshold of GOVERNANCE §4 item 3, never unlock or remove a tool, and never set the spec 103 mark. An item can be `user_ratified` and personal, or `external_untrusted` and internal; the two axes do not meet.
5. **The older meanings keep their names and their mechanisms.** A library source with `private: true` or licence `private` is still withheld as 098 §7.2 and 103 §2 say; a private project is still spec 115's; `private_text` is still spec 119's. In prose from here on, "private" alone means the `personal` trait as displayed, and the others are written "library-private", "private project" and "`private_text`".

## 3. Assignment

The daemon computes the set. An agent never lowers it.

1. **By arrival path.** The operator configures which paths carry which traits: a `[handling]` table in `config.toml` maps a connector or import kind (and, where the connector gives them, a folder, channel, space or address list) to traits. Only the operator edits it; it is a protected path. Proposed defaults, used when the operator has configured nothing for a path:

| Arrives as | Default set | Note |
| :-- | :-- | :-- |
| the operator's email, one sender to few recipients | `personal` | plus `internal` when every address is on the organisation's configured domains |
| email sent to a configured list address, or to more than 10 recipients (company-wide) | `internal` | not `personal` |
| a direct message (one to one) | `personal` | plus `internal` when in the organisation's workspace |
| a channel message (`slack` kind, not a direct message) | `internal` | |
| `wiki`, `private-doc` kinds | `internal` | |
| a meeting record with 1 or 2 participants | `personal` | |
| a meeting record with 3 or more participants | not `personal` | `internal` when every participant is on the organisation's domains or the meeting's path is configured internal; otherwise unlabelled |
| a meeting record whose participants are not known | unlabelled | counted for review (item 5) |
| anything else, any path with no rule | unlabelled | readable, counted for review |

2. **By a marking in the content.** Words such as "internal only" or "confidential" in a document's header, footer, frontmatter or classification line set the trait **only on a path the operator configured**. On any other path (a web page, a record a host hands over, federated content) a marking sets nothing: it is shown beside the item as a note ("says: confidential") and counted for review. Text from outside cannot give itself the weight of §7 or hide itself behind §6.
3. **By the operator.** `delphi handling set <item> <trait>` and the console's equivalent add a trait; `delphi handling clear <item> <trait>` removes one (declassifies). Clearing is the operator's alone, at a terminal or the console, and writes a `handling.cleared` event with the item and the trait.
4. **By an agent, adding only.** An agent (or a host, through `record_research`) may add a trait to an item it writes or reports. An added trait restricts and labels as any other, and is recorded as added by that role. An `internal` added by an agent or a host never carries the weight of §7, which needs a configured path or the operator.
5. **Unknown favours inclusion.** An item with no rule is readable and unlabelled. `delphi handling status` and one line of `delphi doctor` count the unlabelled items by path and the markings seen on unconfigured paths, so the operator can add a rule. Nothing is withheld while it waits.
6. **Where the set lives.** The authority is a record the daemon keeps under a protected path (item id, content hash, each trait, who set it, when). An item's frontmatter shows `handling: [...]` as a view of that record; a frontmatter edit that drops a trait is put back at the next write or reconciliation and logged. Who set a trait is one of `path`, `marking`, `operator`, `agent:<role>`, `host`, `derived` (§4).
7. **Reconciliation.** `delphi handling reconcile [--dry-run]` applies the rules to what the workspace already holds: knowledge-base documents by kind and path, meeting records, library sources a host declared `internal` or `person`, and then the records derived from them (§4). It prints counts by trait and path, changes no `private` flag, removes no trait an operator or agent set, and is safe to run twice. The dry run is the default the first time.

## 4. Propagation

A label that stays on the email and not on the claim written from it protects nothing (E-5). Propagation on writes is part of the first slice.

1. **Used.** A record written by a span carries the union of the traits of the items it **used**: the items its claims are anchored to, the items its quotes come from, the items its citation markers name, and the items a handoff or summary lists as its inputs. This applies to claims, findings, notes, summaries, session summaries, handoffs, decision records, Council evidence and a strategist's advice. The trait is recorded as `derived`, with the ids of the items it came from.
2. **Read, not used.** For labelled items the span was given and did not use (by the enumerated readers of 103 §2), the record carries a weaker note, `may_rest_on`, with the item ids and their traits. The note changes no behaviour for `personal` and `internal`: it is shown to the operator beside the record and counted. This is a stated limit (§9 item 3).
3. **Under a confidential grant, everything written is labelled.** A span that holds a grant of §6 writes records that carry `confidential` (and so `internal`) whether or not they cite a confidential item, from the first confidential item it is given until the task or session ends.
4. **Derived traits follow the source.** When the operator clears a trait from an item, records that carry it only as `derived` from that item lose it at the next reconciliation. A derived trait is otherwise removed only by the operator.
5. **Across projects.** A shared claim (spec 115) carries its traits with it; the label a search result prints includes them.
6. The computation is the daemon's, at the write, from the ids the span was handed and the ids the record names. No tool argument lowers it.

## 5. Private in work output: a placeholder and one ask

Private material is readable by the operator and by agents (subject to §16 questions 1 and 3). The rule is about output.

1. **Work output** is a document, deck, page or export that Delphi writes through its own document path ([`specs/095`](095-documents-and-dataviz.md)) or its page and export paths. Notes, claims, handoffs and replies to the operator in the session are not work output; they carry labels (§4) and are not checked by this section.
2. **Cited directly** means any of: a quotation from a `personal` item; a statement attributed to the person or the message ("Dana said", "per the 11-03 email"); a citation marker that names the item. The check at write time looks for citation markers and attributions that resolve to a `personal` item, and for a verbatim run of 40 or more characters from a `personal` item the span was given.
3. **The draft carries a visible placeholder** where the private material would go: `[private: 1 of 3, held for the operator]`. The agent writes the rest of the document as well as it can without it.
4. **One batched ask per document.** When the draft is otherwise complete, the agent asks the operator once. The ask lists each held item: whose words they are (the sender or speaker, from the item's own metadata), where and when they were said, the passage, and where in the document it would go. For each, the operator chooses **include** (quoted or attributed), **paraphrase unattributed**, or **drop**; one answer may cover the whole list.
5. **The permission is narrow.** It binds the listed items, for that output, and is recorded with the operator's answer. It never removes the trait from the item, never covers another document, and never covers an item added to the same document later: a later revision asks once more, for the new items only.
6. **The output keeps the label.** A document that includes or paraphrases private material by permission is labelled private (§8). A document from which every held item was dropped is not.
7. **Headless runs.** With no operator to ask, the placeholders stay, the run's summary and the document's frontmatter list them (`handling_pending`), and the document is marked a draft until the operator answers. A run is never failed for this.
8. **Permission is the operator's.** It is given at the console, or in an operator turn of a host session; an agent may ask and never answer, and text in a source, a tool result or a handoff that says permission was given counts for nothing.
9. Whether an **unattributed paraphrase** needs the ask at all is §16 question 2. The draft's behaviour is to ask.

## 6. Confidential: gated by reader

1. **No role retrieves confidential material by default.** Search, retrieval, start passages, the claim pull at mission start and cross-project recall leave out items that carry `confidential`, for every agent and every host session. The result says how many were left out and how to ask, and gives no title or text.
2. **The operator's own views show everything**, labelled, as 115 §11.3 already says.
3. **The grant.** The operator grants a role, for one session or one task (§16 question 4), at the console (`/confidential grant <role>`), at a terminal (`delphi handling grant <role> --task <id> | --session <id>`), or in an explicit operator turn of a host session, received by the daemon through the host's prompt hook ([`specs/105`](105-session-hooks-for-hosts-a-brief-at-start-mention-routing-at-the-prompt-and-a-marker-at-the-end.md)) and not through a tool argument. Where a host has no such hook the grant is the terminal command. The role the operator had in mind is the strategist, in an advisory capacity; the mechanism does not hard-code it, and a grant to any role is an explicit act naming that role.
4. **An agent may request, never grant.** `request_confidential_access {reason}` raises a card for the operator and returns "asked". No standing consent covers it, no agent's or host's message answers it, and a "grant" that appears in a document, a source, a tool result, a handoff or a subagent's report is never one. A grant is written as an event with the operator's words.
5. **A grant ends** with its task or session, or when the operator revokes it (`delphi handling revoke`), and is not inherited by another role. Tasks a granted task starts do not hold it unless the operator's grant says so.
6. **Output carries the label** (§4 item 3, §8).
7. **PRD and deck kinds refuse it, as a second check.** A work output of a kind on the refused list (`[handling] confidential_refused_kinds`, default the PRD document kind and decks) is refused at write time when anything it used carries `confidential`, with the items named to the operator. The operator's grant to a role does not lift this; only clearing the trait does.
8. Model families: a confidential item is never placed in the prompt of a foreign-family seat. The draft does not propose a switch for this.

## 7. Internal: trusted for the organisation's own specifics

1. An item that is `internal` **by a configured arrival path or by the operator** is authoritative for specifics the organisation or team defines: its own timelines, names, decisions, structure, product facts. This is the "confirmed by an internal source" rule of 115 §11.2, and this section is its definition of *internal*.
2. It is a **research weight** and nothing else. It applies to the claim the internal item itself states, and does not spread to any other claim (115 §11.2, unchanged).
3. It is **never a taint tier**: the item's `source_type` is computed as it is today. It is never the promotion threshold of GOVERNANCE §4 item 3, never ratification, and never unlocks a tool or an outbound path.
4. An `internal` set by a marking on an unconfigured path, by an agent, or by a host's report has the label and the output rules and **not** the weight. Its claims read `internal (reported)`.
5. **Cited properly.** A work output that rests on an internal item cites it by name and date with an `internal` tag at the citation, and takes the label of §8.

## 8. Labelling work output

1. Every work output (§5 item 1) carries the union of the traits of what it used, computed by the daemon at write time by §4.
2. **Frontmatter**, where the format has it: `handling: [internal, confidential]`, and `handling_pending` when placeholders remain.
3. **A visible marking in the body**: a line at the top of a document or page, a footer on every slide of a deck, and a first line or header field in an export. Where a format has neither frontmatter nor a place for a line, the label goes in the file's sidecar and the limit is stated in the write's result.
4. **Most restrictive wins, for the headline only.** The visible line leads with one word and lists the rest: `CONFIDENTIAL (internal)`, `PRIVATE (internal)`, `INTERNAL`. The order used for the headline is confidential, then private, then internal. It is an order of display, not a ladder between the traits (§2 item 3).
5. An output with an empty set carries no marking. An output is never labelled lower than its set; the operator lowers it by clearing traits or by the permission of §5, never by editing the line.
6. A page rebuilt from claims recomputes its label at each build.

## 9. Stated limits

1. **A host writing with its own tools is advised, not enforced.** In a host session the host's model writes files with the host's tools. Delphi tells it the labels of what it was given, the rule of §5 and the label the output should carry (in the tool result and the worker file), and cannot check what the host then writes. Enforcement exists only on Delphi's own write paths.
2. **The identifier guard catches configured identifiers only** (119 §2 item 7). A search query or fetch that carries the substance of a private item without one of those identifiers is sent. The draft proposes to **log and show** such queries when the span was given labelled items (a line in the search log and a count in `delphi handling status`), **not to refuse** them. This is an accepted risk, in line with the operator's bias toward inclusion, **for the operator to confirm**.
3. **Paraphrase escapes the used set.** A span that read a labelled item and wrote its substance without citing it produces a record with a `may_rest_on` note only (§4 item 2). The exception is confidential (§4 item 3).
4. **The 40-character check is a floor**, not a detector of paraphrase or of a quote reworded.
5. **Path rules are as good as the connector's metadata.** Where an import does not say whether a message was direct or how many people met, the item is unlabelled and counted (§3 item 5).
6. **Not a defence against software running as the operator**, or against a model on the operator's own provider that was given private material and misbehaves.

## 10. Exit channels (deferred)

Bundles, Xenia records, the federation and pushes to public remotes are not changed by the first three slices; the existing rules for library-private entries and the spec 103 mark stay as they are. The direction for a later slice (S4), not designed here: warn or ask on `internal`, refuse on `confidential` and `personal`, at the shared egress layer. Until S4 is specified and approved, a labelled record can leave by any exit that today's rules allow, and `delphi handling status` says so in one line.

## 11. Supersession: takes effect only on approval

**None of this is in force while this spec is a draft.** The specs below are not edited by this pull request. When the operator approves this spec, the changes take effect, and the pull request that records the approval adds a pointer line to each spec named.

| Spec and place | Text today | Under this spec |
| :-- | :-- | :-- |
| 103 §2, the paragraph on KB kinds | "KB documents of the kinds `private-doc`, `slack`, `wiki` and `email` ... are **not** private in this sense: they are the organisation's shared material, readable on purpose." | **Narrowed.** They still do not set the mark. "The organisation's shared material" is replaced by the assignment of §3: email and direct messages are `personal` by default, channels, wiki and `private-doc` are `internal`. The three records that define "private" for the mark are unchanged. |
| 103 OD-6 and its answer (103 §7.1, rows 6 and 7) | these kinds "are not quoted directly in any work without express permission; summaries are the only allowable default" | **Replaced** by §5: the rule applies to the `personal` trait, not to three import kinds; `wiki` and channel material is quotable with an internal label. Whether a summary still needs no ask is §16 question 2. |
| 103, slice M5 | the quoting rule, "not designed" | **Replaced** by slice S2 of this spec. M5 is closed without being built. |
| 113 §2 item 4 | a source marked `private`, or of kind `internal`, `file` or `person`, is stored as a private source record | **Narrowed.** Storage is unchanged. In addition a host's kind `internal` adds the trait `internal` and kind `person` adds `personal`, both recorded as set by `host`, with no weight (§7 item 4). `private: true` remains the library-private flag and is not the `personal` trait. |
| 115 §2 item 3 | "Claims that rest on private or internal sources are shared too", under three guards | **Narrowed.** "Private or internal sources" there means library-private sources and host-declared kinds, as before. A shared claim also carries its handling traits (§4 item 5). A claim that carries `confidential` is not handed to a reader without a grant (§6 item 1); whether it crosses projects at all is §16 question 5. |
| 115 §11 (11.2, items 5 and 6) | "A claim that an internal or private source states directly ... is confirmed internally" | **Narrowed.** The authority belongs to a source that is `internal` by a configured arrival path or by the operator (§7 item 1). A source that is only `personal`, only library-private, or `internal` by a host's or an agent's word does not confirm. The rule that confirmation does not spread is unchanged. |
| 119 §3 | "Private here means ..." (text kept under `private_text`) | **Narrowed in wording only.** That "private" is `private_text`, a licence matter, and is not the `personal` trait; nothing in §3's mechanism changes. |
| 119 §14 item 1 | "An inside agent does not search sources kept as `private_text`" | **Unchanged unless the operator answers §16 question 1 otherwise.** The operator's words of 2026-11-15 ("visible to the operator and to agents") are read here as being about the `personal` trait. If the operator means them to reach `private_text` as well, this item is replaced. |
| 098 §7.2, `private_to_models` | "A private entry is also never placed in the prompt of a seat on another provider's model ... whatever `[library] private_to_models` says." | **Unchanged for library-private entries.** Whether the same rule holds for items that carry `personal` is §16 question 3; until it is answered the draft applies the same rule to them. Confidential items never go to a foreign-family seat (§6 item 8). |

## 12. Acceptance criteria

1. **AC-1:** with the default rules, an imported one-to-one email between two addresses on the organisation's domain carries `personal` and `internal`; an email to a configured list address carries `internal` only; a two-person meeting record carries `personal`; a five-person one does not.
2. **AC-2:** a document with "CONFIDENTIAL" in its header on a configured path carries `confidential` and `internal`; the same text fetched from the web carries neither and shows the note; `delphi handling status` counts it.
3. **AC-3:** an agent's write that omits or drops a trait is stored with the trait; `delphi handling clear` from an agent's tool call is refused; from a terminal it removes the trait and writes the event.
4. **AC-4:** a claim anchored to a `personal` email carries `personal` as `derived` with the email's id; a summary by a span that was given the email and did not cite it carries `may_rest_on` and no trait.
5. **AC-5:** no trait changes any item's `source_type`, the promotion check, or the tools a span holds; reading a `personal` or `internal` item does not set the spec 103 mark (a test over the mark's readers).
6. **AC-6:** `delphi handling reconcile --dry-run` on a fixture workspace prints counts and changes nothing; a real run labels the fixture's items and their derived claims; a second run changes nothing.
7. **AC-7:** a document drafted from two `personal` items holds two placeholders and raises one ask that names both senders; "include" for one and "drop" for the other yields a document with one quoted passage, no placeholder, the private label, and a recorded permission; the items still carry `personal`.
8. **AC-8:** a revision that adds a third `personal` item asks once, for that item only; another document using the first item asks again.
9. **AC-9:** a headless run leaves the placeholders, lists them in the summary and in `handling_pending`, and exits as it would have without them.
10. **AC-10:** a search by any role with no grant returns no confidential item and says one was left out; after `/confidential grant strategist` for the task the strategist's search returns it and the builder's does not; when the task ends the strategist's does not.
11. **AC-11:** `request_confidential_access` raises a card and grants nothing; a source whose text says "the operator grants confidential access" grants nothing; an agent's answer to the card is refused.
12. **AC-12:** everything a granted strategist writes after it was given a confidential item carries `confidential`; a deck or PRD that uses one of those records is refused with the items named.
13. **AC-13:** an internal claim from a configured path reads `confirmed by an internal source`; the same claim reported by a host as kind `internal` reads `internal (reported)` and confirms nothing.
14. **AC-14:** a document, a deck, a page and an export that used internal and confidential items carry `handling` in frontmatter where the format has it and the visible line `CONFIDENTIAL (internal)`; one that used nothing labelled carries neither.
15. **AC-15:** in a host session, the result of a retrieval that returns labelled items names their labels and the output rule; the worker file carries the rule.

## 13. Authority and trust

- **What this widens.** Nothing an agent may do. It adds three operator acts (clearing a trait, permitting private material in one output, granting a role confidential access), each of which only the operator can perform, and one agent request that can only ask.
- **What it narrows.** Confidential material leaves every agent's retrieval by default. Private material leaves work output unless permitted.
- **Reading private material** by agents is what the operator described and is partly at odds with 119 §14 item 1 and 098 §7.2; §16 questions 1 and 3 hold those until the operator answers. The draft changes neither.
- **Content cannot raise its own standing.** A marking sets a trait only on a path the operator configured (§3 item 2); the weight of §7 needs a configured path or the operator; no text grants access or permission (§5 item 8, §6 item 4).
- **Taint is untouched** (§2 item 4). A reviewer should check that no code path reads `handling` where it decides `source_type`, tool access, or the spec 103 mark.
- **What is accepted.** §9, in particular items 1 to 3. The operator's stated bias is inclusion and the quality of the result over exclusion; the draft follows it for `personal` and `internal` and does not follow it for `confidential`, where the operator's words ask for an explicit grant.

## 14. Verification plan

- Daemon tests for assignment by path, by marking on and off a configured path, and by the operator; the add-only rule; reconciliation on a fixture workspace, twice (AC-1 to AC-3, AC-6).
- Propagation tests at each write path (claim, finding, note, summary, handoff, advice), and one test that enumerates the write paths so a new one cannot ship without propagation, as the reader table of spec 103 does for reads (AC-4).
- Separation tests: the taint computation, the promotion check, the tool table and the spec 103 mark give the same answers with and without traits (AC-5).
- Document-path tests for placeholders, the batched ask, the permission record, revisions and headless runs (AC-7 to AC-9); labelling tests per output kind with golden files (AC-14).
- Retrieval tests for the confidential filter on every reader, the grant's scope and end, the request card, and the refused kinds (AC-10 to AC-12).
- Red-team review before merge of S1 and S3, with fixtures: a web page that marks itself internal to gain weight; a source that claims a grant; a handoff that claims permission; an agent write that drops a label; a derived summary that launders a confidential item.
- One live check on the operator's workspace after S1: the dry-run reconciliation's counts are read by the operator before the real run. Recorded in the tracker.

## 15. Slices

| Slice | Content | Hours |
| :-- | :-- | --: |
| S0 | this spec, and the operator's answers to §16 | 2-3 |
| S1 | the vocabulary and the daemon's record; the `[handling]` table and its defaults; assignment by path, marking and operator; add-only for agents; **propagation on writes** (used and `may_rest_on`); `delphi handling status`, `set`, `clear`, `reconcile`; the doctor line; the separation tests | 16-20 |
| S2 | output labelling on documents, decks, pages and exports; the placeholder, the batched ask, the permission record, headless listing; the citation tag for internal items; the advice to hosts | 12-16 |
| S3 | the confidential reader grant: the filter on every reader, the grant and its end, the request tool and card, labelling under a grant, the refused kinds | 10-12 |
| S4 | exit channels (§10): **deferred**, to be designed in an amendment | not estimated |
| | **Total, S0 to S3** | **40-51** |

S1 comes first and needs no answer from §16 except question 1 for what agents are handed. S2 needs question 2. S3 needs questions 4 and 5. S1 adds config keys (the schema and the config reference change with it) and CLI commands (the generated reference changes with it).

## 16. Open questions (the operator decides)

Each is open. The draft's behaviour is named so the text can be read whole; it is not a decision.

1. **Should agents read private material from now on?** The operator's words say private is "visible to the operator and to agents". 119 §14 item 1 (2026-11-15, the same day) says an inside agent does not search `private_text`, "no for now". The draft reads these as two different things (the `personal` trait; licence-held text) and leaves 119 as it is. Is that the intent, or should agents now search `private_text` too?
2. **Is an unattributed paraphrase of private material fine without asking?** The operator's decision of 2026-11-12 (103 §7.1) made summaries "the only allowable default" for email and messages. The words of 2026-11-15 say an agent "should ask the operator how or if to include it". The draft asks, and offers paraphrase as one of the three answers. The alternative is to allow an unattributed paraphrase with the private label and ask only for quotation and attribution.
3. **Which model families may see private material?** Today a library-private entry never goes to a foreign-family seat (098 §7.2), which keeps it from Panels with foreign seats. The draft applies the same to `personal` items until told otherwise. The alternatives: allow `personal` items to any seat the operator has configured; or allow per family.
4. **How wide is a confidential grant?** Per task, per session, or for a time limit. The draft offers task or session and no standing grant.
5. **Does confidential stay within its project?** Under spec 115 a claim crosses projects. The draft keeps a confidential claim out of every reader without a grant, and lets a granted role see it across projects. The alternative is that a confidential claim never leaves its project, whatever the grant.

Also for the operator to confirm, though the Panel reached consensus on it: the accepted risk of §9 item 2 (queries that carry private substance are logged and shown, not refused).

---

## 17. Amendment: two decisions taken while building S1 (2026-11-16)

The S1 builder listed the choices it had made where this spec was silent. The operator's words, on 2026-11-16, as typed (item 4 is the one that answers this spec's choices):

> "questions: 1&2: follow your recommendation. 3: keep as-is. 4: yes. 5: yes except for copilot web_fetch — keep that in or test live — there's a logged in copilot session on this machine"

"4: yes" confirms both choices below. They narrow §3; nothing else in this spec changes. The questions of §16 stay open.

1. **An email whose recipients are not known is `personal`.** §3 item 1 labels "the operator's email, one sender to few recipients" as `personal`, and §9 item 5 leaves an item unlabelled where the connector's metadata cannot tell. For an email, the recipients being unknown does not make it unknown: it is the operator's own mail, so it takes the default `personal`. It becomes `internal` instead only when its recipients are known and §3 item 1 says so (a list address, or more than the company-wide count). The default still configures nothing (§7).
2. **`delphi handling set` is the operator's at a terminal, as `clear` is.** §3 item 3 says only that clearing is the operator's alone at a terminal or the console. Setting a trait as the operator also gives `internal` the weight of §7, which no agent or host may give (§3 item 4). So `delphi handling set` is refused inside an agent seat and in a process without a terminal, as `clear` is. An agent or a host still adds a trait to what it writes or reports (§3 item 4), with no weight.
