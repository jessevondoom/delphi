//! The `bundle` record through the publish gate and the member commands (spec 027 §4.3). Same attacker model as the gate's
//! own tests: a real store, a real roster, real signatures; "refused" means the remote did not move.

use super::*;
use crate::xenia_member::EndorseSpec;
use crate::xenia_publish::tests::{cfg, Fx, Yes};
use crate::xenia_publish::{NoIndex, PublishError};
use delphi_xenia::pointer::{ContentType, Pin, PinKind, Scheme};
use delphi_xenia::record::Check;

const TREE: &str = "0123456789abcdef0123456789abcdef01234567";

fn tree(locator: &str) -> Pointer {
    Pointer { scheme: Scheme::Git, locator: locator.into(), pin: Pin { kind: PinKind::GitTree, value: Some(TREE.into()) }, hash: None, range: None, label: "The bundle tree".into(), content_type: ContentType::Other, bytes: None, access_hint: None }
}

fn bdraft(name: &str, tags: &[&str]) -> BundleDraft {
    BundleDraft {
        id: None,
        name: name.into(),
        version: "1.4.0".into(),
        bundle_hash: hash::of(name.as_bytes()),
        tree: tree("https://git.acme.example/platform/bundle.git#."),
        roles: vec![Role::Strategist, Role::Builder],
        connectors: vec!["slack".into()],
        compat_delphi: Some(">=0.3.0".into()),
        title: "Acme platform".into(),
        summary: "How the platform is built and run.".into(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
        confidence: Confidence::Medium,
        source_ceiling: SourceCeiling::InternalGenerated,
        author_role: Role::Operator,
        body: String::new(),
        review_by: None,
        expires: None,
        status: AuthorStatus::Active,
    }
}

impl Fx {
    fn publish_bundle(&self, who: &str, d: BundleDraft) -> Result<crate::xenia_publish::Published, PublishError> {
        self.publisher(&NoIndex).publish(&self.key(who), vec![d.into_item()], &mut Yes::new())
    }

    fn bundle_id(&self, who: &str, d: BundleDraft) -> String {
        let p = self.publish_bundle(who, d).unwrap_or_else(|e| panic!("publish: {e}"));
        p.paths[0].trim_start_matches("records/bundle/").trim_end_matches(".md").to_string()
    }

    fn set(&self) -> BundleSet {
        self.view();
        BundleSet::offline(&self.store, &cfg()).unwrap()
    }
}

#[test]
fn a_bundle_record_goes_through_the_gate_and_reads_back_as_member_signed_then_endorsed() {
    let fx = Fx::new();
    let mut yes = Yes::new();
    let p = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![bdraft("acme-platform", &["payments"]).into_item()], &mut yes).unwrap();
    assert_eq!(yes.calls, 1);
    let shown = String::from_utf8(yes.seen[0].items[0].bytes.clone()).unwrap();
    assert!(shown.contains("kind: bundle") && shown.contains("bundle_name: \"acme-platform\"") && shown.contains("tree:\n  scheme: git\n"), "{shown}");
    let id = p.paths[0].trim_start_matches("records/bundle/").trim_end_matches(".md").to_string();
    let set = fx.set();
    let b = set.find(&id).expect("the record is in the accepted state");
    assert_eq!((b.level, b.name.as_str(), b.version.as_str(), b.rev, b.publisher_handle.as_str()), (BundleLevel::MemberSigned, "acme-platform", "1.4.0", 1, "erin"));
    assert_eq!(b.signer_dids, vec![fx.key("erin").did()]);
    // A steward of the area endorses the exact revision: the level is endorsed, and the publisher is not in the list.
    let spec = EndorseSpec { target: id.clone(), scope: vec![], checked: vec![Check::TargetOpen, Check::SummaryMatches], note: None };
    fx.catch_up();
    fx.publisher(&NoIndex).endorse(&fx.key("frank"), spec, &mut Yes::new()).unwrap();
    let b = fx.set().find(&id).cloned().unwrap();
    assert_eq!((b.level, b.endorsed_by.clone()), (BundleLevel::Endorsed, vec!["frank".to_string()]));
    // A steward of another area cannot, and says so before anything is sent.
    let before = fx.host_head();
    let spec = EndorseSpec { target: id, scope: vec![], checked: vec![Check::TargetOpen, Check::SummaryMatches], note: None };
    let err = fx.publisher(&NoIndex).endorse(&fx.key("dave"), spec, &mut Yes::new()).unwrap_err();
    assert_eq!(err.reason(), "rule", "{err}");
    assert_eq!(fx.host_head(), before);
}

#[test]
fn a_secret_in_a_bundles_text_blocks_the_publish_before_any_preview_and_is_never_echoed() {
    let fx = Fx::new();
    let before = fx.host_head();
    for edit in [
        |d: &mut BundleDraft| d.title = "Platform sk-ant-abcdefghijklmnopqrstuvwxyz0123456789".into(),
        |d: &mut BundleDraft| d.summary = "contact /Users/jessevondoom/secrets for access".into(),
        |d: &mut BundleDraft| d.body = "-----BEGIN PRIVATE KEY-----\nabc\n-----END PRIVATE KEY-----\n".into(),
    ] {
        let mut d = bdraft("acme-platform", &["payments"]);
        edit(&mut d);
        let mut yes = Yes::new();
        let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut yes).unwrap_err();
        assert_eq!(err.reason(), "scan-blocked", "{err}");
        assert_eq!(yes.calls, 0, "nothing was previewed");
        assert!(!err.to_string().contains("abcdefghijklmnop"), "{err}");
    }
    assert_eq!(fx.host_head(), before);
}

#[test]
fn a_bundle_has_no_content_source_and_a_bad_tree_never_reaches_the_remote() {
    let fx = Fx::new();
    let before = fx.host_head();
    // A bundle claiming promoted content as its origin is refused (a memory is the only kind that has a content source).
    let mut item = bdraft("acme-platform", &["payments"]).into_item();
    item.origin = crate::xenia_publish::Origin::Content(crate::xenia_publish::ContentOrigin { scope: crate::xenia_publish::Scope::Team, promoted: true, file: None });
    assert_eq!(fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![item], &mut Yes::new()).unwrap_err().reason(), "wrong-origin");
    // A tree pointer the verifier would refuse is refused by the serializer, so it cannot be signed.
    for loc in ["ssh://git.acme.example/p.git#.", "file:///etc#.", "https://user:pw@git.acme.example/p.git#.", "https://git.acme.example/p.git#../x", "https://git.acme.example/p.git"] {
        let mut d = bdraft("acme-platform", &["payments"]);
        d.tree = tree(loc);
        let err = fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![d.into_item()], &mut Yes::new()).unwrap_err();
        assert_eq!(err.reason(), "invalid", "{loc}: {err}");
    }
    assert_eq!(fx.host_head(), before);
}

#[test]
fn only_the_owner_publishes_a_new_rev_and_an_id_cannot_change_kind() {
    let fx = Fx::new();
    let id = fx.bundle_id("erin", bdraft("acme-platform", &["payments"]));
    fx.catch_up();
    // Another member cannot take the record over with a higher rev.
    let mut d = bdraft("acme-platform", &["payments"]);
    d.id = Some(id.clone());
    d.version = "9.9.9".into();
    assert_eq!(fx.publish_bundle("hank", d.clone()).unwrap_err().reason(), "not-owner");
    // The owner may: rev 2, new bytes, and an endorsement of rev 1 does not carry.
    let spec = EndorseSpec { target: id.clone(), scope: vec![], checked: vec![Check::TargetOpen, Check::SummaryMatches], note: None };
    fx.publisher(&NoIndex).endorse(&fx.key("frank"), spec, &mut Yes::new()).unwrap();
    assert_eq!(fx.set().find(&id).unwrap().level, BundleLevel::Endorsed);
    fx.catch_up();
    fx.publish_bundle("erin", d).unwrap();
    let b = fx.set().find(&id).cloned().unwrap();
    assert_eq!((b.rev, b.version.as_str(), b.level), (2, "9.9.9", BundleLevel::MemberSigned));
    // An id that is a bundle cannot be reused for a memory.
    let mut m = crate::xenia_publish::tests::draft("A memory", &["rust"]);
    m.id = Some(id);
    assert_eq!(fx.publisher(&NoIndex).publish(&fx.key("erin"), vec![m.into_item()], &mut Yes::new()).unwrap_err().reason(), "id-taken");
}

#[test]
fn the_author_and_a_covering_steward_withdraw_a_bundle_and_a_withdrawn_one_is_not_offered() {
    let fx = Fx::new();
    let id = fx.bundle_id("erin", bdraft("acme-platform", &["payments"]));
    fx.catch_up();
    // A steward of another area cannot hide it; the covering steward can.
    let err = fx.publisher(&NoIndex).withdraw(&fx.key("dave"), &id, delphi_xenia::record::TombstoneReason::Mistake, None, &mut Yes::new()).unwrap_err();
    assert_eq!(err.reason(), "rule", "{err}");
    fx.publisher(&NoIndex).withdraw(&fx.key("frank"), &id, delphi_xenia::record::TombstoneReason::Mistake, None, &mut Yes::new()).unwrap();
    let b = fx.set().find(&id).cloned().unwrap();
    assert_eq!((b.level, b.hidden), (BundleLevel::Hidden, Some("steward-tombstone")));
}
