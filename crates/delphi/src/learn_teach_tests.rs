//! `specs/129` S3: `delphi teach`, `delphi learn lessons`, `delphi learn unteach`, and the card view of `delphi learn list`.

use super::*;
use delphi_core::log::{EventLog, LogOptions};
use delphi_engine::mcp_packs::{Detail, Packs};
use delphi_engine::workspace::WorkspacePaths;

struct Fx {
    _t: tempfile::TempDir,
    ws: WorkspacePaths,
}

fn fx() -> Fx {
    let t = tempfile::tempdir().unwrap();
    let ws = delphi_engine::workspace::make_test_workspace(t.path());
    Fx { _t: t, ws }
}

/// The mission log a running daemon would hold: teach and unteach must not need it.
fn hold_a_mission(f: &Fx) -> EventLog {
    let dir = delphi_core::sessions::dir(&f.ws.dir, "2026-11-20-session");
    for _ in 0..300 {
        match EventLog::open(&dir, LogOptions::default()) {
            Err(LogError::Locked(_)) => std::thread::sleep(std::time::Duration::from_millis(10)),
            other => return other.unwrap(),
        }
    }
    EventLog::open(&dir, LogOptions::default()).unwrap()
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

fn teach(f: &Fx, role: &str, text: &str, goal: Option<&str>) -> Result<String, String> {
    let a = TeachArgs { role: role.into(), text: text.into(), goal: goal.map(String::from), scope: "workspace".into() };
    let p = plan_teach(&f.ws.home, &f.ws.dir, &a)?;
    apply_teach(&f.ws.home, &f.ws.dir, &p)
}

fn lessons_file(f: &Fx) -> std::path::PathBuf {
    f.ws.dir.join("local/memory/agents/builder/lessons.md")
}

fn ledger(f: &Fx) -> Vec<EventEnvelope> {
    replay(&ledger_dir(&f.ws.home, &f.ws.dir, "workspace")).unwrap_or_default()
}

/// Every file a teach or an unteach could touch, as bytes, with the lessons log's events.
fn state(f: &Fx) -> Vec<Option<Vec<u8>>> {
    let mut v: Vec<Option<Vec<u8>>> = ["local/memory/agents/builder/lessons.md", "local/ratified/memory/agents/builder/lessons.md", "local/ratified.lock"].iter().map(|p| std::fs::read(f.ws.dir.join(p)).ok()).collect();
    v.push(Some(serde_json::to_vec(&ledger(f).iter().map(|e| (e.kind.clone(), e.payload.clone())).collect::<Vec<_>>()).unwrap()));
    v
}

fn pack(f: &Fx, role: &str) -> String {
    Packs::new(f.ws.clone()).pack_for(role, false, Detail::Compact, None).unwrap()
}

const LESSON: &str = "T: before a release D: regenerate docs/reference with `delphi docs cli`";

/// AC-1: the lesson is shown, then written with its goal and provenance; the next pack has a new `pack_hash` and carries it. The
/// daemon holding a mission's log is no obstacle: the record goes to the lessons log.
#[test]
fn ac1_teach_writes_the_lesson_with_its_goal_and_provenance_and_the_next_pack_carries_it() {
    let f = fx();
    let _daemon = hold_a_mission(&f);
    let packs = Packs::new(f.ws.clone());
    let before = delphi_engine::mcp_packs::pack_hash_of(&pack(&f, "builder")).unwrap().to_string();
    assert!(!pack(&f, "builder").contains("regenerate docs/reference"));

    let a = parse_teach(&args(&["builder", LESSON, "--goal", "the docs test fails in CI otherwise"])).unwrap();
    let p = plan_teach(&f.ws.home, &f.ws.dir, &a).unwrap();
    let shown = render_teach(&p);
    assert!(shown.contains("teach builder L-W-001") && shown.contains("goal:     the docs test fails in CI otherwise") && shown.contains("origin:   operator-terminal"), "{shown}");
    assert!(shown.contains("- **Why:** the docs test fails in CI otherwise (operator-terminal, "), "the operator reads the block as it will be written: {shown}");
    assert!(!lessons_file(&f).exists(), "planning writes nothing");

    let said = apply_teach(&f.ws.home, &f.ws.dir, &p).unwrap();
    assert!(said.contains("taught builder L-W-001") && said.contains("pack_hash is now") && said.contains("delphi learn unteach builder L-W-001"), "{said}");
    let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
    assert!(text.contains("- **D:** regenerate docs/reference with `delphi docs cli`") && text.contains("- **Why:** the docs test fails in CI otherwise (operator-terminal, "), "{text}");
    assert!(delphi_engine::checks::cap_errors(&text).is_empty(), "{:?}", delphi_engine::checks::cap_errors(&text));
    let (fm, _) = delphi_engine::workspace::split_frontmatter(&text);
    let k = &fm.unwrap()["kept"][0];
    assert_eq!((k["source_type"].as_str(), k["origin"].as_str(), k["tool"].as_str(), k["original_tier"].as_str()), (Some("user_ratified"), Some("operator-terminal"), Some("operator"), Some("L5")));
    assert!(k["ratified_from"].as_str().unwrap().starts_with("teach/"), "{k:?}");
    assert_eq!(std::fs::read_to_string(f.ws.dir.join("local/ratified/memory/agents/builder/lessons.md")).unwrap(), text, "the ratified copy and the pin move with the live file");

    let after = pack(&f, "builder");
    assert_ne!(delphi_engine::mcp_packs::pack_hash_of(&after).unwrap(), before, "the pack_hash changed");
    assert_eq!(packs.compact_pack_hash("builder").unwrap(), delphi_engine::mcp_packs::pack_hash_of(&after).unwrap(), "the hash get_agent compares is the new one");
    assert!(after.contains("regenerate docs/reference") && after.contains("the docs test fails in CI otherwise"), "the next pack carries the lesson and its goal");
    assert!(!pack(&f, "architect").contains("regenerate docs/reference"), "only that role");

    let log = ledger(&f);
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].kind.as_str(), log[0].payload["rule"].as_str(), log[0].payload["goal"].as_str(), log[0].payload["origin"].as_str()), (TAUGHT, Some("L-W-001"), Some("the docs test fails in CI otherwise"), Some("operator-terminal")));
    assert_eq!(k["ratified_from"].as_str(), Some(format!("teach/{}", log[0].seq).as_str()), "the provenance names the record");
}

/// AC-2 and §9 item 1: without a terminal the command refuses and writes nothing; there is no `--yes`.
#[test]
fn ac2_teach_and_unteach_refuse_without_a_terminal_and_write_nothing() {
    let f = fx();
    let before = state(&f);
    // The tests run without a terminal, which is the case being checked.
    let e = teach_run(&f.ws.home, &f.ws.dir, &args(&["builder", LESSON])).unwrap_err();
    assert!(e.contains("needs a terminal") && e.contains("nothing was written"), "{e}");
    assert!(teach_run(&f.ws.home, &f.ws.dir, &args(&["builder", LESSON, "--yes"])).unwrap_err().contains("no --yes"));
    assert_eq!(state(&f), before);
    assert!(!ledger_dir(&f.ws.home, &f.ws.dir, "workspace").exists(), "not even the lessons log is created");

    teach(&f, "builder", LESSON, None).unwrap();
    let taught = state(&f);
    let e = unteach_run(&f.ws.home, &f.ws.dir, &args(&["unteach", "builder", "L-W-001"])).unwrap_err();
    assert!(e.contains("needs a terminal"), "{e}");
    assert!(unteach_run(&f.ws.home, &f.ws.dir, &args(&["unteach", "builder", "L-W-001", "--yes"])).unwrap_err().contains("no --yes"));
    assert_eq!(state(&f), taught);
}

/// AC-5 (terminal side): at the 26th lesson the teach is refused with the three ways out, and nothing is written.
#[test]
fn ac5_the_26th_lesson_is_refused_with_the_three_ways_out() {
    let f = fx();
    let rules: String = (1..=25).map(|i| format!("### `[L-W-{i:03}]` Rule {i}\n- **T:** case {i}\n- **D:** do thing {i}\n- **F:** none\n\n")).collect();
    let full = format!("---\nid: workspace-lessons-builder\nsource_type: user_ratified\nactive_rules: 25\n---\n\n# Lessons\n\n{rules}");
    delphi_engine::ratify::edit_pinned(&f.ws.home, &f.ws.dir, delphi_engine::ratify::Scope::Workspace, "memory/agents/builder/lessons.md", |_| Ok(full.clone())).unwrap();
    let before = state(&f);
    let a = TeachArgs { role: "builder".into(), text: LESSON.into(), goal: None, scope: "workspace".into() };
    let e = plan_teach(&f.ws.home, &f.ws.dir, &a).unwrap_err();
    assert!(e.contains("Rule of 25") && e.contains("merge two lessons") && e.contains("demote one") && e.contains("deterministic script") && e.contains("nothing was written"), "{e}");
    assert_eq!(state(&f), before);
}

/// AC-6 and §9 item 5: a text that relaxes governance, one over 350 characters, a duplicate and an unknown role are refused with
/// the reason, and a refusal writes nothing.
#[test]
fn ac6_relaxing_oversized_duplicate_and_unknown_role_lessons_are_refused_with_the_reason() {
    let f = fx();
    teach(&f, "builder", LESSON, None).unwrap();
    let before = state(&f);
    let plan = |role: &str, text: &str, goal: Option<&str>| plan_teach(&f.ws.home, &f.ws.dir, &TeachArgs { role: role.into(), text: text.into(), goal: goal.map(String::from), scope: "workspace".into() });
    assert!(plan("builder", "T: when in a hurry D: skip the approval cards", None).unwrap_err().contains("relaxes"));
    assert!(plan("builder", "T: always D: run the formatter", Some("so we can skip the approval cards")).unwrap_err().contains("relaxes"), "the goal is served with the lesson, so it is linted too");
    let long = format!("T: before a merge D: {}", "check the generated reference pages twice ".repeat(9));
    let e = plan("builder", &long, None).unwrap_err();
    assert!(e.contains("characters") && e.contains("350"), "{e}");
    let e = plan("builder", "T: before a release D: regenerate docs/reference with `delphi docs cli` now", None).unwrap_err();
    assert!(e.contains("repeats [L-W-001]") && e.contains("delphi learn unteach builder L-W-001"), "the conflict is named with the way to merge: {e}");
    assert!(plan("nobody", LESSON, None).unwrap_err().contains("no role `nobody`"));
    assert!(plan("../builder", LESSON, None).is_err());
    assert!(plan("builder", "regenerate the docs", None).unwrap_err().contains("trigger"));
    assert!(plan("builder", "T: x G: y D: z", Some("w")).is_ok(), "a G: before the D: is the trigger's words, so --goal is the only goal");
    assert!(plan("builder", "T: x D: z G: y", Some("w")).unwrap_err().contains("given twice"));
    assert_eq!(state(&f), before);
}

/// §6 item 5: a lesson that overlaps one the role carries (but does not repeat it) is named before the yes, and the yes then also
/// confirms that both stay.
#[test]
fn an_overlapping_lesson_is_named_and_the_question_says_both_stay() {
    let f = fx();
    teach(&f, "builder", LESSON, None).unwrap();
    let a = TeachArgs { role: "builder".into(), text: "T: before a release tag D: regenerate the man page with `delphi docs man`".into(), goal: None, scope: "workspace".into() };
    let p = plan_teach(&f.ws.home, &f.ws.dir, &a).unwrap();
    assert!(!p.overlaps.is_empty(), "{:?}", p.overlaps);
    let shown = render_teach(&p);
    assert!(shown.contains("it overlaps what the role already carries:\n  [L-W-001]") && shown.contains("Both will stay"), "{shown}");
    assert_eq!(question("Teach it?", &p.overlaps), "Teach it? Both lessons stay.");
    assert_eq!(question("Teach it?", &[]), "Teach it?");
}

/// AC-7, §8 item 3 and §9 item 8: unteach removes the lesson with the pinned write, records `learn.unlearned` with the id, the date,
/// the goal and the words, and teaching the recorded words again restores it. It works while the daemon holds a mission's log.
#[test]
fn ac7_unteach_removes_records_and_a_reteach_restores_the_lesson() {
    let f = fx();
    let _daemon = hold_a_mission(&f);
    teach(&f, "builder", "T: a script needs Python D: call `python`; the CI image has no python3 F: the image gains python3", Some("CI runs failed on it twice")).unwrap();
    teach(&f, "builder", LESSON, None).unwrap();
    let with_both = pack(&f, "builder");
    let carried = delphi_engine::customize::lessons(&f.ws.home, &f.ws.dir, "builder");
    assert_eq!(carried.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["L-W-001", "L-W-002"]);
    let first = carried[0].clone();

    let e = plan_unteach(&f.ws.home, &f.ws.dir, "builder", "L-W-001").unwrap();
    assert!(render_unteach("builder", &e).contains("goal:   CI runs failed on it twice"));
    let said = apply_unteach(&f.ws.home, &f.ws.dir, "builder", "L-W-001").unwrap();
    assert!(said.contains("unteached builder L-W-001") && said.contains("To teach it again: delphi teach builder 'T: a script needs Python"), "{said}");

    let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
    assert!(!text.contains("python3") && text.contains("regenerate docs/reference"), "{text}");
    assert!(!text.contains("\"L-W-001\""), "its provenance line went with it: {text}");
    assert!(delphi_engine::checks::cap_errors(&text).is_empty(), "the file still passes the Rule of 25 check: {:?}", delphi_engine::checks::cap_errors(&text));
    assert_eq!(std::fs::read_to_string(f.ws.dir.join("local/ratified/memory/agents/builder/lessons.md")).unwrap(), text, "the same pinned write as an accept");
    let after = pack(&f, "builder");
    assert!(!after.contains("python3") && after.contains("regenerate docs/reference"), "an unteached lesson is not served");
    assert_ne!(delphi_engine::mcp_packs::pack_hash_of(&after), delphi_engine::mcp_packs::pack_hash_of(&with_both));

    let u = ledger(&f).into_iter().find(|e| e.kind == UNLEARNED).expect("learn.unlearned is recorded");
    assert_eq!((u.payload["rule"].as_str(), u.payload["role"].as_str(), u.payload["goal"].as_str(), u.payload["date"].as_str()), (Some("L-W-001"), Some("builder"), Some("CI runs failed on it twice"), Some(delphi_core::bundles::today().as_str())));
    assert_eq!(u.payload["text"].as_str(), Some(first.lesson.text().as_str()), "the words are kept, so a re-teach is exact");

    // Re-teach from the recorded words: the same lesson, its goal included, is carried again.
    let listed = lessons_run(&f.ws.home, &f.ws.dir, Some("builder"), false).unwrap();
    assert!(listed.contains("unteached L-W-001 on ") && listed.contains("to teach it again: delphi teach builder"), "{listed}");
    teach(&f, "builder", u.payload["text"].as_str().unwrap(), None).unwrap();
    let back = delphi_engine::customize::lessons(&f.ws.home, &f.ws.dir, "builder");
    assert!(back.iter().any(|e| e.lesson == first.lesson), "{back:?}");
    assert!(pack(&f, "builder").contains("the CI image has no python3"));
    assert!(!lessons_run(&f.ws.home, &f.ws.dir, Some("builder"), false).unwrap().contains("unteached L-W-001"), "a restored lesson is no longer offered for re-teaching");

    // An unknown id, an id that is not a lesson's and a role without one write nothing.
    let before = state(&f);
    assert!(plan_unteach(&f.ws.home, &f.ws.dir, "builder", "L-W-009").unwrap_err().contains("no lesson `L-W-009`"));
    assert!(apply_unteach(&f.ws.home, &f.ws.dir, "builder", "L-W-009").unwrap_err().contains("no lesson `L-W-009`"));
    assert!(plan_unteach(&f.ws.home, &f.ws.dir, "builder", "W-001").unwrap_err().contains("no lesson `W-001`"));
    assert!(apply_unteach(&f.ws.home, &f.ws.dir, "architect", "L-W-001").is_err());
    assert_eq!(state(&f), before);
}

/// A lesson that came with the release (`delphi init` seeds `L-BLD-001` for the builder) can be unteached and taught again, and a
/// new lesson taught beside it gets its own `L-W-` id.
#[test]
fn a_seeded_lesson_with_its_own_id_is_unteached_and_restored() {
    let f = fx();
    let seeded = "---\nid: lessons-builder\nsource_type: internal_empirical\nactive_rules: 1\n---\n\n# Builder lessons\n> scope note\n\n### `[L-BLD-001]` Anchor the mouth\n- **T:** Implementing stateful terminal avatars.\n- **D:** Anchor mouth alignment during blinks.\n- **F:** Vertical mouth hopping during redraws.\n";
    delphi_engine::ratify::edit_pinned(&f.ws.home, &f.ws.dir, delphi_engine::ratify::Scope::Workspace, "memory/agents/builder/lessons.md", |_| Ok(seeded.to_string())).unwrap();
    teach(&f, "builder", LESSON, Some("the docs test fails")).unwrap();
    let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
    assert!(text.contains("### `[L-W-001]`") && text.contains("active_rules: 2") && delphi_engine::checks::cap_errors(&text).is_empty(), "{text}");
    let said = apply_unteach(&f.ws.home, &f.ws.dir, "builder", "L-BLD-001").unwrap();
    assert!(said.contains("unteached builder L-BLD-001"), "{said}");
    let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
    assert!(!text.contains("L-BLD-001") && text.contains("> scope note") && text.contains("active_rules: 1") && delphi_engine::checks::cap_errors(&text).is_empty(), "{text}");
    let u = ledger(&f).into_iter().find(|e| e.kind == UNLEARNED).unwrap();
    assert_eq!(u.payload["text"], "T: Implementing stateful terminal avatars. D: Anchor mouth alignment during blinks. F: Vertical mouth hopping during redraws.");
    assert_eq!(u.payload["goal"], "none given");
    teach(&f, "builder", u.payload["text"].as_str().unwrap(), None).unwrap();
    assert!(pack(&f, "builder").contains("Anchor mouth alignment during blinks."));
}

/// §9 item 8: unteach takes the same pinned write as an accept, so a lessons file with edits nobody ratified is not touched.
#[test]
fn unteach_refuses_a_lessons_file_with_unratified_edits() {
    let f = fx();
    teach(&f, "builder", LESSON, None).unwrap();
    let live = lessons_file(&f);
    let sneaked = std::fs::read_to_string(&live).unwrap() + "\n### `[L-W-002]` Sneaked\n- **T:** x\n- **D:** y\n- **F:** z\n";
    std::fs::write(&live, &sneaked).unwrap();
    let e = apply_unteach(&f.ws.home, &f.ws.dir, "builder", "L-W-001").unwrap_err();
    assert!(e.contains("never ratified") && e.contains("nothing was written"), "{e}");
    assert_eq!(std::fs::read_to_string(&live).unwrap(), sneaked);
    assert!(!ledger(&f).iter().any(|e| e.kind == UNLEARNED));
}

/// §9 item 10: a secret typed into a goal or a lesson is masked before it is stored, by the redactor and the corpus of shapes of
/// `specs/102` §3.1 (the same fixtures as `history_redact`'s; a goal is one line, so the two multi-line shapes arrive as one line).
#[test]
fn a_secret_shape_in_a_goal_or_a_lesson_is_redacted_before_it_is_stored() {
    let mixed = "aB3dE6fG9h".repeat(5);
    let hex = "deadbeef".repeat(8);
    let corpus: Vec<(&str, String, String)> = vec![
        ("private-key block", "-----BEGIN RSA PRIVATE KEY----- MIIFAKEFAKEFAKEFAKE -----END RSA PRIVATE KEY-----".into(), "MIIFAKEFAKEFAKEFAKE".into()),
        ("cloud access key pair", format!("aws key AKIAFAKEFAKEFAKE0000 secret {mixed}"), "AKIAFAKEFAKEFAKE0000".into()),
        ("bearer token", "Authorization: Bearer abcDEF123456789xyz works".into(), "abcDEF123456789xyz".into()),
        ("JWT", "token eyJhbGciOiJGQUtFIn0.eyJmYWtlIjoiZmFrZSJ9.ZmFrZXNpZ25hdHVyZQ end".into(), "eyJmYWtlIjoiZmFrZSJ9".into()),
        ("password inside a URL", "clone https://deploy:hunter2hunter2@git.example.test/repo.git".into(), "hunter2hunter2".into()),
        ("token on one line", "my key is sk-ant-FAKEFAKEFAKEFAKEFAKEFAKE0000 ok".into(), "FAKEFAKEFAKEFAKEFAKEFAKE0000".into()),
        ("base64 fragment", format!("blob: {mixed} end"), mixed.clone()),
        ("long hex string", format!("digest {hex} done"), hex.clone()),
        ("name: value secret", "internal_secret_phrase: swordfish99 and nothing else".into(), "swordfish99".into()),
        ("secret from a code fence", "```env db_password: s3cretValue77 ```".into(), "s3cretValue77".into()),
    ];
    assert_eq!(corpus.len(), 10, "one per shape of §3.1");
    let (mut goals, mut lessons) = (Vec::new(), Vec::new());
    for (shape, text, secret) in corpus {
        let f = fx();
        let e = teach(&f, "builder", "T: before a deploy D: read the runbook first", Some(&text));
        // A shape the no-relaxation test or a cap refuses is not stored at all, which is also safe.
        if e.is_ok() {
            goals.push(shape);
            let stored = std::fs::read_to_string(lessons_file(&f)).unwrap();
            assert!(!stored.contains(&secret), "{shape}: the goal reached the lessons file: {stored}");
            assert!(!serde_json::to_string(&ledger(&f)).unwrap().contains(&secret), "{shape}: the goal reached the lessons log");
        }
        let lesson = format!("T: before a deploy D: use {}", text);
        let f = fx();
        if teach(&f, "builder", &lesson, None).is_ok() {
            lessons.push(shape);
            let stored = std::fs::read_to_string(lessons_file(&f)).unwrap();
            assert!(!stored.contains(&secret), "{shape}: the lesson's words reached the lessons file: {stored}");
        }
    }
    assert_eq!((goals.len(), lessons.len()), (10, 10), "every shape was stored masked, none refused: goals {goals:?}, lessons {lessons:?}");
    // The ordinary case is stored as typed.
    let f = fx();
    teach(&f, "builder", LESSON, Some("the docs test fails in CI otherwise")).unwrap();
    assert!(std::fs::read_to_string(lessons_file(&f)).unwrap().contains("- **Why:** the docs test fails in CI otherwise ("));
}

/// §7: `learn lessons` lists what each role carries with its goal, origin, age and provenance; serves are not counted yet (S7).
#[test]
fn learn_lessons_lists_what_a_role_carries_with_its_provenance_and_age() {
    let f = fx();
    // The fixture's operator scope holds one hand-written lesson for the architect, with no provenance.
    let first = lessons_run(&f.ws.home, &f.ws.dir, None, false).unwrap();
    assert!(first.starts_with("architect: 1 lesson(s)") && first.contains("origin: not recorded · date not recorded · from a hand edit") && !first.contains("builder:"), "{first}");
    let empty = tempfile::tempdir().unwrap();
    assert!(lessons_run(empty.path(), &empty.path().join("ws"), None, false).unwrap().contains("no role carries a lesson yet"));
    teach(&f, "builder", LESSON, Some("the docs test fails in CI otherwise")).unwrap();
    let a = TeachArgs { role: "builder".into(), text: "T: a test is flaky D: rerun it once alone and note it".into(), goal: None, scope: "operator".into() };
    apply_teach(&f.ws.home, &f.ws.dir, &plan_teach(&f.ws.home, &f.ws.dir, &a).unwrap()).unwrap();
    let all = lessons_run(&f.ws.home, &f.ws.dir, None, false).unwrap();
    assert!(all.contains("\nbuilder: 2 lesson(s), pack_hash "), "{all}");
    let at = |s: &str| all.find(s).unwrap_or_else(|| panic!("{s}: {all}"));
    assert!(at("L-O-001") < at("L-W-001"), "operator scope first, as the pack reads them");
    assert!(all.contains("goal: the docs test fails in CI otherwise · origin: operator-terminal · since ") && all.contains("(0 day(s)) · from teach/"), "{all}");
    assert!(all.contains("goal: none given · origin: operator-terminal"), "{all}");
    assert!(all.contains("served: not counted yet"), "{all}");
    assert!(ledger_dir(&f.ws.home, &f.ws.dir, "operator").starts_with(&f.ws.home) && replay(&ledger_dir(&f.ws.home, &f.ws.dir, "operator")).unwrap().len() == 1, "an operator lesson is recorded beside the operator's files");
    let j: serde_json::Value = serde_json::from_str(&lessons_run(&f.ws.home, &f.ws.dir, Some("builder"), true).unwrap()).unwrap();
    assert_eq!(j.as_array().unwrap().iter().map(|r| (r["id"].as_str().unwrap(), r["origin"].as_str().unwrap(), r["age_days"].as_i64())).collect::<Vec<_>>(), vec![("L-O-001", "operator-terminal", Some(0)), ("L-W-001", "operator-terminal", Some(0))]);
    assert!(lessons_run(&f.ws.home, &f.ws.dir, Some("nobody"), false).unwrap_err().contains("no role"));
}

/// §7 and §5.3: `learn list` shows each note that can become a lesson as a card: kind, role, goal, origin, recurrence (a host's count
/// labelled host-supplied) and expiry, a card with seven days or fewer marked, and the exact command that accepts it.
#[test]
fn learn_list_shows_cards_with_kind_goal_origin_recurrence_expiry_and_the_accept_command() {
    let f = fx();
    let soon = {
        let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap();
        delphi_core::time::format_unix_millis(now + 3 * 86_400, 0)[..10].to_string()
    };
    for (i, m) in ["host-aaaaaaaa", "host-bbbbbbbb"].iter().enumerate() {
        let mut log = EventLog::open(&delphi_core::sessions::dir(&f.ws.dir, m), LogOptions::default()).unwrap();
        let mut e = EventEnvelope::new(m, "learn.staged", serde_json::json!({"id": "h1", "trigger": "host_note", "summary": "[host:claude-code/opus] T: before a release D: regenerate the docs G: the docs test fails (evidence: s)", "event_seqs": [], "tier": "L0", "valid_until": soon}));
        e.agent = Some("builder".into());
        log.append(e).unwrap();
        if i == 0 {
            let mut other = EventEnvelope::new(m, "learn.staged", serde_json::json!({"id": "h2", "trigger": "host_note", "summary": "[host:claude-code/opus] Prefer small commits (evidence: s)", "event_seqs": [], "tier": "L0"}));
            other.agent = Some("builder".into());
            log.append(other).unwrap();
        }
    }
    let listed = crate::learn_cmd::run_in(&f.ws.dir, "list", None, &crate::learn_cmd::Options { mission: None, json: false }).unwrap();
    assert!(listed.contains("card: recurring · role builder · goal the docs test fails · origin host-reported (session host-aaaaaaaa) · reported by 2 sessions (host-supplied) · expires ") && listed.contains("(3 days left) [expires soon]"), "{listed}");
    assert!(listed.contains("accept: delphi learn keep host-aaaaaaaa/h1 --as lesson --role builder   (stop the daemon first"), "{listed}");
    assert!(listed.contains("card: other · role builder · goal none given · origin host-reported (session host-aaaaaaaa) · once · does not expire\n"), "{listed}");
    assert!(listed.contains("accept: delphi learn keep host-aaaaaaaa/h2 --as lesson --role builder --text \"T: <when it applies> D: <what to do>\""), "{listed}");
    let j: serde_json::Value = serde_json::from_str(&crate::learn_cmd::run_in(&f.ws.dir, "list", None, &crate::learn_cmd::Options { mission: None, json: true }).unwrap()).unwrap();
    let cards: Vec<&serde_json::Value> = j.as_array().unwrap().iter().filter_map(|r| r.get("card")).collect();
    assert_eq!(cards.len(), 3);
    assert!(cards.iter().any(|c| c["kind"] == "recurring" && c["expires_soon"] == true && c["goal"] == "the docs test fails"));

    // The card's goal and the keep it names agree: keeping it writes the goal into the Why: line with the host's origin.
    let a = crate::learn_keep::parse(&args(&["keep", "host-aaaaaaaa/h1", "--as", "lesson", "--role", "builder"]), Some("host-aaaaaaaa/h1")).unwrap();
    let p = crate::learn_keep::plan(&f.ws.home, &f.ws.dir, &a).unwrap();
    let shown = crate::learn_keep::render_plan(&p);
    assert!(shown.contains("  origin:   host-reported\n") && shown.contains("  goal:     the docs test fails\n"), "{shown}");
    crate::learn_keep::apply(&f.ws.home, &f.ws.dir, &p).unwrap();
    let text = std::fs::read_to_string(lessons_file(&f)).unwrap();
    assert!(text.contains("- **Why:** the docs test fails (host-reported, ") && text.contains("origin: \"host-reported\""), "{text}");
    // `--goal` on a keep adds the operator's goal when the note gave none, and is refused beside one it gave.
    let b = crate::learn_keep::parse(&args(&["keep", "host-bbbbbbbb/h1", "--as", "lesson", "--role", "builder", "--goal", "mine"]), Some("host-bbbbbbbb/h1")).unwrap();
    assert!(crate::learn_keep::plan(&f.ws.home, &f.ws.dir, &b).unwrap_err().contains("given twice"));
    assert!(crate::learn_keep::parse(&args(&["keep", "host-aaaaaaaa/h2", "--as", "rule", "--goal", "x"]), Some("host-aaaaaaaa/h2")).unwrap_err().contains("--as lesson"));
}
