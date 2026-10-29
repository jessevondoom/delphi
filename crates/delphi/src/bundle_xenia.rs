//! Bundles through a xenia store (`specs/027` §4.3, slice B8): publish a bundle as a `bundle` record, add one by record id,
//! update it as a new `rev`, and keep its level honest as the store's standing changes.
//!
//! Everything fetched is hostile and the store is only a locator with a signature on it:
//!
//! * **The record is believed only as far as the verified store.** `BundleSet::offline` re-verifies every record from its
//!   bytes against the roster the operator pinned; a record's text never states its level.
//! * **The tree is fetched by the marketplace path** (`bundle_fetch`): https only, no checkout, links and submodules
//!   refused, caps on bytes and files, the pinned commit or tree required. The staged tree then goes through the same
//!   `inspect` as a directory add, and its hash must be exactly the record's `bundle_hash` (a changed byte is tampering).
//! * **The signature on the tree** must be by the member who published the record, or a key in that member's `successor`
//!   chain (`bundle_add::apply_via`). A member cannot publish somebody else's signed bundle as their own, and a record
//!   cannot carry a tree its publisher did not sign.
//! * **Updates only go forward.** A record `rev` or a bundle version below what the install already has is a replay (a
//!   host serving an old head) or a downgrade, and is refused; `rollback` is the operator's way back. An automatic update
//!   (`sync_bundles` with `auto`) never changes a publisher and never lowers a bundle's level.
//! * **The level is the store's.** `endorsed` and `member-signed` are written to the lock from the computed standing, and
//!   `sync_bundles` recomputes them when the store goes stale or an endorsement is withdrawn.

use crate::bundle_add::{self, AddOpts, Mode, Opened, StoreAdd, Via};
use crate::bundle_fetch::{Fetcher, Rev};
use delphi_bundle::{Host, Options};
use delphi_core::bundles::{self, Entry};
use delphi_core::xenia_bundle::{self, BundleDraft, BundleSet, StoreBundle, SyncNote};
use delphi_core::xenia_publish::{Approver, Decision, NoIndex, Preview, Publisher, Published};
use delphi_core::xenia_store::Store;
use delphi_core::xenia_sync::VerifyConfig;
use delphi_xenia::identity::Identity;
use delphi_xenia::pointer::{ContentType, Pin, PinKind, Pointer, Scheme};
use delphi_xenia::record::{check_bundle_tree, AuthorStatus, Confidence, KindFields, Role, SourceCeiling};
use delphi_xenia::tier::BundleLevel;
use std::path::{Path, PathBuf};

fn fail(msg: impl AsRef<str>) -> i32 {
    eprintln!("delphi bundle: {}", msg.as_ref());
    1
}

/// The clock the verifier judges freshness at.
pub fn cfg_now() -> VerifyConfig {
    #[cfg(test)]
    {
        // A fixture's freshness statement is only fresh around its own date; the xenia command tests pin the clock.
        let t = crate::xenia_cmd::TEST_NOW.with(|c| c.get());
        if t != 0 {
            return VerifyConfig::at(t);
        }
    }
    VerifyConfig::at(delphi_core::xenia_index::unix_of(&delphi_core::time::now_rfc3339()).unwrap_or(0))
}

/// `(org, store)` this install pinned for `store`.
fn ids_of(store: &Store) -> Result<(String, String), String> {
    let pins = delphi_xenia::pins::load(&store.pin_storage()).map_err(|e| e.reason().to_string())?.ok_or("this store has not been joined")?;
    Ok((pins.org().to_string(), pins.store().to_string()))
}

/// Where a record's tree is: `(repository url, directory or None for the root, how it is pinned)`. The pointer was checked by
/// the verifier (`check_bundle_tree`); this reads it again so that nothing but a checked pointer is ever followed.
fn tree_target(p: &Pointer) -> Result<(String, Option<String>), String> {
    check_bundle_tree(p).map_err(|b| format!("the record's tree pointer is not acceptable ({}: {})", b.field, b.why))?;
    let (url, frag) = p.locator.split_once('#').ok_or("the tree locator has no directory")?;
    Ok((url.to_string(), if frag == "." { None } else { Some(frag.to_string()) }))
}

fn semver_of(v: &str) -> Option<semver::Version> {
    semver::Version::parse(v).ok()
}

/// The forward-only rule (`specs/027` §4.3 "updates are new revs"): a record older than the one this install added, or a
/// bundle version below the installed one, is refused. The record is what the store serves now; the lock is what happened.
fn forward_only(ws: &Path, org: &str, store: &str, b: &StoreBundle) -> Result<(), String> {
    let lock = bundles::read(ws)?;
    let Some(e) = lock.bundle.iter().find(|e| e.is_active() && e.org == org && e.name == b.name) else { return Ok(()) };
    if e.source.kind == "xenia" {
        if let Some((o, s, id, rev)) = xenia_bundle::parse_source_path(&e.source.path) {
            if o == org && s == store && id == b.id && b.rev < rev {
                return Err(format!("the store serves revision {} of this record but this install already has revision {}: the host is serving an older state (a replay); nothing was changed", b.rev, rev));
            }
        }
    }
    if let (Some(have), Some(want)) = (semver_of(&e.version), semver_of(&b.version)) {
        if want < have {
            return Err(format!("{} is {} here and the record offers {}, which is older; `delphi bundle rollback {}` is how to go back deliberately", e.id(), have, want, e.id()));
        }
    }
    Ok(())
}

/// Fetch the record's tree into a staging directory and run the ordinary add on it.
#[allow(clippy::too_many_arguments)]
fn fetch_and_apply(home: &Path, ws: &Path, set: &BundleSet, b: &StoreBundle, o: &AddOpts, mode: Mode, fetcher: &dyn Fetcher, host: &dyn Host) -> i32 {
    if b.hidden.is_some() || b.level == BundleLevel::Hidden {
        return fail(format!("the record {} is not offered ({}): it was withdrawn; nothing was added", b.id, b.hidden.unwrap_or("hidden")));
    }
    if let Err(e) = forward_only(ws, &set.org, &set.store, b) {
        return fail(e);
    }
    let (url, subdir) = match tree_target(&b.tree) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    let Some(pin) = b.tree.pin.value.clone() else { return fail("the tree is not pinned") };
    let staging = match bundle_add::new_incoming(home) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let source_path = xenia_bundle::source_path(&set.org, &set.store, &b.id, b.rev);
    // Dropped (the staging tree removed) on every path that does not move the tree into the cache.
    let opened = Opened { dir: staging.clone(), kind: "xenia", path: source_path, incoming: Some(staging.clone()) };
    let rev = match b.tree.pin.kind {
        PinKind::GitTree => Rev::Tree(&pin),
        _ => Rev::Sha(&pin),
    };
    match fetcher.fetch(&url, rev, subdir.as_deref(), &staging) {
        Ok(commit) => {
            if b.tree.pin.kind == PinKind::GitCommit && commit != pin {
                return fail(format!("fetched commit {} is not the pinned {}; nothing was added", commit, pin));
            }
        }
        Err(e) => return fail(format!("fetching {}: {}", b.name, e)),
    }
    println!("fetched {}@{} (record {} rev {}, pinned by the store; the store is a locator, the bundle is checked as any other)", b.name, b.version, &b.id[..8], b.rev);
    let via = Via { expect_hash: Some(&b.bundle_hash), store: Some(StoreAdd { bundle: b, org: &set.org, store: &set.store, fresh: set.fresh }) };
    bundle_add::apply_via(home, ws, opened, o, mode, host, &via)
}

fn load_set(store: &Store, cfg: &VerifyConfig) -> Result<BundleSet, String> {
    match xenia_bundle::sync(store, cfg)? {
        SyncNote::Verified => {}
        SyncNote::Offline(why) => eprintln!("note: the store could not be reached ({}); using what this install last verified", why),
    }
    let set = BundleSet::offline(store, cfg)?;
    if !set.fresh {
        eprintln!("note: the store's freshness statement is not current, so no endorsement counts now: every bundle reads member-signed");
    }
    Ok(set)
}

/// `delphi bundle add --from-xenia <org>/<store> <record id>`.
#[allow(clippy::too_many_arguments)]
pub fn add(home: &Path, ws: &Path, store: &Store, cfg: &VerifyConfig, id: &str, o: &AddOpts, fetcher: &dyn Fetcher, host: &dyn Host) -> i32 {
    let set = match load_set(store, cfg) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let Some(b) = set.find(id) else { return fail(format!("the store {}/{} has no bundle record {} (try `delphi bundle store {}/{}`)", set.org, set.store, id, set.org, set.store)) };
    fetch_and_apply(home, ws, &set, b, o, Mode::Add, fetcher, host)
}

/// `delphi bundle update` for an entry whose source is a store: sync, find the record the entry came from, and apply its
/// current revision under the continuity rule. `entry` is the lock entry.
#[allow(clippy::too_many_arguments)]
pub fn update_with(home: &Path, ws: &Path, store: &Store, cfg: &VerifyConfig, entry: &Entry, o: &AddOpts, fetcher: &dyn Fetcher, host: &dyn Host) -> i32 {
    let Some((org, store_name, id, _)) = xenia_bundle::parse_source_path(&entry.source.path) else { return fail(format!("{}: the recorded store source is damaged", entry.id())) };
    match ids_of(store) {
        Ok((o2, s2)) if o2 == org && s2 == store_name => {}
        _ => return fail(format!("{} came from {}/{}, not from this store", entry.id(), org, store_name)),
    }
    let set = match load_set(store, cfg) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let Some(b) = set.find(&id) else { return fail(format!("{}: the store no longer has the record {}; the installed version stays", entry.id(), id)) };
    if let Some(why) = b.hidden {
        return fail(format!("{}: the store withdrew the record ({}); the installed version stays (remove it, or roll back, if that is what you want)", entry.id(), why));
    }
    let eid = entry.id();
    fetch_and_apply(home, ws, &set, b, o, Mode::Update(&eid), fetcher, host)
}

/// `bundle update` calls this for a store source: the store is opened from the install's own pins, the fetch is the real one.
pub fn update_entry(home: &Path, ws: &Path, entry: &Entry, background: bool, host: &dyn Host) -> i32 {
    let Some((org, store_name, _, _)) = xenia_bundle::parse_source_path(&entry.source.path) else { return fail(format!("{}: the recorded store source is damaged", entry.id())) };
    let store = match xenia_bundle::open_joined(home, &org, &store_name) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let o = AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: background };
    update_with(home, ws, &store, &cfg_now(), entry, &o, &crate::bundle_fetch::real_fetcher(home), host)
}

/// One line per record of the store, for `delphi bundle store <org>/<store>`.
pub fn list(store: &Store, cfg: &VerifyConfig, ws: &Path) -> i32 {
    let set = match load_set(store, cfg) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let lock = bundles::read(ws).unwrap_or_default();
    if set.bundles.is_empty() {
        println!("{}/{} offers no bundles", set.org, set.store);
        return 0;
    }
    for b in &set.bundles {
        let have = lock.bundle.iter().find(|e| e.is_active() && e.org == set.org && e.name == b.name);
        let state = match have {
            Some(e) if e.hash == b.bundle_hash => "added".to_string(),
            Some(e) => format!("added {}, this is {}", e.version, b.version),
            None => "not added".to_string(),
        };
        let level = match (b.hidden, b.level) {
            (Some(why), _) => format!("withdrawn ({why})"),
            (None, BundleLevel::Endorsed) => format!("endorsed by {}", b.endorsed_by.join(", ")),
            (None, _) => format!("member-signed by {}{}", b.publisher_handle, if set.fresh { "" } else { " (store stale)" }),
        };
        println!("{}  {}@{}  rev {}  {}  [{}]\n    {}", b.id, b.name, b.version, b.rev, level, state, b.title);
        let same = set.named(&b.name);
        if same.len() > 1 && b.hidden.is_none() {
            println!("    warning: {} records in this store name a bundle `{}`; check the record id before adding", same.len(), b.name);
        }
    }
    0
}

/// What `sync_bundles` found for the entries that came from this store.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// `org/name`, new version, new rev: a newer revision is in the store.
    pub available: Vec<(String, String, u64)>,
    /// Applied by `auto`.
    pub updated: Vec<String>,
    /// Not applied, and why.
    pub skipped: Vec<(String, String)>,
    /// Lock levels recomputed from the store's standing: `org/name`, old level, new level.
    pub relevelled: Vec<(String, String, String)>,
}

/// `delphi xenia sync --auto-update-bundles` for the bundles added from this store: report the newer revisions waiting,
/// recompute the levels in the lock from the store's standing (a stale store, a withdrawn endorsement), and with `auto`
/// apply the updates that are safe to apply unattended. **Safe** means: the same publisher (no `--replace-publisher`
/// is ever implied), a level no lower than the installed one's (a new revision still waiting for an endorsement is
/// reported, not applied), a record that is still offered, and everything an ordinary update checks.
#[allow(clippy::too_many_arguments)]
pub fn sync_bundles(home: &Path, ws: &Path, store: &Store, cfg: &VerifyConfig, auto: bool, background: bool, fetcher: &dyn Fetcher, host: &dyn Host) -> Result<SyncReport, String> {
    let set = load_set(store, cfg)?;
    sync_bundles_in(home, ws, &set, auto, background, fetcher, host)
}

/// [`sync_bundles`] over a store state the caller has just verified (`delphi xenia sync` has synced already and does not fetch
/// twice): `BundleSet::offline` is that state.
pub fn sync_bundles_in(home: &Path, ws: &Path, set: &BundleSet, auto: bool, background: bool, fetcher: &dyn Fetcher, host: &dyn Host) -> Result<SyncReport, String> {
    let lock = bundles::read(ws)?;
    let mut report = SyncReport::default();
    let rank = |l: &str| match l {
        "endorsed" => 2,
        "member-signed" => 1,
        _ => 0,
    };
    for e in lock.bundle.iter().filter(|e| e.is_active() && e.source.kind == "xenia") {
        let Some((org, s, id, rev)) = xenia_bundle::parse_source_path(&e.source.path) else { continue };
        if org != set.org || s != set.store {
            continue;
        }
        let eid = e.id();
        let Some(b) = set.find(&id) else {
            report.skipped.push((eid.clone(), "the store no longer has its record".into()));
            continue;
        };
        if let Some(why) = b.hidden {
            report.skipped.push((eid.clone(), format!("the store withdrew the record ({why}); the installed version stays")));
            continue;
        }
        if b.rev < rev {
            // The store serves an older revision than this install added: a host replaying an old state. Never an update.
            report.skipped.push((eid, format!("the store serves revision {} but this install already has {} (a replay); nothing was changed", b.rev, rev)));
            continue;
        }
        let newer = b.rev > rev || b.bundle_hash != e.hash;
        if !newer {
            // The same bytes: only the standing can have changed.
            if b.level.as_str() != e.provenance {
                let (old, new) = (e.provenance.clone(), b.level.as_str().to_string());
                let (eid2, lvl) = (eid.clone(), new.clone());
                let _ = bundles::update(ws, |l| {
                    if let Some(x) = l.bundle.iter_mut().find(|x| x.id() == eid2 && x.is_active()) {
                        x.provenance = lvl.clone();
                    }
                    Ok(())
                });
                report.relevelled.push((eid, old, new));
            }
            continue;
        }
        report.available.push((eid.clone(), b.version.clone(), b.rev));
        if !auto {
            continue;
        }
        if rank(b.level.as_str()) < rank(&e.provenance) {
            report.skipped.push((eid, format!("revision {} is {} and the installed one is {}: waiting for an endorsement before updating on its own", b.rev, b.level.as_str(), e.provenance)));
            continue;
        }
        let o = AddOpts { skill_roles: Default::default(), order: None, dry_run: false, yes: false, replace_publisher: false, no_vectors: false, embed_in_background: background };
        let eid_mode = e.id();
        if fetch_and_apply(home, ws, set, b, &o, Mode::Update(&eid_mode), fetcher, host) == 0 {
            report.updated.push(eid);
        } else {
            report.skipped.push((eid, "the update did not apply (see the message above)".into()));
        }
    }
    for (id, v, r) in &report.available {
        println!("update available: {} {} (revision {}){}", id, v, r, if report.updated.contains(id) { " applied" } else { "" });
    }
    for (id, old, new) in &report.relevelled {
        println!("level: {} {} -> {}", id, old, new);
    }
    for (id, why) in &report.skipped {
        println!("not updated: {}: {}", id, why);
    }
    Ok(report)
}

// ---------------------------------------------------------------------------------------------------------------
// Publishing

/// What the operator chose; everything else comes from the signed bundle itself.
pub struct PublishArgs {
    pub dir: PathBuf,
    /// `https://host/path.git`: where the tree can be fetched.
    pub url: String,
    /// The directory of the repository the bundle is in; `None` for the root.
    pub subdir: Option<String>,
    pub pin_kind: PinKind,
    /// The full object id of the tree (or commit) the bundle is published at.
    pub pin: String,
    /// A new revision of this record, or `None` for a new record.
    pub id: Option<String>,
    /// The areas stewards endorse by; the manifest's keywords when `None`.
    pub tags: Option<Vec<String>>,
    pub role: Role,
    pub ceiling: SourceCeiling,
    pub review_by: Option<String>,
}

fn clean_tags(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t = t.trim().to_lowercase().replace([' ', '_'], "-");
        if delphi_xenia::is_slug(&t, 32) && !out.contains(&t) && out.len() < 12 {
            out.push(t);
        }
    }
    out
}

/// Publish the signed bundle at `args.dir` as a `bundle` record: build it from the bundle's own manifest, check it is signed
/// by `who`, optionally fetch the pointed-at tree and confirm it is this bundle, then hand it to the publish gate (scan, the
/// exact bytes shown, one decision, sign, push). Returns the record id and revision.
#[allow(clippy::too_many_arguments)]
pub fn publish(home: &Path, store: &Store, cfg: &VerifyConfig, who: &Identity, args: &PublishArgs, verify_tree: Option<&dyn Fetcher>, host: &dyn Host, approver: &mut dyn Approver) -> Result<(String, u64, Published), String> {
    let i = delphi_bundle::inspect(&args.dir, &Options::default(), host);
    let (Some(m), Some(hash)) = (i.manifest.clone(), i.hash.clone()) else { return Err("the bundle does not validate; nothing was published".into()) };
    if !i.report.passed() {
        return Err(format!("the bundle does not validate; nothing was published\n{}", i.report.render(&args.dir.display().to_string())));
    }
    let (org, _) = ids_of(store)?;
    if m.org != org {
        return Err(format!("the bundle is for org `{}` but this store belongs to `{}`", m.org, org));
    }
    match i.report.signer.as_deref() {
        Some(k) if k == who.did() => {}
        Some(_) => return Err("the bundle is signed by another key: publish it with the key that signed it (`delphi bundle sign` first if this is a new version)".into()),
        None => return Err("the bundle is not signed: `delphi bundle sign` it first, so that what the store points at is what you can stand behind".into()),
    }
    let tree = Pointer {
        scheme: Scheme::Git,
        locator: format!("{}#{}", args.url, args.subdir.as_deref().unwrap_or(".")),
        pin: Pin { kind: args.pin_kind, value: Some(args.pin.clone()) },
        hash: None,
        range: None,
        label: "The bundle tree".into(),
        content_type: ContentType::Other,
        bytes: None,
        access_hint: None,
    };
    check_bundle_tree(&tree).map_err(|b| format!("the tree pointer is not acceptable ({}: {})", b.field, b.why))?;
    if let Some(fetcher) = verify_tree {
        let (url, subdir) = tree_target(&tree)?;
        let staging = bundle_add::new_incoming(home)?;
        let got = (|| {
            let rev = if args.pin_kind == PinKind::GitTree { Rev::Tree(&args.pin) } else { Rev::Sha(&args.pin) };
            fetcher.fetch(&url, rev, subdir.as_deref(), &staging)?;
            let seen = delphi_bundle::inspect(&staging, &Options::default(), host);
            if !seen.report.passed() {
                return Err("the tree at that location does not validate as a bundle".to_string());
            }
            seen.hash.ok_or_else(|| "the tree at that location has no bundle hash".to_string())
        })();
        let _ = std::fs::remove_dir_all(&staging);
        let got = got.map_err(|e| format!("checking the tree you pointed at: {e}"))?;
        if got != hash {
            return Err(format!("the tree at that location is bundle {} but the one you are publishing is {}: fix the url or the pin; nothing was published", &got[7..19], &hash[7..19]));
        }
    }
    let tags = clean_tags(&args.tags.clone().unwrap_or_else(|| m.keywords.clone()));
    let draft = BundleDraft {
        id: args.id.clone(),
        name: m.name.clone(),
        version: m.version.clone(),
        bundle_hash: hash.clone(),
        tree,
        roles: m.touches_roles.iter().filter_map(|r| Role::TABLE.iter().find(|(n, _)| n == r).map(|(_, v)| *v)).collect(),
        connectors: m.touches_connectors.clone(),
        compat_delphi: m.compat_delphi.clone(),
        title: m.title.clone(),
        summary: m.description.clone(),
        tags,
        confidence: Confidence::Medium,
        source_ceiling: args.ceiling,
        author_role: args.role,
        body: String::new(),
        review_by: args.review_by.clone(),
        expires: None,
        status: AuthorStatus::Active,
    };
    if draft.roles.len() != m.touches_roles.len() {
        return Err("the bundle names a role that is not a canonical role id".into());
    }
    let publisher = Publisher::new(store, *cfg, &NoIndex);
    let (name, version) = (m.name.clone(), m.version.clone());
    let want_id = args.id.clone();
    let published = publisher
        .publish_with(who, approver, move |view| {
            match &want_id {
                Some(id) => {
                    let old = view.record(id).ok_or_else(|| delphi_core::xenia_publish::PublishError::Rule(format!("{id} is not in the store")))?;
                    let KindFields::Bundle(ob) = &old.record().kind_fields else { return Err(delphi_core::xenia_publish::PublishError::Rule(format!("{id} is not a bundle record"))) };
                    if ob.bundle_name != name {
                        return Err(delphi_core::xenia_publish::PublishError::Rule(format!("{id} is the record of `{}`; a record keeps its bundle name (publish `{}` as a new record)", ob.bundle_name, name)));
                    }
                    if let (Some(have), Some(want)) = (semver_of(&ob.bundle_version), semver_of(&version)) {
                        if want < have || (want == have && ob.bundle_hash != draft.bundle_hash) {
                            return Err(delphi_core::xenia_publish::PublishError::Rule(format!("version {want} does not go forward from {have}: bump the version (`delphi bundle sign` refuses a smaller bump) or keep the same tree")));
                        }
                    }
                }
                None => {
                    for r in &view.records {
                        if let KindFields::Bundle(b) = &r.record().kind_fields {
                            let hidden = view.computed.memories.get(&r.record().id).is_none_or(|s| s.visibility.is_hidden());
                            if b.bundle_name == name && !hidden {
                                return Err(delphi_core::xenia_publish::PublishError::Rule(format!("the store already has a record for `{}` ({}): publish a new revision of it (--id), or choose another name; names are not owned, so two records would be ambiguous", name, r.record().id)));
                            }
                        }
                    }
                }
            }
            Ok(vec![draft.into_item()])
        })
        .map_err(|e| e.to_string())?;
    let id = published.paths.iter().find_map(|p| p.strip_prefix("records/bundle/").and_then(|r| r.strip_suffix(".md"))).map(String::from).ok_or("the store accepted the push but returned no bundle path")?;
    // The revision is read back from the verified store (our push is accepted like anyone else's), not assumed.
    let _ = xenia_bundle::sync(store, cfg);
    let set = BundleSet::offline(store, cfg).ok();
    let rev = set.as_ref().and_then(|s| s.find(&id)).map(|b| b.rev).unwrap_or(1);
    Ok((id, rev, published))
}

/// The approver for a bundle publish: the same card and the same typed code as every other `xenia` command (X9), so that what
/// the operator is shown and what they type back is one thing. A terminal on both ends is required.
pub(crate) struct CardTty;

impl Approver for CardTty {
    fn decide(&mut self, p: &Preview) -> Decision {
        println!("{}", delphi_core::xenia_action::card("Publish a bundle", p));
        crate::xenia_cmd::TtyApprover.decide(p)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The command line

fn open_named(home: &Path, which: &str) -> Result<Store, String> {
    let (org, store) = which.split_once('/').ok_or("name the store as <org>/<store>")?;
    xenia_bundle::open_joined(home, org, store)
}

/// `delphi bundle add <record id> --from-xenia <org>/<store>`.
pub fn cli_add(home: &Path, ws: &Path, id: &str, args: &[String]) -> i32 {
    let flag_set = |f: &str| args.iter().any(|a| a == f);
    let which = crate::bundle_cmd::flag(args, "--from-xenia").unwrap_or_default();
    let store = match open_named(home, &which) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let o = AddOpts { skill_roles: Default::default(), order: crate::bundle_cmd::flag(args, "--order").and_then(|o| o.parse().ok()), dry_run: flag_set("--dry-run"), yes: flag_set("--yes"), replace_publisher: flag_set("--replace-publisher"), no_vectors: flag_set("--no-vectors"), embed_in_background: true };
    add(home, ws, &store, &cfg_now(), id, &o, &crate::bundle_fetch::real_fetcher(home), &crate::bundle_cmd::Real)
}

/// `delphi bundle store <org>/<store> [--auto-update]`.
pub fn cli_store(home: &Path, ws: &Path, which: &str, args: &[String]) -> i32 {
    let store = match open_named(home, which) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let cfg = cfg_now();
    if args.iter().any(|a| a == "--auto-update") {
        return match sync_bundles(home, ws, &store, &cfg, true, true, &crate::bundle_fetch::real_fetcher(home), &crate::bundle_cmd::Real) {
            Ok(_) => 0,
            Err(e) => fail(e),
        };
    }
    list(&store, &cfg, ws)
}

/// `delphi bundle publish <dir> --to <org>/<store> --url <https url> --pin <git-tree|git-commit>:<sha> ...`.
pub fn cli_publish(dir: &Path, args: &[String], host: &dyn Host) -> i32 {
    let flag = |n: &str| crate::bundle_cmd::flag(args, n);
    let home = crate::home::delphi_home(flag("--home").as_deref());
    let Some(to) = flag("--to") else { return fail("publish needs --to <org>/<store>") };
    let store = match open_named(&home, &to) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    publish_cli(&home, &store, &to, dir, &flag, args.iter().any(|x| x == "--no-verify-tree"), host, &mut CardTty)
}

/// The publish command shared by `delphi bundle publish` and `delphi xenia publish --bundle`: `get` reads an option by its
/// `--name`; the approver is the caller's.
#[allow(clippy::too_many_arguments)]
pub fn publish_cli(home: &Path, store: &Store, store_name: &str, dir: &Path, get: &dyn Fn(&str) -> Option<String>, no_verify_tree: bool, host: &dyn Host, approver: &mut dyn Approver) -> i32 {
    let (Some(url), Some(pin)) = (get("--url"), get("--pin")) else { return fail("publishing a bundle needs --url <https git url> and --pin <git-tree|git-commit>:<sha>") };
    let Some((kind, sha)) = pin.split_once(':') else { return fail("--pin wants git-tree:<sha> or git-commit:<sha>") };
    let pin_kind = match kind {
        "git-tree" => PinKind::GitTree,
        "git-commit" => PinKind::GitCommit,
        _ => return fail("--pin wants git-tree:<sha> or git-commit:<sha>"),
    };
    let who = match Identity::load(&crate::xenia_cmd::key_path(home)) {
        Ok(i) => i,
        Err(e) => return fail(format!("this install has no xenia key ({e}); run `delphi xenia whoami`")),
    };
    let a = PublishArgs {
        dir: dir.to_path_buf(),
        url,
        subdir: get("--subdir"),
        pin_kind,
        pin: sha.to_string(),
        id: get("--id"),
        tags: get("--tags").map(|t| t.split(',').map(String::from).collect()),
        role: Role::Operator,
        ceiling: SourceCeiling::InternalGenerated,
        review_by: get("--review-by"),
    };
    if let Err(e) = crate::bundle_licence::gate_dir(dir, host, &crate::bundle_licence::tty_confirm) {
        return fail(e);
    }
    let fetcher = crate::bundle_fetch::real_fetcher(home);
    let verify: Option<&dyn Fetcher> = if no_verify_tree { None } else { Some(&fetcher) };
    match publish(home, store, &cfg_now(), &who, &a, verify, host, approver) {
        Ok((id, rev, p)) => {
            println!("published {} revision {} ({} attempt{}){}", id, rev, p.attempts, if p.attempts == 1 { "" } else { "s" }, if p.already_there { ": the store already held exactly these bytes" } else { "" });
            println!("a steward endorses it with `delphi xenia endorse {}`; members add it with `delphi bundle add {} --from-xenia {}`", id, id, store_name);
            0
        }
        Err(e) => fail(e),
    }
}

#[cfg(test)]
#[path = "bundle_xenia_tests.rs"]
mod tests;
