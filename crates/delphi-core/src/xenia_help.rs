//! How Xenia describes itself to an operator (plan milestone X9c): one catalog of fixed sentences, keyed by the fixed
//! slugs the rest of Xenia already prints, and the few functions that word it for the terminal, the console, the relay
//! and an agent.
//!
//! * [`CATALOG`] maps every operator-visible condition (a sync outcome, a freshness state, a quarantine reason, a
//!   refusal, a warning) to what it means, why it matters, the exact next step, who to ask, and whether it may be
//!   ignored. A test reads the library sources and fails when a slug appears there that has no entry here.
//! * [`TOPICS`] (in `xenia_help_topics.rs`) are the `delphi xenia help <topic>` pages; `docs/guides/xenia-operator.md`
//!   is generated from them and the catalog ([`guide_markdown`]), and a test fails when the file drifts.
//! * [`relay_message`] renders a condition as a short plain-text message for the relay, email and Narrator channel.
//!
//! **Everything here is a fixed string written by us.** Nothing is built from store text, a key, a DID, a path or a
//! URL, and nothing here does I/O, reads state or a clock: a function of its arguments only. A slug that is not in
//! the catalog is never echoed back; callers get `None` or a fixed sentence.

#[path = "xenia_help_topics.rs"]
mod topics;
pub use topics::{Topic, TOPICS};

/// What sort of condition a slug names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The result of `delphi xenia sync`.
    Sync,
    /// Why a freshness statement does not count, or that it does.
    Freshness,
    /// Why a signed file is held back or ignored (`status` lists them as quarantined).
    Quarantine,
    /// Why a record is hidden from everyone.
    Hidden,
    /// What the history check found when it refused a head.
    Finding,
    /// Why a command (publish, endorse, admin ...) was refused.
    Refusal,
    /// A line `status`, `doctor` check 13 and the events show.
    Warning,
    /// Recovery from a rewritten store.
    Recovery,
}

impl Kind {
    pub fn word(self) -> &'static str {
        match self {
            Kind::Sync => "sync result",
            Kind::Freshness => "freshness",
            Kind::Quarantine => "held-back file",
            Kind::Hidden => "hidden record",
            Kind::Finding => "history finding",
            Kind::Refusal => "refusal",
            Kind::Warning => "warning",
            Kind::Recovery => "recovery",
        }
    }
}

/// Who can fix it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Who {
    Nobody,
    You,
    Member,
    Steward,
    Admin,
}

impl Who {
    pub fn text(self) -> &'static str {
        match self {
            Who::Nobody => "nobody: it clears by itself, or nothing is wrong",
            Who::You => "you: it is on this machine",
            Who::Member => "the member who wrote the record",
            Who::Steward => "a steward of your org's store",
            Who::Admin => "an admin of your org's store",
        }
    }
}

/// Can the operator leave it alone?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ignore {
    Yes,
    No,
    /// For a while; the entry's `why` says what happens if it is left.
    ForNow,
}

impl Ignore {
    pub fn text(self) -> &'static str {
        match self {
            Ignore::Yes => "yes",
            Ignore::No => "no",
            Ignore::ForNow => "for a while, not for long",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub slug: &'static str,
    pub kind: Kind,
    /// One line, plain language.
    pub meaning: &'static str,
    /// What is at risk.
    pub why: &'static str,
    /// The exact next step: a command, or a panel.
    pub next: &'static str,
    pub who: Who,
    pub ignore: Ignore,
}

const fn e(slug: &'static str, kind: Kind, who: Who, ignore: Ignore, meaning: &'static str, why: &'static str, next: &'static str) -> Entry {
    Entry { slug, kind, meaning, why, next, who, ignore }
}

use Ignore::{ForNow, No, Yes};
use Kind::{Finding, Freshness, Hidden, Quarantine, Recovery, Refusal, Sync, Warning};
use Who::{Admin, Member, Nobody, Steward, You};

const STATUS: &str = "Run `delphi xenia status` to see which files, then `delphi xenia help troubleshooting`.";
const ASK_ADMIN_REFRESH: &str = "Ask an admin to run `delphi xenia admin refresh`, then run `delphi xenia sync`.";
const REPORT_TO_ADMIN: &str = "Do not retry. Tell an admin what `delphi xenia status` shows; the store host or someone with push rights may have changed history.";
const FIX_AND_RETRY: &str = "Correct the input named in the message and run the command again; nothing was signed or sent.";
const SEE_STORE: &str = "Run `delphi xenia sync`, then `delphi xenia status`, and try again.";
const RECORD_BAD: &str = "The file is held back and never loaded. Tell the member who wrote it (or a steward) to publish a corrected revision.";
const ROSTER_BAD: &str = "This is a problem with the roster chain, not with you. Do not accept it: tell an admin, and run `delphi xenia help recovery`.";
const NOTHING_TO_DO: &str = "Nothing to do.";

/// Every condition an operator can be shown. Slugs are unique (a test pins it). Order is by [`Kind`], then by how often
/// an operator meets it.
pub static CATALOG: &[Entry] = &[
    // ---- sync results -------------------------------------------------------------------------------------
    e("accepted", Sync, Nobody, Yes, "The sync brought in new, verified records.", "Nothing is at risk; this is the normal outcome.", NOTHING_TO_DO),
    e("up-to-date", Sync, Nobody, Yes, "The store has nothing new since your last sync.", "Nothing is at risk. The freshness clock still runs while the store sits still.", "Nothing to do. `delphi xenia status` shows how long the freshness statement has left."),
    e("unavailable", Sync, You, ForNow, "The store could not be reached (offline, a network or login problem, or the host is down).", "Nothing was changed and what you have stays in force, but the freshness statement keeps aging: past its expiry endorsed records read as unendorsed.", "Check the network and your access to the store, then run `delphi xenia sync` again. `delphi xenia help offline` explains what still works."),
    e("tampering", Sync, Admin, No, "The store's new history deleted, reverted or took over signed files, so the whole head was refused.", "Someone with write access to the store may be hiding or changing records. Nothing was applied; you keep what you had.", REPORT_TO_ADMIN),
    e("rejected", Sync, Admin, No, "The new head failed a trust check (the roster, the freshness statement or the chain from the last verified head), so it was refused.", "The store is not proving it is the store you joined. Nothing was applied.", "Run `delphi xenia status` for the reason slug, then `delphi xenia explain <slug>`. If it persists, ask an admin."),
    e("non-fast-forward", Sync, Admin, No, "The store's history was rewritten or force-pushed (or an older view is being served).", "Rewriting history can silently delete signed records. The head was refused and nothing changed.", "Do not retry. Ask an admin to investigate; recovery needs an admin quorum. Run `delphi xenia help recovery`."),
    e("refused", Sync, Admin, No, "The fetched store broke a safety rule (unsafe file types, too large, a bad settings file), so it was refused before any record was read.", "A hostile or broken store could waste disk or try to run something on checkout. Nothing was applied.", "Run `delphi xenia status`, read the reason slug, then `delphi xenia explain <slug>`. Ask an admin."),
    e("diverged", Sync, Admin, No, "Your accepted head and the store's head each have commits the other lacks.", "That is what a force-push looks like. Nothing was applied.", "Do not retry. Ask an admin; see `delphi xenia help recovery`."),
    e("behind", Sync, Admin, ForNow, "The store is serving an older view than you already accepted.", "Updates may be withheld, or a branch was reverted. Nothing was applied, but you may be missing newer records.", "Wait a few minutes and sync again. If it stays behind, ask an admin to look at the store host."),
    // ---- freshness ----------------------------------------------------------------------------------------
    e("fresh", Freshness, Nobody, Yes, "An admin vouched recently that the store is complete, so endorsements count.", "Nothing is at risk.", "Nothing to do. An admin should refresh before it runs out."),
    e("statement-aging", Warning, Admin, ForNow, "The admins' freshness statement is more than half used up.", "When it expires, every endorsement stops counting and records drop from endorsed (L2) to unendorsed (L1).", ASK_ADMIN_REFRESH),
    e("missing", Freshness, Admin, No, "The store has no freshness statement.", "Without one nothing can show the store is current, so no endorsement counts.", ASK_ADMIN_REFRESH),
    e("expired", Freshness, Admin, No, "The freshness statement ran out.", "No endorsement counts until an admin issues a new one: every record reads at most L1 (unendorsed). Withdrawals still apply.", ASK_ADMIN_REFRESH),
    e("nothing-accepted", Freshness, You, No, "No head has been accepted yet, so there is nothing to judge.", "Nothing from the store is in force.", "Run `delphi xenia sync`."),
    e("not-synced", Warning, You, No, "Nothing has been accepted from this store yet.", "None of its records are loaded.", "Run `delphi xenia sync`."),
    e("roster-invalid", Freshness, Admin, No, "The roster the statement depends on does not verify.", "Nothing the store says can be trusted until the roster chain verifies.", ROSTER_BAD),
    e("replayed-statement", Freshness, Admin, No, "The store served an older freshness statement than one you already saw.", "That is how a store hides recent removals: it keeps showing an old \"all clear\". The old statement is ignored.", "Ask an admin to publish a new statement with `delphi xenia admin refresh`; do not accept the old one."),
    e("issued-in-future", Freshness, You, ForNow, "The statement is dated later than this machine's clock.", "Usually this machine's clock is behind; otherwise the statement is forged. Until it is current no endorsement counts.", "Check this machine's date and time (they must be within a few minutes of real time), then sync again. If the clock is right, tell an admin."),
    e("validity-over-cap", Freshness, Admin, No, "The statement claims to be valid for longer than the 14 days every install allows.", "A long-lived statement would let a stale store look fresh. It is ignored.", "Ask an admin to issue a statement with the default 7-day validity."),
    e("expires-too-far", Freshness, Admin, No, "The statement's expiry is too far in the future for this install's clock.", "Same risk as a statement that is valid for too long. It is ignored.", "Check this machine's clock, then ask an admin to refresh with a shorter validity."),
    e("expires-before-issued", Freshness, Admin, No, "The statement expires before it was issued.", "A malformed statement; it is ignored.", ASK_ADMIN_REFRESH),
    e("no-trust-base", Freshness, Admin, No, "This install has never seen a valid freshness statement, so it accepts nothing.", "A new store cannot be trusted until an admin has signed a first statement.", "Ask an admin to run `delphi xenia admin refresh`, then sync."),
    e("no-valid-statement", Freshness, Admin, No, "The store has no freshness statement that checks out.", "Nothing can show the store is complete, so no new head is accepted.", ASK_ADMIN_REFRESH),
    e("no-admin-signature", Freshness, Admin, No, "The statement is not signed by an admin who is active in the roster.", "Anyone could write such a statement; it is ignored.", ASK_ADMIN_REFRESH),
    e("rewrite-quorum-missing", Freshness, Admin, No, "A statement says history was rewritten but a majority of admins did not sign it.", "One admin (or a thief) cannot approve a rewrite alone. It is ignored.", "Ask the admins to agree, then have a majority sign. See `delphi xenia help recovery`."),
    e("older-epoch", Freshness, Admin, No, "The statement was written for an older roster than the current one.", "It does not vouch for the store as it is now. It is ignored.", ASK_ADMIN_REFRESH),
    e("newer-than-chain", Freshness, Admin, No, "The statement is for a roster newer than the one that verified.", "The roster chain you were served is behind the statement, a sign of a withheld update.", "Sync again in a few minutes; if it persists, ask an admin."),
    e("epoch-hash-mismatch", Freshness, Admin, No, "The statement names a different roster than the verified one.", "The statement and the roster disagree; it is ignored.", "Ask an admin to check the store and refresh."),
    e("store-head-not-ancestor", Freshness, Admin, No, "The store's new head does not build on the head the admins last vouched for.", "History may have been rewritten after the admins looked.", REPORT_TO_ADMIN),
    e("sigs-oversize", Freshness, Admin, No, "The statement's signature file is too large.", "A malformed or hostile file; it is ignored.", ASK_ADMIN_REFRESH),
    e("sigs-malformed", Freshness, Admin, No, "The statement's signature file is not in the expected shape.", "It is ignored.", ASK_ADMIN_REFRESH),
    e("no-roster", Sync, Admin, No, "The store has no roster, so there is nobody to trust.", "Nothing can be verified.", "Check you joined the right store with `delphi xenia status`; ask an admin."),
    // ---- warnings -----------------------------------------------------------------------------------------
    e("few-admins", Warning, Admin, ForNow, "Fewer than three people hold an admin key.", "Roster changes and freshness refreshes need two admins: losing one key could freeze the store for good.", "Add admins: `delphi xenia help being-an-admin`."),
    e("stale", Warning, Admin, No, "The freshness statement is not current, so no endorsement counts.", "Every record reads at most L1 (unendorsed) until an admin refreshes.", ASK_ADMIN_REFRESH),
    e("sync-refused", Warning, Admin, No, "The last sync was refused; the store's head was not accepted.", "You are working from older state and the store may be misbehaving.", STATUS),
    e("state-permissions", Warning, You, No, "One of this install's private Xenia state files (pins or journal) is not a private regular file.", "Another user or a link could change what this install trusts.", "Run `chmod 600` on the file named in the warning, or join the store again."),
    e("store-unreadable", Warning, You, No, "This install cannot read its own copy of the store.", "Nothing about the store can be checked.", "Run `delphi xenia sync`; if it fails the same way, move the folder under `$DELPHI_HOME/xenia/` away and join again."),
    e("contested", Hidden, Steward, No, "Stewards who cover the record disagree about hiding it, so it is hidden from everyone.", "A record in dispute is not shown until they agree.", "Ask the covering stewards to agree (one can restore or withdraw)."),
    // ---- hidden records ----------------------------------------------------------------------------------
    e("author-superseded", Hidden, Nobody, Yes, "The author replaced this record with a newer one.", "Nothing: the newer one is shown.", NOTHING_TO_DO),
    e("author-withdrawn", Hidden, Member, Yes, "The author withdrew this record.", "It is gone on purpose.", "If it was a mistake, the author can restore it with `delphi xenia restore`."),
    e("author-tombstone", Hidden, Member, Yes, "The author hid this record.", "It is gone on purpose.", "The author can restore it with `delphi xenia restore <id>`."),
    e("steward-tombstone", Hidden, Steward, Yes, "A steward who covers this record hid it and none disagrees.", "It was judged wrong or unsafe.", "To bring it back, ask the steward or a covering steward to restore it."),
    e("sensitive-hold", Hidden, Steward, No, "A steward hid this record as sensitive; only that steward can reverse it.", "It may hold something that should not be shared.", "Ask that steward. If a secret was published, rotate the secret: hiding does not un-leak it."),
    // ---- held-back and quarantined files -----------------------------------------------------------------
    e("bad-frontmatter", Quarantine, Member, Yes, "The record's header is not in the strict format.", "It is never loaded.", RECORD_BAD),
    e("unsupported-schema", Quarantine, You, ForNow, "The record uses a newer format than this install understands.", "Not an attack: it is held back until you update.", "Update Delphi (`delphi update`), then sync."),
    e("forbidden-key", Quarantine, Member, Yes, "The record tries to state trust (a tier, an endorsement, an approval) in its own text.", "A record cannot vouch for itself. It is never loaded.", RECORD_BAD),
    e("bad-schema", Quarantine, Member, Yes, "The record is missing a required field or has a wrong value.", "It is never loaded.", RECORD_BAD),
    e("mismatch", Quarantine, Member, Yes, "Fields that must agree (path, id, signer, org, store) do not.", "A misplaced or copied record. It is never loaded.", RECORD_BAD),
    e("bad-text", Quarantine, Member, Yes, "The record's text has hidden or control characters, or text that breaks the format.", "Invisible characters can hide instructions. It is never loaded.", RECORD_BAD),
    e("over-limit", Quarantine, Member, Yes, "The record has more tags, pointers or entries than allowed.", "It is never loaded.", RECORD_BAD),
    e("bad-pointer", Quarantine, Member, Yes, "A pointer in the record is not an allowed kind of link.", "Pointers are shown as text and never fetched; a bad one is refused outright.", RECORD_BAD),
    e("bad-date", Quarantine, Member, Yes, "A date in the record is not in the one allowed format.", "It is never loaded.", RECORD_BAD),
    e("not-active", Quarantine, Admin, Yes, "The signer's key is not active in the current roster (removed, retired or not yet added).", "Records and endorsements signed by a key without standing do not count.", "If the person should be a member, an admin adds or rotates their key (`delphi xenia help being-an-admin`)."),
    e("signer-not-active", Quarantine, Admin, Yes, "This key is not an active member of the roster, so nothing it signs counts.", "Anything you publish would be ignored by everyone.", "Run `delphi xenia whoami`, send the DID to an admin, and ask them to add you."),
    e("duplicate-path", Quarantine, Nobody, Yes, "The same file path was seen twice.", "Only one copy can count; the extra is ignored.", NOTHING_TO_DO),
    e("duplicate", Quarantine, Nobody, Yes, "A repeat of something already counted (the same endorsement or tombstone twice).", "Only one counts.", NOTHING_TO_DO),
    e("endorsement-not-steward", Quarantine, Steward, Yes, "The endorsement was signed by someone who is not a steward.", "Only stewards can endorse.", "A steward must endorse the record: `delphi xenia endorse <id>`."),
    e("id-collision", Quarantine, Member, Yes, "Two different records claim the same id.", "Only the first author's record counts.", "The later author publishes under a new id."),
    e("outside-areas", Quarantine, Steward, Yes, "The steward endorsed or hid a record outside the areas they cover.", "A steward's power ends at their areas.", "A steward who covers the record must act, or an admin widens this steward's areas."),
    e("scope-outside-areas", Quarantine, Steward, Yes, "The endorsement names scope tags the steward does not cover.", "The endorsement is ignored.", "Endorse with a scope the steward covers, or ask an admin to widen their areas."),
    e("tombstone-not-author-or-covering-steward", Quarantine, Steward, Yes, "A tombstone was signed by someone who may not hide that record.", "Only the author or a covering steward can hide a record.", NOTHING_TO_DO),
    e("cancelled-by-same-person", Quarantine, Nobody, Yes, "The signer cancelled their own earlier endorsement.", "It no longer counts.", NOTHING_TO_DO),
    e("dispute-of-another-revision", Quarantine, Nobody, Yes, "A dispute is about an earlier revision of the record.", "It does not apply to the current text.", NOTHING_TO_DO),
    e("freshness-stale", Quarantine, Admin, No, "An endorsement is valid but cannot count while the freshness statement is stale.", "Endorsed records read at L1 until it is refreshed.", ASK_ADMIN_REFRESH),
    e("hash-mismatch", Quarantine, Member, Yes, "The bytes do not match the hash that was signed (or the endorsement is for other bytes).", "The record changed after signing, or is corrupt. It is never loaded.", RECORD_BAD),
    e("replaced-by-successor", Quarantine, Nobody, Yes, "A key was rotated and its successor re-issued this.", "The successor's copy counts.", NOTHING_TO_DO),
    e("self-endorsement", Quarantine, Steward, Yes, "A steward tried to endorse their own record (or their predecessor key's).", "Nobody may vouch for themselves.", "Another steward must endorse it."),
    e("target-hidden", Quarantine, Nobody, Yes, "The endorsement is for a record that is now hidden.", "It changes nothing while the record is hidden.", NOTHING_TO_DO),
    e("target-rev-mismatch", Quarantine, Steward, Yes, "The endorsement was for an older revision than the current one.", "A new revision needs a new endorsement.", "A steward reviews the new revision and endorses it: `delphi xenia endorse <id>`."),
    e("unknown-path", Quarantine, Nobody, Yes, "A file in the store is not one of the file kinds Xenia knows.", "It is never loaded.", NOTHING_TO_DO),
    e("missing-sidecar", Quarantine, Member, Yes, "A record has no signature file next to it.", "Unsigned means unverified. It is never loaded.", RECORD_BAD),
    e("not-tracked", Quarantine, You, ForNow, "A record did not make it through the last sync's checks.", "It is not loaded.", "Run `delphi xenia sync`; if it stays, the reason shows in `delphi xenia status`."),
    e("bad-sidecar", Quarantine, Member, Yes, "The signature file next to a record is malformed.", "Unverifiable; never loaded.", RECORD_BAD),
    e("bad-path", Quarantine, Member, Yes, "The file's path or name is not allowed.", "Odd names are a way to hide or overwrite things. It is never loaded.", RECORD_BAD),
    e("bad-field", Quarantine, Member, Yes, "A field has a value that is not allowed.", "It is never loaded.", RECORD_BAD),
    e("bad-did", Quarantine, Member, Yes, "A signer identity is not a valid key identity.", "It cannot be checked. It is never loaded.", RECORD_BAD),
    e("bad-signature", Quarantine, Member, Yes, "The signature does not verify.", "The record is forged or damaged. It is never loaded.", RECORD_BAD),
    e("key", Refusal, You, No, "This install's Xenia key file is missing, damaged or readable by others.", "Without a safe key nothing can be signed.", "Run `delphi xenia whoami` (it creates a key if none exists). If the file is readable by others, run `chmod 600` on it."),
    e("oversize", Quarantine, Member, Yes, "The file is larger than allowed.", "Huge files are a way to exhaust disk or memory. It is never loaded.", RECORD_BAD),
    // ---- history findings ---------------------------------------------------------------------------------
    e("deleted", Finding, Admin, No, "A file that was in the store is gone.", "Deleting a withdrawal, dispute or record is how a store hides something.", REPORT_TO_ADMIN),
    e("no-longer-verifies", Finding, Admin, No, "A file you accepted earlier no longer verifies.", "It was altered in place.", REPORT_TO_ADMIN),
    e("rev-rollback", Finding, Admin, No, "A file went back to an older revision.", "Reverting is how an old, unsafe version comes back.", REPORT_TO_ADMIN),
    e("same-rev-different-hash", Finding, Admin, No, "A file was replaced by different content with the same revision number.", "Swapping content in place defeats review.", REPORT_TO_ADMIN),
    e("tracking-changed", Finding, Admin, No, "A path changed what kind of file it is.", "A way to smuggle one file type in as another.", REPORT_TO_ADMIN),
    e("takeover", Finding, Admin, No, "Someone other than the owner changed a record they did not write.", "Only the author (or their successor key) may revise a record.", REPORT_TO_ADMIN),
    e("author-state-regression", Finding, Admin, No, "A withdrawn or superseded record was brought back to active.", "Hidden content must stay hidden.", REPORT_TO_ADMIN),
    e("epoch-file-changed", Finding, Admin, No, "A roster file that should never change was edited.", "Roster history is pinned; editing it is how trust is hijacked.", REPORT_TO_ADMIN),
    e("epoch-differs-from-chain", Finding, Admin, No, "A roster file differs from the verified chain.", "Same as above.", REPORT_TO_ADMIN),
    e("non-regular-file", Finding, Admin, No, "A path became a link or other unusual file type.", "Links can point outside the store.", REPORT_TO_ADMIN),
    // ---- store safety (refused heads) --------------------------------------------------------------------
    e("hostile-tree", Sync, Admin, No, "The store contains kinds of file a store never legitimately has (links, submodules, hidden git files).", "These can read files outside the store or run code on checkout.", "Do not retry. Ask an admin to clean the store; nothing was applied."),
    e("tree-too-large", Sync, Admin, No, "The store has more files than the safety limit.", "A flood is a way to exhaust disk or memory.", "Ask an admin to trim the store."),
    e("object-too-large", Sync, Admin, No, "A single stored object is over the size limit.", "As above.", "Ask an admin to remove the large file from history."),
    e("too-many-objects", Sync, Admin, No, "One fetch brought in more objects than the limit.", "As above.", "Ask an admin; a store that grew this much should be rebuilt."),
    e("too-many-commits", Sync, Admin, No, "One fetch brought in more commits than the limit.", "As above.", "Sync again later; if it persists ask an admin."),
    e("fetch-too-large", Sync, Admin, No, "The fetch would make the local copy larger than the limit.", "As above.", "Ask an admin to trim the store."),
    e("gitattributes-missing", Sync, Admin, No, "The store lacks its settings file (`.gitattributes`).", "Without it, checkouts could rewrite line endings and break signatures.", "Ask an admin to restore the store's `.gitattributes`."),
    e("gitattributes-unreadable", Sync, Admin, No, "The store's settings file is not readable text or is too large.", "As above.", "Ask an admin to restore it."),
    e("gitattributes-no-default-no-text", Sync, Admin, No, "The store's settings file does not keep files byte-exact.", "As above.", "Ask an admin to restore it."),
    e("gitattributes-forbidden-attribute", Sync, Admin, No, "The store's settings file asks git to run a program or convert files.", "A way to run code or alter signed bytes.", "Do not retry. Ask an admin to inspect the store."),
    e("gitattributes-malformed", Sync, Admin, No, "The store's settings file has a line that is not allowed.", "As above.", "Ask an admin to restore it."),
    e("malformed", Freshness, Admin, No, "A file is not in the expected format.", "It is ignored.", ASK_ADMIN_REFRESH),
    // ---- roster refusals ----------------------------------------------------------------------------------
    e("bad-scope", Refusal, You, No, "The org or store name is not a valid name.", "Nothing was changed.", FIX_AND_RETRY),
    e("no-anchor", Refusal, You, No, "This install has nothing pinned to check the roster against.", "A roster that vouches for itself proves nothing.", "Join the store with `delphi xenia join` (see `delphi xenia help getting-started`)."),
    e("empty-chain", Refusal, Admin, No, "The store serves no roster.", "Nothing can be verified.", ROSTER_BAD),
    e("too-many-epochs", Refusal, Admin, No, "The roster chain is longer than the limit.", "A flood guard.", ROSTER_BAD),
    e("bad-epoch", Refusal, Admin, No, "A roster file is not in the strict format.", "It is not accepted.", ROSTER_BAD),
    e("bad-signatures-file", Refusal, Admin, No, "A roster signatures file is not in the strict format.", "It is not accepted.", ROSTER_BAD),
    e("bad-pins", Refusal, You, No, "Your pinned roster state is damaged.", "This install cannot check the roster.", "Move `$DELPHI_HOME/xenia/<org>/<store>/state` aside and join again."),
    e("bad-request", Refusal, You, No, "The join details you typed are not valid.", "Nothing was written.", "Check the epoch:hash and fingerprint you were given and run `delphi xenia join` again."),
    e("wrong-scope", Refusal, Admin, No, "The file is for another org or store.", "A file from another store was put here.", FIX_AND_RETRY),
    e("epoch-gap", Refusal, Admin, No, "The roster skips an epoch number.", "A missing epoch hides a change.", ROSTER_BAD),
    e("epoch-fork", Refusal, Admin, No, "Two different roster files exist for one epoch number.", "Someone is showing different rosters to different people.", ROSTER_BAD),
    e("epoch-replay", Refusal, Admin, No, "An old roster file was served again under its own number.", "A replay of an old roster.", ROSTER_BAD),
    e("prev-hash-mismatch", Refusal, Admin, No, "A roster file does not follow from the one before it.", "The chain is broken.", ROSTER_BAD),
    e("genesis-prev-hash", Refusal, Admin, No, "The first roster file claims a predecessor.", "A first roster has none.", ROSTER_BAD),
    e("sigs-epoch-mismatch", Refusal, Admin, No, "A signatures file names another epoch.", "Signatures for one roster cannot approve another.", ROSTER_BAD),
    e("genesis-not-fully-signed", Refusal, Admin, No, "Not every founding admin signed the first roster.", "A founder who did not sign never agreed to it.", "Collect every founder's signature; `delphi xenia help genesis`."),
    e("quorum-not-met", Refusal, Admin, No, "Not enough admins (counted per person) have signed.", "A roster change needs a majority of the current admins.", "Ask the other admins to sign: `delphi xenia help being-an-admin`."),
    e("entry-dropped", Refusal, Admin, No, "A roster entry was removed from the file.", "Entries are never deleted; they are marked removed.", ROSTER_BAD),
    e("person-changed", Refusal, Admin, No, "A key's owner changed between epochs.", "A key must always belong to the same person.", ROSTER_BAD),
    e("status-regression", Refusal, Admin, No, "A retired or removed key came back.", "A returning person gets a new key.", ROSTER_BAD),
    e("immutable-field-changed", Refusal, Admin, No, "A field that must not change between epochs did.", "History would be rewritten.", ROSTER_BAD),
    e("bad-new-entry", Refusal, Admin, No, "A new roster entry is not a plain new active key.", "New entries cannot start retired or carry flags.", ROSTER_BAD),
    e("bad-successor", Refusal, Admin, No, "A rotation's successor key is not valid.", "A rotation must name a new, active key of the same person.", ROSTER_BAD),
    e("no-active-admin", Refusal, Admin, No, "The roster would have no active admin.", "Nobody could ever change it again.", "Add or keep at least one more admin first."),
    e("pin-mismatch", Refusal, Admin, No, "A roster file differs from the one pinned when you joined.", "Roster history was changed after you joined.", "Do not accept it. Tell an admin and read `delphi xenia help recovery`."),
    e("pinned-epoch-missing", Refusal, Admin, No, "The store's roster ends before the epoch you pinned.", "You are being shown an older or cut-off roster.", "Sync again later; if it persists, tell an admin."),
    e("admin-not-active", Refusal, You, No, "The admin you named at join is not an active admin of that roster.", "Joining would trust someone who has no standing.", "Ask for the admin's fingerprint again, over a channel that is not the store, and check the epoch."),
    e("already-pinned", Refusal, You, No, "This store is already joined.", "Nothing was changed.", "Use `delphi xenia status`, or `delphi xenia sync`."),
    e("io", Refusal, You, ForNow, "A file or git operation failed on this machine.", "Nothing was signed or sent.", "Check disk space and permissions on `$DELPHI_HOME/xenia`, then retry."),
    // ---- admin refusals -----------------------------------------------------------------------------------
    e("not-next-epoch", Refusal, Admin, No, "The roster file is not the next epoch number.", "Roster changes go in order.", SEE_STORE),
    e("stale-proposal", Refusal, Admin, No, "The roster moved on since this proposal was made.", "Signing it now would undo or clash with the newer roster.", "Make a new proposal on the current roster: `delphi xenia admin propose ...`."),
    e("last-admin", Refusal, Admin, No, "This would remove the last admin.", "The roster could never change again.", "Add another admin first."),
    e("below-minimum-admins", Refusal, Admin, No, "This would leave fewer admins than the local minimum (2).", "Two keys are the least that can still sign a change.", "Add an admin before removing one."),
    e("unknown-member", Refusal, Admin, No, "The member named is not in the roster.", "Nothing was changed.", FIX_AND_RETRY),
    e("already-listed", Refusal, Admin, No, "That key is already in the roster.", "Entries are never dropped; a returning person gets a new key.", FIX_AND_RETRY),
    e("person-exists", Refusal, Admin, No, "That person already has a key in the roster.", "A second key for a person is a rotation, not an addition.", "Use `delphi xenia admin rotate <member> --routine` (or `--compromised`)."),
    e("same-key", Refusal, Admin, No, "The new key is the same as the old one.", "A rotation must change the key.", FIX_AND_RETRY),
    e("bad-input", Refusal, Admin, No, "An input to the command is not valid.", "Nothing was changed.", FIX_AND_RETRY),
    e("bad-validity", Refusal, Admin, No, "The freshness validity is not allowed (zero, or over the 14-day cap).", "Longer validity would let a stale store look fresh.", "Use 7 days (the default) or any value up to 14."),
    e("store-not-usable", Refusal, You, No, "The store cannot be trusted right now (tampering, a refused fetch, not joined, or no trust base).", "Nothing was signed or sent.", "Run `delphi xenia status`, fix what it shows, then try again."),
    e("not-an-approver", Refusal, Admin, No, "This install's key is not an active admin, so it cannot sign this.", "Only admins approve roster changes and freshness statements.", "Ask an admin to do it, or have an admin add your key."),
    e("proposal-not-found", Refusal, Admin, No, "No roster proposal with that number exists.", "Nothing was changed.", "Run `delphi xenia admin propose ...` first, or check the number."),
    e("proposal-exists", Refusal, Admin, No, "A proposal with that number is already in the store.", "Proposals are never overwritten.", "Sign or publish the existing one, or ask an admin to clear the branch."),
    e("proposal-changed", Refusal, Admin, No, "The proposal changed after you reviewed it.", "You would have signed something you did not read.", "Review it again and sign."),
    e("flags-not-acknowledged", Refusal, You, No, "The preview raised flags and the approval did not say you read them.", "Flags mark things worth a second look (links, long random strings, new admins).", "Read the flags on the card and answer again acknowledging them."),
    e("denied", Refusal, You, Yes, "You said no.", "Nothing was signed or sent.", NOTHING_TO_DO),
    e("terminal-only", Refusal, You, No, "The console will not sign this: it touches an admin, rotates a key, starts a store or accepts a rewritten history.", "Those decisions decide who can speak for the whole org, so they are signed only where you type the code yourself.", "Run the terminal command the message names."),
    e("switch-off", Refusal, You, No, "Console admin signing is switched off (the default).", "Roster signing from the console is something you opt in to once.", "Set `[xenia] console_admin = true` in operator/xenia.toml, or run the command in a terminal."),
    e("pending-card", Refusal, You, ForNow, "An admin card is already waiting for your answer.", "Only one admin card is open at a time so none is signed by mistake.", "Answer or deny the waiting card, or wait for it to expire, then ask again."),
    e("rate-limit", Refusal, You, ForNow, "Five admin signatures were made from the console in the last hour.", "A limit keeps a hijacked console from signing a run of roster changes.", "Wait an hour, or sign in a terminal."),
    e("not-interactive", Refusal, You, No, "Nothing at a terminal answered: signing needs a person to type the code.", "A script, a pipe or an environment variable must never be able to sign for you.", "Run the command in a terminal and type the code when asked."),
    e("challenge-failed", Refusal, You, Yes, "What was typed did not match the code for these exact bytes.", "Nothing was signed.", "Run the command again and type the code shown."),
    e("flag-not-acknowledged", Refusal, You, Yes, "A flag on the card was not acknowledged by typing its name.", "Flags do not block, but signing means you read them.", "Run the command again and type each flag's name."),
    e("wrong-confirm", Refusal, You, Yes, "The proposal number (or `refresh`) was not retyped correctly.", "Retyping it shows you are signing this proposal and not another.", "Ask for the card again and retype the number shown."),
    e("approval-mismatch", Refusal, You, No, "The approval was for other bytes than the ones held.", "A mismatch can mean the card was swapped. Nothing was signed.", "Start the action again and read the new card."),
    e("wrong-key", Refusal, You, No, "The key given is not the one the action was prepared for.", "Nothing was signed.", "Start the action again."),
    e("history-failed", Refusal, Admin, No, "The history check found a problem, so no freshness statement was signed.", "Signing over tampered history would vouch for it.", "Run `delphi xenia status` and `delphi xenia help recovery`. Do not refresh until it is explained."),
    e("verify-failed", Refusal, You, No, "The result failed verification before it was sent.", "Nothing was sent.", "Run `delphi xenia sync`, then try again; if it repeats, tell an admin."),
    e("clock-behind", Refusal, Admin, No, "This machine's clock is not ahead of the last statement it accepted.", "The new statement would look like a replay and every install would ignore it.", "Fix this machine's date and time, then retry."),
    e("store-not-empty", Refusal, Admin, No, "The store already has content, so a first roster cannot be written.", "Genesis is only for an empty store.", "Use an empty repository, or join the existing store."),
    e("missing-founder-signatures", Refusal, Admin, No, "Not every founding admin has signed the first roster.", "Every founder must agree to genesis.", "Collect the missing signatures; `delphi xenia help genesis`."),
    e("push-rejected", Refusal, You, No, "The store host refused the push (permissions, a hook, a size limit).", "Nothing local was changed.", "Check you have push rights to the store and read the host's message."),
    e("too-many-races", Refusal, You, ForNow, "The store kept changing while this was being sent.", "Nothing was lost.", "Try again in a moment."),
    // ---- publish / endorse refusals -----------------------------------------------------------------------
    e("not-promoted", Refusal, You, No, "The content did not come from team or workspace scope.", "Operator scope never leaves your machine.", "Move the file under `team/` (or the workspace) first, then publish with `--promoted`."),
    e("wrong-origin", Refusal, You, No, "The record has the wrong kind of origin for this command.", "Provenance must be honest.", FIX_AND_RETRY),
    e("scan-blocked", Refusal, You, No, "The secret and path scan found something in the content.", "A published secret cannot be un-leaked.", "Remove the secret or private path (and rotate the secret if it was real), then publish again."),
    e("source-changed", Refusal, You, No, "The file changed after you previewed it.", "You must sign what you read.", "Run the command again to preview the new bytes."),
    e("rev-regression", Refusal, Member, No, "The new revision number is not higher than the stored one.", "Revisions only go up.", "Run `delphi xenia sync` and try again."),
    e("id-taken", Refusal, Member, No, "That id already exists in the store as something else.", "Ids are unique.", "Use a new id."),
    e("not-owner", Refusal, Member, No, "The record belongs to another member.", "Only the author (or their successor key) can revise it.", "Publish your own record, or ask the author."),
    e("state-regression", Refusal, Member, No, "A withdrawn or superseded record cannot come back.", "Hidden content must stay hidden.", "Publish a new record instead."),
    e("raced", Refusal, You, No, "Someone else published the same id while this was being sent.", "Two writers for one id is a conflict.", "Run `delphi xenia sync`, look at the store, and decide."),
    e("remote-rewrote-history", Refusal, Admin, No, "The store's history is not a fast-forward of what you accepted.", "It was rewritten or reverted. Nothing was sent.", "Do not retry. See `delphi xenia help recovery`."),
    e("rule", Refusal, You, No, "A rule the command checks before signing was broken (for example, endorsing your own record).", "Checking locally saves you a signature that nobody would count.", FIX_AND_RETRY),
    e("unrepresentable", Refusal, You, No, "A one-line field contains a line break or control character.", "The format has no way to carry it.", "Edit the field to plain text on one line."),
    e("invalid", Refusal, You, No, "The record would be refused by every install.", "Publishing it would be pointless.", "Fix the record; the message names the first broken rule."),
    e("not-canonical", Refusal, You, No, "The record did not write out the same way it was read.", "A bug in this tool, caught before anything was sent.", "Report this to the Delphi maintainers with the command you ran."),
    e("too-large", Refusal, You, No, "The record is larger than allowed.", "Large records are refused everywhere.", "Shorten the body or split it."),
    // ---- recovery -----------------------------------------------------------------------------------------
    e("nothing-to-recover", Recovery, Nobody, Yes, "The store's history was not rewritten.", "Nothing to recover from.", NOTHING_TO_DO),
    e("host-unavailable", Recovery, You, ForNow, "The store host cannot be reached.", "Recovery needs to look at the host's history.", "Check the network and retry."),
    e("fetch-refused", Recovery, Admin, No, "The fetched store failed a safety rule.", "It cannot be adopted.", "Ask an admin; `delphi xenia status`."),
    e("tip-refused", Recovery, Admin, No, "The rewritten head failed verification.", "It cannot be adopted.", "Ask an admin; `delphi xenia status`."),
    e("statement-names-no-rewrite", Recovery, Admin, No, "The latest statement does not say which head it replaces.", "Without that, a rewrite cannot be approved.", "Ask a majority of admins to sign a statement naming the replaced head."),
    e("statement-not-current", Recovery, Admin, No, "The statement approving the rewrite is not current.", "An expired approval cannot be used.", "Ask admins for a fresh statement."),
    e("rewrite-of-is-not-our-head", Recovery, Admin, No, "The statement replaces a different head than the one you hold.", "It was written for another state.", "Ask admins which head they replaced."),
    e("rewrite-not-applicable", Recovery, Admin, No, "There is no rewrite to author: the host serves an ordinary update, an older view or something that cannot be judged.", "A rewrite statement for a store that was not rewritten would ask everyone to drop good history.", "Run `delphi xenia sync`; if it is refused as a force-push, check `delphi xenia status` and try again."),
    e("host-is-behind", Recovery, Admin, No, "The host serves an older view than the one this install holds.", "An older tree cannot hold a statement about a later head, so it cannot be recovered by a statement.", "Ask the host's owner to restore the newer history, or wait."),
    e("rewrite-proposal-not-found", Recovery, Admin, No, "The host has no rewrite proposal with that id.", "It was mistyped, or it was never pushed.", "Run `delphi xenia admin rewrite propose` and use the id it prints."),
    e("rewrite-proposal-exists", Recovery, Admin, Yes, "That exact proposal is already on the host.", "The same statement, written at the same time, was proposed before.", "Use `delphi xenia admin rewrite show <id>`, or propose again a minute later."),
    e("rewrite-head-moved", Recovery, Admin, No, "The host no longer serves the head this proposal names, or the proposal is already published.", "A statement is only good for the head it names.", "Run `delphi xenia admin rewrite propose` again."),
    e("rewrite-statement-refused", Recovery, Admin, No, "The proposed statement is malformed, for another store, expired, or does not match its id.", "Signing it would put your key behind something that installs would ignore or that someone altered.", "Do not sign it. Tell the admin who proposed it, and propose a fresh one."),
    e("walk-violations", Recovery, Admin, No, "Even after the rewrite, the history check found tampering.", "The rewrite is not clean.", "Do not accept. Tell the admins."),
    e("host-head-moved-since-approval", Recovery, You, No, "The store changed after you approved the list of files.", "You approved a list that is no longer the whole story.", "Start recovery again and read the new list."),
    e("approval-names-unlisted-path", Recovery, You, No, "The approval names a file that was not on the list.", "Only files you were shown can be replaced.", "Start recovery again."),
    e("unapproved-sticky-violation", Recovery, You, No, "The new head would change a file you did not approve.", "Recovery replaces only the files you reviewed.", "Review the list and approve every path, or refuse."),
    e("creator-changed", Recovery, Admin, No, "The rewrite changes who created a record.", "That is a takeover of someone else's record.", "Read the path list carefully and only approve if admins confirm."),
];

/// The entry for a slug, if there is one.
pub fn lookup(slug: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|x| x.slug == slug)
}

/// Fragments the library's own messages contain when they carry no slug (`Display` of a publish or admin error), mapped
/// to the slug, so a pasted refusal can be explained. Lowercase, matched against the lowercase pasted text.
pub static PHRASES: &[(&str, &str)] = &[
    ("not for the bytes held", "approval-mismatch"),
    ("the approval did not acknowledge them", "flags-not-acknowledged"),
    ("changed after the preview", "source-changed"),
    ("not the key the batch was prepared for", "wrong-key"),
    ("is not an active member of the roster", "signer-not-active"),
    ("belongs to another member", "not-owner"),
    ("secret and path scan found", "scan-blocked"),
    ("only content the operator promoted", "not-promoted"),
    ("cannot be trusted right now", "store-not-usable"),
    ("is not a fast-forward of what this install accepted", "remote-rewrote-history"),
    ("failed verification", "verify-failed"),
    ("the store refused the push", "push-rejected"),
    ("the store kept moving", "too-many-races"),
    ("the store could not be reached", "unavailable"),
    ("the operator declined", "denied"),
    ("while this was being sent", "raced"),
    ("this proposal is stale", "stale-proposal"),
    ("required admins have signed", "quorum-not-met"),
    ("founder(s) have not signed", "missing-founder-signatures"),
    ("already exists in the store as something else", "id-taken"),
    ("would not advance it", "rev-regression"),
];

/// Find the entry a user means by a slug, or by pasted refusal text. Only catalog entries are returned; the pasted text
/// is never echoed.
pub fn find(input: &str) -> Option<&'static Entry> {
    let t = input.trim();
    if t.is_empty() || t.len() > 2000 {
        return None;
    }
    let low = t.to_ascii_lowercase();
    if let Some(x) = lookup(low.trim_matches(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))) {
        return Some(x);
    }
    // A message the library prints without a slug (`refused: the approval is not for ...`).
    if let Some(x) = PHRASES.iter().find(|(p, _)| low.contains(p)).and_then(|(_, s)| lookup(s)) {
        return Some(x);
    }
    // The first word that is a slug: the library prints `slug: detail`.
    low.split(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).filter(|w| w.len() >= 2).find_map(lookup)
}

/// The page for one entry (terminal and console).
pub fn render_entry(x: &Entry) -> String {
    format!(
        "{}  ({})\n  What it means:   {}\n  Why it matters:  {}\n  What to do:      {}\n  Who to ask:      {}\n  Safe to ignore:  {}\n",
        x.slug,
        x.kind.word(),
        x.meaning,
        x.why,
        x.next,
        x.who.text(),
        x.ignore.text()
    )
}

/// `delphi xenia explain <slug-or-text>`.
pub fn explain(input: &str) -> String {
    match find(input) {
        Some(x) => render_entry(x),
        None => "That is not a condition Xenia knows by name. Run `delphi xenia status` to see the slugs it shows, or `delphi xenia help troubleshooting` for the common problems.\n".to_string(),
    }
}

/// `explain` as JSON.
pub fn entry_json(x: &Entry) -> serde_json::Value {
    serde_json::json!({
        "slug": x.slug, "kind": x.kind.word(), "meaning": x.meaning, "why": x.why, "next": x.next,
        "who": x.who.text(), "safe_to_ignore": x.ignore.text(),
    })
}

/// The two lines `status`, `doctor` and the console put under a condition: what to do, and who can.
pub fn what_to_do(slug: &str) -> Option<String> {
    lookup(slug).map(|x| format!("what to do: {} (ask: {})", x.next, x.who.text()))
}

/// A short, secret-free plain-text message about one condition, for the relay, email and the Narrator channel.
///
/// It carries the slug, the meaning, the next step and the command to run on the machine, and nothing else: no store
/// name, URL, key, DID, fingerprint, record text or path (there is no argument it could come from). A slug that is not
/// in the catalog gets a fixed generic message that does not repeat it. **The message cannot approve, sign or run
/// anything**: it says so, and a relay holds no Xenia method (the daemon refuses every `xenia.*` call on that socket).
pub fn relay_message(slug: &str) -> String {
    const FOOT: &str = "Run this on the machine where Delphi is installed; replying to this message cannot approve, sign or change anything.";
    match lookup(slug) {
        Some(x) => format!(
            "Xenia: {} [{}]\nWhy it matters: {}\nWhat to do: {}\nWho can fix it: {}\nFor the full explanation run: delphi xenia explain {}\n{}",
            x.meaning, x.slug, x.why, x.next, x.who.text(), x.slug, FOOT
        ),
        None => format!("Xenia needs attention.\nWhat to do: run `delphi xenia status` and `delphi xenia help troubleshooting`.\n{FOOT}"),
    }
}

/// The text `xenia_help` (the agent tool) returns for a topic or slug. Fixed catalog text only.
pub fn tool_answer(query: &str) -> String {
    let q = query.trim();
    if q.is_empty() || q.len() > 200 {
        return "give a topic (see below) or a condition slug such as `expired` or `tampering`.\n\n".to_string() + &topic_list();
    }
    if let Some(t) = topic(q) {
        return format!("{}\n\n{}", t.title, t.body);
    }
    match lookup(&q.to_ascii_lowercase()) {
        Some(x) => render_entry(x),
        None => format!("That is not a Xenia topic or condition slug.\n\n{}", topic_list()),
    }
}

pub fn topic(name: &str) -> Option<&'static Topic> {
    let n = name.trim().to_ascii_lowercase();
    TOPICS.iter().find(|t| t.name == n)
}

pub fn topic_list() -> String {
    let mut s = String::from("Topics:\n");
    for t in TOPICS {
        s.push_str(&format!("  {:<17} {}\n", t.name, t.title));
    }
    s
}

/// `delphi xenia help` with no topic: a short orientation.
pub fn orientation() -> String {
    format!("{}\n{}", topics::ORIENTATION, topic_list())
}

/// The operator guide, generated from the topics and the catalog. `docs/guides/xenia-operator.md` must equal this.
pub fn guide_markdown() -> String {
    let mut o = String::from(topics::GUIDE_HEAD);
    o.push_str("\n## What Xenia is\n\n");
    o.push_str(topics::ORIENTATION.trim_end());
    o.push_str("\n\n");
    for t in TOPICS {
        o.push_str(&format!("## {}\n\n{}\n\n", t.title, as_markdown(t.body)));
    }
    o.push_str("## Every condition, by slug\n\nThe table is the catalog `delphi xenia explain <slug>` reads. \"Who\" is who can fix it.\n\n| Slug | What it means | What to do | Who |\n| :-- | :-- | :-- | :-- |\n");
    for x in CATALOG {
        o.push_str(&format!("| `{}` | {} | {} | {} |\n", x.slug, md_cell(x.meaning), md_cell(x.next), md_cell(x.who.text())));
    }
    o.push('\n');
    o
}

/// Indented runs (commands, tables, numbered steps) become fenced blocks; other lines are paragraphs.
fn as_markdown(body: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in body.trim_end().lines() {
        let indented = line.starts_with(' ');
        if indented && !in_code {
            out.push_str("```text\n");
            in_code = true;
        } else if !indented && in_code && !line.is_empty() {
            out.push_str("```\n\n");
            in_code = false;
        }
        out.push_str(line);
        out.push('\n');
        if !indented && !in_code && !line.is_empty() {
            out.push('\n');
        }
    }
    if in_code {
        out.push_str("```\n");
    }
    out.trim_end().to_string()
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

#[cfg(test)]
#[path = "xenia_help_tests.rs"]
mod tests;
