//! `delphi`: roadmap Step 1 commands (`specs/001`, `specs/006`).

mod app_cmd;
mod avatar;
mod bundle_add;
mod bundle_cmd;
mod bundle_compat;
mod bundle_connect;
mod connect_cmd;
mod council_watch;
mod connect_files;
mod connect_run;
mod connect_theme;
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
mod claim_refs;
mod console;
mod decisions;
mod docs_cmd;
mod finisher;
mod finisher_fast;
mod finisher_gates;
mod forge_cmd;
mod forge_ready;
mod doctor;
mod escalation;
// Moved to the engine so the daemon's `check` tool and the CLI share one implementation.
mod frontmatter {
    pub use delphi_engine::frontmatter::*;
}
mod home;
mod host_caps;
mod hook_cmd;
mod handling_cmd;
mod hooks_cmd;
mod library_cmd;
mod lint;
mod logs_cmd;
mod merge;
mod missions;
mod models_calibrate;
mod changelog;
mod customize_cmd;
mod kb_review;
mod kb_signals;
mod learn_cmd;
mod learn_keep;
mod learn_teach;
mod agent_cmd;
mod promote_cmd;
mod research_cmd;
mod research_setup_cmd;
mod retro_cmd;
mod train_run;
mod train_items;
mod train_options;
mod train_defects;
mod train_fidelity;
mod train_retention;
mod train_arms;
mod train_hidden;
mod train_gate;
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
mod release_notes;
mod slice_claims;
mod spec_compile;
mod spec_views;
mod specs_cmd;
mod tracker;
mod tracker_migrate;
mod report;
mod run;
mod run_granted;
mod seed;
mod self_install;
mod selfupdate;
mod setup;
mod skills_cmd;
mod claims_cmd;
mod pages_cmd;
mod project;
mod project_index;
mod sources_acquire_cmd;
mod route_cmd;
mod sources_cmd;
mod start;
mod status;
mod theme;
mod test_cmd;
mod private_read_cmd;
mod timers_cmd;
mod train;
mod update;
mod history_cmd;
mod validation_audit_cmd;
mod validation_cmd;
mod validation_tier_cmd;

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
    Cmd { group: "Start", name: "run", about: "one task without a terminal, then a summary", synopsis: "  delphi run \"<task>\" [--on-card deny|policy:<file>|authorization:<id> --auth-run <run>]\n    [--permissions <mode>] [--budget <usd>] [--timeout <dur>]\n    [--json|--jsonl] [--mission <id>] [--keep-daemon] [--workspace <name>] [--home <dir>]", detail: "Exit codes: 0 ok · 1 gates failed · 2 a card was denied · 3 budget · 4 infrastructure ·\n5 an optimistic build awaits decision review.\n`--on-card authorization:<id> --auth-run <run>` (spec 092): approves only a Red `run_command` that\nexactly matches the record's entry, as `/bin/bash <pinned script>` in the sandbox, network off (macOS);\nevery other card is denied. A fresh authenticator code is asked at launch." },
    Cmd { group: "Set up", name: "setup", about: "find the model tools you have and connect them", synopsis: "  delphi setup [--yes] [--check] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "init", about: "create a workspace for a project", synopsis: "  delphi init [--non-interactive] [--workspace <name>] [--project <path>]... [--theme <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "project", about: "register, scaffold, and inspect directory project scopes", synopsis: "  delphi project register [path] [--workspace <name>] [--home <dir>]\n  delphi project init [path] [--yes] [--workspace <name>] [--home <dir>]\n  delphi project list | scope [--workspace <name>] [--home <dir>]\n  delphi project index [path] [--refresh] [--workspace <name>] [--home <dir>]\n  delphi project private <name> on|off [--workspace <name>] [--home <dir>]\n  delphi project relink <project-id> <new-path> [--home <dir>]\n  delphi project clone [path] [--home <dir>]", detail: "A project is a directory scope; Git is optional. `register` writes the identity marker and central project record. `private <name> on` marks a project private (specs/115): it shares none of its research with the workspace's other projects, while its own sessions still read what the others share; `off` clears the flag. The name is the project's display name or its id (`project list` shows both, and `private` beside a private one). Nothing is copied or deleted; the next search follows the flag, and `delphi doctor` prints the setting and the private projects. `init` lists paths first, asks for confirmation, and creates only missing project folders and `_index.md`; in non-interactive use, pass `--yes`. Existing files are preserved. `index` reports whether `_index.md` is current; `index --refresh` regenerates it (and `.delphi-index/` category indexes for large projects) from the files allowed by `.gitignore`, `.delphiignore` and the default exclusions (credentials, hidden files, caches, build and dependency trees, files over 2 MiB; symlinks are not followed). A hand-edited `_index.md` is never overwritten: a proposed replacement goes to `.delphi-index/proposed-index.md` and the command exits 3." },
    Cmd { group: "Set up", name: "auth", about: "sign in to, or out of, a model endpoint", synopsis: "  delphi auth login <name> [--manual] | status | logout <name> [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "endpoints", about: "check that each model endpoint answers", synopsis: "  delphi endpoints test [<endpoint id>] [--probe [--yes] [--ladder]] [--workspace <name>] [--home <dir>]", detail: "Without `--probe` it prints the cached results and spends nothing.\n`--probe` reads each harness CLI's version, prints the calls and their estimated cost,\nand runs only on your yes (`y` at a terminal, or `--yes`). It writes a conformance record\nper endpoint, model and CLI version to `local/carrier-conformance.json` for `delphi doctor`.\n`--ladder` adds the input ladder (100 to 250 KB of filler, stopping at the first size not\nanswered); it runs only on `y` at a terminal, never on `--yes`. A seat's byte budget is then\nthe smaller of its `max_prompt_bytes` and 80% of the largest size answered on that CLI version." },
    Cmd { group: "Set up", name: "connectors", about: "the connectors that are configured, logged in and pinned", synopsis: "  delphi connectors status | test <name> | pin <name> [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Set up", name: "relay", about: "send approval cards to your email or Telegram", synopsis: "  delphi relay [check|totp-enroll|totp-check <code>] [--workspace <name>] [--home <dir>]", detail: "Takes your replies too. Run it in a terminal or a tmux pane." },
    Cmd { group: "Set up", name: "skills", about: "agentskills.io skills in and out", synopsis: "  delphi skills check <dir> | export <name> [--out <dir>] | import <dir> --agents <role,role>\n    [--description \"…\"] [--workspace <name>] [--home <dir>]", detail: "An import is a pending proposal until you /ratify it." },
    Cmd { group: "Set up", name: "bundle", about: "check an org bundle: knowledge, skills and behavior to share", synopsis: "  delphi bundle init <dir> --org <org> --name <name>\n  delphi bundle validate <dir> [--claude] [--against <previous dir>] [--package] [--report <file>] [--workspace <name>]\n  delphi bundle distill [<path|missions/id>...] [--workspace <name>]\n  delphi bundle import <plugin dir> [-o <new dir>] [--as <name>] [--org <org>] [--agents <role,role>] [--agent-role <agent>=<role>]\n  delphi bundle export <name> --claude-plugin <new dir> [--strip-delphi] | --agents-md [<file>] [--workspace <name>]\n  delphi bundle export <name>... --marketplace <dir> --repo <owner/repo|url> --sha <commit> [--ref R] [--subdir D]\n  delphi bundle manifest <dir> [--claude]\n  delphi bundle sign <dir> [--against <previous dir>] [--home <dir>]\n  delphi bundle package <dir> [-o <file.xbundle>] [--vectors]\n  delphi bundle add <dir or file.xbundle> | --from-marketplace <repo> <name> [--order N] [--replace-publisher] [--no-vectors] [--yes] [--dry-run] [--trust ratified] [--trust-source] [--workspace <name>] [--home <dir>]\n  delphi bundle add <record id> --from-xenia <org>/<store> [--order N] [--replace-publisher] [--no-vectors] [--yes] [--dry-run] [--workspace <name>] [--home <dir>]\n  delphi bundle store <org>/<store> [--auto-update] [--workspace <name>] [--home <dir>]\n  delphi bundle publish <dir> --to <org>/<store> --url <https git url> --pin <git-tree|git-commit>:<sha> [--subdir D] [--id <record>] [--tags a,b] [--review-by YYYY-MM-DD] [--no-verify-tree] [--home <dir>]\n  delphi bundle sources add <dir> <source-id>... [--replace] [--home <dir>]   (copy library sources into the bundle's sources/, with their licence record)\n  delphi bundle trust <bundle> --level user_ratified|default [--workspace <name>] [--home <dir>]\n  delphi bundle trust-source <bundle|origin> [--level user_ratified|default] [--revoke] [--workspace <name>] [--home <dir>]\n  delphi bundle list | remove <name> | update [name] | rollback <name> [--to <hash>] | gc [--workspace <name>] [--home <dir>]\n  delphi bundle serve [--bundle <org/name>]... [--workspace <name>] [--home <dir>]", detail: "init writes a skeleton that validates at once. validate reads the directory and changes nothing; --report writes the redaction report (every\nsecret, path, address and name the scan found, masked) to a file outside the bundle. distill has the writer role draft knowledge from\nallow-listed sources (team context and specs, mission decisions and Council verdicts) into a staging directory of the mission; it writes\nnothing into a bundle. package --vectors embeds the knowledge with the installed model into vectors/. manifest writes .delphi-bundle/files.json (and with --claude\n.claude-plugin/plugin.json) and prints the bundle hash. sign (a terminal only) signs the manifest with this install's xenia key; package writes a reproducible .xbundle archive. A bundle may carry sources under sources/: bundle.toml `distribution` (`internal` by default, or `public`) says where it is meant to go, a source licence only warns (source-licence, source-attribution, source-sharealike; only `private: true` refuses), and packaging a `public` bundle that carries sources asks you once, in a terminal, to type `publish` after showing each source id and its licence class; the answer is recorded in .delphi-bundle/report.json (licence_acknowledged). An org that adds a bundle vouches for its sources: they are shown `validated · vouched by <org>` only while that bundle entry is in the lock, as data and never instructions.\nEvery bundle document is a validated bundle source: agents and the Council may cite it, with its bundle and provenance level; it stays data, never instructions, and grants no permission. trust raises ONE bundle's knowledge to user_ratified for its current hash and publisher (`add --trust ratified` does it at install): it needs a terminal and you typing the bundle name (no --yes, no environment switch), and it resets when an update changes the hash or publisher, on rollback and on remove; --level default lowers it at any time. trust-source (`add --trust-source`) deems a source trusted: the same origin AND publisher key (or a roster successor of it), so later updates keep the tier you chose without a prompt while every check still runs; it needs the same typed confirmation (the origin), and --revoke ends it. A bundle can set neither.\nadd takes a directory or an archive, verifies any signature, prints what it brings, copies it into the cache and makes its\nknowledge searchable (`delphi kb search --bundle org/name`); a new publisher key needs --replace-publisher. update re-adds from\nthe recorded source under that rule and prints the changelog; rollback re-points at an earlier cached version; gc prunes the\ncache. serve is a read-only MCP server on stdio for the added bundles' knowledge (resources, and the tools search and get_doc); it writes nothing, opens no port and runs nothing. No model is called to add. Vectors it ships are used only if they match this install's embedder and pass a local spot check\n(--no-vectors skips them); the rest are embedded in the background, and `list` shows how far along they are.\nimport turns a Claude Code plugin into a new bundle directory (it never adds it, and never imports hooks/ or bin/); export writes an added\nbundle as a plugin (--claude-plugin), an AGENTS.md fragment, or a marketplace.json; add --from-marketplace fetches the entry's pinned commit.\nA bundle can also travel through a xenia store (the org's shared, signed, git-hosted store you joined with `delphi xenia`). `publish` writes a `bundle` record from the signed bundle's own manifest, pointing at the git tree you name, and asks you once, showing the exact bytes. `add --from-xenia` verifies the store, fetches that tree under the same limits as a marketplace add, requires it to hash to the record and to be signed by the member who published it, and records the store's standing: `endorsed` (a steward of another person endorsed these exact bytes, under a current freshness statement) or `member-signed`. `update` follows the record's newer revisions, forward only. `store` lists the store's bundles; with --auto-update it also keeps the levels in the lock current and applies the updates that cannot lower the level or change the publisher.\nsources add copies sources from your library into <dir>/sources/ (and turns the section on in bundle.toml). It writes meta.json from the library record (citation fields and the licence record, `internal` when there is none; no status, origin, reliance count or note), source.md when the capture is whole, the excerpts without your purpose notes, and recognised assets. Licence is information, not a gate: nothing is refused for it. The one refusal is a source you flagged private, which never travels in a bundle; the command is also refused inside an agent seat. It refreshes files.json when the bundle has one (sign again if it was signed) and lists the source-licence, source-attribution and source-sharealike warnings `validate` would report. For a public bundle, `package` and `publish` print the sources that carry a licence note and ask you to confirm in a terminal (no --yes); for an internal bundle `publish` asks. The acknowledged ids are written to .delphi-bundle/report.json.\nExit codes: 0 pass · 1 failed · 2 usage." },
    Cmd { group: "Set up", name: "app", about: "start the desktop app and pair it; list, revoke and grant its keys", synopsis: "  delphi app [--workspace <name>] [--home <dir>]\n  delphi app list | log | opt-out | opt-in\n  delphi app approve [--grant request]\n  delphi app revoke <name>\n  delphi app grant <name> [--remove] <grant> [--auto]", detail: "Run it in a terminal (opt-out/opt-in need none): that vouches for the app. `approve` takes the code an app shows.\nA key holds only its grants (read, request, update); `grant <name> update --auto` lets it update without asking.\nThe app is DELPHI_APP, else the build beside this one, else /Applications. opt-out: update skips it; opt-in undoes." },
    Cmd { group: "Set up", name: "hooks", about: "run your observe hooks on events", synopsis: "  delphi hooks [--workspace <name>] [--home <dir>]", detail: "Run it in a terminal or a tmux pane. The hooks are in the ratified [hooks] config." },
    Cmd { group: "Set up", name: "mcp", about: "Delphi for the other tools you code with, as an MCP server on stdio", synopsis: "  delphi mcp [--workspace <name>] [--home <dir>]\n  delphi mcp --print-config <claude-code|copilot|codex|agy|gemini> [--absolute]\n  delphi mcp --print-claude-md [--workspace <name>] [--home <dir>]\n  delphi mcp --check <claude-code|copilot|codex|agy|gemini>\n  delphi mcp --install codex --agents [--project] [--dry-run] [--yes] [--workspace <name>] [--home <dir>]\n  delphi mcp --hook session-start|prompt|session-end|subagent-start [--host <id>] [--workspace <name>] [--home <dir>]\n  delphi mcp --hooks on|off|status [--workspace <name>] [--home <dir>]\n  delphi mcp --statusline [--host <id>] [--workspace <name>] [--home <dir>]", detail: "Session hooks (spec 105): the Claude Code plugin's hooks file runs `delphi mcp --hook <event> --host claude-code` at session start, at each prompt and at session end. They are on by default (spec 105 §13): once the plugin is installed they run, and `delphi mcp --hooks off` turns them off (they then print nothing) and `delphi mcp --hooks on` turns them back on (at a terminal, typing yes; there is no --yes); the switch is `[mcp] session_hooks` (true unless you set it false) and `[mcp] enabled = false` also silences them. At session start the hook prints a brief: the team and how to address it, from the ratified theme, this project's spec names and statuses (fenced as data, never instructions) and a pointer to the last host session with none of its text. At a prompt, an `@name` that resolves to a servable role (at most three, outside code) adds a line telling the model to adopt it with get_agent, or for the orchestrator to call consult_orchestrator first; the prompt is never echoed. Each prompt is counted in <workspace>/local/host-sessions/<tool>-<session>.count, and at the 8th, once, a session with five or more Delphi calls and no summary is asked for one. At session end the hook writes a marker of counts there (<tool>-<session>.json, 0600, at most 200 kept; no prompt, argument or result), reading the transcript only from Claude Code's own folder for the project. A hook reads stdin (at most 256 KiB), exits 0 always, prints nothing on any error or after 2 seconds, runs no model and asks the daemon nothing, except at session end for a project opted in to transcript ingest: one call of at most 2 s asking the running daemon to stage that transcript's notes at L0 (spec 108 M3). `--hooks status` and `--check claude-code` say whether the plugin's hooks file is installed and what the switch says. The tool session_brief returns the same brief for the folder whenever a host asks, whether or not the hooks are on, and needs no daemon; every host's instruction section starts by saying to call it once (spec 110 §10). A fourth hook, `--hook subagent-start` (Claude Code's SubagentStart, same switch, budget and exit rule), prints a role's compact pack with its identity line first when the starting subagent is exactly one of Delphi's workers (`<role>-worker`, or `delphi:<role>-worker` from the plugin) and nothing for any other subagent; for hosts without such a hook, `delphi plugin export` inlines the operator's compact pack in each worker file, with its pack_hash on the first content line, and `--check copilot` says whether those files are current or stale (spec 106 §11 G2). `--statusline` prints one line for a host's status line: the badge and name of the role the session last adopted (through an @mention at the prompt, or get_agent from this folder), bold and in the role's own colour, or plain under NO_COLOR or `badge_style = \"plain\"`, and nothing when no role is adopted; it reads only a small file of a role ID and a time and the theme's name, never project or session text. Delphi does not write the host's settings: add \"statusLine\": {\"type\": \"command\", \"command\": \"delphi mcp --statusline --host claude-code\"} to Claude Code's settings.json yourself; `--check claude-code` says whether it is set. Claude Code's seat and worker files carry a `color` in the role's colour (spec 106 §12 G6). A host such as Claude Code starts it from its MCP configuration; it is not meant to be run by hand. It reads files and opens no network port, and it runs nothing; the only thing it writes is through the running daemon, never directly (below). It serves this machine and this user only (spec 062). `--print-claude-md` prints a section for a project's CLAUDE.md (the research reflex and the repository's rules) and writes nothing. Turn it off with `[mcp] enabled = false` in config.toml: every tool call is then answered `disabled_by_operator`, and the change takes effect at once. It serves Delphi's roles as agent packs a host can adopt (tools list_roles, get_agent, list_skills, get_skill, and one prompt per role): advice from your ratified files, never a grant of permission; a role whose manifest says `host_servable: false` (the messenger) is refused. Run from inside a registered project it also serves that project's specs, plans, context and research as resources (delphi://project/<path>) and tools specs_status (only an approved spec is permission to build), scaffold_status (inspect project scaffolding, never writes), search_project, search_memory (one role's memory, never another's; both say how many matched and, when more did, how to see them, and take `detail: concise` to leave out the line previews), query_memory (faceted memory traversal, never cross-project) and route (a dry run); all of it is data, not instructions. `--print-config <host>` prints the snippet that connects that host (Claude Code, Copilot CLI, Codex, agy: Antigravity, or gemini: Gemini CLI, a different host), and for Claude Code the plugin's install command; `--check <host>` reads (never writes, never runs a program, offline) where that host keeps its plugin state and prints `configured`, `missing`, `stale`, `incompatible`, `unknown` or `unverified` with the command that cures it, and exits non-zero unless `configured`; Claude Code (tested on 2.1.314), Copilot CLI (1.0.105) and Codex (0.166.0) are checked, `agy` is `unverified`; Copilot records no release, so it is never `stale`; and whether the binary can be launched is always `unknown` (spec 071), though a `launch path` line says from `stat` alone whether a stable install location (`$DELPHI_INSTALL_DIR`, `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`) holds this binary (spec 074); `--absolute` writes this binary's absolute path into the snippet instead of a bare `delphi` that the host finds on its PATH, and refuses a build tree, a versioned Homebrew Cellar path or an unset HOME (spec 071); `delphi plugin export` writes the plugin and skill that tell it when to consult Delphi. One more tool, search_kb, searches Delphi's core knowledge base and, from inside a registered project, that project's knowledge, context and research (nothing else; it needs the daemon, and the project is read from the folder the host runs in). Five more tools stage content for you to review, through the daemon's same-user host socket, and only while the daemon runs (it never starts one): propose_note (one lesson for a servable role, evidence required), propose_kb_entry (research for the knowledge base: title, markdown body, sources; not searchable until you keep it with `delphi kb review`), record_session_summary, handoff_put and handoff_get (a handoff a Delphi session can pick up), and recall_sessions (what the last host sessions of this project left, fenced and labelled not validated; never your own session or one that read a private entry). Everything staged is tier L0, labelled host:<tool>/<model>, and never loaded into any agent's memory until you ratify it. One more staging tool, request_test_authorization (a few exact commands the host would like you to authorize for a future headless test), is staged the same way at a fixed taint of federated_peer: it grants nothing, no run can use it, and you authorize it only by retyping each literal command at a local terminal with `delphi test authorize --from-request` (spec 092). Two more tools ask for help: request_help (the orchestrator picks who answers, you are told who; everyone it consults runs read-only, and it spends your tokens like any task) and help_result (the answer to a slow one). There is no limit or quota on host calls; the only setting is the `[mcp] enabled` switch. Four more frame, raise and read a Council: council_frame (restates the question, names the Panel's seats and what to ask the operator, suggests a formal Council when one fits better, and proposes the foreign seats Delphi could run with their carrier, tools and token budget by a static rule, starting none; it reads and spends nothing), request_council (the question, options, stakes and context pointers; it returns a council_id at once), council_status (with a `phase` line while it deliberates) and council_result (the decision, each seat's position and the dissent verbatim, and a ready-made `display` card; advice that binds nothing in your project). The plugin's `delphi-council` skill tells the host how to run a Panel of its own subagents or a formal Council (spec 086). Four more run a Panel, a Council whose seats are the host's own subagents: council_open (starts it; spends none of Delphi's tokens), council_brief and council_submit (a seat's instructions for a round and its answer; Delphi holds Round 1 until every seat is in, parses Round 2 with the same strict rules a daemon Council uses, and copies the dissent verbatim; the moderator's verdict is round 3 by seat orchestrator) and council_choose (reports the option the operator picked after an escalation; recorded as the host's word, starts nothing). Whether a seat really ran in its own context, and on which model, is host-reported and recorded as such. With `foreign_seat` (or `foreign_seats`, a list of `{role, family?, tools?}` within the `[seats] max_foreign_seats` and `max_foreign_tokens` budget), council_open has Delphi run that one seat itself on a model family other than the host's (it spends the operator's tokens through Delphi's own harness, up to a stated ceiling; the host cannot brief or answer for it), so the Panel can honestly span two families. The plugin also carries four seat subagents for the council roles (architect, red-team, strategist, builder): short wrappers that fetch their brief from Delphi, with a model alias or tier the host documents (never a pinned version) so the red-team differs from the architect where the host offers two models. It also carries a worker subagent for every role served to hosts (`<role>-worker`; spec 106): a wrapper that adopts the role with get_agent in its own context, on the session's model; only the builder's and the writer's inherit the session's tools, every other worker gets read tools only. Codex's plugins are not documented to carry agents, so `--install codex --agents` writes `delphi-<role>.toml` and `delphi-<role>-worker.toml` into `~/.codex/agents/` (or `.codex/agents/` with `--project`): it needs a terminal for its yes (or `--yes`), prints the exact paths first, writes nothing before the yes, names a file that differs from what it would write and keeps the old content as `<name>.delphi-bak`, and `--dry-run` only prints. `[host.models.codex]` and `[host.models.copilot]` in config.toml map a role to a model name for those two hosts (no entry: no model line); it is refused inside an agent seat." },
    Cmd { group: "Set up", name: "connect", about: "connect Delphi to Claude Code, Copilot, Codex or Gemini CLI, with consent", synopsis: "  delphi connect claude-code|copilot|codex|gemini\n  delphi connect claude-code|copilot|codex|gemini --status | --dry-run | --remove\n  delphi connect claude-code|copilot --update [--dry-run]\n  delphi connect claude-code|copilot [--dry-run | --update | --status] --source owner/repo[#branch]\n  delphi connect claude-code --theme [--dry-run | --remove] [--light | --dark]", detail: "Role colours in Claude Code (spec 106 G7): `delphi connect claude-code --theme` shows what it would write, asks `[Y/n]` at a terminal, then writes `~/.claude/themes/delphi.json` (your current preset with only the eight subagent colours changed to the roles' own) and sets `theme` in `~/.claude/settings.json` to `custom:delphi`, changing nothing else in that file; `delphi connect claude-code` offers the same once it has connected. A theme of your own (`custom:...`) is never switched: the eight lines to add to it are printed. With `auto` a custom theme stops following the terminal's light or dark background, so `--light` or `--dark` picks the base (dark when neither). `--dry-run` only prints; `--remove` puts the earlier theme back and deletes the file. It refuses inside an agent seat, never follows a linked `settings.json`, and never replaces a `delphi.json` it did not write.\n\nCodex and Gemini CLI (spec 110 X2): `delphi connect codex|gemini` writes Delphi's own files without a question, under Delphi's folder for the host (Codex: `$CODEX_HOME/delphi/` with the instruction section `DELPHI.md` and the adapter; Codex's seat and worker files are left out because no live check showed Codex loading them, and `delphi mcp --install codex --agents` still writes them on request; Gemini CLI: the extension `$GEMINI_CLI_HOME/.gemini/extensions/delphi/` with the server, at this binary's absolute path, and `DELPHI.md` as its context file), never a file it did not write, and records each in `installed.json` there. `CODEX_HOME` and `GEMINI_CLI_HOME` default to `~/.codex` and your home. Then, only when stdin and stdout are a terminal and you type yes to each, it runs Codex's own `codex mcp add delphi -- <absolute path> mcp` and puts a marked block (between `<!-- delphi:begin` and `<!-- delphi:end -->`) holding the section into `$CODEX_HOME/AGENTS.md`; there is no override, and without a terminal it prints the command and the block instead. `--remove` deletes the recorded files that are unchanged (an edited one is kept and named) and, at a terminal on a yes each, runs `codex mcp remove delphi` and takes the block out, leaving every other byte. `--dry-run` prints the plan and writes nothing; `--status` prints what `delphi mcp --check <host>` reads: server, instruction section, role files and hooks, each installed, current, stale or absent. Gemini CLI could not be run in the live check, so its extension is marked unverified, and no hooks are installed for either host (spec 110 §8). Claude Code and Copilot CLI (spec 074 C1 to C3):`--status` is read-only (no TTY needed): it prints where Claude Code stands (`configured`, `missing`, `stale`, `incompatible`, `unknown`), the launch-path line and the next action. `--dry-run` finds the host CLI (`claude` or `copilot`) only in fixed directories (~/.local/bin, ~/.claude/local for Claude Code, /opt/homebrew/bin, /usr/local/bin, /usr/bin; never your PATH), resolves it, requires every path component to belong to you or root and not be writable by group or others, hashes it, and prints the exact plan a future connection would carry out: the fixed argv (no shell), the environment policy, the source `jessevondoom/delphi` at its branch `stable`, the expected writes and network, the state found, and the confirmation phrase that exact plan will need. It runs nothing, not even `--version`, and writes nothing. Bare `delphi connect <host>` needs a terminal on stdin and stdout, is refused inside an agent seat, shows the same plan on /dev/tty and runs only after you enter the exact phrase bound to that plan (pasting is fine). It runs `<host> plugin marketplace add jessevondoom/delphi#stable` then `<host> plugin install delphi@delphi` as fixed argv with no shell, a safe PATH, HOME, LANG, LC_*, TERM and a private TMPDIR, and rechecks the executable's digest before each exec. It refuses if anything of Delphi's is already present. The source is a GitHub repository at its branch `stable`, which the release workflow moves to each release, so the host follows releases and not `main` (spec 074 section 10.4); nothing is pinned. `--status` prints a `channel:` line: `stable`, or that a marketplace added earlier follows the default branch and how to switch. After a verified success it records what it added (<home>/operator/connections.json, 0600) and appends an audit line; a partial result is reported with the manual cure and is never rolled back or recorded. `--remove` (same consent) runs `<host> plugin uninstall delphi@delphi` and `<host> plugin marketplace remove delphi` only while that record, the plugin and the marketplace source and branch still match; for Copilot the installed copy must also be byte-identical (by digest) to the one connect recorded, so an updated or reinstalled plugin is left for you to remove. The plugin runs `~/.local/bin/delphi`, not a `delphi` from the host's PATH (spec 074 section 10.1): the plan's `launch path:` line says what is there, and when nothing is and this binary sits in a stable install location (Homebrew's bin, or DELPHI_INSTALL_DIR) the confirmed plan links it; nothing already there is ever replaced. `--status` prints `launchable: yes` or `no` with the cure. `--update` (spec 074 section 10.2; same terminal, plan and typed phrase, `UPDATE <host> <hash>`) runs the host's own update of a plugin that is already installed: `claude plugin update delphi@delphi`, or for Copilot `copilot plugin marketplace update delphi` then `copilot plugin update delphi@delphi`. It needs no record of an earlier connect (a plugin you installed by hand is updated too), refuses when the plugin is absent or the host's `delphi` marketplace is not this repository, removes nothing, and rewrites connect's record to the updated copy when there is one. `--update --dry-run` prints that plan. `delphi update` names a host whose plugin the program has outrun and points here. `--source owner/repo[#branch]` (spec 074 section 11) installs the plugin from another GitHub repository's marketplace, for a fork or an organization's copy: it is accepted only as that one typed argument (never from the environment or a file), only in the form owner/repo with an optional branch, is the single argv element the host is given, and the plan carries a warning naming it and the repository this build installs from by default. The record of that connect then stands for the source, so `--update`, `--remove` and `--status` need no flag afterwards; for a plugin you installed by hand from another repository, pass `--update --source owner/repo`. A fork that publishes its own builds sets DELPHI_MARKETPLACE_REPO when it compiles, which changes the default and every printed install command. Same-user race between the digest check and exec is residual (spec 074 requirement 5). Exit codes: 0 ok · 1 the host CLI was not found or is unsafe, or the run failed or was partial · 2 usage, refusal or no consent." },
    Cmd { group: "Set up", name: "plugin", about: "export Delphi as a plugin or instructions for another tool", synopsis: "  delphi plugin export --host <claude-code|copilot|codex|agy|gemini> --out <dir>", detail: "Writes into a new or empty directory and never overwrites. For claude-code: a plugin with `.mcp.json` and a `delphi` skill that says when to consult Delphi. For the others: `mcp-config.txt` and `DELPHI.md`. Nothing is installed into the host (spec 062 §10)." },
    Cmd { group: "Work", name: "decisions", about: "review an optimistic build's provisional decisions", synopsis: "  delphi decisions [list|show <id>|ratify <id…|all-routine>|reject <id> [why]] [--mission <id>]\n    [--workspace <name>] [--home <dir>]", detail: "This is the text form for a shell. In the console, `/decisions` with no argument opens the decisions pane:\nevery approval card, provisional decision and agent-raised decision you owe, one at a time." },
    Cmd { group: "Work", name: "private-read", about: "see which tasks read a private entry and clear a mark", synopsis: "  delphi private-read [status | clear <key>]\n    [--workspace <name>] [--home <dir>]", detail: "Spec 103 slices M1 and M2. A task that was handed the text of a private entry is marked. A mark changes nothing that flows in: no read or prompt differs. On a marked task, a web request and a connector call that would carry text out are held and put to you first (Red, the exact text on the card, asked every time, approved only at the console, never by a relay reply); nothing is sent before you answer, and no answer is a denial. Relay and Mail are not held yet. The counter says how many sends were held. Only the operator clears a mark, here or at the console." },
    Cmd { group: "Work", name: "timers", about: "the wakes the daemon is keeping for roles", synopsis: "  delphi timers [list | set <role> (--in <5m> | --at <time>) [--every <5m>] <note…> | cancel <id>]\n    [--mission <id>] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Work", name: "history", about: "the project history: turn it off, erase it, see what is kept", synopsis: "  delphi history status [--workspace <name>] [--home <dir>]\n  delphi history off [--project <id>] [--workspace <name>] [--home <dir>]\n  delphi history purge [--since <7d | date | time>] [--project <id>] [--yes] [--workspace <name>] [--home <dir>]\n  delphi history delete <id> [--project <id>] [--yes] [--workspace <name>] [--home <dir>]", detail: "The history is your own prompts, redacted, with a capped summary of what came of each, kept on this machine only (specs/102). It is on by default, but nothing is recorded until a person at a terminal has answered the install question once (`delphi init`, or the first `delphi` start after an upgrade); an answer of no, `delphi history off` or `[history] enabled = false` records nothing. `status` shows whether it is on, how many records and erased tombstones each project has, the store size against its budget, whether the chain verifies, the time of the last record and how many records are past the age budget (`[history] retention_days`; nothing is deleted at that age); it prints no record text. `off` stops new records at once (for one project with --project) and deletes nothing: it writes a marker file, and removing the marker turns the history back on. `purge` erases the records (all, or those written at or after --since, a duration like 7d, a date or a time) and `delete` erases one record by id (with --project when several projects have a history); each erased record leaves a tombstone, which keeps its id, its time and its keyed hash and no text, so the chain still verifies and the erasure is visible. A record that had already failed the chain check leaves a tombstone that is itself a break. Both ask you to type `yes` (or take --yes). A running daemon holds your last prompt in memory until its trigger fires or it stops: `purge` writes a marker with the time and no text (local/history.purged), and the daemon drops a held prompt noted before it instead of writing it back; `off` keeps it from being written. Every subcommand is the operator's and is refused inside an agent seat; there is no way for a host or a seat to call them." },
    Cmd { group: "Work", name: "handling", about: "how data is handled: private, internal and confidential", synopsis: "  delphi handling status [--json] [--workspace <name>] [--home <dir>]\n  delphi handling set <item> <private|internal|confidential> [--workspace <name>] [--home <dir>]\n  delphi handling clear <item> <private|internal|confidential> [--workspace <name>] [--home <dir>]\n  delphi handling reconcile [--dry-run | --apply] [--json] [--workspace <name>] [--home <dir>]", detail: "Spec 121. Every item (an imported page, a library source or claim, a finding, a provisional note) carries a set of handling traits, kept by the daemon under local/handling/ and <home>/library/handling/; a file's `handling:` frontmatter (a claim's `handling` field) is a view of that record and is never read back. Private (code name personal) is the operator's own mail, direct messages and meetings of one or two people; internal is the organisation's or team's own material; confidential is internal and limited in scope, and brings internal with it. They are not a trust tier: they change no source_type, no promotion, no tool and no private-read mark. The traits come from the arrival path ([handling] in config.toml: org_domains, list_addresses, company_wide_recipients and [[handling.path]] rows), from a marking such as CONFIDENTIAL in a header only on a path you configured (elsewhere it is noted as `says` and sets nothing), from you, from an agent or a host adding one, and from the items a record used (derived; what a span was given and did not use is a may_rest_on note). Only an internal from a configured path or from you is authoritative for the organisation's own specifics. `status` counts what is labelled and what is not, by path, and the markings on paths you did not configure, and changes nothing. `set` adds a trait; `clear` removes one (internal cannot be cleared from a confidential item until confidential is), is remembered so the rule that set it does not put it back while the content is the same, and writes handling.cleared; both refuse inside an agent seat and without a terminal. `reconcile` applies the rules to what the workspace and the library already hold, carries the traits to the records made from them, drops a derived trait whose source lost it, and puts views back; it changes no private flag, removes nothing an operator, agent or host set, and a second run changes nothing. With no flag the first reconcile is a dry run; --apply writes. Labels on work output, the ask before quoting private material, and the confidential reader grant are later slices." },
    Cmd { group: "Work", name: "validation", about: "score the labelled set; audit, report on and retract validated facts", synopsis: "  delphi validation score [--set <file>] [--results <file>] [--home <dir>]\n  delphi validation audit [--card <id>] [--workspace <name>]\n  delphi validation report [--workspace <name>]\n  delphi validation latch raise --to advisory|ratify --reason \"<text>\" [--force] [--workspace <name>]\n  delphi validation retract --all [--workspace <name>]\n  delphi validation unquarantine <fact-id> --reason \"<text>\" [--workspace <name>]\n  delphi validation more <mission> --reason \"<text>\" [--workspace <name>]\n  delphi validation queue [--workspace <name>]\n  delphi validation tier-audit [--tier substantiated|vetted|batch] [--n <count>] [--workspace <name>]\n  delphi validation tier-report [--workspace <name>]\n  delphi validation tier rollback|restore <substantiated|vetted> --reason \"<text>\" [--workspace <name>]\n  delphi validation tier threshold <substantiated|vetted|batch> <percent> [--workspace <name>]\n  delphi validation accept-all --list [--workspace <name>]\n  delphi validation accept-all [--only <id>,<id>] [--workspace <name>]\n  delphi validation accept-all --reject <claim id> [--workspace <name>]", detail: "`accept-all --reject <id>` marks one claim of the batch wrong at a terminal (it reads disproved, nothing is deleted); the decisions pane offers the same per claim.Operator-run; every subcommand refuses inside an agent seat, and `audit` needs a terminal. `score` reads the private set (default <home>/operator/validation/claims.json) and prints counts and rates only: claims scored, false-ratified, poisoned passes, wrongly-escalated rate, the rule-of-three bound and the claims new since the last scored run; it also keeps those counts for `report`. `audit` is the blind audit: it shows one claim from the validator's checks and nothing else, asks true, false or unsure, records your answer, and only then shows the sources and the verdict. A fact you call false trips the latch to advisory, withdraws PROMOTE and quarantines the facts of that batch; a would-ratify card left unanswered for seven days quarantines its fact. `report` prints the audit counts, the rule-of-three bound, the origin shares and the token use, then PROMOTE: yes or no with the conditions it missed, and writes promote.json; it never runs the validator. `latch raise` (or the console card, which asks you to type the word) is the only way up from a tripped latch: it needs a reason, starts a new audit batch, and for ratify needs PROMOTE: yes or --force, which is logged as forced. `retract --all` marks every validated fact superseded by its retraction (nothing is deleted; disproved facts are left as they are) and lists the missions recorded as having read one; it does not mark them stale. `unquarantine` returns one quarantined fact to the status it had (or contested), needs a reason and is journaled as yours; it cannot undo a fact you called false in an audit, and while a false audit holds the latch tripped it refuses until `latch raise`. At a mission's 15% share claims are kept as not validated and queued; `queue` lists them, most-referenced first, then the library's claims the validator could be offered next in the fast lane's order (a higher rung first; ids, rungs and counts only; the order never changes a claim's status; `[research.tiers] fast_lane_enabled` switches it), and `more` raises that mission's cap once (logged, yours alone). The tier audit (specs/100 section 7) is a roll-back trigger, not a promotion gate: `tier-audit` draws a random sample of the claims at a tier, stratified by source class, shows you each claim first, records your true, false or unsure, and only then shows its sources; `tier-report` prints per tier the audited count, the errors, the upper bound on the error rate (3 / sample when there were none), how many claims are not yet audited, the age of the last audit and whether the floor of 30 is met, and it gives no verdict below the floor; it also prints, per rung (corroborated, substantiated, vetted), how many claims have read the rung and how many of those were later contested or disproved, with the sample size, an upper bound at or above the floor and no verdict below it (information for changing the defaults on evidence, not a roll-back trigger; the counts start when the daemon began recording which rung a claim read); `tier threshold` is the error-rate limit you set (it has no default); a hard failure (a vetted or batch-accepted claim you call false) or an upper bound above your threshold after the floor rolls the tier back: the label falls back one rung (vetted reads substantiated, substantiated reads corroborated), nothing is deleted and every claim keeps its tags, and a tier.rollback event is written; `tier restore` is yours alone, needs a reason and starts a new audit batch. Every tier subcommand refuses inside an agent seat, and `tier-audit` needs a terminal. `accept-all` is the bulk accept of the claims that read vetted: `--list` only reads and prints them grouped by project (else scope), and what it leaves out and why (a protected class, a contested claim, a claim whose sources changed since the check); without `--list` it needs a terminal, shows the same list and a glance at three claims with their quotes (a convenience that bounds nothing; the audit is the bound), and accepts only when you type `accept all` (`accept some` with `--only`). Each accepted claim is validated by you, as one at a time, and also marked batch-accepted with the audit batch, so `tier-audit --tier batch` can sample them. It is disabled while the vetted tier is rolled back, and a roll-back flags the batch-accepted claims accepted since the last good audit re-audit due (they stay validated). Format: docs/reference/validation-set.md." },
    Cmd { group: "Work", name: "claims", about: "the library's claims: statements anchored in sources, pulled by pointer", synopsis: "  delphi claims add <text> [--kind fact|episode|procedure|attributed] [--scope personal|project|team|org] [--project <name>]\n    [--valid-until YYYY-MM-DD] [--supersedes <claim>] [--origin <page>] [--source <id> --locator <where> --quote <passage>] [--home <dir>]\n  delphi claims anchor <claim> --source <id> --quote <passage> [--locator <where>] [--prefix <t>] [--suffix <t>] [--home <dir>]\n  delphi claims list [--json] [--home <dir>]\n  delphi claims show <claim> [--json] [--home <dir>]\n  delphi claims validate <claim> [--home <dir>]\n  delphi claims confirm <claim> [--home <dir>]\n  delphi claims review [--json] [--home <dir>]\n  delphi claims pull <words...> [--project <name>] [--json] [--home <dir>]", detail: "A claim is a short statement kept once per install in <home>/library/claims/, with anchors into the library's sources: the source, version, locator, the exact quote and a little text on each side. Its status is derived by the daemon and never typed: a new claim reads not validated, and only `validate` (you, in a terminal; refused inside an agent seat) records a validation, which counts only while an anchor still holds. An attributed claim (an opinion) is never validated as a fact. A source held whole is anchored by quote in its text; one that is not (excerpts, pointer) only inside an excerpt you kept, so a claim cannot quote more of a restricted work than the excerpt bounds allow. When a source gets a new version each anchor is looked up again by its quote, then by its surroundings: found moves to the new version, not found marks the claim contested and leaves the old anchor readable under versions/; nothing is re-pointed to different text. A source marked retired contests the claims it alone supports; `sources purge` turns every anchor into it pointer-only (the quote goes with the capture, the citation and quote hash stay) and a claim no other source supports then reads not validated. `sources show` lists how many of a source's claims stood, were contested or disproved. Every mission start pulls claims from this one library by pointer, chosen by the words of the prompt and the project, ranked by status, recency and how often they were relied on; disproved and retired claims are not offered, and a project-scoped claim goes only to its project. The agent gets the id, status label, origin and score, never the text; opening it is a tainted, read-only read, and a claim that rests on a private source is withheld. `pull` shows what a mission start would be handed and records nothing. The library records which missions were handed a claim (`cited_by`). `review` lists claims that may disagree (two live claims of one kind that say nearly the same thing in different words; a word-overlap match with no model, so it may be a false alarm, and neither is picked) and claims due for review (not confirmed for 90 days, or `--valid-until` within 14 days); both are read on demand and the daemon changes nothing. `confirm` says a claim still holds: it moves only the date and cannot raise a status. The same items appear in the daily digest. `add`, `anchor`, `validate` and `confirm` are refused inside an agent seat." },
    Cmd { group: "Work", name: "library", about: "the library's pictures: which have no licence record", synopsis: "  delphi library assets check [--apply] [--json] [--home <dir>]", detail: "Every picture the library keeps has a licence record beside it in its source's record (spec 118): the picture's own address, the licence as a fixed identifier, which check decided it and where that evidence is, the author, the credit address, share-alike, when it was retrieved, the file's hash, width and height. The daemon writes it when a research run, record_research or an expanded page keeps a picture (the licence decided by code: Creative Commons BY or BY-SA, CC0 or public domain, or the host's trusted-list entry), when a workspace document's picture is captured (internal), or at bundle add from the source's licence. A picture with no record is not served to a page, the console or the app (the image read answers no licence record), and a page's check refuses it. `assets check` lists such pictures; --apply moves each to its source's assets/unlicensed/, which is never served, and deletes nothing (refused inside an agent seat). Exit codes: 0 ok · 2 usage." },
    Cmd { group: "Work", name: "pages", about: "a readable page generated from the library's claims, and dispute from it", synopsis: "  delphi pages show <topic words...> [--draft <file>] [--style apa|mla|chicago|plain] [--home <dir>]\n  delphi pages check <file> [--home <dir>]\n  delphi pages dispute <claim> --note <what is wrong> [--home <dir>]\n  delphi pages search <question words...> [--internal | --external] [--json] [--home <dir>]\n  delphi pages expand <question words...> [--json] [--home <dir>]\n  delphi pages list [--json] [--home <dir>]\n  delphi pages pin|unpin <key> [--home <dir>]\n  delphi pages new <title words...> [--home <dir>]\n  delphi pages topics [--json] [--home <dir>]\n  delphi pages topic <slug> [--version <stamp>] [--json] [--home <dir>]\n  delphi pages versions <slug> [--json] [--home <dir>]\n  delphi pages rebuild <slug> [--home <dir>]\n  delphi pages seeds --count [--json] [--home <dir>] [--workspace <name>]", detail: "A page is generated, never a source, never edited and never trusted text (spec 057). The daemon writes the banner on top (\"Generated page. Not authoritative. ...\") and, at the bottom, a bibliography and citations table with one row per cited source or claim (or \"no sources cited\", marked unsupported in the banner). The body cites with [s-source-id] and [c-claim-id] markers after the sentences or paragraph they support; the glyphs ● validated, ○ not validated, ◆ contested, ✕ disproved are derived by the daemon. Without --draft the body is the library's claims, the unvalidated ones under a visible \"Noted, not validated\" heading. With --draft a file of body text someone proposed (a model, say), the daemon runs the mechanical check and rejects the page, listing each finding, exit 1, if a marker names a missing, retired, purged, disproved or (on an external page) private record or misstates its status; a quotation is not verbatim in a cited source's stored text (checked against the recorded hash) or a quote block cites no source; a table has no citation; an image is not an asset of a cited library source (written lib: then the source id, a slash and the file name); a link is in no cited source's record; or the text carries its own banner or table. Numbers, dates and proper names found in no cited material are not rejected but reported as unverified figures. `check` runs the same check on a whole page file (banner, body, table) and prints the findings. Pages are cached by a hash of the claims they were built from, so a changed or disputed claim rebuilds them. A claim resting on a private source is left off. `dispute` is your \"this is wrong\" on a sentence: it files a contested mark with your note on the cited claim, which then reads not validated until the contest is resolved, and is refused inside an agent seat. `search` finds what the library holds for the words and has the writer role compose a page from it, which the daemon then checks mechanically and the validator checks sentence by sentence; a page that fails twice is shown as the claims alone (exit 1). --internal never touches the web; --external also searches the engines in [search] and captures a few results as noted, not validated, sources (a query that names a private source is not sent, and a private source never reaches an external page). Without either, [library.pages] external decides (ask means internal unless you say --external). Pages are cached by the material's hashes: a changed source, an expired external page ([library.pages] external_ttl) or a new matching record marks the page stale and it is rebuilt on the next search; `pin` keeps a page out of eviction. A page costs a writer call and a validator call, counted against the validator's share of the week (spec 040 §20) and [library.pages] per_day; a cached page costs nothing. Topic pages (spec 117) belong to a topic, not a search: `new` makes one (refused inside an agent seat), and a research topic makes one for its question. Each is `library/pages/<slug>.md`, rebuilt in place: `rebuild` asks the daemon now, and with [library.pages] auto_rebuild (default true) the daemon rebuilds a due page while it is idle, within per_day. A page is due when a claim or source it was built from changed, a research topic it follows gained a claim, or its lifetime passed. The page it replaces is kept under versions (`versions` lists them, `topic --version` reads one) and the topic says what changed, counted by code (\"2 statements added, 1 source replaced\"). No private record is put on a topic page or in the writer's prompt. `seeds --count` (spec 139) prints, for each topic, how many spans each source of its seed would give: the answers of the research topics it follows, the summaries and kept excerpts (at most 300 characters each) of the sources their records read, the findings of the sessions that ran that research, kept summary pages that cite those sources, ingested research files, today's search, and the live claims; then what the caps (40 spans, 64 KiB) keep. A source's own text is never counted. It reads only: nothing is written and no model is called. Exit codes: 0 ok · 1 rejected, failed or claims only · 2 usage · 4 infrastructure." },
    Cmd { group: "Work", name: "sources", about: "the library's sources: capture, excerpts, bibliography", synopsis: "  delphi sources add <url | file | doi> [--level full|excerpts|pointer] [--licence <id>] [--private]\n    [--title <t>] [--rights-holder <name>] [--attribution <text>] [--max-text-mib <n>] [--max-source-mib <n>] [--home <dir>]\n  delphi sources list [--json] [--home <dir>]\n  delphi sources show <id> [--json] [--offline] [--home <dir>]\n  delphi sources history <id> [--json] [--home <dir>]\n  delphi sources verify [<id>] [--json] [--home <dir>]\n  delphi sources archive <id | url> [--licence <id>] [--title <t>] [--level full|excerpts|pointer] [--workspace <name>] [--home <dir>]\n  delphi sources excerpt add <id> --locator <where> --purpose <why> (--text <passage> | --file <file>) [--prefix <t>] [--suffix <t>]\n    [--work-bytes <n>] [--home <dir>]\n  delphi sources excerpt list <id> [--json] [--home <dir>]\n  delphi sources bib [<id>...] [--format apa|mla|chicago|plain|csl-json|bibtex] [--home <dir>]\n  delphi sources purge <id> [--home <dir>]\n  delphi sources private list [--json] | purge [--older-than <days>] [--home <dir>]\n  delphi sources acquire status|run|batch [<batch>]|accept <batch> [<id>...]|skip <batch> [<id>...] [--home <dir>]\n  delphi sources acquire rule add --topic <t>... --archive <id>... [--max-mib <n>] | list | remove <rule> [--home <dir>]\n  delphi sources reading-list list|show <list> | add <list> <id>... [--title <t>] | remove <list> <id> [--home <dir>]", detail: "One library per install, in <home>/library/. A URL is fetched by the validation fetcher (https only, no private addresses, size cap, cloaking check). The licence decides the level: open licences and your own material are kept whole, restricted ones as excerpts, an unknown one as a pointer; --private keeps a restricted source whole for your own use and it never leaves this install. A research run keeps a page of unknown or closed licence as [library] unknown_licence says: `excerpts` (the default) keeps each passage it quoted (at most 300 characters, with up to 200 on either side) as an excerpt that is searched and anchored; `pointer` keeps no text; `private_text` keeps the whole text as a private source, quoted at most 300 characters at a time and never exported, unless the page shows a rights notice. `private list` shows that text and its size; `private purge` removes it (or what is older than --older-than days) and keeps the pointers and claims, their anchors marked text_removed; past [library] private_text_max_mib (500) the oldest goes first. Everything captured reads not validated. Limits: 64 MiB of text and 512 MiB per source, 20 GiB for the library; over a limit the capture falls back and says so. `purge` deletes the capture and keeps the citation and the content hash. Metadata is read by code, never guessed: Wikipedia (CC BY-SA, the revision from the ETag), a DOI's landing page (citation meta tags) and a Gutenberg header; a field the page does not state is left empty. `show` is an open: if the source was last checked more than 30 days ago it is fetched once through the same fetcher (no tokens; skipped offline or with --offline), an unchanged page only moves last_checked, a changed one becomes a new version with a change note and the old one kept, and a gone one (404/410) is marked retired. `archive` looks for a copy of a PUBLIC page in the Internet Archive (read-only: it asks whether a copy exists and reads it, and never asks the archive to save anything; a private source or an address that is not public sends nothing) and keeps it as a labelled version of the source: its banner says it is an archived copy from that date, not the live page, it adds no origin and no trusted source to any claim, and a claim resting on it reads checked against an archived copy. With [research] archive_fallback = true `show` also looks when it finds a public source with no readable text gone. `excerpt add` keeps one quoted passage with its locator and purpose: at most 4 KiB each, and for a restricted source not held whole at most 16 KiB in total, and at most 10% of the work when it is over 10 KB (the work's length is recorded at capture, or give --work-bytes); each excerpt carries an anchor (quote, prefix and suffix, and a character range when the text is held) that is looked up again when the source changes, and one that cannot be found is marked contested, never re-pointed. `bib` writes a bibliography from the citation fields alone, in apa, mla, chicago or plain text, or as csl-json or bibtex for a reference manager; private sources are left out of every export. `history` lists every kept capture, oldest first, with its change note (sizes and the sections that came or went, no model) and how many claim anchors point at it; a purge deletes the earlier ones. Each capture keeps the bytes as they came (`raw`, beside a text that is kept) and a record, capture.json: the SHA-256 of the bytes and of the stored text, who captured it, what made the text and in which version, and each later time the same thing was read (the same bytes, or other bytes with the same text, are a revisit on that record and not a new version). `verify` hashes every kept `raw` and `source.md` again and reports each that is not what its record states; it changes nothing and exits 1 when one differs. `add`, `purge`, `excerpt add` and `bib` are refused inside an agent seat.\nAcquisition (off until `[library.acquire] enabled = true`): once a day the daemon looks for public-domain and research sources matching your projects and `interests` in the archives listed in context/sources/archives.toml (and operator/archives.toml), and puts what it found in the digest as ONE batch, never a blocking prompt; nothing is fetched until you accept. `accept` captures the chosen items in full under the archive's own terms (the archive's rate limits, its notice and attribution kept, the public-domain basis recorded from the archive's own record, never typed by an agent); `skip` declines them for good. A standing rule (`acquire rule add`) takes anything on those topics from those archives up to a size without asking. The byte budget (`byte_budget_mib`) stops acquisition and deletes nothing; acquired sources read not validated and their claims start noted. `run` searches now without the scout's model call. A private source's tags never leave the install. `reading-list` keeps named lists of source ids. All of it is refused inside an agent seat." },
    Cmd { group: "Work", name: "route", about: "which role or agent fits a task (a dry run)", synopsis: "  delphi route \"<task>\" [--domain <d>] [--role <role>] [--limit <1-5>] [--threshold <0-1>] [--json] [--workspace <name>] [--home <dir>]\n  delphi route --adopt-eval <labeled.json> [--threshold <0-1>] [--json] [--workspace <name>] [--home <dir>]\n  delphi route --eval <labeled.json> [--json] [--workspace <name>] [--home <dir>]\n  delphi route --corrections [--json] [--workspace <name>] [--home <dir>]", detail: "--corrections counts, from the route records of every mission, how often the operator named a role other than the one the ranking chose (pairs, most frequent first); it prints no task and changes nothing.\nRanks Delphi's canonical roles and the agents of the added bundles together for a task, by name, tags, domain and summary, plus the\nlocal embedding model when one is installed. Each candidate says which role it runs as, where that runs (`locus`: host, inside or either), what\nthe task looks like it wants (`trigger`: none, research, panel, council or red_team, from a fixed table of words; research puts the scout first when no role is named), how sure the ranking is\n(`confidence`: the score, less what a close rival takes away) and whether that reaches `[routing] adopt_threshold` (`adopt`). Below it, adopt no role.\nThe default threshold is provisional, set on a small labeled set; `adopt` is a recommendation, never an order.\nNothing is dispatched, no model is called, nothing is created and no daemon is started. `--adopt-eval` takes a JSON list of {\"task\", \"expect\"}\n(`expect` is a role id, or `none` for a task that should adopt nothing) and prints how often `adopt` was right at the threshold. `--eval` takes a JSON list of\n{\"task\", \"target\"} (target is `bundle:<bundle>/<agent>`) and prints recall at 1, 3 and 5, latency and the tokens of the domain index against the\nuncapped flat list. Turn routing on with `[routing] registry = \"auto\"`." },
    Cmd { group: "Work", name: "needs", about: "what the roadmap is waiting on evidence for", synopsis: "  delphi needs [add <item> <why>] [--workspace <name>] [--home <dir>]", detail: "After the roadmap items it prints the fact validator's open work, once it has run here: the blind-audit\ncards waiting (and the oldest's age) and the PROMOTE line from the last `delphi validation report`.\nIt reads files only and never runs the validator." },
    Cmd { group: "Work", name: "kb", about: "the knowledge base: index it, search it", synopsis: "  delphi kb search <query> [--role <role>] [--untrusted|--all] [--staged] [--unreviewed] [--about] [--summaries|--bibs]\n  delphi kb search <query> --project <name> | --all-projects\n  delphi kb import <file.md | dir | -> [--home <dir>] [--workspace <name>]\n  delphi kb review [stats | show <ref> | keep <ref>…|--all | discard <ref>…|--all] | flag <ptr> --why <text>\n  delphi kb refresh [--stale [--minutes <n, at most 4>] | <id> (--still-current | <file.md | ->)]\n  delphi kb quarantine [<id> [--restore]] | extract <page-id> (--auto | <candidates.json | ->)\n  delphi kb index | reindex | stats | health [--json] | attest [<page-id>] | reads [--top <n>] | disputes [clear <ptr>]\n  delphi kb serve [--bind <host:port>] [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Inspect", name: "status", about: "workspace, daemon, models, health and mission at a glance", synopsis: "  delphi status [--workspace <name>] [--home <dir>]", detail: "Needs no terminal. Exits 1 only if `delphi doctor` finds a failure." },
    Cmd { group: "Inspect", name: "missions", about: "the workspace's missions, newest first", synopsis: "  delphi missions [--workspace <name>] [--home <dir>]", detail: "Reopen one with `delphi chat --mission <id>`." },
    Cmd { group: "Inspect", name: "changelog", about: "what the team recorded as finished, for a pull request", synopsis: "  delphi changelog [--mission <id>] [--session <s_0001>] [--since <date or time>] [--json]\n    [--workspace <name>] [--home <dir>]", detail: "Prints Markdown by session, from the mission's log (the latest unless --mission). --json: one line per change." },
    Cmd { group: "Inspect", name: "release", about: "whether this build is a signed release; assemble the release notes", synopsis: "  delphi release [status]\n  delphi release changelog [--check | --write <version> [--date <YYYY-MM-DD>]] [--repo <dir>]", detail: "status: signed by the maintainers' key, unsigned (a local or development build), or invalid. Needs no terminal. changelog (specs/036 section 9): reads the fragments under changelog.d/ in a checkout (one file per change: a `section:` line, Added, Changed, Removed, Fixed or Security, then a Markdown list item) and prints the next release's section; --check only checks them (exit 1 on a malformed one, as `delphi lint` does); --write <version> puts the section above the newest release in CHANGELOG.md, dated --date or today (UTC), and deletes the fragments it used, refusing a version CHANGELOG.md already has and writing nothing when any fragment is malformed. The release pull request runs it; no other command or workflow does." },
    Cmd { group: "Inspect", name: "council", about: "follow a Council or Panel from a second terminal", synopsis: "  delphi council watch [<council_id>] [--mission <id>] [--once] [--interval <secs>]\n    [--workspace <name>] [--home <dir>]", detail: "Reads the mission's event log: asks no daemon, calls no model, writes nothing. With no id it follows\nthe latest Council or Panel in the workspace's missions (host sessions too), one still running first.\nPrints the planned seats, each fixed-form line as it happens (`HH:MM:SS role kind name`) and a\nprogress line every --interval seconds (default 10, 2 to 300) when something changed; no seat text.\nExits 0 when it is decided, needs the operator, is not decided, failed or was cancelled, printing the\nstate and the record's path. --once prints the current state and exits." },
    Cmd { group: "Inspect", name: "sessions",about: "the finished console sessions, to review later", synopsis: "  delphi sessions [--mission <id>] [--limit <n>] [--full] [--json] [--home <dir>]", detail: "Written when a session ends (/new, or a daemon restart): the first words of what was asked, the changes recorded, and the summary when [session] summary is on. Reads the central log; calls no model." },
    Cmd { group: "Inspect", name: "research", about: "set up a search engine; research topics, logs, ingest, suggest; retractions", synopsis: "  delphi research topics [--due] [--json] [--home <dir>] [--workspace <name>]\n  delphi research show <topic-id> [--json] [--home <dir>] [--workspace <name>]\n  delphi research continue <topic-id> [--note <text>] [--json] [--home <dir>] [--workspace <name>]\n  delphi research log <topic-id | research-id> [--json] [--home <dir>] [--workspace <name>]\n  delphi research migrate [--apply] [--json] [--home <dir>] [--workspace <name>]\n  delphi research ingest <path>... | --all [--apply] [--fetch] [--json] [--home <dir>] [--workspace <name>]\n  delphi research retractions import <file> [--home <dir>] [--workspace <name>]\n  delphi research retractions status [--json] [--home <dir>] [--workspace <name>]\n  delphi research suggest [--limit <n>] [--json] [--home <dir>] [--workspace <name>]\n  delphi research setup [--engine brave|mojeek|searxng|kagi|google|parallel] [--credential <env:NAME|keychain:name>]\n    [--url <instance>] [--cx <id>] [--no-test] [--home <dir>] [--workspace <name>]", detail: "`topics` lists the research records on disk, newest first: one per topic, in research/topics/<topic-id>/ of the workspace, or of a registered project when the run was asked from inside it (specs/112 §2). A topic's id is a slug of the question and eight hex characters of its hash, so the same question asked again is the same topic. `show` prints one record: the question, who asked, the state, each run, every query with its provider and count of results, every page read with its source id and content hash, the pages that were not read and why, the claim ids, the gaps, and the answer of the newest finished run. The record is written as the run goes, so a run that was stopped shows how far it got; a finished run's result is read from it for as long as it exists. Everything printed is data from outside and has not been checked. `continue` asks the running daemon for a new run on a topic that has a record (specs/112 §5): it asks the topic's own question, sends the queries the record's gaps call for (the sources on your lists when they found nothing, the wider web when it found nothing, all of them when coverage was thin or the budget ran out or the record has no gap), and does not read again a page the record holds whose content has not changed; `--note` is one line of at most 120 characters that narrows the search. It spends tokens and reaches the network, so it runs only at a terminal with the daemon started, and is refused inside an agent seat; an agent asks with its request_research tool, and a host passes topic_id to request_research. `topics --due` lists the topics that are due and why: the newest run is older than the topic's refresh days ([research] refresh_days, default 90), or a source it rests on has changed or been retracted. It only lists: nothing is run again until you or an agent asks. Each claim a run keeps is written to the library (library/claims/<id>.json) with an anchor per source (the source, the quote found in it, the page's address), the project when the run belongs to one, and the topic; a claim the library already holds gains the anchor and the topic instead of a second claim, and every such claim reads noted until the library's own rules say otherwise (specs/112 §3). `migrate` does the same for the claims an older research/host-claims.jsonl lists, which were kept only in that file: it prints what it would write and changes nothing, and writes only with --apply (refused inside an agent seat); running it again adds nothing, and the file itself is never changed. `ingest` (specs/113 §3) turns research that already exists as files into topics: each Markdown file named (or, with --all, every registered project's research/ folder and the workspace's imported summaries) becomes a topic record with runner ingest, the file a private source of kind file with its hash (its locator, title and date, never its text), and its body the answer by reference, not by copy; a file whose hash is already recorded is skipped and a changed file adds a run to its topic. Up to 20 sentences of each file become claims anchored to their line of the file, kept as ingested from a file, private to the project and counting for no corroboration; the URLs the file lists become source pointers, and with --fetch each one on the lists of sites research may read is read by Delphi's own fetcher and kept, and a claim whose sentence appears on that page gains a second anchor there (without it nothing is fetched). A file that lists no source is flagged `no sources listed` in its record, and an imported summary that lists none says so in search results. It prints what it would do and writes nothing without --apply (refused inside an agent seat). `log` prints what a research run searched, of whom, when, and how many results came back and were kept (specs/101 §3): one line per search with the query that was sent, a search withheld for a private claim without any query text, searches your host declared marked as such, and a found / duplicates / kept / not-kept flow that adds up. Written into the topic's folder (search-log.json) once the job has finished, for the topic's newest run; a log written before topics existed is still read from research/search-logs/. A private log stays on this machine. `retractions import` reads the Retraction Watch CSV from a file you downloaded (the repository is gitlab.com/crossref/retraction-watch-data; a URL is refused, because Delphi makes no call to fetch it) into a local index; when [research] retractions is on it then checks every source that has a DOI against it, marks a retracted source (it stops counting as an origin and a claim only it supports becomes contested) and writes nothing off: the capture stays readable (specs/101 §4). `retractions status` prints the snapshot's date, size and age, and says stale after [research] retractions_stale_days (default 90). `suggest` lists the questions asked of the knowledge base most often and not answered (specs/114 section 6): the searches by the console, an inside agent or a host that found nothing (local/kb-misses.jsonl), grouped by their words, the most asked first and then the newest, each with how often, when last and by whom. It prints only: nothing is researched, no daemon is needed, and it is refused inside an agent seat. Apart from `continue` and `ingest --fetch`, none of these calls a model or the network.\n\n`setup` (specs/119 section 2) chooses the search engine research asks. At a terminal it offers Brave first and Mojeek second (any supported engine is taken), prints one line before a key is entered (queries leave this machine for the engine chosen; check its terms and your employer's rules first), takes the key with hidden input into your keychain and never into a file, writes the `[search]` table of config.toml, runs one test query and one test fetch of a site research may read (`--no-test` skips both), prints what worked, and shows the change for you to ratify: `[search]` names who receives your queries, so it takes effect only once ratified. A key the engine rejects is not stored and nothing is written. Without a terminal it asks nothing: pass `--engine`, and for an engine with a key `--credential` with a reference (`env:NAME` or `keychain:name`, never the key itself); it then writes the table, runs the test and does not ratify. It is refused inside an agent seat. Exit codes: 0 set up · 1 a test failed or nothing was saved · 2 usage or refusal." },
    Cmd { group: "Inspect", name: "tools", about: "how tools are used, and how often models record their changes", synopsis: "  delphi tools [--mission <id> | --all] [--json | --prom] [--workspace <name>] [--home <dir>]", detail: "Per model: runs that changed something and recorded a change. --prom: Prometheus text. Counts only." },
    Cmd { group: "Set up", name: "customize", about: "teach Delphi a note, a rule, a skill or a profile line yourself", synopsis: "  delphi customize note \"<text>\" [--mission <id>]\n  delphi customize rule \"<text>\" [--scope workspace|operator] [--why \"…\"] [--replace <ID> | --remove <ID>]\n  delphi customize skill <dir> --agents <role,role> [--description \"…\"]\n  delphi customize profile [\"<line>\" | --file <path>]\n    [--yes] [--workspace <name>] [--home <dir>]", detail: "What you write is yours: a rule, a skill or a profile line is ratified as you write it, after the same checks a proposal gets (no relaxing a rule, the caps, no near-duplicate). `note` only stages a note for review. Each verb needs a terminal; `--yes` skips the typed yes, not the terminal. A rule applies from the next task." },
    Cmd { group: "Inspect", name: "retro", about: "rules the Orchestrator proposed, waiting for your review", synopsis: "  delphi retro list [--mission <id>] [--json]\n  delphi retro show <mission>/<id> [--json]\n  delphi retro stale [--json]\n  delphi retro recurring [--json]\n  delphi retro mine [--mission <id>] [--json]\n  delphi retro stats [--mission <id>] [--json]\n  delphi retro accept <mission>/<id> [--scope workspace|operator]\n  delphi retro reject <mission>/<id> [--reason <text>]\n  delphi retro promote <project-id> <record>... --summary <text> --mission <id> [--uncertainty <text>]\n  delphi retro promotions [--json]\n  delphi retro withdraw <entry> [--reason <text>]\n    [--workspace <name>] [--home <dir>]", detail: "Proposals are made with the Orchestrator's `suggest_rule` tool and are never loaded into a prompt until you ratify them. `accept` and `reject` need a terminal and a typed yes, and `accept` needs the daemon stopped; `--scope` ratifies at a different scope than proposed. A promotion (`promote`) is a proposal to keep a summary of eligible project memory records as home memory: it records the summary, the records, their hashes and the home records it was checked against, and shows any duplicate or conflicting home entry without merging it. Accepting one runs the checks again and refuses if a record became private, retired or secret-bearing or if the proposal or home memory changed since; the entry keeps its provenance and its source tier and validates nothing. `reject` records only the outcome and your reason. `withdraw` retires a promoted entry with an audit reference and deletes nothing. `stale` lists staged notes past their `valid_until` date and rules files not confirmed within 90 days; it changes nothing. `recurring` (also shown at the end of `list`) counts, from a read-only scan of the mission logs, the staged claims that more than one daemon-assigned mission produced, with Council claims grouped by seat; a count a host reported is labelled `reported by N host sessions` and never ranks above a note your own work produced; it is a number to read and never proposes or writes a rule. `mine` ranks the failed or looping spans of a mission (a loop stop, a red gate, a denied card, an escalation, repeated tool failures) by a cause computed from event kinds and codes, never from text, and writes the report to the mission's `reports/trace-mining.json`; it applies nothing. `stats` shows, for each rule and skill that tasks carried, how often those tasks went first pass, with the count and a 95% interval, `not enough data` below 10 finished build tasks, and the comparison with tasks that did not carry it labelled as co-occurring, never causing; one task in five is a holdout counted apart. It changes nothing." },
    Cmd { group: "Inspect", name: "agent", about: "promote or nerf a role: the class of model it is assigned", synopsis: "  delphi agent promote|nerf <role> [--kind <task kind>] [--reason <text>]\n  delphi agent grades\n  delphi agent clear <role> [--kind <task kind>]\n    [--workspace <name>] [--home <dir>]", detail: "A grade (-2 to 2) is kept in the ratified [grades] table. promote and nerf edit config.toml and show the diff; nothing changes until you /ratify it.\ngrades lists them with the measured results beside each, the ratchet of each plan-following role (the class its builds start at) and the suggestions from the results, as counts. Delphi may suggest a grade and never applies one.\nclear lets the ratchet try again a class it stopped after two failures in a row (kind `build` unless --kind says otherwise). `--auto` runs one bounded session for as long as the command runs: it merges queued pull requests oldest first, one at a time, each re-checked just before it merges, and stops at the first of: the cap on merged pull requests (default 10), the time cap (4h), a STOP file at $DELPHI_HOME/merge/STOP, Ctrl-C, 2 errors in a row, or 30 idle minutes. It never merges a pull request that changes a gate (those are left for a person at a terminal) and cannot change its own limits while it runs. A chat quote must ask for automatic merging; the number of one pull request or `all` does not. Refused inside an agent seat." },
    Cmd { group: "Inspect", name: "test", about: "requests to authorize a test in advance: review, authorize, revoke", synopsis: "  delphi test review [<id>] [--expand]\n  delphi test authorize --from-request <id> --run <run> [--purpose <text>] [--expires 2h] [--uses 10] [--drop <entry number>]... [--no-pin-tree] [--show]\n  delphi test authorize --run <run> --intent <text> --purpose <text> --entry-json <json>... [--expires 2h] [--uses 10] [--no-pin-tree] [--show]\n  delphi test refuse <id>\n  delphi test list\n  delphi test revoke <id>\n    [--workspace <name>] [--home <dir>]", detail: "An agent (the request_test_authorization tool) or a host (the MCP tool of the same name) may ask you to authorize, in advance, a few exact commands for a future headless test. A request is staged inert at tier L0 and grants nothing; it can never be used by the run that wrote it. An authorization lets a headless run that you launch yourself with `--on-card authorization:<id> --auth-run <run>` run exactly the matching commands in the sandbox; you give a fresh authenticator code again at launch; nothing else runs.\n`review` with no id lists the pending requests. With one it shows, in this order: (1) the manifest Delphi renders from structured fields (argument vector, binary, scripts it executes, provenance; no shell string, glob or wrapper); (2) a dry run against recent recorded cards; (3) a diff against your earlier grants; (4) who asked, the taint (computed by the daemon from the span; a host request has none and is fixed at federated_peer) and the span; (5) the requester's own words, collapsed and stripped of control and escape characters (`--expand` shows them, as data).\n`authorize` works at a local terminal only: it refuses over a pipe, a script, a relay or inside an agent seat, and it refuses when no second factor is configured ([relay] step_up = \"totp\" with a totp_credential in the ratified configuration, see `delphi relay totp-enroll`); it never falls back to no step-up. For a request from a host you retype each literal command (nothing is prefilled, nothing can be confirmed instead); for an agent span below internal_empirical you retype it or type CONFIRM; then you give the authenticator code. You may drop entries (`--drop`), shorten `--expires` (default 2h, at most 24h) and `--uses` (default 10, at most 50), and you choose the run id (`--run`, required). It writes a new record that stores the request's id and hash, into one file under the Delphi home whose hash is pinned in the operator ratified.lock; a copy of that file anywhere else, or one whose hash does not match, is ignored and logged. Entries that could merge, open a pull request, use a credential, reach the network, dispatch another agent, or write to .git, hooks, configuration or the store are rejected when the record is made. The first slice admits one kind of action: a sandboxed shell command run as `/bin/bash <absolute script> [fixed arguments]` (the script pinned by hash; no -c, -s, -i, --rcfile or --init-file; python3 and perl are not admitted yet).\nPaths that do not exist yet are written as tokens (spec 092 S2d2a): a run's snapshot, scratch directory and canary are made when it starts, so an entry names them as whole elements, `@snapshot`, `@snapshot/<relative path>` (one tracked file; the only token a pinned file may use), `@scratch`, `@canary` and `@operator_home` (a decoy, never your real home), and nothing else resolves; a token is never part of other text, never `argv[0]`, and a `<relative path>` has no empty, `.` or `..` component and only A-Z a-z 0-9 . _ + - (128 characters each). The record stores that text, a run's card must equal it exactly, and only then does the daemon put in the paths it made; a card never supplies a path. `authorize` runs from the project's directory, shows the fixed token table, each pinned file's size and blake3 hash, and with `--show` the file's content (at most 64 KiB of each, with a note when cut; the hash covers the whole file), refuses a pinned file that is untracked, modified or a symlink, and has you RETYPE the symbolic text (nothing to confirm). The snapshot's tree hash is pinned too, so a run whose project differs from this one is refused; `--no-pin-tree` opts out and then only the pinned files are checked, so every `@snapshot/<rel>` argument must itself be a pinned file. The tree hash you are shown is the one stored: if a tracked file changes while you read, nothing is authorized. `refuse` closes a request (its manifest is not staged again for 24 hours). `revoke` is irreversible and takes effect at the next card. `list` shows pending requests and records. Every step is written to a hash-chained audit log by this command (before the change it announces), never by an agent; a log or ledger that fails its checks stops every command with a message, and `delphi doctor` reports it. Requests are limited over the whole workspace (a new session id buys no new budget), and only the red-team, investigator and orchestrator may ask. The store is written so that a crash cannot leave it unreadable: a leftover `.next` file beside the store is finished by the next command, and nothing needs repairing by hand. Refused inside an agent seat." },
    Cmd { group: "Set up", name: "teach", about: "teach a role a lesson yourself, at a terminal", synopsis: "  delphi teach <role> \"T: <when> D: <what to do> [F: <what would refute it>] [G: <why>]\"\n    [--goal \"<why>\"] [--scope workspace|operator] [--workspace <name>] [--home <dir>]", detail: "Writes a lesson for one role (its `lessons.md`, workspace scope unless `--scope operator`, which follows you into every workspace), with its goal in the lesson's Why: line, recorded as user_ratified with the origin operator-terminal. It shows the lesson as it will be written and writes only after you type yes at a terminal: there is no --yes, and input that is not a terminal is refused. It runs the checks every accept runs: secret shapes are masked, the no-relaxation test (the goal too), 350 characters a lesson, the Rule of 25 (the 26th is refused with the three ways to make room) and the duplicate test; a lesson that overlaps one the role carries is named, and your yes keeps both. The write is the pinned write `learn keep` uses, so it works while the daemon runs; the role's next pack carries the lesson and its pack_hash changes. The teach is recorded in the lessons log (`local/lessons-log/`, or `operator/lessons-log/`). `delphi learn lessons` lists what a role carries; `delphi learn unteach` takes a lesson back." },
    Cmd { group: "Inspect", name: "learn", about: "notes staged from denials, corrections, retries and escalations", synopsis: "  delphi learn list [--mission <id>] [--json]\n  delphi learn show <mission>/<id> [--json]\n  delphi learn drop <mission>/<id>\n  delphi learn wrong <mission>/<id>\n  delphi learn keep <mission>/<id> --as lesson --role <role> | --as rule\n    [--scope workspace|operator] [--text \"T: <when> D: <what to do> [F: …] [G: <why>]\"] [--goal \"<why>\"]\n  delphi learn lessons [<role>] [--json]\n  delphi learn unteach <role> <lesson id>\n  delphi learn ingest --from claude-code --role <role> [--path <file|dir>] [--enable]\n    [--workspace <name>] [--home <dir>]", detail: "Staged notes are never loaded into a prompt; they wait for review. `drop` needs the daemon stopped. `list` shows the staged notes, then the notes of the provisional tier (unreviewed notes a later run of the same role may be given as data; `[memory] provisional`), the most used first and then the oldest. `keep` turns an agent's or a host's note into a lesson for one role (its `lessons.md`, workspace scope unless `--scope operator`) or a shared rule (`heuristics.md`), recorded as user_ratified with where it came from: it shows what it will write and writes only after you type yes at a terminal (there is no --yes), and refuses text the no-relaxation test or the Rule of 25 refuses. For a provisional note it works while the daemon runs (the write is a compare-and-swap on the file's pin) and the note moves to `provisional/kept/`; a note that is only staged is closed in its session's log, so that needs the daemon stopped. `wrong` marks a provisional note wrong: it moves to `provisional/rejected/`, is no longer retrieved, and the same note is not kept again for 180 days; nothing is deleted. A lesson needs a trigger and a directive (`T: … D: …`); its goal, `G:` after the directive or `--goal`, is written into its Why: line, and its origin (host-reported or agent-note) comes from where the note was staged, never from its words. `list` shows each note that can become a lesson as a card: its kind (`recurring` for a host note reported by two or more sessions, a count the host supplies; otherwise `other`), role, goal, origin, recurrence, expiry (seven days or fewer is marked) and the exact command that accepts it. `lessons` lists what each role carries, from the ratified copies its pack reads, with goal, origin, age and where it came from; serves are not counted yet. `unteach` takes a lesson back with the same pinned write, after you type yes at a terminal, and records `learn.unlearned` with its id, the date, its goal and its words in the lessons log (`local/lessons-log/`, or `operator/lessons-log/` for an operator lesson), so teaching the same words again restores it; it works while the daemon runs. Capture is set in [learning] in config.toml. `ingest` reads a coding tool's own session logs after the fact and stages what fixed rules find (your corrections, failures that were retried) as L0 notes for one role; it is off for a project until you run it once with `--enable` from inside that project, reads the current folder's logs unless `--path` says otherwise, and needs the daemon stopped." },
    Cmd { group: "Inspect", name: "capabilities", about: "what each model CLI here can use: connectors, skills, models", synopsis: "  delphi capabilities [--refresh] [--ask] [--json] [--quiet] [--models] [--project <dir>]... [--home <dir>]", detail: "--models lists every discovered model with its class (spec 056; takes --workspace); one no longer\nlisted is kept as gone.\nAsks each CLI's own list commands and reads the skill directories; saves <home>/capabilities.json.\nSetup and update refresh it in the background. --ask also asks the claude and agy models what they can\nuse; that answer is shown apart and is not verified. Calls no model otherwise.\nA refresh reads Codex's rate-limit meter and, when the workspace sets [harness.copilot]\nquota_source = \"gh\", Copilot's pool; neither calls a model, and nothing decides on them." },
    Cmd { group: "Inspect", name: "logs", about: "the error log: what went wrong, with context", synopsis: "  delphi logs [--lines <n>] [--since 1h] [--level error|warn|debug] [--component <name>] [--mission <id>] [--json] [--follow] [--path] [--home <dir>]", detail: "Reads $DELPHI_HOME/logs/delphi.log.jsonl, written by the CLI, the daemon and the console: errors and panics by default, redacted, 10 MB x 3 files.\nDELPHI_LOG=off|error|warn|debug or [logging] level in <home>/operator/config.toml sets what is kept. Read-only and offline. --path prints the file's location.\nComponents: cli, daemon, engine, harness, provider, ui, xenia, relay, tool." },
    Cmd { group: "Maintain", name: "merge", about: "merge this repository's queued pull requests, with a recorded approval", synopsis: "  delphi merge <PR>... [--operator-said \"<the operator's words>\"] [--wait 40m | --no-wait] [--dry-run] [--home <dir>]\n  delphi merge --auto [--operator-said \"<words>\"] [--max-prs <n>] [--for 4h] [--idle 30m] [--pause-after-errors <n>] [--poll 60s] [--batch[=N]] [--home <dir>]\n  delphi merge --all [--operator-said \"<words>\"] [--wait 40m | --no-wait] [--dry-run]\n  delphi merge --all | <PR>... --batch[=N] [--operator-said \"<words>\"] [--dry-run] [--home <dir>]\n  delphi merge queue <PR>\n  delphi merge status\n  delphi merge log [--open] [--pr <n>] [--json] [--home <dir>]\n  delphi merge fix <PR> [--done | --needs-human \"<why>\"] [--home <dir>]\n  delphi merge refresh <PR> [--home <dir>]\n  delphi merge repair-base [--base main] [--dry-run] [--operator-said \"<words>\"] [--home <dir>]\n  delphi merge cleanup [<PR>...] [--merged] [--dry-run] [--yes]", detail: "The core-repo merge flow (specs/094). Agents queue a ready pull request with the `merge-queue` label (`delphi merge queue <PR>`); a merge is started by the operator.\nFor each pull request the command derives, from git and `gh`, the head commit, the changed paths and their hash, the base commit, the checks and the linked spec (the `Implements-Spec:` trailer); nothing is typed. It refuses unless the pull request is open, not a draft, queued, mergeable, every check passes, the spec is approved on the base branch (or the change is inside the bounded exemption of specs/003), and its head contains the tip of the base: CI tested the branch against the base as it was at the last push, so a head that is behind (the compare API's `behind_by` above 0) is refused, with the count, until it is refreshed and CI has run on the new head. The queue does not require that. A refusal lists every reason.\n`refresh <PR>` updates a pull request that is behind: in a temporary worktree under the Delphi home it merges the base into the pull request's head (a merge commit, never a rebase), settles only conflicts in the compiled spec views and the spec groups file (the groups file by keeping both sides' lines, the base's first; the views by taking the base's side; the spec tracker is not settled since specs/135 E3: its slices are per-slice files), always reruns the spec compile and checks it, so every generated view is what the compile writes for the merged specs (even where the merge had no conflict), checks that what the pull request changes outside the generated files is exactly what it changed before, and pushes one commit with a plain push (never a force; a branch that moved meanwhile rejects it). Any other conflict, a gate path, more than 10 conflicted files, a failed compile, a widened change or a rejected push stops it with nothing pushed and an error entry of class `refresh-refused` for `merge fix`. It refuses a draft, a fork's pull request and a gate-changing one, never merges anything and needs no approval of its own. Run it in the repository.\nAfter each merge, at `status` and at each --auto poll, the command also checks the base: it fetches it and runs the spec compile check on its tip in a temporary worktree (no build). When the committed compiled spec views are not what the compile writes (drift, which can enter by any route, a native merge included), it writes an error entry of class `base-drift`, and --all and --auto merge nothing more until a check finds the base current again; `<PR>` says so and goes on. `repair-base` makes the repair: from the base tip (default `main`) in a temporary worktree it runs the spec compile, commits only the compiled spec tree, pushes a new branch with a plain push and opens a pull request \"Recompile compiled/specs on main\". It asks you first (type `repair`, or an agent passes your words asking for a repair with --operator-said), --dry-run only lists the files, and it never merges: you merge the repair once CI is green.\n`<PR>` and --all refresh a pull request themselves when it is behind and otherwise ready (green, queued, not gate-changing): once the approval is in hand they refresh it as above, wait for CI on the new head (--wait, default 40m, at most 4h; --no-wait refreshes and stops), and merge the new head only when every check passed on it and it is still current. A base that moves again while it waits gets one more refresh; after two in one run it stops and says so. A pull request that is behind and red, or behind with checks running, is refused and not refreshed. The approval carries over only to a head the same run made (the ledger row says `carried_from`); a head that moved any other way needs you again. --all does this for each pull request in turn, so N pull requests take about N CI runs. --auto refreshes it and moves on, and merges it at a later poll once CI is green; a refresh that fails counts as an error. `status` says which ones will be refreshed at merge. A dry run never refreshes.\n--batch[=N] (specs/094 §13; N from 2 to 8, default 4) merges several pull requests through one rollup pull request. From the queued pull requests (--all) or the ones named, in order, it chooses up to N that are ready except perhaps for being behind and do not change a gate (the others stay on the serial path, each with its reason), shows them with their heads and specs and asks you to approve the batch (type `all`; an agent's quote must name every number, or say all/every/each for --all). It then builds in a temporary worktree under the Delphi home a branch from the base tip with each one's approved head merged in turn (one merge commit each; conflicts settled only as `refresh` settles them, the earlier side first; the spec compile after each), checks that the result changes exactly what those pull requests change outside the generated files and touches no gate path, pushes it with a plain push to a new branch `delphi-rollup/<id>` and opens a pull request whose body carries one manifest (the base commit and each pull request's head and spec). --dry-run only prints the candidate and its manifest. `delphi merge <PR>` recognises a rollup only by its branch, exactly one manifest and a rebuild from that manifest (on the current base tip) whose tree equals the rollup's; anything else is an ordinary pull request again and refused as a cross-spec bundle (class `rollup-refused`), and a pull request in it whose head moved makes it stale (`rollup-stale`). Once the rollup's checks are green (`ci-gate` among them; it waits as --wait says) and every pull request in it is still open at its approved head, it merges the rollup with a merge commit at the head it opened, after an `approved` ledger row for each pull request in it (its own spec and head, `carried_into` the rollup), then a `merged` row each. GitHub marks each one merged; one it does not mark whose head is in the base is closed with a comment linking the rollup. Each branch is then cleaned up as after any merge. A red, pending or stale rollup is never merged. A red one (its `ci-gate` failed; CI already retries a failing test once) is bisected at once: it is closed with a comment and its branch removed, a rollup of each half is built from the base tip on the same approval (only those pull requests; adding one needs a new approval) and opened, and both are awaited; a green half merges as above and a red one is split again on the new tip, for at most two rounds. A pull request red on its own gets an entry of class `rollup-red` for `merge fix`; both halves green while the whole was red puts every pull request in it on the serial path with `rollup-red`; what two rounds do not isolate goes back to the serial path (it stays queued for --all without --batch). A half whose checks do not finish within the wait stays open, and `status` lists it as bisecting. `delphi merge <rollup>` merges a rollup opened earlier on the same terms once you approve its pull requests again (type `all`, or a quote naming every number); --all never merges a rollup. `--auto --batch[=N]` does the same inside the bounded session, on the session's approval and nothing more: only queued pull requests it would merge one at a time are batched. At each poll it looks at its rollups in flight without waiting (a green one merges, a red one is bisected, two halves are decided together once both have finished, one whose base moved is built again), then, with nothing in flight and two or more that may be batched, opens one rollup of up to N, never more than the merges left under --max-prs (each pull request in it counts); with one it takes the serial path. While a rollup is in flight a pull request that could join a batch waits for the next one. A `rollup-red` entry counts toward --pause-after-errors. Rollups open when the session ends stay open, named in the summary; `status` lists them and the latest batching session. A session has no dry run.\nApproval is `tty` (you type the PR number; `all` for --all) or `chat-quote` (an agent passes your own words with --operator-said; the quote must name the pull request, or say all/every/each for --all). A chat quote is self-attested by the caller and is recorded as such. The merge is a merge commit, made only if the head is still the one derived, and the decision is appended to $DELPHI_HOME/merge/ledger.jsonl and posted on the pull request.\nA pull request that touches a path in merge-gates.toml (read from the BASE branch; the file is on its own list) changes a gate: it is never queued, never merged by --all, refuses a chat quote, and needs you at a terminal, shown the whole diff. --dry-run changes nothing.\nAfter a merge the command removes what belonged to the pull request and prints one line per step: the remote branch (kept if another open pull request is based on it, or its tip is not the merged commit), the worktrees that have the branch (kept, and reported, when locked, dirty, the main worktree, or the directory you are in; never --force), then the local branch with `git branch -d` only. The base, default, main and master branches and any pull request that is not MERGED are never touched. It also releases the slice claims (specs/135, `delphi forge claim`) of this repository held by the merged branch, one line each. A cleanup failure, an unreadable claims file included, is printed and never undoes a merge. Before it merges (and in a --dry-run), a live claim another branch holds on the same spec is printed as a note; a claim never refuses a merge. `delphi merge cleanup <PR>...` does the same for merged pull requests; `--merged` sweeps the backlog of recent merged pull requests whose branch still exists, and only prints the plan until you add --yes.\n\nWhen a pull request cannot merge for a reason somebody must act on (a conflict, a red check, no checks, an unapproved spec, a head behind the base, a head that moved, a `gh` failure), an entry is written to $DELPHI_HOME/merge/errors.jsonl and the central log (`delphi logs`); waiting (a draft, running checks) is not an error. The same problem on the same head is one more attempt, not a new entry, and a merge resolves the entries open for it. `log` shows them and the latest ledger rows. `fix <PR>` prepares a worktree on the pull request's branch (or points at the one where it is already checked out), writes a brief for the builder (the git-merge-and-conflict-resolution skill; never a rebase or force-push; stop on a gate file, more than 10 conflicting files or red tests) and moves the entry to `fixing`; the builder reports `--done` once it has pushed (the head must have moved; the earlier approval no longer applies) or `--needs-human` with the reason.\nRefused inside an agent seat. Needs the GitHub CLI (`gh`) signed in. Exit codes: 0 ok · 1 refused, declined or failed · 2 usage or refusal · 4 `gh` missing or unreachable." },
    Cmd { group: "Inspect", name: "debug", about: "pack a redacted slice of the log for sharing", synopsis: "  delphi debug bundle [--since 24h] [-o <file>] [--mission <id>] [--workspace <name>] [--home <dir>]", detail: "Writes one .tar.gz (mode 0600, never over an existing file): the log slice, the `delphi doctor` report, version and OS, the config's key paths with values\nredacted, and the last events of the mission (types and short status fields only; no prompts, outputs or file contents). Prints what it included. Read-only and offline." },
    Cmd { group: "Inspect", name: "doctor", about: "check the workspace, its files and its models", synopsis: "  delphi doctor [--fix] [--workspace <name>] [--home <dir>] [--require-providers]", detail: "--fix asks before each repair and needs a terminal." },
    Cmd { group: "Inspect", name: "models", about: "the model pool: usable, out of quota, cooling down", synopsis: "  delphi models [--probe] [--workspace <name>] [--home <dir>]\n  delphi models calibrate <endpoint>/<model> | --all-at low|standard|premium\n    [--concurrency 1|2] [--dry-run]", detail: "calibrate (spec 056 M5) SPENDS tokens and premium requests: ten seeded-defect review cases, one bare call per\nmodel. It prints the calls and cost first, and runs only on y or yes typed at a terminal (no --yes; not in a seat).\n--all-at standard is the 1x-and-below run. Cached cases are free. Counts feed routing only, never a prompt;\n`delphi models` shows them and any class proposal, which you ratify in model-classes.toml." },
    Cmd { group: "Inspect", name: "daemon", about: "the background process: status, stop, restart", synopsis: "  delphi daemon status|stop|restart [--workspace <name>] [--home <dir>]", detail: "" },
    Cmd { group: "Inspect", name: "lint", about: "check a repository's Delphi files", synopsis: "  delphi lint --repo <path>", detail: "" },
    Cmd { group: "Work", name: "forge", about: "claim a spec slice before building it; finish it before its PR", synopsis: "  delphi forge claim <spec> <slice> [--by <holder>] [--ttl <dur>] [--take] [--local] [--repo <dir>] [--home <dir>]\n  delphi forge claim --spec-number [--by <holder>] [--ttl <dur>] [--local] [--repo <dir>] [--home <dir>]\n  delphi forge release <spec> <slice> [--by <holder>] [--local] [--repo <dir>] [--home <dir>]\n  delphi forge release --spec-number <NNN> [--by <holder>] [--repo <dir>]\n  delphi forge list [--all] [--json] [--sweep] [--local] [--repo <dir>] [--home <dir>]\n  delphi forge finish [--check] [--gates [--fast-only]] [--spec <spec> --slice <slice>] [--body] [--repo <dir>] [--home <dir>]\n  delphi forge tracker [--spec <spec>] [--repo <dir>]\n  delphi forge tracker migrate [--spec <spec> | --section <heading>] [--dry-run] [--repo <dir>]\n  delphi forge ready <PR> [--wait <dur>] [--repo <dir>]", detail: "Spec 135. claim records that a slice of an approved spec is being built, in <home>/forge/claims.json under a lock, so another session that claims it is refused and told who holds it (exit 1). A claim belongs to a holder (--by, by default the branch checked out at --repo) and lasts --ttl (default 8h); claiming again renews it, --take takes over a live claim of another holder (printed and recorded), and an expired claim is replaced. A draft spec is refused; the tracker is read for warnings only (a slice it does not list, a slice it says is merged). When the checkout has an origin remote the claim is also a ref, refs/delphi/claims/<spec>/<SLICE>, pushed with a lease (a compare-and-swap), so sessions on other machines see it and are refused too; it names the holder, a random machine id kept in <home>/forge/machine-id, the branch and the times. A remote that cannot be reached never fails the command: the claim is kept on this machine with a warning. --local skips the remote. A claim binds only sessions that ask; nothing refuses a pull request for a claimed slice, and nothing reads the refs to allow anything. release removes your claim (and your ref, or an expired one); list shows the live claims of this repository, then the remote ones (--all: every repository and the expired ones); --sweep first deletes remote claims that are expired, whose branch merged into origin/main (a merge commit, or, for a github.com origin, a merged pull request of that branch by `gh pr list --state merged --head <branch>`, so a squash merge counts), or whose reserved spec number is now on origin/main. claim --spec-number reserves the next free spec number (above specs/ on origin/main, this checkout and live reservations) as refs/delphi/claims/spec-number/<NNN> for --ttl (default 7 days) and prints it: run it before naming a new spec file; offline it prints the checkout's next number and says it is not reserved. release --spec-number <NNN> gives one back. Specs/135 section 14.11. finish does the deterministic chores before a pull request, in-process: it regenerates docs/reference, the ecosystem files and the compiled specs (--check compares and exits 1 on drift), checks that the slice's tracker row no longer says not built (the slice comes from --spec/--slice or this branch's claim), prints the three gates, and with --body prints a pull request body and renews the claim (its ref on origin too, quietly kept local when the remote cannot be reached). --gates runs the three gates instead of printing them, in order, in the checkout, as your own process (not sandboxed), stopping at the first that fails: cargo test (with --no-fail-fast), cargo clippy, then this program's lint; a failing test is rerun once by name (--exact), and one that passes then is reported flaky and the gate passes; when cargo nextest is installed the tests run as CI runs them instead (spec 125-A1): cargo nextest run --profile ci, which retries a failing test once itself and names the flaky ones, then cargo test --doc; a failed gate prints its last 40 lines and exits 1; --body ticks the gates that passed. tracker prints every slice as one table, read-only: the rows of the spec tracker and the per-slice files (`missions/2026-10-23-foundational-design/tracker/<spec>/<slice>.md`, frontmatter `state`, `date`, optional `pr` and `title`, then prose; a file wins over a row of the same slice), each with where it came from; --spec keeps one spec's slices. finish and the claim warning read both shapes. tracker migrate (specs/135 section 14.14) moves the table's slice rows into per-slice files once: the state cell is mapped to a state (a cell that does not map clearly gets the closest and is kept verbatim), the date is the cell's, else the day of the last commit that changed the row (git blame), else today; `pr` when the cell names exactly one; the row itself is the prose. It never overwrites a file, skips a row whose spec has no spec file or whose slice makes no valid id, and removes a row from the table only when every file for it was written, leaving a pointer line to `forge tracker --spec`. --spec or --section (text of a heading) limits it; --dry-run writes nothing and prints what it would write. Before the full gates --gates runs a fast tier (specs/135 §14.12): it maps the files changed since the merge base with origin/main (untracked ones too) to the workspace crates that hold them and the crates that depend on those, runs the tests and Clippy for those crates (-p each), then the lint, and stops if they fail; a change to a manifest, Cargo.lock, a build script, the toolchain, .cargo/, .config/, schemas/, specs/, compiled/ or any other path outside a crate (but a few the tests read, such as missions/ and docs/) selects every crate, and then only the full gates run. --fast-only stops after a passing fast tier: an early check, not the gate; AGENTS.md still requires the full gates before you commit or push. ready <PR> (specs/135 section 14.13) marks a draft pull request ready only once the CI run of its latest push has registered (`gh run list` for its head commit, every 15 s for at most --wait, default 3m; none: not marked, exit 1), then `gh pr ready`, then waits as long for a second run of that head (the ready event, where macOS runs); when none comes it toggles draft and ready once and says so (exit 1 if still none). Read-only `gh` calls apart from `gh pr ready`; exit 4 when gh fails. No daemon is started, and no process without --gates, ready or a sweep of a github.com origin (which run gh)." },
    Cmd { group: "Inspect", name: "specs", about: "compile the specs into one JSON file each, and read them", synopsis: "  delphi specs compile [--check] [--repo <path>]\n  delphi specs show <spec> [--section <n>] [--json] [--for <label>]\n  delphi specs list [--group <g>] [--status <s>] [--json]\n  delphi specs report [--json]\n  delphi specs view [<group>] [--repo <path>]", detail: "show prints a compiled spec and refuses one whose source has changed since; --section prints that section of the source spec itself, with its hash; --for labels the read (a slice or a branch) for the counter. list shows every spec and whether its compiled file is current. report prints the read counter: reads made through these commands and the Delphi tools, never a host that opens the files directly. A lookup never creates the Delphi home; --home names it. compile: Reads specs/*.md and compiled/groups.toml and writes compiled/specs/data/<spec>.json, each a quotation of its source with a hash (spec 088), then an index beside them, rendered from those files for people to review. It never edits a spec and never decides anything: the source spec stays the authority. --check compares instead of writing and fails when a file is stale, missing or has no source, or when a page differs from a fresh render of the JSON beside it. A failed compile writes nothing. Run it from the repository root or pass --repo. view: prints one group's page, rendered from the committed JSON and the groups file by the same renderer (spec 088 section 14: the group pages are not committed); with no group, lists the group ids and titles. It writes nothing and is only as current as the committed JSON." },
    Cmd { group: "Inspect", name: "check", about: "the deterministic checks behind the built-in skills", synopsis: "  delphi check <dag|handoff|provenance|rules|review-md|spec|brief|kb|tui|skill> <path> [--repo <path>]\n  delphi check fact <claim.json> [--home <dir>] [--offline]\n  delphi check record <mission-dir> --event <seq> [--hash <h>]", detail: "`record` verifies one event in a mission log: its hash chain against the daemon's stored head (no model,\nno network). An agent-authored event only warns \"an agent wrote this\". `fact` reads {claim, needed_for, kind,\nsnapshots: [ids]} and checks the claim against snapshots the daemon stored: origins, independence, classes.\nIt runs no model; the judge and the contradiction search are the daemon's. It looks a stored publisher\npage's DOI up at doi.org, as the daemon does; `--offline` skips that." },
    Cmd { group: "Maintain", name: "self-install", about: "put this program on your PATH, as install.sh does", synopsis: "  delphi self-install [--allow-downgrade]", detail: "Copies the running delphi to ~/.local/bin/delphi (or $DELPHI_INSTALL_DIR/delphi): written under a temporary name,\nmode 0755, then moved into place. It never writes over a symbolic link or over a file that is not a delphi\n(a regular program holding delphi's own text; the file is read, never run), and prints what it did. A delphi\nnewer than this one (its version is read from its bytes) is kept unless --allow-downgrade is given. The desktop\napp carries a copy of delphi and shows this command, to be run from that copy, when no delphi is installed.\nNeeds no terminal. Refused inside an agent seat." },
    Cmd { group: "Maintain", name: "update", about: "the program, the built-in files, then the daemons", synopsis: "  delphi update [--check] [--version vX.Y.Z | --dev] [--rollback] [--review] [--no-restart] [--allow-downgrade]\n    [--only-newer] [--workspace <name>] [--home <dir>]", detail: "--dev installs the latest dev build of main (built on request). --only-newer refuses a --version not newer than this.\n--rollback undoes all three steps. --review leaves the release's changes pending for\n/ratify instead of ratifying them. On macOS the desktop app goes with them (DELPHI_NO_APP=1 skips it).\nA host's Delphi plugin is not part of the program and is not updated here: when one is older than\nthe release now installed, the last lines name the host and the command (`delphi connect <host> --update`),\nand say so when an installed plugin cannot be launched. Nothing is run or written for that." },
    Cmd { group: "Maintain", name: "train", about: "training mode: check, price and run cases that test Delphi itself", synopsis: "  delphi train validate [--suite <name>] [--repo <path>]\n  delphi train plan --budget-usd <n> [--suite <name>] [--split dev|holdout|all] [--seeds <n>] [--home <new dir>] [--repo <path>]\n  delphi train run --budget-usd <n> --template-home <dir> --work-dir <new dir> [--overlay <dir> | --arms-file <f>] [--case <id>] [--seeds <n>] [--resume] [--pace-window <0-1>] [--allow-sleep] [--dry-run]\n  delphi train run --bench <benchmark.toml> --arms-file <routing arms> --budget-usd <n> --work-dir <new dir> [--resume] [--dry-run]\n  delphi train gate --work-dir <dir of a run> --bench <benchmark.toml>", detail: "For a source checkout (it needs dev/train/suites). validate reads every case and names what is wrong; plan lists the cases a run\nwould use and the most it could cost, and refuses a budget below that. Neither runs anything or calls a model.\nrun SPENDS MONEY: each case runs `delphi run` in a copy of the template home, baseline and (with --overlay) candidate, and prints pass rates.\nrun --bench is the routing gate of specs/056 §10.6: two arms that differ only in [arm.routing] assignment, per-run token caps,\nthe token ceiling printed first and a yes typed at a terminal; gate prints the five criteria, the verdict and the contrast.\nExit codes: 0 ok · 1 problems · 2 usage · 3 over budget." },
    Cmd { group: "Maintain", name: "xenia", about: "the org's shared, signed memory store: join, sync, status, publish, endorse", synopsis: "  delphi xenia whoami | help [topic] | explain <slug or message>   [--json]\n  delphi xenia join <url> --org <o> --store <s> --admin <fingerprint> --head <epoch>:<hash> [--allow-local]\n  delphi xenia sync [--auto-update-bundles] | status [--store <org>/<store>] [--json]\n  delphi xenia publish <file> --title <t> --summary <s> --promoted [--tags a,b] [--review-by <date>]\n  delphi xenia publish --bundle <dir> --url <https url> --pin <git-tree|git-commit>:<sha> [--id <record>]\n  delphi xenia endorse|dispute|retract|withdraw|restore <id> [--scope a,b] [--note <n>] [--reason <r>]\n  delphi xenia republish --old <did> | admin add|remove|rotate|show|sign|publish|refresh|genesis ...", detail: "Commands that sign show the exact bytes and want the card's code typed at a terminal. Start: `delphi xenia help`." },
    Cmd { group: "Maintain", name: "docs", about: "print the command reference or a man page", synopsis: "  delphi docs cli | man | catalog [--repo <dir>] | execute-names [--repo <dir>] | coverage\n  delphi docs ecosystem [--repo <dir>] --check | --write", detail: "Generated from the table behind --help, so it is never out of date. `cli` is Markdown (it is docs/reference/cli.md); `man` is roff; `catalog` lists the built-in roles and skills from their manifests (docs/reference/catalog.md), read from a checkout. `execute-names` lists every `execute` name of the built-in roles with what grants it: the daemon, a host only, or nothing (docs/reference/execute-names.md; specs/124). `coverage` is Markdown (docs/reference/harness-coverage.md): the disposition of every flag and command of the claude, agy and codex CLIs, from the manifests Delphi checks against each CLI\'s --help (specs/046). `ecosystem --check` fails when the plugin and marketplace files (`ecosystem/`, `.claude-plugin/marketplace.json`) differ from what this release generates; `--write` regenerates them (specs/071)." },
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
    ("DELPHI_ASCII_BADGES", "1 draws agent badges in plain ASCII, for fonts that cannot show them"),
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
const VALUE_FLAGS: &[&str] = &["--home", "--workspace", "--operator-said", "--max-prs", "--for", "--pause-after-errors", "--idle", "--poll", "--pr", "--needs-human"];

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

/// What `delphi observe` and `delphi narrator` answer now (`specs/111` §10).
const RETIRED_NARRATOR: &str = "the terminal narrator is retired: the desktop app shows the team, a council, the knowledge base and what was produced (docs/guides/desktop-app.md)";

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
            None if c == "version" => print_help(None),
            None if c == "observe" || c == "narrator" => {
                eprintln!("delphi: {RETIRED_NARRATOR}");
                2
            }
            None => {
                eprintln!("delphi: no command `{}`; `delphi --help` lists them", c);
                2
            }
        },
    }
}

/// The release files this binary carries, by logical path (the paths the index uses for `core`, `context` and `about`).
fn release_files() -> Vec<(String, &'static [u8])> {
    seed::files().into_iter().map(|f| (f.logical, f.bytes)).collect()
}

fn main() {
    // `specs/109` §12.1 item 2: the release files compiled into this binary, so a host search can say whether a release file is the
    // text this binary shipped. Hashed only when a search first asks; it changes a label, never a tier.
    delphi_engine::host_labels::set_release_files(release_files);
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
        Some("specs") => spec_compile::cli(&args),
        Some("forge") => forge_cmd::cli(&args),
        Some("check") => cmd_check(&args),
        Some("setup") => cmd_setup(&args),
        Some("update") => cmd_update(&args),
        Some("self-install") => self_install::run(xenia_terminal::inside_an_agent_seat(), args.iter().any(|a| a == "--allow-downgrade")),
        Some("init") => cmd_init(&args),
        Some("project") => project::cli(&args),
        Some("doctor") => cmd_doctor(&args),
        Some("logs") => logs_cmd::logs(&args),
        Some("merge") => merge::cli(&args),
        Some("debug") => logs_cmd::debug(&args),
        Some("status") => cmd_status(&args),
        Some("missions") => cmd_missions(&args),
        Some("changelog") => cmd_changelog(&args),
        Some("release") => match args.first().map(String::as_str) {
            None | Some("status") => release::cli(None),
            Some("changelog") => release_notes::cli(&args[1..]),
            Some(_) => {
                eprintln!("{}", release_notes::usage());
                2
            }
        },
        Some("sessions") => cmd_sessions(&args),
        Some("council") => match resolve_ws("council", &args) {
            Ok((_home, ws)) => council_watch::cli(&ws.dir, &args),
            Err(code) => code,
        },
        Some("capabilities") if args.iter().any(|a| a == "--models") => match resolve_ws("capabilities", &args) {
            Ok((home, ws)) => delphi_engine::models_cli::inventory(&ws.dir, &home, args.iter().any(|a| a == "--json")),
            Err(code) => code,
        },
        Some("capabilities") => {
            let copilot_gh = resolve_ws_quiet(&args).and_then(|d| std::fs::read_to_string(d.join("config.toml")).ok()).is_some_and(|c| host_caps::copilot_gh_opt_in(&c));
            host_caps::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref()), copilot_gh)
        }
        Some("tools") => cmd_tools(&args),
        Some("learn") => cmd_learn(&args),
        Some("teach") => cmd_teach(&args),
        Some("retro") => cmd_retro(&args),
        Some("research") => cmd_research(&args),
        Some("test") => cmd_test(&args),
        Some("agent") => cmd_agent(&args),
        Some("customize") => cmd_customize(&args),
        Some("chat") => cmd_chat(&args),
        Some("run") => cmd_run(&args),
        Some("relay") => cmd_relay(&args),
        Some("decisions") => cmd_decisions(&args),
        Some("skills") => cmd_skills(&args),
        Some("claims") => claims_cmd::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref())),
        Some("sources") => sources_cmd::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref())),
        Some("pages") => cmd_pages(&args),
        Some("library") => library_cmd::cli(&args, &home::delphi_home(flag(&args, "--home").as_deref())),
        Some("bundle") => cmd_bundle(&args),
        Some("mcp") => cmd_mcp(&args),
        Some("plugin") => cmd_plugin(&args),
        Some("connect") => {
            let code = connect_cmd::cmd(&args);
            // Spec 119 §2 item 2: a connection that was made ends with one line when the operator has chosen no search engine.
            if code == 0 && !args.iter().any(|a| matches!(a.as_str(), "--status" | "--dry-run" | "--remove")) {
                if let Some(line) = resolve_ws_quiet(&args).and_then(|dir| research_setup_cmd::hint(&dir)) {
                    println!("{line}");
                }
            }
            code
        }
        Some("hooks") => cmd_hooks(&args),
        Some("app") => cmd_app(&args),
        Some("timers") => cmd_timers(&args),
        Some("private-read") => cmd_private_read(&args),
        Some("daemon") => cmd_daemon(&args),
        Some("kb") => cmd_kb(&args),
        Some("needs") => cmd_needs(&args),
        Some("route") => match resolve_ws("route", &args) {
            Ok((home, ws)) => route_cmd::run(&args, &home, &ws.dir),
            Err(code) => code,
        },
        Some("handling") => match resolve_ws("handling", &args) {
            Ok((home, ws)) => handling_cmd::run(&args, &home, &ws.dir),
            Err(code) => code,
        },
        Some("validation") => cmd_validation(&args),
        Some("history") => cmd_history(&args),
        Some("train") => train::run(&args),
        Some("xenia") => xenia_cmd::run(&args),
        Some("docs") => docs_cmd::docs(&args),
        Some("completions") => docs_cmd::completions(&args),
        Some("connectors") => cmd_core("connectors", &args),
        Some("auth") => cmd_core("auth", &args),
        Some("endpoints") => cmd_core("endpoints", &args),
        Some("models") if args.first().map(String::as_str) == Some("calibrate") => match resolve_ws("models", &args) {
            Ok((_home, ws)) => {
                let mut rest: Vec<String> = Vec::new();
                let mut it = args[1..].iter();
                while let Some(a) = it.next() {
                    if a == "--workspace" || a == "--home" {
                        it.next();
                    } else {
                        rest.push(a.clone());
                    }
                }
                models_calibrate::cli(&ws.dir, &rest)
            }
            Err(code) => code,
        },
        Some("models") => cmd_core("models", &args),
        Some("version") => {
            println!("delphi {}", env!("CARGO_PKG_VERSION"));
            0
        }
        // Retired with the terminal narrator (`specs/111` §10); said plainly so an old habit or script gets an answer.
        Some("observe") | Some("narrator") => {
            eprintln!("delphi: {RETIRED_NARRATOR}");
            2
        }
        Some(other) => {
            eprintln!("delphi: unknown command `{}`; `delphi --help` lists them", other);
            eprintln!("commands: {}", CMDS.iter().filter(|c| !c.name.is_empty()).map(|c| c.name).chain(["version"]).collect::<Vec<_>>().join(" "));
            2
        }
    };
    log_exit(cmd.as_deref(), &args, code);
    exit(code);
}

/// Start the central error log (`specs/039`) for this process and install the panic hook. The commands that never touch
/// the Delphi home (`lint`, `specs`, `check`, `docs`, `completions`, `version`, `release`) log nothing; so do the daemon and its
/// relays, which start their own (the daemon names its home itself).
fn start_error_log(cmd: Option<&str>, args: &[String]) {
    // A host's session hook (spec 105) writes no log: it reads, prints and writes only its own marker and counter files.
    let session_hook = cmd == Some("mcp") && args.iter().any(|a| a == "--hook" || a == "--statusline");
    if session_hook || matches!(cmd, Some("__core" | "__mcp-bridge" | "__agy-hook" | "lint" | "specs" | "check" | "docs" | "completions" | "version" | "release" | "help"))|| (cmd.is_none() && args.iter().any(|a| a == "--version")) {
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
    let finding = code == 1 && matches!(name, "doctor" | "lint" | "specs" | "forge" | "check" | "status" | "train" | "bundle" | "update" | "learn" | "retro" | "decisions" | "xenia");
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
        "fact" => check::fact(std::path::Path::new(target), &home::delphi_home(flag(args, "--home").as_deref()), args.iter().any(|a| a == "--offline")),
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
        let code = update::run(&home, only.as_deref(), has("--check"), has("--allow-downgrade"), has("--rollback"), has("--review"), has("--require-signed"));
        // This is the freshly installed program (it is how the old one asks for the workspace step), so its release is the one to compare.
        if !has("--rollback") {
            print_host_plugin_notes();
        }
        return code;
    }
    let Ok(dest) = std::env::current_exe().and_then(std::fs::canonicalize) else {
        eprintln!("delphi update: cannot locate this executable");
        return 1;
    };
    // Spec 122 §10.1 item 4: the copy inside the desktop app is never rewritten in place; only `--check`, which writes nothing, runs.
    if let Some(bundle) = selfupdate::inside_app_bundle(&dest).filter(|_| !has("--check")) {
        eprintln!("delphi update: {}", selfupdate::inside_app_refusal(&dest, &bundle));
        return 1;
    }
    let api = std::env::var("DELPHI_RELEASES_URL").unwrap_or_else(|_| selfupdate::DEFAULT_API.to_string());
    if api != selfupdate::DEFAULT_API {
        eprintln!("delphi update: reading releases from {api} (DELPHI_RELEASES_URL), not from the project's repository");
    }
    let mut workspace_code = 0;
    let mut already_newest = false;
    let code = selfupdate::run_with(args, &selfupdate::RealNet, &dest, &api, &home, &mut |event| {
        if event == selfupdate::Event::UpToDate {
            workspace_code = update::run(&home, only.as_deref(), has("--check"), has("--allow-downgrade"), false, has("--review"), has("--require-signed"));
            already_newest = true;
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
    // Spec 074 §10.2: the program was already the newest, so this process is the one whose release the host plugins are compared with.
    if already_newest {
        print_host_plugin_notes();
    }
    // The CLIs may have changed with the machine; map them again with the program just installed, without waiting.
    if code == 0 && !has("--check") {
        host_caps::refresh_in_background(&dest, &home);
    }
    // No workspace yet is not a failure of the update, and neither is a rollback with nothing to restore there.
    if code != 0 || workspace_code == 2 || has("--rollback") { code } else { workspace_code }
}

/// Spec 074 §10.2 item 1: after an update, say which host's Delphi plugin the program has outrun, and whether a plugin can be
/// launched. Read-only, and never part of the exit code.
fn print_host_plugin_notes() {
    let Some(h) = std::env::var("HOME").ok().filter(|h| std::path::Path::new(h).is_absolute()) else { return };
    let user_home = std::path::Path::new(&h);
    for line in connect_cmd::host_plugin_notes(user_home, env!("CARGO_PKG_VERSION"), &launchable_now(user_home)) {
        println!("{line}");
    }
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
            // The install question of the project history (specs/102 §7 item 3): asked here, before setup takes over, because setup returns
            // from this function. A person at a terminal answers; a script, an agent or `--non-interactive` is not asked.
            if interactive {
                history_cmd::ask_at_install(&ws.dir);
            }
            let code = if tty && wants_setup(true, true, &prompt("Connect a model now? (y/n)", "y")) {
                let mut sa = vec!["--home".to_string(), home.display().to_string(), "--workspace".to_string(), ws.name.clone()];
                if args.iter().any(|a| a == "--yes" || a == "-y") {
                    sa.push("--yes".into());
                }
                cmd_setup(&sa)
            } else {
                println!("  next: run `delphi setup` to connect a model, then `delphi`");
                println!("  to use Delphi from another coding tool: `delphi mcp --print-config claude-code` (or copilot, codex, agy)");
                0
            };
            // Spec 119 §2 item 2: the last line names the command when the operator has chosen no search engine.
            if let Some(line) = research_setup_cmd::hint(&ws.dir) {
                println!("{line}");
            }
            code
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
        home::Resolve::NoneRegistered(_) if home.join("workspaces").join(start::DEFAULT_WORKSPACE).join("config.toml").exists() => start::DEFAULT_WORKSPACE.to_string(),
        home::Resolve::NoneRegistered(root) => {
            eprintln!("delphi {}: no project scope contains {}; run `delphi` to initialize Delphi or `delphi project register {}` to register it", cmd, root.display(), root.display());
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

/// The workspace for this folder as `resolve_ws` finds it, without a message; `None` when there is none.
/// The operator's packs for the workspace of this folder, quietly (spec 106 §11 G2 item 2): `None` when no workspace is found.
fn operator_packs(args: &[String]) -> Option<delphi_engine::mcp_packs::Packs> {
    let dir = resolve_ws_quiet(args)?;
    let name = dir.file_name()?.to_str()?.to_string();
    let home = dir.parent()?.parent()?.to_path_buf();
    Some(delphi_engine::mcp_packs::Packs::new(delphi_engine::workspace::WorkspacePaths::new(&home, &name)))
}

/// The folders where an operator keeps a host's agent files that `delphi plugin export` wrote, for `--check` (spec 106 §11 G2): Copilot
/// CLI's project folder (`.github/agents` in this folder) and its user folder (`~/.copilot/agents`). Other hosts: none known.
fn inlined_dirs(host: &str, home: &std::path::Path) -> Vec<PathBuf> {
    match host {
        "copilot" => std::env::current_dir().map(|c| c.join(".github/agents")).into_iter().chain([home.join(".copilot/agents")]).collect(),
        _ => Vec::new(),
    }
}

fn resolve_ws_quiet(args: &[String]) -> Option<PathBuf> {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().ok()?;
    let name = match home::resolve_workspace(&home, &cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => n,
        home::Resolve::NoneRegistered(_) | home::Resolve::NotInGitRepo => start::DEFAULT_WORKSPACE.to_string(),
        _ => return None,
    };
    let dir = home.join("workspaces").join(name);
    dir.join("config.toml").is_file().then_some(dir)
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
/// `delphi app …` (`specs/120`): the operator's, at a terminal. The refusal comes first, so an agent seat or a script is told
/// so before a workspace is looked up or a daemon is spoken to, and no ticket is issued.
fn cmd_app(args: &[String]) -> i32 {
    // `opt-out` and `opt-in` need no terminal (spec 122 §10.3); every other form does.
    let parsed = app_cmd::parse(args);
    let opt = matches!(parsed, Ok(app_cmd::Action::OptOut | app_cmd::Action::OptIn));
    if let Err(why) = app_cmd::gate(xenia_terminal::inside_an_agent_seat(), opt || chat::is_tty()) {
        eprintln!("delphi app: {}", why);
        return 2;
    }
    let action = match parsed {
        Ok(a) => a,
        Err(e) => {
            eprintln!("delphi app: {}", e);
            return 2;
        }
    };
    if opt {
        return app_cmd::opt(&action, &home::delphi_home(flag(args, "--home").as_deref()), &mut std::io::stdout());
    }
    let (home, ws) = match resolve_ws("app", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    app_cmd::run(action, &home, &ws.dir, &ws.name)
}

fn cmd_hooks(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("hooks", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    hooks_cmd::run(&home, &ws.dir)
}

/// Spec 074 L1: the launch-path line, from `stat` only.
pub(crate) fn launch_path_line(home: &std::path::Path) -> String {
    let install = std::env::var_os("DELPHI_INSTALL_DIR").map(std::path::PathBuf::from);
    match std::env::current_exe() {
        Ok(exe) => delphi_engine::mcp_check::launch_path(home, install.as_deref(), &exe),
        Err(_) => "unknown (this binary's path cannot be read)".into(),
    }
}

/// Spec 074 §10.1: whether a host plugin can launch Delphi, from `stat` only (the plugin launches `~/.local/bin/delphi`).
pub(crate) fn launchable_now(home: &std::path::Path) -> delphi_engine::mcp_check::Launchable {
    let install = std::env::var_os("DELPHI_INSTALL_DIR").map(std::path::PathBuf::from);
    match std::env::current_exe() {
        Ok(exe) => delphi_engine::mcp_check::launchable(home, install.as_deref(), &exe),
        Err(_) => delphi_engine::mcp_check::Launchable { ok: false, this_binary: false, detail: "this binary's path cannot be read".into() },
    }
}

/// `delphi mcp`: Delphi for host agents on stdio (`specs/062`). A folder with no workspace still starts, with the defaults.
fn cmd_mcp(args: &[String]) -> i32 {
    // Spec 105: a host's session hook. Exits 0 and prints nothing on any error (no message: stderr is the host's too).
    if let Some(i) = args.iter().position(|a| a == "--hook") {
        return hook_cmd::run_hook(args, args.get(i + 1).map(String::as_str));
    }
    // Spec 106 §12 G6: a host's status line. Exits 0 and prints nothing on any error, like a hook.
    if args.iter().any(|a| a == "--statusline") {
        return hook_cmd::run_statusline(args);
    }
    if let Some(i) = args.iter().position(|a| a == "--hooks") {
        return match resolve_ws("mcp", args) {
            Ok((_, ws)) => hook_cmd::switch(args.get(i + 1).map(String::as_str), &ws.dir),
            Err(code) => code,
        };
    }
    if let Some(i) = args.iter().position(|a| a == "--install") {
        return cmd_mcp_install(args, args.get(i + 1).map(String::as_str));
    }
    if let Some(i) = args.iter().position(|a| a == "--check") {
        let Some(host) = args.get(i + 1) else {
            eprintln!("delphi mcp --check: name a host: {}", delphi_engine::mcp_hosts::HOSTS.join(", "));
            return 2;
        };
        let home = std::env::var("HOME").ok().filter(|h| std::path::Path::new(h).is_absolute());
        let Some(home) = home else {
            eprintln!("delphi mcp --check: HOME is unset or not an absolute path");
            return 2;
        };
        return match delphi_engine::mcp_check::check_host(host, std::path::Path::new(&home), env!("CARGO_PKG_VERSION")) {
            Ok(c) => {
                println!("{host}: {}: {}", c.state.name(), c.detail);
                println!("{host}: launchable: {}", connect_cmd::sanitize(&launchable_now(std::path::Path::new(&home)).line()));
                println!("{host}: launch path: {}", launch_path_line(std::path::Path::new(&home)));
                // Spec 110 §3 item 4: server, instruction section, role files and hooks, as `delphi connect` writes them.
                if connect_files::handles(host) {
                    for l in connect_files::check_lines(host).unwrap_or_else(|e| vec![format!("{host}: {e}")]) {
                        println!("{l}");
                    }
                }
                // Spec 106 §11 G2 item 2: worker files with an inlined pack, against the pack the operator's files give now.
                let dirs = inlined_dirs(host, std::path::Path::new(&home));
                if !dirs.is_empty() {
                    let packs = operator_packs(args);
                    let states = delphi_engine::host_hooks::inlined_states(&dirs, &|role| packs.as_ref().and_then(|p| delphi_engine::host_hooks::current_inline_hash(p, role, host)));
                    let stale: Vec<String> = states.iter().filter(|(_, ok)| !ok).map(|(p, _)| p.display().to_string()).collect();
                    let line = match (states.len(), stale.len()) {
                        (0, _) => "none found (in .github/agents of this folder or ~/.copilot/agents)".to_string(),
                        (n, 0) => format!("current ({n} files)"),
                        (n, _) => format!("stale ({} of {n} files carry an older pack_hash; export again with `delphi plugin export --host {host}`): {}", stale.len(), stale.join(", ")),
                    };
                    println!("{host}: inlined packs: {line}");
                }
                if host == "claude-code" {
                    // Spec 105 A7: the plugin's hooks file and the switch. Neither changes the exit code.
                    println!("{host}: hooks file: {}", hook_cmd::hooks_file_line());
                    let switch = resolve_ws_quiet(args).map(|d| match delphi_engine::host_hooks::session_hooks_key(&d) {
                        Some(true) if delphi_engine::host_hooks::switch_on(&d) => "on".to_string(),
                        Some(true) => "on, but silenced by [mcp] enabled = false".to_string(),
                        Some(false) => "off (`delphi mcp --hooks on` at a terminal turns it back on; on is the default)".to_string(),
                        None => "unknown ([mcp] cannot be read; the hooks print nothing)".to_string(),
                    });
                    println!("{host}: [mcp] session_hooks: {}", switch.unwrap_or_else(|| "unknown (no workspace found for this folder)".into()));
                    // Spec 106 §12 G6 item 2: read, never written.
                    println!("{host}: status line: {}", connect_cmd::sanitize(&delphi_engine::mcp_check::check_statusline(std::path::Path::new(&home))));
                }
                if let Some(fix) = c.fix {
                    // Spec 074 §11: a host connected to another source is told to add that one, not this build's own.
                    let source = connect_run::recorded_source(&home::delphi_home(None), host).unwrap_or_default();
                    println!("fix: {}", connect_cmd::sanitize(&connect_cmd::localize(&fix, &source)));
                }
                // Spec 074 §10.3: the same update, run for the operator after they confirm its plan.
                if let Some(p) = connect_cmd::update_pointer(host, c.state) {
                    println!("or:  {p}");
                }
                i32::from(c.state != delphi_engine::mcp_check::State::Configured)
            }
            Err(e) => {
                eprintln!("delphi mcp --check: {e}");
                2
            }
        };
    }
    if args.iter().any(|a| a == "--print-claude-md") {
        // Spec 098 S5: a section for the project's CLAUDE.md. Printed, never written.
        print!("{}", delphi_engine::host_guidance::claude_md_template());
        return 0;
    }
    if let Some(i) = args.iter().position(|a| a == "--print-config") {
        let Some(host) = args.get(i + 1) else {
            eprintln!("delphi mcp --print-config: name a host: {}", delphi_engine::mcp_hosts::HOSTS.join(", "));
            return 2;
        };
        let snippet = if args.iter().any(|a| a == "--absolute") {
            std::env::current_exe()
                .map_err(|e| format!("cannot find this binary: {e}"))
                .and_then(|exe| delphi_engine::mcp_hosts::absolute_command(&exe, std::env::var("HOME").ok().as_deref()))
                .and_then(|cmd| delphi_engine::mcp_hosts::config_snippet_with(host, &cmd))
        } else {
            delphi_engine::mcp_hosts::config_snippet(host)
        };
        return match snippet {
            Ok(s) => {
                print!("{s}");
                if let Some(hint) = delphi_engine::mcp_hosts::install_hint(host) {
                    print!("{hint}");
                }
                0
            }
            Err(e) => {
                eprintln!("delphi mcp --print-config: {e}");
                2
            }
        };
    }
    match resolve_ws("mcp", args) {
        Ok((_, ws)) => delphi_engine::mcp_host::run(Some(&ws.dir)),
        Err(_) => {
            eprintln!("delphi mcp: serving with the defaults and no workspace");
            delphi_engine::mcp_host::run(None)
        }
    }
}

/// The operator's `[host.models.<host>]` from the workspace's config.toml, quietly: no workspace or no table means no mapping. A mapping
/// that is refused (unknown host or role) is an error the operator must see.
fn host_models(args: &[String], host: &str) -> Result<std::collections::BTreeMap<String, String>, String> {
    let home = home::delphi_home(flag(args, "--home").as_deref());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let name = match home::resolve_workspace(&home, &cwd, flag(args, "--workspace")) {
        home::Resolve::Found(n) => n,
        _ => start::DEFAULT_WORKSPACE.to_string(),
    };
    let Ok(text) = std::fs::read_to_string(home.join("workspaces").join(name).join("config.toml")) else { return Ok(Default::default()) };
    let cfg = delphi_adapter::config::Config::parse(&text).map_err(|e| format!("config.toml: {}", e.join("; ")))?;
    Ok(cfg.host.models.get(host).cloned().unwrap_or_default())
}

/// `delphi mcp --install codex --agents`: write the Codex seat subagents into the operator's own Codex agents directory (spec 086 §5.2).
fn cmd_mcp_install(args: &[String], host: Option<&str>) -> i32 {
    use std::io::IsTerminal;
    let has = |f: &str| args.iter().any(|a| a == f);
    if host != Some("codex") || !has("--agents") {
        eprintln!("delphi mcp --install: only `--install codex --agents` exists (Codex plugins are not documented to carry agents); the other hosts get their agents in the plugin");
        return 2;
    }
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi mcp --install: installing agent files is a human act and is refused inside an agent seat");
        return 2;
    }
    let dir = if has("--project") {
        match std::env::current_dir() {
            Ok(d) => d.join(".codex/agents"),
            Err(e) => {
                eprintln!("delphi mcp --install: {e}");
                return 2;
            }
        }
    } else {
        match std::env::var("HOME").ok().filter(|h| std::path::Path::new(h).is_absolute()) {
            Some(h) => PathBuf::from(h).join(".codex/agents"),
            None => {
                eprintln!("delphi mcp --install: HOME is unset or not an absolute path");
                return 2;
            }
        }
    };
    let models = match host_models(args, "codex") {
        Ok(m) => m,
        Err(e) => {
            eprintln!("delphi mcp --install: {e}");
            return 2;
        }
    };
    let (dry, yes) = (has("--dry-run"), has("--yes"));
    if !dry && !yes && !(std::io::stdin().is_terminal() && std::io::stdout().is_terminal()) {
        eprintln!("delphi mcp --install: a yes needs a terminal; run it in one, pass --yes, or use --dry-run to see the paths");
        return 2;
    }
    let mut confirm = || {
        use std::io::{BufRead, Write};
        print!("Write these files? Type yes: ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).is_ok() && line.trim() == "yes"
    };
    match delphi_engine::mcp_hosts::install_codex_agents(&dir, &models, dry, yes, &mut confirm, &mut std::io::stdout()) {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("delphi mcp --install: {e}");
            1
        }
    }
}

fn cmd_plugin(args: &[String]) -> i32 {
    let val = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1));
    if args.first().map(String::as_str) != Some("export") {
        eprintln!("usage: delphi plugin export --host <{}> --out <dir>", delphi_engine::mcp_hosts::HOSTS.join("|"));
        return 2;
    }
    let (Some(host), Some(out)) = (val("--host"), val("--out")) else {
        eprintln!("delphi plugin export: --host and --out are required");
        return 2;
    };
    let models = match host_models(args, host) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("delphi plugin export: {e}");
            return 2;
        }
    };
    // Spec 106 §11 G2 item 2: on the operator's machine, a host whose subagent start cannot add a pack gets it inlined in each worker.
    let packs = operator_packs(args);
    let source = |role: &str| packs.as_ref().and_then(|p| delphi_engine::host_hooks::inline_pack(p, role, host));
    let inline: Option<delphi_engine::mcp_hosts::PackSource> = packs.as_ref().map(|_| &source as delphi_engine::mcp_hosts::PackSource);
    if packs.is_none() && !delphi_engine::mcp_hosts::has_subagent_hook(host) {
        eprintln!("delphi plugin export: no Delphi workspace found for this folder, so the worker files carry no inlined pack (they call get_agent)");
    }
    match delphi_engine::mcp_hosts::write_export_inlined(host, std::path::Path::new(out), env!("CARGO_PKG_VERSION"), &models, inline) {
        Ok(files) => {
            for f in files {
                println!("wrote {}", std::path::Path::new(out).join(f).display());
            }
            0
        }
        Err(e) => {
            eprintln!("delphi plugin export: {e}");
            2
        }
    }
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
    // Listing and reading topics need no workspace; with one, a topic's due line also looks at its research topics.
    if matches!(args.first().map(String::as_str), Some("topics" | "topic")) {
        let ws = resolve_ws_quiet(args).filter(|d| d.is_dir());
        return pages_cmd::cli(args, &home::delphi_home(flag(args, "--home").as_deref()), ws.as_deref());
    }
    if !matches!(args.first().map(String::as_str), Some("search" | "expand" | "rebuild" | "seeds")) {
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

/// `delphi private-read [status|clear <key>]`: the private-read marks and the observe counter (`private_read_cmd.rs`, `specs/103` M1).
fn cmd_private_read(args: &[String]) -> i32 {
    let (home, ws) = match resolve_ws("private-read", args) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if matches!(a.as_str(), "--workspace" | "--home") {
            it.next();
        } else {
            rest.push(a.clone());
        }
    }
    private_read_cmd::cli(&home, &ws.dir, &rest)
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
    chat::daemon(&home, &ws.dir, action, &theme_names(&ws))
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

/// `delphi history off|purge|delete|status` (`specs/102` §7 item 6): the operator's, refused inside an agent seat.
fn cmd_history(args: &[String]) -> i32 {
    // The refusal comes first, so an agent seat is told so before a workspace is looked up.
    if xenia_terminal::inside_an_agent_seat() && matches!(args.first().map(String::as_str), Some("off" | "purge" | "delete" | "status")) {
        eprintln!("delphi history {}: refused; this is the operator's and this process was started by an agent seat", args[0]);
        return 2;
    }
    match resolve_ws("history", args) {
        Ok((_, ws)) => history_cmd::run(args, &ws.dir, |n| flag(args, n)),
        Err(code) => code,
    }
}

fn cmd_validation(args: &[String]) -> i32 {
    let sub = args.first().map(String::as_str);
    if !matches!(sub, Some("score" | "audit" | "report" | "latch" | "retract" | "unquarantine" | "more" | "queue" | "tier-audit" | "tier-report" | "tier" | "accept-all")) {
        eprintln!("usage: delphi validation score|audit|report|latch raise|retract --all|unquarantine|more|queue|tier-audit|tier-report|tier rollback|restore|threshold|accept-all (delphi help validation)");
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
        "queue" => done(Ok(validation_audit_cmd::queue(&ws.dir, &home))),
        "tier-report" => done(Ok(validation_tier_cmd::report(&ws.dir, &home))),
        "tier" => {
            let r = validation_tier_cmd::tier(&ws.dir, args, &flag(args, "--reason").unwrap_or_default());
            // A roll-back flags the batch-accepted claims accepted since the last good audit `re-audit due` (`specs/100` §7.3).
            let flagged = if r.is_ok() && args.get(1).map(String::as_str) == Some("rollback") { validation_tier_cmd::flag_reaudit(&ws.dir, &home) } else { 0 };
            done(r.map(|s| if flagged > 0 { format!("{s}\n{flagged} batch-accepted claim(s) accepted since the last good audit are flagged re-audit due; they stay validated\n") } else { s + "\n" }))
        }
        "accept-all" => {
            if args.iter().any(|a| a == "--list") {
                return match validation_tier_cmd::accept_all_list(&ws.dir, &home, &mut std::io::stdout()) {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("delphi validation accept-all: {e}");
                        2
                    }
                };
            }
            // Accepting is the operator's act: it needs a real terminal and the phrase typed there, so a script or a host agent cannot give it.
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                eprintln!("delphi validation accept-all: needs a terminal; accepting is yours to do (`--list` only reads)");
                return 2;
            }
            if let Some(claim) = flag(args, "--reject") {
                return match validation_tier_cmd::accept_all_reject(&ws.dir, &home, &claim, &mut std::io::stdin().lock(), &mut std::io::stdout()) {
                    Ok(()) => 0,
                    Err(e) => {
                        eprintln!("delphi validation accept-all: {e}");
                        2
                    }
                };
            }
            let only = flag(args, "--only").map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect::<Vec<_>>());
            match validation_tier_cmd::accept_all(&ws.dir, &home, only, &mut std::io::stdin().lock(), &mut std::io::stdout()) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("delphi validation accept-all: {e}");
                    2
                }
            }
        }
        "tier-audit" => {
            if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                eprintln!("delphi validation tier-audit: needs a terminal; the answer is yours to give");
                return 2;
            }
            let tier = flag(args, "--tier").unwrap_or_else(|| "substantiated".into());
            let n = flag(args, "--n").and_then(|n| n.parse::<usize>().ok()).unwrap_or(delphi_engine::tier_audit::DEFAULT_SAMPLE);
            match validation_tier_cmd::audit(&ws.dir, &home, &tier, n, &mut std::io::stdin().lock(), &mut std::io::stdout()) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("delphi validation tier-audit: {e}");
                    2
                }
            }
        }
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
            print!("{}{}{}{}", needs::render(&needs::report_in(&ws.dir, Some(&home))), needs::validation_section(&ws.dir, delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0)), needs::accept_all_section(&ws.dir, &home), needs::candidates_section(&home));
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
    if matches!(args.first().map(String::as_str), Some("import" | "review" | "refresh" | "quarantine" | "extract" | "attest")) && xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi kb {}: refused; this is the operator's and this process was started by an agent seat", args[0]);
        return 2;
    }
    // `specs/115` §4 item 2: the projects' research, private projects' included, is the operator's view; an agent's session reads
    // shared research through its own tools, under the private flag, the setting and the private-read mark.
    if args.first().map(String::as_str) == Some("search") && args.iter().any(|a| a == "--project" || a == "--all-projects") && xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi kb search: --project and --all-projects are the operator's; this process was started by an agent seat");
        return 2;
    }
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
    if rest.first().map(String::as_str) == Some("review") {
        return kb_review::cli(&home, &ws_dir, &rest[1..]);
    }
    if rest.first().map(String::as_str) == Some("health") {
        return kb_health(&home, &ws_dir, &rest[1..]);
    }
    if matches!(rest.first().map(String::as_str), Some("reads" | "flag" | "disputes")) {
        return kb_signals::run(&ws_dir, &rest);
    }
    delphi_engine::kb::run(&home, &ws_dir, &rest)
}

/// `delphi kb health [--json]` (`specs/119` §5): the state of the knowledge loop, counted from the files. It reads only, except that
/// it adds today's line to the workspace's `local/health.jsonl` when it is not there yet. The workspace was resolved by the caller,
/// so nothing is ever created for a home or a workspace that does not exist.
fn kb_health(home: &std::path::Path, ws_dir: &std::path::Path, args: &[String]) -> i32 {
    if let Some(bad) = args.iter().find(|a| a.as_str() != "--json") {
        eprintln!("delphi kb health: unknown argument `{bad}`\nusage: delphi kb health [--json] [--home <dir>] [--workspace <name>]");
        return 2;
    }
    let now = delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0);
    let health = delphi_engine::knowledge_health::gather(home, ws_dir, now);
    if let Err(e) = delphi_engine::knowledge_health::write_daily(ws_dir, &health) {
        eprintln!("delphi kb health: the daily line was not written ({e})");
    }
    if args.iter().any(|a| a == "--json") {
        println!("{}", serde_json::to_string_pretty(&health.to_json()).unwrap_or_else(|_| "{}".into()));
        return 0;
    }
    println!("knowledge health, {}", health.date);
    for line in health.lines(now) {
        println!("  {} {}", if line.stalled { "!" } else { " " }, line.text);
    }
    0
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
    // The install question of the project history (specs/102 §7 items 3 and 4): an install that was never asked, including one that
    // existed before the history did, is asked once here, at a terminal. Until it is answered nothing is recorded. The console
    // (`delphi chat`) says the first-run notice once after a yes.
    if on_terminal {
        history_cmd::ask_at_install(&ws.dir);
    }
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
        Ok((home, ws)) if args.first().map(String::as_str) == Some("ingest") => learn_cmd::ingest_cli(&delphi_engine::workspace::WorkspacePaths::new(&home, &ws.name), &learn_cmd::Ingest { from: flag(args, "--from"), role: flag(args, "--role"), path: flag(args, "--path").map(PathBuf::from), enable: args.iter().any(|a| a == "--enable") }),
        Ok((home, ws)) if args.first().map(String::as_str) == Some("keep") => learn_cmd::keep_cli(&home, &ws.dir, args),
        Ok((home, ws)) if args.first().map(String::as_str) == Some("lessons") => learn_cmd::lessons_cli(&home, &ws.dir, args),
        Ok((home, ws)) if args.first().map(String::as_str) == Some("unteach") => learn_cmd::unteach_cli(&home, &ws.dir, args),
        Ok((_home, ws)) => learn_cmd::cli(&ws.dir, args, &learn_cmd::Options { mission: flag(args, "--mission"), json: args.iter().any(|a| a == "--json") }),
        Err(code) => code,
    }
}

/// `delphi teach <role> "<text>"`: the operator teaches a role a lesson at a terminal (see `learn_teach.rs`).
fn cmd_teach(args: &[String]) -> i32 {
    match resolve_ws("teach", args) {
        Ok((home, ws)) => learn_teach::teach_cli(&home, &ws.dir, args),
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

/// `delphi research log <id>`: the search log of a finished host research job (see `research_cmd.rs`).
/// `delphi research setup`: choose the search engine research asks (see `research_setup_cmd.rs`).
fn cmd_research(args: &[String]) -> i32 {
    match resolve_ws("research", args) {
        Ok((home, ws)) if args.first().map(String::as_str) == Some("setup") => research_setup_cmd::cli(&home, &ws.dir, args),
        Ok((home, ws)) => research_cmd::cli(&home, &ws.dir, args, &research_cmd::Options { json: args.iter().any(|a| a == "--json") }),
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

/// `delphi test review|authorize|refuse|list|revoke`: the operator's side of an authorized test (see `test_cmd.rs`, spec 092).
fn cmd_test(args: &[String]) -> i32 {
    match resolve_ws("test", args) {
        Ok((home, ws)) => test_cmd::cli(&home, &ws.dir, args),
        Err(code) => code,
    }
}

/// `delphi agent promote|nerf|grades`: a role's grade, as a pending ratified edit (see `agent_cmd.rs`).
fn cmd_agent(args: &[String]) -> i32 {
    match resolve_ws("agent", args) {
        Ok((_home, ws)) => agent_cmd::cli(&ws.dir, args, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)),
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
    println!("to use Delphi from another coding tool: `delphi mcp --print-config claude-code` (or copilot, codex, agy)");
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
