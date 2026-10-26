//! `delphi bundle init <dir> --org <org> --name <name>` (`specs/027` §2.1): a skeleton that passes `validate` at once
//! and says what to write next. Offline, no model, and never inside a registered project repository (D-03): a bundle is
//! its own directory, and a file written into a project could be committed and shipped by accident.

use std::fs;
use std::path::{Path, PathBuf};

/// The nearest ancestor of `dir` that exists, canonical, so a not-yet-created directory can be placed.
fn existing_ancestor(dir: &Path) -> Option<PathBuf> {
    let abs = if dir.is_absolute() { dir.to_path_buf() } else { std::env::current_dir().ok()?.join(dir) };
    let mut cur = abs;
    loop {
        if let Ok(c) = fs::canonicalize(&cur) {
            return Some(c);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// The registered project (of any workspace under `home`) that contains `dir`, if there is one.
pub fn project_containing(home: &Path, dir: &Path) -> Option<PathBuf> {
    let at = existing_ancestor(dir)?;
    let entries = fs::read_dir(home.join("workspaces")).ok()?;
    entries.flatten().flat_map(|e| crate::home::projects(&e.path())).find(|p| at.starts_with(p))
}

fn one_year_on(today: &str) -> String {
    let (y, rest) = today.split_at(4);
    let year: u32 = y.parse().unwrap_or(2026);
    // 29 February has no successor; the 28th is a year out enough.
    let rest = if rest == "-02-29" { "-02-28" } else { rest };
    format!("{}{}", year + 1, rest)
}

fn front(id: &str, title: &str, summary: &str, tag: &str, name: &str, now: &str) -> String {
    format!("---\nid: {id}\ntitle: \"{title}\"\nauthor: operator\ntimestamp: \"{now}\"\nsource_type: internal_generated\nconfidence: medium\nstatus: active\nsource_mission: {name}\ntags: [{tag}]\nsummary: \"{summary}\"\n---\n")
}

/// The files of a new bundle, as (path, text). `now` is `YYYY-MM-DDTHH:MM:SSZ`.
pub fn skeleton(org: &str, name: &str, now: &str, delphi_version: &str) -> Vec<(String, String)> {
    let today = &now[..10];
    let review = one_year_on(today);
    let toml = format!(
        "[bundle]\nschema = 1\nname = \"{name}\"\norg = \"{org}\"\nversion = \"0.1.0\"\ntitle = \"{name}\"\ndescription = \"What {org} knows about <topic>, in one sentence a stranger can judge the bundle by.\"\nlicense = \"Proprietary\"\naudience = \"org\"\nreview_by = \"{review}\"\nlanguage = \"en\"\n\n[compat]\ndelphi = \">={delphi_version}\"\nbundle_schema = 1\n\n[publisher]\nname = \"{org}\"\n\n[sections]\nknowledge = true\nbehavior = true\n\n[touches]\nroles = []\nconnectors = []\nnetwork = false\n"
    );
    let readme = format!(
        "# {name}\n\n<!-- Replace this skeleton. Adding a bundle is the operator's approval, and this file is what they read first. -->\n\nAdding this bundle gives your Delphi install what {org} knows about <topic>.\n\n**What loads when you add it**\n\n- `knowledge/`: documents your agents can search; never always loaded.\n- `behavior/`: goals, perspectives, role guidance and rules, if the bundle has any; loaded on add.\n\n**What it asks of you:** nothing beyond reading this file. It carries no hooks, no credentials and no permission grants. Remove it with `delphi bundle remove {name}`.\n"
    );
    let index = format!("{}\n- [Example document](example.md): Replace this with a real document, then list every knowledge file here.\n", front(&format!("{name}/index"), "Knowledge index", &format!("Where to start in the {name} bundle."), "index", name, now));
    let example = format!("{}\n## What this is\nA placeholder so the bundle validates. Replace it with something {org} knows that an agent could not guess: an architecture, a decision and why, a runbook, a vocabulary. Write facts, never instructions; instructions belong in `behavior/`.\n", front(&format!("{name}/example"), "Example document", "A placeholder; replace it with real knowledge.", "example", name, now));
    let goals = format!("{}\n<!-- Goals look like this (at most 12 active, 600 characters each). Never invent one; write only what your organization has decided.\n\n## [G-{org}-01] A measurable outcome\n- **Horizon:** 2026-Q4\n- **Owner role:** architect\n- **Measure:** the number, where it is read\n- **Status:** active\n- **Why:** one sentence\n-->\n", front(&format!("{name}/behavior/goals"), "Goals", "This bundle's goals.", "goals", name, now));
    let persp = format!("{}\n<!-- Perspectives look like this (at most 12, 350 characters each).\n\n## [P-{org}-01] A habit of judgment\n- **Statement:** choose X unless Y.\n- **Apply when:** the situation.\n- **Counter-case:** when the habit does not hold.\n-->\n", front(&format!("{name}/behavior/perspectives"), "Perspectives", "How this organization weighs choices.", "perspectives", name, now));
    vec![
        ("bundle.toml".into(), toml),
        ("README.md".into(), readme),
        ("CHANGELOG.md".into(), "# Changelog\n\n## 0.1.0\n- First version.\n".into()),
        ("knowledge/index.md".into(), index),
        ("knowledge/example.md".into(), example),
        ("behavior/goals.md".into(), goals),
        ("behavior/perspectives.md".into(), persp),
    ]
}

fn valid_org(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_name(s: &str) -> bool {
    s.len() <= 48 && s.bytes().next().is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// What to do next, for the author.
pub const NEXT: &str = "next:\n  1. edit bundle.toml (title, description, publisher) and README.md: what adding this gives a team, and what it asks\n  2. write knowledge/ files (frontmatter as in knowledge/example.md) and list each in knowledge/index.md\n  3. optional: behavior/goals.md and perspectives.md (never invented; write what is decided), skills/, agents/, evals/\n  4. `delphi bundle validate <dir> --report <file outside the bundle>` and work every finding\n  5. `delphi bundle package <dir>`, and `delphi bundle sign <dir>` if you hold a publisher key";

/// Create the skeleton at `dir`. Refuses a directory that has files in it and a place inside a registered project.
pub fn init(home: &Path, dir: &Path, org: &str, name: &str, now: &str) -> Result<Vec<String>, String> {
    if !valid_org(org) {
        return Err(format!("--org `{}` must be 1 to 32 characters of a-z, 0-9 and -", org));
    }
    if !valid_name(name) {
        return Err(format!("--name `{}` must be kebab-case: a-z, 0-9 and -, at most 48 characters, starting with a letter or digit", name));
    }
    if let Some(p) = project_containing(home, dir) {
        return Err(format!("{} is inside the registered project {}; a bundle lives in its own directory (D-03)", dir.display(), p.display()));
    }
    if dir.exists() && (!dir.is_dir() || fs::read_dir(dir).map_err(|e| e.to_string())?.next().is_some()) {
        return Err(format!("{} exists and is not an empty directory", dir.display()));
    }
    let mut written = Vec::new();
    for (rel, text) in skeleton(org, name, now, env!("CARGO_PKG_VERSION")) {
        let p = dir.join(&rel);
        fs::create_dir_all(p.parent().unwrap_or(dir)).map_err(|e| format!("{}: {}", rel, e))?;
        fs::write(&p, text).map_err(|e| format!("{}: {}", rel, e))?;
        written.push(rel);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle_cmd::Real;
    use delphi_bundle::Options;

    const NOW: &str = "2026-10-26T09:00:00Z";

    #[test]
    fn a_fresh_skeleton_validates_and_a_goal_over_600_characters_then_fails_the_same_check() {
        let t = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let dir = t.path().join("acme-kit");
        let written = init(home.path(), &dir, "acme", "acme-kit", NOW).unwrap();
        assert!(written.contains(&"bundle.toml".to_string()) && written.contains(&"knowledge/index.md".to_string()));
        let r = delphi_bundle::validate(&dir, &Options::default(), &Real);
        assert!(r.passed(), "{}", r.render("skeleton"));
        assert!(r.fails.is_empty() && r.warns.iter().all(|w| w.step != 8), "{}", r.render("skeleton"));
        // review_by is a year out, so `package` (which makes a past date an error) accepts it
        let m = r.manifest.unwrap();
        assert_eq!((m.org.as_str(), m.name.as_str(), m.version.as_str()), ("acme", "acme-kit", "0.1.0"));
        assert!(std::fs::read_to_string(dir.join("bundle.toml")).unwrap().contains("review_by = \"2027-10-26\""));
        let p = delphi_bundle::validate(&dir, &Options { packaging: true, ..Options::default() }, &Real);
        assert!(p.passed(), "{}", p.render("skeleton"));
        // a goal over the 600-character cap fails (the deck's check is this validate)
        let goals = std::fs::read_to_string(dir.join("behavior/goals.md")).unwrap();
        let long = format!("{}\n## [G-acme-01] A goal\n- **Horizon:** 2026-Q4\n- **Owner role:** architect\n- **Measure:** {}\n- **Status:** active\n- **Why:** because\n", goals, "m".repeat(700));
        std::fs::write(dir.join("behavior/goals.md"), long).unwrap();
        let r = delphi_bundle::validate(&dir, &Options::default(), &Real);
        assert!(r.fails.iter().any(|f| f.step == 10 && f.at.contains("goals.md")), "{}", r.render("skeleton"));
    }

    #[test]
    fn init_refuses_a_bad_name_a_used_directory_and_a_place_inside_a_registered_project() {
        let t = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        assert!(init(home.path(), &t.path().join("a"), "Acme", "x", NOW).unwrap_err().contains("--org"));
        assert!(init(home.path(), &t.path().join("a"), "acme", "Bad_Name", NOW).unwrap_err().contains("--name"));
        let used = t.path().join("used");
        std::fs::create_dir_all(&used).unwrap();
        std::fs::write(used.join("keep.txt"), "x").unwrap();
        assert!(init(home.path(), &used, "acme", "kit", NOW).unwrap_err().contains("not an empty directory"));
        assert_eq!(std::fs::read_to_string(used.join("keep.txt")).unwrap(), "x", "an existing file is never touched");
        // a project registered in a workspace of this home
        let project = t.path().join("repo");
        std::fs::create_dir_all(project.join("sub")).unwrap();
        let ws = home.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("config.toml"), format!("[workspace]\nprojects = [\"{}\"]\n", project.display())).unwrap();
        let e = init(home.path(), &project.join("sub/bundle"), "acme", "kit", NOW).unwrap_err();
        assert!(e.contains("registered project"), "{}", e);
        assert!(!project.join("sub/bundle").exists(), "nothing was created");
        assert!(init(home.path(), &t.path().join("elsewhere"), "acme", "kit", NOW).is_ok());
    }

    #[test]
    fn a_leap_day_has_a_year_out() {
        assert_eq!(one_year_on("2028-02-29"), "2029-02-28");
        assert_eq!(one_year_on("2026-10-26"), "2027-10-26");
    }
}
