//! Library acquisition (`specs/055` §8; slice SK6): bringing in public-domain and research sources the operator is likely to
//! want, as **one batch in the digest** (`specs/052` §3.3), never a blocking prompt.
//!
//! The job is daemon-executed. A scout may *widen the topics* (a tool-less, budgeted model call; `specs/040` §20.2), and that is all
//! it does: the daemon searches the archives, reads their answers with its own parser, derives the licence from the archive's own
//! record (never from a model, never from the page), decides the capture level and runs the capture through the V2a fetcher via
//! [`Library::add_url`]. Nothing here lets an agent set a licence, raise a level or change a status (`specs/055` §10).
//!
//! * **Archives** are listed in `context/sources/archives.toml` (ratified in a release) plus `$DELPHI_HOME/operator/archives.toml`,
//!   the same shape of convention as `trusted.toml`. An archive names the hosts it may fetch from, how to read its search answer,
//!   either an open licence or a public-domain rule (jurisdiction, field, cut-off year), the archive's own licence or terms text,
//!   its attribution form and its politeness limits (`min_delay_ms`, `max_requests`).
//! * **Public domain is a recorded basis**, `public-domain:<jurisdiction>:<basis>`, computed from the archive's rule and the year its
//!   catalogue gives. A candidate with no year, or a year outside the rule, is not proposed.
//! * **Acquired material is not validated.** The source record says `not validated`, carries `acquired` (archive, batch, basis,
//!   topic, bytes) and the archive's `archive_notice`; any claim later made from it starts *noted*.
//! * **Budgets:** bytes per config (`[library.acquire] byte_budget_mib`; over it acquisition stops, nothing is evicted), and tokens
//!   for the scout as a share of the install's trailing week (`token_share_percent`).
//! * **Standing rules** ("take anything on these topics from these archives, up to this size") accept a matching candidate when it is
//!   proposed; it still appears in the batch, marked as taken by the rule. Only the operator creates one.
//! * `private` is the one hard refusal: a private source's tags never seed a topic, and no acquired source can be private.

use super::*;
use crate::fact_fetch::fetch_catalog;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

const DEFAULT_ARCHIVES: &str = include_str!("../../../context/sources/archives.toml");
pub const MAX_TOPICS: usize = 12;
const MAX_WIDENED: usize = 6;
const RESULT_CAP: usize = 40;

/// The public-domain rule of an archive: a candidate qualifies when the year its catalogue gives for `field` is before `before`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PdRule {
    pub jurisdiction: String,
    /// What the year is: `published`, or `author died`.
    pub field: String,
    pub before: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Archive {
    pub id: String,
    pub name: String,
    /// Registrable domains this archive may be read from (search, items). A URL elsewhere is refused.
    pub hosts: Vec<String>,
    /// `{query}` is replaced by the topic, URL-encoded.
    pub search_url: String,
    /// Dotted path to the array of results in the JSON answer (`results`, `response.docs`).
    pub results_path: String,
    pub id_path: String,
    pub title_path: String,
    #[serde(default)]
    pub author_path: Option<String>,
    #[serde(default)]
    pub year_path: Option<String>,
    #[serde(default)]
    pub size_path: Option<String>,
    /// `{id}` is replaced by the result's id.
    pub item_url: String,
    /// An open licence for everything the archive serves (an SPDX id), or…
    #[serde(default)]
    pub licence: Option<String>,
    /// …a public-domain rule applied to each result's year.
    #[serde(default)]
    pub public_domain: Option<PdRule>,
    /// The archive's own licence or terms text, kept with every source taken from it.
    pub notice: String,
    /// `{title}`, `{author}` and `{archive}` are filled in.
    #[serde(default)]
    pub attribution: Option<String>,
    #[serde(default = "two_secs")]
    pub min_delay_ms: u64,
    #[serde(default = "six")]
    pub max_requests: usize,
    /// Operator list only: remove this archive for this install.
    #[serde(default)]
    pub block: bool,
}

fn two_secs() -> u64 {
    2000
}

fn six() -> usize {
    6
}

#[derive(Deserialize)]
struct ArchiveFile {
    #[serde(default)]
    archive: Vec<Archive>,
}

fn host_ok(h: &str) -> bool {
    !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-') && !h.starts_with('.')
}

fn covered(hosts: &[String], url: &str) -> bool {
    let Ok(u) = delphi_adapter::reqwest::Url::parse(url) else { return false };
    let Some(h) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    hosts.iter().any(|d| {
        let d = d.to_ascii_lowercase();
        h == d || h.ends_with(&format!(".{}", d))
    })
}

impl Archive {
    fn check(&self, operator: bool, allow_http: bool) -> Result<(), String> {
        let who = format!("archive `{}`", self.id);
        if !valid_id(&self.id) {
            return Err(format!("{}: the id is lowercase letters, digits and hyphens", who));
        }
        if self.block {
            if !operator {
                return Err(format!("{}: `block` is for the operator's list; the default list only adds archives", who));
            }
            return Ok(());
        }
        if self.hosts.is_empty() || !self.hosts.iter().all(|h| host_ok(h)) {
            return Err(format!("{}: `hosts` names registrable domains", who));
        }
        for (what, url, slot) in [("search_url", &self.search_url, "{query}"), ("item_url", &self.item_url, "{id}")] {
            if !url.contains(slot) {
                return Err(format!("{}: `{}` has no {}", who, what, slot));
            }
            let u = delphi_adapter::reqwest::Url::parse(&url.replace(slot, "x")).map_err(|_| format!("{}: `{}` is not a URL", who, what))?;
            if u.scheme() != "https" && !(allow_http && u.scheme() == "http") {
                return Err(format!("{}: `{}` must be https", who, what));
            }
            if !covered(&self.hosts, u.as_str()) {
                return Err(format!("{}: `{}` is not on the archive's hosts", who, what));
            }
        }
        match (&self.licence, &self.public_domain) {
            (Some(l), None) => {
                if !matches!(classify(l), Licence::Open { .. }) {
                    return Err(format!("{}: `licence` must be an open SPDX id; use `public_domain` for out-of-copyright works", who));
                }
            }
            (None, Some(p)) => {
                if p.jurisdiction.trim().is_empty() || p.field.trim().is_empty() || p.before < 1 {
                    return Err(format!("{}: `public_domain` needs a jurisdiction, a field and a cut-off year", who));
                }
                if self.year_path.is_none() {
                    return Err(format!("{}: `public_domain` needs `year_path`, the year the rule is applied to", who));
                }
            }
            _ => return Err(format!("{}: set exactly one of `licence` and `public_domain`", who)),
        }
        if self.notice.trim().is_empty() {
            return Err(format!("{}: `notice` is the archive's own licence or terms text, and is kept with each source", who));
        }
        if self.max_requests == 0 || self.max_requests > 50 {
            return Err(format!("{}: `max_requests` is 1 to 50", who));
        }
        Ok(())
    }

    /// The licence record for one candidate, and the human reading of its basis. `None` when the archive's rule does not clear it.
    fn licence_for(&self, year: Option<i64>) -> Option<(String, String)> {
        if let Some(l) = &self.licence {
            return Some((l.clone(), format!("{} serves this under {}", self.name, l)));
        }
        let rule = self.public_domain.as_ref()?;
        let y = year?;
        if y >= rule.before {
            return None;
        }
        let basis = format!("{} {}, before {}; {}; per {}", rule.field, y, rule.before, rule.jurisdiction, self.name);
        Some((format!("public-domain:{}:{}", rule.jurisdiction.trim(), basis.replace(':', " ")), basis))
    }
}

/// The union of the default archive list and the operator's.
#[derive(Debug, Clone, Default)]
pub struct ArchiveList {
    archives: Vec<Archive>,
}

impl ArchiveList {
    pub fn parse(default: &str, operator: Option<&str>) -> Result<ArchiveList, String> {
        Self::parse_inner(default, operator, false)
    }

    fn parse_inner(default: &str, operator: Option<&str>, allow_http: bool) -> Result<ArchiveList, String> {
        let mut all: Vec<Archive> = Vec::new();
        let d: ArchiveFile = toml::from_str(default).map_err(|e| format!("context/sources/archives.toml: {}", e))?;
        for a in d.archive {
            a.check(false, allow_http).map_err(|e| format!("context/sources/archives.toml: {}", e))?;
            all.push(a);
        }
        if let Some(t) = operator {
            let o: ArchiveFile = toml::from_str(t).map_err(|e| format!("operator/archives.toml: {}", e))?;
            for a in o.archive {
                a.check(true, allow_http).map_err(|e| format!("operator/archives.toml: {}", e))?;
                all.retain(|x| x.id != a.id);
                all.push(a);
            }
        }
        all.retain(|a| !a.block);
        Ok(ArchiveList { archives: all })
    }

    /// The list this build ships and the operator's own file under `home`.
    pub fn load(home: &Path) -> Result<ArchiveList, String> {
        Self::parse(DEFAULT_ARCHIVES, std::fs::read_to_string(home.join("operator/archives.toml")).ok().as_deref())
    }

    #[cfg(test)]
    pub(crate) fn fixture(text: &str) -> ArchiveList {
        Self::parse_inner(text, None, true).unwrap()
    }

    pub fn archives(&self) -> &[Archive] {
        &self.archives
    }

    pub fn get(&self, id: &str) -> Option<&Archive> {
        self.archives.iter().find(|a| a.id == id)
    }
}

/// What the job needs from config, so the engine does not depend on how it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobConfig {
    pub byte_budget: u64,
    pub max_item: u64,
    pub batch_size: usize,
    pub token_share_percent: u64,
}

impl Default for JobConfig {
    fn default() -> Self {
        JobConfig { byte_budget: 2048 * MIB, max_item: 16 * MIB, batch_size: 8, token_share_percent: 5 }
    }
}

/// A standing rule: take anything on these topics from these archives, up to this size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub topics: Vec<String>,
    pub archives: Vec<String>,
    pub max_bytes: u64,
}

impl Rule {
    fn to_json(&self) -> Value {
        json!({"id": self.id, "topics": self.topics, "archives": self.archives, "max_bytes": self.max_bytes})
    }

    fn from_json(v: &Value) -> Option<Rule> {
        let strs = |k: &str| v[k].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>());
        Some(Rule { id: v["id"].as_str()?.into(), topics: strs("topics")?, archives: strs("archives")?, max_bytes: v["max_bytes"].as_u64()? })
    }

    fn takes(&self, topic: &str, archive: &str, size: Option<u64>) -> bool {
        self.topics.iter().any(|t| t == topic) && self.archives.iter().any(|a| a == archive) && size.is_none_or(|s| s <= self.max_bytes)
    }
}

/// A search topic: lowercase words, no markup, so nothing but plain words goes into a request to an archive.
pub fn clean_topic(s: &str) -> Option<String> {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { ' ' }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = t.chars().count();
    (2..=60).contains(&n).then_some(t)
}

fn plain(s: &str, max: usize) -> String {
    let one: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > max { format!("{}…", one.chars().take(max - 1).collect::<String>()) } else { one }
}

fn at<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(v, |cur, k| match k.parse::<usize>() {
        Ok(i) if cur.is_array() => cur.get(i),
        _ => cur.get(k),
    })
}

fn text_at(v: &Value, path: &str) -> Option<String> {
    match at(v, path)? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(a) => a.first().and_then(|x| x.as_str().map(String::from)),
        _ => None,
    }
}

fn year_at(v: &Value, path: &str) -> Option<i64> {
    match at(v, path)? {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.trim().get(..4).and_then(|y| y.parse().ok()),
        Value::Array(a) => a.first().and_then(|x| x.as_i64().or_else(|| x.as_str().and_then(|s| s.trim().get(..4)).and_then(|y| y.parse().ok()))),
        _ => None,
    }
}

fn urlencode(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b == b'-' { (b as char).to_string() } else { format!("%{:02X}", b) }).collect()
}

fn canonical(url: &str) -> Option<String> {
    let mut u = delphi_adapter::reqwest::Url::parse(url).ok()?;
    u.set_fragment(None);
    Some(u.to_string())
}

/// One candidate read from an archive's answer, before it is placed in a batch.
#[derive(Debug, Clone, PartialEq)]
struct Candidate {
    archive: String,
    url: String,
    title: String,
    author: Option<String>,
    year: Option<i64>,
    size: Option<u64>,
    licence: String,
    basis: String,
}

fn candidates(a: &Archive, body: &[u8]) -> Vec<Candidate> {
    let Ok(v) = serde_json::from_slice::<Value>(body) else { return Vec::new() };
    let Some(list) = at(&v, &a.results_path).and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .take(RESULT_CAP)
        .filter_map(|r| {
            let id = text_at(r, &a.id_path)?;
            let id: String = id.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).collect();
            if id.is_empty() {
                return None;
            }
            let url = canonical(&a.item_url.replace("{id}", &id))?;
            if !covered(&a.hosts, &url) {
                return None;
            }
            let title = plain(&text_at(r, &a.title_path)?, 200);
            let year = a.year_path.as_deref().and_then(|p| year_at(r, p));
            let (licence, basis) = a.licence_for(year)?;
            Some(Candidate {
                archive: a.id.clone(),
                url,
                title,
                author: a.author_path.as_deref().and_then(|p| text_at(r, p)).map(|s| plain(&s, 120)),
                year,
                size: a.size_path.as_deref().and_then(|p| at(r, p)).and_then(Value::as_u64),
                licence,
                basis,
            })
        })
        .collect()
}

/// The scout's one job: widen the topics. It sees topics only, has no tools and returns words; the daemon cleans them.
pub type ScoutFuture<'a> = Pin<Box<dyn Future<Output = Result<(Vec<String>, u64), String>> + Send + 'a>>;

pub trait Scout: Send + Sync {
    /// `room` is the tokens left in the scout's weekly share; the call must fit in it. Returns suggested topics and the tokens used.
    fn widen<'a>(&'a self, seed: &'a [String], room: u64) -> ScoutFuture<'a>;
}

/// What one run of the job did, for the operator's one line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobReport {
    pub batch: Option<String>,
    pub proposed: usize,
    pub taken_by_rule: usize,
    pub captured: Vec<String>,
    pub notes: Vec<String>,
    pub scout_tokens: u64,
}

fn slug(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    t.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

impl Library {
    fn acquire_dir(&self) -> PathBuf {
        self.root.join("acquire")
    }

    fn read_json(&self, rel: &str) -> Option<Value> {
        serde_json::from_slice(&std::fs::read(self.acquire_dir().join(rel)).ok()?).ok()
    }

    fn write_acquire(&self, rel: &str, v: &Value) -> Result<(), String> {
        self.ensure().map_err(|e| e.to_string())?;
        let p = self.acquire_dir().join(rel);
        std::fs::create_dir_all(p.parent().unwrap_or(&self.root)).map_err(|e| e.to_string())?;
        self.write_json(&p, v)
    }

    pub fn rules(&self) -> Vec<Rule> {
        self.read_json("rules.json").and_then(|v| v.as_array().map(|a| a.iter().filter_map(Rule::from_json).collect())).unwrap_or_default()
    }

    /// The operator's standing rule. Topics are cleaned the way a search topic is; an archive must exist if `known` is given.
    pub fn add_rule(&self, topics: &[String], archives: &[String], max_bytes: u64, known: Option<&ArchiveList>) -> Result<Rule, String> {
        let topics: Vec<String> = topics.iter().filter_map(|t| clean_topic(t)).collect();
        if topics.is_empty() || archives.is_empty() || max_bytes == 0 {
            return Err("a rule needs at least one topic, one archive and a size".into());
        }
        if let Some(k) = known {
            if let Some(a) = archives.iter().find(|a| k.get(a).is_none()) {
                return Err(format!("`{}` is not a listed archive", plain(a, 60)));
            }
        }
        let mut rules = self.rules();
        let n = rules.iter().filter_map(|r| r.id.strip_prefix("rule-").and_then(|n| n.parse::<u64>().ok())).max().unwrap_or(0) + 1;
        let rule = Rule { id: format!("rule-{}", n), topics, archives: archives.to_vec(), max_bytes };
        rules.push(rule.clone());
        self.write_acquire("rules.json", &Value::Array(rules.iter().map(Rule::to_json).collect()))?;
        self.log("acquire_rule", json!({"op": "add", "rule": rule.to_json()}));
        Ok(rule)
    }

    pub fn remove_rule(&self, id: &str) -> Result<(), String> {
        let rules = self.rules();
        if !rules.iter().any(|r| r.id == id) {
            return Err(format!("no rule `{}`", plain(id, 40)));
        }
        let kept: Vec<Value> = rules.iter().filter(|r| r.id != id).map(Rule::to_json).collect();
        self.write_acquire("rules.json", &Value::Array(kept))?;
        self.log("acquire_rule", json!({"op": "remove", "id": id}));
        Ok(())
    }

    fn skipped(&self) -> Vec<String> {
        self.read_json("skipped.json").and_then(|v| v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())).unwrap_or_default()
    }

    /// Every batch, oldest first.
    pub fn batches(&self) -> Vec<Value> {
        let Ok(rd) = std::fs::read_dir(self.acquire_dir().join("batches")) else { return Vec::new() };
        let mut v: Vec<Value> = rd.flatten().filter_map(|e| serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()).collect();
        v.sort_by(|a: &Value, b: &Value| a["id"].as_str().cmp(&b["id"].as_str()));
        v
    }

    pub fn batch(&self, id: &str) -> Option<Value> {
        self.batches().into_iter().find(|b| b["id"] == id)
    }

    /// The batch waiting for the operator: at least one item still `proposed`.
    pub fn open_batch(&self) -> Option<Value> {
        self.batches().into_iter().rev().find(|b| b["items"].as_array().is_some_and(|i| i.iter().any(|x| x["status"] == "proposed")))
    }

    fn save_batch(&self, b: &Value) -> Result<(), String> {
        let id = b["id"].as_str().ok_or("batch without an id")?;
        self.write_acquire(&format!("batches/{}.json", id), b)
    }

    /// Bytes acquisition holds now: what its sources' captures say they took. Purged sources count nothing.
    pub fn acquired_bytes(&self) -> u64 {
        self.list().iter().filter(|(_, m)| !m["acquired"].is_null() && m["purged"].is_null()).map(|(id, _)| dir_bytes(&self.source_dir(id))).sum()
    }

    /// The operator's act on a batch: `accept` or `skip` all of it or the named items. Returns the ids acted on.
    pub fn decide_batch(&self, batch: &str, action: &str, only: &[String]) -> Result<Vec<String>, String> {
        if !["accept", "skip"].contains(&action) {
            return Err(format!("`{}` is not accept or skip", plain(action, 30)));
        }
        let mut b = self.batch(batch).ok_or_else(|| format!("no batch `{}`", plain(batch, 40)))?;
        let mut done = Vec::new();
        for it in b["items"].as_array_mut().into_iter().flatten() {
            let id = it["id"].as_str().unwrap_or("").to_string();
            if it["status"] == "proposed" && (only.is_empty() || only.contains(&id)) {
                it["status"] = json!(if action == "accept" { "accepted" } else { "skipped" });
                it["decided"] = json!(delphi_core::time::now_rfc3339());
                done.push(id);
            }
        }
        if let Some(missing) = only.iter().find(|o| !done.contains(o)) {
            return Err(format!("`{}` is not waiting in batch {}", plain(missing, 60), batch));
        }
        if action == "skip" {
            let mut s = self.skipped();
            s.extend(done.iter().cloned());
            s.sort();
            s.dedup();
            self.write_acquire("skipped.json", &json!(s))?;
        }
        self.save_batch(&b)?;
        self.log("acquire_decide", json!({"batch": batch, "action": action, "ids": done}));
        Ok(done)
    }

    /// Topics the job may search for: the operator's interests, the workspace's project names and the tags of the library's own
    /// sources. A private source's tags never go into a request to an archive (`specs/055` §10.1).
    pub fn seed_topics(&self, interests: &[String], projects: &[String]) -> Vec<String> {
        let tags = self.list().into_iter().filter(|(_, m)| may_leave(m, Channel::WebSearch).is_ok() && m["purged"].is_null()).flat_map(|(_, m)| m["tags"].as_array().cloned().unwrap_or_default()).filter_map(|t| t.as_str().map(String::from)).collect::<Vec<_>>();
        let mut out: Vec<String> = Vec::new();
        for t in interests.iter().chain(projects).chain(&tags).filter_map(|t| clean_topic(t)) {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out.truncate(MAX_TOPICS);
        out
    }

    fn state(&self) -> Value {
        self.read_json("state.json").unwrap_or(json!({}))
    }

    /// Is a daily run due: none in the last 24 hours, and no batch is waiting for the operator.
    pub fn job_due(&self, now_unix: i64) -> bool {
        self.open_batch().is_none() && self.state()["last_run_unix"].as_i64().is_none_or(|t| now_unix - t >= 86_400)
    }

    fn mark_run(&self, now_unix: i64) {
        let mut s = self.state();
        s["last_run_unix"] = json!(now_unix);
        let _ = self.write_acquire("state.json", &s);
    }

    /// Scout tokens spent in the trailing week.
    pub fn scout_spent(&self, now_unix: i64) -> u64 {
        std::fs::read_to_string(self.acquire_dir().join("tokens.jsonl")).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).filter(|v| v["at"].as_i64().unwrap_or(0) >= now_unix - 7 * 86_400).map(|v| v["tokens"].as_u64().unwrap_or(0)).sum()
    }

    fn record_scout(&self, now_unix: i64, tokens: u64) {
        use std::io::Write;
        let _ = self.ensure();
        let _ = std::fs::create_dir_all(self.acquire_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(self.acquire_dir().join("tokens.jsonl")) {
            let _ = writeln!(f, "{}", json!({"at": now_unix, "tokens": tokens}));
        }
    }

    /// Tokens the scout may use now: its share of the install's trailing week less what it already used (`specs/040` §20.2).
    pub fn scout_room(&self, cfg: &JobConfig, install_week_tokens: u64, now_unix: i64) -> u64 {
        let share = ((cfg.token_share_percent as f64 / 100.0) * install_week_tokens as f64) as u64;
        share.saturating_sub(self.scout_spent(now_unix))
    }

    /// Search the archives for `topics` and write one batch of proposals. Rate limits and the request cap of each archive hold across
    /// topics. Candidates already in the library, skipped before or proposed before are left out; candidates a standing rule covers are
    /// marked `accepted` (taken by the rule). `Ok(None)` when nothing new was found or a batch is already waiting.
    pub async fn propose(&self, policy: &FetchPolicy, archives: &ArchiveList, cfg: &JobConfig, topics: &[String]) -> Result<Option<Value>, String> {
        if self.open_batch().is_some() {
            return Ok(None);
        }
        let rules = self.rules();
        let skipped = self.skipped();
        let earlier: Vec<String> = self.batches().iter().flat_map(|b| b["items"].as_array().cloned().unwrap_or_default()).filter_map(|i| i["id"].as_str().map(String::from)).collect();
        let mut items: Vec<Value> = Vec::new();
        let mut used: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let mut last: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        'topics: for topic in topics.iter().filter_map(|t| clean_topic(t)) {
            for a in archives.archives() {
                if *used.get(&a.id).unwrap_or(&0) >= a.max_requests {
                    continue;
                }
                if let Some(t) = last.get(&a.id) {
                    let wait = std::time::Duration::from_millis(a.min_delay_ms).saturating_sub(t.elapsed());
                    if !wait.is_zero() {
                        tokio::time::sleep(wait).await;
                    }
                }
                *used.entry(a.id.clone()).or_default() += 1;
                last.insert(a.id.clone(), std::time::Instant::now());
                let url = a.search_url.replace("{query}", &urlencode(&topic));
                let Ok(body) = fetch_catalog(policy, &url).await else { continue };
                for c in candidates(a, &body) {
                    let id = source_id(&c.url);
                    if self.read_meta(&id).is_some() || skipped.contains(&id) || earlier.contains(&id) || items.iter().any(|i| i["id"] == id) {
                        continue;
                    }
                    if c.size.is_some_and(|s| s > cfg.max_item) {
                        continue;
                    }
                    let rule = rules.iter().find(|r| r.takes(&topic, &a.id, c.size));
                    items.push(json!({
                        "id": id, "archive": c.archive, "url": c.url, "title": c.title, "author": c.author, "year": c.year, "size": c.size,
                        "topic": topic, "licence": c.licence, "basis": c.basis,
                        "status": if rule.is_some() { "accepted" } else { "proposed" }, "rule": rule.map(|r| r.id.clone()),
                    }));
                    if items.len() >= cfg.batch_size {
                        break 'topics;
                    }
                }
            }
        }
        if items.is_empty() {
            return Ok(None);
        }
        let now = delphi_core::time::now_rfc3339();
        let n = self.batches().len() + 1;
        let b = json!({"id": format!("acq-{}-{}", &now[..10], n), "created": now, "topics": topics, "items": items});
        self.save_batch(&b)?;
        self.log("acquire_batch", json!({"batch": b["id"], "items": items.len()}));
        Ok(Some(b))
    }

    /// Capture every accepted item through the V2a fetcher. The daemon decides the level (`full`) and the licence (the archive's
    /// record); the archive's notice, attribution and politeness limits are honoured; the acquisition byte budget stops the run
    /// and evicts nothing.
    pub async fn run_accepted(&self, policy: &FetchPolicy, archives: &ArchiveList, cfg: &JobConfig) -> Vec<(String, Result<Stored, String>)> {
        let mut out = Vec::new();
        let mut last: std::collections::HashMap<String, std::time::Instant> = std::collections::HashMap::new();
        let mut stopped = false;
        for mut batch in self.batches() {
            let mut changed = false;
            let items = batch["items"].as_array().cloned().unwrap_or_default();
            for (i, it) in items.iter().enumerate().filter(|(_, it)| it["status"] == "accepted") {
                let id = it["id"].as_str().unwrap_or("").to_string();
                let set = |batch: &mut Value, status: &str, why: Option<String>| {
                    batch["items"][i]["status"] = json!(status);
                    if let Some(w) = why {
                        batch["items"][i]["reason"] = json!(plain(&w, 300));
                    }
                };
                let Some(a) = it["archive"].as_str().and_then(|a| archives.get(a)) else {
                    set(&mut batch, "failed", Some("the archive is no longer listed".into()));
                    changed = true;
                    continue;
                };
                let url = it["url"].as_str().unwrap_or("").to_string();
                let licence = it["licence"].as_str().unwrap_or("").to_string();
                // Recomputed from the archive's own rule: a stored licence the archive would not give now is not used.
                let year = it["year"].as_i64();
                if !covered(&a.hosts, &url) || a.licence_for(year).map(|(l, _)| l) != Some(licence.clone()) {
                    set(&mut batch, "failed", Some("the archive's record no longer clears this item".into()));
                    changed = true;
                    continue;
                }
                if stopped || self.acquired_bytes() + it["size"].as_u64().unwrap_or(0) >= cfg.byte_budget {
                    stopped = true;
                    set(&mut batch, "budget", Some(format!("the acquisition byte budget ({} bytes) is reached; raise `[library.acquire] byte_budget_mib` or purge something, then accept again", cfg.byte_budget)));
                    changed = true;
                    continue;
                }
                if let Some(t) = last.get(&a.id) {
                    let wait = std::time::Duration::from_millis(a.min_delay_ms).saturating_sub(t.elapsed());
                    if !wait.is_zero() {
                        tokio::time::sleep(wait).await;
                    }
                }
                last.insert(a.id.clone(), std::time::Instant::now());
                let author = it["author"].as_str().unwrap_or("");
                let opts = AddOpts {
                    licence: Some(licence.clone()),
                    level: Some(Level::Full),
                    private: false,
                    title: it["title"].as_str().map(String::from),
                    rights_holder: None,
                    attribution_text: a.attribution.as_ref().map(|t| t.replace("{title}", it["title"].as_str().unwrap_or("")).replace("{author}", author).replace("{archive}", &a.name)),
                    doi: None,
                    raise_text: None,
                    raise_source: None,
                };
                let budgets = Budgets { text: cfg.max_item, ..Budgets::default() };
                match self.add_url(policy, &url, &opts, &budgets).await {
                    Ok(s) => {
                        let bytes = dir_bytes(&self.source_dir(&s.id));
                        let note = json!({
                            "archive": a.id, "batch": batch["id"], "topic": it["topic"], "basis": it["basis"], "bytes": bytes, "rule": it["rule"],
                            "at": delphi_core::time::now_rfc3339(), "validated": false,
                        });
                        let _ = self.annotate(&s.id, |m| {
                            m.insert("acquired".into(), note.clone());
                            m.insert("archive_notice".into(), json!(a.notice));
                            if let Some(t) = it["topic"].as_str() {
                                let tags = m.entry("tags").or_insert(json!([]));
                                if let Some(arr) = tags.as_array_mut() {
                                    if !arr.iter().any(|x| x == t) {
                                        arr.push(json!(t));
                                    }
                                }
                            }
                        });
                        if let Some(t) = it["topic"].as_str() {
                            let _ = self.reading_add(&format!("acquired-{}", slug(t)), Some(&format!("Acquired: {}", t)), std::slice::from_ref(&s.id));
                        }
                        set(&mut batch, "captured", (!s.notes.is_empty()).then(|| s.notes.join("; ")));
                        out.push((id, Ok(s)));
                    }
                    Err(e) => {
                        set(&mut batch, "failed", Some(e.clone()));
                        out.push((id, Err(e)));
                    }
                }
                changed = true;
            }
            if changed {
                let _ = self.save_batch(&batch);
            }
        }
        out
    }

    /// Rewrite one source's `meta.json` in place for a daemon-side annotation (never the licence, level or status).
    fn annotate(&self, id: &str, f: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
        let mut m = self.read_meta(id).ok_or("no such source")?;
        let obj = m.as_object_mut().ok_or("bad record")?;
        let keep = (obj.get("licence").cloned(), obj.get("capture_level").cloned(), obj.get("status").cloned(), obj.get("private").cloned());
        f(obj);
        for (k, v) in [("licence", keep.0), ("capture_level", keep.1), ("status", keep.2), ("private", keep.3)] {
            match v {
                Some(v) => obj.insert(k.into(), v),
                None => obj.remove(k),
            };
        }
        self.write_json(&self.source_dir(id).join("meta.json"), &m)
    }

    /// One run of the whole job: optionally widen the topics with the scout, search, write the batch, and execute what a standing
    /// rule accepted. `install_week_tokens` bounds the scout (`specs/040` §20.2); with no room or no scout it is model-free.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_job(&self, policy: &FetchPolicy, archives: &ArchiveList, cfg: &JobConfig, seed: Vec<String>, scout: Option<&dyn Scout>, install_week_tokens: u64, now_unix: i64) -> JobReport {
        let mut report = JobReport::default();
        self.mark_run(now_unix);
        if archives.archives().is_empty() {
            report.notes.push("no archive is listed (context/sources/archives.toml, operator/archives.toml)".into());
            return report;
        }
        let mut topics = seed;
        if let Some(s) = scout {
            let room = self.scout_room(cfg, install_week_tokens, now_unix);
            if room == 0 {
                report.notes.push("the scout's token share for the week is used up; searching the topics you and the workspace gave".into());
            } else {
                match s.widen(&topics, room).await {
                    Ok((more, tokens)) => {
                        self.record_scout(now_unix, tokens);
                        report.scout_tokens = tokens;
                        for t in more.iter().filter_map(|t| clean_topic(t)).take(MAX_WIDENED) {
                            if !topics.contains(&t) {
                                topics.push(t);
                            }
                        }
                    }
                    Err(e) => report.notes.push(format!("the scout could not widen the topics: {}", plain(&e, 120))),
                }
            }
        }
        topics.truncate(MAX_TOPICS + MAX_WIDENED);
        if topics.is_empty() {
            report.notes.push("no topics: add `[library.acquire] interests` or register a project".into());
            return report;
        }
        match self.propose(policy, archives, cfg, &topics).await {
            Ok(Some(b)) => {
                report.batch = b["id"].as_str().map(String::from);
                let items = b["items"].as_array().cloned().unwrap_or_default();
                report.taken_by_rule = items.iter().filter(|i| i["status"] == "accepted").count();
                report.proposed = items.len() - report.taken_by_rule;
            }
            Ok(None) => report.notes.push("nothing new to propose".into()),
            Err(e) => report.notes.push(e),
        }
        if report.taken_by_rule > 0 {
            for (id, r) in self.run_accepted(policy, archives, cfg).await {
                match r {
                    Ok(_) => report.captured.push(id),
                    Err(e) => report.notes.push(format!("{}: {}", id, plain(&e, 160))),
                }
            }
        }
        self.log("acquire_run", json!({"batch": report.batch, "proposed": report.proposed, "taken_by_rule": report.taken_by_rule, "captured": report.captured.len(), "scout_tokens": report.scout_tokens}));
        report
    }
}
