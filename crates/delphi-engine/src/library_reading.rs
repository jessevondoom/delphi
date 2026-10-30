//! Reading lists (`specs/055` §8, slice SK6): named, ordered lists of library sources, so an acquired shelf can be browsed in the
//! order someone chose. A list holds source ids and an optional note, nothing else: no text is copied, so a list can never carry
//! more than the library lets out. The daemon adds acquired sources to an `acquired-<topic>` list; the operator makes and edits others.

use super::*;

fn list_id_ok(id: &str) -> bool {
    valid_id(id)
}

impl Library {
    fn reading_dir(&self) -> PathBuf {
        self.root.join("reading-lists")
    }

    pub fn reading_lists(&self) -> Vec<Value> {
        let Ok(rd) = std::fs::read_dir(self.reading_dir()) else { return Vec::new() };
        let mut v: Vec<Value> = rd.flatten().filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()).collect();
        v.sort_by(|a: &Value, b: &Value| a["id"].as_str().cmp(&b["id"].as_str()));
        v
    }

    pub fn reading_list(&self, id: &str) -> Option<Value> {
        if !list_id_ok(id) {
            return None;
        }
        serde_json::from_slice(&std::fs::read(self.reading_dir().join(format!("{}.json", id))).ok()?).ok()
    }

    /// Add sources to a list, making it if it is new. Every source must exist; one already on the list is not added twice.
    pub fn reading_add(&self, id: &str, title: Option<&str>, sources: &[String]) -> Result<Value, String> {
        if !list_id_ok(id) {
            return Err(format!("`{}` is not a valid list id (lowercase letters, digits, hyphens)", id.chars().take(40).collect::<String>()));
        }
        if let Some(s) = sources.iter().find(|s| self.read_meta(s).is_none()) {
            return Err(format!("no source `{}`", s.chars().take(60).collect::<String>()));
        }
        let mut l = self.reading_list(id).unwrap_or_else(|| json!({"id": id, "title": title.unwrap_or(id), "created": delphi_core::time::now_rfc3339(), "items": []}));
        for s in sources {
            if !l["items"].as_array().is_some_and(|a| a.iter().any(|i| i["source"] == *s)) {
                l["items"].as_array_mut().unwrap().push(json!({"source": s, "added": delphi_core::time::now_rfc3339()}));
            }
        }
        self.write_reading(id, &l)?;
        Ok(l)
    }

    pub fn reading_remove(&self, id: &str, source: &str) -> Result<(), String> {
        let mut l = self.reading_list(id).ok_or_else(|| format!("no reading list `{}`", id.chars().take(40).collect::<String>()))?;
        let before = l["items"].as_array().map_or(0, Vec::len);
        l["items"].as_array_mut().unwrap().retain(|i| i["source"] != *source);
        if l["items"].as_array().map_or(0, Vec::len) == before {
            return Err(format!("`{}` is not on the list", source.chars().take(60).collect::<String>()));
        }
        self.write_reading(id, &l)
    }

    fn write_reading(&self, id: &str, l: &Value) -> Result<(), String> {
        self.ensure().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(self.reading_dir()).map_err(|e| e.to_string())?;
        self.write_json(&self.reading_dir().join(format!("{}.json", id)), l)
    }
}
