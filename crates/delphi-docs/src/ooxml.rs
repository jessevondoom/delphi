//! Office Open XML packages (docx, xlsx, pptx): a zip of XML parts joined by relationship files
//! (`specs/014` §4). This module finds parts and follows relationships; the readers are in `docx`, `xlsx`
//! and `pptx`.

use crate::archive::Archive;
use crate::sniff::Format;
use crate::zipfile::Zip;
use crate::{DocError, Limits};
use std::collections::HashMap;

pub struct Pkg<'a> {
    pub zip: &'a Zip<'a>,
    pub limits: &'a Limits,
}

#[derive(Debug, Clone)]
pub struct Rel {
    /// Part path from the package root, or the URL when `external`.
    pub target: String,
    pub kind: String,
    pub external: bool,
}

/// Which office format a zip is, from the parts it contains.
pub fn kind_of(zip: &Zip) -> Option<Format> {
    let has = |n: &str| zip.items().iter().any(|i| i.name == n);
    if has("word/document.xml") {
        Some(Format::Docx)
    } else if has("xl/workbook.xml") {
        Some(Format::Xlsx)
    } else if has("ppt/presentation.xml") {
        Some(Format::Pptx)
    } else {
        None
    }
}

/// `base` is a part path; `target` is relative to its directory unless it starts with `/`.
pub fn resolve(base: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { vec![] } else { base.rsplit_once('/').map(|(d, _)| d.split('/').collect()).unwrap_or_default() };
    for seg in target.trim_start_matches('/').split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

impl Pkg<'_> {
    pub fn has(&self, name: &str) -> bool {
        self.zip.items().iter().any(|i| i.name == name)
    }

    /// The bytes of a part, or `None` if the package has no such part.
    pub fn part(&self, name: &str) -> Result<Option<Vec<u8>>, DocError> {
        match self.zip.items().iter().position(|i| i.name == name) {
            Some(i) => self.zip.read(i, self.limits).map(Some),
            None => Ok(None),
        }
    }

    /// Names of parts under `prefix`, in package order.
    pub fn parts_under(&self, prefix: &str) -> Vec<String> {
        self.zip.items().iter().filter(|i| i.name.starts_with(prefix)).map(|i| i.name.clone()).collect()
    }

    /// Relationships of `part` (`dir/_rels/file.rels`), by id.
    pub fn rels(&self, part: &str) -> HashMap<String, Rel> {
        let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
        let path = if dir.is_empty() { format!("_rels/{}.rels", file) } else { format!("{}/_rels/{}.rels", dir, file) };
        let mut out = HashMap::new();
        let Ok(Some(xml)) = self.part(&path) else { return out };
        let _ = crate::xml::walk(&xml, self.limits.max_nodes, |ev| {
            if let crate::xml::Ev::Start { name, attrs } = ev {
                if name == "Relationship" {
                    let (Some(id), Some(target)) = (crate::xml::attr(&attrs, "Id"), crate::xml::attr(&attrs, "Target")) else { return };
                    let external = crate::xml::attr(&attrs, "TargetMode") == Some("External");
                    let target = if external { target.to_string() } else { resolve(part, target) };
                    out.insert(id.to_string(), Rel { target, kind: crate::xml::attr(&attrs, "Type").unwrap_or("").rsplit('/').next().unwrap_or("").to_string(), external });
                }
            }
        });
        out
    }
}

#[cfg(test)]
mod tests {
    use super::resolve;

    #[test]
    fn relationship_targets_resolve_against_the_part_directory() {
        assert_eq!(resolve("xl/workbook.xml", "worksheets/sheet1.xml"), "xl/worksheets/sheet1.xml");
        assert_eq!(resolve("ppt/slides/slide1.xml", "../notesSlides/notesSlide1.xml"), "ppt/notesSlides/notesSlide1.xml");
        assert_eq!(resolve("xl/workbook.xml", "/xl/styles.xml"), "xl/styles.xml");
        assert_eq!(resolve("word/document.xml", "../../../etc/x"), "etc/x", "never above the package root");
        assert_eq!(resolve("document.xml", "a/./b"), "a/b");
    }
}
