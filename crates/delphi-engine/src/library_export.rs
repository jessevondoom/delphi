//! Library sources into a bundle's `sources/` (`specs/023` §2.1, `specs/027` §2.3a, `specs/055` §7; slice SK7).
//!
//! The author's act, run by the operator's command. The copy carries the licence record as it stands (`internal` when
//! the record has none) and is never refused for it: licence is information, and the warning and the confirmation happen
//! later, at `validate`, `package` and `publish`. The one refusal is a source flagged `private` (§10.1), decided here in
//! code from the library record. What is written is `meta.json` rebuilt from a list of citation and licence fields (so no
//! status, origin, reliance count or operator note travels), `source.md` when the capture is whole, the excerpts without
//! their purpose notes, and the assets of a recognised type.

use super::*;

const CITATION: [&str; 21] = [
    "type", "title", "author", "editor", "translator", "container-title", "publisher", "publisher-place", "issued", "volume", "issue", "page", "edition", "URL", "DOI", "ISBN", "ISSN", "language", "licence", "rights_holder", "attribution_text",
];
const RECORD: [&str; 5] = ["share_alike", "retrieved", "canonical_url", "content_hash", "version"];
const ASSET_EXTS: [&str; 7] = ["png", "jpg", "jpeg", "webp", "svg", "pdf", "csv"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub level: Level,
    pub licence: String,
    pub text_bytes: u64,
    pub assets: usize,
}

fn https_only(m: &mut Map<String, Value>, k: &str) {
    if m.get(k).and_then(Value::as_str).is_some_and(|u| !u.to_ascii_lowercase().starts_with("https://") || u.split('/').nth(2).is_some_and(|h| h.contains('@'))) {
        m.remove(k);
    }
}

impl Library {
    /// Copy `ids` into `<dir>/sources/`. Each result is its own: a refused source does not stop the others. An id already
    /// under `sources/` is left alone unless `replace`.
    pub fn export_to_bundle(&self, dir: &Path, ids: &[String], replace: bool) -> Vec<(String, Result<Exported, String>)> {
        ids.iter().map(|id| (id.clone(), self.export_one(dir, id, replace))).collect()
    }

    fn export_one(&self, dir: &Path, id: &str, replace: bool) -> Result<Exported, String> {
        let meta = self.read_meta(id).ok_or_else(|| format!("no source `{}` in the library", id))?;
        if is_private(&meta) {
            return Err("flagged private: a private source never travels in a bundle (unflag it in the library if that was a mistake)".into());
        }
        let src = self.source_dir(id);
        let held = meta["purged"].is_null();
        let text = if held { std::fs::read_to_string(src.join("source.md")).ok() } else { None };
        if text.as_ref().is_some_and(|t| t.len() as u64 > delphi_bundle::sources::MAX_TEXT_BYTES) {
            return Err("the capture is over the 16 MiB cap for a bundle's source text".into());
        }
        let dest = dir.join("sources").join(id);
        if dest.exists() && !replace {
            return Err("already under sources/ (use --replace to overwrite it)".into());
        }
        let mut out = Map::new();
        for k in CITATION.iter().chain(RECORD.iter()) {
            if let Some(v) = meta.get(*k).filter(|v| !v.is_null()) {
                out.insert((*k).into(), v.clone());
            }
        }
        for k in ["canonical_url", "URL"] {
            https_only(&mut out, k);
        }
        let licence = out.get("licence").and_then(Value::as_str).map(str::trim).filter(|l| !l.is_empty()).unwrap_or("internal").to_string();
        out.insert("licence".into(), json!(licence));
        let excerpts: Vec<Value> = if held {
            self.excerpts(id).into_iter().map(|mut e| {
                if let Some(o) = e.as_object_mut() {
                    o.retain(|k, _| ["id", "locator", "text", "bytes", "anchor"].contains(&k.as_str()));
                }
                e
            }).collect()
        } else {
            Vec::new()
        };
        let level = if text.is_some() { Level::Full } else if !excerpts.is_empty() { Level::Excerpts } else { Level::Pointer };
        out.insert("capture_level".into(), json!(level.as_str()));
        let assets: Vec<PathBuf> = if held && text.is_some() { Self::asset_files(&src.join("assets")) } else { Vec::new() };
        if replace && dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
        }
        std::fs::create_dir_all(&dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
        let write = |name: &str, bytes: Vec<u8>| std::fs::write(dest.join(name), bytes).map_err(|e| format!("{}: {}", name, e));
        write("meta.json", serde_json::to_vec_pretty(&Value::Object(out)).unwrap_or_default())?;
        let text_bytes = text.as_ref().map_or(0, |t| t.len() as u64);
        if let Some(t) = text {
            write("source.md", t.into_bytes())?;
        }
        if !excerpts.is_empty() {
            write("excerpts.json", serde_json::to_vec_pretty(&excerpts).unwrap_or_default())?;
        }
        let mut copied = 0;
        for a in &assets {
            if std::fs::metadata(a).is_ok_and(|m| m.len() <= delphi_bundle::sources::MAX_ASSET_BYTES) {
                std::fs::create_dir_all(dest.join("assets")).map_err(|e| e.to_string())?;
                std::fs::copy(a, dest.join("assets").join(a.file_name().unwrap_or_default())).map_err(|e| format!("{}: {}", a.display(), e))?;
                copied += 1;
            }
        }
        self.log("source_exported", json!({"id": id, "level": level.as_str()}));
        Ok(Exported { level, licence, text_bytes, assets: copied })
    }

    fn asset_files(dir: &Path) -> Vec<PathBuf> {
        let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut v: Vec<PathBuf> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| ASSET_EXTS.contains(&x.to_ascii_lowercase().as_str())))
            .collect();
        v.sort();
        v
    }
}
