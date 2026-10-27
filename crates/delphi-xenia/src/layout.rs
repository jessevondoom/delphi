//! Store layout and paths (spec 012 §7). The **path is authoritative** for a record's `kind` and `id`
//! (spec 011 §3.4 step 1): this module is the only place that turns a path from a hostile store into a kind
//! and an id, and it accepts exactly `records/<kind>/<id>.md` and the sidecar `records/<kind>/<id>.sig.json`.

use crate::{ulid, Error, Result};

/// The record kinds: the Phase 1 three (spec 012 §5) and `bundle` (Phase 2, spec 027 §4.3; it supersedes the
/// provisional `skill` kind). Anything else is `unknown kind directory` and is quarantined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Memory,
    Endorsement,
    Tombstone,
    Bundle,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Memory, Kind::Endorsement, Kind::Tombstone, Kind::Bundle];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Memory => "memory",
            Kind::Endorsement => "endorsement",
            Kind::Tombstone => "tombstone",
            Kind::Bundle => "bundle",
        }
    }

    /// Kinds a steward can endorse and a tombstone can hide: the ones that carry content (`memory`, `bundle`).
    pub fn is_target(self) -> bool {
        matches!(self, Kind::Memory | Kind::Bundle)
    }

    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordPath {
    pub kind: Kind,
    pub id: String,
}

impl RecordPath {
    /// `records/<kind>/<id>.md` only. The sidecar path is derived, never parsed on its own.
    pub fn parse(path: &str) -> Result<RecordPath> {
        let rest = path.strip_prefix("records/").ok_or(Error::BadPath("not under records/"))?;
        let (kind, file) = rest.split_once('/').ok_or(Error::BadPath("no kind directory"))?;
        let kind = Kind::parse(kind).ok_or(Error::BadPath("unknown kind directory"))?;
        let id = file.strip_suffix(".md").ok_or(Error::BadPath("not a .md record"))?;
        if !ulid::is_valid(id) {
            return Err(Error::BadPath("file name is not a ULID"));
        }
        Ok(RecordPath { kind, id: id.to_string() })
    }

    pub fn record(&self) -> String {
        format!("records/{}/{}.md", self.kind.as_str(), self.id)
    }

    pub fn sidecar(&self) -> String {
        format!("records/{}/{}.sig.json", self.kind.as_str(), self.id)
    }
}

pub fn roster_epoch(n: u64) -> String {
    format!("roster/epochs/{n}.json")
}

pub fn roster_epoch_sigs(n: u64) -> String {
    format!("roster/epochs/{n}.sigs.json")
}

pub const FRESHNESS: &str = "roster/freshness.json";
pub const FRESHNESS_SIGS: &str = "roster/freshness.sigs.json";

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JBX3Q8T2M9Z4K7WYH5N6D0RA";

    #[test]
    fn accepts_exactly_the_record_shape() {
        let p = RecordPath::parse(&format!("records/memory/{ID}.md")).unwrap();
        assert_eq!(p.kind, Kind::Memory);
        assert_eq!(p.id, ID);
        assert_eq!(p.record(), format!("records/memory/{ID}.md"));
        assert_eq!(p.sidecar(), format!("records/memory/{ID}.sig.json"));
        assert_eq!(RecordPath::parse(&p.record()).unwrap(), p);
    }

    #[test]
    fn refuses_everything_else() {
        for bad in [
            format!("records/memory/{ID}.sig.json"),
            format!("records/skill/{ID}.md"),
            format!("records/{ID}.md"),
            format!("record/memory/{ID}.md"),
            format!("/records/memory/{ID}.md"),
            format!("records/memory/../memory/{ID}.md"),
            format!("records/memory/sub/{ID}.md"),
            format!("records/memory/{ID}.MD"),
            format!("records/memory/{}.md", ID.to_lowercase()),
            "records/memory/.md".to_string(),
            format!("records/memory/{ID}.md\n"),
            String::new(),
        ] {
            assert!(RecordPath::parse(&bad).is_err(), "{bad:?}");
        }
    }
}
