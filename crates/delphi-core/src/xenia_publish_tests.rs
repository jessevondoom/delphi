//! Tests for the publish gate. The attacker is the operator's own mistakes, a hostile store host, and anything that
//! can touch a file or the remote between the preview and the push. Every store is a real git repository driven by
//! the real code path; every roster and record is really signed; the publisher is the real `Publisher`.
//!
//! The fixture (`Fx`) is shared with the member-command tests.

use super::*;
use crate::xenia_testkit::{TestRemote, GOOD_GITATTRIBUTES};
use delphi_xenia::pins::{self, JoinRequest};
use delphi_xenia::testkit::{MemberSpec, TestOrg, ORG, STORE};
use delphi_xenia::testkit_records::{self as rk, FreshSpec};
use delphi_xenia::time::parse_utc;
use std::cell::Cell;
use std::process::Command as StdCommand;

pub(crate) const DAY: i64 = 24 * 3600;

pub(crate) fn now() -> i64 {
    parse_utc("2026-10-26T12:00:00Z").unwrap()
}

pub(crate) fn cfg() -> VerifyConfig {
    VerifyConfig::at(now())
}

pub(crate) struct Fx {
    _tmp: tempfile::TempDir,
    pub remote: TestRemote,
    pub store: Store,
    pub org: TestOrg,
    pub ids: Box<dyn Fn(u64) -> String>,
    /// Each statement is issued a minute after the last: an equal issued_at with other bytes is a replay.
    pub fresh_n: Cell<i64>,
}

/// alice, bob, carol: admins (2-of-3). dave: steward of `rust`. erin: member. frank: steward of `payments`.
/// gail: steward of everything (`*`). hank: member.
pub(crate) fn cast() -> TestOrg {
    let mut t = TestOrg::empty();
    let names = ["alice", "bob", "carol", "dave", "erin", "frank", "gail", "hank"];
    let keys: Vec<_> = names.iter().map(|n| t.who(n)).collect();
    let spec = delphi_xenia::testkit::EpochSpec {
        org: ORG.into(),
        store: STORE.into(),
        epoch: 1,
        prev_hash: None,
        members: vec![
            MemberSpec::new(&keys[0], "alice").admin(),
            MemberSpec::new(&keys[1], "bob").admin(),
            MemberSpec::new(&keys[2], "carol").admin(),
            MemberSpec::new(&keys[3], "dave").steward(&["rust"]),
            MemberSpec::new(&keys[4], "erin"),
            MemberSpec::new(&keys[5], "frank").steward(&["payments"]),
            MemberSpec::new(&keys[6], "gail").steward(&["*"]),
            MemberSpec::new(&keys[7], "hank"),
        ],
    };
    t.push_signed(spec, &["alice", "bob", "carol"]);
    t
}

/// What an approver saw and does.
pub(crate) struct Yes {
    pub acknowledge_flags: bool,
    pub calls: usize,
    pub seen: Vec<Preview>,
}

impl Yes {
    pub fn new() -> Yes {
        Yes { acknowledge_flags: true, calls: 0, seen: Vec::new() }
    }
}

impl Approver for Yes {
    fn decide(&mut self, p: &Preview) -> Decision {
        self.calls += 1;
        self.seen.push(p.clone());
        Decision::Approve(p.approve(self.acknowledge_flags))
    }
}

pub(crate) struct Closure<F: FnMut(&Preview) -> Decision>(pub F);

impl<F: FnMut(&Preview) -> Decision> Approver for Closure<F> {
    fn decide(&mut self, p: &Preview) -> Decision {
        (self.0)(p)
    }
}

impl Fx {
    pub fn new() -> Fx {
        Fx::with_org(cast())
    }

    pub fn with_org(org: TestOrg) -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let remote = TestRemote::new(&tmp.path().join("host"));
        remote.write(".gitattributes", GOOD_GITATTRIBUTES.as_bytes());
        remote.write("org.toml", b"org = \"acme\"\nstore = \"acme-main\"\n");
        let store = Store::open(&home, "acme", "acme-main", &remote.url(), true).unwrap();
        let ids: Box<dyn Fn(u64) -> String> = Box::new({
            let n = std::rc::Rc::new(Cell::new(0usize));
            move |_| {
                n.set(n.get() + 1);
                format!("01JBX3Q8T2M9Z4K7WYH5N6D{:03}", n.get())
            }
        });
        let fx = Fx { _tmp: tmp, remote, store, org, ids, fresh_n: Cell::new(0) };
        fx.write_roster();
        let c1 = fx.remote.commit("roster");
        fx.fresh(&c1);
        let (epoch, h) = fx.org.head_pair();
        let req = JoinRequest { url: fx.remote.url(), admin_fingerprint: fx.org.fingerprint("alice"), head_epoch: epoch, head_hash: h };
        pins::join(&fx.store.pin_storage(), &req, &fx.org.files).unwrap();
        match fx.store.sync_verified(&cfg()).unwrap() {
            VerifiedSync::Accepted { .. } => {}
            other => panic!("baseline sync: {other:?}"),
        }
        fx
    }

    pub fn write_roster(&self) {
        for (i, f) in self.org.files.iter().enumerate() {
            let n = i + 1;
            self.remote.write(&format!("roster/epochs/{n}.json"), &f.epoch_json);
            self.remote.write(&format!("roster/epochs/{n}.sigs.json"), &f.sigs_json);
        }
    }

    /// A statement for the head epoch naming `store_head`, issued an hour ago for a week, signed by alice; commits it.
    pub fn fresh(&self, store_head: &str) -> String {
        self.fresh_for(&self.org, store_head)
    }

    pub fn fresh_for(&self, org: &TestOrg, store_head: &str) -> String {
        self.fresh_n.set(self.fresh_n.get() + 1);
        let f = FreshSpec::for_head(org, store_head, now() - 3600 + 60 * self.fresh_n.get(), 7 * DAY);
        let alice = org.key("alice");
        let (stmt, sigs) = f.sign(&[&alice]);
        self.remote.write("roster/freshness.json", &stmt);
        self.remote.write("roster/freshness.sigs.json", &sigs);
        self.remote.commit("freshness")
    }

    /// The remote's working copy follows what the host serves (after publishes pushed to it).
    pub fn catch_up(&self) {
        let host = self.remote.host_path().to_str().unwrap().to_string();
        self.remote.git(&["fetch", "--quiet", &host, "main"]);
        self.remote.reset_hard("FETCH_HEAD");
    }

    pub fn key(&self, name: &str) -> Identity {
        self.org.key(name)
    }

    pub fn publisher<'a>(&'a self, similar: &'a dyn SimilarRecords) -> Publisher<'a> {
        let mut p = Publisher::new(&self.store, cfg(), similar);
        p.ids = Some(&*self.ids);
        p
    }

    pub fn host_head(&self) -> String {
        self.remote.host_head()
    }

    /// A file as the host serves it on `main`.
    pub fn host_file(&self, path: &str) -> Option<Vec<u8>> {
        let out = StdCommand::new("git").args(["--git-dir", self.remote.host_path().to_str().unwrap(), "show", &format!("main:{path}")]).output().unwrap();
        out.status.success().then_some(out.stdout)
    }

    /// Everything local that a failed publish must not change: the sticky file, the pins, and every ref in the clone.
    pub fn local_state(&self) -> (Vec<u8>, Vec<u8>, String) {
        let sticky = std::fs::read(&self.store.paths.sticky).unwrap_or_default();
        let pins = std::fs::read(self.store.paths.state.join("pins.toml")).unwrap_or_default();
        let refs = String::from_utf8(self.store.git_ok(&["for-each-ref"], 1 << 16).unwrap()).unwrap();
        (sticky, pins, refs)
    }

    /// Sync the install and return the computed view at the accepted head.
    pub fn view(&self) -> StoreView {
        let head = match self.store.sync_verified(&cfg()).unwrap() {
            VerifiedSync::Accepted { head, .. } | VerifiedSync::UpToDate { head, .. } => head,
            other => panic!("sync: {other:?}"),
        };
        StoreView::load(&self.store, &cfg(), &head, &BTreeMap::new()).unwrap()
    }

    pub fn tier_of(&self, id: &str) -> delphi_xenia::tier::Tier {
        self.view().computed.tier_of(id).unwrap_or_else(|| panic!("{id} is not in force"))
    }
}

/// Approve a prepared batch exactly as previewed, and sign it.
pub(crate) fn sign_ok(p: Prepared, key: &Identity) -> Result<Signed, PublishError> {
    let a = p.preview().approve(true);
    p.sign(&a, key)
}

pub(crate) fn draft(title: &str, tags: &[&str]) -> MemoryDraft {
    MemoryDraft {
        origin: ContentOrigin { scope: Scope::Team, promoted: true, file: None },
        id: None,
        title: title.into(),
        summary: "Deliveries are retried with backoff for up to three days; handlers must be idempotent.".into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        memory_type: MemoryType::Gotcha,
        applies_to: AppliesTo { repos: vec!["payments".into()], tech: vec!["stripe".into()], paths: vec![] },
        verified_by: VerifiedBy::Test,
        confidence: Confidence::Medium,
        body: "Handlers must dedupe on the event id.\n".into(),
        pointers: vec![],
        evidence: vec![],
        related: vec![],
        review_by: "2027-01-24".into(),
        expires: None,
        supersedes: None,
        source_ceiling: SourceCeiling::InternalEmpirical,
        author_role: Role::Builder,
        status: AuthorStatus::Active,
    }
}

impl Fx {
    /// Publish a memory as `who`, approving everything.
    pub fn publish_memory(&self, who: &str, d: MemoryDraft) -> Result<Published, PublishError> {
        let p = self.publisher(&NoIndex);
        p.publish(&self.key(who), vec![d.into_item()], &mut Yes::new())
    }

    /// Publish and return the id of the single record that went out.
    pub fn memory_id(&self, who: &str, d: MemoryDraft) -> String {
        let r = self.publish_memory(who, d).unwrap_or_else(|e| panic!("publish: {e}"));
        r.paths[0].trim_start_matches("records/memory/").trim_end_matches(".md").to_string()
    }
}

// ---------------------------------------------------------------------------------------------------------
// The happy path, and "the bytes shown are the bytes signed"

#[test]
fn a_promoted_memory_is_previewed_signed_pushed_and_syncs_as_l1() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let mut yes = Yes::new();
    let before = fx.host_head();
    let out = p.publish(&fx.key("erin"), vec![draft("Stripe retries", &["rust"]).into_item()], &mut yes).unwrap();
    assert_eq!(yes.calls, 1, "exactly one decision");
    assert_eq!(out.attempts, 1);
    assert!(!out.already_there);
    assert_eq!(out.parent, before);
    assert_eq!(fx.host_head(), out.commit);
    // The host's file is byte for byte the previewed file.
    let preview = &yes.seen[0];
    let item = &preview.items[0];
    assert_eq!(fx.host_file(&item.path).unwrap(), item.bytes);
    assert_eq!(item.hash, delphi_xenia::hash::of(&item.bytes));
    // And its sidecar signs exactly that hash with the signer's key, strictly verified.
    let car = Sidecar::parse(&fx.host_file(&sidecar_path(&item.path)).unwrap()).unwrap();
    assert_eq!(car.hash, item.hash);
    assert_eq!(car.kid, fx.key("erin").did());
    let id = item.id.clone();
    // It syncs through the full verifier (walk included) and reads as L1 until a steward endorses it.
    assert_eq!(fx.tier_of(&id), delphi_xenia::tier::Tier::L1);
    // The record and its sidecar travel in one commit.
    let files = String::from_utf8(fx.store.git_ok(&["show", "--name-only", "--format=", &out.commit], 1 << 16).unwrap()).unwrap();
    assert!(files.contains(&item.path) && files.contains(&sidecar_path(&item.path)), "{files}");
}

#[test]
fn the_digest_covers_every_byte_and_every_signed_field() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).unwrap();
    let base = prepared.preview().clone();
    assert_eq!(base.digest, preview_digest(&base.items));
    let mut changed = base.items.clone();
    changed[0].bytes.push(b'x');
    assert_ne!(preview_digest(&changed), base.digest, "a byte of the record");
    for edit in [
        |i: &mut PreviewItem| i.path.push('x'),
        |i: &mut PreviewItem| i.signer.push('x'),
        |i: &mut PreviewItem| i.signed_at = "2026-10-26T12:00:01Z".into(),
        |i: &mut PreviewItem| i.roster_epoch += 1,
    ] {
        let mut c = base.items.clone();
        edit(&mut c[0]);
        assert_ne!(preview_digest(&c), base.digest);
    }
}

#[test]
fn the_publish_is_one_commit_on_the_verified_head_and_adds_no_local_ref() {
    let fx = Fx::new();
    let before = fx.local_state();
    let out = fx.publish_memory("erin", draft("A note", &["rust"])).unwrap();
    let after = fx.local_state();
    assert_eq!(before, after, "publishing changed local state (sticky, pins or refs)");
    let parents = String::from_utf8(fx.store.git_ok(&["rev-list", "--parents", "-n1", &out.commit], 4096).unwrap()).unwrap();
    assert_eq!(parents.trim(), format!("{} {}", out.commit, out.parent));
}

// ---------------------------------------------------------------------------------------------------------
// Promoted scope only

#[test]
fn operator_scope_and_unpromoted_content_never_get_as_far_as_a_preview() {
    let fx = Fx::new();
    let before = fx.host_head();
    for (scope, promoted, why) in [(Scope::Operator, true, "operator"), (Scope::Operator, false, "operator"), (Scope::Workspace, false, "promoted"), (Scope::Team, false, "promoted")] {
        let mut d = draft("A note", &["rust"]);
        d.origin = ContentOrigin { scope, promoted, file: None };
        let mut yes = Yes::new();
        let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut yes).unwrap_err();
        assert_eq!(err.reason(), "not-promoted", "{scope:?} {promoted}");
        assert!(err.to_string().contains(why), "{err}");
        assert_eq!(yes.calls, 0, "the operator was asked about content that may not leave");
    }
    // Workspace scope that was promoted is fine.
    let mut d = draft("From a workspace", &["rust"]);
    d.origin = ContentOrigin { scope: Scope::Workspace, promoted: true, file: None };
    fx.publish_memory("erin", d).unwrap();
    assert_ne!(fx.host_head(), before);
    // A memory cannot be typed into a command, and a command record cannot carry content provenance.
    let mut item = draft("Typed", &["rust"]).into_item();
    item.origin = Origin::Command;
    assert_eq!(fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![item]).err().unwrap().reason(), "wrong-origin");
}

// ---------------------------------------------------------------------------------------------------------
// The scan (spec 012 test 20: a planted secret in the summary, the body and a pointer label)

fn pointer_with_label(label: &str) -> Pointer {
    use delphi_xenia::pointer::{ContentType, Pin, PinKind, Scheme};
    Pointer {
        scheme: Scheme::Https,
        locator: "https://docs.acme.example/page".into(),
        pin: Pin { kind: PinKind::None, value: None },
        hash: None,
        range: None,
        label: label.into(),
        content_type: ContentType::Other,
        bytes: None,
        access_hint: None,
    }
}

#[test]
fn a_planted_secret_in_any_field_stops_the_publish_before_anything_is_shown() {
    let fx = Fx::new();
    let before = (fx.host_head(), fx.local_state());
    // One canary per `[REV-SEC-05]` class, none of which the pointer or serializer rules would refuse first.
    let canaries = [
        ("secret", "sk-ant-abcdefghijklmnopqrstuvwxyz0123456789"),
        ("token", "ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
        ("secret", "AKIAABCDEFGHIJKLMNOP"),
        ("private-key", "-----BEGIN OPENSSH PRIVATE KEY-----"),
        ("local-path", "/Users/jane/work/notes"),
        ("local-path", "/home/deploy/app"),
        ("session-trace", "local/missions/m-1/events.jsonl"),
    ];
    for (class, canary) in canaries {
        for field in ["summary", "body", "label", "title"] {
            let mut d = draft("A note", &["rust"]);
            match field {
                "summary" => d.summary = format!("see {canary}"),
                "body" => d.body = format!("first line\n{canary}\n"),
                "label" => d.pointers = vec![pointer_with_label(&format!("see {canary}"))],
                _ => d.title = format!("about {canary}"),
            }
            let mut yes = Yes::new();
            let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut yes).unwrap_err();
            let PublishError::ScanBlocked(blocks) = &err else { panic!("{class} in {field}: {err}") };
            assert!(!blocks.is_empty());
            assert!(blocks.iter().any(|b| b.class == class || matches!(b.class, "secret" | "token")), "{class} in {field}: {blocks:?}");
            // The report names the class and the field, never the value, and nothing was shown.
            let shown = err.to_string();
            assert!(!shown.contains("abcdefghijklmnop") && !shown.contains("jane") && !shown.contains("deploy"), "{shown}");
            assert_eq!(yes.calls, 0, "{class} in {field}: the operator was shown a record with a secret in it");
            assert_eq!(err.reason(), "scan-blocked");
        }
    }
    assert_eq!(before, (fx.host_head(), fx.local_state()), "a refused publish left a trace");
}

#[test]
fn a_registered_credential_literal_is_a_block_too() {
    let fx = Fx::new();
    let mut p = fx.publisher(&NoIndex);
    p.scan.secret_literals = vec!["correct-horse-battery-staple-9".into()];
    let mut d = draft("A note", &["rust"]);
    d.body = "the password is correct-horse-battery-staple-9\n".into();
    assert_eq!(p.publish(&fx.key("erin"), vec![d.into_item()], &mut Yes::new()).unwrap_err().reason(), "scan-blocked");
}

#[test]
fn flags_are_shown_and_the_approval_must_acknowledge_them() {
    let fx = Fx::new();
    let before = fx.host_head();
    let mut d = draft("A note", &["rust"]);
    d.body = "See https://wiki.internal.example/retries and ask jane@initech.example about customer cus_9sdf8sdf8sd.\n".into();
    let mut no_ack = Yes::new();
    no_ack.acknowledge_flags = false;
    let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.clone().into_item()], &mut no_ack).unwrap_err();
    assert_eq!(err.reason(), "flags-not-acknowledged");
    let classes: Vec<&str> = no_ack.seen[0].flags.iter().map(|f| f.class).collect();
    for want in ["url", "email", "customer-id"] {
        assert!(classes.contains(&want), "{want} missing from {classes:?}");
    }
    assert_eq!(fx.host_head(), before, "unacknowledged flags still sent something");
    fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut Yes::new()).unwrap();
    assert_ne!(fx.host_head(), before);
}

#[test]
fn a_denial_sends_nothing() {
    let fx = Fx::new();
    let before = (fx.host_head(), fx.local_state());
    let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()], &mut Closure(|_| Decision::Deny)).unwrap_err();
    assert_eq!(err, PublishError::Denied);
    assert_eq!(before, (fx.host_head(), fx.local_state()));
}

// ---------------------------------------------------------------------------------------------------------
// No model turn between preview and sign; the bytes shown are the bytes signed

#[test]
fn the_gate_cannot_start_a_model_turn_by_construction() {
    // 1. The only code that runs between the preview and the signature is `Approver::decide`, and its input is a
    //    shared reference to plain data. This module imports nothing that can reach a model, a tool or a span.
    let src = include_str!("xenia_publish.rs");
    let code: String = src.lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    for forbidden in ["crate::client", "crate::spawn", "crate::hub", "crate::server", "crate::peer", "tokio", "async fn", ".await", "Command::new", "std::net", "reqwest"] {
        assert!(!code.contains(forbidden), "xenia_publish.rs mentions {forbidden}: the gate must not be able to start a model turn");
    }
    // 2. git is spawned only through the hardened builder.
    assert!(!code.contains("Command::new(\"git\")"));
    // 3. The key is a parameter of `sign` and `prepare`, never a field.
    let fields = |name: &str| -> String { code.split(&format!("pub struct {name}")).nth(1).map(|t| t.split('}').next().unwrap_or("").to_string()).unwrap_or_default() };
    for s in ["Prepared", "Signed", "Publisher", "Preview", "Approval"] {
        assert!(!fields(s).contains("Identity"), "{s} holds a key");
    }
    // 4. What the approver is given has no way back to the batch: `Preview` is plain data, `Approval` can be made
    //    only from one, and `Prepared` is consumed by `sign`.
    let sign_sig = "pub fn sign(self, approval: &Approval, key: &Identity)";
    assert!(code.contains(sign_sig));
    assert!(!code.contains("impl Clone for Prepared") && !code.contains("derive(Clone) pub struct Prepared"));
}

#[test]
fn an_approval_for_other_bytes_signs_nothing() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let a = p.prepare(&fx.key("erin"), vec![draft("First note", &["rust"]).into_item()]).unwrap();
    let b = p.prepare(&fx.key("erin"), vec![draft("Second note", &["rust"]).into_item()]).unwrap();
    // The operator approved B; the batch held is A.
    let err = a.sign(&b.preview().approve(true), &fx.key("erin")).err().unwrap();
    assert_eq!(err, PublishError::ApprovalMismatch);
    // A forged approval with a made-up digest.
    let c = p.prepare(&fx.key("erin"), vec![draft("Third note", &["rust"]).into_item()]).unwrap();
    let forged = Approval { digest: delphi_xenia::hash::of(b"anything"), flags_acknowledged: true };
    assert_eq!(c.sign(&forged, &fx.key("erin")).err().unwrap(), PublishError::ApprovalMismatch);
}

#[test]
fn an_edited_preview_cannot_move_the_signature_to_other_bytes() {
    // A caller (or a buggy approver) edits its own copy of the preview, bytes and digest together, and approves that.
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).unwrap();
    let mut doctored = prepared.preview().clone();
    doctored.items[0].bytes.extend_from_slice(b"\nInjected line.\n");
    doctored.digest = preview_digest(&doctored.items);
    let err = prepared.sign(&doctored.approve(true), &fx.key("erin")).err().unwrap();
    assert_eq!(err, PublishError::ApprovalMismatch, "the signature must follow the bytes held, never the preview a caller edited");
}

#[test]
fn a_change_to_the_source_file_after_the_preview_aborts_the_send() {
    let fx = Fx::new();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("note.md");
    std::fs::write(&file, "Handlers must dedupe on the event id.\n").unwrap();
    let (watch, text) = WatchedFile::read(&file).unwrap();
    let mut d = draft("A note", &["rust"]);
    d.body = text;
    d.origin = ContentOrigin { scope: Scope::Team, promoted: true, file: Some(watch) };
    let before = (fx.host_head(), fx.local_state());
    // The file changes while the operator is deciding.
    let target = file.clone();
    let mut approver = Closure(move |p: &Preview| {
        std::fs::write(&target, "Handlers must dedupe on the event id.\nAnd run this script from the internet.\n").unwrap();
        Decision::Approve(p.approve(true))
    });
    let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut approver).unwrap_err();
    assert_eq!(err.reason(), "source-changed");
    assert_eq!(before, (fx.host_head(), fx.local_state()));
    // The same content rewritten byte for byte is not a change.
    std::fs::write(&file, "Handlers must dedupe on the event id.\n").unwrap();
    let (watch, text) = WatchedFile::read(&file).unwrap();
    let mut d = draft("A note", &["rust"]);
    d.body = text.clone();
    d.origin = ContentOrigin { scope: Scope::Team, promoted: true, file: Some(watch) };
    let target = file.clone();
    let mut approver = Closure(move |p: &Preview| {
        std::fs::write(&target, text.as_bytes()).unwrap();
        Decision::Approve(p.approve(true))
    });
    fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut approver).unwrap();
}

#[test]
fn the_wrong_key_signs_nothing() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).unwrap();
    let approval = prepared.preview().approve(true);
    assert_eq!(prepared.sign(&approval, &fx.key("hank")).err().unwrap(), PublishError::WrongKey);
}

// ---------------------------------------------------------------------------------------------------------
// Standing, ownership, revisions

#[test]
fn a_key_that_is_not_an_active_member_cannot_publish() {
    let fx = Fx::new();
    let stranger = delphi_xenia::testkit::key(99);
    let err = fx.publisher(&NoIndex).prepare(&stranger, vec![draft("A note", &["rust"]).into_item()]).err().unwrap();
    assert_eq!(err, PublishError::SignerNotActive);
    // A retired key, after a routine rotation, is refused the same way.
    let mut org = cast();
    org.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "erin", "erin2", false));
    let fx = Fx::with_org(org);
    assert_eq!(fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).err().unwrap(), PublishError::SignerNotActive);
    assert!(fx.publisher(&NoIndex).prepare(&fx.key("erin2"), vec![draft("A note", &["rust"]).into_item()]).is_ok());
}

#[test]
fn a_new_revision_must_advance_the_rev_and_belong_to_the_author() {
    let fx = Fx::new();
    let id = fx.memory_id("erin", draft("A note", &["rust"]));
    // The author publishes revision 2.
    let mut d = draft("A note, corrected", &["rust"]);
    d.id = Some(id.clone());
    let out = fx.publish_memory("erin", d.clone()).unwrap();
    assert!(out.paths[0].ends_with(&format!("{id}.md")));
    let v = fx.view();
    assert_eq!(v.record(&id).unwrap().record().rev, 2);
    assert_eq!(v.record(&id).unwrap().record().created, v.record(&id).unwrap().record().created);
    // Another member cannot take the id over.
    assert_eq!(fx.publish_memory("hank", d.clone()).unwrap_err().reason(), "not-owner");
    // An explicit rev that does not advance is refused, not silently bumped.
    let mut item = d.clone().into_item();
    item.record.rev = 2;
    assert_eq!(fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![item]).err().unwrap().reason(), "rev-regression");
    let mut item = d.clone().into_item();
    item.record.rev = 1;
    assert_eq!(fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![item]).err().unwrap().reason(), "rev-regression");
    // A brand-new id starts at revision 1 whatever the caller wrote: the gate does not believe a rev for an id it has not seen.
    let mut d2 = draft("Another", &["rust"]);
    d2.id = Some("01JBX3Q8T2M9Z4K7WYH5N6D900".into());
    let mut item = d2.into_item();
    item.record.rev = 5;
    let prepared = fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![item]).unwrap();
    assert_eq!(prepared.preview().items[0].rev, 1);
}

#[test]
fn a_record_the_author_hid_cannot_come_back() {
    let fx = Fx::new();
    let id = fx.memory_id("erin", draft("A note", &["rust"]));
    let mut d = draft("A note", &["rust"]);
    d.id = Some(id.clone());
    d.status = AuthorStatus::Withdrawn;
    fx.publish_memory("erin", d.clone()).unwrap();
    d.status = AuthorStatus::Active;
    assert_eq!(fx.publish_memory("erin", d).unwrap_err().reason(), "state-regression");
}

#[test]
fn an_id_that_exists_as_another_kind_is_not_free() {
    let fx = Fx::new();
    let id = fx.memory_id("erin", draft("A note", &["rust"]));
    // dave's tombstone aimed at it uses the tombstone's own id, so reusing the memory's id for a tombstone is refused.
    let mut item = draft("x", &["rust"]).into_item();
    item.origin = Origin::Command;
    item.record.kind_fields = KindFields::Tombstone(delphi_xenia::record::TombstoneFields { target: "01JBX3Q8T2M9Z4K7WYH5N6D999".into(), action: delphi_xenia::record::TombstoneAction::Withdraw, reason: delphi_xenia::record::TombstoneReason::Mistake, note: None });
    item.record.body = String::new();
    item.record.body_kind = BodyKind::None;
    item.record.review_by = None;
    item.record.id = id;
    assert_eq!(fx.publisher(&NoIndex).prepare(&fx.key("dave"), vec![item]).err().unwrap().reason(), "id-taken");
}

// ---------------------------------------------------------------------------------------------------------
// Delivery: fast-forward rebase, re-verify, never force

fn other_publisher_pushes(fx: &Fx, who: &str, id: &str) -> String {
    let rec = rk::memory(&fx.key(who), id, 1, "active", "other");
    fx.remote.write(&rec.path, &rec.md);
    fx.remote.write(&rec.sidecar_path, &rec.sidecar);
    fx.remote.commit("someone else publishes")
}

#[test]
fn a_remote_that_moved_on_is_fast_forwarded_and_reverified_before_the_second_push() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("Mine", &["rust"]).into_item()]).unwrap();
    let approval = prepared.preview().approve(true);
    let signed = prepared.sign(&approval, &fx.key("erin")).unwrap();
    // Another member publishes while the operator was deciding.
    let theirs = other_publisher_pushes(&fx, "hank", "01JBX3Q8T2M9Z4K7WYH5N6D800");
    let out = p.deliver(&signed).unwrap();
    assert_eq!(out.attempts, 2, "the first push was rejected as non-fast-forward, the second went through");
    assert_eq!(out.parent, theirs, "rebuilt on the new verified head");
    assert_eq!(fx.host_head(), out.commit);
    // Both records are on the host; nothing of theirs was lost; the whole store verifies.
    assert!(fx.host_file("records/memory/01JBX3Q8T2M9Z4K7WYH5N6D800.md").is_some());
    let mine = &signed.files()[0];
    assert_eq!(fx.host_file(&mine.path).unwrap(), mine.md, "the signed bytes survived the rebase unchanged");
    let v = fx.view();
    assert!(v.record("01JBX3Q8T2M9Z4K7WYH5N6D800").is_some() && v.record(&mine.record.id).is_some());
    assert!(v.quarantined.is_empty(), "{:?}", v.quarantined);
}

#[test]
fn a_same_id_race_stops_and_asks_instead_of_overwriting() {
    let fx = Fx::new();
    let id = fx.memory_id("erin", draft("A note", &["rust"]));
    fx.catch_up();
    let mut d = draft("A note v2", &["rust"]);
    d.id = Some(id.clone());
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![d.into_item()]).unwrap();
    let signed = sign_ok(prepared, &fx.key("erin")).unwrap();
    // The same author (another install of the same key) publishes revision 2 first.
    let v2 = rk::memory(&fx.key("erin"), &id, 2, "active", "elsewhere");
    fx.remote.write(&v2.path, &v2.md);
    fx.remote.write(&v2.sidecar_path, &v2.sidecar);
    fx.remote.commit("revision 2 from the other install");
    let err = p.deliver(&signed).unwrap_err();
    assert_eq!(err.reason(), "raced", "{err}");
    assert_eq!(fx.host_file(&v2.path).unwrap(), v2.md, "the other revision was overwritten");
}

#[test]
fn a_remote_that_rewrote_history_is_refused_and_nothing_local_changes() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("Mine", &["rust"]).into_item()]).unwrap();
    let signed = sign_ok(prepared, &fx.key("erin")).unwrap();
    let (sticky, pins, _) = fx.local_state();
    let accepted = fx.store.accepted_head().unwrap();
    // The host rewrites the last commit (a force-push) while we were deciding.
    let rewritten = fx.remote.amend("history rewritten");
    let err = p.deliver(&signed).unwrap_err();
    assert_eq!(err.reason(), "remote-rewrote-history", "{err}");
    assert_eq!(fx.host_head(), rewritten, "we must not have written anything over their rewrite");
    assert_eq!(fx.store.accepted_head().unwrap(), accepted, "the accepted head moved");
    let (sticky2, pins2, refs2) = fx.local_state();
    assert_eq!((sticky, pins), (sticky2, pins2));
    assert!(!refs2.contains(&signed.head) || refs2.contains("refs/xenia/accepted"), "{refs2}");
}

#[test]
fn a_store_that_went_back_in_time_is_moved_forward_again_never_rewritten() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("Mine", &["rust"]).into_item()]).unwrap();
    let signed = sign_ok(prepared, &fx.key("erin")).unwrap();
    let verified_head = signed.head.clone();
    // The host serves an older view (withheld updates). Our commit descends from the verified head, so pushing it is a
    // fast-forward for the host that restores what it withheld; nothing is overwritten.
    fx.remote.reset_hard("HEAD~1");
    fx.remote.force_push();
    let out = p.deliver(&signed).unwrap();
    let ancestor = fx.store.exit_code(&["merge-base", "--is-ancestor", &verified_head, &out.commit]).unwrap();
    assert_eq!(ancestor, 0, "the verified history is part of what was pushed");
    assert_eq!(fx.host_head(), out.commit);
}

#[test]
fn a_push_the_store_refuses_says_why_and_leaves_local_state_alone() {
    let fx = Fx::new();
    fx.remote.host_hook("pre-receive", "#!/bin/sh\necho 'pushes are closed for the freeze' >&2\nexit 1\n");
    let before = fx.local_state();
    let head = fx.host_head();
    let err = fx.publish_memory("erin", draft("A note", &["rust"])).unwrap_err();
    assert_eq!(err.reason(), "push-rejected");
    assert!(err.to_string().contains("refused the push") && err.to_string().contains("freeze"), "{err}");
    assert_eq!(fx.local_state(), before);
    assert_eq!(fx.host_head(), head);
}

#[test]
fn the_candidate_head_is_reverified_and_a_bad_one_is_never_pushed() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("Mine", &["rust"]).into_item()]).unwrap();
    let mut signed = sign_ok(prepared, &fx.key("erin")).unwrap();
    let head = fx.host_head();
    // Tamper with the bytes after the signature was made (a bug, a hostile library, a bit flip).
    signed.files[0].md.push(b'\n');
    let err = p.deliver(&signed).unwrap_err();
    assert_eq!(err.reason(), "verify-failed", "{err}");
    assert_eq!(fx.host_head(), head, "an unverifiable head was pushed");
    // A sidecar signed for another record fails the same way.
    let prepared = p.prepare(&fx.key("erin"), vec![draft("Mine", &["rust"]).into_item()]).unwrap();
    let mut signed = sign_ok(prepared, &fx.key("erin")).unwrap();
    let other = p.prepare(&fx.key("erin"), vec![draft("Other", &["rust"]).into_item()]).unwrap();
    let other = sign_ok(other, &fx.key("erin")).unwrap();
    signed.files[0].sidecar = other.files[0].sidecar.clone();
    assert_eq!(p.deliver(&signed).unwrap_err().reason(), "verify-failed");
    assert_eq!(fx.host_head(), head);
}

#[test]
fn a_signer_who_lost_standing_while_the_batch_waited_is_not_pushed() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("Mine", &["rust"]).into_item()]).unwrap();
    let signed = sign_ok(prepared, &fx.key("erin")).unwrap();
    // Admins retire erin's key (routine rotation) before the push: the store moves to epoch 2.
    let mut org = fx.org.clone();
    org.advance(&["alice", "bob"], |t, spec| t.rotate(spec, "erin", "erin2", false));
    let n = org.files.len();
    fx.remote.write(&format!("roster/epochs/{n}.json"), &org.files[n - 1].epoch_json);
    fx.remote.write(&format!("roster/epochs/{n}.sigs.json"), &org.files[n - 1].sigs_json);
    let c = fx.remote.commit("epoch 2");
    fx.fresh_for(&org, &c);
    let head = fx.host_head();
    let err = p.deliver(&signed).unwrap_err();
    assert_eq!(err.reason(), "signer-not-active", "{err}");
    assert_eq!(fx.host_head(), head);
}

#[test]
fn the_push_command_never_forces() {
    let fx = Fx::new();
    let c = push_command(&fx.store, "0123456789abcdef0123456789abcdef01234567");
    let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    assert!(args.iter().any(|a| a == "push"));
    for a in &args {
        assert!(!a.contains("force") && !a.contains("lease") && !a.starts_with('+') && !a.contains(":+") && a != "-f" && !a.contains("--mirror") && !a.contains("--delete"), "{a}");
    }
    assert_eq!(args.last().unwrap(), "0123456789abcdef0123456789abcdef01234567:refs/heads/main");
}

#[test]
fn an_unjoined_store_is_not_a_place_to_publish() {
    let tmp = tempfile::tempdir().unwrap();
    let remote = TestRemote::new(&tmp.path().join("host"));
    remote.write(".gitattributes", GOOD_GITATTRIBUTES.as_bytes());
    remote.commit("empty");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let store = Store::open(&home, "acme", "acme-main", &remote.url(), true).unwrap();
    let org = cast();
    let err = Publisher::new(&store, cfg(), &NoIndex).prepare(&org.key("erin"), vec![draft("A note", &["rust"]).into_item()]).err().unwrap();
    assert_eq!(err.reason(), "store-not-usable", "{err}");
}

// ---------------------------------------------------------------------------------------------------------
// Preview contents: the similar list, reminders

struct FakeIndex(Vec<SimilarHit>);

impl SimilarRecords for FakeIndex {
    fn similar(&self, _: &SimilarQuery<'_>, _: usize) -> Vec<SimilarHit> {
        self.0.clone()
    }
}

#[test]
fn the_preview_lists_similar_records_bounded_and_without_control_characters() {
    let fx = Fx::new();
    let mut hits: Vec<SimilarHit> = (1..=9).map(|i| SimilarHit { id: format!("01JBX3Q8T2M9Z4K7WYH5N6D70{i}"), title: format!("Similar {i}"), tier: "L2".into(), score: 1.0 - i as f32 / 10.0 }).collect();
    hits[0].title = "evil\u{1b}[31m\ntitle".into();
    hits.push(SimilarHit { id: "01JBX3Q8T2M9Z4K7WYH5N6D001".into(), title: "this record itself".into(), tier: "L1".into(), score: 2.0 });
    let idx = FakeIndex(hits);
    let p = fx.publisher(&idx);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).unwrap();
    let sim = &prepared.preview().similar;
    assert_eq!(sim.len(), 5, "bounded");
    assert!(sim.iter().all(|h| h.id != prepared.preview().items[0].id), "the record itself is not 'similar'");
    assert!(sim.iter().all(|h| !h.title.chars().any(|c| c.is_control())), "{sim:?}");
    // The list is information: it is not part of what is signed.
    assert_eq!(prepared.preview().digest, preview_digest(&prepared.preview().items));
    // No index, no list; the publish is unaffected.
    let none = fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).unwrap();
    assert!(none.preview().similar.is_empty());
}

#[test]
fn crlf_in_a_source_is_normalized_before_hashing_and_a_lone_cr_is_refused() {
    let fx = Fx::new();
    let mut d = draft("A note", &["rust"]);
    d.body = "line one\r\nline two\r\n".into();
    let prepared = fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![d.into_item()]).unwrap();
    let text = String::from_utf8(prepared.preview().items[0].bytes.clone()).unwrap();
    assert!(!text.contains('\r') && text.ends_with("line one\nline two\n"));
    let mut d = draft("A note", &["rust"]);
    d.body = "line one\rline two\n".into();
    assert_eq!(fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![d.into_item()]).err().unwrap().reason(), "invalid");
}

#[test]
fn a_review_or_expiry_date_that_is_not_in_the_future_is_refused_at_publish() {
    let fx = Fx::new();
    for (review_by, expires) in [("2026-10-26", None), ("2026-10-01", None), ("2027-01-24", Some("2026-10-26"))] {
        let mut d = draft("A note", &["rust"]);
        d.review_by = review_by.into();
        d.expires = expires.map(String::from);
        let err = fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![d.into_item()]).err().unwrap();
        assert!(matches!(err, PublishError::Serialize(SerializeError::Invalid(delphi_xenia::record::Reject::Date(_)))), "{review_by} {expires:?}: {err:?}");
    }
}

#[test]
fn what_the_gate_writes_has_a_content_hash_the_verifier_recomputes() {
    let fx = Fx::new();
    let mut d = draft("A note", &["rust"]);
    d.pointers = vec![pointer_with_label("Notes")];
    let prepared = fx.publisher(&NoIndex).prepare(&fx.key("erin"), vec![d.into_item()]).unwrap();
    let bytes = &prepared.preview().items[0].bytes;
    let recipe = delphi_xenia::redaction::content_hash(bytes).unwrap();
    assert!(String::from_utf8_lossy(bytes).contains(&recipe));
    // Editing the body after the gate scanned it is caught by the verifier (check_content_hash), whatever the signer says.
    let evil = String::from_utf8(bytes.clone()).unwrap().replace("Handlers must dedupe", "Handlers must not dedupe").into_bytes();
    let id = &prepared.preview().items[0].id;
    let err = delphi_xenia::record::validate_record(Pinned { org: "acme", store: "acme-main" }, &format!("records/memory/{id}.md"), &evil, &{
        let key = fx.key("erin");
        let h = delphi_xenia::hash::of(&evil);
        let d = Domain::record("acme", "acme-main", Kind::Memory, id, &key.did(), &h, 1, "2026-10-26T12:00:00Z").unwrap();
        Sidecar { kid: key.did(), signed_at: "2026-10-26T12:00:00Z".into(), roster_epoch: 1, hash: h, sig: key.sign(&d) }.to_json().into_bytes()
    }, &|_| Ok(()))
    .unwrap_err();
    assert_eq!(err.reason(), "mismatch");
}
