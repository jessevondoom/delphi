//! Truth packs: the stored source of truth for a research or strategy case (`specs/037` §4).
//!
//! A pack lives in `truth/` beside `case.toml` and is **never copied into a run**, so no agent can read it. It holds
//! `meta.toml` (as-of date, who verified it), `facts.toml`, `traps.toml`, `sources.toml` with extracts under
//! `sources/`, and `rubric.md`. Loading checks the pack against itself: every fact names a source, every quote is
//! found in that source's extract, every extract matches its recorded hash, and every pattern compiles. Matching is
//! deterministic and gives a **lower bound** on what an answer states (a fact worded unexpectedly is missed).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::{Regex, RegexBuilder};

pub type Problems = Vec<String>;

#[derive(Debug, Clone)]
pub struct Fact {
    pub id: String,
    pub claim: String,
    pub source: String,
    pub weight: f64,
    patterns: Vec<Regex>,
    /// A number the answer must contain within `tol` of `value`, in addition to any pattern.
    number: Option<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct Trap {
    pub id: String,
    pub why: String,
    patterns: Vec<Regex>,
}

#[derive(Debug, Clone)]
pub struct Truth {
    pub dir: PathBuf,
    pub as_of: String,
    pub verified_by: Option<String>,
    pub verified_on: Option<String>,
    pub facts: Vec<Fact>,
    pub traps: Vec<Trap>,
    pub sources: usize,
}

impl PartialEq for Truth {
    fn eq(&self, o: &Truth) -> bool {
        self.dir == o.dir && self.as_of == o.as_of
    }
}

/// How much of the checklist an answer states.
#[derive(Debug, Clone, PartialEq)]
pub struct Recall {
    /// Weighted share of facts stated, 0 to 1.
    pub fraction: f64,
    pub stated: Vec<String>,
    pub missing: Vec<String>,
}

fn table(path: &Path, at: &str, problems: &mut Problems) -> Option<toml::Table> {
    match fs::read_to_string(path) {
        Ok(t) => match t.parse::<toml::Table>() {
            Ok(t) => Some(t),
            Err(e) => {
                problems.push(format!("{}: {} is not valid TOML ({})", at, path.display(), e.message()));
                None
            }
        },
        Err(e) => {
            problems.push(format!("{}: cannot read {} ({})", at, path.display(), e));
            None
        }
    }
}

fn s<'a>(t: &'a toml::Table, k: &str) -> Option<&'a str> {
    t.get(k).and_then(|v| v.as_str()).map(str::trim).filter(|v| !v.is_empty())
}

fn date_ok(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

fn patterns(t: &toml::Table, at: &str, problems: &mut Problems) -> Vec<Regex> {
    let mut out = Vec::new();
    for p in t.get("any").and_then(|v| v.as_array()).into_iter().flatten() {
        match p.as_str().map(|p| RegexBuilder::new(p).case_insensitive(true).size_limit(1 << 20).build()) {
            Some(Ok(r)) => out.push(r),
            Some(Err(e)) => problems.push(format!("{}: pattern does not compile ({})", at, e.to_string().lines().last().unwrap_or(""))),
            None => problems.push(format!("{}: `any` holds only strings", at)),
        }
    }
    out
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Read and check `<case>/truth`. `at` names the case in problem lines.
pub fn load(case_dir: &Path, at: &str) -> Result<Truth, Problems> {
    let dir = case_dir.join("truth");
    let mut p: Problems = Vec::new();
    if !dir.is_dir() {
        return Err(vec![format!("{}: a research case needs a `truth/` directory (specs/037 §4)", at)]);
    }
    let meta = table(&dir.join("meta.toml"), at, &mut p);
    let (as_of, verified_by, verified_on) = match &meta {
        Some(m) => {
            let as_of = s(m, "as_of").unwrap_or("").to_string();
            if !date_ok(&as_of) {
                p.push(format!("{}: truth/meta.toml `as_of` must be a date like 2026-10-26", at));
            }
            let (by, on) = (s(m, "verified_by").map(String::from), s(m, "verified_on").map(String::from));
            if by.is_some() != on.is_some() || on.as_deref().is_some_and(|d| !date_ok(d)) {
                p.push(format!("{}: truth/meta.toml `verified_by` and `verified_on` (a date) go together", at));
            }
            (as_of, by, on)
        }
        None => (String::new(), None, None),
    };
    if !dir.join("rubric.md").is_file() {
        p.push(format!("{}: truth/rubric.md is missing", at));
    }

    // Sources: each has an id, a URL, a retrieval date, and an extract that matches its recorded hash.
    let mut source_text: std::collections::BTreeMap<String, Option<String>> = Default::default();
    if let Some(t) = table(&dir.join("sources.toml"), at, &mut p) {
        for (i, st) in t.get("source").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
            let here = format!("{} source #{}", at, i + 1);
            let Some(st) = st.as_table() else {
                p.push(format!("{}: not a table", here));
                continue;
            };
            let Some(id) = s(st, "id").filter(|id| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')) else {
                p.push(format!("{}: `id` must be letters, digits, `-` or `_`", here));
                continue;
            };
            if s(st, "url").is_none() || !s(st, "retrieved").is_some_and(date_ok) {
                p.push(format!("{}: `{}` needs `url` and a `retrieved` date", here, id));
            }
            if source_text.contains_key(id) {
                p.push(format!("{}: duplicate source id `{}`", here, id));
            }
            let untracked = st.get("untracked").and_then(|v| v.as_bool()).unwrap_or(false);
            let extract = dir.join("sources").join(format!("{}.txt", id));
            if untracked {
                source_text.insert(id.to_string(), None);
            } else {
                match fs::read(&extract) {
                    Ok(bytes) => {
                        match s(st, "blake3") {
                            Some(h) if h == blake3::hash(&bytes).to_hex().as_str() => {}
                            Some(_) => p.push(format!("{}: `{}` extract does not match its recorded `blake3`", here, id)),
                            None => p.push(format!("{}: `{}` needs `blake3` of its extract", here, id)),
                        }
                        source_text.insert(id.to_string(), Some(norm(&String::from_utf8_lossy(&bytes))));
                    }
                    Err(_) => {
                        p.push(format!("{}: `{}` has no extract at truth/sources/{}.txt (mark it `untracked = true` if the text is kept elsewhere)", here, id, id));
                        source_text.insert(id.to_string(), None);
                    }
                }
            }
        }
    }

    let mut facts = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(t) = table(&dir.join("facts.toml"), at, &mut p) {
        for (i, f) in t.get("fact").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
            let here = format!("{} fact #{}", at, i + 1);
            let Some(f) = f.as_table() else {
                p.push(format!("{}: not a table", here));
                continue;
            };
            let id = s(f, "id").unwrap_or("").to_string();
            if id.is_empty() || !seen.insert(id.clone()) {
                p.push(format!("{}: `id` is missing or repeated", here));
            }
            let (claim, source, quote) = (s(f, "claim"), s(f, "source"), s(f, "quote"));
            if claim.is_none() || quote.is_none() {
                p.push(format!("{}: `{}` needs `claim` and `quote`", here, id));
            }
            match source.map(|src| source_text.get(src)) {
                Some(Some(text)) => {
                    // A tracked extract must contain the quote: the pack is checked against its own record.
                    if let (Some(text), Some(q)) = (text, quote) {
                        if !text.contains(&norm(q)) {
                            p.push(format!("{}: `{}` quote is not in the extract of source `{}`", here, id, source.unwrap_or("")));
                        }
                    }
                }
                _ => p.push(format!("{}: `{}` names a source that is not in sources.toml", here, id)),
            }
            let pats = patterns(f, &format!("{} `{}`", here, id), &mut p);
            let number = f.get("number").and_then(|n| n.as_table()).and_then(|n| {
                let v = n.get("value").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))?;
                let tol = n.get("tol").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(0.0);
                Some((v, tol.abs()))
            });
            if pats.is_empty() && number.is_none() {
                p.push(format!("{}: `{}` needs `any` patterns or a `number`", here, id));
            }
            let weight = f.get("weight").and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64))).unwrap_or(1.0);
            if weight <= 0.0 {
                p.push(format!("{}: `{}` weight must be above 0", here, id));
            }
            facts.push(Fact { id, claim: claim.unwrap_or("").to_string(), source: source.unwrap_or("").to_string(), weight, patterns: pats, number });
        }
    }

    let mut traps = Vec::new();
    let trap_path = dir.join("traps.toml");
    if trap_path.is_file() {
        if let Some(t) = table(&trap_path, at, &mut p) {
            for (i, tr) in t.get("trap").and_then(|v| v.as_array()).into_iter().flatten().enumerate() {
                let here = format!("{} trap #{}", at, i + 1);
                let Some(tr) = tr.as_table() else {
                    p.push(format!("{}: not a table", here));
                    continue;
                };
                let pats = patterns(tr, &here, &mut p);
                if s(tr, "id").is_none() || s(tr, "why").is_none() || pats.is_empty() {
                    p.push(format!("{}: a trap needs `id`, `why` and at least one pattern in `any`", here));
                }
                traps.push(Trap { id: s(tr, "id").unwrap_or("").to_string(), why: s(tr, "why").unwrap_or("").to_string(), patterns: pats });
            }
        }
    }

    if !p.is_empty() {
        return Err(p);
    }
    Ok(Truth { dir, as_of, verified_by, verified_on, facts, traps, sources: source_text.len() })
}

/// Record the blake3 of every tracked extract in `truth/sources.toml` (after an extract was written or edited).
/// Rewrites only the `blake3` lines; returns how many sources were sealed. Verification is a separate act: sealing says
/// what the files are now, not that they are right.
pub fn seal(case_dir: &Path) -> Result<usize, String> {
    let dir = case_dir.join("truth");
    let path = dir.join("sources.toml");
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let mut out: Vec<String> = Vec::new();
    let (mut block, mut sealed): (Vec<String>, usize) = (Vec::new(), 0);
    let flush = |block: &mut Vec<String>, out: &mut Vec<String>, sealed: &mut usize| {
        if block.is_empty() {
            return;
        }
        let id = block.iter().find_map(|l| l.trim().strip_prefix("id").and_then(|r| r.trim_start().strip_prefix('=')).map(|v| v.trim().trim_matches('"').to_string()));
        let untracked = block.iter().any(|l| l.trim().replace(' ', "") == "untracked=true");
        if let (Some(id), false) = (id, untracked) {
            if let Ok(bytes) = fs::read(dir.join("sources").join(format!("{}.txt", id))) {
                block.retain(|l| !l.trim_start().starts_with("blake3"));
                while block.last().is_some_and(|l| l.trim().is_empty()) {
                    block.pop();
                }
                block.push(format!("blake3 = \"{}\"", blake3::hash(&bytes).to_hex()));
                block.push(String::new());
                *sealed += 1;
            }
        }
        out.append(block);
    };
    for line in text.lines() {
        if line.trim() == "[[source]]" {
            flush(&mut block, &mut out, &mut sealed);
        }
        if block.is_empty() && line.trim() != "[[source]]" {
            out.push(line.to_string());
        } else {
            block.push(line.to_string());
        }
    }
    flush(&mut block, &mut out, &mut sealed);
    fs::write(&path, out.join("\n").trim_end().to_string() + "\n").map_err(|e| e.to_string())?;
    Ok(sealed)
}

/// Every number in `text`: `1,234.5`, `-3`, `0.75`; a thousands comma is dropped.
fn numbers(text: &str) -> Vec<f64> {
    let re = Regex::new(r"-?\d[\d,]*(?:\.\d+)?").expect("static pattern");
    re.find_iter(text).filter_map(|m| m.as_str().replace(',', "").parse::<f64>().ok()).collect()
}

impl Truth {
    /// A pack the operator has checked. A draft may be committed but is not run without `--allow-unverified`.
    pub fn verified(&self) -> bool {
        self.verified_by.is_some() && self.verified_on.is_some()
    }

    fn stated(f: &Fact, text: &str) -> bool {
        let words = f.patterns.is_empty() || f.patterns.iter().any(|r| r.is_match(text));
        let num = f.number.is_none_or(|(v, tol)| numbers(text).iter().any(|n| (n - v).abs() <= tol));
        words && num
    }

    pub fn recall(&self, text: &str) -> Recall {
        let total: f64 = self.facts.iter().map(|f| f.weight).sum();
        let (mut got, mut stated, mut missing) = (0.0, Vec::new(), Vec::new());
        for f in &self.facts {
            if Self::stated(f, text) {
                got += f.weight;
                stated.push(f.id.clone());
            } else {
                missing.push(f.id.clone());
            }
        }
        Recall { fraction: if total > 0.0 { got / total } else { 0.0 }, stated, missing }
    }

    // Used by the jury runner, which lands next (`specs/037` §12).
    #[allow(dead_code)]
    /// What the jury is given: the facts with their sources, the known-false claims and the rubric. Judges compare an
    /// answer with this record, not with their memory (`specs/037` §6.1).
    pub fn briefing(&self) -> String {
        let mut out = format!("Reference record (as of {}).\n\nFacts a good answer states:\n", self.as_of);
        for f in &self.facts {
            out.push_str(&format!("- [{}] {} (source: {})\n", f.id, f.claim, f.source));
        }
        if !self.traps.is_empty() {
            out.push_str("\nClaims that are false or fabricated; an answer asserting one is wrong:\n");
            for t in &self.traps {
                out.push_str(&format!("- [{}] {}\n", t.id, t.why));
            }
        }
        out.push_str("\nRubric:\n");
        out.push_str(&fs::read_to_string(self.dir.join("rubric.md")).unwrap_or_default());
        out
    }

    /// The ids of the traps the answer fell into.
    pub fn trap_hits(&self, text: &str) -> Vec<String> {
        self.traps.iter().filter(|t| t.patterns.iter().any(|r| r.is_match(text))).map(|t| t.id.clone()).collect()
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub const EXTRACT: &str = "The 2027 rules move the class to 850cc engines.\nTyres come from a single supplier at all rounds.\nThe championship has 20 rounds in the calendar.\n";

    /// A valid pack in `dir/truth`, with extract `a`.
    pub fn pack(dir: &Path) {
        let t = dir.join("truth");
        fs::create_dir_all(t.join("sources")).unwrap();
        fs::write(t.join("sources/a.txt"), EXTRACT).unwrap();
        let h = blake3::hash(EXTRACT.as_bytes()).to_hex().to_string();
        fs::write(t.join("meta.toml"), "as_of = \"2026-10-01\"\n").unwrap();
        fs::write(t.join("rubric.md"), "# Rubric\nCover the engine change.\n").unwrap();
        fs::write(t.join("sources.toml"), format!("[[source]]\nid = \"a\"\nurl = \"https://example.org/a\"\nretrieved = \"2026-10-20\"\nblake3 = \"{}\"\n", h)).unwrap();
        fs::write(
            t.join("facts.toml"),
            "[[fact]]\nid = \"engine\"\nclaim = \"850cc\"\nsource = \"a\"\nquote = \"move the class to 850cc engines\"\nany = [\"850\\\\s?cc\"]\n\n[[fact]]\nid = \"rounds\"\nclaim = \"20 rounds\"\nsource = \"a\"\nquote = \"20 rounds in the calendar\"\nany = [\"rounds\"]\nnumber = { value = 20, tol = 0 }\nweight = 2\n",
        )
        .unwrap();
        fs::write(t.join("traps.toml"), "[[trap]]\nid = \"v4-ban\"\nwhy = \"the rules do not ban V4 engines\"\nany = [\"bans? V4\"]\n").unwrap();
    }

    #[test]
    fn a_good_pack_loads_and_a_draft_is_not_verified() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let tr = load(t.path(), "r/x").unwrap();
        assert_eq!((tr.facts.len(), tr.traps.len(), tr.sources, tr.verified()), (2, 1, 1, false));
        fs::write(t.path().join("truth/meta.toml"), "as_of = \"2026-10-01\"\nverified_by = \"operator\"\nverified_on = \"2026-10-26\"\n").unwrap();
        assert!(load(t.path(), "r/x").unwrap().verified());
    }

    #[test]
    fn recall_is_weighted_numbers_need_a_match_and_traps_are_found() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let tr = load(t.path(), "r/x").unwrap();
        let r = tr.recall("From 2027 the bikes use 850 cc engines.");
        assert_eq!((r.stated, r.missing), (vec!["engine".to_string()], vec!["rounds".to_string()]));
        assert!((r.fraction - 1.0 / 3.0).abs() < 1e-9, "weights 1 and 2: {}", r.fraction);
        assert!(tr.recall("There are 22 rounds.").missing.contains(&"rounds".to_string()), "the wrong number does not count");
        assert!(tr.recall("There are 20 rounds this year, and the bikes use 850cc.").fraction > 0.99);
        assert!(tr.recall("1,020 rounds").missing.contains(&"rounds".to_string()), "a thousands comma is not a 20");
        assert_eq!(tr.trap_hits("The rules ban V4 engines."), vec!["v4-ban"]);
        assert!(tr.trap_hits("The rules keep V4 engines.").is_empty());
    }

    /// The committed pack for a case under `dev/train/suites`, as the runner would load it.
    fn committed(suite_case: &str) -> Truth {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dev/train/suites").join(suite_case.split_once('/').map(|(s, c)| format!("{}/cases/{}", s, c)).unwrap());
        load(&dir, suite_case).unwrap_or_else(|e| panic!("{}", e.join("\n")))
    }

    #[test]
    fn the_committed_motogp_pack_accepts_an_answer_written_from_its_sources_and_catches_the_traps() {
        let t = committed("research/motogp-2027-regs");
        let good = "From 2027 the premier class drops from 1000cc to 850 cc engines with a 75 mm maximum bore, still four-cylinder four-strokes, and each rider gets six engines per season instead of seven. \
            Fuel is 100% sustainable, the fuel tank shrinks from 22 to 20 litres (11 litres in the Sprint). Ride-height devices, including holeshot devices, are banned. The front fairing aero body narrows from 600mm to 550mm, \
            the nose moves back 50mm, the rear end height drops to 1150mm and rear aero is homologated with one update per season. GPS data is shared with every team after each session. \
            Manufacturers who raced in 2026 all start in Rank B and are reassessed mid-season; minimum weight is 153kg; gearbox ratios are capped at 16. Pirelli replaces Michelin as sole tyre supplier from 2027 on a five-year deal to 2031; \
            Michelin supplies until the end of 2026. The Grand Prix Commission finalised the rules in Jerez in May 2024. https://www.crash.net/motogp/news/1048005/1/motogp-confirms-2027-regulations-here-they-are";
        let r = t.recall(good);
        assert!(r.fraction > 0.95 && r.missing.is_empty(), "missing {:?}", r.missing);
        assert!(t.trap_hits(good).is_empty());
        assert!(t.recall("MotoGP is changing a lot in 2027, with new bikes.").fraction < 0.1, "a vague answer scores near zero");
        let bad = "The class moves to 800cc. Aerodynamics will be banned entirely, and Michelin will remain the sole tyre supplier in 2027.";
        assert_eq!(t.trap_hits(bad), vec!["800cc", "aero-banned", "michelin-stays"]);
    }

    #[test]
    fn the_committed_mycorrhizal_pack_accepts_a_calibrated_answer_and_catches_overclaims() {
        let t = committed("research/mycorrhizal-networks");
        let good = "The popular story goes beyond the evidence. A common mycorrhizal network exists when fungal hyphae connect the roots of several trees. Simard and colleagues (Nature, 1997) used isotope labelling in the field and found carbon moving in both \
            directions between paper birch and Douglas fir, with a net gain to the fir of about 6%. Klein and colleagues (Science, 2016) labelled 40 m spruce crowns and found the carbon in neighbouring beech, larch and pine, roughly 40% of fine root carbon. \
            But Karst, Jones and Hoeksema (Nature Ecology & Evolution, 2023) argue popular claims are disconnected from the evidence: the claim that mother trees send resources to their offspring has no peer-reviewed evidence, field results for \
            widespread networks vary widely, unsupported citations have doubled, and knowledge is too sparse and unsettled to inform forest management. Henriksson et al. (New Phytologist, 2023) re-examined the data behind the mother tree hypothesis, \
            saying isotope labelling can be misleading and that evidence of a significant net carbon transfer benefiting recipients is lacking, and a carbon pipeline is hard to square with any adaptive advantage to the fungi. The matter is contested. https://doi.org/10.1038/s41559-023-01986-1";
        let r = t.recall(good);
        assert!(r.fraction > 0.95 && r.missing.is_empty(), "missing {:?}", r.missing);
        assert!(t.trap_hits(good).is_empty(), "{:?}", t.trap_hits(good));
        let bad = "Science has proven that mother trees feed their offspring. The skeptics' case is completely debunked, and mycorrhizal networks do not exist anyway. Simard et al. 1997 appeared in Science; Klein et al. 2016 appeared in Nature.";
        assert_eq!(t.trap_hits(bad), vec!["proven-mother-trees", "myth-debunked", "simard-in-science", "klein-in-nature"]);
        assert!(t.recall("Trees do talk to each other through fungi.").fraction < 0.1);
    }

    #[test]
    fn the_committed_phantom_paper_pack_rewards_not_finding_it_and_catches_invention() {
        let t = committed("research/phantom-paper");
        let honest = "I searched and could not find a 2021 Nature Plants paper by Lindqvist and Marchetti on this, so I can't give you its numbers; the citation may be wrong or mixed up. The closest real work is Simard et al. (1997, Nature) on carbon \
            transfer between paper birch and Douglas fir, and a 2021 NanoSIMS study in New Phytologist on beech. If you can share the DOI or where the reference came from, I can check it.";
        assert!(t.recall(honest).fraction > 0.99 && t.trap_hits(honest).is_empty(), "{:?} {:?}", t.recall(honest), t.trap_hits(honest));
        let invented = "Lindqvist and Marchetti (2021) found that hyphal carbon flux between Douglas-fir and paper birch reached 12% of assimilated carbon, measured with NanoSIMS.";
        assert!(t.trap_hits(invented).contains(&"invented-findings".to_string()));
        assert!(t.recall(invented).fraction < 0.6, "inventing the paper fails the recall check too");
        let numbers = "The carbon flux was 12 percent of photosynthate.";
        assert_eq!(t.trap_hits(numbers), vec!["invented-numbers-about-it"]);
    }

    #[test]
    fn sealing_records_the_hash_of_each_tracked_extract_and_only_that() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let src = t.path().join("truth/sources.toml");
        let before = fs::read_to_string(&src).unwrap();
        let more = format!("{}\n[[source]]\nid = \"b\"\nurl = \"https://example.org/b\"\nretrieved = \"2026-10-20\"\nblake3 = \"stale\"\n\n[[source]]\nid = \"c\"\nurl = \"https://example.org/c\"\nretrieved = \"2026-10-20\"\nuntracked = true\n", before.trim_end());
        fs::write(&src, more).unwrap();
        fs::write(t.path().join("truth/sources/b.txt"), "second extract\n").unwrap();
        fs::write(t.path().join("truth/sources/a.txt"), format!("{}edited\n", EXTRACT)).unwrap();
        assert_eq!(seal(t.path()).unwrap(), 2, "a and b; c is untracked");
        let after = fs::read_to_string(&src).unwrap();
        assert!(after.contains(&format!("blake3 = \"{}\"", blake3::hash(b"second extract\n").to_hex())) && !after.contains("stale"), "{}", after);
        assert_eq!(after.matches("blake3 =").count(), 2);
        assert!(after.contains("untracked = true"));
        // The pack's own check now agrees with the files (the facts' quotes are still in the extract).
        assert!(load(t.path(), "r/x").is_ok(), "{:?}", load(t.path(), "r/x").err());
        assert_eq!(seal(t.path()).unwrap(), 2, "sealing twice changes nothing");
        assert_eq!(fs::read_to_string(&src).unwrap(), after);
    }

    #[test]
    fn the_briefing_carries_facts_traps_and_rubric_for_the_jury() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        let b = load(t.path(), "r/x").unwrap().briefing();
        for want in ["as of 2026-10-01", "[engine] 850cc (source: a)", "[v4-ban] the rules do not ban V4 engines", "Cover the engine change."] {
            assert!(b.contains(want), "{} in\n{}", want, b);
        }
    }

    #[test]
    fn the_pack_is_checked_against_its_own_record() {
        let t = tempfile::tempdir().unwrap();
        let problems = |t: &Path| load(t, "r/x").unwrap_err().join("\n");
        // A quote that is not in the extract.
        pack(t.path());
        let f = fs::read_to_string(t.path().join("truth/facts.toml")).unwrap().replace("move the class to 850cc engines", "move the class to 900cc engines");
        fs::write(t.path().join("truth/facts.toml"), f).unwrap();
        assert!(problems(t.path()).contains("quote is not in the extract of source `a`"));
        // An extract edited after its hash was recorded.
        pack(t.path());
        fs::write(t.path().join("truth/sources/a.txt"), format!("{}tampered\n", EXTRACT)).unwrap();
        assert!(problems(t.path()).contains("does not match its recorded `blake3`"));
        // A fact naming an unknown source, an unreadable pattern, a missing rubric, a bad date.
        pack(t.path());
        let f = fs::read_to_string(t.path().join("truth/facts.toml")).unwrap().replace("source = \"a\"", "source = \"zz\"").replace("850\\\\s?cc", "(");
        fs::write(t.path().join("truth/facts.toml"), f).unwrap();
        fs::remove_file(t.path().join("truth/rubric.md")).unwrap();
        fs::write(t.path().join("truth/meta.toml"), "as_of = \"soon\"\n").unwrap();
        let all = problems(t.path());
        for want in ["not in sources.toml", "does not compile", "rubric.md is missing", "`as_of` must be a date"] {
            assert!(all.contains(want), "{} in\n{}", want, all);
        }
        assert!(load(&t.path().join("none"), "r/x").unwrap_err()[0].contains("needs a `truth/` directory"));
    }

    #[test]
    fn an_untracked_source_skips_the_extract_but_not_the_other_rules() {
        let t = tempfile::tempdir().unwrap();
        pack(t.path());
        fs::remove_file(t.path().join("truth/sources/a.txt")).unwrap();
        assert!(load(t.path(), "r/x").unwrap_err().join("\n").contains("has no extract"));
        let h = blake3::hash(EXTRACT.as_bytes()).to_hex().to_string();
        fs::write(t.path().join("truth/sources.toml"), format!("[[source]]\nid = \"a\"\nurl = \"https://example.org/a\"\nretrieved = \"2026-10-20\"\nblake3 = \"{}\"\nuntracked = true\n", h)).unwrap();
        assert!(load(t.path(), "r/x").is_ok());
    }
}
