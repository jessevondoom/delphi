//! Spec 102 S7: calibration of the daemon paste rule (§3.4, `history_redact::scrub`) over a labelled set of prompts.
//!
//! Each fixture is labelled by how the operator produced it: `Typed` (written by the operator) or `Pasted` (an artifact the operator
//! pasted, possibly after a typed line of introduction). The rule is run as the daemon runs it today (`Typing::NotSeen`, since no input
//! timing reaches the daemon yet) and as it would run with `Typing::Typed`. The two error rates are:
//!
//! * **false-paste**: a `Typed` prompt in which some span was hashed (the record loses words the operator wrote);
//! * **false-typed**: a `Pasted` prompt in which NO span was hashed (pasted text is kept as if typed). This is the one that matters
//!   for privacy; the rule errs toward not storing, so false-paste is the price.
//!
//! The thresholds are hypotheses (§3.4, OD-3). This test pins the measured rates as ceilings so a change to the rule cannot make
//! either worse without a failing test. All text is invented.

use delphi_engine::history_redact::{scrub, Input, Typing, DEFAULT_PASTE_CHARS};

const KEY: &[u8] = &[9u8; 32];

fn words(n: usize) -> String {
    const W: &[&str] = &["the", "queue", "migration", "needs", "a", "plan", "before", "we", "touch", "billing", "and", "I", "would", "like", "it", "short", "but", "complete", "with", "owners"];
    (0..n).map(|i| W[(i * 7 + i / 3) % W.len()]).collect::<Vec<_>>().join(" ")
}

fn typed() -> Vec<(&'static str, String)> {
    vec![
        ("one-line ask", "Summarize what changed in the last release and flag anything risky.".into()),
        ("one-line question with parens", "Which is better (Postgres or SQLite) for a single-user tool that must run offline?".into()),
        ("two short paragraphs", "I want to restructure the onboarding doc so a new hire can follow it in one sitting.\n\nStart from the current outline and cut anything that only matters after the first week.".into()),
        ("three short paragraphs", format!("Context first.\n\n{}.\n\nAnd the last thing: {}.", words(30), words(25))),
        ("bulleted goals", "Goals for this week:\n- finish the migration plan\n- review the three open PRs\n- draft the customer update\n- book the retro".into()),
        ("numbered goals", "1. Decide on the queue.\n2. Write the rollout steps.\n3. Ask legal about the data clause.".into()),
        ("goal with deadline", "Ship the pricing page by Friday; if that slips, tell me by Wednesday what to cut.".into()),
        ("inline code mention", "Rename `fn load_config()` to `fn read_config()` everywhere and fix the callers.".into()),
        ("path and flag", "Run cargo test -p delphi-engine and tell me which of the history tests are slow.".into()),
        ("long typed paragraph (450 chars)", format!("{}.", words(80))),
        ("long typed paragraph (700 chars)", format!("{}. {}.", words(70), words(60))),
        ("operator-written 3-line snippet", "Make this pass:\n\nfn add(a: i32, b: i32) -> i32 {\n    a + b\n}\n".into()),
        ("operator-written config lines", "Use these settings:\nretries: 3\ntimeout: 30\nbackoff: exponential".into()),
        ("operator-written key-value brief", "Audience: the board\nTone: plain\nLength: one page\nOwner: me".into()),
        ("short list with semicolons", "Compare three vendors; keep to price; flag lock-in; no more than a page.".into()),
        ("question about a spec", "Does spec 102 allow the report to quote a prompt? I thought it only counted.".into()),
        ("casual", "ok go ahead and merge it, thanks".into()),
        ("typed with quote", "She said \"ship it\" but I think we should wait until the numbers are in.".into()),
        ("multi-sentence plan", "First read the three vendor decks. Then list claims that disagree. Then ask me which two matter most.".into()),
        ("typed email draft request", "Draft a reply to Dana: thank her, say we cannot start before March, offer a call on the 4th.".into()),
        ("typed with numbers", "Budget is 40k for Q1, 55k for Q2; move 5k from tools to research if the first quarter underspends.".into()),
        ("two-line instruction", "Fix the failing test in the history module.\nDo not change the public API.".into()),
        ("typed bullets with sub-points", "Plan:\n- research the options\n  - three vendors\n  - two open source\n- write the memo".into()),
        ("typed prompt with a url", "Read https://example.test/docs/queues and tell me whether it supports priorities.".into()),
        ("typed prompt with a filename list", "Look at README.md, docs/guides/rotation.md and the CONTRIBUTING file and reconcile them.".into()),
        ("typed three-line poem-like note", "Keep it short\nKeep it plain\nKeep it honest".into()),
        ("typed prompt with arrows", "Map the flow: intake -> triage -> review -> publish, and mark where we wait.".into()),
        ("typed prompt with colon heading", "Question: should the retro be weekly or biweekly given the team size?".into()),
        ("typed 399 chars", words(80).chars().take(399).collect::<String>()),
        ("typed prompt with a parenthetical aside", "Update the changelog (the one in docs, not the root one) and mention the purge fix.".into()),
        ("typed two-paragraph with a list", "The report needs three parts.\n\n- what we did\n- what we found\n- what is open".into()),
        ("typed instructions with a code word", "Use the word [draft] in the title until I approve it.".into()),
    ]
}

fn pasted() -> Vec<(&'static str, String)> {
    let log = "2026-11-12 10:00:01 INFO server started on port 8080\n2026-11-12 10:00:02 INFO loading config from /etc/app/config.toml\n2026-11-12 10:00:05 WARN slow query took 1200ms\n2026-11-12 10:00:09 ERROR connection reset by peer\n2026-11-12 10:00:09 INFO retrying in 5s\n";
    let trace = "thread 'main' panicked at src/lib.rs:42:5:\nindex out of bounds: the len is 3 but the index is 7\nstack backtrace:\n   0: rust_begin_unwind\n   1: core::panicking::panic_fmt\n   2: app::run\n   3: app::main\n";
    let py = "Traceback (most recent call last):\n  File \"app.py\", line 12, in <module>\n    main()\n  File \"app.py\", line 8, in main\n    raise ValueError(\"bad input\")\nValueError: bad input\n";
    let email = "From: dana@example.test\nTo: me@example.test\nSubject: Re: Q1 timing\n\nHi,\n\nThanks for the note. We cannot start before March because the team is committed through February, but we could do a scoping call on the 4th. Let me know if that works and I will send an invite. Also, legal asked whether the data clause covers backups; I said probably but I will confirm.\n\nBest,\nDana\n";
    let web = format!("Queues are a fundamental building block of distributed systems. {} In practice, teams choose between a log-structured broker and a task queue. {}", words(50), words(60));
    let json = "{\n  \"name\": \"svc\",\n  \"version\": \"1.2.3\",\n  \"deps\": [\"a\", \"b\", \"c\"],\n  \"limits\": {\"cpu\": 2, \"mem\": 512}\n}\n";
    let diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@\n fn a() {}\n+fn b() {}\n fn c() {}\n";
    let table = "id | name | status\n1 | alpha | open\n2 | beta | closed\n3 | gamma | open\n4 | delta | closed\n";
    let csv = "id,name,qty,price\n1,widget,10,2.50\n2,gadget,3,19.99\n3,gizmo,7,4.25\n4,doohickey,1,99.00\n";
    let shell = "$ cargo build\n   Compiling foo v0.1.0\nerror[E0308]: mismatched types\n  --> src/main.rs:4:13\n   |\n4  |     let x: u32 = \"a\";\n   |            ---   ^^^ expected `u32`, found `&str`\n";
    let sql = "SELECT id, name\nFROM users\nWHERE created_at > '2026-01-01'\nORDER BY name;\n";
    let html = "<div class=\"card\">\n  <h2>Title</h2>\n  <p>Body text</p>\n</div>\n";
    let long_doc = format!("{}.\n\n{}.\n\n{}.", words(90), words(90), words(90));
    let quote_block = format!("> {}\n> {}\n> {}\n", words(14), words(14), words(14));
    let mut v: Vec<(&'static str, String)> = vec![
        ("log, 5 lines", log.into()),
        ("log with intro", format!("This keeps failing:\n\n{log}")),
        ("rust panic", trace.into()),
        ("python traceback", py.into()),
        ("compiler error with intro", format!("What does this mean?\n\n{shell}")),
        ("email with headers", email.into()),
        ("email body only", email.split("\n\n").nth(2).unwrap().to_string() + "\n\n" + email.split("\n\n").nth(3).unwrap()),
        ("web text 600+ chars", web.clone()),
        ("web text with intro", format!("Summarize this:\n\n{web}")),
        ("json object", json.into()),
        ("json with intro", format!("Is this valid?\n\n{json}")),
        ("diff", diff.into()),
        ("pipe table", table.into()),
        ("csv", csv.into()),
        ("sql", sql.into()),
        ("html", html.into()),
        ("long document 3 paragraphs", long_doc.clone()),
        ("long document with intro", format!("Edit this for tone:\n\n{long_doc}")),
        ("quoted block", quote_block),
        ("fenced code", "Here is the file:\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n".into()),
        ("fenced log", format!("```\n{log}```\n")),
        ("single 1,500-char key-like run", "Z9xQ7vL2mP4tR8wK3nB6yH1cD5fG0jS".repeat(48)),
        ("long hex dump", "deadbeefcafebabe0123456789abcdef".repeat(10)),
        ("base64 blob", "QUJDREVGR0hJSktMTU5PUFFSU1RVVldYWVowMTIzNDU2Nzg5".repeat(8)),
        ("yaml file", "services:\n  web:\n    image: app:1.2\n    ports:\n      - \"8080:80\"\n    env:\n      LOG: debug\n".into()),
        ("toml file", "[package]\nname = \"x\"\nversion = \"0.1.0\"\n[dependencies]\nserde = \"1\"\n".into()),
        ("env file", "HOST=localhost\nPORT=5432\nMODE=dev\nREGION=eu\n".into()),
        ("short two-line error", "error: could not find `Cargo.toml` in `/tmp` or any parent directory".into()),
        ("short single-line log", "2026-11-12 10:00:09 ERROR connection reset by peer".into()),
        ("short pasted sentence from a page", "Queues decouple producers from consumers.".into()),
        ("meeting notes pasted, 8 lines", "Attendees: A, B, C\nDecision: ship Friday\nAction: A writes memo\nAction: B reviews\nRisk: vendor delay\nNext: sync Wednesday\nNotes: none\nEnd\n".into()),
        ("chat transcript pasted", "Dana: can we start in March?\nMe: probably, let me check\nDana: great\nMe: yes the 4th works\nDana: sending an invite\n".into()),
    ];
    v.truncate(34);
    v
}

fn run(label_typed: bool, typing: Typing, set: &[(&'static str, String)]) -> (usize, Vec<&'static str>) {
    let mut wrong = vec![];
    for (name, text) in set {
        let s = scrub(KEY, &Input { text, marked: &[], typing, paste_chars: DEFAULT_PASTE_CHARS });
        let hashed = !s.pasted.is_empty();
        if label_typed {
            // False-paste: the operator's own words were hashed.
            if hashed {
                wrong.push(*name);
            }
        } else {
            // False-typed: nothing was hashed AND some line of the pasted text is still in the stored text (a block the redactor
            // masked, such as a long hex dump, leaks nothing and is not counted).
            let survives = text.lines().map(str::trim).filter(|l| l.chars().count() >= 10).any(|l| s.text.contains(l));
            if !hashed && survives {
                wrong.push(*name);
            }
        }
    }
    (wrong.len(), wrong)
}

#[test]
fn the_fixture_set_has_at_least_sixty_labelled_prompts() {
    assert!(typed().len() + pasted().len() >= 60, "{} typed + {} pasted", typed().len(), pasted().len());
}

#[test]
fn measured_error_rates_stay_within_the_ceilings() {
    let (t, p) = (typed(), pasted());
    let (fp_n, fp) = run(true, Typing::NotSeen, &t);
    let (ft_n, ft) = run(false, Typing::NotSeen, &p);
    let (fp_typed_n, _) = run(true, Typing::Typed, &t);
    let (ft_typed_n, ft_typed) = run(false, Typing::Typed, &p);
    eprintln!("not seen (today): false-paste {fp_n}/{} = {:.0}%  false-typed {ft_n}/{} = {:.0}%", t.len(), 100.0 * fp_n as f64 / t.len() as f64, p.len(), 100.0 * ft_n as f64 / p.len() as f64);
    eprintln!("  false-paste: {fp:?}\n  false-typed: {ft:?}");
    eprintln!("typed (daemon saw it typed): false-paste {fp_typed_n}/{}  false-typed {ft_typed_n}/{}  ({ft_typed:?})", t.len(), p.len());
    // Ceilings: the rate measured when S7 was written. A change to the rule that makes either worse fails here.
    assert!(ft_n <= FALSE_TYPED_CEILING, "false-typed rose to {ft_n}: {ft:?}");
    assert!(fp_n <= FALSE_PASTE_CEILING, "false-paste rose to {fp_n}: {fp:?}");
}

const FALSE_TYPED_CEILING: usize = 4; // of 32; was 11 of 32 before the S7 line shapes
const FALSE_PASTE_CEILING: usize = 5; // of 32; unchanged by S7
