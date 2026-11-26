//! `delphi bridge setup claude` (`specs/143` §2.9, slice G): give the single-turn bridge its subscription token.
//!
//! At a terminal it says first what the bridge spends (the operator's own subscription quota, shared with their own use) and that
//! heavy or unattended use belongs on an API key (§3, §2.5). It then asks the operator to run `claude setup-token` themselves, takes
//! the token with hidden input, and runs **one test call** through the bridge's own launch (`harness_single::launch`: a home built
//! empty for the call, tools off, the token in the child's environment only). Only a token that answered is stored: in the keychain,
//! under service `delphi` and account [`ENTRY`], so that the reference [`REFERENCE`] names it. The reference, never the token, is
//! written to `[bridge.claude] credential` in `config.toml`, and the change is shown for the operator to ratify like any
//! configuration.
//!
//! Without a terminal it asks nothing and stores nothing: it takes `--credential` with a reference (`keychain:name`, `env:NAME`,
//! `command:[…]`), runs the test with the token that reference names, writes the reference, and does not ratify. Inside an agent seat
//! it is refused whatever the flags.
//!
//! Delphi never reads the operator's own CLI login, never copies a credential file and never runs the test in the operator's own
//! configuration home (§2.9 item 2): the test call is the bridge's launch, nothing else.
//!
//! Everything that touches the world goes through [`Ui`], [`Env`] and a [`Tester`], so the flow is tested with scripted answers, a
//! fake keychain and a fake call.

use crate::setup::{Env, Ui};
use anyhow::{anyhow, bail, Result};
use delphi_adapter::credential::{CredentialRef, Secret};
use std::path::{Path, PathBuf};
use toml_edit::{value, DocumentMut, Item, Table};

/// The keychain account the token is kept under (service `delphi`).
pub const ENTRY: &str = "bridge-claude";
/// The reference written to `[bridge.claude] credential`.
pub const REFERENCE: &str = "keychain:bridge-claude";
/// The model the test call asks for: the cheapest alias the CLI knows.
pub const TEST_MODEL: &str = "haiku";
/// The test call's prompt: public words, the same on every install.
pub const TEST_PROMPT: &str = "Reply with the single word: ready";

const USAGE: &str = "usage: delphi bridge setup claude [--credential <keychain:name|env:NAME|command:[...]>] [--no-test] [--home <dir>] [--workspace <name>]";

/// §3: said before a token is asked for, and in the documentation wherever the bridge is described.
pub const QUOTA_NOTICE: &str = "The bridge lets Delphi's background jobs (topic pages, their validator, summaries, the scout's own calls) use your Claude subscription through your own `claude` CLI, one prompt and one answer with its tools off. Every bridged call spends your own subscription quota, the same quota your own use of Claude draws on. It is for you alone, on this machine. For heavy or unattended use, add an API-key endpoint instead (an [[endpoints]] entry with path = \"api\"): the vendor asks that products use API keys, and an API key does not share your plan's limits. You can keep every bridged job manual with [bridge.jobs].";

/// One test call with `token`: the first words of the answer, or why it failed.
pub type Tester<'a> = &'a dyn Fn(&Secret) -> Result<String, String>;

pub struct Opts {
    pub home: PathBuf,
    pub ws_dir: PathBuf,
    /// A reference to where the token is kept (`--credential`); never the token itself.
    pub credential: Option<String>,
    /// No terminal: nothing is asked, nothing is stored, nothing is ratified.
    pub from_flags: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// The reference written to `[bridge.claude] credential`; `None` when nothing was written.
    pub reference: Option<String>,
    /// `None`: not run (`--no-test`).
    pub test_ok: Option<bool>,
    pub stored: bool,
    pub ratified: bool,
}

/// Write `credential = <reference>` under `[bridge.claude]`, keeping the rest of the file as it is.
fn write_reference(doc: &mut DocumentMut, reference: &str) {
    let bridge = doc.entry("bridge").or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    let Some(bridge) = bridge.as_table_mut() else { return };
    let claude = bridge.entry("claude").or_insert_with(|| Item::Table(Table::new()));
    if let Some(t) = claude.as_table_mut() {
        t["credential"] = value(reference);
    }
}

/// Show the configuration change and ratify it on the operator's yes. Without a terminal nothing is ratified.
fn ratify(ui: &mut dyn Ui, home: &Path, ws_dir: &Path, ask: bool) -> bool {
    use delphi_engine::ratify;
    let pending: Vec<_> = ratify::list(home, ws_dir).into_iter().filter(|p| p.logical.starts_with("config.toml")).collect();
    let mut all = true;
    for p in pending {
        ui.say(&format!("\nconfig.toml changed. This is what you are about to trust ({}):\n{}", p.summary, p.diff));
        if !ask {
            ui.say("  Not ratified: that is yours to do. Review it and run `/ratify` in the console, or run this command again at a terminal.");
            all = false;
        } else if ui.yes_no("Ratify this (the bridge may then serve single model calls with this token)?", true) {
            match p.live_blake3.as_deref().map(|h| ratify::apply(home, ws_dir, &p.id, h)) {
                Some(Ok(_)) => ui.say("  Ratified."),
                other => {
                    ui.say(&format!("  Could not ratify: {:?}", other.map(|r| r.map(|_| ()))));
                    all = false;
                }
            }
        } else {
            ui.say("  Not ratified; it stays a pending proposal (`/ratify` in the console).");
            all = false;
        }
    }
    all
}

/// The flow. `resolve` reads the token a reference names (for `--credential`); `test` is `None` with `--no-test`.
pub fn run(ui: &mut dyn Ui, env: &dyn Env, resolve: &dyn Fn(&str) -> Option<Secret>, test: Option<Tester>, o: &Opts) -> Result<Outcome> {
    ui.say("Delphi bridge setup: claude\n");
    ui.say(QUOTA_NOTICE);
    let cfg_path = o.ws_dir.join("config.toml");
    let text = std::fs::read_to_string(&cfg_path).map_err(|e| anyhow!("{}: {}", cfg_path.display(), e))?;
    let mut doc: DocumentMut = text.parse().map_err(|e| anyhow!("config.toml is not valid TOML: {}", e))?;

    // 1. The token: a reference from a flag, or typed here, hidden.
    let mut out = Outcome::default();
    let (reference, token, typed) = if let Some(c) = &o.credential {
        match CredentialRef::parse(c) {
            Ok(CredentialRef::Env(_)) | Ok(CredentialRef::Keychain(_)) | Ok(CredentialRef::Command(_)) => {}
            Ok(_) => bail!("--credential takes a reference to where the token is kept: keychain:name, env:NAME or command:[...]"),
            // The value is not repeated: what was passed may be the token itself.
            Err(_) => bail!("--credential takes a reference such as {REFERENCE}, never the token itself"),
        }
        (c.clone(), resolve(c), false)
    } else if o.from_flags {
        bail!("without a terminal, pass --credential with a reference to where the token is kept (never the token itself)\n{USAGE}");
    } else {
        ui.say("\nIn another terminal, run `claude setup-token` yourself and sign in. It prints a long-lived token for your subscription. Delphi does not run it for you and never reads your own `claude` login.");
        let t = ui.secret("Paste the token here (it is not shown)");
        let t = t.trim();
        if t.len() < 20 || t.chars().any(char::is_whitespace) {
            ui.say("  That does not look like a token; nothing was saved.");
            return Ok(out);
        }
        (REFERENCE.to_string(), Some(Secret::new(t.to_string(), None)), true)
    };

    // 2. One test call through the bridge's own launch. A token typed here is tested before it is stored.
    if let Some(test) = test {
        let Some(token) = token.as_ref() else {
            ui.say(&format!("  ✗ test call: `{reference}` names no token that can be read; nothing was written."));
            out.test_ok = Some(false);
            return Ok(out);
        };
        ui.say("\nOne test call (a few thousand tokens of your quota)…");
        match test(token) {
            Ok(words) => {
                ui.say(&format!("  ✓ test call: claude answered ({} characters)", words.chars().count()));
                out.test_ok = Some(true);
            }
            Err(why) => {
                ui.say(&format!("  ✗ test call: {why}"));
                ui.say("  The token was not stored and config.toml was not changed.");
                out.test_ok = Some(false);
                return Ok(out);
            }
        }
    }

    // 3. The token in the keychain, never in a file.
    if typed {
        let t = token.as_ref().expect("typed");
        env.secret_set(ENTRY, t.expose()).map_err(|e| anyhow!("could not store the token in your keychain: {}", e))?;
        out.stored = true;
        ui.say("  The token is stored in your keychain (service delphi, account bridge-claude), not in config.toml.");
    }

    // 4. The reference in config.toml, checked by the daemon's own parser before it is saved.
    write_reference(&mut doc, &reference);
    let written = doc.to_string();
    delphi_adapter::config::Config::parse(&written).map_err(|e| anyhow!("the change would not parse: {}", e.join("; ")))?;
    std::fs::write(&cfg_path, written)?;
    ui.say(&format!("  [bridge.claude] credential = \"{reference}\""));
    out.reference = Some(reference);

    // 5. The change, for the operator to ratify.
    // Nothing pending reads as ratified at a terminal; from flags nothing is ratified, whatever is pending.
    out.ratified = ratify(ui, &o.home, &o.ws_dir, !o.from_flags) && !o.from_flags;
    let test = match out.test_ok {
        Some(true) => "worked",
        Some(false) => "failed",
        None => "not run",
    };
    ui.say(&format!("\nDone. Test call: {test}. {}", if out.ratified { "Ratified." } else { "Not ratified yet." }));
    Ok(out)
}

/// The test call the command makes: the bridge's own launch (`harness_single`), one prompt, tools off, the token in the child's
/// environment only, the call's home and working directory removed afterwards. Only the length of the answer is printed.
fn bridge_test(home: &Path, ws_dir: &Path, token: &Secret) -> Result<String, String> {
    use delphi_engine::harness_single as bridge;
    let call = bridge::CallHome::make(&bridge::roots(home), &[ws_dir.to_path_buf()])?;
    let launch = bridge::launch("claude", TEST_MODEL, TEST_PROMPT.into(), &call, Some(token.clone()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
    rt.block_on(bridge::run(&launch, std::time::Duration::from_secs(90))).map(|a| a.text).map_err(|e| e.to_string())
}

/// `delphi bridge …` from the command line. Exit codes: 0 set up · 1 the test failed or nothing was saved · 2 usage or refusal.
pub fn cli(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    use std::io::IsTerminal;
    if args.first().map(String::as_str) != Some("setup") || args.get(1).map(String::as_str) != Some("claude") {
        eprintln!("delphi bridge: only `setup claude` exists so far\n{USAGE}");
        return 2;
    }
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi bridge setup: refused inside an agent seat; giving the bridge a subscription token is the operator's act, at a terminal");
        return 2;
    }
    let known = ["--credential", "--home", "--workspace"];
    let mut skip = false;
    for a in args.iter().skip(2) {
        if skip {
            skip = false;
        } else if known.contains(&a.as_str()) {
            skip = true;
        } else if a != "--no-test" {
            eprintln!("delphi bridge setup: unknown argument `{a}`\n{USAGE}");
            return 2;
        }
    }
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let o = Opts { home: home.to_path_buf(), ws_dir: ws_dir.to_path_buf(), credential: crate::flag(args, "--credential"), from_flags: !tty };
    if !tty && o.credential.is_none() {
        eprintln!("delphi bridge setup: needs a terminal (it takes the token with hidden input). Without one, pass --credential with a reference to where the token is kept.\n{USAGE}");
        return 2;
    }
    let resolve = |c: &str| -> Option<Secret> {
        use delphi_adapter::credential::{Resolved, Resolver};
        match Resolver::system(std::sync::Arc::new(|_| {})).resolve(&CredentialRef::parse(c).ok()?) {
            Ok(Resolved::Token(t)) => Some(t),
            _ => None,
        }
    };
    let tester = |t: &Secret| bridge_test(home, ws_dir, t);
    let test: Option<Tester> = if args.iter().any(|a| a == "--no-test") { None } else { Some(&tester) };
    match run(&mut crate::setup::TerminalUi { auto: !tty }, &crate::setup::RealEnv, &resolve, test, &o) {
        Ok(out) if out.reference.is_some() && out.test_ok != Some(false) => 0,
        Ok(_) => 1,
        Err(e) => {
            eprintln!("delphi bridge setup: {:#}", e);
            2
        }
    }
}

// ── passing a limit: the typed confirmation (`specs/143` §2.8 item 3, slice F) ─────────────────────────────────────────────────

/// The words the confirmation picks from: short, plain, unlike one another.
const WORDS: [&str; 16] = ["amber", "basil", "cedar", "delta", "ember", "fjord", "garnet", "harbor", "indigo", "juniper", "kestrel", "lantern", "meadow", "nutmeg", "orchid", "pebble"];

/// A word of [`WORDS`] chosen from the system's randomness; `None` when there is none to read.
pub fn random_word() -> Option<&'static str> {
    use std::io::Read;
    let mut b = [0u8; 1];
    std::fs::File::open("/dev/urandom").ok()?.read_exact(&mut b).ok()?;
    Some(WORDS[(b[0] as usize) % WORDS.len()])
}

/// One line of what a credential has spent today against the daily cap, from a preflight's `usage`.
pub fn usage_line(u: &serde_json::Value) -> String {
    format!("Today through the bridge: {} of {} calls, {} of {} tokens.", u["calls_today"], u["daily_calls"], u["tokens_today"], u["daily_tokens"])
}

/// The confirmation, pure: it passes only when stdin and stdout are a terminal (`terminal`), nothing in the process says an agent
/// seat started it (`refusal`), and `word` is typed back exactly through `ask` (which reads the controlling terminal). No flag, no
/// environment variable and no piped input stands in for it. `what` says what will be spent.
pub fn confirm(what: &str, word: &str, terminal: bool, refusal: Option<String>, ask: &mut dyn FnMut(&str) -> Option<String>) -> Result<(), String> {
    if !terminal {
        return Err("passing a limit needs the confirmation typed at a terminal, and this command's input or output is not one; it stays within the limits".into());
    }
    if let Some(why) = refusal {
        return Err(format!("passing a limit is the operator's act: {why}"));
    }
    match ask(&format!("{what}\nTo pass the limit, type the word `{word}`: ")) {
        Some(typed) if typed.trim() == word => Ok(()),
        Some(_) => Err("the word typed was not the one asked for; nothing was run".into()),
        None => Err("no answer was typed; nothing was run".into()),
    }
}

/// [`confirm`] at the operator's real terminal: stdin and stdout must be one, the process must not be inside an agent seat (its
/// variables or its ancestry), and the word is read from `/dev/tty`.
pub fn confirm_at_terminal(what: &str) -> Result<(), String> {
    use std::io::IsTerminal;
    let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let refusal = if crate::xenia_terminal::inside_an_agent_seat() { Some("this process runs inside an agent seat".to_string()) } else { crate::run_granted::seat_refusal() };
    let word = random_word().ok_or("no randomness to choose the confirmation word with")?;
    confirm(what, word, terminal, refusal, &mut |prompt| crate::run_granted::tty_ask(prompt))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const TOKEN: &str = "sk-ant-oat01-fixture-bridge-token-0001";

    #[derive(Default)]
    struct Keychain {
        stored: RefCell<Vec<(String, String)>>,
    }
    impl Env for Keychain {
        fn run(&self, _: &str, _: &[&str]) -> Option<(i32, String)> {
            None
        }
        fn run_tty(&self, _: &str, _: &[&str]) -> bool {
            false
        }
        fn secret_set(&self, name: &str, secret: &str) -> Result<(), String> {
            self.stored.borrow_mut().push((name.into(), secret.into()));
            Ok(())
        }
        fn http_get(&self, _: &str, _: &[(&str, &str)]) -> Result<(u16, String), String> {
            Err("no network in a test".into())
        }
    }

    #[derive(Default)]
    struct Script {
        said: Vec<String>,
        secret: String,
        asked_secret: usize,
    }
    impl Ui for Script {
        fn say(&mut self, text: &str) {
            self.said.push(text.to_string());
        }
        fn yes_no(&mut self, _: &str, default: bool) -> bool {
            default
        }
        fn line(&mut self, _: &str, default: &str) -> String {
            default.to_string()
        }
        fn secret(&mut self, _: &str) -> String {
            self.asked_secret += 1;
            self.secret.clone()
        }
        fn open_url(&mut self, _: &str) {}
    }

    fn workspace() -> (tempfile::TempDir, Opts) {
        let d = tempfile::tempdir().unwrap();
        let ws = d.path().join("workspaces/w");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("config.toml"), "# my settings\n[routing]\nsingle_call_harness = \"auto\"\n").unwrap();
        let o = Opts { home: d.path().to_path_buf(), ws_dir: ws, credential: None, from_flags: false };
        (d, o)
    }

    fn no_resolve(_: &str) -> Option<Secret> {
        None
    }

    // AC-19: the confirmation passes only at a terminal, outside a seat, with the exact word; a wrong word, no answer, no terminal or
    // a seat each refuse, and nothing is asked when it cannot pass.
    #[test]
    fn the_typed_confirmation_passes_only_with_the_word_at_a_terminal_outside_a_seat() {
        let asked = RefCell::new(0);
        let answer = |a: Option<&str>| {
            let a = a.map(String::from);
            let asked = &asked;
            move |_: &str| {
                *asked.borrow_mut() += 1;
                a.clone()
            }
        };
        assert!(confirm("spend", "cedar", true, None, &mut answer(Some(" cedar\n"))).is_ok());
        assert!(confirm("spend", "cedar", true, None, &mut answer(Some("yes"))).unwrap_err().contains("not the one"));
        assert!(confirm("spend", "cedar", true, None, &mut answer(None)).unwrap_err().contains("no answer"));
        assert_eq!(*asked.borrow(), 3);
        assert!(confirm("spend", "cedar", false, None, &mut answer(Some("cedar"))).unwrap_err().contains("not one"));
        assert!(confirm("spend", "cedar", true, Some("refused: an agent seat".into()), &mut answer(Some("cedar"))).unwrap_err().contains("operator's act"));
        assert_eq!(*asked.borrow(), 3, "nothing is asked when it cannot pass");
        assert!(WORDS.contains(&random_word().unwrap()));
    }

    // AC-21 and §2.9 item 1: at a terminal the notice comes first, the token is taken hidden, tested, kept in the keychain under the
    // account the reference names, and only the reference is written; the rest of config.toml is kept; the token is said nowhere.
    #[test]
    fn at_a_terminal_the_token_is_tested_then_kept_in_the_keychain_and_only_its_reference_is_written() {
        let (_d, o) = workspace();
        let kc = Keychain::default();
        let mut ui = Script { secret: format!("  {TOKEN}\n"), ..Default::default() };
        let seen = RefCell::new(Vec::new());
        let test = |t: &Secret| {
            seen.borrow_mut().push(t.expose().to_string());
            Ok("ready".to_string())
        };
        let out = run(&mut ui, &kc, &no_resolve, Some(&test), &o).unwrap();
        assert_eq!((out.reference.as_deref(), out.test_ok, out.stored), (Some(REFERENCE), Some(true), true));
        assert_eq!(seen.borrow().as_slice(), [TOKEN.to_string()], "the typed token is the one tested");
        assert_eq!(kc.stored.borrow().as_slice(), [(ENTRY.to_string(), TOKEN.to_string())]);
        assert_eq!(ui.said[1], QUOTA_NOTICE, "the notice comes before the token is asked for");
        assert!(ui.said.iter().any(|s| s.contains("claude setup-token")));
        let cfg = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        assert!(cfg.starts_with("# my settings\n[routing]\nsingle_call_harness = \"auto\"\n"), "{cfg}");
        let parsed = delphi_adapter::config::Config::parse(&cfg).unwrap();
        assert_eq!(parsed.bridge.claude_credential.as_deref(), Some(REFERENCE));
        assert!(!cfg.contains(TOKEN) && !ui.said.join("\n").contains(TOKEN), "the token is in no file and on no screen");
    }

    // A token the test call refuses is not stored and nothing is written; something that is not a token is not even tested.
    #[test]
    fn a_token_that_fails_its_test_or_is_no_token_is_neither_stored_nor_written() {
        let (_d, o) = workspace();
        let before = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        let kc = Keychain::default();
        let fails = |_: &Secret| Err("the login did not hold (login)".to_string());
        let mut ui = Script { secret: TOKEN.into(), ..Default::default() };
        let out = run(&mut ui, &kc, &no_resolve, Some(&fails), &o).unwrap();
        assert_eq!((out.reference, out.test_ok, out.stored), (None, Some(false), false));
        let called = RefCell::new(0);
        let counts = |_: &Secret| {
            *called.borrow_mut() += 1;
            Ok("ready".to_string())
        };
        let mut ui = Script { secret: "yes please".into(), ..Default::default() };
        let out = run(&mut ui, &kc, &no_resolve, Some(&counts), &o).unwrap();
        assert_eq!((out.reference, out.stored, *called.borrow()), (None, false, 0));
        assert!(kc.stored.borrow().is_empty());
        assert_eq!(std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap(), before);
    }

    // Without a terminal: a reference is required, the token it names is tested, nothing is asked, stored or ratified; a value that
    // is not a reference is refused, so a token can never be passed as a flag.
    #[test]
    fn without_a_terminal_only_a_reference_is_taken_and_nothing_is_asked_or_stored() {
        let (_d, mut o) = workspace();
        o.from_flags = true;
        let kc = Keychain::default();
        let ok = |_: &Secret| Ok("ready".to_string());
        let mut ui = Script::default();
        assert!(run(&mut ui, &kc, &no_resolve, Some(&ok), &o).is_err(), "no reference");
        o.credential = Some(TOKEN.into());
        assert!(run(&mut ui, &kc, &no_resolve, Some(&ok), &o).is_err(), "a token is not a reference");
        o.credential = Some("env:BRIDGE_TOKEN".into());
        let resolve = |c: &str| (c == "env:BRIDGE_TOKEN").then(|| Secret::new(TOKEN.into(), None));
        let out = run(&mut ui, &kc, &resolve, Some(&ok), &o).unwrap();
        assert_eq!((out.reference.as_deref(), out.test_ok, out.stored, out.ratified), (Some("env:BRIDGE_TOKEN"), Some(true), false, false));
        assert_eq!(ui.asked_secret, 0);
        assert!(kc.stored.borrow().is_empty());
        // A reference that names nothing readable fails the test and writes nothing.
        o.credential = Some("env:NOT_SET".into());
        let (_d2, mut o2) = workspace();
        o2.from_flags = true;
        o2.credential = o.credential.clone();
        let out = run(&mut ui, &kc, &resolve, Some(&ok), &o2).unwrap();
        assert_eq!((out.reference, out.test_ok), (None, Some(false)));
    }
}
