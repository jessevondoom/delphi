//! Spec 106 §12 G7: `delphi connect claude-code --theme [--dry-run | --remove] [--light | --dark]`. Claude Code draws a subagent's
//! line in one of eight named colours whose shade its theme sets; this writes a theme that sets those eight (and nothing else) to
//! the colours of the roles that own them, and selects it. It writes two files it names first, `<claude>/themes/delphi.json` and
//! the `theme` key of `<claude>/settings.json`, after a `[Y/n]` at a terminal; it never replaces a theme file Delphi did not write,
//! never switches away from a custom theme of the operator's own, and `--remove` puts the earlier theme back.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde_json::Value;

use delphi_engine::mcp_hosts;

use crate::connect_cmd::sanitize;

/// The theme's file name without `.json`, and the preference Claude Code stores when it is selected (`custom:<slug>`).
pub const SLUG: &str = "delphi";
pub const PREF: &str = "custom:delphi";
/// The presets a custom theme may start from (Claude Code's custom-theme reference).
pub const BASES: [&str; 6] = ["dark", "light", "dark-daltonized", "light-daltonized", "dark-ansi", "light-ansi"];
const MAX_FILE: u64 = 1024 * 1024;

/// Delphi's theme for one base: the eight subagent colours, the light pair on a light base.
pub fn theme_text(base: &str) -> String {
    let overrides = mcp_hosts::claude_theme_overrides(base.starts_with("light"));
    let mut o = format!("{{\n  \"name\": \"Delphi\",\n  \"base\": \"{base}\",\n  \"overrides\": {{\n");
    for (i, (token, hex)) in overrides.iter().enumerate() {
        o.push_str(&format!("    \"{token}\": \"{hex}\"{}\n", if i + 1 < overrides.len() { "," } else { "" }));
    }
    o.push_str("  }\n}\n");
    o
}

/// Whether a theme file is one Delphi wrote, in this release or another: named `Delphi`, on a known base, and overriding nothing
/// but subagent colours. The base it names, when it is.
pub fn ours(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let o = v.as_object()?;
    if o.keys().any(|k| !matches!(k.as_str(), "name" | "base" | "overrides")) || o.get("name")?.as_str()? != "Delphi" {
        return None;
    }
    let base = o.get("base")?.as_str()?;
    let overrides = o.get("overrides")?.as_object()?;
    let known = |k: &str| mcp_hosts::CLAUDE_PALETTE.iter().any(|n| mcp_hosts::claude_theme_token(n) == k);
    (BASES.contains(&base) && overrides.iter().all(|(k, v)| known(k) && v.is_string())).then(|| base.to_string())
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while matches!(b.get(i), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        i += 1;
    }
    i
}

/// The index after the closing quote of the string that opens at `i`.
fn string_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i + 1;
    loop {
        match b.get(j)? {
            b'\\' => j += 2,
            b'"' => return Some(j + 1),
            _ => j += 1,
        }
    }
}

/// The index after the value that starts at `i`: a string, an object or array (brackets counted, strings skipped), or a bare word.
fn value_end(b: &[u8], i: usize) -> Option<usize> {
    match b.get(i)? {
        b'"' => string_end(b, i),
        b'{' | b'[' => {
            let (mut depth, mut j) = (0usize, i);
            loop {
                match b.get(j)? {
                    b'"' => {
                        j = string_end(b, j)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(j + 1);
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
        }
        _ => {
            let mut j = i;
            while j < b.len() && !matches!(b[j], b',' | b'}' | b' ' | b'\n' | b'\r' | b'\t') {
                j += 1;
            }
            (j > i).then_some(j)
        }
    }
}

/// Where the value of the object's own `key` sits in `text` (not a key of the same name deeper in), or `None` when it has none.
fn top_level_value(text: &str, key: &str) -> Result<Option<std::ops::Range<usize>>, String> {
    let bad = || "it is not a JSON object this command can read".to_string();
    let b = text.as_bytes();
    let mut i = skip_ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return Err(bad());
    }
    i += 1;
    loop {
        i = skip_ws(b, i);
        match b.get(i) {
            Some(b'}') => return Ok(None),
            Some(b'"') => {}
            _ => return Err(bad()),
        }
        let key_end = string_end(b, i).ok_or_else(bad)?;
        let name: String = serde_json::from_str(&text[i..key_end]).map_err(|_| bad())?;
        i = skip_ws(b, key_end);
        if b.get(i) != Some(&b':') {
            return Err(bad());
        }
        i = skip_ws(b, i + 1);
        let end = value_end(b, i).ok_or_else(bad)?;
        if name == key {
            return Ok(Some(i..end));
        }
        i = skip_ws(b, end);
        match b.get(i) {
            Some(b',') => i += 1,
            Some(b'}') => return Ok(None),
            _ => return Err(bad()),
        }
    }
}

/// `text` with its `theme` set to `value` and every other byte as it was. The result is parsed and compared with the original plus
/// that one change, so an edit that would alter anything else is refused instead of written.
pub fn set_theme(text: &str, value: &str) -> Result<String, String> {
    let before: Value = serde_json::from_str(text).map_err(|_| "it is not valid JSON".to_string())?;
    if !before.is_object() {
        return Err("it is not a JSON object".into());
    }
    let quoted = Value::String(value.to_string()).to_string();
    let out = match top_level_value(text, "theme")? {
        Some(r) => {
            if !text[r.clone()].starts_with('"') {
                return Err("its `theme` is not a string".into());
            }
            format!("{}{quoted}{}", &text[..r.start], &text[r.end..])
        }
        None => {
            let open = text.find('{').ok_or("it is not a JSON object")?;
            let next = skip_ws(text.as_bytes(), open + 1);
            if text.as_bytes().get(next) == Some(&b'}') {
                format!("{}{{\n  \"theme\": {quoted}\n}}{}", &text[..open], &text[next + 1..])
            } else {
                format!("{}{{\n  \"theme\": {quoted},{}", &text[..open], &text[open + 1..])
            }
        }
    };
    let mut want = before;
    want["theme"] = Value::String(value.to_string());
    match serde_json::from_str::<Value>(&out) {
        Ok(after) if after == want => Ok(out),
        _ => Err("the edit would change more than `theme`".into()),
    }
}

/// What the command would do, decided from the two files' text alone.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Write the theme and select it. `previous` is the `theme` the settings held (none: the key was absent).
    Apply { base: String, guessed: bool, previous: Option<String>, theme: String, settings: String },
    /// Delphi's theme is already selected; `refresh` is its file's new text when this release's colours differ.
    AlreadyOn { refresh: Option<String> },
    /// The operator selected a custom theme of their own, which is left alone; these are the lines to add to it.
    OwnTheme { current: String, lines: String },
}

/// `settings`: the text of `settings.json`, or none when there is no such file. `theme_file`: the text of `themes/delphi.json`,
/// or none. `light`: which base to use when the settings name none (`auto`, absent, or a name this release does not know).
pub fn plan(settings: Option<&str>, theme_file: Option<&str>, light: bool) -> Result<Plan, String> {
    let existing_base = match theme_file {
        Some(t) => Some(ours(t).ok_or("themes/delphi.json exists and is not a theme Delphi wrote; it was left alone")?),
        None => None,
    };
    let text = settings.unwrap_or("{}\n");
    let doc: Value = serde_json::from_str(text).map_err(|_| "settings.json is not valid JSON; it was left alone".to_string())?;
    let current = match doc.get("theme") {
        None => None,
        Some(Value::String(s)) => Some(s.clone()),
        Some(_) => return Err("settings.json has a `theme` that is not a string; it was left alone".into()),
    };
    if current.as_deref() == Some(PREF) {
        let base = existing_base.unwrap_or_else(|| if light { "light" } else { "dark" }.to_string());
        let want = theme_text(&base);
        return Ok(Plan::AlreadyOn { refresh: (theme_file != Some(want.as_str())).then_some(want) });
    }
    if let Some(c) = current.as_deref().filter(|c| c.starts_with("custom:")) {
        let lines: String = mcp_hosts::claude_theme_overrides(light).iter().map(|(t, hex)| format!("    \"{t}\": \"{hex}\",\n")).collect();
        return Ok(Plan::OwnTheme { current: c.to_string(), lines });
    }
    let (base, guessed) = match current.as_deref() {
        Some(c) if BASES.contains(&c) => (c.to_string(), false),
        _ => (if light { "light" } else { "dark" }.to_string(), true),
    };
    let settings = set_theme(text, PREF).map_err(|e| format!("settings.json: {e}; it was left alone"))?;
    Ok(Plan::Apply { theme: theme_text(&base), base, guessed, previous: current, settings })
}

/// The roles drawn in each of the eight names: the owner first, then any that share the name.
fn slot_lines(light: bool) -> String {
    let mut o = String::new();
    for (name, (_, hex)) in mcp_hosts::CLAUDE_PALETTE.iter().zip(mcp_hosts::claude_theme_overrides(light)) {
        let roles: Vec<&str> = mcp_hosts::CLAUDE_COLORS.iter().filter(|(_, c)| c == name).map(|(r, _)| *r).collect();
        let shared = if roles.len() > 1 { format!(" (also drawn in it: {})", roles[1..].join(", ")) } else { String::new() };
        o.push_str(&format!("    {name:<7} {hex}  {}{shared}\n", roles.first().copied().unwrap_or("")));
    }
    o
}

/// The plan as the operator reads it before the question.
pub fn render(p: &Plan, dir: &Path) -> String {
    let d = sanitize(&dir.display().to_string());
    match p {
        Plan::Apply { base, guessed, previous, .. } => {
            let was = previous.as_deref().map(|s| format!("\"{}\"", sanitize(s))).unwrap_or_else(|| "not set".into());
            let mut o = format!("Delphi's role colours for Claude Code\n  write   {d}/themes/delphi.json: the `{base}` preset with only the eight subagent colours changed:\n");
            o.push_str(&slot_lines(base.starts_with("light")));
            o.push_str(&format!("  change  {d}/settings.json: \"theme\" from {was} to \"{PREF}\"; nothing else in the file changes\n"));
            if *guessed {
                o.push_str(&format!("  note    your theme is {was}. A custom theme has one base and does not follow the terminal's light or dark background; this uses `{base}` (`--light` or `--dark` picks the other)\n"));
            }
            o.push_str("  undo    delphi connect claude-code --theme --remove\n");
            o
        }
        Plan::AlreadyOn { refresh: Some(_) } => format!("Delphi's theme is already selected; {d}/themes/delphi.json would be rewritten with this release's colours\n"),
        Plan::AlreadyOn { refresh: None } => "Delphi's theme is already selected and current; nothing to do\n".into(),
        Plan::OwnTheme { current, lines } => format!(
            "You use a custom theme of your own (\"{}\"), so Delphi does not switch it. To draw Delphi's roles in their colours, add these to its `overrides`:\n{lines}",
            sanitize(current)
        ),
    }
}

/// Claude Code's own folder: `CLAUDE_CONFIG_DIR` when it is an absolute path, else `~/.claude`.
pub fn claude_dir(home: &Path) -> PathBuf {
    std::env::var("CLAUDE_CONFIG_DIR").ok().map(PathBuf::from).filter(|p| p.is_absolute()).unwrap_or_else(|| home.join(".claude"))
}

/// A file's text, `None` when it does not exist. A symlink, a directory or an oversized file is refused, never followed or replaced.
fn read_plain(path: &Path) -> Result<Option<String>, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let m = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{name}: {e}")),
    };
    if !m.is_file() || m.len() > MAX_FILE {
        return Err(format!("{name} is a link or not a plain file of a sane size, so Delphi does not edit it; set \"theme\": \"{PREF}\" there yourself"));
    }
    std::fs::read_to_string(path).map(Some).map_err(|e| format!("{name}: {e}"))
}

fn write_atomic(path: &Path, text: &str, mode: u32) -> Result<(), String> {
    let dir = path.parent().ok_or("no parent folder")?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.delphi-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let done = (|| {
        let mut f = std::fs::OpenOptions::new().create_new(true).write(true).mode(mode).custom_flags(libc::O_NOFOLLOW).open(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done.map_err(|e| format!("{name}: {e}"))
}

fn record_path(delphi_home: &Path) -> PathBuf {
    delphi_home.join("operator/claude-code-theme.json")
}

fn load(dir: &Path, light: bool) -> Result<Plan, String> {
    plan(read_plain(&dir.join("settings.json"))?.as_deref(), read_plain(&dir.join("themes").join(format!("{SLUG}.json")))?.as_deref(), light)
}

/// Do what `p` says. The theme file is written first, so a failure never leaves the settings naming a theme that is not there.
pub fn apply(p: &Plan, dir: &Path, delphi_home: &Path) -> Result<String, String> {
    let theme_path = dir.join("themes").join(format!("{SLUG}.json"));
    match p {
        Plan::Apply { theme, settings, previous, .. } => {
            std::fs::create_dir_all(dir.join("themes")).map_err(|e| format!("themes: {e}"))?;
            write_atomic(&theme_path, theme, 0o644)?;
            let record = record_path(delphi_home);
            if let Some(parent) = record.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            write_atomic(&record, &serde_json::json!({ "previous": previous }).to_string(), 0o600)?;
            let settings_path = dir.join("settings.json");
            let mode = std::fs::metadata(&settings_path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o600);
            write_atomic(&settings_path, settings, mode)?;
            Ok("Delphi's theme is selected. A running Claude Code session picks it up; `/theme` changes it back at any time.".into())
        }
        Plan::AlreadyOn { refresh: Some(theme) } => {
            write_atomic(&theme_path, theme, 0o644)?;
            Ok("themes/delphi.json rewritten with this release's colours.".into())
        }
        Plan::AlreadyOn { refresh: None } | Plan::OwnTheme { .. } => Ok(String::new()),
    }
}

/// What `--remove` would do: the theme to put back (when Delphi's is selected) and whether its file goes.
pub struct Removal {
    pub restore: Option<(String, String)>,
    pub delete_theme: bool,
}

pub fn removal(dir: &Path, delphi_home: &Path) -> Result<Removal, String> {
    let theme_file = read_plain(&dir.join("themes").join(format!("{SLUG}.json")))?;
    let base = theme_file.as_deref().and_then(ours);
    let settings = read_plain(&dir.join("settings.json"))?;
    let selected = settings.as_deref().and_then(|t| serde_json::from_str::<Value>(t).ok()).and_then(|v| v.get("theme").and_then(Value::as_str).map(str::to_string));
    let restore = match (selected.as_deref(), settings.as_deref()) {
        (Some(PREF), Some(text)) => {
            let recorded = read_plain(&record_path(delphi_home)).ok().flatten().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.get("previous").and_then(Value::as_str).map(str::to_string));
            // With nothing recorded (the key was absent, or the record is gone) the theme's own base is the look the operator had.
            let to = recorded.filter(|p| p != PREF).or(base.clone()).unwrap_or_else(|| "dark".into());
            Some((to.clone(), set_theme(text, &to).map_err(|e| format!("settings.json: {e}; it was left alone"))?))
        }
        _ => None,
    };
    Ok(Removal { restore, delete_theme: base.is_some() })
}

pub fn remove(r: &Removal, dir: &Path, delphi_home: &Path) -> Result<String, String> {
    let mut did = Vec::new();
    if let Some((to, text)) = &r.restore {
        let settings_path = dir.join("settings.json");
        let mode = std::fs::metadata(&settings_path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o600);
        write_atomic(&settings_path, text, mode)?;
        did.push(format!("theme set back to \"{}\"", sanitize(to)));
    }
    if r.delete_theme {
        std::fs::remove_file(dir.join("themes").join(format!("{SLUG}.json"))).map_err(|e| format!("themes/delphi.json: {e}"))?;
        did.push("themes/delphi.json removed".into());
    }
    let _ = std::fs::remove_file(record_path(delphi_home));
    Ok(if did.is_empty() { "nothing of Delphi's theme is present; nothing changed".into() } else { did.join("; ") })
}

fn at_a_terminal() -> bool {
    // SAFETY: isatty has no preconditions.
    unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 }
}

/// `[Y/n]`: an empty line, `y` or `yes` is a yes; anything else, or no line at all, is a no.
fn yes(question: &str) -> bool {
    print!("{question} [Y/n] ");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    match std::io::stdin().read_line(&mut s) {
        Ok(n) if n > 0 => matches!(s.trim().to_lowercase().as_str(), "" | "y" | "yes"),
        _ => false,
    }
}

fn usage() -> i32 {
    eprintln!("usage: delphi connect claude-code --theme [--dry-run | --remove] [--light | --dark]");
    2
}

/// `delphi connect claude-code --theme ...`; `flags` are the arguments after the host.
pub fn cmd(flags: &[&str], home: &Path, delphi_home: &Path) -> i32 {
    let has = |f: &str| flags.contains(&f);
    if flags.iter().any(|a| !matches!(*a, "--theme" | "--dry-run" | "--remove" | "--light" | "--dark")) || (has("--light") && has("--dark")) || (has("--dry-run") && has("--remove")) {
        return usage();
    }
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi connect: changing a host's theme is a human act and is refused inside an agent seat");
        return 2;
    }
    let dir = claude_dir(home);
    if has("--remove") {
        let r = match removal(&dir, delphi_home) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("delphi connect: {e}");
                return 1;
            }
        };
        if r.restore.is_none() && !r.delete_theme {
            println!("nothing of Delphi's theme is present; nothing changed");
            return 0;
        }
        if !at_a_terminal() {
            eprintln!("delphi connect: removing the theme needs a terminal on both stdin and stdout");
            return 2;
        }
        let what = r.restore.as_ref().map(|(to, _)| format!("Set Claude Code's theme back to \"{}\" and remove Delphi's theme file?", sanitize(to))).unwrap_or_else(|| "Remove Delphi's theme file?".into());
        if !yes(&what) {
            println!("left as it is");
            return 0;
        }
        return match remove(&r, &dir, delphi_home) {
            Ok(m) => {
                println!("{m}");
                0
            }
            Err(e) => {
                eprintln!("delphi connect: {e}");
                1
            }
        };
    }
    let p = match load(&dir, has("--light")) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("delphi connect: {e}");
            return 1;
        }
    };
    print!("{}", render(&p, &dir));
    if has("--dry-run") || matches!(p, Plan::AlreadyOn { refresh: None } | Plan::OwnTheme { .. }) {
        return 0;
    }
    if !at_a_terminal() {
        eprintln!("delphi connect: selecting the theme needs a terminal on both stdin and stdout (use --dry-run to preview)");
        return 2;
    }
    finish(&p, &dir, delphi_home)
}

fn finish(p: &Plan, dir: &Path, delphi_home: &Path) -> i32 {
    let question = if matches!(p, Plan::Apply { .. }) { "Use Delphi's role colours in Claude Code?" } else { "Rewrite Delphi's theme file?" };
    if !yes(question) {
        println!("left as it is; `delphi connect claude-code --theme` asks again");
        return 0;
    }
    match apply(p, dir, delphi_home) {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("delphi connect: {e}");
            1
        }
    }
}

/// After `delphi connect claude-code` succeeds (already at a terminal, outside an agent seat): offer the theme once. A refusal to
/// plan is a tip, not a failure of the connect that just worked.
pub fn offer(home: &Path, delphi_home: &Path) {
    let dir = claude_dir(home);
    match load(&dir, false) {
        Ok(p @ Plan::Apply { .. }) => {
            print!("\n{}", render(&p, &dir));
            finish(&p, &dir, delphi_home);
        }
        Ok(_) => {}
        Err(e) => println!("tip: Delphi's role colours for Claude Code were not offered: {e}. `delphi connect claude-code --theme --dry-run` says more."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_theme_is_eight_subagent_colours_on_the_named_base_and_is_recognised_as_delphis() {
        for base in BASES {
            let t = theme_text(base);
            let v: Value = serde_json::from_str(&t).unwrap();
            assert_eq!((v["name"].as_str(), v["base"].as_str()), (Some("Delphi"), Some(base)));
            let o = v["overrides"].as_object().unwrap();
            assert_eq!(o.len(), 8);
            assert!(o.keys().all(|k| k.ends_with("_FOR_SUBAGENTS_ONLY")));
            assert_eq!(ours(&t).as_deref(), Some(base));
        }
        assert_eq!(serde_json::from_str::<Value>(&theme_text("dark")).unwrap()["overrides"]["purple_FOR_SUBAGENTS_ONLY"], "#a855f7");
        assert_eq!(serde_json::from_str::<Value>(&theme_text("light-ansi")).unwrap()["overrides"]["purple_FOR_SUBAGENTS_ONLY"], "#7e22ce");
        // A theme with another name, another token or another field is the operator's, not Delphi's.
        assert!(ours(&theme_text("dark").replace("Delphi", "Mine")).is_none());
        assert!(ours(&theme_text("dark").replace("purple_FOR_SUBAGENTS_ONLY", "claude")).is_none());
        assert!(ours(r#"{"name":"Delphi","base":"dark","overrides":{},"extra":1}"#).is_none());
        assert!(ours("not json").is_none());
        // An older release's file (fewer overrides) is still Delphi's, so a later release may refresh it.
        assert_eq!(ours(r##"{"name":"Delphi","base":"light","overrides":{"red_FOR_SUBAGENTS_ONLY":"#000000"}}"##).as_deref(), Some("light"));
    }

    #[test]
    fn only_the_theme_value_changes_and_a_nested_or_quoted_theme_is_not_mistaken_for_it() {
        let text = "{\n  \"env\": { \"theme\": \"nested\", \"x\": \"a } \\\" { b\" },\n  \"theme\": \"auto\",\n  \"list\": [1, {\"theme\": 2}],\n  \"last\": true\n}\n";
        let out = set_theme(text, PREF).unwrap();
        assert_eq!(out, text.replace("\"theme\": \"auto\"", "\"theme\": \"custom:delphi\""));
        // Put back, the file is byte for byte what it was.
        assert_eq!(set_theme(&out, "auto").unwrap(), text);
        // No key: it is added first, and everything after the brace is untouched.
        let none = "{\n  \"model\": \"x\",\n  \"n\": [1, 2]\n}\n";
        assert_eq!(set_theme(none, PREF).unwrap(), "{\n  \"theme\": \"custom:delphi\",\n  \"model\": \"x\",\n  \"n\": [1, 2]\n}\n");
        assert_eq!(set_theme("{}\n", PREF).unwrap(), "{\n  \"theme\": \"custom:delphi\"\n}\n");
        assert_eq!(set_theme("{ }", PREF).unwrap(), "{\n  \"theme\": \"custom:delphi\"\n}");
        // Refused: not an object, not JSON, a theme that is not a string, and a duplicate key (the edit would not be the one that counts).
        for bad in ["[1]", "{", "// c\n{}", "{\"theme\": 3}", "{\"theme\": \"a\", \"theme\": \"b\"}"] {
            assert!(set_theme(bad, PREF).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_plan_keeps_a_named_base_guesses_for_auto_and_leaves_a_custom_theme_alone() {
        let Plan::Apply { base, guessed, previous, theme, settings } = plan(Some("{\"theme\": \"light-daltonized\"}"), None, false).unwrap() else { panic!() };
        assert_eq!((base.as_str(), guessed, previous.as_deref()), ("light-daltonized", false, Some("light-daltonized")));
        assert_eq!(ours(&theme).as_deref(), Some("light-daltonized"));
        assert_eq!(settings, "{\"theme\": \"custom:delphi\"}");
        for (settings, light, want) in [(Some("{\"theme\": \"auto\"}"), false, "dark"), (Some("{\"theme\": \"auto\"}"), true, "light"), (Some("{\"a\": 1}"), false, "dark"), (None, true, "light"), (Some("{\"theme\": \"someday\"}"), false, "dark")] {
            let Plan::Apply { base, guessed, .. } = plan(settings, None, light).unwrap() else { panic!() };
            assert_eq!((base.as_str(), guessed), (want, true), "{settings:?}");
        }
        let Plan::Apply { settings, previous, .. } = plan(None, None, false).unwrap() else { panic!() };
        assert_eq!((settings.as_str(), previous), ("{\n  \"theme\": \"custom:delphi\"\n}\n", None));
        let Plan::OwnTheme { current, lines } = plan(Some("{\"theme\": \"custom:dracula\"}"), None, false).unwrap() else { panic!() };
        assert_eq!(current, "custom:dracula");
        assert_eq!(lines.lines().count(), 8);
        assert!(lines.contains("\"purple_FOR_SUBAGENTS_ONLY\": \"#a855f7\","));
        // Already selected: current is nothing to do; another release's file is refreshed on its own base.
        assert_eq!(plan(Some("{\"theme\": \"custom:delphi\"}"), Some(&theme_text("light")), false).unwrap(), Plan::AlreadyOn { refresh: None });
        let old = r##"{"name":"Delphi","base":"light","overrides":{"red_FOR_SUBAGENTS_ONLY":"#000000"}}"##;
        assert_eq!(plan(Some("{\"theme\": \"custom:delphi\"}"), Some(old), false).unwrap(), Plan::AlreadyOn { refresh: Some(theme_text("light")) });
        // Refused: a delphi.json that is not Delphi's, settings that are not JSON, a theme that is not a string.
        assert!(plan(Some("{}"), Some("{\"name\": \"Mine\"}"), false).unwrap_err().contains("not a theme Delphi wrote"));
        assert!(plan(Some("{ nope"), None, false).is_err());
        assert!(plan(Some("{\"theme\": 1}"), None, false).is_err());
        let p = plan(Some("{\"theme\": \"auto\"}"), None, false).unwrap();
        let shown = render(&p, Path::new("/h/.claude"));
        assert!(shown.contains("from \"auto\" to \"custom:delphi\"") && shown.contains("does not follow the terminal") && shown.contains("purple  #a855f7  orchestrator") && shown.contains("--theme --remove"), "{shown}");
        assert!(shown.contains("blue    #38bdf8  architect (also drawn in it: investigator, administrator, assistant)"), "{shown}");
    }

    #[test]
    fn apply_then_remove_leaves_the_settings_byte_for_byte_and_never_touches_a_file_that_is_not_delphis() {
        let t = tempfile::tempdir().unwrap();
        let (dir, dh) = (t.path().join("claude"), t.path().join("delphi"));
        std::fs::create_dir_all(&dir).unwrap();
        let original = "{\n  \"model\": \"x\",\n  \"theme\": \"auto\",\n  \"hooks\": {}\n}\n";
        std::fs::write(dir.join("settings.json"), original).unwrap();
        std::fs::set_permissions(dir.join("settings.json"), std::fs::Permissions::from_mode(0o640)).unwrap();
        let p = load(&dir, false).unwrap();
        apply(&p, &dir, &dh).unwrap();
        let now = std::fs::read_to_string(dir.join("settings.json")).unwrap();
        assert_eq!(now, original.replace("auto", "custom:delphi"));
        assert_eq!(std::fs::metadata(dir.join("settings.json")).unwrap().permissions().mode() & 0o777, 0o640, "the file's mode is kept");
        assert_eq!(ours(&std::fs::read_to_string(dir.join("themes/delphi.json")).unwrap()).as_deref(), Some("dark"));
        assert_eq!(load(&dir, false).unwrap(), Plan::AlreadyOn { refresh: None });
        let r = removal(&dir, &dh).unwrap();
        assert_eq!(r.restore.as_ref().map(|(to, _)| to.as_str()), Some("auto"));
        remove(&r, &dir, &dh).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("settings.json")).unwrap(), original);
        assert!(!dir.join("themes/delphi.json").exists() && !record_path(&dh).exists());
        let again = removal(&dir, &dh).unwrap();
        assert!(again.restore.is_none() && !again.delete_theme, "a second remove finds nothing");

        // No settings file at all: one is created, and with no recorded theme a remove falls back to the theme's base.
        let (dir2, dh2) = (t.path().join("c2"), t.path().join("d2"));
        std::fs::create_dir_all(&dir2).unwrap();
        apply(&load(&dir2, true).unwrap(), &dir2, &dh2).unwrap();
        assert_eq!(std::fs::read_to_string(dir2.join("settings.json")).unwrap(), "{\n  \"theme\": \"custom:delphi\"\n}\n");
        assert_eq!(removal(&dir2, &dh2).unwrap().restore.unwrap().0, "light");

        // Someone else's delphi.json and a linked settings.json are refused before anything is written.
        let dir3 = t.path().join("c3");
        std::fs::create_dir_all(dir3.join("themes")).unwrap();
        std::fs::write(dir3.join("themes/delphi.json"), "{\"name\": \"Mine\"}").unwrap();
        std::fs::write(dir3.join("settings.json"), "{}").unwrap();
        assert!(load(&dir3, false).is_err());
        assert!(!removal(&dir3, &dh).unwrap().delete_theme, "a remove leaves it too");
        let dir4 = t.path().join("c4");
        std::fs::create_dir_all(&dir4).unwrap();
        std::fs::write(t.path().join("real.json"), "{\"theme\": \"dark\"}").unwrap();
        std::os::unix::fs::symlink(t.path().join("real.json"), dir4.join("settings.json")).unwrap();
        assert!(load(&dir4, false).unwrap_err().contains("link"));
        assert_eq!(std::fs::read_to_string(t.path().join("real.json")).unwrap(), "{\"theme\": \"dark\"}");
    }

    #[test]
    fn the_command_refuses_bad_flags_and_writes_nothing_without_a_terminal() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path();
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/settings.json"), "{\"theme\": \"dark\"}").unwrap();
        let dh = home.join("d");
        assert_eq!(cmd(&["--theme", "--nope"], home, &dh), 2);
        assert_eq!(cmd(&["--theme", "--light", "--dark"], home, &dh), 2);
        assert_eq!(cmd(&["--theme", "--dry-run", "--remove"], home, &dh), 2);
        if std::env::var_os("CLAUDE_CONFIG_DIR").is_none() && !crate::xenia_terminal::inside_an_agent_seat() {
            assert_eq!(cmd(&["--theme", "--dry-run"], home, &dh), 0);
            // Tests have no terminal on stdin and stdout, so the bare form stops before the question.
            if !at_a_terminal() {
                assert_eq!(cmd(&["--theme"], home, &dh), 2);
            }
            assert_eq!(std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(), "{\"theme\": \"dark\"}");
            assert!(!home.join(".claude/themes").exists());
            assert_eq!(cmd(&["--theme", "--remove"], home, &dh), 0, "nothing present is not an error");
        }
    }
}
