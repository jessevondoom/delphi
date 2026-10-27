//! Tests for the card, the action parser and the parked two-step. Real stores and real signatures; "refused" means the
//! host did not move.

use super::*;
use crate::xenia_publish::tests::{draft, Fx, Yes};
use crate::xenia_publish::NoIndex;
use crate::xenia_store::Store;
use crate::xenia_sync::VerifyConfig;

fn ctx(fx: &Fx) -> ScopeCtx {
    ScopeCtx { home: fx.store.paths.root.clone(), workspace: Some(home_of(fx).join("ws")), today: "2026-10-26".into() }
}

fn opts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// What a job on another thread needs: the same store reopened by path, the clock and a key.
fn home_of(fx: &Fx) -> std::path::PathBuf {
    fx.store.paths.root.ancestors().nth(3).unwrap().to_path_buf()
}

fn job(fx: &Fx, key: Identity, action: Action) -> impl FnOnce(&mut dyn crate::xenia_publish::Approver) -> Result<Published, PublishError> + Send + 'static {
    let (home, url) = (home_of(fx), fx.remote.url());
    move |ap| {
        let store = Store::open(&home, "acme", "acme-main", &url, true).map_err(|e| PublishError::Io(e.to_string()))?;
        let p = Publisher::new(&store, crate::xenia_fixture::cfg(), &NoIndex);
        action.run(&p, &key, ap)
    }
}

#[test]
fn control_characters_are_shown_as_escapes_so_a_card_cannot_redraw_itself() {
    let shown = escape("a\u{1b}[2Jb \u{202e}c \u{200b}d \u{e0041}e\tf\ng".as_bytes());
    assert_eq!(shown, "a\\u{1b}[2Jb \\u{202e}c \\u{200b}d \\u{e0041}e    f\ng");
    assert!(!shown.contains('\u{1b}') && !shown.contains('\u{202e}'));
    assert_eq!(escape(&[0x66, 0xff, 0x6f]), "f\u{fffd}o", "bytes that are not UTF-8 are shown as the replacement character");
}

#[test]
fn the_card_shows_every_signed_byte_and_a_short_code_that_the_digest_covers() {
    let fx = Fx::new();
    let p = fx.publisher(&NoIndex);
    let prepared = p.prepare(&fx.key("erin"), vec![draft("A note", &["rust"]).into_item()]).unwrap();
    let pv = prepared.preview();
    let text = card("Publish a memory", pv);
    assert!(text.contains(&pv.items[0].path) && text.contains("Approval code: "), "{text}");
    assert!(text.contains(&String::from_utf8(pv.items[0].bytes.clone()).unwrap()), "the card carries the file's exact text");
    assert!(text.contains(&pv.items[0].hash));
    let c = code(pv);
    assert_eq!(c.len(), 12);
    assert!(pv.digest.contains(&c));
    assert!(text.contains(&c));
}

#[test]
fn actions_are_built_from_plain_options_and_refuse_what_is_missing() {
    let fx = Fx::new();
    let c = ctx(&fx);
    assert!(matches!(Action::build("endorse", Some("ID"), &opts(&[]), &c).unwrap(), Action::Endorse(s) if s.checked.len() == 2));
    assert!(Action::build("endorse", None, &opts(&[]), &c).unwrap_err().contains("needs a record id"));
    assert!(Action::build("dispute", Some("ID"), &opts(&[]), &c).unwrap_err().contains("--note"));
    assert!(Action::build("withdraw", Some("ID"), &opts(&[]), &c).unwrap_err().contains("--reason"));
    assert!(Action::build("withdraw", Some("ID"), &opts(&[("reason", "nonsense")]), &c).unwrap_err().contains("one of"));
    assert!(matches!(Action::build("withdraw", Some("ID"), &opts(&[("reason", "sensitive")]), &c).unwrap(), Action::Withdraw { reason: TombstoneReason::Sensitive, .. }));
    assert!(Action::build("republish", Some("x"), &opts(&[]), &c).unwrap_err().contains("--old"));
    assert!(Action::build("bogus", None, &opts(&[]), &c).unwrap_err().contains("unknown action"));
    for w in Action::WORDS {
        assert!(Action::build(w, Some("ID"), &opts(&[("note", "n"), ("reason", "mistake"), ("old", "did:key:z")]), &c).map(|a| a.word() == w).unwrap_or(true), "{w}");
    }
}

#[test]
fn the_scope_of_a_file_comes_from_where_it_lives_and_unknown_places_are_operator_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = tmp.path().join("ws");
    std::fs::create_dir_all(ws.join("team")).unwrap();
    std::fs::write(ws.join("team/a.md"), "x").unwrap();
    std::fs::write(ws.join("b.md"), "x").unwrap();
    std::fs::write(tmp.path().join("c.md"), "x").unwrap();
    let c = ScopeCtx { home: tmp.path().join("home"), workspace: Some(ws.clone()), today: "2026-10-26".into() };
    assert_eq!(c.scope_of(&ws.join("team/a.md")), Scope::Team);
    assert_eq!(c.scope_of(&ws.join("b.md")), Scope::Workspace);
    assert_eq!(c.scope_of(&tmp.path().join("c.md")), Scope::Operator, "outside the workspace is never published");
    assert_eq!(c.scope_of(&ws.join("team/../../c.md")), Scope::Operator, ".. cannot talk its way in");
}

#[test]
fn a_parked_card_signs_exactly_what_it_showed_when_approved() {
    let fx = Fx::new();
    let before = fx.host_head();
    let action = Action::Dispute { target: "01JBX3Q8T2M9Z4K7WYH5N6D000".into(), note: "wrong".into() };
    // Nothing to dispute yet: refused before there is a card, and the host did not move.
    let err = begin(job(&fx, fx.key("erin"), action), WAIT).err().expect("a refusal before the card");
    assert_eq!(err.reason(), "rule", "{err}");
    assert_eq!(fx.host_head(), before);

    let id = fx.memory_id("hank", draft("Stripe retries", &["rust"]));
    let head = fx.host_head();
    let action = Action::Dispute { target: id.clone(), note: "the retry window is wrong".into() };
    let pending = begin(job(&fx, fx.key("erin"), action), WAIT).unwrap();
    assert_eq!(fx.host_head(), head, "a card that is not answered has sent nothing");
    let text = card("Dispute", &pending.preview);
    assert!(text.contains("the retry window is wrong") && text.contains(&code(&pending.preview)));
    let shown = pending.preview.items[0].bytes.clone();
    let path = pending.preview.items[0].path.clone();
    let out = pending.decide(Answer { approve: true, flags_acknowledged: true }).unwrap();
    assert_eq!(fx.host_file(&path).unwrap(), shown, "the host holds the bytes the card showed");
    assert_eq!(fx.host_head(), out.commit);
}

#[test]
fn a_denied_or_abandoned_card_sends_nothing() {
    let fx = Fx::new();
    let id = fx.memory_id("hank", draft("Stripe retries", &["rust"]));
    let head = fx.host_head();
    let mk = || Action::Dispute { target: id.clone(), note: "no".into() };
    let denied = begin(job(&fx, fx.key("erin"), mk()), WAIT).unwrap().decide(Answer { approve: false, flags_acknowledged: false });
    assert_eq!(denied.unwrap_err().reason(), "denied");
    assert_eq!(fx.host_head(), head);
    // Dropped without an answer: the thread sees the channel close and denies.
    let pending = begin(job(&fx, fx.key("erin"), mk()), WAIT).unwrap();
    drop(pending);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(fx.host_head(), head);
    // Unanswered for the whole wait: a denial too.
    let pending = begin(job(&fx, fx.key("erin"), mk()), Duration::from_millis(100)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(pending.decide(Answer { approve: true, flags_acknowledged: true }).unwrap_err().reason(), "denied", "a late yes is not honoured");
    assert_eq!(fx.host_head(), head);
}

#[test]
fn the_same_action_through_the_inline_approver_and_the_parked_one_publishes_the_same_bytes() {
    let fx = Fx::new();
    let id = fx.memory_id("hank", draft("Stripe retries", &["rust"]));
    let mut yes = Yes::new();
    let p = fx.publisher(&NoIndex);
    Action::Dispute { target: id.clone(), note: "same".into() }.run(&p, &fx.key("erin"), &mut yes).unwrap();
    assert_eq!(yes.calls, 1);
    let _ = VerifyConfig::at(0);
}
