# Xenia: the operator's guide

Xenia is your organization's shared, signed memory: records one person publishes, a steward vouches for and everyone else can search. This guide is generated from the same text `delphi xenia help` prints, so the two cannot disagree (a test fails when this file drifts; regenerate it with `DELPHI_REGEN_DOCS=1 cargo test -p delphi-core xenia_help`). The design is [`specs/011`](../../specs/011-xenia-trust-model.md) and [`012`](../../specs/012-xenia-record-schema.md).

Everything here works offline and never calls a model. Ask the same questions of the running assistant with the `xenia_help` tool, or in a terminal with `delphi xenia help <topic>` and `delphi xenia explain <slug>`.

## What Xenia is

Xenia is your organization's shared, signed memory. A member publishes a record (a finding, a decision, a gotcha, a how-to); a steward who covers its subject reads it and endorses it; every install checks the signatures itself, so the store's host never has to be trusted. Records nobody has endorsed are still searchable, but at a lower trust level (L1); endorsed ones are L2.

Three roles, all of them people who hold a key:
  member    publishes records, disputes or withdraws their own.
  steward   a member who also endorses records in the areas they cover.
  admin     changes the roster (who is in, who is out) and keeps the store's freshness statement current. A store should have three, so that any two can act.

The commands most people need:
  delphi xenia whoami             your identity: send the DID to an admin to be added.
  delphi xenia join <url> ...     start using a store (an admin gives you the exact line).
  delphi xenia sync               fetch and verify; it never changes a file it cannot prove.
  delphi xenia status             read this first when anything looks wrong.
  delphi xenia publish <file> ... share a record (you approve the exact bytes at a terminal).
  delphi xenia endorse <id>       stewards vouch for a record.
  delphi xenia explain <slug>     what any word in `status` means and what to do about it.
  delphi xenia help <topic>       the pages below.

Reading `status`: the accepted head is the version of the store you trust. Roster shows epoch, admins, stewards and members. Freshness says whether an admin has recently vouched that the store is complete: when it is stale no endorsement counts and every record reads L1. Records shows how many are L2, L1, hidden, contested or quarantined (held back, never loaded). Anything under Warnings has a line starting `what to do`.

## Getting started: joining a store

You need three things from an admin, given to you over a channel that is not the store itself (a call, a chat with someone you know): the store's address, the admin's fingerprint, and the roster head `<epoch>:<hash>`. The admin's genesis step prints the exact line.


```text
  1. Run `delphi xenia whoami`. It creates your key on first use and prints your DID and fingerprint. Send both to an admin; they add you to the roster. Keep the key file private (it is mode 0600 under your Delphi home; never share it).
  2. Run the join line the admin gave you:
       delphi xenia join <url> --org <org> --store <store> --admin <fingerprint> --head <epoch>:<hash>
     Join fetches the store and checks that the roster verifies to that head and that the admin you named is active in it. If anything differs it writes nothing. An address that is a local path needs `--allow-local`.
  3. Run `delphi xenia sync`, then `delphi xenia status`. A fresh store with no valid statement accepts nothing (`no-trust-base`): ask an admin to refresh.
  4. Search endorsed records from a workspace with a knowledge base; they show up in `delphi kb search` and to agents through the `xenia_search` tool.

```

Your clock must be within a few minutes of real time: freshness is judged by it (`issued-in-future`).

If join is refused, `delphi xenia explain <word in the message>`.

## Publishing a record

Only content you have already promoted from team or workspace scope can be published; anything from your operator scope never leaves the machine.


```text
  delphi xenia publish <file> --title "..." --summary "..." --promoted [--tags a,b] [--type finding|decision|gotcha|how-to] [--review-by YYYY-MM-DD]

```

What happens: Delphi builds the record, scans it for secrets, private paths and personal identifiers, then shows you a card with the exact bytes it will sign, any flags and similar existing records. You type the card's short code at a terminal. No model runs between the card and the signature. A published secret cannot be un-leaked: rotate it.


A record you publish reads L1 until a steward who covers its tags endorses it. You cannot endorse your own. To change it, publish again (a new revision) with the same file; to take it down, `delphi xenia withdraw <id> --reason mistake`; to bring it back, `delphi xenia restore <id>`.


If it is refused, the message names a slug: `delphi xenia explain <slug>`.

## Endorsing, disputing and withdrawing (stewards and members)

A steward reads the exact revision (and opens its pointers) and signs an endorsement with `delphi xenia endorse <id> [--scope a,b] [--note "..."]`. It counts only when: you are not the author (or the author's predecessor key), your areas cover the record, you are an active member, and the freshness statement is current. A new revision needs a new endorsement.


```text
  delphi xenia dispute <id> --note "why"      any member: flags a record to stewards; it does not change the tier by itself.
  delphi xenia retract <id>                    withdraw your own endorsement.
  delphi xenia withdraw <id> --reason <r>      hide a record: yours, or (for a covering steward) anyone's. If stewards disagree the record is `contested`: hidden until they agree.
  delphi xenia restore <id>                    bring a hidden record back.

```

You can do these at a terminal, or in the console with `/xenia endorse <id>` followed by `/xenia sign <code>`: the console shows the same card and only your own typed code signs. An agent cannot answer for you.

## Being an admin

An admin holds a key that can change the roster and sign the freshness statement. Admins are counted per person, and a roster change needs a majority of the current admins, so keep at least three people as admins: with two, losing one key freezes the store (`few-admins`).


Your duties:

```text
  - Refresh the freshness statement before it expires. The default validity is 7 days and no install accepts more than 14. `status` warns at half of its life (`statement-aging`). Refreshing first checks the whole history for tampering and refuses to sign if it finds any: `delphi xenia admin refresh`.
  - Handle roster changes as proposals: one admin proposes (`delphi xenia admin propose ...`, an add, a removal or a rotation), each other admin reviews the preview and signs, and once enough have signed one admin publishes it. Read every preview: it lists the exact change and flags anything that adds an admin, widens a steward's areas, lets a key speak twice for one person, or voids someone's tombstones. You may refuse.
  - Never share your admin key, and run admin commands only at a terminal you control.

```

Admin subcommands: `add`, `remove`, `rotate` (with `--routine` or `--compromised`), `show <n>`, `sign <n>`, `publish <n>`, `refresh` and `genesis draft|sign|publish`; `delphi xenia admin` lists them. They need your key and a terminal; a relay or an agent cannot run them.

## The genesis ceremony: creating a store with three admins

Genesis writes the very first roster. Every founding admin must agree to it, so every founder signs the same draft. For a 2-of-3 store:


Prepare first:

```text
  - A private, EMPTY git repository, reachable over https or ssh, where all three admins can push. (The first push must create the branch; a store that already has content is refused: `store-not-empty`.)
  - Three people, each with a Delphi install. Each runs `delphi xenia whoami` and sends the DID and fingerprint to the person drafting the roster.
  - Clocks within a few minutes of real time on all three machines.
  - A short call or chat for the fingerprints: members will check them over a channel that is not the store.

```

The ceremony:

```text
  1. One founder drafts the roster naming the three people and their DIDs as admins (plus any first members and stewards) and sends the draft to the other two.
  2. Each founder reads the preview of every key and role, and signs the draft they parsed. Anyone who is not named, or who sees a key they do not recognize, refuses.
  3. One founder collects the three signatures and pushes. The first commit holds the roster, the second the first freshness statement. The command prints a join line with the store address, the admin's fingerprint and `1:<hash>`.
  4. Each founder (and later every member) reads the fingerprint to the other over a call, runs the join line, then `delphi xenia sync` and `delphi xenia status`. Status should show epoch 1, 3 admins and a fresh statement.

```

The commands, in order: `delphi xenia admin genesis draft --org <o> --store <s> --founder <did>,<person>,<handle>,admin ... --out draft.json` (one `--founder` per person); each founder `delphi xenia admin genesis sign draft.json --org <o> --store <s> --out <name>.sig.json`; then `delphi xenia admin genesis publish draft.json --url <url> --org <o> --store <s> --sig a.sig.json --sig b.sig.json --sig c.sig.json`.


Then each admin puts a reminder in their calendar to refresh every week (`delphi xenia admin refresh`).

## Rotating a key: routine or compromised

You must say which. There is no default.


Routine (a new laptop, a planned change): an admin proposes `rotate <member> --routine`. The old key is marked retired with the new key as successor, same person. The member then runs `delphi xenia republish --resign` with the new key, which re-publishes their records, re-issues their endorsements and re-signs their tombstones and disputes as new revisions, so they keep their tier. Records with an effective withdraw are skipped, so rotation cannot resurface hidden content.


Compromised (the key may be stolen):

```text
  1. Take the old key out of use. Rotate any secret it could have exposed.
  2. An admin proposes `rotate <member> --compromised`; the other admins sign and one publishes. This voids everything the old key signed from that epoch on: records, endorsements and tombstones, including withdrawals.
  3. Nothing is re-signed automatically, and `republish --resign` refuses a compromised rotation.
  4. The new key's owner re-publishes only what they reviewed, as new revisions.
  5. Stewards re-endorse what is worth keeping.
  6. A record a thief hid can be restored afresh by its author or a covering steward.
  7. Watch `delphi xenia status` for contested or quarantined records for a few days.

```

With 2-of-3 admins, a compromised admin key plus one colluding admin is a quorum: if two admins may be compromised, rebuild the store from a new genesis (everyone re-joins).

## Freshness: why endorsements stop counting, and how to refresh

A store can hide a removal or a withdrawal by serving old data. To prevent it, admins sign a freshness statement that says "as of this head the store is complete" and expires (default 7 days, cap 14). While it is current, endorsements count. When it is stale, every record reads at most L1 (unendorsed); withdrawals and hides still apply.


```text
  delphi xenia status                       shows the age and share of life used. Past 50%, a warning.
  delphi xenia admin refresh                (an admin) checks the history, then signs a new statement.

```

Reasons a statement is not current (see `status`, then `delphi xenia explain <slug>`): expired, missing, replayed-statement (the store served an older one than you saw), issued-in-future (a clock problem), validity-over-cap, no-trust-base (you have never seen a valid one).


If you are not an admin: tell one. If you are, refresh at least twice per statement life, and put it in your calendar.

## Recovery when the store's history was rewritten

If the host's history is force-pushed or rewritten, your sync refuses it (`non-fast-forward`, `diverged`, or `tampering`): nothing is applied, you keep what you had, and nothing is lost. Do not keep retrying.


Drill:

```text
  1. Run `delphi xenia status` to see which signed files were deleted or reverted.
  2. Tell the admins. They must find out who rewrote the history and why (a force-push by someone with write access? a restored backup? an attack?).
  3. If the rewrite is legitimate, a MAJORITY of the epoch's admins (counted per person) sign a freshness statement for the new head that names the rewritten head it replaces. One admin cannot do this alone.
  4. Each install's operator is shown every signed file that was deleted or reverted and confirms with `delphi xenia sync --accept-rewritten-head`. Only the files you approve are replaced; everything else you have stays. It is a terminal-only command: it first runs an ordinary sync, does nothing unless the admins' statement is authentic, and then needs a person at a terminal to type the commit and the number of files listed. A host that went back in time (an older view than you hold) cannot be recovered this way.
  5. If a majority of admins is compromised, or the roster is deadlocked, the org rebuilds from a new genesis and everyone re-joins with the new fingerprint, over a channel that is not the store.

```

Do not accept a rewrite just to make a warning go away.

## Working offline

Everything but `sync`, `join` and the commands that push works without a network: `status`, `explain`, `help`, search over what you already have. A failed sync (`unavailable`) changes nothing.


The clock keeps running: the freshness statement has a fixed expiry, so after a long time offline the endorsed records you hold read L1 (stale) until you sync and an admin has refreshed. That is deliberate and safe; it does not delete anything.


Signing commands (`publish`, `endorse`, admin commands) need a network to push; the card and your typed code come first, and nothing is sent when the push fails.

## Approving: console versus terminal, and why a human must

Signing is always a person's decision. The card shows the exact bytes; you type the card's short code; nothing else signs. No model runs between the card and the signature. An agent, the relay (email or chat) and the Narrator cannot approve or sign anything in Xenia.


At a terminal: every command that signs works, including all admin commands. It refuses when stdin or stdout is not a terminal.


In the console: `/xenia endorse|dispute|withdraw|restore|publish|republish ...` shows the card, `/xenia sign <code>` signs, `/xenia cancel <code>` refuses. The console may approve routine member actions. It never approves genesis, rotation, rewrite recovery, anything that touches an admin, or accepting a rewritten head. Console approval of routine admin proposals that touch no admin is controlled by `[xenia] console_admin` in the operator config and is OFF by default (it arrives with the admin commands).


The relay only delivers messages. A relay message that says what to do tells you what to run yourself.

## The security model in plain words

What a hostile or careless store host can do: refuse to answer, serve old data, delete or rewrite history, or fill the store with junk. What it cannot do: make a record appear that no active member signed, make an endorsement count that no active steward signed, change who the admins are without a quorum of them, or make you accept a new head that deletes something you already saw. Your install checks every signature and every rule itself, and refuses the whole head when a check fails, keeping what you had.


What it can still do: withhold new updates (you will see `unavailable`, `behind` or a stale statement, never silent acceptance) and learn what you read from it if the host is not private. Records are text written by colleagues: they reach a model only as short, labelled summaries, and reading one lowers that task's trust ceiling.


Why approval needs a human: a signature is the only thing that makes a record count, so the person whose name goes on it must see the exact bytes and type the card's code at their own terminal or console. Agents, the relay and the Narrator are refused by name, and Xenia help text is the same on every surface: it is fixed text, never built from store content.


Console and terminal: the console may approve routine member actions (publish, endorse, dispute, withdraw, restore, republish). Everything touching admins, genesis, rotation and accepting a rewritten head is terminal-only. Console approval of routine admin proposals that touch no admin is off by default and controlled by `[xenia] console_admin` in the operator config.


The honest limits: a compromised admin key plus one colluding admin is a quorum in a 2-of-3 store; a published secret cannot be recalled, only rotated; the store's own clock does not exist, so your machine's clock matters.

## Troubleshooting

Start with `delphi xenia status`, then `delphi xenia explain <slug>` for any word it shows. `delphi doctor` check 13 repeats the warnings with the same advice.


```text
  Symptom                                   Slug                  Do this
  ----------------------------------------  --------------------  --------------------------------------------
  Everything reads L1, nothing endorsed     stale / expired       ask an admin: delphi xenia admin refresh
  Sync says it cannot reach the store       unavailable           check network and access; retry
  Sync refused, 'tampering'                 tampering             do not retry; tell an admin
  Sync refused, 'non-fast-forward'          non-fast-forward      do not retry; delphi xenia help recovery
  Sync refused, 'rejected'                  rejected              delphi xenia status; explain the reason slug
  Nothing accepted on a new join            no-trust-base         ask an admin to refresh
  Statement 'issued in the future'          issued-in-future      fix this machine's clock
  My record is quarantined                  see the slug          delphi xenia explain <slug>
  Publish refused                           the slug it prints    delphi xenia explain <slug>
  I cannot publish at all                   signer-not-active     send your DID to an admin to be added
  A record is hidden and I do not know why  contested / *-tombstone   delphi xenia explain <slug>
  Fewer than three admins                   few-admins            add an admin: help being-an-admin

```

If a message is not in this list, `delphi xenia explain "<the text>"` matches it; unknown text is not echoed back.

## Every condition, by slug

The table is the catalog `delphi xenia explain <slug>` reads. "Who" is who can fix it.

| Slug | What it means | What to do | Who |
| :-- | :-- | :-- | :-- |
| `accepted` | The sync brought in new, verified records. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `up-to-date` | The store has nothing new since your last sync. | Nothing to do. `delphi xenia status` shows how long the freshness statement has left. | nobody: it clears by itself, or nothing is wrong |
| `unavailable` | The store could not be reached (offline, a network or login problem, or the host is down). | Check the network and your access to the store, then run `delphi xenia sync` again. `delphi xenia help offline` explains what still works. | you: it is on this machine |
| `tampering` | The store's new history deleted, reverted or took over signed files, so the whole head was refused. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `rejected` | The new head failed a trust check (the roster, the freshness statement or the chain from the last verified head), so it was refused. | Run `delphi xenia status` for the reason slug, then `delphi xenia explain <slug>`. If it persists, ask an admin. | an admin of your org's store |
| `non-fast-forward` | The store's history was rewritten or force-pushed (or an older view is being served). | Do not retry. Ask an admin to investigate; recovery needs an admin quorum. Run `delphi xenia help recovery`. | an admin of your org's store |
| `refused` | The fetched store broke a safety rule (unsafe file types, too large, a bad settings file), so it was refused before any record was read. | Run `delphi xenia status`, read the reason slug, then `delphi xenia explain <slug>`. Ask an admin. | an admin of your org's store |
| `diverged` | Your accepted head and the store's head each have commits the other lacks. | Do not retry. Ask an admin; see `delphi xenia help recovery`. | an admin of your org's store |
| `behind` | The store is serving an older view than you already accepted. | Wait a few minutes and sync again. If it stays behind, ask an admin to look at the store host. | an admin of your org's store |
| `fresh` | An admin vouched recently that the store is complete, so endorsements count. | Nothing to do. An admin should refresh before it runs out. | nobody: it clears by itself, or nothing is wrong |
| `statement-aging` | The admins' freshness statement is more than half used up. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `missing` | The store has no freshness statement. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `expired` | The freshness statement ran out. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `nothing-accepted` | No head has been accepted yet, so there is nothing to judge. | Run `delphi xenia sync`. | you: it is on this machine |
| `not-synced` | Nothing has been accepted from this store yet. | Run `delphi xenia sync`. | you: it is on this machine |
| `roster-invalid` | The roster the statement depends on does not verify. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `replayed-statement` | The store served an older freshness statement than one you already saw. | Ask an admin to publish a new statement with `delphi xenia admin refresh`; do not accept the old one. | an admin of your org's store |
| `issued-in-future` | The statement is dated later than this machine's clock. | Check this machine's date and time (they must be within a few minutes of real time), then sync again. If the clock is right, tell an admin. | you: it is on this machine |
| `validity-over-cap` | The statement claims to be valid for longer than the 14 days every install allows. | Ask an admin to issue a statement with the default 7-day validity. | an admin of your org's store |
| `expires-too-far` | The statement's expiry is too far in the future for this install's clock. | Check this machine's clock, then ask an admin to refresh with a shorter validity. | an admin of your org's store |
| `expires-before-issued` | The statement expires before it was issued. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `no-trust-base` | This install has never seen a valid freshness statement, so it accepts nothing. | Ask an admin to run `delphi xenia admin refresh`, then sync. | an admin of your org's store |
| `no-valid-statement` | The store has no freshness statement that checks out. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `no-admin-signature` | The statement is not signed by an admin who is active in the roster. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `rewrite-quorum-missing` | A statement says history was rewritten but a majority of admins did not sign it. | Ask the admins to agree, then have a majority sign. See `delphi xenia help recovery`. | an admin of your org's store |
| `older-epoch` | The statement was written for an older roster than the current one. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `newer-than-chain` | The statement is for a roster newer than the one that verified. | Sync again in a few minutes; if it persists, ask an admin. | an admin of your org's store |
| `epoch-hash-mismatch` | The statement names a different roster than the verified one. | Ask an admin to check the store and refresh. | an admin of your org's store |
| `store-head-not-ancestor` | The store's new head does not build on the head the admins last vouched for. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `sigs-oversize` | The statement's signature file is too large. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `sigs-malformed` | The statement's signature file is not in the expected shape. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `no-roster` | The store has no roster, so there is nobody to trust. | Check you joined the right store with `delphi xenia status`; ask an admin. | an admin of your org's store |
| `few-admins` | Fewer than three people hold an admin key. | Add admins: `delphi xenia help being-an-admin`. | an admin of your org's store |
| `stale` | The freshness statement is not current, so no endorsement counts. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `sync-refused` | The last sync was refused; the store's head was not accepted. | Run `delphi xenia status` to see which files, then `delphi xenia help troubleshooting`. | an admin of your org's store |
| `state-permissions` | One of this install's private Xenia state files (pins or journal) is not a private regular file. | Run `chmod 600` on the file named in the warning, or join the store again. | you: it is on this machine |
| `store-unreadable` | This install cannot read its own copy of the store. | Run `delphi xenia sync`; if it fails the same way, move the folder under `$DELPHI_HOME/xenia/` away and join again. | you: it is on this machine |
| `contested` | Stewards who cover the record disagree about hiding it, so it is hidden from everyone. | Ask the covering stewards to agree (one can restore or withdraw). | a steward of your org's store |
| `author-superseded` | The author replaced this record with a newer one. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `author-withdrawn` | The author withdrew this record. | If it was a mistake, the author can restore it with `delphi xenia restore`. | the member who wrote the record |
| `author-tombstone` | The author hid this record. | The author can restore it with `delphi xenia restore <id>`. | the member who wrote the record |
| `steward-tombstone` | A steward who covers this record hid it and none disagrees. | To bring it back, ask the steward or a covering steward to restore it. | a steward of your org's store |
| `sensitive-hold` | A steward hid this record as sensitive; only that steward can reverse it. | Ask that steward. If a secret was published, rotate the secret: hiding does not un-leak it. | a steward of your org's store |
| `bad-frontmatter` | The record's header is not in the strict format. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `unsupported-schema` | The record uses a newer format than this install understands. | Update Delphi (`delphi update`), then sync. | you: it is on this machine |
| `forbidden-key` | The record tries to state trust (a tier, an endorsement, an approval) in its own text. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-schema` | The record is missing a required field or has a wrong value. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `mismatch` | Fields that must agree (path, id, signer, org, store) do not. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-text` | The record's text has hidden or control characters, or text that breaks the format. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `over-limit` | The record has more tags, pointers or entries than allowed. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-pointer` | A pointer in the record is not an allowed kind of link. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-date` | A date in the record is not in the one allowed format. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `not-active` | The signer's key is not active in the current roster (removed, retired or not yet added). | If the person should be a member, an admin adds or rotates their key (`delphi xenia help being-an-admin`). | an admin of your org's store |
| `signer-not-active` | This key is not an active member of the roster, so nothing it signs counts. | Run `delphi xenia whoami`, send the DID to an admin, and ask them to add you. | an admin of your org's store |
| `duplicate-path` | The same file path was seen twice. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `duplicate` | A repeat of something already counted (the same endorsement or tombstone twice). | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `endorsement-not-steward` | The endorsement was signed by someone who is not a steward. | A steward must endorse the record: `delphi xenia endorse <id>`. | a steward of your org's store |
| `id-collision` | Two different records claim the same id. | The later author publishes under a new id. | the member who wrote the record |
| `outside-areas` | The steward endorsed or hid a record outside the areas they cover. | A steward who covers the record must act, or an admin widens this steward's areas. | a steward of your org's store |
| `scope-outside-areas` | The endorsement names scope tags the steward does not cover. | Endorse with a scope the steward covers, or ask an admin to widen their areas. | a steward of your org's store |
| `tombstone-not-author-or-covering-steward` | A tombstone was signed by someone who may not hide that record. | Nothing to do. | a steward of your org's store |
| `cancelled-by-same-person` | The signer cancelled their own earlier endorsement. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `dispute-of-another-revision` | A dispute is about an earlier revision of the record. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `freshness-stale` | An endorsement is valid but cannot count while the freshness statement is stale. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `hash-mismatch` | The bytes do not match the hash that was signed (or the endorsement is for other bytes). | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `replaced-by-successor` | A key was rotated and its successor re-issued this. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `self-endorsement` | A steward tried to endorse their own record (or their predecessor key's). | Another steward must endorse it. | a steward of your org's store |
| `target-hidden` | The endorsement is for a record that is now hidden. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `target-rev-mismatch` | The endorsement was for an older revision than the current one. | A steward reviews the new revision and endorses it: `delphi xenia endorse <id>`. | a steward of your org's store |
| `unknown-path` | A file in the store is not one of the file kinds Xenia knows. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `missing-sidecar` | A record has no signature file next to it. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `not-tracked` | A record did not make it through the last sync's checks. | Run `delphi xenia sync`; if it stays, the reason shows in `delphi xenia status`. | you: it is on this machine |
| `bad-sidecar` | The signature file next to a record is malformed. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-path` | The file's path or name is not allowed. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-field` | A field has a value that is not allowed. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-did` | A signer identity is not a valid key identity. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `bad-signature` | The signature does not verify. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `key` | This install's Xenia key file is missing, damaged or readable by others. | Run `delphi xenia whoami` (it creates a key if none exists). If the file is readable by others, run `chmod 600` on it. | you: it is on this machine |
| `oversize` | The file is larger than allowed. | The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision. | the member who wrote the record |
| `deleted` | A file that was in the store is gone. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `no-longer-verifies` | A file you accepted earlier no longer verifies. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `rev-rollback` | A file went back to an older revision. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `same-rev-different-hash` | A file was replaced by different content with the same revision number. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `tracking-changed` | A path changed what kind of file it is. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `takeover` | Someone other than the owner changed a record they did not write. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `author-state-regression` | A withdrawn or superseded record was brought back to active. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `epoch-file-changed` | A roster file that should never change was edited. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `epoch-differs-from-chain` | A roster file differs from the verified chain. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `non-regular-file` | A path became a link or other unusual file type. | Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history. | an admin of your org's store |
| `hostile-tree` | The store contains kinds of file a store never legitimately has (links, submodules, hidden git files). | Do not retry. Ask an admin to clean the store; nothing was applied. | an admin of your org's store |
| `tree-too-large` | The store has more files than the safety limit. | Ask an admin to trim the store. | an admin of your org's store |
| `object-too-large` | A single stored object is over the size limit. | Ask an admin to remove the large file from history. | an admin of your org's store |
| `too-many-objects` | One fetch brought in more objects than the limit. | Ask an admin; a store that grew this much should be rebuilt. | an admin of your org's store |
| `too-many-commits` | One fetch brought in more commits than the limit. | Sync again later; if it persists ask an admin. | an admin of your org's store |
| `fetch-too-large` | The fetch would make the local copy larger than the limit. | Ask an admin to trim the store. | an admin of your org's store |
| `gitattributes-missing` | The store lacks its settings file (`.gitattributes`). | Ask an admin to restore the store's `.gitattributes`. | an admin of your org's store |
| `gitattributes-unreadable` | The store's settings file is not readable text or is too large. | Ask an admin to restore it. | an admin of your org's store |
| `gitattributes-no-default-no-text` | The store's settings file does not keep files byte-exact. | Ask an admin to restore it. | an admin of your org's store |
| `gitattributes-forbidden-attribute` | The store's settings file asks git to run a program or convert files. | Do not retry. Ask an admin to inspect the store. | an admin of your org's store |
| `gitattributes-malformed` | The store's settings file has a line that is not allowed. | Ask an admin to restore it. | an admin of your org's store |
| `malformed` | A file is not in the expected format. | Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`. | an admin of your org's store |
| `bad-scope` | The org or store name is not a valid name. | Correct the input named in the message and run the command again; nothing was signed or sent. | you: it is on this machine |
| `no-anchor` | This install has nothing pinned to check the roster against. | Join the store with `delphi xenia join` (see `delphi xenia help getting-started`). | you: it is on this machine |
| `empty-chain` | The store serves no roster. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `too-many-epochs` | The roster chain is longer than the limit. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `bad-epoch` | A roster file is not in the strict format. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `bad-signatures-file` | A roster signatures file is not in the strict format. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `bad-pins` | Your pinned roster state is damaged. | Move `$DELPHI_HOME/xenia/<org>/<store>/state` aside and join again. | you: it is on this machine |
| `bad-request` | The join details you typed are not valid. | Check the epoch:hash and fingerprint you were given and run `delphi xenia join` again. | you: it is on this machine |
| `wrong-scope` | The file is for another org or store. | Correct the input named in the message and run the command again; nothing was signed or sent. | an admin of your org's store |
| `epoch-gap` | The roster skips an epoch number. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `epoch-fork` | Two different roster files exist for one epoch number. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `epoch-replay` | An old roster file was served again under its own number. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `prev-hash-mismatch` | A roster file does not follow from the one before it. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `genesis-prev-hash` | The first roster file claims a predecessor. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `sigs-epoch-mismatch` | A signatures file names another epoch. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `genesis-not-fully-signed` | Not every founding admin signed the first roster. | Collect every founder's signature; `delphi xenia help genesis`. | an admin of your org's store |
| `quorum-not-met` | Not enough admins (counted per person) have signed. | Ask the other admins to sign: `delphi xenia help being-an-admin`. | an admin of your org's store |
| `entry-dropped` | A roster entry was removed from the file. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `person-changed` | A key's owner changed between epochs. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `status-regression` | A retired or removed key came back. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `immutable-field-changed` | A field that must not change between epochs did. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `bad-new-entry` | A new roster entry is not a plain new active key. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `bad-successor` | A rotation's successor key is not valid. | This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`. | an admin of your org's store |
| `no-active-admin` | The roster would have no active admin. | Add or keep at least one more admin first. | an admin of your org's store |
| `pin-mismatch` | A roster file differs from the one pinned when you joined. | Do not accept it. Tell an admin and read `delphi xenia help recovery`. | an admin of your org's store |
| `pinned-epoch-missing` | The store's roster ends before the epoch you pinned. | Sync again later; if it persists, tell an admin. | an admin of your org's store |
| `admin-not-active` | The admin you named at join is not an active admin of that roster. | Ask for the admin's fingerprint again, over a channel that is not the store, and check the epoch. | you: it is on this machine |
| `already-pinned` | This store is already joined. | Use `delphi xenia status`, or `delphi xenia sync`. | you: it is on this machine |
| `io` | A file or git operation failed on this machine. | Check disk space and permissions on `$DELPHI_HOME/xenia`, then retry. | you: it is on this machine |
| `not-next-epoch` | The roster file is not the next epoch number. | Run `delphi xenia sync`, then `delphi xenia status`, and try again. | an admin of your org's store |
| `stale-proposal` | The roster moved on since this proposal was made. | Make a new proposal on the current roster: `delphi xenia admin propose ...`. | an admin of your org's store |
| `last-admin` | This would remove the last admin. | Add another admin first. | an admin of your org's store |
| `below-minimum-admins` | This would leave fewer admins than the local minimum (2). | Add an admin before removing one. | an admin of your org's store |
| `unknown-member` | The member named is not in the roster. | Correct the input named in the message and run the command again; nothing was signed or sent. | an admin of your org's store |
| `already-listed` | That key is already in the roster. | Correct the input named in the message and run the command again; nothing was signed or sent. | an admin of your org's store |
| `person-exists` | That person already has a key in the roster. | Use `delphi xenia admin rotate <member> --routine` (or `--compromised`). | an admin of your org's store |
| `same-key` | The new key is the same as the old one. | Correct the input named in the message and run the command again; nothing was signed or sent. | an admin of your org's store |
| `bad-input` | An input to the command is not valid. | Correct the input named in the message and run the command again; nothing was signed or sent. | an admin of your org's store |
| `bad-validity` | The freshness validity is not allowed (zero, or over the 14-day cap). | Use 7 days (the default) or any value up to 14. | an admin of your org's store |
| `store-not-usable` | The store cannot be trusted right now (tampering, a refused fetch, not joined, or no trust base). | Run `delphi xenia status`, fix what it shows, then try again. | you: it is on this machine |
| `not-an-approver` | This install's key is not an active admin, so it cannot sign this. | Ask an admin to do it, or have an admin add your key. | an admin of your org's store |
| `proposal-not-found` | No roster proposal with that number exists. | Run `delphi xenia admin propose ...` first, or check the number. | an admin of your org's store |
| `proposal-exists` | A proposal with that number is already in the store. | Sign or publish the existing one, or ask an admin to clear the branch. | an admin of your org's store |
| `proposal-changed` | The proposal changed after you reviewed it. | Review it again and sign. | an admin of your org's store |
| `flags-not-acknowledged` | The preview raised flags and the approval did not say you read them. | Read the flags on the card and answer again acknowledging them. | you: it is on this machine |
| `denied` | You said no. | Nothing to do. | you: it is on this machine |
| `terminal-only` | The console will not sign this: it touches an admin, rotates a key, starts a store or accepts a rewritten history. | Run the terminal command the message names. | you: it is on this machine |
| `switch-off` | Console admin signing is switched off (the default). | Set `[xenia] console_admin = true` in operator/xenia.toml, or run the command in a terminal. | you: it is on this machine |
| `pending-card` | An admin card is already waiting for your answer. | Answer or deny the waiting card, or wait for it to expire, then ask again. | you: it is on this machine |
| `rate-limit` | Five admin signatures were made from the console in the last hour. | Wait an hour, or sign in a terminal. | you: it is on this machine |
| `not-interactive` | Nothing at a terminal answered: signing needs a person to type the code. | Run the command in a terminal and type the code when asked. | you: it is on this machine |
| `challenge-failed` | What was typed did not match the code for these exact bytes. | Run the command again and type the code shown. | you: it is on this machine |
| `flag-not-acknowledged` | A flag on the card was not acknowledged by typing its name. | Run the command again and type each flag's name. | you: it is on this machine |
| `wrong-confirm` | The proposal number (or `refresh`) was not retyped correctly. | Ask for the card again and retype the number shown. | you: it is on this machine |
| `approval-mismatch` | The approval was for other bytes than the ones held. | Start the action again and read the new card. | you: it is on this machine |
| `wrong-key` | The key given is not the one the action was prepared for. | Start the action again. | you: it is on this machine |
| `history-failed` | The history check found a problem, so no freshness statement was signed. | Run `delphi xenia status` and `delphi xenia help recovery`. Do not refresh until it is explained. | an admin of your org's store |
| `verify-failed` | The result failed verification before it was sent. | Run `delphi xenia sync`, then try again; if it repeats, tell an admin. | you: it is on this machine |
| `clock-behind` | This machine's clock is not ahead of the last statement it accepted. | Fix this machine's date and time, then retry. | an admin of your org's store |
| `store-not-empty` | The store already has content, so a first roster cannot be written. | Use an empty repository, or join the existing store. | an admin of your org's store |
| `missing-founder-signatures` | Not every founding admin has signed the first roster. | Collect the missing signatures; `delphi xenia help genesis`. | an admin of your org's store |
| `push-rejected` | The store host refused the push (permissions, a hook, a size limit). | Check you have push rights to the store and read the host's message. | you: it is on this machine |
| `too-many-races` | The store kept changing while this was being sent. | Try again in a moment. | you: it is on this machine |
| `not-promoted` | The content did not come from team or workspace scope. | Move the file under `team/` (or the workspace) first, then publish with `--promoted`. | you: it is on this machine |
| `wrong-origin` | The record has the wrong kind of origin for this command. | Correct the input named in the message and run the command again; nothing was signed or sent. | you: it is on this machine |
| `scan-blocked` | The secret and path scan found something in the content. | Remove the secret or private path (and rotate the secret if it was real), then publish again. | you: it is on this machine |
| `source-changed` | The file changed after you previewed it. | Run the command again to preview the new bytes. | you: it is on this machine |
| `rev-regression` | The new revision number is not higher than the stored one. | Run `delphi xenia sync` and try again. | the member who wrote the record |
| `id-taken` | That id already exists in the store as something else. | Use a new id. | the member who wrote the record |
| `not-owner` | The record belongs to another member. | Publish your own record, or ask the author. | the member who wrote the record |
| `state-regression` | A withdrawn or superseded record cannot come back. | Publish a new record instead. | the member who wrote the record |
| `raced` | Someone else published the same id while this was being sent. | Run `delphi xenia sync`, look at the store, and decide. | you: it is on this machine |
| `remote-rewrote-history` | The store's history is not a fast-forward of what you accepted. | Do not retry. See `delphi xenia help recovery`. | an admin of your org's store |
| `rule` | A rule the command checks before signing was broken (for example, endorsing your own record). | Correct the input named in the message and run the command again; nothing was signed or sent. | you: it is on this machine |
| `unrepresentable` | A one-line field contains a line break or control character. | Edit the field to plain text on one line. | you: it is on this machine |
| `invalid` | The record would be refused by every install. | Fix the record; the message names the first broken rule. | you: it is on this machine |
| `not-canonical` | The record did not write out the same way it was read. | Report this to the Delphi maintainers with the command you ran. | you: it is on this machine |
| `too-large` | The record is larger than allowed. | Shorten the body or split it. | you: it is on this machine |
| `nothing-to-recover` | The store's history was not rewritten. | Nothing to do. | nobody: it clears by itself, or nothing is wrong |
| `host-unavailable` | The store host cannot be reached. | Check the network and retry. | you: it is on this machine |
| `fetch-refused` | The fetched store failed a safety rule. | Ask an admin; `delphi xenia status`. | an admin of your org's store |
| `tip-refused` | The rewritten head failed verification. | Ask an admin; `delphi xenia status`. | an admin of your org's store |
| `statement-names-no-rewrite` | The latest statement does not say which head it replaces. | Ask a majority of admins to sign a statement naming the replaced head. | an admin of your org's store |
| `statement-not-current` | The statement approving the rewrite is not current. | Ask admins for a fresh statement. | an admin of your org's store |
| `rewrite-of-is-not-our-head` | The statement replaces a different head than the one you hold. | Ask admins which head they replaced. | an admin of your org's store |
| `walk-violations` | Even after the rewrite, the history check found tampering. | Do not accept. Tell the admins. | an admin of your org's store |
| `host-head-moved-since-approval` | The store changed after you approved the list of files. | Start recovery again and read the new list. | you: it is on this machine |
| `approval-names-unlisted-path` | The approval names a file that was not on the list. | Start recovery again. | you: it is on this machine |
| `unapproved-sticky-violation` | The new head would change a file you did not approve. | Review the list and approve every path, or refuse. | you: it is on this machine |
| `creator-changed` | The rewrite changes who created a record. | Read the path list carefully and only approve if admins confirm. | an admin of your org's store |

