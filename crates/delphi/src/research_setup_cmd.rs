//! `delphi research setup` (`specs/119` §2 items 1, 2 and 6; slice B1): choose the search engine research asks, and prove it works.
//!
//! It asks which engine (Brave is suggested first and Mojeek second; any engine the code supports is taken), says once that
//! queries leave this machine for that engine, takes the key with hidden input straight into the keychain as `delphi setup` takes
//! a model key (never a file, never the screen), writes the `[search]` table of the workspace's `config.toml`, runs one test query
//! and one test fetch from the trusted list, and prints what worked. `[search]` is one of the ratified access blocks, because it
//! names who receives queries: the change is shown and takes effect only when the operator ratifies it.
//!
//! Without a terminal it asks nothing and needs its answers as flags: `--engine`, and for an engine with a key `--credential`
//! with a *reference* (`env:NAME`, `keychain:name`), never the key itself. It then writes the table, runs the test, and does not
//! ratify. Inside an agent seat it is refused whatever the flags (§11: no agent can run it).
//!
//! Everything that touches the world goes through [`Ui`], [`Env`] and [`Net`], so the flow is tested with scripted answers, a fake
//! keychain and a fake engine.
//!
//! Not here yet (later slices, asked after the engine is chosen and before the summary): the question about how much text of a
//! source of unknown licence is kept (§3, B2) and the defaults of §4 (B3).

use crate::setup::{Env, Ui};
use anyhow::{anyhow, bail, Result};
use delphi_adapter::credential::CredentialRef;
use delphi_adapter::research::search::{self, Provider, ProviderSpec, SearchError};
use delphi_adapter::research::transport::Transport;
use delphi_adapter::research::{fetch, SearchConfig};
use delphi_engine::research_setup::{engine_state_in, EngineState, SETUP_COMMAND};
use std::path::{Path, PathBuf};
use toml_edit::{value, Array, DocumentMut, Item, Table};

/// The order engines are offered in (§2 item 6: Brave first, Mojeek second; operator, 2026-11-14).
pub const OFFERED: [Provider; 6] = [Provider::Brave, Provider::Mojeek, Provider::Searxng, Provider::Kagi, Provider::Google, Provider::Parallel];
/// The test query: public words, the same on every install.
const TEST_QUERY: &str = "encyclopedia open access";

const USAGE: &str = "usage: delphi research setup [--engine brave|mojeek|searxng|kagi|google|parallel] [--credential <env:NAME|keychain:name>] [--url <searxng instance>] [--cx <google engine id>] [--no-test] [--home <dir>] [--workspace <name>]";

/// The network, for the two tests.
pub struct Net<'a> {
    pub transport: &'a dyn Transport,
    /// The key a credential reference names, or `None` when it cannot be read.
    pub resolve: &'a dyn Fn(&str) -> Option<String>,
}

pub struct Opts {
    pub home: PathBuf,
    pub ws_dir: PathBuf,
    pub engine: Option<String>,
    pub credential: Option<String>,
    pub url: Option<String>,
    pub cx: Option<String>,
    /// No terminal: nothing is asked, every answer is a flag, and nothing is ratified.
    pub from_flags: bool,
}

#[derive(Debug, Default, PartialEq)]
pub struct Outcome {
    /// The engine written to `[search]`; `None` when nothing was written.
    pub engine: Option<String>,
    pub ratified: bool,
    /// `None`: not run (`--no-test`).
    pub search_ok: Option<bool>,
    pub fetch_ok: Option<bool>,
}

/// §2 item 6: said before any key is entered and before any query is sent.
pub fn notice(engine: &str) -> String {
    format!("Queries leave this machine for the engine chosen ({engine}). Check its terms and your employer's rules first.")
}

fn title(p: Provider) -> &'static str {
    match p {
        Provider::Brave => "Brave Search",
        Provider::Mojeek => "Mojeek",
        Provider::Searxng => "SearXNG",
        Provider::Kagi => "Kagi",
        Provider::Google => "Google Programmable Search",
        Provider::Parallel => "Parallel",
    }
}

fn about(p: Provider) -> &'static str {
    match p {
        Provider::Brave => "its own index; needs a key (suggested)",
        Provider::Mojeek => "its own index; needs a key",
        Provider::Searxng => "an instance you name; no key, and no vendor sees your queries but the instance",
        Provider::Kagi => "needs a key",
        Provider::Google => "needs a key and an engine id, and only with a Gemini endpoint configured",
        Provider::Parallel => "hosted, no key or account, rate limited",
    }
}

/// The line `delphi init` and `delphi connect` end with when the operator has chosen no engine (§2 item 2); `None` when one is chosen
/// or the config cannot be read.
pub fn hint(ws_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(ws_dir.join("config.toml")).ok()?;
    match engine_state_in(&text)? {
        EngineState::Chosen => None,
        EngineState::None => Some(format!("  research: no search engine is set up, so research cannot search the web. Run `{SETUP_COMMAND}`.")),
        EngineState::DefaultOnly => Some(format!("  research: no search engine of your own is set up (the keyless default, parallel, is asked: rate limited, and it sees your queries). Run `{SETUP_COMMAND}` to choose one.")),
    }
}

/// `[search]` in `doc` with `p` first, the engines already written after it, and `p`'s own settings.
fn write_search(doc: &mut DocumentMut, p: Provider, credential: Option<&str>, url: Option<&str>, cx: Option<&str>) {
    if !doc.get("search").is_some_and(|s| s.is_table()) {
        doc["search"] = Item::Table(Table::new());
    }
    let search = doc["search"].as_table_mut().expect("search is a table");
    let mut names = Array::new();
    names.push(p.name());
    for old in search.get("providers").and_then(|v| v.as_array()).into_iter().flatten().filter_map(|v| v.as_str()) {
        if old != p.name() {
            names.push(old);
        }
    }
    search["providers"] = value(names);
    let settings: Vec<(&str, &str)> = [("credential", credential), ("url", url), ("cx", cx)].into_iter().filter_map(|(k, v)| v.map(|v| (k, v))).collect();
    if !settings.is_empty() {
        if !search.get(p.name()).is_some_and(|s| s.is_table()) {
            search[p.name()] = Item::Table(Table::new());
        }
        for (k, v) in settings {
            search[p.name()][k] = value(v);
        }
    }
}

/// What `SearchConfig::parse` says of the `[search]` table in `doc`.
fn check(doc: &DocumentMut) -> Result<SearchConfig> {
    let root: toml::Value = toml::from_str(&doc.to_string()).map_err(|e| anyhow!("config.toml would not be valid TOML: {}", e))?;
    let gemini = root.get("endpoints").and_then(|e| e.as_array()).is_some_and(|a| a.iter().any(|e| e.get("family").and_then(|f| f.as_str()) == Some("gemini")));
    let table = root.get("search").ok_or_else(|| anyhow!("no [search] table was written"))?;
    SearchConfig::parse(table, gemini).map_err(|errs| anyhow!("{}", errs.join("; ")))
}

fn search_failure(e: &SearchError) -> String {
    match e {
        SearchError::NotConfigured => "no engine".into(),
        SearchError::AllFailed(why) => why.iter().map(|(p, w)| format!("{}: {}", p.name(), w)).collect::<Vec<_>>().join("; "),
    }
}

/// Show the access change and ratify it on the operator's yes. Without a terminal nothing is ratified.
fn ratify(ui: &mut dyn Ui, home: &Path, ws_dir: &Path, ask: bool) -> bool {
    use delphi_engine::ratify;
    let pending: Vec<_> = ratify::list(home, ws_dir).into_iter().filter(|p| p.logical.starts_with("config.toml")).collect();
    let mut all = true;
    for p in pending {
        ui.say(&format!("\nWho receives your queries changed. This is what you are about to trust ({}):\n{}", p.summary, p.diff));
        if !ask {
            ui.say("  Not ratified: that is yours to do. Review it and run `/ratify` in the console, or run this command again at a terminal.");
            all = false;
        } else if ui.yes_no("Ratify this (research will send queries only to the engines listed here)?", true) {
            match p.live_blake3.as_deref().map(|h| ratify::apply(home, ws_dir, &p.id, h)) {
                Some(Ok(_)) => ui.say("  Ratified."),
                other => {
                    ui.say(&format!("  Could not ratify: {:?}", other.map(|r| r.map(|_| ()))));
                    all = false;
                }
            }
        } else {
            ui.say("  Not ratified; it stays a pending proposal (`/ratify` in the console). Until then the last ratified [search] stays in force.");
            all = false;
        }
    }
    all
}

pub fn run(ui: &mut dyn Ui, env: &dyn Env, net: Option<&Net>, o: &Opts) -> Result<Outcome> {
    ui.say("Delphi research setup\n");
    let cfg_path = o.ws_dir.join("config.toml");
    let text = std::fs::read_to_string(&cfg_path).map_err(|e| anyhow!("{}: {}", cfg_path.display(), e))?;
    let mut doc: DocumentMut = text.parse().map_err(|e| anyhow!("config.toml is not valid TOML: {}", e))?;
    match engine_state_in(&text) {
        Some(EngineState::Chosen) => ui.say("  A search engine is already set up; the one you choose now is asked first."),
        Some(EngineState::DefaultOnly) => ui.say("  No engine of your own is set up: the keyless default (parallel) is asked."),
        Some(EngineState::None) => ui.say("  No search engine is set up, so research cannot search the web."),
        None => bail!("[search] in config.toml cannot be read; fix it by hand first (`delphi doctor` says what is wrong)"),
    }

    // 1. Which engine.
    let name = match (&o.engine, o.from_flags) {
        (Some(e), _) => e.clone(),
        (None, true) => bail!("without a terminal, name the engine with --engine\n{USAGE}"),
        (None, false) => {
            ui.say("\nWhich search engine should research ask?");
            for p in OFFERED {
                ui.say(&format!("  {:<9} {}", p.name(), about(p)));
            }
            ui.line("Engine", OFFERED[0].name())
        }
    };
    let name = name.trim().to_lowercase();
    let p = Provider::parse(&name).ok_or_else(|| anyhow!("unknown engine `{}` (expected {})", name, OFFERED.map(|p| p.name()).join(", ")))?;

    // 2. The one-line notice, before a key is entered or a query is sent (§2 item 6).
    ui.say(&format!("\n{}", notice(p.name())));

    // 3. What the engine needs besides a key.
    let ask = |ui: &mut dyn Ui, given: &Option<String>, flag: &str, question: &str| -> Result<String> {
        match (given, o.from_flags) {
            (Some(v), _) => Ok(v.trim().to_string()),
            (None, true) => bail!("without a terminal, {} needs {flag}\n{USAGE}", p.name()),
            (None, false) => Ok(ui.line(question, "").trim().to_string()),
        }
    };
    let url = if p == Provider::Searxng { Some(ask(ui, &o.url, "--url", "The SearXNG instance's address (https://…)")?) } else { None };
    let cx = if p == Provider::Google { Some(ask(ui, &o.cx, "--cx", "The Programmable Search engine id (cx)")?) } else { None };

    // 4. The key: a reference from a flag, or typed here, hidden, and kept in the keychain.
    let entry = format!("{}-search-key", p.name());
    let mut typed: Option<String> = None;
    let credential: Option<String> = if !p.needs_key() {
        None
    } else if let Some(c) = &o.credential {
        match CredentialRef::parse(c) {
            Ok(CredentialRef::Env(_)) | Ok(CredentialRef::Keychain(_)) => Some(c.clone()),
            Ok(_) => bail!("--credential takes a reference to where the key is kept: env:NAME or keychain:name"),
            Err(m) => bail!("--credential: {} (a reference such as env:{}_API_KEY, never the key itself)", m, p.name().to_uppercase()),
        }
    } else if o.from_flags {
        bail!("without a terminal, {} needs --credential <env:NAME|keychain:name> (a reference, never the key itself)\n{USAGE}", p.name());
    } else {
        let key = ui.secret(&format!("{} API key", title(p)));
        let key = key.trim();
        if key.len() < 8 {
            ui.say("  That does not look like a key; nothing was saved.");
            return Ok(Outcome::default());
        }
        typed = Some(key.to_string());
        Some(format!("keychain:{entry}"))
    };

    // 5. The table as it will be written, checked by the daemon's own parser before anything is saved.
    write_search(&mut doc, p, credential.as_deref(), url.as_deref(), cx.as_deref());
    let parsed = check(&doc)?;

    // 6. One test query. A key typed here is tested before it is stored, as a model key is.
    let mut out = Outcome::default();
    let mut rows = Vec::new();
    if let Some(net) = net {
        let key = typed.clone().or_else(|| credential.as_deref().and_then(net.resolve));
        let spec = ProviderSpec { provider: p, key, base_url: url.clone(), cx: cx.clone() };
        match search::search(net.transport, &[spec], TEST_QUERY) {
            Ok(found) => {
                ui.say(&format!("  ✓ test query: {} answered with {} result(s)", p.name(), found.rows.len()));
                rows = found.rows;
                out.search_ok = Some(true);
            }
            Err(e) => {
                ui.say(&format!("  ✗ test query: {} did not answer ({})", p.name(), search_failure(&e)));
                out.search_ok = Some(false);
                if typed.is_some() {
                    ui.say("  The key was not stored and config.toml was not changed.");
                    return Ok(out);
                }
            }
        }
    }
    if let Some(key) = &typed {
        env.secret_set(&entry, key).map_err(|e| anyhow!("could not store the key in your keychain: {}", e))?;
        ui.say("  The key is stored in your keychain, not in config.toml.");
    }
    std::fs::write(&cfg_path, doc.to_string())?;
    out.engine = Some(p.name().to_string());
    ui.say(&format!("  [search] written: providers = [{}]", parsed.providers.iter().map(|p| format!("\"{}\"", p.name())).collect::<Vec<_>>().join(", ")));

    // 7. One test fetch from the trusted list: a result of the test query that is on it, else the first listed site.
    if let Some(net) = net {
        match delphi_engine::research_setup::lists(&o.ws_dir, &o.home) {
            Err(e) => {
                ui.say(&format!("  ✗ test fetch: the trusted list could not be read ({})", e.chars().take(120).collect::<String>()));
                out.fetch_ok = Some(false);
            }
            Ok(list) => {
                let target = rows.iter().map(|r| r.url.clone()).find(|u| list.allows_url(u).is_some()).or_else(|| list.sources().next().map(|s| format!("https://{}/", s.host)));
                match target.as_deref().map(|u| (u, fetch::fetch(net.transport, &list, u))) {
                    Some((_, Ok(page))) => {
                        ui.say(&format!("  ✓ test fetch: {} answered ({} characters)", page.source.host, page.body.chars().count()));
                        out.fetch_ok = Some(true);
                    }
                    Some((u, Err(e))) => {
                        ui.say(&format!("  ✗ test fetch: {} did not answer ({})", u, e));
                        out.fetch_ok = Some(false);
                    }
                    None => {
                        ui.say("  ✗ test fetch: the trusted list is empty, so there is nothing research may read");
                        out.fetch_ok = Some(false);
                    }
                }
            }
        }
    }

    // (§2 item 5, slices B2 and B3: the questions about kept text and the reviewed defaults go here.)

    // 8. The access change, for the operator to ratify.
    out.ratified = ratify(ui, &o.home, &o.ws_dir, !o.from_flags);

    // 9. What worked.
    let word = |b: Option<bool>| match b {
        Some(true) => "worked",
        Some(false) => "failed",
        None => "not run",
    };
    ui.say(&format!("\nDone. Engine: {}. Test query: {}. Test fetch: {}. {}", p.name(), word(out.search_ok), word(out.fetch_ok), if out.ratified { "Ratified: research can search." } else { "Not ratified yet: research keeps the last ratified [search] until you ratify this." }));
    Ok(out)
}

/// `delphi research setup …` from the command line. Exit codes: 0 set up · 1 a test failed or nothing was saved · 2 usage or refusal.
pub fn cli(home: &Path, ws_dir: &Path, args: &[String]) -> i32 {
    use std::io::IsTerminal;
    if crate::xenia_terminal::inside_an_agent_seat() {
        eprintln!("delphi research setup: refused inside an agent seat; choosing who receives queries is the operator's act, at a terminal");
        return 2;
    }
    let known = ["--engine", "--credential", "--url", "--cx", "--home", "--workspace"];
    let mut skip = false;
    for a in args.iter().skip(1) {
        if skip {
            skip = false;
        } else if known.contains(&a.as_str()) {
            skip = true;
        } else if a != "--no-test" {
            eprintln!("delphi research setup: unknown argument `{a}`\n{USAGE}");
            return 2;
        }
    }
    let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let o = Opts { home: home.to_path_buf(), ws_dir: ws_dir.to_path_buf(), engine: crate::flag(args, "--engine"), credential: crate::flag(args, "--credential"), url: crate::flag(args, "--url"), cx: crate::flag(args, "--cx"), from_flags: !tty };
    if !tty && o.engine.is_none() {
        eprintln!("delphi research setup: needs a terminal (it asks which engine and takes a key with hidden input). Without one, pass --engine and, for an engine with a key, --credential with a reference.\n{USAGE}");
        return 2;
    }
    let transport = if args.iter().any(|a| a == "--no-test") {
        None
    } else {
        match delphi_adapter::research::transport::HttpTransport::new() {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("delphi research setup: the test could not start ({e}); pass --no-test to set up without it");
                return 1;
            }
        }
    };
    let resolve = |c: &str| -> Option<String> {
        use delphi_adapter::credential::{Resolved, Resolver};
        match Resolver::system(std::sync::Arc::new(|_| {})).resolve(&CredentialRef::parse(c).ok()?) {
            Ok(Resolved::Token(t)) => Some(t.expose().to_string()),
            _ => None,
        }
    };
    let net = transport.as_ref().map(|t| Net { transport: t, resolve: &resolve });
    match run(&mut crate::setup::TerminalUi { auto: !tty }, &crate::setup::RealEnv, net.as_ref(), &o) {
        Ok(out) if out.engine.is_some() && out.search_ok != Some(false) && out.fetch_ok != Some(false) => 0,
        Ok(_) => 1,
        Err(e) => {
            eprintln!("delphi research setup: {:#}", e);
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delphi_adapter::research::transport::Response;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Default)]
    struct Keychain {
        stored: RefCell<Vec<(String, String)>>,
        locked: bool,
    }

    impl Env for Keychain {
        fn run(&self, _: &str, _: &[&str]) -> Option<(i32, String)> {
            None
        }
        fn run_tty(&self, _: &str, _: &[&str]) -> bool {
            false
        }
        fn secret_set(&self, name: &str, secret: &str) -> Result<(), String> {
            if self.locked {
                return Err("the keychain is locked".into());
            }
            self.stored.borrow_mut().push((name.into(), secret.into()));
            Ok(())
        }
        fn http_get(&self, _: &str, _: &[(&str, &str)]) -> Result<(u16, String), String> {
            Err("offline".into())
        }
    }

    /// Scripted answers; everything said and every question asked is kept, in order.
    #[derive(Default)]
    struct Script {
        lines: VecDeque<String>,
        secrets: VecDeque<String>,
        yes: VecDeque<bool>,
        said: Vec<String>,
    }

    impl Ui for Script {
        fn say(&mut self, text: &str) {
            self.said.push(text.to_string());
        }
        fn yes_no(&mut self, question: &str, default: bool) -> bool {
            self.said.push(format!("? {question}"));
            self.yes.pop_front().unwrap_or(default)
        }
        fn line(&mut self, question: &str, default: &str) -> String {
            self.said.push(format!("? {question}"));
            self.lines.pop_front().unwrap_or_else(|| default.to_string())
        }
        fn secret(&mut self, question: &str) -> String {
            self.said.push(format!("? {question} (hidden)"));
            self.secrets.pop_front().unwrap_or_default()
        }
        fn open_url(&mut self, _: &str) {}
    }

    /// A request the fake web saw: its address and its headers.
    type Request = (String, Vec<(String, String)>);

    /// A fake engine and a fake trusted site: every request is recorded with its headers.
    struct FakeWeb {
        seen: Mutex<Vec<Request>>,
        engine_status: u16,
    }

    impl FakeWeb {
        fn new(engine_status: u16) -> FakeWeb {
            FakeWeb { seen: Mutex::new(Vec::new()), engine_status }
        }
        fn urls(&self) -> Vec<String> {
            self.seen.lock().unwrap().iter().map(|(u, _)| u.clone()).collect()
        }
    }

    impl Transport for FakeWeb {
        fn get(&self, url: &str, headers: &[(&str, &str)], _: usize, _: Duration) -> Result<Response, String> {
            self.seen.lock().unwrap().push((url.to_string(), headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()));
            let page = |status: u16, ct: &str, body: &str| Response { status, content_type: Some(ct.into()), location: None, body: body.as_bytes().to_vec(), truncated: false };
            if url.contains("api.search.brave.com") {
                return Ok(page(self.engine_status, "application/json", r#"{"web": {"results": [{"title": "Off list", "url": "https://example.org/x", "description": "d"}, {"title": "Encyclopedia", "url": "https://en.wikipedia.org/wiki/Encyclopedia", "description": "d"}]}}"#));
            }
            if url.starts_with("https://en.wikipedia.org/") {
                return Ok(page(200, "text/html", "<html><body><p>An encyclopedia is a reference work.</p></body></html>"));
            }
            Err(format!("no route to {url}"))
        }
    }

    fn workspace() -> (tempfile::TempDir, Opts) {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().to_path_buf();
        let ws = crate::home::init(&crate::home::InitOptions { home: home.clone(), workspace: "w".into(), projects: vec![], theme: "greek".into() }).unwrap();
        let o = Opts { home, ws_dir: ws.dir, engine: None, credential: None, url: None, cx: None, from_flags: false };
        (t, o)
    }

    fn effective_search(o: &Opts) -> SearchConfig {
        let live = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        let (text, _) = delphi_engine::boot::effective_config(&o.ws_dir, &live).unwrap();
        delphi_adapter::config::Config::parse(&text).unwrap().search
    }

    // specs/119 AC-1: on a fresh home, setup with a test engine writes [search], keeps the key outside the config file, and the
    // configuration the daemon then runs on has the engine, so a following request_research is not refused.
    #[test]
    fn on_a_fresh_home_setup_writes_search_keeps_the_key_in_the_keychain_and_the_daemon_then_has_the_engine() {
        let (_t, o) = workspace();
        assert!(!effective_search(&o).explicit, "a fresh workspace has no engine of the operator's own");
        let key = "BSA-not-a-real-key-0123456789";
        let mut ui = Script { secrets: [key.to_string()].into(), ..Default::default() };
        let (env, web) = (Keychain::default(), FakeWeb::new(200));
        let net = Net { transport: &web, resolve: &|_| None };
        let out = run(&mut ui, &env, Some(&net), &o).unwrap();
        assert_eq!(out, Outcome { engine: Some("brave".into()), ratified: true, search_ok: Some(true), fetch_ok: Some(true) });

        let cfg = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        assert!(cfg.contains("[search]") && cfg.contains("providers = [\"brave\"]") && cfg.contains("credential = \"keychain:brave-search-key\""), "{cfg}");
        assert!(!cfg.contains(key), "the key is not in config.toml");
        assert_eq!(*env.stored.borrow(), vec![("brave-search-key".to_string(), key.to_string())]);
        assert!(ui.said.iter().all(|l| !l.contains(key)), "the key is never printed");

        // What the daemon reads: the ratified table, with the engine the operator chose.
        let s = effective_search(&o);
        assert!(s.explicit && s.providers == vec![Provider::Brave], "{s:?}");
        assert_eq!(engine_state_in(&cfg), Some(EngineState::Chosen));
        assert_eq!(hint(&o.ws_dir), None, "nothing left to say at the end of init or connect");

        // The test query carried the typed key to the engine; the test fetch read the trusted result, not the unlisted one.
        let seen = web.seen.lock().unwrap();
        assert!(seen[0].0.contains("api.search.brave.com") && seen[0].1.iter().any(|(_, v)| v == key));
        assert_eq!(seen[1].0, "https://en.wikipedia.org/wiki/Encyclopedia");
        assert_eq!(seen.len(), 2, "one query and one fetch");
        let said = ui.said.join("\n");
        for needle in ["✓ test query: brave answered with 2 result(s)", "✓ test fetch: en.wikipedia.org answered", "Ratified: research can search."] {
            assert!(said.contains(needle), "{needle}: {said}");
        }
    }

    // specs/119 §2 item 6: Brave is offered first and Mojeek second, any supported engine is taken, and the one-line notice is
    // printed before a key is asked for.
    #[test]
    fn brave_and_mojeek_are_suggested_first_and_the_notice_comes_before_the_key() {
        assert_eq!((OFFERED[0], OFFERED[1]), (Provider::Brave, Provider::Mojeek));
        for p in Provider::ALL {
            assert!(OFFERED.contains(&p), "{} is offered", p.name());
        }
        let (_t, o) = workspace();
        let mut ui = Script { lines: ["mojeek".to_string()].into(), secrets: ["mojeek-key-0123456789".to_string()].into(), yes: [false].into(), ..Default::default() };
        let out = run(&mut ui, &Keychain::default(), None, &o).unwrap();
        assert_eq!(out, Outcome { engine: Some("mojeek".into()), ratified: false, search_ok: None, fetch_ok: None });
        let at = |needle: &str| ui.said.iter().position(|l| l.contains(needle)).unwrap_or_else(|| panic!("{needle}: {:?}", ui.said));
        assert!(at("brave ") < at("mojeek ") && at("mojeek ") < at("searxng "), "the order engines are offered in");
        let notice_at = at("Queries leave this machine for the engine chosen (mojeek). Check its terms and your employer's rules first.");
        assert!(at("? Engine") < notice_at && notice_at < at("? Mojeek API key (hidden)"));
        // Declined at the ratify question: written, pending, and the daemon still has no engine of the operator's own.
        assert!(std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap().contains("providers = [\"mojeek\"]"));
        assert!(!effective_search(&o).explicit);
        assert!(hint(&o.ws_dir).is_none(), "the live file names an engine; the pending change is the doctor's to report");
    }

    #[test]
    fn a_key_the_engine_rejects_is_not_stored_and_nothing_is_written() {
        let (_t, o) = workspace();
        let before = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        let mut ui = Script { secrets: ["wrong-key-0123456789".to_string()].into(), ..Default::default() };
        let (env, web) = (Keychain::default(), FakeWeb::new(401));
        let out = run(&mut ui, &env, Some(&Net { transport: &web, resolve: &|_| None }), &o).unwrap();
        assert_eq!(out, Outcome { engine: None, ratified: false, search_ok: Some(false), fetch_ok: None });
        assert!(env.stored.borrow().is_empty());
        assert_eq!(std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap(), before);
        assert_eq!(web.urls().len(), 1, "no fetch after a failed query with a typed key");
        // Too short to be a key: nothing is asked of the engine at all.
        let web = FakeWeb::new(200);
        let mut ui = Script { secrets: ["abc".to_string()].into(), ..Default::default() };
        assert_eq!(run(&mut ui, &env, Some(&Net { transport: &web, resolve: &|_| None }), &o).unwrap(), Outcome::default());
        assert!(web.urls().is_empty());
        // A keychain that cannot take the key: an error, and the config is still untouched.
        let mut ui = Script { secrets: ["good-key-0123456789".to_string()].into(), ..Default::default() };
        let err = run(&mut ui, &Keychain { locked: true, ..Default::default() }, None, &o).unwrap_err();
        assert!(err.to_string().contains("keychain"), "{err}");
        assert_eq!(std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap(), before);
    }

    // Without a terminal: answers are flags, the key is a reference, nothing is asked and nothing is ratified.
    #[test]
    fn from_flags_it_asks_nothing_takes_a_reference_never_a_key_and_does_not_ratify() {
        let (_t, mut o) = workspace();
        o.from_flags = true;
        let env = Keychain::default();
        let mut ui = Script::default();
        let before = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        assert!(run(&mut ui, &env, None, &o).unwrap_err().to_string().contains("--engine"));
        o.engine = Some("brave".into());
        assert!(run(&mut ui, &env, None, &o).unwrap_err().to_string().contains("--credential"));
        for bad in ["BSA-a-pasted-key", "none", "command:cat /tmp/key"] {
            o.credential = Some(bad.into());
            assert!(run(&mut ui, &env, None, &o).is_err(), "{bad} is not a reference to a key");
        }
        assert_eq!(std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap(), before, "a refusal writes nothing");

        o.credential = Some("env:BRAVE_API_KEY".into());
        let web = FakeWeb::new(200);
        let resolve = |c: &str| (c == "env:BRAVE_API_KEY").then(|| "key-from-the-environment".to_string());
        let mut ui = Script::default();
        let out = run(&mut ui, &env, Some(&Net { transport: &web, resolve: &resolve }), &o).unwrap();
        assert_eq!(out, Outcome { engine: Some("brave".into()), ratified: false, search_ok: Some(true), fetch_ok: Some(true) });
        assert!(ui.said.iter().all(|l| !l.starts_with("? ")), "nothing was asked: {:?}", ui.said);
        assert!(env.stored.borrow().is_empty(), "no key is taken without a terminal");
        assert!(web.seen.lock().unwrap()[0].1.iter().any(|(_, v)| v == "key-from-the-environment"));
        assert!(ui.said.iter().any(|l| l.contains("Not ratified: that is yours to do")));
        assert!(std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap().contains("credential = \"env:BRAVE_API_KEY\""));
        assert!(!effective_search(&o).explicit, "written, pending, not in force");
    }

    #[test]
    fn an_engine_already_written_stays_as_a_fallback_and_a_keyless_engine_needs_its_address() {
        let (_t, mut o) = workspace();
        o.from_flags = true;
        o.engine = Some("searxng".into());
        assert!(run(&mut Script::default(), &Keychain::default(), None, &o).unwrap_err().to_string().contains("--url"));
        o.url = Some("http://search.example.org".into());
        assert!(run(&mut Script::default(), &Keychain::default(), None, &o).unwrap_err().to_string().contains("must be https"), "the daemon's own parser checks the table before it is written");
        o.url = Some("https://search.example.org".into());
        run(&mut Script::default(), &Keychain::default(), None, &o).unwrap();
        o.engine = Some("parallel".into());
        o.url = None;
        run(&mut Script::default(), &Keychain::default(), None, &o).unwrap();
        let cfg = std::fs::read_to_string(o.ws_dir.join("config.toml")).unwrap();
        assert!(cfg.contains("providers = [\"parallel\", \"searxng\"]") && cfg.contains("url = \"https://search.example.org\""), "{cfg}");
        o.engine = Some("altavista".into());
        assert!(run(&mut Script::default(), &Keychain::default(), None, &o).unwrap_err().to_string().contains("unknown engine"));
    }

    // specs/119 §2 item 2: the one line at the end of init and connect, by what the live config says.
    #[test]
    fn the_hint_names_the_command_when_no_engine_is_chosen_and_is_silent_when_one_is() {
        let (_t, o) = workspace();
        let cfg = o.ws_dir.join("config.toml");
        let base = std::fs::read_to_string(&cfg).unwrap();
        assert!(hint(&o.ws_dir).unwrap().contains("Run `delphi research setup` to choose one."), "a fresh workspace: the keyless default only");
        std::fs::write(&cfg, format!("{base}\n[search]\nproviders = []\n")).unwrap();
        assert_eq!(hint(&o.ws_dir).unwrap(), "  research: no search engine is set up, so research cannot search the web. Run `delphi research setup`.");
        std::fs::write(&cfg, format!("{base}\n[search]\nproviders = [\"parallel\"]\n")).unwrap();
        assert_eq!(hint(&o.ws_dir), None);
        std::fs::write(&cfg, "not toml [").unwrap();
        assert_eq!(hint(&o.ws_dir), None, "an unreadable config is the doctor's to report");
        assert_eq!(hint(&o.ws_dir.join("nowhere")), None);
    }
}
