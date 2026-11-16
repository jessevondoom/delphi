//! What a research run needs from the workspace (`specs/028` §10): the two trusted-source lists and the configured search engines
//! with their keys. Reading files and resolving keys can block (a keychain, a helper command), so the caller reads them on a
//! blocking thread (`EngineService::load_run_web`).

use crate::engine::Engine;
use delphi_adapter::credential::{CredentialRef, Resolved, Resolver};
use delphi_adapter::research::search::ProviderSpec;
use delphi_adapter::research::trusted::TrustedList;
use std::path::Path;
use std::sync::Arc;

/// The command that sets a search engine up (`specs/119` §2 item 1). Every message that says research cannot search names it.
pub const SETUP_COMMAND: &str = "delphi research setup";

/// What `[search]` says about engines (`specs/119` §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineState {
    /// `providers = []`: no engine may be asked. It is the operator's way of keeping queries on this machine (`specs/119` §13.2
    /// item 7): a research run then reads what Delphi holds and the trusted addresses it already knows, and is not refused.
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

/// Where a research run gets its web results (`specs/119` §13.2 items 6 and 7). With no engine (`providers = []`) the run is local. A
/// request is never refused for want of a source. The private-read mark of `specs/103` does not change the answer (`specs/119` §15):
/// a marked session researches as any other, and what keeps private text in is the cleaning of each query and the identifier guard.
pub fn sources(search: &delphi_adapter::research::SearchConfig) -> crate::research_run::Sources {
    if engine_state(search) == EngineState::None {
        crate::research_run::Sources::Local
    } else {
        crate::research_run::Sources::Engines
    }
}

/// What the reply to a request says when the session that asked is under the private-read mark (`specs/119` §15): fixed words.
pub const MARK_NOTE: &str = "this session has read private material. The research runs as usual: every query is cleaned of secrets, paths and private addresses and passes the identifier guard, and the search log of the result lists each query the run planned, so the operator can see what left this machine.";

/// What the reply to a request says when its run is local (`specs/119` §13.2 item 6): fixed words.
pub fn local_note() -> String {
    format!("this run sends no web search: it reads what Delphi already holds and the trusted sites it already knows by their addresses. Its answer's label says so. The operator can set up a search engine at a terminal with `{}`", SETUP_COMMAND)
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

/// The engines the validator's contradiction search may ask (`specs/028` §12.1, `specs/119` §13.3 item 12). It sends claim text, so:
/// none unless the operator wrote `providers`; and Parallel's keyless engine, which `delphi init` and setup write so that research
/// always has a working table, only when the operator has turned background validation on (the setup question that says claim text
/// goes to the search engine). Brave, or any other engine the operator set up with a key or an address, is asked as before.
pub fn validator_engines(engines: Vec<ProviderSpec>, search: &delphi_adapter::research::SearchConfig, background: bool) -> Vec<ProviderSpec> {
    if !search.explicit {
        return Vec::new();
    }
    engines.into_iter().filter(|e| background || e.provider != delphi_adapter::research::search::Provider::Parallel).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // specs/119 §13.3 item 12: Parallel written by init or setup does not start receiving claim text. The validator's search asks it
    // only with background validation on; an engine the operator set up is asked as before; the unwritten default never is.
    #[test]
    fn the_validators_search_asks_parallel_only_when_background_validation_is_on() {
        use delphi_adapter::research::search::Provider;
        let cfg = |t: &str| delphi_adapter::research::SearchConfig::parse(&toml::from_str::<toml::Value>(t).unwrap()["search"], false).unwrap();
        let specs = |s: &delphi_adapter::research::SearchConfig| -> Vec<ProviderSpec> { s.providers.iter().map(|p| ProviderSpec { provider: *p, key: None, base_url: None, cx: None }).collect() };
        let names = |s: &delphi_adapter::research::SearchConfig, background: bool| -> Vec<Provider> { validator_engines(specs(s), s, background).into_iter().map(|e| e.provider).collect() };
        let init = cfg("[search]\nproviders = [\"parallel\"]\n");
        assert!(names(&init, false).is_empty(), "the table init writes gives the validator no engine");
        assert_eq!(names(&init, true), vec![Provider::Parallel]);
        let both = cfg("[search]\nproviders = [\"parallel\", \"brave\"]\n[search.brave]\ncredential = \"env:K\"\n");
        assert_eq!(names(&both, false), vec![Provider::Brave]);
        assert_eq!(names(&both, true), vec![Provider::Parallel, Provider::Brave]);
        let unwritten = delphi_adapter::research::SearchConfig::default();
        assert!(names(&unwritten, true).is_empty() && names(&unwritten, false).is_empty(), "the unwritten default is never asked");
        assert!(names(&cfg("[search]\nproviders = []\n"), true).is_empty());
    }

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
        assert!(local_note().contains("`delphi research setup`"));
        // specs/119 §13.2: no engine is a local run; nothing is a refusal.
        use crate::research_run::Sources;
        let cfg = |t: &str| delphi_adapter::research::SearchConfig::parse(&toml::from_str::<toml::Value>(t).unwrap()["search"], false).unwrap();
        assert_eq!(sources(&cfg("[search]\nproviders = []\n")), Sources::Local);
        assert_eq!(sources(&cfg("[search]\nproviders = [\"parallel\"]\n")), Sources::Engines);
        assert_eq!(sources(&delphi_adapter::research::SearchConfig::default()), Sources::Engines);
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
