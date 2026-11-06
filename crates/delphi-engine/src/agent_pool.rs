//! The name pool for one-off agents (`specs/087` §4).
//!
//! A pool is a list in the theme pack (`themes/<theme>/theme.md`): a slot id (`pool-01`..), a display name, 3 to 5 keywords
//! and a badge face. It has no role, no scope and no authority: a pool name is a label. Code in this repository refers to
//! slots and role IDs only; the name comes out of the pack at display time and is never written in a Rust source.
//!
//! This module parses the pool, checks it ([`validate`]) and chooses an entry for an agent ([`pick`]). Both are pure.

use serde_yaml::Value;
use std::collections::HashSet;

/// How many entries a pool has (`specs/087` §4.1).
pub const POOL_SIZE: usize = 36;
/// The characters a pool or core face may use: width 1 in common terminal fonts, plus the pool-only ambiguous-width ones
/// (`specs/087` §6.2).
pub const ALLOWED_FACE_CHARS: &str = "•·°^_-─=.ouwO~><*¬×‿≈ωᴥ☆✦◉◔■▪";
/// The two generic faces a pool entry may share with a core role (`specs/087` §4.1).
pub const GENERIC_FACES: [&str; 2] = ["•_•", "•─•"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolEntry {
    /// `pool-01`..`pool-36`: what events, stored data and code refer to.
    pub slot: String,
    /// The display name, from the theme pack only.
    pub name: String,
    pub keywords: Vec<String>,
    /// Three cells, the badge face between `⟦` and `⟧`.
    pub face: String,
}

/// An agent that is live in a session: the identity it was given stays its own for its life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveAgent {
    pub agent_id: String,
    pub role: String,
    pub slot: String,
    /// 1 for the first agent to wear a slot, 2 for the second (`<name> II`), and so on.
    pub numeral: u32,
}

/// A chosen identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub slot: String,
    /// The name as displayed, with a roman numeral after it when the slot is already worn (`<name> II`).
    pub name: String,
    pub face: String,
    pub numeral: u32,
}

/// The pool in a theme pack's mapping. A pack with no `pool` key has an empty pool (dynamic agents then have no pool name);
/// one with the key must be well formed.
pub fn pool_from_value(root: &Value) -> Result<Vec<PoolEntry>, Vec<String>> {
    let Some(list) = root.get("pool") else { return Ok(Vec::new()) };
    let Some(seq) = list.as_sequence() else { return Err(vec!["`pool` must be a list".to_string()]) };
    let mut out = Vec::new();
    let mut errs = Vec::new();
    for (i, item) in seq.iter().enumerate() {
        let text = |k: &str| item.get(k).and_then(Value::as_str).map(str::to_string);
        let keywords: Option<Vec<String>> = item.get("keywords").and_then(Value::as_sequence).map(|s| s.iter().filter_map(|k| k.as_str().map(str::to_string)).collect());
        match (text("slot"), text("name"), keywords, text("face")) {
            (Some(slot), Some(name), Some(keywords), Some(face)) => out.push(PoolEntry { slot, name, keywords, face }),
            _ => errs.push(format!("pool[{}] needs slot, name, keywords and face", i + 1)),
        }
    }
    if errs.is_empty() { Ok(out) } else { Err(errs) }
}

/// The glyph table of the pack: each non-ASCII glyph a face uses, and the one ASCII character that stands for it
/// (`specs/087` §6.2).
pub fn ascii_glyphs_from_value(root: &Value) -> Result<Vec<(char, char)>, Vec<String>> {
    let Some(m) = root.get("ascii_glyphs") else { return Ok(Vec::new()) };
    let Some(map) = m.as_mapping() else { return Err(vec!["`ascii_glyphs` must be a mapping".to_string()]) };
    let mut out = Vec::new();
    let mut errs = Vec::new();
    for (k, v) in map {
        let (k, v) = (k.as_str().unwrap_or(""), v.as_str().unwrap_or(""));
        let (mut kc, mut vc) = (k.chars(), v.chars());
        match (kc.next(), kc.next(), vc.next(), vc.next()) {
            (Some(a), None, Some(b), None) => out.push((a, b)),
            _ => errs.push(format!("ascii_glyphs `{}`: both sides must be one character", k)),
        }
    }
    if errs.is_empty() { Ok(out) } else { Err(errs) }
}

/// The face with every non-ASCII glyph replaced through the pack's table; a glyph without an entry stays as it is.
pub fn ascii_face(face: &str, glyphs: &[(char, char)]) -> String {
    face.chars().map(|c| if c.is_ascii() { c } else { glyphs.iter().find(|(g, _)| *g == c).map(|(_, a)| *a).unwrap_or(c) }).collect()
}

/// Whether a pool entry from a pack that was not checked by the lint is safe to show: a name of letters, a three-cell face
/// from the allowed set, a slot id. A workspace's own theme text is read without the full lint at run time.
pub fn usable(e: &PoolEntry) -> bool {
    e.slot.len() == 7
        && e.slot.starts_with("pool-")
        && e.slot[5..].bytes().all(|b| b.is_ascii_digit())
        && !e.name.trim().is_empty()
        && e.name.chars().count() <= 64
        && e.name.chars().all(|c| c.is_alphabetic() || c == ' ')
        && e.face.chars().count() == 3
        && e.face.chars().all(|c| ALLOWED_FACE_CHARS.contains(c))
}

/// The pool of a theme pack's text, leniently: the pool a workspace theme carries, or empty if it has none or cannot be read.
pub fn pool_from_theme_text(text: &str) -> Vec<PoolEntry> {
    let Some(start) = text.find("```yaml\n").map(|i| i + 8) else { return Vec::new() };
    let Some(end) = text[start..].find("\n```").map(|i| i + start) else { return Vec::new() };
    match serde_yaml::from_str::<Value>(&text[start..end]) {
        Ok(root) => pool_from_value(&root).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// The pool of the pack built into the binary.
pub fn default_pool() -> Vec<PoolEntry> {
    pool_from_theme_text(delphi_core::theme_names::DEFAULT_THEME_TEXT)
}

/// Every rule `specs/087` §6.1 holds for a pool. `core_idle_faces` are the idle faces of the core roles, `core_faces` every
/// idle and blink face of them (for the glyph table), `other_names` the display names the pack gives its roles. An empty pool
/// is valid (a pack may have none).
pub fn validate(pool: &[PoolEntry], core_idle_faces: &[&str], core_faces: &[&str], other_names: &[String], glyphs: &[(char, char)]) -> Vec<String> {
    let mut errs = Vec::new();
    if pool.is_empty() {
        return errs;
    }
    if pool.len() != POOL_SIZE {
        errs.push(format!("pool has {} entries, not {}", pool.len(), POOL_SIZE));
    }
    let mut names: HashSet<String> = HashSet::new();
    let mut slots: HashSet<&str> = HashSet::new();
    for (i, e) in pool.iter().enumerate() {
        let want = format!("pool-{:02}", i + 1);
        if e.slot != want {
            errs.push(format!("pool[{}] slot is `{}`, expected `{}`", i + 1, e.slot, want));
        }
        if !slots.insert(e.slot.as_str()) {
            errs.push(format!("pool slot `{}` is repeated", e.slot));
        }
        let lower = e.name.trim().to_lowercase();
        if lower.is_empty() || e.name.chars().count() > 64 || !e.name.chars().all(|c| c.is_alphabetic() || c == ' ') {
            errs.push(format!("pool {} name `{}` must be 1 to 64 letters", e.slot, e.name));
        }
        if !names.insert(lower.clone()) {
            errs.push(format!("pool name `{}` is not unique", e.name));
        }
        if other_names.iter().any(|n| n.to_lowercase() == lower) {
            errs.push(format!("pool name `{}` is also a role's name", e.name));
        }
        if !(3..=5).contains(&e.keywords.len()) {
            errs.push(format!("pool {} has {} keywords, not 3 to 5", e.slot, e.keywords.len()));
        }
        for k in &e.keywords {
            if k.is_empty() || k.chars().count() > 32 || !k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '\'') {
                errs.push(format!("pool {} keyword `{}` must be 1 to 32 lowercase letters, digits, hyphens or apostrophes", e.slot, k));
            }
        }
        if e.face.chars().count() != 3 {
            errs.push(format!("pool {} face `{}` is not exactly three cells", e.slot, e.face));
        }
        if let Some(c) = e.face.chars().find(|c| !ALLOWED_FACE_CHARS.contains(*c)) {
            errs.push(format!("pool {} face `{}` uses `{}`, outside the allowed set", e.slot, e.face, c));
        }
        if core_idle_faces.contains(&e.face.as_str()) && !GENERIC_FACES.contains(&e.face.as_str()) {
            errs.push(format!("pool {} face `{}` is a core role's face (only the generic `•_•` and `•─•` may be shared)", e.slot, e.face));
        }
    }
    // Every non-ASCII glyph of any face has one ASCII stand-in in the pack.
    let mut needed: Vec<char> = pool.iter().flat_map(|e| e.face.chars()).chain(core_faces.iter().flat_map(|f| f.chars())).filter(|c| !c.is_ascii()).collect();
    needed.sort_unstable();
    needed.dedup();
    for g in needed {
        match glyphs.iter().find(|(k, _)| *k == g) {
            None => errs.push(format!("ascii_glyphs has no entry for `{}`", g)),
            Some((_, a)) if !a.is_ascii_graphic() => errs.push(format!("ascii_glyphs `{}` maps to a character that is not printable ASCII", g)),
            _ => {}
        }
    }
    errs
}

/// A stable 64-bit hash (FNV-1a) so a choice is the same across runs and builds.
fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3))
}

/// A crude stem: lowercase, then drop one common ending. Enough to make "tests" meet "test" and "debugging" meet "debug".
fn stem(w: &str) -> String {
    let w = w.to_lowercase();
    for suffix in ["ing", "ed", "es", "s"] {
        if let Some(rest) = w.strip_suffix(suffix) {
            if rest.chars().count() >= 4 {
                return rest.to_string();
            }
        }
    }
    w
}

fn stems_meet(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (short, long) = if a.chars().count() <= b.chars().count() { (a, b) } else { (b, a) };
    short.chars().count() >= 5 && long.starts_with(short)
}

const STOP: [&str; 8] = ["the", "and", "for", "with", "job", "one", "all", "its"];

fn words(text: &str) -> Vec<String> {
    let mut v: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(&w.to_lowercase().as_str()))
        .map(stem)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// How many of an entry's keywords the task words touch. A compound keyword (`finish-the-job`) counts once, when any of
/// its parts meets a word.
fn score(e: &PoolEntry, task: &[String]) -> usize {
    e.keywords.iter().filter(|k| k.split('-').filter(|p| p.chars().count() >= 3 && !STOP.contains(p)).any(|p| task.iter().any(|w| stems_meet(&stem(p), w)))).count()
}

fn roman(mut n: u32) -> String {
    let table = [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];
    let mut out = String::new();
    for (v, s) in table {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn make(e: &PoolEntry, numeral: u32) -> Pick {
    let name = if numeral > 1 { format!("{} {}", e.name, roman(numeral)) } else { e.name.clone() };
    Pick { slot: e.slot.clone(), name, face: e.face.clone(), numeral }
}

/// Choose an identity for `agent_id`, a one-off agent of `role` working on `task` (`specs/087` §4.3).
///
/// 1. An agent already live keeps the identity it has.
/// 2. Among the free entries, the one whose keywords the task touches most wins. With no match, the free entry at
///    `hash(agent_id) mod size`, scanning forward, wins.
/// 3. A face different from every live agent of the same role is preferred over a better keyword score only when scores tie.
/// 4. If every entry is live, the least-worn entry is reused with a numeral (`<name> II`).
///
/// `None` only for an empty pool.
pub fn pick(pool: &[PoolEntry], role: &str, task: &str, agent_id: &str, live: &[LiveAgent]) -> Option<Pick> {
    if pool.is_empty() {
        return None;
    }
    if let Some(l) = live.iter().find(|l| l.agent_id == agent_id) {
        if let Some(e) = pool.iter().find(|e| e.slot == l.slot) {
            return Some(make(e, l.numeral));
        }
    }
    let task_words = words(task);
    let worn = |slot: &str| live.iter().filter(|l| l.slot == slot).count();
    let same_role_faces: Vec<&str> = live.iter().filter(|l| l.role == role).filter_map(|l| pool.iter().find(|e| e.slot == l.slot)).map(|e| e.face.as_str()).collect();
    let start = (fnv1a(agent_id) % pool.len() as u64) as usize;
    let best = (0..pool.len()).min_by_key(|&i| {
        let e = &pool[i];
        // Fewest wearers first (a free entry has none), then the best keyword match, then a face not already on this role,
        // then the first entry at or after the hashed start.
        (worn(&e.slot), std::cmp::Reverse(score(e, &task_words)), same_role_faces.contains(&e.face.as_str()), (i + pool.len() - start) % pool.len())
    })?;
    let e = &pool[best];
    Some(make(e, worn(&e.slot) as u32 + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool() -> Vec<PoolEntry> {
        default_pool()
    }

    fn live(id: &str, role: &str, p: &Pick) -> LiveAgent {
        LiveAgent { agent_id: id.into(), role: role.into(), slot: p.slot.clone(), numeral: p.numeral }
    }

    #[test]
    fn the_built_in_pack_has_the_thirty_six_names_in_slot_order() {
        let p = pool();
        assert_eq!(p.len(), POOL_SIZE);
        for (i, e) in p.iter().enumerate() {
            assert_eq!(e.slot, format!("pool-{:02}", i + 1));
        }
        assert!(p.iter().all(|e| !e.name.is_empty() && (3..=5).contains(&e.keywords.len())));
        assert_eq!(p[30].face, "•_¬", "pool-31 mirrors the validator's `¬_•`");
    }

    #[test]
    fn a_task_that_touches_a_name_s_keywords_gets_that_name() {
        let p = pool();
        let pick1 = pick(&p, "investigator", "chase the flaky test and the regression it hides", "agent-1", &[]).unwrap();
        assert_eq!(pick1.slot, "pool-36", "flaky + regression are pool-36's keywords");
        let pick2 = pick(&p, "builder", "write the schema for the new file format", "agent-2", &[]).unwrap();
        assert_eq!(pick2.slot, "pool-34", "schema + format are pool-34's keywords");
    }

    #[test]
    fn with_no_match_the_choice_is_deterministic_and_depends_on_the_agent() {
        let p = pool();
        let a = pick(&p, "builder", "zzz qqq", "agent-a", &[]).unwrap();
        let a2 = pick(&p, "builder", "zzz qqq", "agent-a", &[]).unwrap();
        assert_eq!(a, a2, "the same agent always gets the same identity");
        let ids: HashSet<String> = (0..40).map(|i| pick(&p, "builder", "zzz qqq", &format!("agent-{i}"), &[]).unwrap().slot).collect();
        assert!(ids.len() > 10, "the hash spreads agents across the pool: {}", ids.len());
    }

    #[test]
    fn no_two_live_agents_share_a_name_and_a_live_agent_keeps_its_own() {
        let p = pool();
        let mut lives: Vec<LiveAgent> = Vec::new();
        for i in 0..POOL_SIZE {
            let id = format!("a{i}");
            let k = pick(&p, "builder", "flaky regression flaky", &id, &lives).unwrap();
            assert_eq!(k.numeral, 1, "a free entry exists for agent {i}");
            assert!(!lives.iter().any(|l| l.slot == k.slot), "{} is worn twice", k.slot);
            lives.push(live(&id, "builder", &k));
        }
        // Asking again for an agent that is live returns what it has.
        let again = pick(&p, "builder", "something else entirely", "a0", &lives).unwrap();
        assert_eq!(again.slot, lives[0].slot);
        assert_eq!(again.numeral, 1);
    }

    #[test]
    fn when_every_name_is_live_the_least_worn_is_reused_with_a_numeral() {
        let p = pool();
        let mut lives: Vec<LiveAgent> = Vec::new();
        for i in 0..POOL_SIZE {
            let id = format!("a{i}");
            let k = pick(&p, "builder", "x", &id, &lives).unwrap();
            lives.push(live(&id, "builder", &k));
        }
        let extra = pick(&p, "builder", "flaky regression", "extra", &lives).unwrap();
        assert_eq!(extra.numeral, 2);
        assert!(extra.name.ends_with(" II"), "{}", extra.name);
        lives.push(live("extra", "builder", &extra));
        let more = pick(&p, "builder", "flaky regression", "more", &lives).unwrap();
        assert_ne!(more.slot, extra.slot, "the least worn entry is next, not the same one again");
        assert_eq!(more.numeral, 2);
    }

    #[test]
    fn a_face_different_from_the_same_role_s_live_agents_wins_a_tie() {
        let p = pool();
        // Entries tie on score (none); the live builder wears the generic `•_•` (pool-09), so the next one must not.
        let worn = p.iter().find(|e| e.slot == "pool-09").unwrap();
        let wearing = LiveAgent { agent_id: "x".into(), role: "builder".into(), slot: worn.slot.clone(), numeral: 1 };
        for i in 0..60 {
            let k = pick(&p, "builder", "zzz", &format!("id{i}"), std::slice::from_ref(&wearing)).unwrap();
            assert_ne!(k.face, worn.face, "agent id{i} got the face a live builder already wears");
        }
    }

    #[test]
    fn roman_numerals() {
        assert_eq!((roman(2), roman(4), roman(9), roman(14), roman(40)), ("II".into(), "IV".into(), "IX".into(), "XIV".into(), "XL".into()));
    }

    fn core_idle() -> Vec<&'static str> {
        vec!["•─•", "•=•", "•.•", "•o•", "•u•", "•‿•", "•^•", "•w•", "¬_•", "¬_¬", "•_•", "•••"]
    }

    fn core_all() -> Vec<&'static str> {
        let mut v = core_idle();
        v.extend(["-─-", ">=<", "-.-", "-o-", "-u-", "-‿-", "*^*", "·w·", "-_-", "•_-", "-_•", "---"]);
        v
    }

    fn glyphs() -> Vec<(char, char)> {
        let text = delphi_core::theme_names::DEFAULT_THEME_TEXT;
        let start = text.find("```yaml\n").unwrap() + 8;
        let end = text[start..].find("\n```").unwrap() + start;
        let root: Value = serde_yaml::from_str(&text[start..end]).unwrap();
        ascii_glyphs_from_value(&root).unwrap()
    }

    #[test]
    fn the_built_in_pool_passes_every_rule() {
        let e = validate(&pool(), &core_idle(), &core_all(), &[], &glyphs());
        assert!(e.is_empty(), "{e:?}");
    }

    #[test]
    fn every_face_has_an_ascii_form_of_three_ascii_characters() {
        let g = glyphs();
        for f in pool().iter().map(|e| e.face.as_str()).chain(core_all()) {
            let a = ascii_face(f, &g);
            assert!(a.chars().count() == 3 && a.is_ascii(), "{f} -> {a}");
        }
    }

    #[test]
    fn broken_pools_are_named() {
        let g = glyphs();
        let mut p = pool();
        p[1].name = p[0].name.clone();
        p[2].keywords.truncate(2);
        p[3].face = "•••".into();
        p[4].face = "•_".into();
        p[5].face = "☃_☃".into();
        p[6].slot = "pool-99".into();
        let e = validate(&p, &core_idle(), &core_all(), &[], &g).join("\n");
        for want in ["not unique", "2 keywords", "core role's face", "not exactly three cells", "outside the allowed set", "expected `pool-07`"] {
            assert!(e.contains(want), "missing `{want}` in:\n{e}");
        }
        let short = validate(&pool()[..35], &core_idle(), &core_all(), &[], &g);
        assert!(short.iter().any(|m| m.contains("35 entries")), "{short:?}");
        assert!(validate(&[], &core_idle(), &core_all(), &[], &g).is_empty(), "no pool is fine");
        let collide = validate(&pool(), &core_idle(), &core_all(), &[pool()[0].name.to_lowercase()], &g);
        assert!(collide.iter().any(|m| m.contains("also a role's name")), "{collide:?}");
    }

    #[test]
    fn a_pack_without_a_pool_has_an_empty_one() {
        assert!(pool_from_theme_text("---\nid: t\n---\n```yaml\nlabels: {}\n```\n").is_empty());
        assert!(pick(&[], "builder", "x", "a", &[]).is_none());
    }
}
