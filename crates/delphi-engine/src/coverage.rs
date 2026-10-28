//! Harness coverage (`specs/046` §6, `specs/047`): every flag and subcommand of `claude`, `agy` and `codex` has a
//! disposition in a manifest (`coverage/<cli>.cov`), so "do we support it?" has an answer a test can check.
//!
//! * A **snapshot** (`coverage/snapshots/<cli>.help`) is the CLI's own `--help` for the top level and for each group
//!   in [`groups`], one `##### <cli> <path>` section per page.
//! * A **manifest** row is `kind | path | names | disposition | slice | reason`. `kind` is `flag`, `cmd` or `group`
//!   (a group covers everything beneath its path); `path` is `.` for the top level, `*` for a flag on any page.
//! * [`unclassified`] lists what a help snapshot has that no row covers. The tests run it on the checked-in snapshots,
//!   and `delphi doctor` runs it on the installed CLI, which is the alarm for a release that adds surface.

use std::collections::BTreeSet;

pub const CLIS: &[&str] = &["claude", "agy", "codex", "copilot"];

/// The pages (beyond the top level) whose `--help` is walked. Only groups and flag-only commands are listed: a leaf
/// command is never run, because a CLI that does not understand `--help` might run the command.
pub fn groups(cli: &str) -> &'static [&'static [&'static str]] {
    match cli {
        "claude" => &[&["agents"], &["auth"], &["auth", "status"], &["auto-mode"], &["mcp"], &["plugin"], &["plugin", "marketplace"]],
        "agy" => &[&["mcp"], &["plugin"], &["remote-control"]],
        "codex" => &[&["exec"], &["app-server"], &["debug"], &["login"], &["mcp"], &["plugin"], &["features"], &["cloud"], &["remote-control"], &["sandbox"], &["review"]],
        "copilot" => &[&["login"], &["workflow"], &["sessions"], &["memories"], &["plugin"], &["plugin", "marketplace"], &["mcp"], &["skill"], &["instruction"], &["lsp"], &["sandbox"]],
        _ => &[],
    }
}

pub fn manifest_text(cli: &str) -> &'static str {
    match cli {
        "claude" => include_str!("../coverage/claude.cov"),
        "agy" => include_str!("../coverage/agy.cov"),
        "codex" => include_str!("../coverage/codex.cov"),
        "copilot" => include_str!("../coverage/copilot.cov"),
        _ => "",
    }
}

pub fn snapshot_text(cli: &str) -> &'static str {
    match cli {
        "claude" => include_str!("../coverage/snapshots/claude.help"),
        "agy" => include_str!("../coverage/snapshots/agy.help"),
        "codex" => include_str!("../coverage/snapshots/codex.help"),
        "copilot" => include_str!("../coverage/snapshots/copilot.help"),
        _ => "",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Flag,
    Cmd,
}

/// One flag or subcommand found on a help page. `path` is the space-joined command path (`""` for the top level).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item {
    pub kind: Kind,
    pub path: String,
    pub name: String,
}

impl Item {
    fn full(&self) -> String {
        match (self.kind, self.path.is_empty()) {
            (Kind::Cmd, true) => self.name.clone(),
            (Kind::Cmd, false) => format!("{} {}", self.path, self.name),
            (Kind::Flag, _) => self.path.clone(),
        }
    }

    pub fn describe(&self) -> String {
        let at = if self.path.is_empty() { String::new() } else { format!(" (in `{}`)", self.path) };
        match self.kind {
            Kind::Flag => format!("flag `{}`{}", self.name, at),
            Kind::Cmd => format!("command `{}`{}", self.name, at),
        }
    }
}

/// Parse a snapshot (or any text of `##### <cli> <path>` sections) into the flags and commands it shows.
pub fn parse_snapshot(text: &str) -> Vec<Item> {
    let mut out = Vec::new();
    let mut path = String::new();
    let mut in_cmds = false;
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("##### ") {
            // "claude mcp add" → path "mcp add"; the first word is the program.
            path = h.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
            in_cmds = false;
            continue;
        }
        let trimmed = line.trim_end();
        if line.is_empty() {
            in_cmds = false;
            continue;
        }
        if trimmed.is_empty() {
            continue; // a spacer inside a command list (Claude's `mcp add` examples)
        }
        if !line.starts_with(' ') {
            in_cmds = trimmed == "Commands:" || trimmed == "Available subcommands:";
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        if in_cmds {
            if indent == 2 && body.starts_with(|c: char| c.is_ascii_lowercase()) {
                let first = body.split_whitespace().next().unwrap_or("");
                let name = first.split(['|', ',']).next().unwrap_or("");
                if !name.is_empty() && name != "help" {
                    out.push(Item { kind: Kind::Cmd, path: path.clone(), name: name.to_string() });
                }
            }
            continue;
        }
        if indent <= 6 && body.starts_with('-') {
            for tok in body.split_whitespace() {
                let t = tok.trim_end_matches(',');
                if t.starts_with("--") {
                    let name = t.split(['=', '[', '<']).next().unwrap_or(t);
                    out.push(Item { kind: Kind::Flag, path: path.clone(), name: name.to_string() });
                } else if !t.starts_with('-') {
                    break;
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// Delphi passes it today.
    Used,
    /// Planned in the slice named in the row.
    Planned,
    /// Delphi may run it to learn something; it changes nothing.
    ReadOnly,
    /// It changes the operator's global state or account; the operator types it.
    OperatorOnly,
    /// Delphi never uses it (it would widen authority, hand work to a vendor, or bypass a gate).
    Refused,
    /// Not relevant to a headless span.
    Na,
}

impl Disposition {
    pub fn word(self) -> &'static str {
        match self {
            Disposition::Used => "used",
            Disposition::Planned => "planned",
            Disposition::ReadOnly => "read-only",
            Disposition::OperatorOnly => "operator-only",
            Disposition::Refused => "refused",
            Disposition::Na => "n/a",
        }
    }

    fn parse(s: &str) -> Option<Disposition> {
        Some(match s {
            "used" => Disposition::Used,
            "planned" => Disposition::Planned,
            "read-only" => Disposition::ReadOnly,
            "operator-only" => Disposition::OperatorOnly,
            "refused" => Disposition::Refused,
            "n/a" | "na" => Disposition::Na,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    Flag,
    Cmd,
    Group,
}

#[derive(Debug, Clone)]
pub struct Row {
    pub kind: RowKind,
    /// `""` for the top level, `"*"` for a flag on any page.
    pub path: String,
    pub names: Vec<String>,
    pub disposition: Disposition,
    pub slice: String,
    pub reason: String,
}

impl Row {
    /// A row whose reason starts with `hidden:` documents a flag the CLI accepts but does not list in `--help`.
    pub fn hidden(&self) -> bool {
        self.reason.starts_with("hidden:")
    }
}

/// Parse a manifest. Blank lines and `#` comments are skipped; a malformed row is an error naming its line.
pub fn parse_manifest(text: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let cells: Vec<&str> = l.splitn(6, '|').map(str::trim).collect();
        let bad = |why: &str| format!("line {}: {}: {}", n + 1, why, l);
        if cells.len() != 6 {
            return Err(bad("want `kind | path | names | disposition | slice | reason`"));
        }
        let kind = match cells[0] {
            "flag" => RowKind::Flag,
            "cmd" => RowKind::Cmd,
            "group" => RowKind::Group,
            _ => return Err(bad("kind is flag, cmd or group")),
        };
        let disposition = Disposition::parse(cells[3]).ok_or_else(|| bad("unknown disposition"))?;
        let path = if cells[1] == "." { String::new() } else { cells[1].to_string() };
        let names: Vec<String> = cells[2].split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        if names.is_empty() && kind != RowKind::Group {
            return Err(bad("no name"));
        }
        if disposition != Disposition::Used && cells[5].is_empty() {
            return Err(bad("every disposition but `used` needs a reason"));
        }
        if disposition == Disposition::Planned && cells[4].is_empty() {
            return Err(bad("`planned` needs a slice"));
        }
        rows.push(Row { kind, path, names, disposition, slice: cells[4].to_string(), reason: cells[5].to_string() });
    }
    Ok(rows)
}

pub fn manifest(cli: &str) -> Vec<Row> {
    parse_manifest(manifest_text(cli)).unwrap_or_else(|e| panic!("coverage/{}.cov: {}", cli, e))
}

fn path_covers(group: &str, full: &str) -> bool {
    full == group || full.strip_prefix(group).is_some_and(|rest| rest.starts_with(' '))
}

/// The row that covers an item: its own row, else a flag row for any page, else the nearest group.
pub fn resolve<'a>(rows: &'a [Row], item: &Item) -> Option<&'a Row> {
    let (want, name) = (if item.kind == Kind::Flag { RowKind::Flag } else { RowKind::Cmd }, item.name.as_str());
    rows.iter()
        .find(|r| r.kind == want && r.path == item.path && r.names.iter().any(|n| n == name))
        .or_else(|| if item.kind == Kind::Flag { rows.iter().find(|r| r.kind == RowKind::Flag && r.path == "*" && r.names.iter().any(|n| n == name)) } else { None })
        .or_else(|| {
            let full = item.full();
            rows.iter().filter(|r| r.kind == RowKind::Group && r.names.is_empty() && path_covers(&r.path, &full)).max_by_key(|r| r.path.len())
        })
}

/// What a help text shows that no row covers.
pub fn unclassified(rows: &[Row], help: &str) -> Vec<Item> {
    parse_snapshot(help).into_iter().filter(|i| resolve(rows, i).is_none()).collect()
}

/// Rows that name something the snapshot no longer shows (and are not marked `hidden:`): a renamed or removed flag.
pub fn stale(rows: &[Row], help: &str) -> Vec<String> {
    let items = parse_snapshot(help);
    let mut out = Vec::new();
    for r in rows.iter().filter(|r| r.kind != RowKind::Group && !r.hidden() && r.path != "*") {
        for n in &r.names {
            let kind = if r.kind == RowKind::Flag { Kind::Flag } else { Kind::Cmd };
            if !items.iter().any(|i| i.kind == kind && i.path == r.path && &i.name == n) {
                out.push(format!("{} `{}` in `{}`", if r.kind == RowKind::Flag { "flag" } else { "command" }, n, r.path));
            }
        }
    }
    let seen: BTreeSet<&str> = items.iter().filter(|i| i.kind == Kind::Flag).map(|i| i.name.as_str()).collect();
    for r in rows.iter().filter(|r| r.kind == RowKind::Flag && r.path == "*" && !r.hidden()) {
        for n in r.names.iter().filter(|n| !seen.contains(n.as_str())) {
            out.push(format!("flag `{}` (any page)", n));
        }
    }
    out
}

/// Walk a CLI's help into snapshot text. `run` is given the words before `--help` and returns the page, or `None` if
/// the program cannot be started.
pub fn collect(cli: &str, run: &dyn Fn(&[&str]) -> Option<String>) -> Option<String> {
    let mut out = format!("##### {}\n{}\n", cli, run(&[])?.trim_end());
    for g in groups(cli) {
        if let Some(page) = run(g) {
            out.push_str(&format!("\n##### {} {}\n{}\n", cli, g.join(" "), page.trim_end()));
        }
    }
    Some(out)
}

/// The installed CLI's surface that has no disposition: the drift alarm for a release that adds flags or commands.
pub fn drift(cli: &str, run: &dyn Fn(&[&str]) -> Option<String>) -> Option<Vec<Item>> {
    let help = collect(cli, run)?;
    Some(unclassified(&manifest(cli), &help))
}

/// `docs/reference/harness-coverage.md`: the manifests as tables.
pub fn markdown() -> String {
    let mut s = String::from("# Harness coverage\n\nGenerated by `delphi docs coverage` from `crates/delphi-engine/coverage/*.cov`; do not edit by hand. Every flag and subcommand of the subscription CLIs has exactly one disposition (`specs/046`, `specs/047`). `used`: Delphi passes it. `planned`: in the named slice. `read-only`: Delphi may run it to learn something. `operator-only`: it changes the operator's global state or account, so the operator types it. `refused`: Delphi never uses it. `n/a`: not relevant to a headless span. A `group` row covers everything beneath its path.\n");
    for cli in CLIS {
        s.push_str(&format!("\n## {}\n\n| Kind | Where | Name | Disposition | Slice | Why |\n| :-- | :-- | :-- | :-- | :-- | :-- |\n", cli));
        for r in manifest(cli) {
            let kind = match r.kind {
                RowKind::Flag => "flag",
                RowKind::Cmd => "command",
                RowKind::Group => "group",
            };
            let where_ = match r.path.as_str() {
                "" => "top level".to_string(),
                "*" => "any page".to_string(),
                p => format!("`{}`", p),
            };
            let names = if r.names.is_empty() { "(everything beneath)".to_string() } else { r.names.iter().map(|n| format!("`{}`", n)).collect::<Vec<_>>().join(", ") };
            s.push_str(&format!("| {} | {} | {} | {} | {} | {} |\n", kind, where_, names, r.disposition.word(), r.slice, r.reason.replace('|', "\\|")));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP: &str = "##### tool
Usage: tool [options] [command]

Options:
  -p, --print                 print and exit
  --allowedTools, --allowed-tools <tools...>
                              names (e.g. \"--not-a-flag\")
  --cloud [description|id]    a cloud session
  -c                          short only

Commands:
  mcp                         manage servers
  plugin|plugins [options]    plugins
  help [command]              help

##### tool mcp
Usage: tool mcp

Options:
      --json                  json

Commands:
  add [options] <name>        add one
";

    fn items() -> Vec<(Kind, String, String)> {
        parse_snapshot(HELP).into_iter().map(|i| (i.kind, i.path, i.name)).collect()
    }

    #[test]
    fn a_help_page_yields_long_flags_and_commands_only() {
        let got = items();
        for f in ["--print", "--allowedTools", "--allowed-tools", "--cloud"] {
            assert!(got.contains(&(Kind::Flag, String::new(), f.to_string())), "{}", f);
        }
        assert!(got.contains(&(Kind::Flag, "mcp".into(), "--json".into())));
        assert!(got.contains(&(Kind::Cmd, String::new(), "plugin".into())) && got.contains(&(Kind::Cmd, "mcp".into(), "add".into())));
        assert!(!got.iter().any(|i| i.2 == "--not-a-flag" || i.2 == "help" || i.2 == "-c"), "{:?}", got);
    }

    #[test]
    fn rows_resolve_exactly_then_by_wildcard_then_by_group() {
        let rows = parse_manifest(
            "# c\nflag | . | --print | used | | \nflag | * | --json | read-only | | machine output\ncmd | . | mcp | read-only | | list\ngroup | mcp | | operator-only | | writes global state\ncmd | mcp | list | read-only | | read\n",
        )
        .unwrap();
        let item = |k, p: &str, n: &str| Item { kind: k, path: p.into(), name: n.into() };
        assert_eq!(resolve(&rows, &item(Kind::Flag, "", "--print")).unwrap().disposition, Disposition::Used);
        assert_eq!(resolve(&rows, &item(Kind::Flag, "mcp", "--json")).unwrap().disposition, Disposition::ReadOnly);
        assert_eq!(resolve(&rows, &item(Kind::Cmd, "mcp", "add")).unwrap().disposition, Disposition::OperatorOnly, "the group covers what has no row");
        assert_eq!(resolve(&rows, &item(Kind::Cmd, "mcp", "list")).unwrap().disposition, Disposition::ReadOnly, "an own row beats the group");
        assert_eq!(resolve(&rows, &item(Kind::Flag, "mcp add", "--scope")).unwrap().disposition, Disposition::OperatorOnly);
        assert!(resolve(&rows, &item(Kind::Flag, "", "--new")).is_none());
        assert!(resolve(&rows, &item(Kind::Flag, "mcpx", "--new")).is_none(), "a group matches whole words only");
    }

    #[test]
    fn a_new_flag_is_unclassified_and_a_removed_one_is_stale() {
        let rows = parse_manifest("flag | . | --print | used | | \nflag | . | --gone | refused | | removed upstream\ngroup | mcp | | refused | | no\n").unwrap();
        let u: Vec<String> = unclassified(&rows, HELP).iter().map(Item::describe).collect();
        assert!(u.contains(&"flag `--cloud`".to_string()) && !u.iter().any(|d| d.contains("--print")), "{:?}", u);
        assert_eq!(stale(&rows, HELP), vec!["flag `--gone` in ``".to_string()]);
    }

    #[test]
    fn bad_rows_are_refused() {
        assert!(parse_manifest("flag | . | --x | refused | | ").is_err(), "a reason is required");
        assert!(parse_manifest("flag | . | --x | planned | | later").is_err(), "a slice is required");
        assert!(parse_manifest("flag | . | --x | maybe | | r").is_err());
        assert!(parse_manifest("nonsense").is_err());
    }

    #[test]
    fn collect_walks_the_listed_groups_only() {
        let seen = std::cell::RefCell::new(Vec::new());
        let run = |args: &[&str]| -> Option<String> {
            seen.borrow_mut().push(args.join(" "));
            Some(format!("page for {}", args.join(" ")))
        };
        let text = collect("agy", &run).unwrap();
        assert_eq!(*seen.borrow(), vec!["".to_string(), "mcp".into(), "plugin".into(), "remote-control".into()]);
        assert!(text.contains("##### agy plugin\npage for plugin"));
        assert!(collect("agy", &|_| None).is_none());
    }

    #[test]
    fn the_checked_in_snapshots_are_fully_classified() {
        let mut problems = Vec::new();
        for cli in CLIS {
            let rows = manifest(cli);
            for i in unclassified(&rows, snapshot_text(cli)) {
                problems.push(format!("{}: {} has no disposition in coverage/{}.cov", cli, i.describe(), cli));
            }
            for old in stale(&rows, snapshot_text(cli)) {
                problems.push(format!("{}: coverage/{}.cov names {} but the snapshot no longer shows it (renamed upstream? mark the reason `hidden:` if it is accepted but unlisted)", cli, cli, old));
            }
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }

    #[test]
    fn every_flag_the_harness_emits_is_a_used_row_and_every_used_flag_is_emitted() {
        let src = include_str!("harness.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        let mut emitted = BTreeSet::new();
        for part in code.split('"').skip(1).step_by(2) {
            if part.starts_with("--") && part.len() > 2 && part[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                emitted.insert(part.to_string());
            }
        }
        let used: BTreeSet<String> = CLIS.iter().flat_map(|c| manifest(c)).filter(|r| r.kind == RowKind::Flag && r.disposition == Disposition::Used).flat_map(|r| r.names).collect();
        let unlisted: Vec<_> = emitted.difference(&used).collect();
        assert!(unlisted.is_empty(), "harness.rs passes flags the manifests do not call `used`: {:?}", unlisted);
        // A row whose reason starts `as ` documents a long flag that harness.rs passes in its short form.
        let short: BTreeSet<String> = CLIS.iter().flat_map(|c| manifest(c)).filter(|r| r.reason.starts_with("as ")).flat_map(|r| r.names).collect();
        let dead: Vec<_> = used.difference(&emitted).filter(|n| !short.contains(*n)).collect();
        assert!(dead.is_empty(), "manifest rows say `used` for flags harness.rs does not pass: {:?}", dead);
    }

    #[test]
    fn the_reference_lists_every_cli() {
        let md = markdown();
        for cli in CLIS {
            assert!(md.contains(&format!("## {}\n", cli)));
        }
    }

    /// `cargo test -p delphi-engine --lib coverage::tests::refresh_snapshots -- --ignored`: rewrite the snapshots from the
    /// installed CLIs. Read-only (`--help` pages only); review the diff, then classify what is new.
    #[test]
    #[ignore]
    fn refresh_snapshots() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("coverage/snapshots");
        for cli in CLIS {
            let run = |args: &[&str]| -> Option<String> {
                let out = std::process::Command::new(cli).args(args).arg("--help").stdin(std::process::Stdio::null()).output().ok()?;
                Some(format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
            };
            let version = std::process::Command::new(cli).arg("--version").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
            match collect(cli, &run) {
                Some(text) => std::fs::write(dir.join(format!("{}.help", cli)), format!("## version: {}\n{}", version, text)).unwrap(),
                None => eprintln!("{} is not installed; its snapshot is unchanged", cli),
            }
        }
    }
}
