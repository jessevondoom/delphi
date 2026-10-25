//! What zip and tar share: a list of members that can each be read, and the listing and member-lookup
//! rules on top (`specs/014` §4, §6).
//!
//! Member names are display-only. A member is found by an exact match against the listing or by its
//! `#N` position, and a name is never joined to a filesystem path, so a hostile name (`../x`, `/etc/x`,
//! `C:\x`) can be listed and read but cannot point anywhere.

use crate::{DocError, Limits};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    File,
    Dir,
    /// Target as stored. Never followed.
    Symlink(String),
    /// Hard links, devices, FIFOs: listed, not readable.
    Other(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Safe to print: control characters are replaced and the length is bounded.
    pub name: String,
    pub size: u64,
    pub compressed: Option<u64>,
    pub kind: Kind,
    /// The stored name could escape a directory if someone joined it to a path (`..`, absolute, drive,
    /// backslash, control character).
    pub unsafe_name: bool,
    pub encrypted: bool,
}

pub trait Archive {
    fn kind(&self) -> &'static str;
    fn items(&self) -> &[Item];
    /// Members the archive claims beyond the ones listed (listing capped at `max_members`).
    fn omitted(&self) -> usize;
    /// The directory was cut or damaged; what is listed is what could be recovered.
    fn damaged(&self) -> bool;
    /// Bytes of member `i`, bounded by `limits` and checked for corruption.
    fn read(&self, i: usize, limits: &Limits) -> Result<Vec<u8>, DocError>;
}

/// The cap on one member's expanded size: absolute, and `MAX_EXPANSION_RATIO` times what it compressed from
/// (with a floor, so a small file of zeros is not mistaken for a bomb).
pub fn member_cap(compressed: u64, limits: &Limits) -> usize {
    let ratio = (compressed as usize).saturating_mul(crate::limits::MAX_EXPANSION_RATIO).max(1 << 20);
    ratio.min(limits.max_expanded).min(limits.max_input)
}

/// Make a stored name printable and say whether it is unsafe to join to a path.
pub fn clean_name(raw: &[u8]) -> (String, bool) {
    let s = String::from_utf8_lossy(raw);
    let mut unsafe_name = s.contains('\\') || s.starts_with('/') || s.chars().any(|c| c.is_control());
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        unsafe_name = true;
    }
    if s.split(['/', '\\']).any(|part| part == "..") {
        unsafe_name = true;
    }
    let shown: String = s.chars().take(240).map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect();
    (shown, unsafe_name)
}

pub fn human(n: u64) -> String {
    const U: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{} B", n) } else { format!("{:.1} {}", v, U[i]) }
}

/// Find a member: an exact name, or `#N`. The first exact match wins; `#N` reaches duplicates.
pub fn resolve(items: &[Item], wanted: &str) -> Result<usize, DocError> {
    if let Some(n) = wanted.strip_prefix('#') {
        return match n.parse::<usize>() {
            Ok(i) if i < items.len() => Ok(i),
            _ => Err(DocError(format!("no member {} (the archive lists {} members, #0 to #{})", wanted, items.len(), items.len().saturating_sub(1)))),
        };
    }
    items.iter().position(|it| it.name == wanted).ok_or_else(|| DocError(format!("no member named `{}`; list the archive to see the names, or use #N", wanted)))
}

/// Rows per listing page.
pub const PAGE: usize = 500;

/// The listing as text, `start..end` being member positions. Returns (text, shown_end, losses).
pub fn listing(ar: &dyn Archive, start: usize, end: Option<usize>, limits: &Limits) -> (String, bool, Vec<String>) {
    let items = ar.items();
    let total_size: u64 = items.iter().map(|i| i.size).sum();
    let total_comp: u64 = items.iter().filter_map(|i| i.compressed).sum();
    let mut out = format!("{} · {} member{} · {} uncompressed", ar.kind(), items.len() + ar.omitted(), if items.len() + ar.omitted() == 1 { "" } else { "s" }, human(total_size));
    if total_comp > 0 {
        out.push_str(&format!(" · {} compressed", human(total_comp)));
    }
    out.push('\n');
    let end = end.map(|e| e.saturating_add(1)).unwrap_or(start.saturating_add(PAGE)).min(start.saturating_add(PAGE)).min(items.len());
    let mut shown = start.min(items.len());
    let mut cut = false;
    for (i, it) in items.iter().enumerate().take(end).skip(start) {
        let mut line = format!("#{:<4} {}", i, it.name);
        match &it.kind {
            Kind::File => line.push_str(&format!("  {}", human(it.size))),
            Kind::Dir => line.push_str("  [directory]"),
            Kind::Symlink(t) => line.push_str(&format!(" -> {}  [symlink, not followed]", t.chars().take(120).map(|c| if c.is_control() { '\u{fffd}' } else { c }).collect::<String>())),
            Kind::Other(what) => line.push_str(&format!("  [{}, not readable]", what)),
        }
        if let (Kind::File, Some(c)) = (&it.kind, it.compressed) {
            if c != it.size {
                line.push_str(&format!(" ({} compressed)", human(c)));
            }
        }
        if it.unsafe_name {
            line.push_str("  [unsafe name]");
        }
        if it.encrypted {
            line.push_str("  [encrypted]");
        }
        if out.len() + line.len() + 1 > limits.max_output {
            cut = true;
            break;
        }
        out.push_str(&line);
        out.push('\n');
        shown = i + 1;
    }
    if shown < items.len() {
        out.push_str(&format!("(members #{}-#{} of {} shown; pass start/end to page)\n", start, shown.saturating_sub(1), items.len()));
        cut = true;
    }
    let mut loss = vec![];
    let n = |f: &dyn Fn(&Item) -> bool| items.iter().filter(|i| f(i)).count();
    if n(&|i| i.unsafe_name) > 0 {
        loss.push(format!("{} member name(s) are unsafe as paths; they are listed and readable here but nothing is extracted to disk", n(&|i| i.unsafe_name)));
    }
    if n(&|i| i.encrypted) > 0 {
        loss.push(format!("{} encrypted member(s) cannot be read", n(&|i| i.encrypted)));
    }
    if n(&|i| matches!(i.kind, Kind::Symlink(_))) > 0 {
        loss.push("symbolic links are listed, never followed".to_string());
    }
    if ar.omitted() > 0 {
        loss.push(format!("{} more member(s) beyond the {} the reader lists", ar.omitted(), limits.max_members));
    }
    if ar.damaged() {
        loss.push("the archive directory is damaged or cut off; only the recoverable members are listed".to_string());
    }
    (out, cut, loss)
}
