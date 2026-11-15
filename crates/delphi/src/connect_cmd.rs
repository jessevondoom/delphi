//! Spec 074 C1 and C2: `delphi connect claude-code [--dry-run | --status | --remove]`. `--dry-run` and `--status` execute nothing and write nothing,
//! and never runs the host CLI (not even `--version`). The execution path (C2) is in `connect_run`.

use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use delphi_engine::mcp_check;

use crate::connect_run;

/// The marketplace repository this build installs from by default (spec 074 §11): Delphi's own, or a fork's when it was built
/// with `DELPHI_MARKETPLACE_REPO`.
pub const SOURCE: &str = delphi_engine::mcp_hosts::MARKETPLACE_REPO;
/// Where a template's add row takes the source the operator consented to.
const SOURCE_SLOT: &str = "{source}";

/// The marketplace a connect installs from, or expects the host to have: a GitHub repository and, optionally, a branch.
/// It comes from this build's default, from `--source` typed on the command line, or from the record of an earlier connect.
/// Never from the environment or a file an agent could write (spec 074 §11 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub repo: String,
    pub reference: Option<String>,
}

impl Default for Source {
    fn default() -> Self {
        Source { repo: SOURCE.to_string(), reference: None }
    }
}

impl Source {
    pub fn parse(s: &str) -> Result<Source, String> {
        delphi_engine::mcp_hosts::parse_source(s).map(|(repo, reference)| Source { repo, reference })
    }

    /// The one argv element a host is given: `owner/repo` or `owner/repo#branch`.
    pub fn arg(&self) -> String {
        match &self.reference {
            Some(r) => format!("{}#{r}", self.repo),
            None => self.repo.clone(),
        }
    }

    /// Whether this is the repository the build installs from by default.
    pub fn is_builtin(&self) -> bool {
        self.repo == SOURCE
    }
}
const MAX_EXE: u64 = 1024 * 1024 * 1024;

/// One host's fixed commands (spec 074 C3 table). Every element is a constant: nothing from the operator, an agent, the environment or a file.
pub struct Template {
    pub host: &'static str,
    pub exe: &'static str,
    pub id: &'static str,
    /// The host version the add and remove rows were run on.
    pub proven_on: &'static str,
    /// The host version the update rows were run on (spec 074 §10.5): its own proof, not the add row's.
    pub proven_update: &'static str,
    pub dirs: &'static [&'static str],
    pub add: [&'static [&'static str]; 2],
    pub remove: [&'static [&'static str]; 2],
    /// Spec 074 §10.2: the host's own update of an installed plugin. Empty for a host with no proven row.
    pub update: &'static [&'static [&'static str]],
    pub writes: &'static str,
    pub manual_remove: &'static str,
    pub manual_update: &'static str,
}

pub const TEMPLATES: [Template; 2] = [
    Template {
        host: "claude-code",
        exe: "claude",
        // v2: the update row was added (spec 074 §10.2), so an audit line tells the two sets of rows apart.
        id: "claude-v2",
        proven_on: "Claude Code 2.1.314",
        proven_update: "Claude Code 2.1.325",
        dirs: &[".local/bin", ".claude/local", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"],
        add: [&["plugin", "marketplace", "add", SOURCE_SLOT], &["plugin", "install", "delphi@delphi"]],
        remove: [&["plugin", "uninstall", "delphi@delphi"], &["plugin", "marketplace", "remove", "delphi"]],
        writes: "~/.claude/plugins/ (marketplace and plugin files), ~/.claude/settings.json (enabledPlugins, extraKnownMarketplaces)",
        manual_remove: "claude plugin uninstall delphi@delphi && claude plugin marketplace remove delphi",
        // Proven on Claude Code 2.1.325 to refresh a stale marketplace clone by itself (spec 074 §10.5).
        update: &[&["plugin", "update", "delphi@delphi"]],
        manual_update: "claude plugin update delphi@delphi",
    },
    Template {
        host: "copilot",
        exe: "copilot",
        id: "copilot-v2",
        proven_on: "Copilot CLI 1.0.105",
        proven_update: "Copilot CLI 1.0.111",
        dirs: &[".local/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"],
        add: [&["plugin", "marketplace", "add", SOURCE_SLOT], &["plugin", "install", "delphi@delphi"]],
        remove: [&["plugin", "uninstall", "delphi@delphi"], &["plugin", "marketplace", "remove", "delphi"]],
        writes: "~/.copilot/ (settings.json: enabledPlugins, extraKnownMarketplaces; installed-plugins/)",
        manual_remove: "copilot plugin uninstall delphi@delphi && copilot plugin marketplace remove delphi",
        // Both proven to run on Copilot CLI 1.0.111; whether the second refreshes the catalog alone was not established.
        update: &[&["plugin", "marketplace", "update", "delphi"], &["plugin", "update", "delphi@delphi"]],
        manual_update: "copilot plugin marketplace update delphi && copilot plugin update delphi@delphi",
    },
];

impl Template {
    /// The two commands an operator would type to do by hand what an add does, for `source`.
    pub fn manual_add(&self, source: &Source) -> String {
        format!("{} plugin marketplace add {} && {} plugin install delphi@delphi", self.exe, source.arg(), self.exe)
    }
}

pub fn template(host: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.host == host)
}

/// A host CLI found in a fixed directory, resolved and checked.
#[derive(Debug, PartialEq, Eq)]
pub struct HostExe {
    pub listed: PathBuf,
    pub canonical: PathBuf,
    pub uid: u32,
    pub mode: u32,
    pub sha256: String,
}

/// Replace every control character so a path or value cannot forge a line of the plan or move the cursor.
pub fn sanitize(s: &str) -> String {
    s.chars().flat_map(|c| if c.is_control() { c.escape_default().collect::<Vec<_>>() } else { vec![c] }).collect()
}

/// The fixed directories searched for the host CLI, in order (relative entries are under `home`). The operator's `PATH` is never a source.
pub fn search_dirs(t: &Template, home: &Path) -> Vec<PathBuf> {
    t.dirs.iter().map(|d| if d.starts_with('/') { PathBuf::from(d) } else { home.join(d) }).collect()
}

/// Every component of `canonical` must be owned by the user or root and not writable by group or others, and the file regular and executable.
fn path_is_safe(canonical: &Path, floor: Option<&Path>) -> Result<(), String> {
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    let mut cur = Some(canonical);
    while let Some(p) = cur {
        if Some(p) == floor {
            break;
        }
        let m = std::fs::symlink_metadata(p).map_err(|_| format!("{} cannot be inspected", sanitize(&p.display().to_string())))?;
        if m.file_type().is_symlink() {
            return Err(format!("{} is a symlink after resolution", sanitize(&p.display().to_string())));
        }
        if m.uid() != me && m.uid() != 0 {
            return Err(format!("{} is owned by another user", sanitize(&p.display().to_string())));
        }
        if m.mode() & 0o022 != 0 {
            return Err(format!("{} is writable by group or others", sanitize(&p.display().to_string())));
        }
        cur = p.parent().filter(|q| !q.as_os_str().is_empty());
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC).open(path).map_err(|_| "the host CLI cannot be opened".to_string())?;
    let m = f.metadata().map_err(|_| "the host CLI cannot be inspected".to_string())?;
    if !m.is_file() || m.len() > MAX_EXE {
        return Err("the host CLI is not a regular file of a sane size".into());
    }
    let mut h = Sha256::new();
    let mut f = f.take(MAX_EXE);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|_| "the host CLI cannot be read".to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Find and check the host CLI named `name` in `dirs`; the first existing candidate decides (a later one is never a fallback for an unsafe one).
pub fn find_host(dirs: &[PathBuf], name: &str) -> Result<HostExe, String> {
    find_host_above(dirs, name, None)
}

/// As `find_host`, trusting everything at and above `floor` (tests only: a scratch directory sits under a sticky, world-writable `/tmp`).
pub fn find_host_above(dirs: &[PathBuf], name: &str, floor: Option<&Path>) -> Result<HostExe, String> {
    for d in dirs {
        let listed = d.join(name);
        if std::fs::symlink_metadata(&listed).is_err() {
            continue;
        }
        let canonical = std::fs::canonicalize(&listed).map_err(|_| format!("{} does not resolve", sanitize(&listed.display().to_string())))?;
        path_is_safe(&canonical, floor)?;
        let m = std::fs::symlink_metadata(&canonical).map_err(|_| "the host CLI cannot be inspected".to_string())?;
        if !m.is_file() || m.mode() & 0o111 == 0 {
            return Err(format!("{} is not an executable regular file", sanitize(&canonical.display().to_string())));
        }
        let sha256 = sha256_file(&canonical)?;
        return Ok(HostExe { listed, canonical, uid: m.uid(), mode: m.mode() & 0o7777, sha256 });
    }
    Err(format!("`{name}` was not found in {}", dirs.iter().map(|d| sanitize(&d.display().to_string())).collect::<Vec<_>>().join(", ")))
}

/// The fixed argv of each step (no shell; every element is a constant).
pub fn argv(exe: &Path, steps: &[&[&str]]) -> Vec<Vec<String>> {
    steps.iter().map(|st| std::iter::once(exe.display().to_string()).chain(st.iter().map(|x| x.to_string())).collect()).collect()
}

/// The argv of an add for `source`: the template's rows with the source slot filled. The source was parsed as a GitHub
/// `owner/repo[#branch]`, so it is one element that no host CLI can read as a flag.
pub fn add_argv(exe: &Path, t: &Template, source: &Source) -> Vec<Vec<String>> {
    let mut steps = argv(exe, &t.add);
    for step in &mut steps {
        for x in step.iter_mut().filter(|x| *x == SOURCE_SLOT) {
            *x = source.arg();
        }
    }
    steps
}

/// [`render_plan_from`] for this build's own repository.
#[cfg(test)]
pub fn render_plan(t: &Template, exe: &HostExe, pre: &mcp_check::Check, addable: bool, link: &connect_run::Link, home: &Path) -> String {
    render_plan_from(t, exe, pre, addable, link, home, &Source::default())
}

/// The plan text: everything the operator would consent to. Dynamic values are sanitised.
pub fn render_plan_from(t: &Template, exe: &HostExe, pre: &mcp_check::Check, addable: bool, link: &connect_run::Link, home: &Path, source: &Source) -> String {
    let mut o = String::new();
    o.push_str(&format!("delphi connect {}: plan (nothing has been run or written)\n", t.host));
    o.push_str(&format!("host CLI: {} (resolves to {})\n", sanitize(&exe.listed.display().to_string()), sanitize(&exe.canonical.display().to_string())));
    o.push_str(&format!("  owner uid {}, mode {:o}, sha256 {}\n", exe.uid, exe.mode, exe.sha256));
    o.push_str(&format!("  the host version is not probed (that would run a program you have not consented to); the argv below was proven on {}\n", t.proven_on));
    for (i, a) in add_argv(&exe.canonical, t, source).iter().enumerate() {
        o.push_str(&format!("step {}: {}\n", i + 1, a.iter().map(|x| sanitize(x)).collect::<Vec<_>>().join(" ")));
    }
    o.push_str("working directory: a private temporary directory made at run time\n");
    o.push_str("environment: only a fixed safe PATH, HOME, LANG and LC_*, TERM and a private TMPDIR; proxy, token, loader, Git, npm, Python, Ruby and XDG variables are dropped\n");
    let follows = source.reference.as_deref().map_or_else(|| "its default branch".to_string(), |r| format!("its branch `{}`", sanitize(r)));
    o.push_str(&format!("source: {}, a GitHub repository ({follows}), fetched by {} at connect time; the host then tracks it (no commit or version is pinned: spec 071 D-5(B))\n", sanitize(&source.repo), t.exe));
    if !source.is_builtin() {
        o.push_str(&format!("WARNING: {} is not the repository this build of Delphi installs from ({SOURCE}). The skill text, hooks and server declaration that steer {} will come from it, now and at every update. Go on only if you trust everyone who can push there\n", sanitize(&source.repo), t.exe));
    }
    o.push_str(&format!("expected writes: {}\n", t.writes));
    o.push_str(&connect_run::link_line(link, home));
    o.push_str("network: github.com, as the host decides\n");
    o.push_str(&format!("state found now: {}: {}\n", pre.state.name(), sanitize(&pre.detail)));
    if addable {
        o.push_str("this would add: the `delphi` marketplace and the `delphi@delphi` plugin\n");
    } else {
        o.push_str(&format!("connect would refuse: it only adds what is absent. Use `delphi connect {} --status` for the cure\n", t.host));
    }
    o.push_str(&format!("removal: `delphi connect {} --remove`, only for what connect added and only while the host's state still matches its record\n", t.host));
    o.push_str("failure: there is no automatic rollback; a partial result is reported with the manual cure\n");
    o
}

/// The confirmation the execution path will require for this plan, bound to the whole plan text.
pub fn confirmation(host: &str, plan: &str) -> String {
    let h: String = Sha256::digest(plan.as_bytes()).iter().take(6).map(|b| format!("{b:02x}")).collect();
    format!("CONNECT {host} {h}")
}

/// Spec 074 §10.2 item 1: what `delphi update` says about the host plugins once the program is current. One line for each host
/// whose Delphi plugin is older than this release, with the cure, and one when a plugin is installed and cannot be launched.
/// Read-only: it runs no host CLI and writes nothing.
pub fn host_plugin_notes(home: &Path, release: &str, launch: &mcp_check::Launchable) -> Vec<String> {
    use mcp_check::State;
    let mut out = Vec::new();
    let mut installed = false;
    for host in ["claude-code", "copilot", "codex"] {
        let Ok(c) = mcp_check::check_host(host, home, release) else { continue };
        installed |= matches!(c.state, State::Configured | State::Stale | State::Incompatible);
        if c.state != State::Stale {
            continue;
        }
        let cure = match template(host).filter(|t| !t.update.is_empty()) {
            Some(t) => format!("`delphi connect {host} --update` (or `{}`)", t.manual_update),
            None => c.fix.as_deref().map_or_else(|| format!("see `delphi mcp --check {host}`"), |f| format!("`{}`", sanitize(f))),
        };
        out.push(format!("{host}: the Delphi plugin there was not updated with the program: {}. Update it: {cure}", sanitize(&c.detail)));
    }
    // Not launchable, or launchable as another install: either way the plugin does not run the program just updated.
    if installed && (!launch.ok || !launch.this_binary) {
        out.push(format!("host plugins: launchable: {}", sanitize(&launch.line())));
    }
    out
}

/// Spec 074 §10.3 item 1: beside the host's own command, the consented way to cure a stale plugin, for a host with an update row.
pub fn update_pointer(host: &str, state: mcp_check::State) -> Option<String> {
    (state == mcp_check::State::Stale && template(host).is_some_and(|t| !t.update.is_empty())).then(|| format!("delphi connect {host} --update"))
}

/// Spec 074 §10.3 item 2: how to let Claude Code keep the plugin current by itself. Claude Code documents only this interactive
/// route for a marketplace the operator added, so Delphi neither sets nor reads the setting.
pub const AUTO_UPDATE_TIP: &str = "tip: Claude Code does not auto-update a marketplace you added yourself. To let it keep the Delphi plugin current, run `/plugin` in a session, open Marketplaces, select `delphi` and choose Enable auto-update. Until then, `delphi update` tells you when the plugin falls behind";

/// A fix the read-only check printed names this build's repository; for a host connected to another source, name that one.
pub fn localize(fix: &str, source: &Source) -> String {
    if source.is_builtin() && source.reference.is_none() {
        fix.to_string()
    } else {
        fix.replace(&format!("marketplace add {SOURCE}"), &format!("marketplace add {}", source.arg()))
    }
}

fn usage() -> i32 {
    eprintln!("usage: delphi connect claude-code|copilot|codex|gemini [--dry-run | --status | --remove]\n       delphi connect claude-code|copilot --update [--dry-run]\n       delphi connect claude-code|copilot [--dry-run | --update | --status] --source owner/repo[#branch]\n       delphi connect claude-code --theme [--dry-run | --remove] [--light | --dark]");
    2
}

pub fn cmd(args: &[String]) -> i32 {
    let rest: Vec<&str> = args.iter().map(String::as_str).collect();
    let status = rest.contains(&"--status");
    let dry = rest.contains(&"--dry-run");
    let remove = rest.contains(&"--remove");
    let update = rest.contains(&"--update");
    // Spec 074 §11: `--source owner/repo[#branch]`, typed here and nowhere else. Its value is taken out before a host is looked for.
    let mut rest = rest;
    let named = match rest.iter().position(|a| *a == "--source") {
        Some(i) => {
            let Some(value) = rest.get(i + 1).copied().filter(|v| !v.starts_with("--")) else {
                eprintln!("delphi connect: `--source` needs a GitHub repository: owner/repo, or owner/repo#branch");
                return 2;
            };
            rest.drain(i..=i + 1);
            match Source::parse(value) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("delphi connect: {e}");
                    return 2;
                }
            }
        }
        None => None,
    };
    let Some(host) = rest.iter().find(|a| !a.starts_with("--")) else { return usage() };
    // Spec 110 X2: Codex and Gemini CLI install Delphi's own files, not a marketplace plugin. `--remove --dry-run` previews a removal.
    if crate::connect_files::handles(host) {
        if named.is_some() {
            eprintln!("delphi connect: `--source` is for the hosts that install the Delphi plugin from a marketplace (claude-code, copilot); {host} gets Delphi's own files");
            return 2;
        }
        if update {
            // Spec 074 §10.2 item 6: these hosts have no marketplace plugin that connect manages.
            eprintln!("delphi connect: `--update` is for the hosts that install the Delphi plugin (claude-code, copilot). For {host}, `delphi connect {host} --status` says what is stale and `delphi connect {host}` writes Delphi's files again");
            return 2;
        }
        if [status, dry, remove].iter().filter(|x| **x).count() > 1 && !(dry && remove && !status) || rest.iter().any(|a| a.starts_with("--") && !matches!(*a, "--status" | "--dry-run" | "--remove")) {
            return usage();
        }
        let models = crate::host_models(args, host).unwrap_or_default();
        return crate::connect_files::cmd(host, status, dry, remove, &models);
    }
    // Spec 106 §12 G7: Claude Code's theme is its own small step, with its own plan and a [Y/n], not the typed phrase.
    if rest.contains(&"--theme") {
        if *host != "claude-code" {
            eprintln!("delphi connect: `--theme` is for claude-code, the one host with colours a theme can set");
            return 2;
        }
        let Some(home) = std::env::var("HOME").ok().filter(|h| Path::new(h).is_absolute()) else {
            eprintln!("delphi connect: HOME is unset or not an absolute path");
            return 2;
        };
        let flags: Vec<&str> = rest.iter().copied().filter(|a| a.starts_with("--")).collect();
        if rest.iter().filter(|a| !a.starts_with("--")).count() != 1 {
            return usage();
        }
        return crate::connect_theme::cmd(&flags, Path::new(&home), &crate::home::delphi_home(None));
    }
    let Some(t) = template(host) else {
        eprintln!("delphi connect: `{}` is not a host connect supports (claude-code, copilot, codex, gemini); use the manual commands from `delphi mcp --print-config {}`", sanitize(host), sanitize(host));
        return 2;
    };
    // One mode at a time; `--update --dry-run` previews an update.
    if [status, dry, remove, update].iter().filter(|x| **x).count() > 1 && !(update && dry && !status && !remove) || rest.iter().any(|a| a.starts_with("--") && !matches!(*a, "--status" | "--dry-run" | "--remove" | "--update")) {
        return usage();
    }
    let Some(home) = std::env::var("HOME").ok().filter(|h| Path::new(h).is_absolute()) else {
        eprintln!("delphi connect: HOME is unset or not an absolute path");
        return 2;
    };
    let home = Path::new(&home);
    let release = env!("CARGO_PKG_VERSION");
    let delphi_home = crate::home::delphi_home(None);
    // The source: what was typed, else what an earlier connect recorded for this host, else this build's own repository.
    let source = named.clone().or_else(|| connect_run::recorded_source(&delphi_home, t.host)).unwrap_or_default();
    let pre = match mcp_check::check_host(t.host, home, release) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    if status {
        println!("{}: {}: {}", t.host, pre.state.name(), sanitize(&pre.detail));
        println!("{}: launchable: {}", t.host, sanitize(&crate::launchable_now(home).line()));
        println!("{}: launch path: {}", t.host, crate::launch_path_line(home));
        println!("{}: source: {}", t.host, sanitize(&connect_run::source_line(t, home, &source)));
        println!("next: {}", pre.fix.as_deref().map(|f| sanitize(&localize(f, &source))).unwrap_or_else(|| "nothing to do".into()));
        if let Some(p) = update_pointer(t.host, pre.state) {
            println!("or:   {p} (the same update, run for you after you confirm its plan)");
        }
        return 0;
    }
    if !dry {
        if let Err(e) = connect_run::may_run_here() {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    }
    let addable = connect_run::addable(t, &pre, home);
    let link = connect_run::link_now(home);
    let exe = match find_host(&search_dirs(t, home), t.exe) {
        Ok(exe) => exe,
        Err(e) => {
            eprintln!("delphi connect: {e}. Manual commands: {}", if remove { t.manual_remove.to_string() } else if update { t.manual_update.to_string() } else { t.manual_add(&source) });
            return 1;
        }
    };
    if update {
        if let Err(e) = connect_run::updatable_from(t, home, &source) {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    }
    if dry && update {
        let plan = connect_run::update_plan_from(t, &exe, &pre, &link, home, &source);
        print!("{plan}");
        println!("confirmation that `delphi connect {} --update` will require for exactly this plan: {}", t.host, connect_run::update_confirmation(t.host, &plan));
        return 0;
    }
    if dry {
        let plan = render_plan_from(t, &exe, &pre, addable, &link, home, &source);
        print!("{plan}");
        println!("confirmation that `delphi connect {}` will require for exactly this plan: {}", t.host, confirmation(t.host, &plan));
        return 0;
    }
    let (plan, expect) = if remove {
        if let Err(e) = connect_run::removable(t, home, &delphi_home) {
            eprintln!("delphi connect: {e}");
            return 2;
        }
        let plan = connect_run::remove_plan(t, &exe);
        let expect = connect_run::remove_confirmation(t.host, &plan);
        (plan, expect)
    } else if update {
        let plan = connect_run::update_plan_from(t, &exe, &pre, &link, home, &source);
        let expect = connect_run::update_confirmation(t.host, &plan);
        (plan, expect)
    } else {
        if !addable {
            eprintln!("delphi connect: something of Delphi's is already present, and connect only adds what is absent: {}: {}", pre.state.name(), sanitize(&pre.detail));
            eprintln!("next: {}", pre.fix.as_deref().map(|f| sanitize(&localize(f, &source))).unwrap_or_else(|| format!("see `delphi connect {} --status`", t.host)));
            if let Some(p) = update_pointer(t.host, pre.state) {
                eprintln!("or:   {p}");
            }
            return 2;
        }
        let plan = render_plan_from(t, &exe, &pre, true, &link, home, &source);
        let expect = confirmation(t.host, &plan);
        (plan, expect)
    };
    let proof = match connect_run::consent::ask(&plan, &expect) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 2;
        }
    };
    let result = if remove {
        connect_run::remove(t, &proof, &exe, home, &delphi_home)
    } else if update {
        connect_run::update_from(t, &proof, &exe, home, &delphi_home, release, &link, &source)
    } else {
        connect_run::connect_from(t, &proof, &exe, home, &delphi_home, release, &link, &source)
    };
    match result {
        Ok(m) => {
            println!("{m}");
            if !remove && t.host == "claude-code" {
                println!("{AUTO_UPDATE_TIP}");
            }
            if !remove && !update && t.host == "claude-code" {
                // Spec 106 §12 G6 item 3: a role's name is bold in the host's own text with `badge_style = "markdown"`, and in colour in a
                // status line Delphi never writes for you.
                println!("tip: `[mcp] badge_style = \"markdown\"` in config.toml makes a role's name bold where it signs; for a status line naming the adopted role in its colour, add \"statusLine\": {{\"type\": \"command\", \"command\": \"delphi mcp --statusline --host claude-code\"}} to Claude Code's settings.json yourself");
                // Spec 106 §12 G7: offer the role colours once, here, where the operator is already at a terminal.
                crate::connect_theme::offer(home, &delphi_home);
            }
            0
        }
        Err(e) => {
            eprintln!("delphi connect: {}", e.message());
            if matches!(e, connect_run::Refusal::NotAllowed(_)) {
                2
            } else {
                1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// The canonical scratch directory's parent: trusted, because the system temp directory is sticky and world-writable.
    fn root(t: &tempfile::TempDir) -> PathBuf {
        std::fs::canonicalize(t.path()).unwrap().parent().unwrap().to_path_buf()
    }

    fn exe_in(dir: &Path, mode: u32) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let p = dir.join("claude");
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
        p
    }

    #[test]
    fn a_symlinked_install_is_resolved_and_hashed_and_a_missing_cli_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let real = exe_in(&t.path().join("share/versions"), 0o755);
        std::fs::create_dir_all(t.path().join("bin")).unwrap();
        std::os::unix::fs::symlink(&real, t.path().join("bin/claude")).unwrap();
        let h = find_host_above(&[t.path().join("bin")], "claude", Some(&root(&t))).unwrap();
        assert_eq!(h.canonical, std::fs::canonicalize(&real).unwrap());
        assert_eq!(h.sha256.len(), 64);
        assert!(find_host_above(&[t.path().join("none")], "claude", Some(&root(&t))).unwrap_err().contains("was not found"));
    }

    #[test]
    fn a_writable_parent_a_writable_file_and_a_non_executable_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("a");
        exe_in(&d, 0o755);
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o775)).unwrap();
        assert!(find_host_above(std::slice::from_ref(&d), "claude", Some(&root(&t))).unwrap_err().contains("writable by group or others"));
        let d2 = t.path().join("b");
        exe_in(&d2, 0o757);
        assert!(find_host_above(&[d2], "claude", Some(&root(&t))).unwrap_err().contains("writable"));
        let d3 = t.path().join("c");
        exe_in(&d3, 0o644);
        assert!(find_host_above(&[d3], "claude", Some(&root(&t))).unwrap_err().contains("not an executable"));
    }

    #[test]
    fn the_first_candidate_decides_and_an_unsafe_one_is_not_skipped() {
        let t = tempfile::tempdir().unwrap();
        let bad = t.path().join("bad");
        exe_in(&bad, 0o777);
        let good = t.path().join("good");
        exe_in(&good, 0o755);
        assert!(find_host_above(&[bad, good], "claude", Some(&root(&t))).is_err());
    }

    #[test]
    fn the_plan_is_fixed_argv_sanitised_and_the_confirmation_binds_the_whole_plan() {
        let t = tempfile::tempdir().unwrap();
        exe_in(&t.path().join("x"), 0o755);
        let mut h = find_host_above(&[t.path().join("x")], "claude", Some(&root(&t))).unwrap();
        let pre = mcp_check::Check { state: mcp_check::State::Missing, detail: "not installed\nFORGED: yes".into(), fix: None };
        let none = connect_run::Link::Unavailable;
        let plan = render_plan(template("copilot").unwrap(), &h, &pre, true, &none, t.path());
        assert!(plan.contains(&format!("plugin marketplace add {SOURCE}\n")) && plan.contains("plugin install delphi@delphi"));
        assert!(!plan.contains("WARNING: "), "this build's own repository needs no warning: {plan}");

        // Spec 074 §11: a source the operator named is the one argv element, is warned about, and changes the confirmation.
        let fork = Source::parse("fastly/delphi#stable").unwrap();
        let forked = render_plan_from(template("copilot").unwrap(), &h, &pre, true, &none, t.path(), &fork);
        assert!(forked.contains("plugin marketplace add fastly/delphi#stable\n") && !forked.contains(SOURCE_SLOT), "{forked}");
        assert!(forked.contains("source: fastly/delphi, a GitHub repository (its branch `stable`)") && forked.contains("WARNING: fastly/delphi is not the repository this build"), "{forked}");
        assert_ne!(confirmation("copilot", &forked), confirmation("copilot", &plan), "another source is another plan");
        assert_eq!(add_argv(Path::new("/x/copilot"), template("copilot").unwrap(), &fork)[0], ["/x/copilot", "plugin", "marketplace", "add", "fastly/delphi#stable"]);
        assert_eq!(add_argv(Path::new("/x/claude"), template("claude-code").unwrap(), &Source::default())[0][4], SOURCE);
        assert_eq!(template("claude-code").unwrap().manual_add(&fork), "claude plugin marketplace add fastly/delphi#stable && claude plugin install delphi@delphi");
        assert_eq!(localize("claude plugin marketplace add jessevondoom/delphi && claude plugin install delphi@delphi".replace("jessevondoom/delphi", SOURCE).as_str(), &fork), "claude plugin marketplace add fastly/delphi#stable && claude plugin install delphi@delphi");
        assert_eq!(localize("claude plugin update delphi@delphi", &fork), "claude plugin update delphi@delphi");
        assert!(plan.contains("launch path: the plugin runs") && plan.contains("no link is made"), "the plan says what happens to the launch path: {plan}");
        let made = render_plan(template("copilot").unwrap(), &h, &pre, true, &connect_run::Link::Make("/opt/homebrew/bin/delphi".into()), t.path());
        assert!(made.contains("this would make it a link to /opt/homebrew/bin/delphi"));
        assert_ne!(confirmation("copilot", &made), confirmation("copilot", &plan), "the link is part of what is confirmed");
        assert!(plan.contains("not probed") && plan.contains("this would add"));
        assert!(!plan.contains("\nFORGED"), "a control character cannot start a new line");
        let c = confirmation("copilot", &plan);
        assert!(c.starts_with("CONNECT copilot ") && c.len() == "CONNECT copilot ".len() + 12);
        h.sha256 = "0".repeat(64);
        assert_ne!(confirmation("copilot", &render_plan(template("copilot").unwrap(), &h, &pre, true, &none, t.path())), c, "another plan needs another confirmation");
        let present = mcp_check::Check { state: mcp_check::State::Configured, detail: "x".into(), fix: None };
        assert!(render_plan(template("copilot").unwrap(), &h, &present, false, &none, t.path()).contains("connect would refuse"));
    }

    /// Spec 074 §10.2 item 1: `delphi update` names a plugin the program has outrun, and stays silent otherwise.
    #[test]
    fn update_names_a_stale_plugin_with_its_cure_and_says_nothing_for_a_host_without_one() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        let ok = mcp_check::Launchable { ok: true, this_binary: true, detail: "x".into() };
        let no = mcp_check::Launchable { ok: false, this_binary: false, detail: "nothing at the launch path".into() };
        let other = mcp_check::Launchable { ok: true, this_binary: false, detail: "a different `delphi`".into() };
        assert!(host_plugin_notes(home, "0.6.2", &no).is_empty(), "no plugin anywhere: nothing to say, even about the launch path");
        std::fs::create_dir_all(home.join(".claude/plugins")).unwrap();
        std::fs::write(home.join(".claude/plugins/installed_plugins.json"), r#"{"version":2,"plugins":{"delphi@delphi":[{"scope":"user","version":"0.5.4"}]}}"#).unwrap();
        std::fs::write(home.join(".claude/settings.json"), r#"{"enabledPlugins":{"delphi@delphi":true}}"#).unwrap();
        let notes = host_plugin_notes(home, "0.6.2", &ok);
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0].starts_with("claude-code: ") && notes[0].contains("plugin 0.5.4 is older than this binary (0.6.2)"), "{notes:?}");
        assert!(notes[0].contains("`delphi connect claude-code --update`") && notes[0].contains("`claude plugin update delphi@delphi`"), "{notes:?}");
        let notes = host_plugin_notes(home, "0.6.2", &no);
        assert!(notes.len() == 2 && notes[1].starts_with("host plugins: launchable: no (nothing at the launch path)"), "{notes:?}");
        // Current: silent about the release, and still says when it cannot be launched or when the plugin runs another install.
        assert!(host_plugin_notes(home, "0.5.4", &ok).is_empty());
        let skew = host_plugin_notes(home, "0.5.4", &other);
        assert!(skew.len() == 1 && skew[0].contains("launchable: yes (a different `delphi`)"), "{skew:?}");
        for tm in &TEMPLATES {
            assert!(tm.id.ends_with("-v2") && tm.proven_update != tm.proven_on, "{}: the update rows carry their own proof", tm.host);
        }
        assert_eq!(host_plugin_notes(home, "0.5.4", &no).len(), 1);
        // Codex has no update row: its own fix is printed.
        std::fs::create_dir_all(home.join(".codex/plugins/cache/delphi/delphi/0.5.4")).unwrap();
        std::fs::write(home.join(".codex/config.toml"), "[plugins.\"delphi@delphi\"]\nenabled = true\n").unwrap();
        let notes = host_plugin_notes(home, "0.6.2", &ok);
        assert!(notes.iter().any(|n| n.starts_with("codex: ") && n.contains("`codex plugin add delphi@delphi`") && !n.contains("--update")), "{notes:?}");
    }

    /// Spec 074 §10.3: the consented update is named beside the host's own command only where it exists and only for a stale plugin.
    #[test]
    fn the_update_pointer_is_for_a_stale_plugin_on_a_host_with_an_update_row() {
        use mcp_check::State;
        assert_eq!(update_pointer("claude-code", State::Stale).as_deref(), Some("delphi connect claude-code --update"));
        assert_eq!(update_pointer("copilot", State::Stale).as_deref(), Some("delphi connect copilot --update"));
        assert_eq!(update_pointer("codex", State::Stale), None, "codex has no row");
        for s in [State::Configured, State::Missing, State::Incompatible, State::Unknown, State::Unverified] {
            assert_eq!(update_pointer("claude-code", s), None);
        }
        assert!(AUTO_UPDATE_TIP.contains("/plugin") && AUTO_UPDATE_TIP.contains("Enable auto-update") && !AUTO_UPDATE_TIP.contains('\n'));
    }

    #[test]
    fn nothing_executes_without_a_terminal_and_flags_are_exclusive() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(cmd(&a(&["claude-code"])), 2);
        assert_eq!(cmd(&a(&["copilot"])), 2);
        assert_eq!(cmd(&a(&["vim", "--dry-run"])), 2);
        assert_eq!(cmd(&a(&["codex", "--status", "--remove"])), 2, "codex flags are exclusive too");
        assert_eq!(cmd(&a(&["claude-code", "--dry-run", "--status"])), 2);
        assert_eq!(cmd(&a(&["claude-code", "--update", "--remove"])), 2, "one mode at a time");
        assert_eq!(cmd(&a(&["claude-code", "--update", "--status"])), 2);
        assert_eq!(cmd(&a(&["codex", "--update"])), 2, "codex has no marketplace plugin that connect manages");
        assert_eq!(cmd(&a(&["gemini", "--update", "--dry-run"])), 2);
        // Spec 074 §11: a source is a GitHub owner/repo and nothing else, and only for the hosts with a marketplace.
        assert_eq!(cmd(&a(&["claude-code", "--dry-run", "--source"])), 2, "a value is needed");
        assert_eq!(cmd(&a(&["claude-code", "--dry-run", "--source", "--status"])), 2, "a flag is not a value");
        assert_eq!(cmd(&a(&["claude-code", "--dry-run", "--source", "https://example.com/x.git"])), 2);
        assert_eq!(cmd(&a(&["claude-code", "--dry-run", "--source", "fastly/delphi; rm -rf"])), 2);
        assert_eq!(cmd(&a(&["codex", "--source", "fastly/delphi"])), 2, "codex has no marketplace that connect manages");
        assert_eq!(cmd(&a(&["--source", "fastly/delphi"])), 2, "the value of --source is never taken for a host");
        assert_eq!(cmd(&a(&[])), 2);
    }
}
