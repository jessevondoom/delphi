//! The `delphi xenia help <topic>` pages. Fixed text: nothing here is built from a store, a key or a path. The operator
//! guide (`docs/guides/xenia-operator.md`) is generated from these (see `xenia_help::guide_markdown`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Topic {
    pub name: &'static str,
    pub title: &'static str,
    pub body: &'static str,
}

pub(super) const GUIDE_HEAD: &str = "# Xenia: the operator's guide

Xenia is your organization's shared, signed memory: records one person publishes, a steward vouches for and everyone else can search. This guide is generated from the same text `delphi xenia help` prints, so the two cannot disagree (a test fails when this file drifts; regenerate it with `DELPHI_REGEN_DOCS=1 cargo test -p delphi-core xenia_help`). The design is [`specs/011`](../../specs/011-xenia-trust-model.md) and [`012`](../../specs/012-xenia-record-schema.md).

Everything here works offline and never calls a model. Ask the same questions of the running assistant with the `xenia_help` tool, or in a terminal with `delphi xenia help <topic>` and `delphi xenia explain <slug>`.
";

pub(super) const ORIENTATION: &str = "Xenia is your organization's shared, signed memory. A member publishes a record (a finding, a decision, a gotcha, a how-to); a steward who covers its subject reads it and endorses it; every install checks the signatures itself, so the store's host never has to be trusted. Records nobody has endorsed are still searchable, but at a lower trust level (L1); endorsed ones are L2.

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
";

pub static TOPICS: &[Topic] = &[
    Topic {
        name: "getting-started",
        title: "Getting started: joining a store",
        body: r#"You need three things from an admin, given to you over a channel that is not the store itself (a call, a chat with someone you know): the store's address, the admin's fingerprint, and the roster head `<epoch>:<hash>`. The admin's genesis step prints the exact line.

  1. Run `delphi xenia whoami`. It creates your key on first use and prints your DID and fingerprint. Send both to an admin; they add you to the roster. Keep the key file private (it is mode 0600 under your Delphi home; never share it).
  2. Run the join line the admin gave you:
       delphi xenia join <url> --org <org> --store <store> --admin <fingerprint> --head <epoch>:<hash>
     Join fetches the store and checks that the roster verifies to that head and that the admin you named is active in it. If anything differs it writes nothing. An address that is a local path needs `--allow-local`.
  3. Run `delphi xenia sync`, then `delphi xenia status`. A fresh store with no valid statement accepts nothing (`no-trust-base`): ask an admin to refresh.
  4. Search endorsed records from a workspace with a knowledge base; they show up in `delphi kb search` and to agents through the `xenia_search` tool.

Your clock must be within a few minutes of real time: freshness is judged by it (`issued-in-future`).
If join is refused, `delphi xenia explain <word in the message>`."#,
    },
    Topic {
        name: "publishing",
        title: "Publishing a record",
        body: r#"Only content you have already promoted from team or workspace scope can be published; anything from your operator scope never leaves the machine.

  delphi xenia publish <file> --title "..." --summary "..." --promoted [--tags a,b] [--type finding|decision|gotcha|how-to] [--review-by YYYY-MM-DD]

What happens: Delphi builds the record, scans it for secrets, private paths and personal identifiers, then shows you a card with the exact bytes it will sign, any flags and similar existing records. You type the card's short code at a terminal. No model runs between the card and the signature. A published secret cannot be un-leaked: rotate it.

A record you publish reads L1 until a steward who covers its tags endorses it. You cannot endorse your own. To change it, publish again (a new revision) with the same file; to take it down, `delphi xenia withdraw <id> --reason mistake`; to bring it back, `delphi xenia restore <id>`.

If it is refused, the message names a slug: `delphi xenia explain <slug>`."#,
    },
    Topic {
        name: "endorsing",
        title: "Endorsing, disputing and withdrawing (stewards and members)",
        body: r#"A steward reads the exact revision (and opens its pointers) and signs an endorsement with `delphi xenia endorse <id> [--scope a,b] [--note "..."]`. It counts only when: you are not the author (or the author's predecessor key), your areas cover the record, you are an active member, and the freshness statement is current. A new revision needs a new endorsement.

  delphi xenia dispute <id> --note "why"      any member: flags a record to stewards; it does not change the tier by itself.
  delphi xenia retract <id>                    withdraw your own endorsement.
  delphi xenia withdraw <id> --reason <r>      hide a record: yours, or (for a covering steward) anyone's. If stewards disagree the record is `contested`: hidden until they agree.
  delphi xenia restore <id>                    bring a hidden record back.

You can do these at a terminal, or in the console with `/xenia endorse <id>` followed by `/xenia sign <code>`: the console shows the same card and only your own typed code signs. An agent cannot answer for you."#,
    },
    Topic {
        name: "being-an-admin",
        title: "Being an admin",
        body: r#"An admin holds a key that can change the roster and sign the freshness statement. Admins are counted per person, and a roster change needs a majority of the current admins, so keep at least three people as admins: with two, losing one key freezes the store (`few-admins`).

Your duties:
  - Refresh the freshness statement before it expires. The default validity is 7 days and no install accepts more than 14. `status` warns at half of its life (`statement-aging`). Refreshing first checks the whole history for tampering and refuses to sign if it finds any: `delphi xenia admin refresh`.
  - Handle roster changes as proposals: one admin proposes (`delphi xenia admin propose ...`, an add, a removal or a rotation), each other admin reviews the preview and signs, and once enough have signed one admin publishes it. Read every preview: it lists the exact change and flags anything that adds an admin, widens a steward's areas, lets a key speak twice for one person, or voids someone's tombstones. You may refuse.
  - Never share your admin key, and run admin commands only at a terminal you control.

Admin subcommands: `add`, `remove`, `rotate` (with `--routine` or `--compromised`), `show <n>`, `sign <n>`, `publish <n>`, `refresh` and `genesis draft|sign|publish`; `delphi xenia admin` lists them. They need your key and a terminal; a relay or an agent cannot run them."#,
    },
    Topic {
        name: "genesis",
        title: "The genesis ceremony: creating a store with three admins",
        body: r#"Genesis writes the very first roster. Every founding admin must agree to it, so every founder signs the same draft. For a 2-of-3 store:

Prepare first:
  - A private, EMPTY git repository, reachable over https or ssh, where all three admins can push. (The first push must create the branch; a store that already has content is refused: `store-not-empty`.)
  - Three people, each with a Delphi install. Each runs `delphi xenia whoami` and sends the DID and fingerprint to the person drafting the roster.
  - Clocks within a few minutes of real time on all three machines.
  - A short call or chat for the fingerprints: members will check them over a channel that is not the store.

The ceremony:
  1. One founder drafts the roster naming the three people and their DIDs as admins (plus any first members and stewards) and sends the draft to the other two.
  2. Each founder reads the preview of every key and role, and signs the draft they parsed. Anyone who is not named, or who sees a key they do not recognize, refuses.
  3. One founder collects the three signatures and pushes. The first commit holds the roster, the second the first freshness statement. The command prints a join line with the store address, the admin's fingerprint and `1:<hash>`.
  4. Each founder (and later every member) reads the fingerprint to the other over a call, runs the join line, then `delphi xenia sync` and `delphi xenia status`. Status should show epoch 1, 3 admins and a fresh statement.

The commands, in order: `delphi xenia admin genesis draft --org <o> --store <s> --founder <did>,<person>,<handle>,admin ... --out draft.json` (one `--founder` per person); each founder `delphi xenia admin genesis sign draft.json --org <o> --store <s> --out <name>.sig.json`; then `delphi xenia admin genesis publish draft.json --url <url> --org <o> --store <s> --sig a.sig.json --sig b.sig.json --sig c.sig.json`.

Then each admin puts a reminder in their calendar to refresh every week (`delphi xenia admin refresh`)."#,
    },
    Topic {
        name: "rotating-keys",
        title: "Rotating a key: routine or compromised",
        body: r#"You must say which. There is no default.

Routine (a new laptop, a planned change): an admin proposes `rotate <member> --routine`. The old key is marked retired with the new key as successor, same person. The member then runs `delphi xenia republish --resign` with the new key, which re-publishes their records, re-issues their endorsements and re-signs their tombstones and disputes as new revisions, so they keep their tier. Records with an effective withdraw are skipped, so rotation cannot resurface hidden content.

Compromised (the key may be stolen):
  1. Take the old key out of use. Rotate any secret it could have exposed.
  2. An admin proposes `rotate <member> --compromised`; the other admins sign and one publishes. This voids everything the old key signed from that epoch on: records, endorsements and tombstones, including withdrawals.
  3. Nothing is re-signed automatically, and `republish --resign` refuses a compromised rotation.
  4. The new key's owner re-publishes only what they reviewed, as new revisions.
  5. Stewards re-endorse what is worth keeping.
  6. A record a thief hid can be restored afresh by its author or a covering steward.
  7. Watch `delphi xenia status` for contested or quarantined records for a few days.

With 2-of-3 admins, a compromised admin key plus one colluding admin is a quorum: if two admins may be compromised, rebuild the store from a new genesis (everyone re-joins)."#,
    },
    Topic {
        name: "freshness",
        title: "Freshness: why endorsements stop counting, and how to refresh",
        body: r#"A store can hide a removal or a withdrawal by serving old data. To prevent it, admins sign a freshness statement that says "as of this head the store is complete" and expires (default 7 days, cap 14). While it is current, endorsements count. When it is stale, every record reads at most L1 (unendorsed); withdrawals and hides still apply.

  delphi xenia status                       shows the age and share of life used. Past 50%, a warning.
  delphi xenia admin refresh                (an admin) checks the history, then signs a new statement.

Reasons a statement is not current (see `status`, then `delphi xenia explain <slug>`): expired, missing, replayed-statement (the store served an older one than you saw), issued-in-future (a clock problem), validity-over-cap, no-trust-base (you have never seen a valid one).

If you are not an admin: tell one. If you are, refresh at least twice per statement life, and put it in your calendar."#,
    },
    Topic {
        name: "recovery",
        title: "Recovery when the store's history was rewritten",
        body: r#"If the host's history is force-pushed or rewritten, your sync refuses it (`non-fast-forward`, `diverged`, or `tampering`): nothing is applied, you keep what you had, and nothing is lost. Do not keep retrying.

Drill:
  1. Run `delphi xenia status` to see which signed files were deleted or reverted.
  2. Tell the admins. They must find out who rewrote the history and why (a force-push by someone with write access? a restored backup? an attack?).
  3. If the rewrite is legitimate, a MAJORITY of the epoch's admins (counted per person) sign a freshness statement for the new head that names the rewritten head it replaces. One admin cannot do this alone.
  4. Each install's operator is shown every signed file that was deleted or reverted and confirms with `delphi xenia sync --accept-rewritten-head`. Only the files you approve are replaced; everything else you have stays. In this build the library is complete but the command is not yet wired into the CLI: until it is, a rewritten store stays refused (safe), and the fallback is to join the rebuilt store afresh.
  5. If a majority of admins is compromised, or the roster is deadlocked, the org rebuilds from a new genesis and everyone re-joins with the new fingerprint, over a channel that is not the store.

Do not accept a rewrite just to make a warning go away."#,
    },
    Topic {
        name: "offline",
        title: "Working offline",
        body: r#"Everything but `sync`, `join` and the commands that push works without a network: `status`, `explain`, `help`, search over what you already have. A failed sync (`unavailable`) changes nothing.

The clock keeps running: the freshness statement has a fixed expiry, so after a long time offline the endorsed records you hold read L1 (stale) until you sync and an admin has refreshed. That is deliberate and safe; it does not delete anything.

Signing commands (`publish`, `endorse`, admin commands) need a network to push; the card and your typed code come first, and nothing is sent when the push fails."#,
    },
    Topic {
        name: "approval",
        title: "Approving: console versus terminal, and why a human must",
        body: r#"Signing is always a person's decision. The card shows the exact bytes; you type the card's short code; nothing else signs. No model runs between the card and the signature. An agent, the relay (email or chat) and the Narrator cannot approve or sign anything in Xenia.

At a terminal: every command that signs works, including all admin commands. It refuses when stdin or stdout is not a terminal.

In the console: `/xenia endorse|dispute|withdraw|restore|publish|republish ...` shows the card, `/xenia sign <code>` signs, `/xenia cancel <code>` refuses. The console may approve routine member actions. It never approves genesis, rotation, rewrite recovery, anything that touches an admin, or accepting a rewritten head. Console approval of routine admin proposals that touch no admin is controlled by `[xenia] console_admin` in the operator config and is OFF by default (it arrives with the admin commands).

The relay only delivers messages. A relay message that says what to do tells you what to run yourself."#,
    },
    Topic {
        name: "security",
        title: "The security model in plain words",
        body: r#"What a hostile or careless store host can do: refuse to answer, serve old data, delete or rewrite history, or fill the store with junk. What it cannot do: make a record appear that no active member signed, make an endorsement count that no active steward signed, change who the admins are without a quorum of them, or make you accept a new head that deletes something you already saw. Your install checks every signature and every rule itself, and refuses the whole head when a check fails, keeping what you had.

What it can still do: withhold new updates (you will see `unavailable`, `behind` or a stale statement, never silent acceptance) and learn what you read from it if the host is not private. Records are text written by colleagues: they reach a model only as short, labelled summaries, and reading one lowers that task's trust ceiling.

Why approval needs a human: a signature is the only thing that makes a record count, so the person whose name goes on it must see the exact bytes and type the card's code at their own terminal or console. Agents, the relay and the Narrator are refused by name, and Xenia help text is the same on every surface: it is fixed text, never built from store content.

Console and terminal: the console may approve routine member actions (publish, endorse, dispute, withdraw, restore, republish). Everything touching admins, genesis, rotation and accepting a rewritten head is terminal-only. Console approval of routine admin proposals that touch no admin is off by default and controlled by `[xenia] console_admin` in the operator config.

The honest limits: a compromised admin key plus one colluding admin is a quorum in a 2-of-3 store; a published secret cannot be recalled, only rotated; the store's own clock does not exist, so your machine's clock matters."#,
    },
    Topic {
        name: "troubleshooting",
        title: "Troubleshooting",
        body: r#"Start with `delphi xenia status`, then `delphi xenia explain <slug>` for any word it shows. `delphi doctor` check 13 repeats the warnings with the same advice.

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

If a message is not in this list, `delphi xenia explain "<the text>"` matches it; unknown text is not echoed back."#,
    },
];
