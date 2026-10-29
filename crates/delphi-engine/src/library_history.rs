//! A source's version history (`specs/055` §3.5, slice SK4): every capture the library kept, oldest first, each with the
//! change note written when it replaced the one before, and how many claim anchors still point at it. Read only: it
//! derives everything from `meta.json` and `versions/<n>/`, so it cannot change a source, a claim or a status.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct VersionEntry {
    pub version: u64,
    /// The newest capture, the one `source.md` holds.
    pub current: bool,
    pub retrieved: Option<String>,
    pub content_hash: Option<String>,
    pub capture_level: Option<String>,
    pub bytes: Option<u64>,
    /// How this capture differed from the one before it; none for the first.
    pub change_note: Option<String>,
    /// Live anchors of claims that point at this version of the text.
    pub anchors: usize,
}

impl VersionEntry {
    pub fn to_json(&self) -> Value {
        json!({"version": self.version, "current": self.current, "retrieved": self.retrieved, "content_hash": self.content_hash, "capture_level": self.capture_level, "bytes": self.bytes, "change_note": self.change_note, "anchors": self.anchors})
    }
}

fn entry_of(meta: &Value, text: &Path, version: u64, current: bool, anchors: &[(String, usize)]) -> VersionEntry {
    let s = |k: &str| meta[k].as_str().map(String::from);
    VersionEntry {
        version,
        current,
        retrieved: s("retrieved"),
        content_hash: s("content_hash"),
        capture_level: s("capture_level"),
        bytes: std::fs::metadata(text).map(|m| m.len()).ok(),
        change_note: s("change_note"),
        anchors: anchors.iter().filter(|(v, _)| *v == version.to_string()).map(|(_, n)| n).sum(),
    }
}

impl Library {
    /// The kept captures of a source, oldest first. `None` for an unknown id. A purge deletes `versions/`, so a purged source
    /// lists only what its record still says.
    pub fn history(&self, id: &str) -> Option<Vec<VersionEntry>> {
        let meta = self.read_meta(id)?;
        let mut anchors: Vec<(String, usize)> = Vec::new();
        for c in self.citing(id) {
            for a in c["anchors"].as_array().into_iter().flatten() {
                if a["source_id"] == id && a["state"].as_str().unwrap_or("active") == "active" {
                    anchors.push((a["version"].as_str().unwrap_or("").to_string(), 1));
                }
            }
        }
        let dir = self.source_dir(id);
        let mut out: Vec<VersionEntry> = Vec::new();
        for e in std::fs::read_dir(dir.join("versions")).into_iter().flatten().flatten() {
            let Some(n) = e.file_name().to_str().and_then(|n| n.parse::<u64>().ok()) else { continue };
            let Some(m) = std::fs::read(e.path().join("meta.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
            out.push(entry_of(&m, &e.path().join("source.md"), n, false, &anchors));
        }
        let now = meta["version"].as_str().and_then(|v| v.parse::<u64>().ok()).unwrap_or(1);
        out.push(entry_of(&meta, &dir.join("source.md"), now, true, &anchors));
        out.sort_by_key(|e| e.version);
        Some(out)
    }
}
