//! The licence record of every picture the library keeps (`specs/118` §2).
//!
//! A picture in a source's `assets/` is served (to a page, the console or the app) only when the source's record lists it in
//! `images` with a licence: the picture's own address, the licence as a fixed identifier, which check decided it and where that
//! evidence is, the author, the credit address, share-alike, when it was retrieved, the file's hash, width and height. The record is
//! written by the daemon at capture: by the research runner, `record_research` and page expansion through
//! `research_images::keep_for_source` (the licence decided by code, never by a model), by a workspace document's capture (the
//! operator's own material, `internal`), and at `bundle add` from the source's licence when that licence is one a picture may be kept
//! under. A picture with no record is refused (`no licence record`); `delphi library assets check` lists such pictures and, with
//! `--apply`, moves them to `assets/unlicensed/`, which is never served. Nothing here deletes a file.
//!
//! A page that shows a picture carries, under it, the credit line the daemon writes from the record ([`Library::credit_line`]): the
//! writer never writes it, and `delphi pages check` fails a page whose figure lacks it.

use super::*;

/// What the image method answers for a library picture with no licence record (§2 item 2).
pub const NO_RECORD: &str = "no licence record";
/// The folder inside `assets/` that `assets check --apply` moves unrecorded pictures to. Nothing in it is served.
pub const UNLICENSED_DIR: &str = "unlicensed";
/// How the daemon's credit line under a page's picture starts (§2 item 4). Only the daemon writes a line that starts so.
pub const CREDIT_PREFIX: &str = "*Picture credit: ";
/// The bucket name in `lib:<bucket>/<file>` for agent-made figures (`specs/118` §5 item 2); never a source id.
pub const FIGURES_BUCKET: &str = "_figures";
/// The licence recorded on an agent-made figure (`specs/118` §5 item 2).
pub const GENERATED_LICENCE: &str = "generated";
/// How the daemon's label line under an agent-made figure starts (`specs/118` §5 item 2).
pub const FIGURE_LABEL_PREFIX: &str = "Figure made by ";
/// The check that decided the licence of a workspace document's picture: it is the operator's own material.
pub const EVIDENCE_WORKSPACE: &str = "workspace";
/// The check that decided the licence of a picture adopted with a bundle's source: that source's licence.
pub const EVIDENCE_BUNDLE: &str = "bundle";

/// The field of a PNG's record that names the SVG it was drawn from (`specs/118` §4 item 2): `{"format": "svg", "file", "blake3"}`.
/// That SVG is kept beside the PNG for the record. It has no record of its own, so nothing serves it.
pub const RENDERED_FROM: &str = "rendered_from";

/// Is this file name one of a picture (the formats a capture may keep: PNG, JPEG, GIF, WebP, and the SVG a PNG was drawn from)?
pub fn is_picture_name(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"].iter().any(|e| l.ends_with(e))
}

/// The SVG file a record says its PNG was drawn from, when its name is a plain one (`<hex>.svg`).
pub fn svg_original(rec: &Value) -> Option<&str> {
    let f = rec[RENDERED_FROM]["file"].as_str()?;
    let stem = f.strip_suffix(".svg")?;
    (!stem.is_empty() && stem.len() <= 96 && stem.chars().all(|c| c.is_ascii_hexdigit())).then_some(f)
}

/// The facts of one picture's licence record, as a capture knows them.
pub struct PictureFacts<'a> {
    pub file: &'a str,
    pub source_id: &'a str,
    /// The picture's own address (or, for a workspace picture, its path in the workspace).
    pub source_url: &'a str,
    /// A fixed identifier: `CC-BY-4.0`, `CC-BY-SA-4.0`, `CC0-1.0`, `public-domain`, or the operator's `internal`.
    pub licence: &'a str,
    /// Which check decided the licence, and where its evidence is.
    pub evidence: &'a str,
    pub evidence_url: &'a str,
    pub author: &'a str,
    pub credit_url: &'a str,
    pub share_alike: bool,
    pub retrieved: &'a str,
    pub blake3: &'a str,
    pub width: usize,
    pub height: usize,
}

/// One line of external text for a record or the credit line: no control character, no Markdown that could make a link, an emphasis
/// or a picture of it, at most `n` characters.
pub(super) fn one_line(s: &str, n: usize) -> String {
    s.chars().map(|c| if c.is_control() || matches!(c, '*' | '_' | '[' | ']' | '`' | '<' | '>' | '!' | '|') { ' ' } else { c }).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ").chars().take(n).collect()
}

/// An address for a record or the credit line, kept exactly as it is when it is one: `https://`, `http://`, `workspace:` or
/// `bundle:`, at most `n` characters, with no control character, no white space and none of the characters that could make a
/// link, an emphasis or a picture of it (`< > * [ ] ` | ! "`). An `_` stays, so Wikipedia and Commons addresses are not broken.
/// Anything else is made [`one_line`].
fn one_url(s: &str, n: usize) -> String {
    let t = s.trim();
    let scheme = ["https://", "http://", "workspace:", "bundle:"].iter().any(|p| t.to_ascii_lowercase().starts_with(p));
    let plain = t.chars().all(|c| !c.is_control() && !c.is_whitespace() && !matches!(c, '<' | '>' | '*' | '[' | ']' | '`' | '|' | '!' | '"'));
    if scheme && plain && t.chars().count() <= n { t.to_string() } else { one_line(s, n) }
}

/// A fixed word chosen by code (the check that decided a licence): letters, digits, `_` and `-` only.
fn one_word(s: &str, n: usize) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')).take(n).collect()
}

/// The record of §2 item 1, as it is kept in the source's `images`.
pub fn picture_record(f: &PictureFacts<'_>) -> Value {
    let mut r = json!({
        "file": f.file,
        "source_id": f.source_id,
        "source_url": one_url(f.source_url, 500),
        "licence": one_line(f.licence, 60),
        "licence_evidence": {"check": one_word(f.evidence, 40), "url": one_url(f.evidence_url, 500)},
        "author": one_line(f.author, 200),
        "credit_url": one_url(f.credit_url, 500),
        "share_alike": f.share_alike,
        "retrieved": f.retrieved,
        "blake3": f.blake3,
        "width": f.width,
        "height": f.height,
    });
    r["attribution"] = json!(credit_text(&r));
    r
}

/// The licence's name as a credit line shows it.
fn licence_name(licence: &str) -> String {
    if licence == "public-domain" { "public domain".to_string() } else { licence.to_string() }
}

/// The words of the credit line, from the record alone: author, licence name, and the credit address (the picture's own address when
/// an older record has no credit address).
fn credit_text(rec: &Value) -> String {
    let author = one_line(rec["author"].as_str().unwrap_or(""), 200);
    let credit = rec["credit_url"].as_str().filter(|c| !c.trim().is_empty()).or(rec["source_url"].as_str()).unwrap_or("");
    format!("{}, {}, {}", if author.is_empty() { "author not stated" } else { author.as_str() }, licence_name(&one_line(rec["licence"].as_str().unwrap_or(""), 60)), one_url(credit, 500))
}

/// Is `rec` a licence record of `file` in source `id`: it names the file, it names no other source, and it states a licence? An SVG
/// never has one, whatever a record says: it is never shown as SVG (`specs/118` §4 item 2), only drawn to a PNG at capture.
fn is_record_of(rec: &Value, id: &str, file: &str) -> bool {
    let licence = rec["licence"].as_str().map(str::trim).unwrap_or("");
    !file.to_ascii_lowercase().ends_with(".svg") && rec["file"].as_str() == Some(file) && rec["source_id"].as_str().is_none_or(|s| s == id) && !licence.is_empty() && !matches!(licence.to_ascii_lowercase().as_str(), "unknown" | "none")
}

/// One file of a source's `assets/` that has no licence record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrecorded {
    pub source_id: String,
    pub file: String,
    /// It was moved to `assets/unlicensed/` (with `--apply`).
    pub moved: bool,
}

impl Library {
    /// The licence record of picture `file` of source `id` (§2 item 1), or of agent-made figure `file` when `id` is
    /// [`FIGURES_BUCKET`] (`specs/118` §5 item 2).
    pub fn asset_record(&self, id: &str, file: &str) -> Option<Value> {
        if id == FIGURES_BUCKET {
            return self.figure_asset_record(file);
        }
        let meta = self.read_meta(id)?;
        meta["images"].as_array()?.iter().find(|r| is_record_of(r, id, file)).cloned()
    }

    /// The daemon's credit line under a page's picture `lib:<id>/<file>` (§2 item 4), from its licence record; `None` without one.
    pub fn credit_line(&self, id: &str, file: &str) -> Option<String> {
        self.asset_record(id, file).map(|r| format!("{}{}*", CREDIT_PREFIX, credit_text(&r)))
    }

    /// The credit of a record, for the image method's answer: the line's words and the fields they come from.
    pub fn credit_of(rec: &Value) -> Value {
        json!({"line": credit_text(rec), "author": rec["author"], "licence": rec["licence"], "credit_url": rec["credit_url"], "share_alike": rec["share_alike"] == true})
    }

    /// Put `records` into source `id`'s `images`, replacing any record of the same file and keeping the others; a record whose file
    /// is not in the source's `assets/` is left out. Returns how many records the source now lists.
    pub fn set_asset_records(&self, id: &str, records: &[Value]) -> Result<usize, String> {
        let mut meta = self.read_meta(id).ok_or("no such source")?;
        let assets = self.source_dir(id).join("assets");
        let mut list: Vec<Value> = meta["images"].as_array().cloned().unwrap_or_default();
        for r in records {
            let Some(file) = r["file"].as_str() else { continue };
            if !assets.join(file).is_file() || file.contains(['/', '\\']) {
                continue;
            }
            list.retain(|x| x["file"].as_str() != Some(file));
            list.push(r.clone());
        }
        let n = list.len();
        meta["images"] = Value::Array(list);
        self.write_json(&self.source_dir(id).join("meta.json"), &meta)?;
        Ok(n)
    }

    /// The pictures of every source's `assets/` and of `figures/` that have no licence record (§2 item 3). With `apply` each
    /// is moved to `unlicensed/`, which is never served; nothing is deleted. A file already there is not listed again.
    pub fn assets_check(&self, apply: bool) -> Vec<Unrecorded> {
        let mut out = Vec::new();
        let mut scan = |source_id: &str, dir: PathBuf| {
            let Ok(rd) = std::fs::read_dir(&dir) else { return };
            let mut files: Vec<String> = rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())).filter_map(|e| e.file_name().to_str().map(String::from)).filter(|n| is_picture_name(n)).collect();
            files.sort();
            // The SVGs pictures were drawn from belong to their PNG's record; they are not served and are not unrecorded.
            let originals: Vec<String> = self.read_meta(source_id).and_then(|m| m["images"].as_array().map(|a| a.iter().filter(|r| r["file"].as_str().is_some_and(|f| is_record_of(r, source_id, f))).filter_map(|r| svg_original(r).map(String::from)).collect())).unwrap_or_default();
            for file in files {
                if self.asset_record(source_id, &file).is_some() || originals.contains(&file) {
                    continue;
                }
                let moved = apply && {
                    let to = dir.join(UNLICENSED_DIR);
                    std::fs::create_dir_all(&to).is_ok() && !to.join(&file).exists() && std::fs::rename(dir.join(&file), to.join(&file)).is_ok()
                };
                out.push(Unrecorded { source_id: source_id.to_string(), file, moved });
            }
        };
        for (id, _) in self.list() {
            scan(&id, self.source_dir(&id).join("assets"));
        }
        scan(FIGURES_BUCKET, self.figures_dir());
        if apply && out.iter().any(|u| u.moved) {
            self.log("assets_unlicensed", json!({"moved": out.iter().filter(|u| u.moved).map(|u| format!("{}/{}", u.source_id, u.file)).collect::<Vec<_>>()}));
        }
        out
    }

    /// The records of the pictures an added bundle's source brought (§2 item 1): the bundle's own record of a picture is kept only when
    /// its licence passes the same decision a captured picture's does, and a picture with none takes the source's licence when that
    /// licence is one a picture may be kept under (or is the organisation's own, `own` or `internal`). Anything else stays unrecorded,
    /// so it is not served and `assets check` lists it. What the bundle says decided the licence is not read: the check is `bundle`.
    pub(crate) fn record_bundle_pictures(&self, id: &str, label: &str, given: &[Value]) {
        let Some(meta) = self.read_meta(id) else { return };
        let Ok(rd) = std::fs::read_dir(self.source_dir(id).join("assets")) else { return };
        let allowed = |l: &str| -> Option<(String, bool)> {
            match l.trim() {
                "own" | "internal" => Some((l.trim().to_string(), false)),
                other => match delphi_adapter::research::images::judge_name(&other.replace('-', " ")) {
                    delphi_adapter::research::images::Verdict::Allowed(x) => Some((x.spdx, x.share_alike)),
                    delphi_adapter::research::images::Verdict::Refused(_) => None,
                },
            }
        };
        let source_licence = meta["licence"].as_str().unwrap_or("unknown").to_string();
        let retrieved = delphi_core::time::now_rfc3339();
        let mut records = Vec::new();
        for e in rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_file())) {
            // An SVG is never shown as SVG (`specs/118` §4 item 2), so a bundle's SVG gets no record and is never served.
            let Some(file) = e.file_name().to_str().map(String::from).filter(|n| is_picture_name(n) && !n.to_ascii_lowercase().ends_with(".svg")) else { continue };
            let bytes = std::fs::read(e.path()).unwrap_or_default();
            let (w, h) = delphi_docs::pixels::decode(&bytes).map(|p| (p.width, p.height)).unwrap_or((0, 0));
            let own = given.iter().find(|r| r["file"].as_str() == Some(file.as_str()));
            let (licence, author, credit) = match own.and_then(|r| allowed(r["licence"].as_str().unwrap_or("")).map(|l| (l, r))) {
                Some((l, r)) => (l, r["author"].as_str().unwrap_or("").to_string(), r["credit_url"].as_str().unwrap_or("").to_string()),
                None => match allowed(&source_licence) {
                    Some(l) => (l, meta["rights_holder"].as_str().unwrap_or("").to_string(), String::new()),
                    None => continue,
                },
            };
            let credit = if credit.is_empty() { format!("bundle {}", label) } else { credit };
            records.push(picture_record(&PictureFacts {
                file: &file,
                source_id: id,
                source_url: &format!("bundle:{}", label),
                licence: &licence.0,
                evidence: EVIDENCE_BUNDLE,
                evidence_url: &format!("bundle:{}", label),
                author: &author,
                credit_url: &credit,
                share_alike: licence.1,
                retrieved: &retrieved,
                blake3: &blake3::hash(&bytes).to_hex(),
                width: w,
                height: h,
            }));
        }
        if !records.is_empty() {
            let _ = self.set_asset_records(id, &records);
        }
    }

    /// The page body with the daemon's credit or figure-label line put under each library picture that has a record (§2 item 4,
    /// `specs/118` §5 item 2). A line that shows a `lib:` picture is followed by the line; a picture with no record gets none.
    pub fn with_credits(&self, body: &str) -> String {
        let mut out = String::with_capacity(body.len() + 256);
        for line in body.lines() {
            out.push_str(line);
            out.push('\n');
            for (sid, file) in lib_pictures(line) {
                let label = if sid == FIGURES_BUCKET { self.figure_label(&file) } else { self.credit_line(&sid, &file) };
                if let Some(c) = label {
                    out.push_str(&c);
                    out.push('\n');
                }
            }
        }
        if !body.ends_with('\n') {
            out.pop();
        }
        out
    }
}

/// The library pictures (`![..](lib:<source>/<file>)`) a line shows, in order.
pub fn lib_pictures(line: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = line[i..].find("](lib:") {
        let at = i + p;
        let start = at + "](lib:".len();
        let Some(close) = line[start..].find(')') else { break };
        let image = line[..at].rfind('[').is_some_and(|o| line[..o].ends_with('!'));
        if image {
            if let Some((sid, file)) = line[start..start + close].trim().split_once('/') {
                out.push((sid.to_string(), file.to_string()));
            }
        }
        i = start + close;
    }
    out
}
