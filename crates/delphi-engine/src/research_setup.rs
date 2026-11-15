//! What the research job needs from the workspace (`specs/028` §10): the two trusted-source lists, the configured search engines with
//! their keys, a transport and the scout. Reading files and resolving keys can block (a keychain, a helper command), so the caller runs
//! [`deps`] on a blocking thread.

use crate::engine::Engine;
use crate::fact_sources::SourceList;
use crate::research::{Deps, EngineJudge, Limits, Scope};
use delphi_adapter::credential::{CredentialRef, Resolved, Resolver};
use delphi_adapter::research::search::ProviderSpec;
use delphi_adapter::research::transport::HttpTransport;
use delphi_adapter::research::trusted::TrustedList;
use std::path::Path;
use std::sync::Arc;

/// The command that sets a search engine up (`specs/119` §2 item 1). Every message that says research cannot search names it.
pub const SETUP_COMMAND: &str = "delphi research setup";

/// What `[search]` says about engines (`specs/119` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    /// `providers = []`: no engine can be asked, and a research request is refused.
    None,
    /// No `providers` key: only the keyless default of `specs/028` §12 is asked. The operator has chosen nothing.
    DefaultOnly,
    /// The operator wrote `providers` and it names at least one engine.
    Chosen,
}

pub fn engine_state(search: &delphi_adapter::research::SearchConfig) -> EngineState {
    match (search.providers.is_empty(), search.explicit) {
        (true, _) => EngineState::None,
        (false, false) => EngineState::DefaultOnly,
        (false, true) => EngineState::Chosen,
    }
}

/// [`engine_state`] of a `config.toml`'s text, as the doctor reads it; `None` when the file or its `[search]` table cannot be read.
pub fn engine_state_in(config_text: &str) -> Option<EngineState> {
    let root: toml::Value = toml::from_str(config_text).ok()?;
    let gemini = root.get("endpoints").and_then(|e| e.as_array()).is_some_and(|a| a.iter().any(|e| e.get("family").and_then(|f| f.as_str()) == Some("gemini")));
    match root.get("search") {
        Some(t) => delphi_adapter::research::SearchConfig::parse(t, gemini).ok().map(|s| engine_state(&s)),
        None => Some(EngineState::DefaultOnly),
    }
}

/// What a research request is told when no engine can be asked (`specs/119` §2 item 4).
pub fn no_engine_message() -> String {
    format!("research cannot run: no search engine is set up (`providers` under [search] in config.toml is empty). The operator sets one up at a terminal with `{}`", SETUP_COMMAND)
}

/// The default list shipped in this build, used when the workspace's copy of `context/sources/trusted.toml` is missing.
const SHIPPED_LIST: &str = include_str!("../../../context/sources/trusted.toml");

/// The workspace's lists: the default (the workspace's copy of the release's file, else the one compiled in) and the operator's own.
pub fn lists(ws: &Path, home: &Path) -> Result<TrustedList, String> {
    let default = std::fs::read_to_string(ws.join("base/context/sources/trusted.toml")).unwrap_or_else(|_| SHIPPED_LIST.to_string());
    let operator = std::fs::read_to_string(home.join("operator/trusted-sources.toml")).ok();
    TrustedList::parse(&default, operator.as_deref())
}

/// The configured engines in order, each with its key resolved by the same sources as a model endpoint's (`specs/009` §4). A key that
/// cannot be resolved leaves the engine in the list without one, and the search passes it over with that reason.
pub fn engines(engine: &Engine) -> Vec<ProviderSpec> {
    let cfg = engine.cfg.get();
    let reg = engine.secrets.clone();
    let resolver = Resolver::system(Arc::new(move |s| reg.register(s)));
    cfg.search
        .providers
        .iter()
        .map(|p| {
            let s = cfg.search.settings.get(p.name()).cloned().unwrap_or_default();
            let key = s.credential.as_deref().and_then(|c| CredentialRef::parse(c).ok()).and_then(|r| match resolver.resolve(&r) {
                Ok(Resolved::Token(t)) => Some(t.expose().to_string()),
                _ => None,
            });
            ProviderSpec { provider: *p, key, base_url: s.url, cx: s.cx }
        })
        .collect()
}

pub fn deps(engine: &Arc<Engine>, ws: &Path, home: &Path, scope: Scope) -> Result<Deps, String> {
    if engine_state(&engine.cfg.get().search) == EngineState::None {
        return Err(no_engine_message());
    }
    let list = lists(ws, home)?;
    let date = (engine.clock)().get(..10).unwrap_or("").to_string();
    Ok(Deps {
        transport: Arc::new(HttpTransport::new()?),
        judge: Arc::new(EngineJudge(engine.clone())),
        list: Arc::new(list),
        engines: engines(engine),
        ws_dir: ws.to_path_buf(),
        scope,
        validator: Arc::new(SourceList::new(engine.cfg.get().validator.sources.clone())),
        weak_sources: engine.cfg.get().validator.weak_sources,
        date,
        limits: Limits::default(),
        identifiers: crate::query_guard::identifiers(ws),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // specs/119 §2: what counts as an engine. An empty list is none; no table is the keyless default, which the operator did not choose.
    #[test]
    fn the_engine_state_is_none_for_an_empty_list_default_for_no_table_and_chosen_for_a_written_one() {
        assert_eq!(engine_state_in(""), Some(EngineState::DefaultOnly));
        assert_eq!(engine_state_in("[search]\n"), Some(EngineState::DefaultOnly));
        assert_eq!(engine_state_in("[search]\nproviders = []\n"), Some(EngineState::None));
        assert_eq!(engine_state_in("[search]\nproviders = [\"parallel\"]\n"), Some(EngineState::Chosen));
        assert_eq!(engine_state_in("[search]\nproviders = [\"brave\"]\n[search.brave]\ncredential = \"keychain:brave-search-key\"\n"), Some(EngineState::Chosen));
        assert_eq!(engine_state_in("[search]\nproviders = [\"nonesuch\"]\n"), None, "a table that cannot be read");
        assert_eq!(engine_state_in("not toml ["), None);
        assert!(no_engine_message().contains("`delphi research setup`"));
    }

    #[test]
    fn the_shipped_list_is_a_valid_default_and_the_operators_adds_to_it() {
        let home = tempfile::tempdir().unwrap();
        let ws = tempfile::tempdir().unwrap();
        let l = lists(ws.path(), home.path()).unwrap();
        assert!(l.allows_url("https://en.wikipedia.org/wiki/X").is_some() && l.allows_url("https://example.org/").is_none());
        std::fs::create_dir_all(home.path().join("operator")).unwrap();
        std::fs::write(home.path().join("operator/trusted-sources.toml"), "[[source]]\nhost = \"example.org\"\nreuse = \"summary-only\"\n").unwrap();
        assert!(lists(ws.path(), home.path()).unwrap().allows_url("https://example.org/").is_some());
        std::fs::write(home.path().join("operator/trusted-sources.toml"), "[[source]]\nhost = \"https://bad/\"\nreuse = \"full\"\n").unwrap();
        assert!(lists(ws.path(), home.path()).unwrap_err().contains("operator/trusted-sources.toml"));
    }

    #[test]
    fn the_shipped_list_lets_wikipedia_keep_pictures_but_never_stores_wikimedia_pages_whole() {
        let l = TrustedList::parse(SHIPPED_LIST, None).unwrap();
        for url in ["https://upload.wikimedia.org/wikipedia/commons/a/ab/X.png", "https://commons.wikimedia.org/w/api.php?action=query"] {
            assert!(l.allows_url(url).is_some(), "{} is listed so that a Wikipedia picture can be checked and fetched", url);
        }
        for host in ["upload.wikimedia.org", "commons.wikimedia.org"] {
            let s = l.lookup(host).unwrap();
            assert_eq!((s.reuse, s.images), (delphi_adapter::research::trusted::Reuse::SummaryOnly, false), "{}: text is never kept whole and its pictures need a licence of their own", host);
        }
        assert!(l.lookup("en.wikipedia.org").is_some() && !l.lookup("en.wikipedia.org").unwrap().images, "a Wikipedia page's licence is not its pictures'");
        assert!(l.allows_url("https://commons.wikimedia.org.evil.example/").is_none() && l.allows_url("http://upload.wikimedia.org/x.png").is_none());
    }

    #[test]
    fn the_workspace_copy_of_the_default_list_wins_over_the_compiled_in_one() {
        let (home, ws) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        std::fs::create_dir_all(ws.path().join("base/context/sources")).unwrap();
        std::fs::write(ws.path().join("base/context/sources/trusted.toml"), "[[source]]\nhost = \"only.example\"\nreuse = \"full\"\n").unwrap();
        let l = lists(ws.path(), home.path()).unwrap();
        assert!(l.allows_url("https://only.example/").is_some() && l.allows_url("https://en.wikipedia.org/wiki/X").is_none());
    }
}
