//! The release seed embedded in the binary (`specs/006` §2.1). This repository is the install source.

use include_dir::{include_dir, Dir};

static AGENTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../agents");
static SKILLS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../skills");
static THEMES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../themes");
static CONTEXT: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../context");
static MEMORY: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../memory");
pub const CONFIG_TEMPLATE: &str = include_str!("../config.example.toml");

/// Which zone a seed file is installed into (`specs/006` §3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// Zone 1: release snapshot (`base/`).
    Base,
    /// Zone 2: team-shared seed (`team/`).
    Team,
    /// Zone 3: operator-private seed (`local/`).
    Local,
}

pub struct SeedFile {
    pub logical: String,
    pub zone: Zone,
    pub bytes: &'static [u8],
}

fn collect(dir: &'static Dir<'static>, prefix: &str, zone: Zone, out: &mut Vec<SeedFile>) {
    for f in dir.files() {
        out.push(SeedFile { logical: format!("{}/{}", prefix, f.path().to_string_lossy()), zone, bytes: f.contents() });
    }
    for d in dir.dirs() {
        collect(d, prefix, zone, out);
    }
}

/// All seed files with their logical paths.
pub fn files() -> Vec<SeedFile> {
    let mut v = vec![
        SeedFile { logical: "GOVERNANCE.md".into(), zone: Zone::Base, bytes: include_bytes!("../../../GOVERNANCE.md") },
        SeedFile { logical: "SPEC.md".into(), zone: Zone::Base, bytes: include_bytes!("../../../SPEC.md") },
        SeedFile { logical: "REVIEW.md".into(), zone: Zone::Team, bytes: include_bytes!("../../../REVIEW.md") },
    ];
    collect(&AGENTS, "agents", Zone::Base, &mut v);
    collect(&SKILLS, "skills", Zone::Base, &mut v);
    collect(&THEMES, "themes", Zone::Base, &mut v);
    collect(&CONTEXT, "context", Zone::Team, &mut v);
    collect(&MEMORY, "memory", Zone::Local, &mut v);
    // 006-A2: release heuristics are part of the release snapshot (Zone 1), not team content.
    for f in v.iter_mut().filter(|f| f.logical == "context/heuristics.md") {
        f.zone = Zone::Base;
    }
    v
}

/// The built-in manifest of a canonical role (`agents/<role>/AGENT.md`), as shipped in this binary.
pub fn builtin_agent(role: &str) -> Option<&'static str> {
    AGENTS.get_file(format!("{}/AGENT.md", role))?.contents_utf8()
}

/// Built-in theme names (`themes/<name>/theme.md`).
pub fn builtin_themes() -> Vec<String> {
    THEMES.dirs().map(|d| d.path().to_string_lossy().to_string()).filter(|n| THEMES.get_file(format!("{}/theme.md", n)).is_some()).collect()
}
