//! `delphi`: roadmap Step 1 commands (`specs/001`, `specs/006`).

mod avatar;
mod bundle_add;
mod bundle_cmd;
mod bundle_compat;
mod bundle_connect;
mod bundle_init;
mod bundle_licence;
mod bundle_fetch;
mod bundle_life;
mod bundle_pack;
mod bundle_sources;
mod bundle_trust;
mod bundle_xenia;
mod bundle_report;
mod check;
mod chat;
mod console;
mod decisions;
mod docs_cmd;
mod doctor;
mod escalation;
// Moved to the engine so the daemon's `check` tool and the CLI share one implementation.
mod frontmatter {
    pub use delphi_engine::frontmatter::*;
}
mod home;
mod host_caps;
mod hooks_cmd;
mod lint;
mod logs_cmd;
mod missions;
mod changelog;
mod customize_cmd;
mod learn_cmd;
mod retro_cmd;
mod train_run;
mod train_arms;
mod train_hidden;
mod train_judge;
mod train_report;
mod train_review;
mod train_truth;
mod sessions_cmd;
mod toolreport;
mod needs;
mod xenia_admin_cmd;
mod xenia_recover;
mod xenia_terminal;
mod xenia_cmd;
mod observer;
mod ratify_console;
mod relay;
mod release;
mod report;
mod run;
mod seed;
mod selfupdate;
mod setup;
mod skills_cmd;
mod claims_cmd;
mod pages_cmd;
mod sources_cmd;
mod start;
mod status;
mod theme;
mod timers_cmd;
mod train;
mod update;
mod validation_audit_cmd;
mod validation_cmd;

use std::path::PathBuf;
use std::process::exit;

/// One command: where it is listed, the sentence beside it, its option lines, and what only `help <command>` says.
struct Cmd {
    group: &'static str,
    /// The word after `delphi`; empty for bare `delphi`.
    name: &'static str,
    about: &'static str,
    synopsis: &'static str,
    detail: &'static str,
}

/// The one table behind `delphi --help`, `delphi help <command>` and the unknown-command message.
const CMDS: &[Cmd] = &[
    Cmd { group: "Start", name: "", about: "set up whatever is missing, then open the console", synopsis: "  delphi [--no-onboard] [--plain] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Start", name: "chat", about: "the console, on today's mission or one you name", synopsis: "  delphi chat [--plain] [--inline] [--no-mouse] [--workspace <name>] [--mission <id>] [--home <dir>]", detail: "`delphi missions` lists the missions you can reopen." },
    Cmd { group: "Start", name: "run", about: "one task without a terminal, then a summary", synopsis: "  delphi run \"<task>\" [--on-card deny|policy:<file>] [--permissions <mode>] [--budget <usd>] [--timeout <dur>]\n    [--json|--jsonl] [--mission <id>] [--keep-daemon] [--workspace <name>] [--home <dir>]", detail: "Exit codes: 0 ok · 1 gates failed · 2 a card was denied · 3 budget · 4 infrastructure ·\n5 an optimistic build awaits decision review." },
    Cmd { group: "Set up", name: "setup", about: "find the model tools you have and connect them", synopsis: "  delphi setup [--yes] [--check] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "init", about: "create a workspace for a project", synopsis: "  delphi init [--non-interactive] [--workspace <name>] [--project <path>]... [--theme <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "auth", about: "sign in to, or out of, a model endpoint", synopsis: "  delphi auth login <name> [--manual] | status | logout <name> [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "endpoints", about: "check that each model endpoint answers", synopsis: "  delphi endpoints test [<endpoint id>] [--probe] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "connectors", about: "the connectors that are configured, logged in and pinned", synopsis: "  delphi connectors status | test <name> | pin <name> [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "relay", about: "send approval cards to your email or Telegram", synopsis: "  delphi relay [check|totp-enroll|totp-check <code>] [--workspace <name>] [--home <dir>]", detail: "Takes your replies too. Run it in a terminal or a tmux pane." },
    Cmd { group: "Set up", name: "skills", about: "agentskills.io skills in and out", synopsis: "  delphi skills check <dir> | export <name> [--out <dir>] | import <dir> --agents <role,role>\n    [--description \"…\"] [--workspace <name>] [--home <dir>]", detail: "An import is a pending proposal until you /ratify it." },
    Cmd { group: "Set up", name: "bundle", about: "check an org bundle: knowledge, skills and behavior to share", synopsis: "  delphi bundle init <dir> --org <org> --name <name>\n  delphi bundle validate <dir> [--claude] [--against <previous dir>] [--package] [--report <file>] [--workspace <name>]\n  delphi bundle distill [<path|missions/id>...] [--workspace <name>]\n  delphi bundle import <plugin dir> [-o <new dir>] [--as <name>] [--org <org>] [--agents <role,role>] [--agent-role <agent>=<role>]\n  delphi bundle export <name> --claude-plugin <new dir> [--strip-delphi] | --agents-md [<file>] [--workspace <name>]\n  delphi bundle export <name>... --marketplace <dir> --repo <owner/repo|url> --sha <commit> [--ref R] [--subdir D]\n  delphi bundle manifest <dir> [--claude]\n  delphi bundle sign <dir> [--against <previous dir>] [--home <dir>]\n  delphi bundle package <dir> [-o <file.xbundle>] [--vectors]\n  delphi bundle add <dir or file.xbundle> | --from-marketplace <repo> <name> [--order N] [--replace-publisher] [--no-vectors] [--yes] [--dry-run] [--trust ratified] [--trust-source] [--workspace <name>] [--home <dir>]\n  delphi bundle add <record id> --from-xenia <org>/<store> [--order N] [--replace-publisher] [--no-vectors] [--yes] [--dry-run] [--workspace <name>] [--home <dir>]\n  delphi bundle store <org>/<store> [--auto-update] [--workspace <name>] [--home <dir>]\n  delphi bundle publish <dir> --to <org>/<store> --url <https git url> --pin <git-tree|git-commit>:<sha> [--subdir D] [--id <record>] [--tags a,b] [--review-by YYYY-MM-DD] [--no-verify-tree] [--home <dir>]\n  delphi bundle sources add <dir> <source-id>... [--replace] [--home <dir>]   (copy library sources into the bundle's sources/, with their licence record)\n  delphi bundle trust <bundle> --level user_ratified|default [--workspace <name>] [--home <dir>]\n  delphi bundle trust-source <bundle|origin> [--level user_ratified|default] [--revoke] [--workspace <name>] [--home <dir>]\n  delphi bundle list | remove <name> | update [name] | rollback <name> [--to <hash>] | gc [--workspace <name>] [--home <dir>]\n  delphi bundle serve [--bundle <org/name>]... [--workspace <name>] [--home <dir>]", detail: "init writes a skeleton that validates at once. validate reads the directory and changes nothing; --report writes the redaction report (every\nsecret, path, address and name the scan found, masked) to a file outside the bundle. distill has the writer role draft knowledge from\nallow-listed sources (team context and specs, mission decisions and Council verdicts) into a staging directory of the mission; it writes\nnothing into a bundle. package --vectors embeds the knowledge with the installed model into vectors/. manifest writes .delphi-bundle/files.json (and with --claude\n.claude-plugin/plugin.json) and prints the bundle hash. sign (a terminal only) signs the manifest with this install's xenia key; package writes a reproducible .xbundle archive. A bundle may carry sources under sources/: bundle.toml `distribution` (`internal` by default, or `public`) says where it is meant to go, a source licence only warns (source-licence, source-attribution, source-sharealike; only `private: true` refuses), and packaging a `public` bundle that carries sources asks you once, in a terminal, to type `publish` after showing each source id and its licence class; the answer is recorded in .delphi-bundle/report.json (licence_acknowledged). An org that adds a bundle vouches for its sources: they are shown `validated · vouched by <org>` only while that bundle entry is in the lock, as data and never instructions.\nEvery bundle document is a validated bundle source: agents and the Council may cite it, with its bundle and provenance level; it stays data, never instructions, and grants no permission. trust raises ONE bundle's knowledge to user_ratified for its current hash and publisher (`add --trust ratified` does it at install): it needs a terminal and you typing the bundle name (no --yes, no environment switch), and it resets when an update changes the hash or publisher, on rollback and on remove; --level default lowers it at any time. trust-source (`add --trust-source`) deems a source trusted: the same origin AND publisher key (or a roster successor of it), so later updates keep the tier you chose without a prompt while every check still runs; it needs the same typed confirmation (the origin), and --revoke ends it. A bundle can set neither.\nadd takes a directory or an archive, verifies any signature, prints what it brings, copies it into the cache and makes its\nknowledge searchable (`delphi kb search --bundle org/name`); a new publisher key needs --replace-publisher. update re-adds from\nthe recorded source under that rule and prints the changelog; rollback re-points at an earlier cached version; gc prunes the\ncache. serve is a read-only MCP server on stdio for the added bundles' knowledge (resources, and the tools search and get_doc); it writes nothing, opens no port and runs nothing. No model is called to add. Vectors it ships are used only if they match this install's embedder and pass a local spot check\n(--no-vectors skips them); the rest are embedded in the background, and `list` shows how far along they are.\nimport turns a Claude Code plugin into a new bundle directory (it never adds it, and never imports hooks/ or bin/); export writes an added\nbundle as a plugin (--claude-plugin), an AGENTS.md fragment, or a marketplace.json; add --from-marketplace fetches the entry's pinned commit.\nA bundle can also travel through a xenia store (the org's shared, signed, git-hosted store you joined with `delphi xenia`). `publish` writes a `bundle` record from the signed bundle's own manifest, pointing at the git tree you name, and asks you once, showing the exact bytes. `add --from-xenia` verifies the store, fetches that tree under the same limits as a marketplace add, requires it to hash to the record and to be signed by the member who published it, and records the store's standing: `endorsed` (a steward of another person endorsed these exact bytes, under a current freshness statement) or `member-signed`. `update` follows the record's newer revisions, forward only. `store` lists the store's bundles; with --auto-update it also keeps the levels in the lock current and applies the updates that cannot lower the level or change the publisher.\nsources add copies sources from your library into <dir>/sources/ (and turns the section on in bundle.toml). It writes meta.json from the library record (citation fields and the licence record, `internal` when there is none; no status, origin, reliance count or note), source.md when the capture is whole, the excerpts without your purpose notes, and recognised assets. Licence is information, not a gate: nothing is refused for it. The one refusal is a source you flagged private, which never travels in a bundle; the command is also refused inside an agent seat. It refreshes files.json when the bundle has one (sign again if it was signed) and lists the source-licence, source-attribution and source-sharealike warnings `validate` would report. For a public bundle, `package` and `publish` print the sources that carry a licence note and ask you to confirm in a terminal (no --yes); for an internal bundle `publish` asks. The acknowledged ids are written to .delphi-bundle/report.json.\nExit codes: 0 pass · 1 failed · 2 usage." },
    Cmd { group: "Set up", name: "hooks", about: "run your observe hooks on events", synopsis: "  delphi hooks [--workspace <name>] [--home <dir>]", detail: "Run it in a terminal or a tmux pane. The hooks are in the ratified [hooks] config." },
    Cmd { group: "Work", name: "decisions", about: "review an optimistic build's provisional decisions", synopsis: "  delphi decisions [list|show <id>|ratify <id…|all-routine>|reject <id> [why]] [--mission <id>]\n    [--workspace <name>] [--home <dir>]", detail: "This is the text form for a shell. In the console, `/decisions` with no argument opens the decisions pane:\nevery approval card, provisional decision and agent-raised decision you owe, one at a time." },
    Cmd { group: "Work", name: "timers", about: "the wakes the daemon is keeping for roles", synopsis: "  delphi timers [list | set <role> (--in <5m> | --at <time>) [--every <5m>] <note…> | cancel <id>]\n    [--mission <id>] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Work", name: "validation", about: "score the labelled set; audit, report on and retract validated facts", synopsis: "  delphi validation score [--set <file>] [--results <file>] [--home <dir>]\n  delphi validation audit [--card <id>] [--workspace <name>]\n  delphi validation report [--workspace <name>]\n  delphi validation latch raise --to advisory|ratify --reason \"<text>\" [--force] [--workspace <name>]\n  delphi validation retract --all [--workspace <name>]\n  delphi validation unquarantine <fact-id> --reason \"<text>\" [--workspace <name>]\n  delphi validation more <mission> --reason \"<text>\" [--workspace <name>]\n  delphi validation queue [--workspace <name>]", detail: "Operator-run; every subcommand refuses inside an agent seat, and `audit` needs a terminal. `score` reads the private set (default <home>/operator/validation/claims.json) and prints counts and rates only: claims scored, false-ratified, poisoned passes, wrongly-escalated rate, the rule-of-three bound and the claims new since the last scored run; it also keeps those counts for `report`. `audit` is the blind audit: it shows one claim from the validator's checks and nothing else, asks true, false or unsure, records your answer, and only then shows the sources and the verdict. A fact you call false trips the latch to advisory, withdraws PROMOTE and quarantines the facts of that batch; a would-ratify card left unanswered for seven days quarantines its fact. `report` prints the audit counts, the rule-of-three bound, the origin shares and the token use, then PROMOTE: yes or no with the conditions it missed, and writes promote.json; it never runs the validator. `latch raise` (or the console card, which asks you to type the word) is the only way up from a tripped latch: it needs a reason, starts a new audit batch, and for ratify needs PROMOTE: yes or --force, which is logged as forced. `retract --all` marks every validated fact superseded by its retraction (nothing is deleted; disproved facts are left as they are) and lists the missions recorded as having read one; it does not mark them stale. `unquarantine` returns one quarantined fact to the status it had (or contested), needs a reason and is journaled as yours; it cannot undo a fact you called false in an audit, and while a false audit holds the latch tripped it refuses until `latch raise`. At a mission's 15% share claims are kept as not validated and queued; `queue` lists them, most-referenced first, and `more` raises that mission's cap once (logged, yours alone). Format: docs/reference/validation-set.md." },
    Cmd { group: "Work", name: "claims", about: "the library's claims: statements anchored in sources, pulled by pointer", synopsis: "  delphi claims add <text> [--kind fact|episode|procedure|attributed] [--scope personal|project|team|org] [--project <name>]\n    [--valid-until YYYY-MM-DD] [--supersedes <claim>] [--origin <page>] [--source <id> --locator <where> --quote <passage>] [--home <dir>]\n  delphi claims anchor <claim> --source <id> --quote <passage> [--locator <where>] [--prefix <t>] [--suffix <t>] [--home <dir>]\n  delphi claims list [--json] [--home <dir>]\n  delphi claims show <claim> [--json] [--home <dir>]\n  delphi claims validate <claim> [--home <dir>]\n  delphi claims confirm <claim> [--home <dir>]\n  delphi claims review [--json] [--home <dir>]\n  delphi claims pull <words...> [--project <name>] [--json] [--home <dir>]", detail: "A claim is a short statement kept once per install in <home>/library/claims/, with anchors into the library's sources: the source, version, locator, the exact quote and a little text on each side. Its status is derived by the daemon and never typed: a new claim reads not validated, and only `validate` (you, in a terminal; refused inside an agent seat) records a validation, which counts only while an anchor still holds. An attributed claim (an opinion) is never validated as a fact. A source held whole is anchored by quote in its text; one that is not (excerpts, pointer) only inside an excerpt you kept, so a claim cannot quote more of a restricted work than the excerpt bounds allow. When a source gets a new version each anchor is looked up again by its quote, then by its surroundings: found moves to the new version, not found marks the claim contested and leaves the old anchor readable under versions/; nothing is re-pointed to different text. A source marked retired contests the claims it alone supports; `sources purge` turns every anchor into it pointer-only (the quote goes with the capture, the citation and quote hash stay) and a claim no other source supports then reads not validated. `sources show` lists how many of a source's claims stood, were contested or disproved. Every mission start pulls claims from this one library by pointer, chosen by the words of the prompt and the project, ranked by status, recency and how often they were relied on; disproved and retired claims are not offered, and a project-scoped claim goes only to its project. The agent gets the id, status label, origin and score, never the text; opening it is a tainted, read-only read, and a claim that rests on a private source is withheld. `pull` shows what a mission start would be handed and records nothing. The library records which missions were handed a claim (`cited_by`). `review` lists claims that may disagree (two live claims of one kind that say nearly the same thing in different words; a word-overlap match with no model, so it may be a false alarm, and neither is picked) and claims due for review (not confirmed for 90 days, or `--valid-until` within 14 days); both are read on demand and the daemon changes nothing. `confirm` says a claim still holds: it moves only the date and cannot raise a status. The same items appear in the daily digest. `add`, `anchor`, `validate` and `confirm` are refused inside an agent seat." },
    Cmd { group: "Work", name: "pages", about: "a readable page generated from the library's claims, and dispute from it", synopsis: "  delphi pages show <topic words...> [--draft <file>] [--style apa|mla|chicago|plain] [--home <dir>]\n  delphi pages check <file> [--home <dir>]\n  delphi pages dispute <claim> --note <what is wrong> [--home <dir>]\n  delphi pages search <question words...> [--internal | --external] [--json] [--home <dir>]\n  delphi pages list [--json] [--home <dir>]\n  delphi pages pin|unpin <key> [--home <dir>]", detail: "A page is generated, never a source, never edited and never trusted text (spec 057). The daemon writes the banner on top (\"Generated page. Not authoritative. ...\") and, at the bottom, a bibliography and citations table with one row per cited source or claim (or \"no sources cited\", marked unsupported in the banner). The body cites with [s-source-id] and [c-claim-id] markers after the sentences or paragraph they support; the glyphs ● validated, ○ not validated, ◆ contested, ✕ disproved are derived by the daemon. Without --draft the body is the library's claims, the unvalidated ones under a visible \"Noted, not validated\" heading. With --draft a file of body text someone proposed (a model, say), the daemon runs the mechanical check and rejects the page, listing each finding, exit 1, if a marker names a missing, retired, purged, disproved or (on an external page) private record or misstates its status; a quotation is not verbatim in a cited source's stored text (checked against the recorded hash) or a quote block cites no source; a table has no citation; an image is not an asset of a cited library source (written lib: then the source id, a slash and the file name); a link is in no cited source's record; or the text carries its own banner or table. Numbers, dates and proper names found in no cited material are not rejected but reported as unverified figures. `check` runs the same check on a whole page file (banner, body, table) and prints the findings. Pages are cached by a hash of the claims they were built from, so a changed or disputed claim rebuilds them. A claim resting on a private source is left off. `dispute` is your \"this is wrong\" on a sentence: it files a contested mark with your note on the cited claim, which then reads not validated until the contest is resolved, and is refused inside an agent seat. `search` finds what the library holds for the words and has the writer role compose a page from it, which the daemon then checks mechanically and the validator checks sentence by sentence; a page that fails twice is shown as the claims alone (exit 1). --internal never touches the web; --external also searches the engines in [search] and captures a few results as noted, not validated, sources (a query that names a private source is not sent, and a private source never reaches an external page). Without either, [library.pages] external decides (ask means internal unless you say --external). Pages are cached by the material's hashes: a changed source, an expired external page ([library.pages] external_ttl) or a new matching record marks the page stale and it is rebuilt on the next search; `pin` keeps a page out of eviction. A page costs a writer call and a validator call, counted against the validator's share of the week (spec 040 §20) and [library.pages] per_day; a cached page costs nothing. Exit codes: 0 ok · 1 rejected, failed or claims only · 2 usage · 4 infrastructure." },
    Cmd { group: "Work", name: "sources", about: "the library's sources: capture, excerpts, bibliography", synopsis: "  delphi sources add <url | file | doi> [--level full|excerpts|pointer] [--licence <id>] [--private]\n    [--title <t>] [--rights-holder <name>] [--attribution <text>] [--max-text-mib <n>] [--max-source-mib <n>] [--home <dir>]\n  delphi sources list [--json] [--home <dir>]\n  delphi sources show <id> [--json] [--offline] [--home <dir>]\n  delphi sources history <id> [--json] [--home <dir>]\n  delphi sources excerpt add <id> --locator <where> --purpose <why> (--text <passage> | --file <file>) [--prefix <t>] [--suffix <t>]\n    [--work-bytes <n>] [--home <dir>]\n  delphi sources excerpt list <id> [--json] [--home <dir>]\n  delphi sources bib [<id>...] [--format apa|mla|chicago|plain|csl-json|bibtex] [--home <dir>]\n  delphi sources purge <id> [--home <dir>]", detail: "One library per install, in <home>/library/. A URL is fetched by the validation fetcher (https only, no private addresses, size cap, cloaking check). The licence decides the level: open licences and your own material are kept whole, restricted ones as excerpts, an unknown one as a pointer; --private keeps a restricted source whole for your own use and it never leaves this install. Everything captured reads not validated. Limits: 64 MiB of text and 512 MiB per source, 20 GiB for the library; over a limit the capture falls back and says so. `purge` deletes the capture and keeps the citation and the content hash. Metadata is read by code, never guessed: Wikipedia (CC BY-SA, the revision from the ETag), a DOI's landing page (citation meta tags) and a Gutenberg header; a field the page does not state is left empty. `show` is an open: if the source was last checked more than 30 days ago it is fetched once through the same fetcher (no tokens; skipped offline or with --offline), an unchanged page only moves last_checked, a changed one becomes a new version with a change note and the old one kept, and a gone one (404/410) is marked retired. `excerpt add` keeps one quoted passage with its locator and purpose: at most 4 KiB each, and for a restricted source not held whole at most 10% of the work or 16 KiB in total (the work's length is recorded at capture, or give --work-bytes); each excerpt carries an anchor (quote, prefix and suffix, and a character range when the text is held) that is looked up again when the source changes, and one that cannot be found is marked contested, never re-pointed. `bib` writes a bibliography from the citation fields alone, in apa, mla, chicago or plain text, or as csl-json or bibtex for a reference manager; private sources are left out of every export. `history` lists every kept capture, oldest first, with its change note (sizes and the sections that came or went, no model) and how many claim anchors point at it; a purge deletes the earlier ones. `add`, `purge`, `excerpt add` and `bib` are refused inside an agent seat." },
    Cmd { group: "Work", name: "needs", about: "what the roadmap is waiting on evidence for", synopsis: "  delphi needs [add <item> <why>] [--workspace <name>] [--home <dir>]", detail: "After the roadmap items it prints the fact validator's open work, once it has run here: the blind-audit\ncards waiting (and the oldest's age) and the PROMOTE line from the last `delphi validation report`.\nIt reads files only and never runs the validator." },
    Cmd { group: "Work", name: "kb", about: "the knowledge base: index it, search it", synopsis: "  delphi kb index | reindex | stats | search <query> [--role <role>] [--untrusted] [--staged]\n  delphi kb serve [--bind <host:port>] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Work", name: "observe", about: "the read-only live window (also: delphi narrator)", synopsis: "  delphi observe [dashboard|council|epic|agents|timeline|changes]\n    [--once] [--workspace <name>] [--home <dir>]", detail: "--once prints one frame as text; it works without a terminal." },
    Cmd { group: "Inspect", name: "status", about: "workspace, daemon, models, health and mission at a glance", synopsis: "  delphi status [--workspace <name>] [--home <dir>]", detail: "Needs no terminal. Exits 1 only if `delphi doctor` finds a failure." },
    Cmd { group: "Inspect", name: "missions", about: "the workspace's missions, newest first", synopsis: "  delphi missions [--workspace <name>] [--home <dir>]", detail: "Reopen one with `delphi chat --mission <id>`." },
    Cmd { group: "Inspect", name: "changelog", about: "what the team recorded as finished, for a pull request", synopsis: "  delphi changelog [--mission <id>] [--session <s_0001>] [--since <date or time>] [--json]\n    [--workspace <name>] [--home <dir>]", detail: "Prints Markdown by session, from the mission's log (the latest unless --mission). --json: one line per change." },
    Cmd { group: "Inspect", name: "release", about: "whether this build is a signed release", synopsis: "  delphi release [status]", detail: "Signed by the maintainers' key, unsigned (a local or development build), or invalid. Needs no terminal." },
    Cmd { group: "Inspect", name: "sessions", about: "the finished console sessions, to review later", synopsis: "  delphi sessions [--mission <id>] [--limit <n>] [--full] [--json] [--home <dir>]", detail: "Written when a session ends (/new, or a daemon restart): the first words of what was asked, the changes recorded, and the summary when [session] summary is on. Reads the central log; calls no model." },
    Cmd { group: "Inspect", name: "tools", about: "how tools are used, and how often models record their changes", synopsis: "  delphi tools [--mission <id> | --all] [--json | --prom] [--workspace <name>] [--home <dir>]", detail: "Per model: runs that changed something and recorded a change. --prom: Prometheus text. Counts only." },
    Cmd { group: "Set up", name: "customize", about: "teach Delphi a note, a rule, a skill or a profile line yourself", synopsis: "  delphi customize note \"<text>\" [--mission <id>]\n  delphi customize rule \"<text>\" [--scope workspace|operator] [--why \"…\"] [--replace <ID> | --remove <ID>]\n  delphi customize skill <dir> --agents <role,role> [--description \"…\"]\n  delphi customize profile [\"<line>\" | --file <path>]\n    [--yes] [--workspace <name>] [--home <dir>]", detail: "What you write is yours: a rule, a skill or a profile line is ratified as you write it, after the same checks a proposal gets (no relaxing a rule, the caps, no near-duplicate). `note` only stages a note for review. Each verb needs a terminal; `--yes` skips the typed yes, not the terminal. A rule applies from the next task." },
    Cmd { group: "Inspect", name: "retro", about: "rules the Orchestrator proposed, waiting for your review", synopsis: "  delphi retro list [--mission <id>] [--json]\n  delphi retro show <mission>/<id> [--json]\n  delphi retro accept <mission>/<id> [--scope workspace|operator]\n  delphi retro reject <mission>/<id>\n    [--workspace <name>] [--home <dir>]", detail: "Proposals are made with the Orchestrator's `suggest_rule` tool and are never loaded into a prompt until you ratify them. `accept` and `reject` need a terminal and a typed yes, and `accept` needs the daemon stopped; `--scope` ratifies at a different scope than proposed." },
    Cmd { group: "Inspect", name: "learn", about: "notes staged from denials, corrections, retries and escalations", synopsis: "  delphi learn list [--mission <id>] [--json]\n  delphi learn show <mission>/<id> [--json]\n  delphi learn drop <mission>/<id>\n    [--workspace <name>] [--home <dir>]", detail: "Staged notes are never loaded into a prompt; they wait for review. `drop` needs the daemon stopped. Capture is set in [learning] in config.toml." },
    Cmd { group: "Inspect", name: "capabilities", about: "what each model CLI here can use: connectors, skills, models", synopsis: "  delphi capabilities [--refresh] [--ask] [--json] [--quiet] [--models] [--project <dir>]... [--home <dir>]", detail: "--models lists every discovered model with its class (spec 056; takes --workspace); one no longer\nlisted is kept as gone.\nAsks each CLI's own list commands and reads the skill directories; saves <home>/capabilities.json.\nSetup and update refresh it in the background. --ask also asks the claude and agy models what they can\nuse; that answer is shown apart and is not verified. Calls no model otherwise." },
    Cmd { group: "Inspect", name: "logs", about: "the error log: what went wrong, with context", synopsis: "  delphi logs [--lines <n>] [--since 1h] [--level error|warn|debug] [--component <name>] [--mission <id>] [--json] [--follow] [--path] [--home <dir>]", detail: "Reads $DELPHI_HOME/logs/delphi.log.jsonl, written by the CLI, the daemon and the console: errors and panics by default, redacted, 10 MB x 3 files.\nDELPHI_LOG=off|error|warn|debug or [logging] level in <home>/operator/config.toml sets what is kept. Read-only and offline. --path prints the file's location.\nComponents: cli, daemon, engine, harness, provider, ui, xenia, relay, tool." },
    Cmd { group: "Inspect", name: "debug", about: "pack a redacted slice of the log for sharing", synopsis: "  delphi debug bundle [--since 24h] [-o <file>] [--mission <id>] [--workspace <name>] [--home <dir>]", detail: "Writes one .tar.gz (mode 0600, never over an existing file): the log slice, the `delphi doctor` report, version and OS, the config's key paths with values\nredacted, and the last events of the mission (types and short status fields only; no prompts, outputs or file contents). Prints what it included. Read-only and offline." },
    Cmd { group: "Inspect", name: "doctor", about: "check the workspace, its files and its models", synopsis: "  delphi doctor [--fix] [--workspace <name>] [--home <dir>] [--require-providers]", detail: "--fix asks before each repair and needs a terminal." },
    Cmd { group: "Inspect", name: "models", about: "the model pool: usable, out of quota, cooling down", synopsis: "  delphi models [--probe] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Inspect", name: "daemon", about: "the background process: status, stop, restart", synopsis: "  delphi daemon status|stop|restart [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Inspect", name: "lint", about: "check a repository's Delphi files", synopsis: "  delphi lint --repo <path>", detail: "" },
    Cmd { group: "Inspect", name: "check", about: "the deterministic checks behind the built-in skills", synopsis: "  delphi check <dag|handoff|provenance|rules|review-md|spec|kb|tui|skill> <path> [--repo <path>]\n  delphi check fact <claim.json> [--home <dir>]\n  delphi check record <mission-dir> --event <seq> [--hash <h>]", detail: "`record` verifies one event in a mission log: its hash chain against the daemon's stored head (no model,\nno network). An agent-authored event only warns \"an agent wrote this\". `fact` reads {claim, needed_for, kind,\nsnapshots: [ids]} and checks the claim against snapshots the daemon stored: origins, independence, classes.\nIt makes no network call and runs no model; the judge and the contradiction search are the daemon's." },
    Cmd { group: "Maintain", name: "update", about: "the program, the built-in files, then the daemons", synopsis: "  delphi update [--check] [--version vX.Y.Z | --dev] [--rollback] [--review] [--no-restart] [--allow-downgrade]\n    [--workspace <name>] [--home <dir>]", detail: "--dev installs the latest build of main. --rollback undoes all three steps. --review leaves the release's\nchanges pending for /ratify instead of ratifying them." },
    Cmd { group: "Maintain", name: "train", about: "training mode: check, price and run cases that test Delphi itself", synopsis: "  delphi train validate [--suite <name>] [--repo <path>]\n  delphi train plan --budget-usd <n> [--suite <name>] [--split dev|holdout|all] [--seeds <n>] [--home <new dir>] [--repo <path>]\n  delphi train run --budget-usd <n> --template-home <dir> --work-dir <new dir> [--overlay <dir> | --arms-file <f>] [--case <id>] [--seeds <n>] [--resume] [--pace-window <0-1>] [--allow-sleep] [--dry-run]", detail: "For a source checkout (it needs dev/train/suites). validate reads every case and names what is wrong; plan lists the cases a run\nwould use and the most it could cost, and refuses a budget below that. Neither runs anything or calls a model.\nrun SPENDS MONEY: each case runs `delphi run` in a copy of the template home, baseline and (with --overlay) candidate, and prints pass rates.\nExit codes: 0 ok · 1 problems · 2 usage · 3 over budget." },
    Cmd { group: "Maintain", name: "xenia", about: "the org's shared, signed memory store: join, sync, status, publish, endorse", synopsis: "  delphi xenia whoami | help [topic] | explain <slug or message>   [--json]\n  delphi xenia join <url> --org <o> --store <s> --admin <fingerprint> --head <epoch>:<hash> [--allow-local]\n  delphi xenia sync [--auto-update-bundles] | status [--store <org>/<store>] [--json]\n  delphi xenia publish <file> --title <t> --summary <s> --promoted [--tags a,b] [--review-by <date>]\n  delphi xenia publish --bundle <dir> --url <https url> --pin <git-tree|git-commit>:<sha> [--id <record>]\n  delphi xenia endorse|dispute|retract|withdraw|restore <id> [--scope a,b] [--note <n>] [--reason <r>]\n  delphi xenia republish --old <did> | admin add|remove|rotate|show|sign|publish|refresh|genesis ...", detail: "Commands that sign show the exact bytes and want the card's code typed at a terminal. Start: `delphi xenia help`." },
    Cmd { group: "Maintain", name: "docs", about: "print the command reference or a man page", synopsis: "  delphi docs cli | man | catalog [--repo <dir>] | coverage", detail: "Generated from the table behind --help, so it is never out of date. `cli` is Markdown (it is docs/reference/cli.md); `man` is roff; `catalog` lists the built-in roles and skills from their manifests (docs/reference/catalog.md), read from a checkout. `coverage` is Markdown (docs/reference/harness-coverage.md): the disposition of every flag and command of the claude, agy and codex CLIs, from the manifests Delphi checks against each CLI\'s --help (specs/046)." },
    Cmd { group: "Maintain", name: "completions", about: "print a shell completion script", synopsis: "  delphi completions bash|zsh|fish", detail: "For example: `delphi completions zsh > ~/.zfunc/_delphi`." },
    Cmd { group: "Maintain", name: "help", about: "this list, or one command's options", synopsis: "  delphi help [<command>]", detail: "`delphi <command> --help` says the same." },
];

const ENV: &[(&str, &str)] = &[
    ("DELPHI_HOME", "where Delphi keeps its files (default ~/.delphi); --home overrides it"),
    ("DELPHI_WORKSPACE", "the workspace to use when --workspace is not given"),
    ("DELPHI_UI", "the delphi-ui program to use (default: next to delphi)"),
    ("DELPHI_THEME", "dark or light, when the terminal cannot say"),
    ("NO_COLOR", "no colour at all; TERM=dumb also drops styling"),
    ("DELPHI_REDUCE_MOTION", "1 stops every console animation (also /reduce-motion)"),
    ("DELPHI_NO_ONBOARD", "1 skips the first-run questions"),
    ("DELPHI_LOG", "off, error (default), warn or debug: what `delphi logs` keeps"),
];

/// `delphi --help`: the commands by purpose, one sentence each, then the environment.
fn usage() -> String {
    let mut out = String::from("usage: delphi [command] [options]\n");
    let mut group = "";
    for c in CMDS {
        if c.group != group {
            group = c.group;
            out.push_str(&format!("\n{}\n", group));
        }
        out.push_str(&format!("  {:<22}{}\n", format!("delphi {}", c.name).trim_end(), c.about));
    }
    out.push_str("\n`delphi help <command>` (or `<command> --help`) shows a command's options.\n\nEnvironment\n");
    for (k, v) in ENV {
        out.push_str(&format!("  {:<22}{}\n", k, v));
    }
    out.push_str("\n  delphi --version      the release of this program");
    out
}

/// One command's help: its option lines, the sentence, and what only this page says.
fn usage_of(cmd: &str) -> Option<String> {
    let c = CMDS.iter().find(|c| !c.name.is_empty() && c.name == cmd)?;
    let mut out = format!("usage:\n{}\n\n{}.", c.synopsis, c.about[..1].to_uppercase() + &c.about[1..]);
    if !c.detail.is_empty() {
        out.push_str(&format!("\n{}", c.detail));
    }
    Some(out)
}

/// Flags that take a value, so the word after one is not the command (`delphi --home X doctor`).
const VALUE_FLAGS: &[&str] = &["--home", "--workspace"];

/// The first word that is not a flag or a flag's value: the command, wherever the flags put it.
fn command_index(args: &[String]) -> Option<usize> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if VALUE_FLAGS.contains(&a) {
            i += 2;
        } else if a.starts_with('-') && a != "-" {
            i += 1;
        } else {
            return Some(i);
        }
    }
    None
}


/// Help is answered before anything runs: it never creates a workspace, starts a daemon or asks a question.
/// Returns the exit code when `args` asked for help.
fn help_requested(args: &[String]) -> Option<i32> {
    let ci = command_index(args);
    if let Some(i) = ci.filter(|&i| args[i] == "help") {
        return Some(print_help(args.get(i + 1).filter(|s| !s.starts_with('-')).map(|s| s.as_str())));
    }
    // Only words before the command's own arguments count, so `delphi run "fix -h"` still runs.
    let before = &args[..ci.unwrap_or(args.len())];
    let after = &args[ci.map_or(args.len(), |i| i + 1)..];
    let asked = |a: &String| a == "--help" || a == "-h";
    let free_text = ci.is_some_and(|i| args[i] == "run");
    if before.iter().any(asked) || after.iter().filter(|a| !free_text || a.starts_with('-')).any(asked) {
        return Some(print_help(ci.map(|i| args[i].as_str())));
    }
    None
}

fn print_help(cmd: Option<&str>) -> i32 {
    match cmd {
        None => {
            println!("{}", usage());
            0
        }
        Some(c) => match usage_of(c) {
            Some(u) => {
                println!("{}", u);
                0
            }
            None if c == "narrator" => print_help(Some("observe")),
            None if c == "version" => print_help(None),
            None => {
                eprintln!("delphi: no command `{}`; `delphi --help` lists them", c);
                2
            }
        },
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(|s| s.as_str()) != Some("__core") {
        if let Some(code) = help_requested(&args) {
            exit(code);
        }
    }
    // Flags may come before the command; the command's own handler reads them from anywhere.
    let ci = command_index(&args);
    let cmd = ci.map(|i| args[i].clone());
    let rest: Vec<String> = match ci {
        Some(i) => args.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, a)| a.clone()).collect(),
        None => args.clone(),
    };
    let args = rest;
    start_error_log(cmd.as_deref(), &args);
    let code = match cmd.as_deref() {
        None if args.iter().any(|a| a == "--version") => {
            println!("delphi {}", env!("CARGO_PKG_VERSION"));
            0
        }
        None => cmd_start(&args),
        // The daemon and its maintenance commands run from this same executable.
        Some("__core") if ci == Some(0) => {
            // The daemon reports the release of this program, not of the library crate it is built from, so a client
            // can tell a daemon left over from an older release.
            // SAFETY: set before any thread exists.
            unsafe { std::env::set_var("DELPHI_RELEASE_VERSION", env!("CARGO_PKG_VERSION")) };
            delphi_engine::core_main::run(args.clone())
        }
        // The tool bridge's relay, started by a subscription CLI as its MCP server (`delphi_engine::bridge`).
        Some("__mcp-bridge") if ci == Some(0) => delphi_engine::bridge::run_relay(&args),
        // The Antigravity CLI's `PreToolUse` gate hook: asks the span's server whether a tool call may run.
        Some("__agy-hook") if ci == Some(0) => delphi_engine::bridge::run_agy_hook(),
        Some("lint") => cmd_lint(&args),
        Some("check") => cmd_check(&args),
        Some("setup") => cmd_setup(&args),
        Some("update") => cmd_update(&args),
        Some("init") => cmd_init(&args),
        Some("doctor") => cmd_doctor(&args),
        Some("logs") => logs_cmd::logs(&args),
        Some("debug") => logs_cmd::debug(&args),
        Some("status") => cmd_status(&args),
        Some("missions") => cmd_missions(&args),
        Some("changelog") => cmd_changelog(&args),
        Some("release") => release::cli(None),
        Some("sessions") => cmd_sessions(&args),
        Some("capabilities") if args.iter().any(|a| a == "--models") => match resolve_ws("capabilities", &args) {
            Ok((home, ws)) => delphi_engine::models_cli::inventory(&ws.dir, &home, args.iter().any(|a| a == "--json")),
            Err(code) => code,
        },
        Some("capabilities") => host_caps::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref())),
        Some("tools") => cmd_tools(&args),
        Some("learn") => cmd_learn(&args),
        Some("retro") => cmd_retro(&args),
        Some("customize") => cmd_customize(&args),
        Some("chat") => cmd_chat(&args),
        Some("run") => cmd_run(&args),
        Some("relay") => cmd_relay(&args),
        Some("decisions") => cmd_decisions(&args),
        Some("skills") => cmd_skills(&args),
        Some("claims") => claims_cmd::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref())),
        Some("sources") => sources_cmd::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref())),
        Some("pages") => cmd_pages(&args),
        Some("bundle") => cmd_bundle(&args),
        Some("hooks") => cmd_hooks(&args),
        Some("timers") => cmd_timers(&args),
        Some("daemon") => cmd_daemon(&args),
        Some("kb") => cmd_kb(&args),
        Some("needs") => cmd_needs(&args),
        Some("validation") => cmd_validation(&args),
        Some("train") => train::run(&args),
        Some("xenia") => xenia_cmd::run(&args),
        Some("docs") => docs_cmd::docs(&args),
        Some("completions") => docs_cmd::completions(&args),
        Some("connectors") => cmd_core("connectors", &args),
        Some("narrator") | Some("observe") => cmd_narrator(&args),
        Some("auth") => cmd_core("auth", &args),
        Some("endpoints") => cmd_core("endpoints", &args),
        Some("models") => cmd_core("models", &args),
        Some("version") => {
            println!("delphi {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some(other) => {
            eprintln!("delphi: unknown command `{}`; `delphi --help` lists them", other);
            eprintln!("commands: {}", CMDS.iter().filter(|c| !c.name.is_empty()).map(|c| c.name).chain(["narrator", "version"]).collect::<Vec<_>>().join(" "));
            2
        }
    };
    log_exit(cmd.as_deref(), &args, code);
    exit(code);
}

/// Start the central error log (`specs/039`) for this process and install the panic hook. The commands that never touch
/// the Delphi home (`lint`, `check`, `docs`, `completions`, `version`, `release`) log nothing; so do the daemon and its
/// relays, which start their own (the daemon names its home itself).
fn start_error_log(cmd: Option<&str>, args: &[String]) {
    if matches!(cmd, Some("__core" | "__mcp-bridge" | "__agy-hook" | "lint" | "check" | "docs" | "completions" | "version" | "release" | "help")) || (cmd.is_none() && args.iter().any(|a| a == "--version")) {
        delphi_core::elog::install_panic_hook("cli");
        return;
    }
    let home = home::delphi_home(flag(args, "--home").as_deref());
    delphi_core::elog::init(&home, "cli", env!("CARGO_PKG_VERSION"));
    delphi_core::elog::install_panic_hook("cli");
}

/// A command that ends non-zero is one record: the command, the exit code and the SHAPE of its arguments (flag names and
/// how many words), never their values. A report that found something (`doctor`, `lint`, `train`...) is a warning, not an error.
fn log_exit(cmd: Option<&str>, args: &[String], code: i32) {
    if code == 0 || !delphi_core::elog::enabled(delphi_core::elog::Level::Warn) {
        return;
    }
    let name = cmd.unwrap_or("start");
    let flags: Vec<&str> = args.iter().filter(|a| a.starts_with("--")).map(|a| a.split('=').next().unwrap_or("")).collect();
    let words = args.iter().filter(|a| !a.starts_with('-')).count();
    let shape = format!("{} word(s) {}", words, flags.join(" "));
    let ctx = serde_json::json!({"command": name, "exit_code": code, "args_shape": shape});
    let msg = format!("delphi {} exited with code {}", name, code);
    let finding = code == 1 && matches!(name, "doctor" | "lint" | "check" | "status" | "train" | "bundle" | "update" | "learn" | "retro" | "decisions" | "xenia");
    if code == 2 || finding {
        delphi_core::elog::warn("cli", "cli_exit", &msg, ctx);
    } else {
        delphi_core::elog::error("cli", "cli_exit", &msg, ctx);
    }
}

/// An error a command prints and then returns a code for: its text goes to the error log too (redacted there).
fn log_cli_error(cmd: &str, text: &str) {
    delphi_core::elog::error("cli", "cli_error", text, serde_json::json!({"command": cmd}));
}

/// Value of `--flag <value>` in `args`.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// `delphi check <kind> <path>`: the deterministic helpers behind the built-in skills (see `check.rs`). Reads files only.
fn cmd_check(args: &[String]) -> i32 {
    let positional: Vec<&String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[*i - 1].as_str(), "--repo" | "--home" | "--event" | "--hash"))).map(|(_, a)| a).collect();
    let [kind, target] = positional[..] else {
        eprintln!("{}", usage());
        return 2;
    };
    let repo = flag(args, "--repo").map(PathBuf::from);
    let result = match kind.as_str() {
        "fact" => check::fact(std::path::Path::new(target), &home::delphi_home(flag(args, "--home").as_deref())),
        "record" => match flag(args, "--event").and_then(|s| s.parse::<u64>().ok()) {
            Some(seq) => check::record(std::path::Path::new(target), seq, flag(args, "--hash").as_deref()),
            None => Err("record needs --event <seq>".to_string()),
        },
        _ => check::run(kind, std::path::Path::new(target), repo.as_deref()),
    };
    match result {
        Ok(report) => {
            report.print();
            report.exit_code()
        }
        Err(e) => {
            eprintln!("delphi check: {}", e);
            2
        }
    }
}

fn cmd_lint(args: &[String]) -> i32 {
    let root = match flag(args, "--repo") {
        Some(p) => PathBuf::from(p),
        None if args.is_empty() && std::env::current_dir().ok().and_then(|d| home::git_root(&d)).is_some() => PathBuf::from("."),
        None if args.is_empty() => {
            eprintln!("delphi lint: not in a git repository; pass --repo <path>");
            return 2;
        }
        None if args.iter().any(|a| a == "--repo") => PathBuf::from("."),
        None => {
            eprintln!("{}", usage());
            return 2;
        }
    };
    let tree = match lint::Tree::from_repo(&root) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("delphi lint: cannot read {}: {}", root.display(), e);
            return 2;
        }
    };
    let report = lint::run(&tree);
    report.print();
    println!(
        "delphi lint: {} file(s), {} fail, {} warn",
        tree.files.len(),
        report.count(report::Level::Fail),
        report.count(report::Level::Warn)
    );
    report.exit_code()
}

/// All values of a repeatable flag.
fn flags(args: &[String], name: &str) -> Vec<String> {
    args.iter().enumerate().filter(|(_, a)| *a == name).filter_map(|(i, _)| args.get(i + 1).cloned()).collect()
}

fn prompt(question: &str, default: &str) -> String {
    use std::io::{BufRead, Write};
    print!("{} [{}]: ", question, default);
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let _ = std::io::stdin().lock().read_line(&mut line);
    let v = line.trim();
    if v.is_empty() { default.to_string() } else { v.to_string() }
}

/// `delphi update`: bring everything to the newest release, in one command. The program is replaced first (see
/// `selfupdate.rs`); then the freshly installed program refreshes the built-in agents, skills and themes in the
/// workspaces (they are compiled into it, so the old program would write the old files) and restarts any running
/// daemon on itself. If the program is already the newest, only the workspace files are refreshed.
/// `--rollback` puts the previous program and the previous workspace files back. `--workspaces-only` is the
/// workspace step alone, which is how the new program is asked to do it.
fn cmd_update(args: &[String]) -> i32 {
    let has = |n: &str| args.iter().any(|a| a == n);
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let only = flag(args, "--workspace");
    if has("--workspaces-only") {
        return update::run(&home, only.as_deref(), has("--check"), has("--allow-downgrade"), has("--rollback"), has("--review"), has("--require-signed"));
    }
    let Ok(dest) = std::env::current_exe().and_then(std::fs::canonicalize) else {
        eprintln!("delphi update: cannot locate this executable");
        return 1;
    };
    let api = std::env::var("DELPHI_RELEASES_URL").unwrap_or_else(|_| selfupdate::DEFAULT_API.to_string());
    if api != selfupdate::DEFAULT_API {
        eprintln!("delphi update: reading releases from {api} (DELPHI_RELEASES_URL), not from the project's repository");
    }
    let mut workspace_code = 0;
    let code = selfupdate::run_with(args, &selfupdate::RealNet, &dest, &api, &mut |event| {
        if event == selfupdate::Event::UpToDate {
            workspace_code = update::run(&home, only.as_deref(), has("--check"), has("--allow-downgrade"), false, has("--review"), has("--require-signed"));
            return;
        }
        let mut step = vec!["update".to_string(), "--workspaces-only".into(), "--home".into(), home.to_string_lossy().to_string()];
        step.extend(only.iter().flat_map(|w| ["--workspace".to_string(), w.clone()]));
        step.extend(["--allow-downgrade", "--review", "--require-signed"].iter().filter(|f| has(f)).map(|f| f.to_string()));
        if event == selfupdate::Event::RolledBack {
            step.push("--rollback".into());
        }
        workspace_code = std::process::Command::new(&dest).args(&step).stdin(std::process::Stdio::null()).status().ok().and_then(|s| s.code()).unwrap_or(1);
        if !has("--no-restart") {
            restart_daemons(&dest, &home, only.as_deref());
        }
    });
    // The CLIs may have changed with the machine; map them again with the program just installed, without waiting.
    if code == 0 && !has("--check") {
        host_caps::refresh_in_background(&dest, &home);
    }
    // No workspace yet is not a failure of the update, and neither is a rollback with nothing to restore there.
    if code != 0 || workspace_code == 2 || has("--rollback") { code } else { workspace_code }
}

/// Restart every (or the named) workspace's running daemon with the freshly installed `exe`, so none keeps running the
/// old release. Each is restarted by that program itself (`daemon restart`).
fn restart_daemons(exe: &std::path::Path, home: &std::path::Path, only: Option<&str>) {
    let Ok(rd) = std::fs::read_dir(home.join("workspaces")) else { return };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|d| d.join("config.toml").exists()).collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let running = only.is_none_or(|o| o == name) && delphi_core::spawn::layout(home, &dir).is_ok_and(|l| delphi_core::client::Client::connect(&l.sock).is_ok());
        if !running {
            continue;
        }
        println!("Restarting the daemon of workspace `{}`…", name);
        let ok = std::process::Command::new(exe).args(["daemon", "restart", "--workspace", &name, "--home"]).arg(home).stdin(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
        if !ok {
            eprintln!("delphi update: could not restart it; run `delphi daemon restart --workspace {}`", name);
        }
    }
}

/// `delphi setup`: detect, ask, write, ratify, prove (see `setup.rs`).
fn cmd_setup(args: &[String]) -> i32 {
    let o = setup::Opts {
        home: home::delphi_home(flag(args, "--home").as_deref()),
        cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        workspace: flag(args, "--workspace"),
        yes: args.iter().any(|a| a == "--yes" || a == "-y"),
        check: args.iter().any(|a| a == "--check"),
    };
    match setup::run(&mut setup::TerminalUi { auto: o.yes }, &setup::RealEnv, &o) {
        Ok(_) => {
            if !o.check {
                if let Ok(exe) = std::env::current_exe() {
                    host_caps::refresh_in_background(&exe, &o.home);
                }
            }
            0
        }
        Err(e) => {
            log_cli_error("setup", &format!("{:#}", e));
            eprintln!("delphi setup: {:#}", e);
            1
        }
    }
}

/// Spec 054 FR1: only an interactive init on a terminal continues into setup, and only on a yes.
fn wants_setup(interactive: bool, stdin_is_terminal: bool, answer: &str) -> bool {
    interactive && stdin_is_terminal && !matches!(answer.trim().to_lowercase().as_str(), "n" | "no")
}

fn cmd_init(args: &[String]) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut projects: Vec<PathBuf> = flags(args, "--project").iter().map(PathBuf::from).collect();
    if projects.is_empty() {
        if let Some(root) = home::git_root(&cwd) {
            projects.push(root);
        }
    }
    let projects: Vec<PathBuf> = projects.iter().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())).collect();
    let default_name = projects
        .first()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".into());
    let interactive = !args.iter().any(|a| a == "--non-interactive");
    let mut name = flag(args, "--workspace").unwrap_or(default_name);
    let mut theme = flag(args, "--theme").unwrap_or_else(|| "greek".into());
    if interactive {
        name = prompt("Workspace name", &name);
        theme = prompt("Theme (built-in: greek)", &theme);
    }
    match home::init(&home::InitOptions { home: home.clone(), workspace: name, projects: projects.clone(), theme }) {
        Ok(ws) => {
            println!("delphi init: workspace `{}` created at {}", ws.name, ws.dir.display());
            for p in &projects {
                println!("  project: {} (unchanged; Delphi writes nothing into projects)", p.display());
            }
            use std::io::IsTerminal;
            let tty = interactive && std::io::stdin().is_terminal();
            if tty && wants_setup(true, true, &prompt("Connect a model now? (y/n)", "y")) {
                let mut sa = vec!["--home".to_string(), home.display().to_string(), "--workspace".to_string(), ws.name.clone()];
                if args.iter().any(|a| a == "--yes" || a == "-y") {
                    sa.push("--yes".into());
                }
                return cmd_setup(&sa);
            }
            println!("  next: run `delphi setup` to connect a model, then `delphi`");
            0
        }
        Err(e) => {
            log_cli_error("init", &format!("{:#}", e));
            eprintln!("delphi init: {:#}", e);
            1
        }
    }
}

/// Find the workspace for a command: `--workspace`, `DELPHI_WORKSPACE`, or the project containing the
/// current directory.
fn resolve_ws(cmd: &str, args: &[String]) -> Result<(PathBuf, home::Workspace), i32> {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = match home::resolve_workspace(&home, &cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => n,
        home::Resolve::Ambiguous(v) => {
            eprintln!("delphi {}: several workspaces register this project ({}); pass --workspace or set DELPHI_WORKSPACE", cmd, v.join(", "));
            return Err(2);
        }
        home::Resolve::NoneRegistered(root) => {
            eprintln!("delphi {}: no workspace registers {}; run `delphi init --project {}`", cmd, root.display(), root.display());
            return Err(2);
        }
        // Outside any project, the workspace that bare `delphi` made for that case, if it exists.
        home::Resolve::NotInGitRepo if home.join("workspaces").join(start::DEFAULT_WORKSPACE).join("config.toml").exists() => start::DEFAULT_WORKSPACE.to_string(),
        home::Resolve::NotInGitRepo => {
            eprintln!("delphi {}: not inside a git repository; pass --workspace <name>, or run `delphi` to create the default workspace", cmd);
            return Err(2);
        }
        home::Resolve::NoSuchWorkspace(n) => {
            eprintln!("delphi {}: no workspace named `{}` in {}", cmd, n, home.display());
            return Err(2);
        }
    };
    let ws = home::Workspace { dir: home.join("workspaces").join(&name), name };
    Ok((home, ws))
}

/// Display names for the active theme, from its ratified pack (display only; behavior never depends on it).
fn theme_names(ws: &home::Workspace) -> std::collections::HashMap<String, String> {
    let cfg = std::fs::read_to_string(ws.dir.join("config.toml")).unwrap_or_default();
    let theme = toml::from_str::<toml::Value>(&cfg).ok().and_then(|v| v.get("theme").and_then(|t| t.get("name")).and_then(|n| n.as_str()).map(String::from)).unwrap_or_else(|| "greek".into());
    let text = std::fs::read_to_string(ws.dir.join("local/ratified/themes").join(&theme).join("theme.md")).unwrap_or_default();
    theme::parse(&text).map(|t| t.roles.into_iter().map(|(role, name, _, _)| (role, name)).collect()).unwrap_or_default()
}

/// The full-screen console (D-28): `$DELPHI_UI`, or `delphi-ui` next to this executable. Absent, the inline console runs.
fn ui_executable() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("DELPHI_UI").map(std::path::PathBuf::from).or_else(|| std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("delphi-ui"))))?;
    p.is_file().then_some(p)
}

fn cmd_chat(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("chat", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    // A signed release refreshes this workspace on its first start, and says so (specs/036). Nothing else starts this.
    if let Some(line) = update::auto_refresh(&home, &ws.dir) {
        eprintln!("{line}");
    }
    let names = theme_names(&ws);
    if args.iter().any(|a| a == "--no-mouse") {
        // SAFETY: set before any thread exists; the full-screen console reads it at start.
        unsafe { std::env::set_var("DELPHI_NO_MOUSE", "1") };
    }
    // The inline console on a terminal; the plain line console when piped, or when asked for with --plain.
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if on_terminal && !args.iter().any(|a| a == "--plain" || a == "--inline") {
        if let Some(ui) = ui_executable() {
            // Exit code 3: the console is not the same release as this program; the inline console takes over.
            let code = chat::launch_ui(&ui, &home, &ws.dir, flag(args, "--mission"), &names);
            if code != 3 {
                return code;
            }
        }
    }
    // An older `delphi self-update` replaced only this program: say what is missing and how to get it.
    let notice = (!args.iter().any(|a| a == "--inline") && ui_executable().is_none())
        .then_some("(the full-screen console, delphi-ui, is not installed next to this program, so this is the inline console. Install both: curl -fsSL https://github.com/jessevondoom/delphi/releases/latest/download/install.sh | sh)");
    if on_terminal && !args.iter().any(|a| a == "--plain") {
        console::run(&home, &ws.dir, flag(args, "--mission"), names, notice)
    } else {
        chat::run(&home, &ws.dir, flag(args, "--mission"), names)
    }
}

/// `delphi hooks`: run the observe hooks (`hooks_cmd.rs`, `specs/018`).
fn cmd_hooks(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("hooks", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    hooks_cmd::run(&home, &ws.dir)
}

/// `delphi bundle validate|manifest` read a directory; `add|list|remove` change one workspace (`bundle_cmd.rs`).
fn cmd_bundle(args: &[String]) -> i32 {
    // `distill` is a task for the writer role, so it goes through the daemon like `delphi run "/distill …"` (`specs/027` §2.3).
    if args.first().map(|a| a.as_str()) == Some("distill") {
        let mut run_args: Vec<String> = Vec::new();
        let mut words: Vec<String> = Vec::new();
        let mut it = args[1..].iter();
        while let Some(a) = it.next() {
            if matches!(a.as_str(), "--workspace" | "--home" | "--mission" | "--timeout" | "--budget") {
                run_args.push(a.clone());
                run_args.extend(it.next().cloned());
            } else if a.starts_with("--") {
                run_args.push(a.clone());
            } else {
                words.push(a.clone());
            }
        }
        let task = format!("/distill {}", words.join(" "));
        run_args.extend(["--task".to_string(), task.trim().to_string()]);
        return cmd_run(&run_args);
    }
    if matches!(args.first().map(|a| a.as_str()), Some("add" | "list" | "remove" | "update" | "rollback" | "gc" | "export" | "serve" | "report" | "connectors" | "trust" | "trust-source")) {
        let (home, ws) = match resolve_ws("bundle", args) {
            Ok(v) => v,
            Err(code) => return code,
        };
        let mut rest = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if matches!(a.as_str(), "--workspace" | "--home") {
                it.next();
            } else {
                rest.push(a.clone());
            }
        }
        return bundle_cmd::run_in(&home, &ws.dir, &rest);
    }
    if args.first().map(String::as_str) == Some("sources") {
        return bundle_sources::cli(&args[1..], &home::delphi_home(flag(args, "--home").as_deref()));
    }
    bundle_cmd::run(args)
}

/// `delphi skills check|export|import` (`skills_cmd.rs`). `check` reads one directory and needs no workspace.
fn cmd_skills(args: &[String]) -> i32 {
    if args.first().map(|a| a.as_str()) == Some("check") {
        return skills_cmd::cli(std::path::Path::new("."), args);
    }
    let (_home, ws) = match resolve_ws("skills", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rest: Vec<String> = {
        let mut out = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if matches!(a.as_str(), "--workspace" | "--home") {
                it.next();
            } else {
                out.push(a.clone());
            }
        }
        out
    };
    skills_cmd::cli(&ws.dir, &rest)
}

/// `delphi decisions …`: review an optimistic build's provisional decisions from a shell (`decisions.rs`).
fn cmd_pages(args: &[String]) -> i32 {
    if args.first().map(String::as_str) != Some("search") {
        return pages_cmd::cli(args, &home::delphi_home(flag(args, "--home").as_deref()), None);
    }
    let (home, ws) = match resolve_ws("pages", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    pages_cmd::cli(args, &home, Some(&ws.dir))
}

fn cmd_decisions(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("decisions", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rest: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[*i - 1].as_str(), "--workspace" | "--home" | "--mission"))).map(|(_, a)| a.clone()).collect();
    decisions::cli(&home, &ws.dir, flag(args, "--mission"), &rest)
}

/// `delphi timers [list|set|cancel]`: the wakes the daemon is keeping (`timers_cmd.rs`, `specs/017`).
fn cmd_timers(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("timers", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    // `--in`, `--at` and `--every` belong to `set`; only the shared flags are taken out here.
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "--workspace" | "--home" | "--mission") {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    timers_cmd::cli(&home, &ws.dir, flag(args, "--mission"), &rest)
}

/// `delphi relay [check|totp-enroll|totp-check]`: the relay for approval cards (`relay/`, `specs/014`).
fn cmd_relay(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("relay", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let rest: Vec<String> = args.iter().enumerate().filter(|(i, a)| !a.starts_with("--") && (*i == 0 || !matches!(args[*i - 1].as_str(), "--workspace" | "--home"))).map(|(_, a)| a.clone()).collect();
    relay::cli::run(&home, &ws.dir, &rest)
}

/// `delphi run "<task>"`: one task with no terminal, ending in a summary and an exit code (`run.rs`).
fn cmd_run(args: &[String]) -> i32 {
    let opts = match run::parse_args(args) {
        Ok(o) => o,
        Err(m) => {
            eprintln!("delphi run: {}", m);
            return 4;
        }
    };
    let (home, ws) = match resolve_ws("run", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    if let Some(line) = update::auto_refresh(&home, &ws.dir) {
        eprintln!("{line}");
    }
    run::run(&home, &ws.dir, opts)
}

fn cmd_daemon(args: &[String]) -> i32 {
    let Some(action) = args.first().filter(|a| !a.starts_with("--")) else {
        eprintln!("{}", usage());
        return 2;
    };
    let (home, ws) = match resolve_ws("daemon", &args[1..]) {
        Ok(v) => v,
        Err(code) => return code,
    };
    chat::daemon(&home, &ws.dir, action)
}

/// `delphi auth …` and `delphi endpoints …`, in this process.
fn cmd_core(sub: &str, args: &[String]) -> i32 {
    let (_home, ws) = match resolve_ws(sub, args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--workspace" || a == "--home" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    match sub {
        "connectors" => delphi_engine::connectors_cli::run(&ws.dir, &rest),
        "auth" => delphi_engine::auth_cli::run(&ws.dir, &rest),
        "models" => delphi_engine::models_cli::run(&ws.dir, &rest),
        _ => delphi_engine::endpoints_cli::run(&ws.dir, &rest),
    }
}

fn cmd_narrator(args: &[String]) -> i32 {
    // The classic Rust Observer is retired (`specs/005e` §9.2): the narrator is the full-screen window.
    if let Some(bad) = args.iter().find(|a| a.starts_with("--") && !matches!(a.as_str(), "--once" | "--workspace" | "--home")) {
        eprintln!("delphi narrator: unknown option `{}` (see `delphi observe --help`)", bad);
        return 2;
    }
    let (home, ws) = match resolve_ws("narrator", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let view_arg = args.iter().find(|a| !a.starts_with("--") && Some(a.as_str()) != flag(args, "--workspace").as_deref() && Some(a.as_str()) != flag(args, "--home").as_deref());
    let view = match view_arg {
        Some(v) => match observer::View::parse(v) {
            Some(v) => Some(v),
            None => {
                eprintln!("delphi narrator: unknown view `{}` (dashboard, council, epic, agents, timeline or changes)", v);
                return 2;
            }
        },
        None => None,
    };
    let once = args.iter().any(|a| a == "--once");
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if !once && !on_terminal {
        eprintln!("delphi narrator: needs a terminal; `--once` prints one frame without one");
        return 1;
    }
    let Some(ui) = ui_executable() else {
        eprintln!("delphi narrator: the narrator is the delphi-ui program, which was not found next to `delphi`; run `delphi update` to get it");
        return 1;
    };
    let code = chat::launch_ui_observer(&ui, &home, &ws.dir, view.map(observer::View::name), &theme_names(&ws), once);
    if code == 3 {
        eprintln!("delphi narrator: delphi-ui is a different release from this program; run `delphi update`");
        return 1;
    }
    code
}

fn cmd_validation(args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str);
    if !matches!(sub, Some("score" | "audit" | "report" | "latch" | "retract" | "unquarantine" | "more" | "queue")) {
        eprintln!("usage: delphi validation score|audit|report|latch raise|retract --all|unquarantine|more|queue (delphi help validation)");
        return 2;
    }
    if xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi validation {}: refused; this is the operator's and this process was started by an agent seat", sub.unwrap_or(""));
        return 2;
    }
    if sub != Some("score") {
        return cmd_validation_audit(args);
    }
    let home = home::delphi_home(flag(args, "--home").as_deref());
    match validation_cmd::run(flag(args, "--set").map(PathBuf::from), flag(args, "--results").map(PathBuf::from), &home) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(e) => {
            log_cli_error("validation", "the labelled-set score failed");
            eprintln!("delphi validation score: {e}");
            2
        }
    }
}

fn cmd_validation_audit(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("validation", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let done = |r: Result<String, String>| match r {
        Ok(t) => {
            print!("{t}");
            0
        }
        Err(e) => {
            eprintln!("delphi validation: {e}");
            2
        }
    };
    match args[0].as_str() {
        "report" => done(validation_audit_cmd::report(&ws.dir, &home)),
        "retract" if args.iter().any(|a| a == "--all") => done(Ok(validation_audit_cmd::retract(&ws.dir))),
        "retract" => {
            eprintln!("usage: delphi validation retract --all");
            2
        }
        "queue" => done(Ok(validation_audit_cmd::queue(&ws.dir))),
        "more" => {
            let mission = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
            if mission.is_empty() {
                eprintln!("usage: delphi validation more <mission> --reason \"<text>\"");
                return 2;
            }
            done(validation_audit_cmd::more(&ws.dir, &mission, &flag(args, "--reason").unwrap_or_default()))
        }
        "unquarantine" => {
            let fact = args.get(1).filter(|a| !a.starts_with("--")).cloned().unwrap_or_default();
            if fact.is_empty() {
                eprintln!("usage: delphi validation unquarantine <fact-id> --reason \"<text>\"");
                return 2;
            }
            done(validation_audit_cmd::unquarantine(&ws.dir, &fact, &flag(args, "--reason").unwrap_or_default()))
        }
        "latch" if args.get(1).map(String::as_str) == Some("raise") => {
            let (to, reason) = (flag(args, "--to").unwrap_or_default(), flag(args, "--reason").unwrap_or_default());
            done(validation_audit_cmd::latch_raise(&ws.dir, &to, &reason, args.iter().any(|a| a == "--force")))
        }
        "latch" => {
            eprintln!("usage: delphi validation latch raise --to advisory|ratify --reason \"<text>\" [--force]");
            2
        }
        _ => {
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                eprintln!("delphi validation audit: needs a terminal; the answer is yours to give");
                return 2;
            }
            match validation_audit_cmd::audit(&ws.dir, flag(args, "--card").as_deref(), &mut std::io::stdin().lock(), &mut std::io::stdout()) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("delphi validation audit: {e}");
                    2
                }
            }
        }
    }
}

fn cmd_needs(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("needs", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--workspace" || a == "--home" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    match rest.first().map(String::as_str) {
        None => {
            print!("{}{}", needs::render(&needs::report_in(&ws.dir, Some(&home))), needs::validation_section(&ws.dir, delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)));
            0
        }
        Some("add") if rest.len() >= 2 => match needs::add(&ws.dir, &rest[1], &rest[2..].join(" ")) {
            Ok(()) => {
                println!("recorded in {}", ws.dir.join("local/needs.jsonl").display());
                0
            }
            Err(e) => {
                log_cli_error("needs", &format!("{:#}", e));
                eprintln!("delphi needs: {:#}", e);
                2
            }
        },
        _ => {
            eprintln!("usage: delphi needs [add <item> <why>]  (items: {})", needs::item_ids().join(", "));
            2
        }
    }
}

fn cmd_kb(args: &[String]) -> i32 {
    // `eval` and `model` work on documents and model files, not on a workspace.
    let (home, ws_dir) = if matches!(args.first().map(String::as_str), Some("eval" | "model")) {
        (home::delphi_home(flag(args, "--home").as_deref()), PathBuf::new())
    } else {
        match resolve_ws("kb", args) {
            Ok((home, ws)) => (home, ws.dir),
            Err(code) => return code,
        }
    };
    // The flags that select the workspace are not part of the query.
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--workspace" || a == "--home" {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    delphi_engine::kb::run(&home, &ws_dir, &rest)
}

/// Bare `delphi`: create what is missing (workspace, model access), then open the console.
fn cmd_start(args: &[String]) -> i32 {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    let interactive = on_terminal && !args.iter().any(|a| a == "--no-onboard") && std::env::var_os("DELPHI_NO_ONBOARD").is_none();
    let ws = match start::preflight(&home, &cwd, flag(args, "--workspace"), interactive) {
        Ok(ws) => ws,
        Err(e) => {
            log_cli_error("start", &format!("{:#}", e));
            eprintln!("delphi: {:#}", e);
            return 2;
        }
    };
    // From here it is `delphi chat` on the workspace that was just resolved.
    let mut rest: Vec<String> = args.iter().filter(|a| *a != "--no-onboard").cloned().collect();
    if flag(&rest, "--workspace").is_none() {
        rest.extend(["--workspace".into(), ws.name]);
    }
    cmd_chat(&rest)
}

/// `delphi status`: workspace, daemon, models, health and mission in a few lines, with no terminal (see `status.rs`).
fn cmd_status(args: &[String]) -> i32 {
    match resolve_ws("status", args) {
        Ok((home, ws)) => status::cli(&home, &ws),
        Err(code) => code,
    }
}

/// `delphi missions`: the workspace's missions, newest first (see `missions.rs`).
fn cmd_missions(args: &[String]) -> i32 {
    match resolve_ws("missions", args) {
        Ok((_home, ws)) => missions::cli(&ws.dir),
        Err(code) => code,
    }
}

/// `delphi changelog`: what the team recorded as finished in a mission, as Markdown for a pull request (see `changelog.rs`).
fn cmd_changelog(args: &[String]) -> i32 {
    match resolve_ws("changelog", args) {
        Ok((_home, ws)) => changelog::cli(&ws.dir, &changelog::Options { mission: flag(args, "--mission"), filter: delphi_engine::changelog::Filter { session: flag(args, "--session"), since: flag(args, "--since") }, json: args.iter().any(|a| a == "--json") }),
        Err(code) => code,
    }
}

/// `delphi tools`: tool use and `record_change` adoption by model, from the mission logs (see `toolreport.rs`).
fn cmd_tools(args: &[String]) -> i32 {
    match resolve_ws("tools", args) {
        Ok((_home, ws)) => toolreport::cli(&ws.dir, &toolreport::Options { mission: flag(args, "--mission"), all: args.iter().any(|a| a == "--all"), json: args.iter().any(|a| a == "--json"), prom: args.iter().any(|a| a == "--prom") }),
        Err(code) => code,
    }
}

/// `delphi sessions`: the central log of finished sessions (see `sessions_cmd.rs`).
fn cmd_sessions(args: &[String]) -> i32 {
    match resolve_ws("sessions", args) {
        Ok((home, _ws)) => sessions_cmd::cli(&home, &sessions_cmd::Options { mission: flag(args, "--mission"), json: args.iter().any(|a| a == "--json"), limit: flag(args, "--limit").and_then(|v| v.parse().ok()).unwrap_or(20), full: args.iter().any(|a| a == "--full") }),
        Err(code) => code,
    }
}

/// `delphi learn list|show|drop`: the notes the daemon staged from what happened (see `learn_cmd.rs`).
fn cmd_learn(args: &[String]) -> i32 {
    match resolve_ws("learn", args) {
        Ok((_home, ws)) => learn_cmd::cli(&ws.dir, args, &learn_cmd::Options { mission: flag(args, "--mission"), json: args.iter().any(|a| a == "--json") }),
        Err(code) => code,
    }
}

/// `delphi customize note|rule|skill|profile`: the operator writes to the stores directly (see `customize_cmd.rs`).
fn cmd_customize(args: &[String]) -> i32 {
    match resolve_ws("customize", args) {
        Ok((home, ws)) => customize_cmd::cli(&ws.dir, args, &customize_cmd::Options { home }),
        Err(code) => code,
    }
}

/// `delphi retro list|show`: the proposals the Orchestrator made, waiting for review (see `retro_cmd.rs`).
fn cmd_retro(args: &[String]) -> i32 {
    match resolve_ws("retro", args) {
        Ok((home, ws)) => retro_cmd::cli(&ws.dir, args, &retro_cmd::Options { home: home.clone(), scope: flag(args, "--scope"), mission: flag(args, "--mission"), json: args.iter().any(|a| a == "--json") }),
        Err(code) => code,
    }
}

fn cmd_doctor(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("doctor", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let opts = doctor::Options { require_providers: args.iter().any(|a| a == "--require-providers") };
    let on_terminal = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    if args.iter().any(|a| a == "--fix") {
        if !on_terminal {
            eprintln!("delphi doctor --fix: needs a terminal (it asks before each fix); run `delphi doctor` for the report");
            return 2;
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        return start::doctor_fix(&mut setup::TerminalUi { auto: false }, &home, &ws, &cwd, &opts);
    }
    let report = doctor::run(&home, &ws, &opts);
    report.print();
    println!("delphi doctor: workspace `{}`: {}", ws.name, report.worst());
    report.exit_code()
}

#[cfg(test)]
mod init_setup_tests {
    use super::wants_setup;

    #[test]
    fn only_an_interactive_terminal_yes_continues_into_setup() {
        assert!(wants_setup(true, true, "y"));
        assert!(wants_setup(true, true, ""));
        assert!(!wants_setup(true, true, "No"));
        assert!(!wants_setup(false, true, "y"));
        assert!(!wants_setup(true, false, "y"));
    }
}
