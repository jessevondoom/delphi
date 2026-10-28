//! Research for the archives pane (`specs/028` §10): trusted-source lists, web search with a privacy-friendly fallback order, and
//! the allow-checked fetch of a page. Deterministic code only; the job that drives it and the scout's decision call are E3.

pub mod fetch;
pub mod html;
pub mod search;
pub mod transport;
pub mod trusted;

use crate::credential::CredentialRef;
use search::Provider;
use serde::Deserialize;
use std::collections::BTreeMap;

/// One engine's settings as written in `[search.<name>]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSettings {
    /// Where the key comes from (`specs/009` §4): `env:BRAVE_API_KEY`, `keychain:…`, `command:…`.
    #[serde(default)]
    pub credential: Option<String>,
    /// SearXNG: the instance.
    #[serde(default)]
    pub url: Option<String>,
    /// Google Programmable Search: the engine id.
    #[serde(default)]
    pub cx: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    #[serde(default)]
    providers: Vec<String>,
    #[serde(default)]
    brave: Option<ProviderSettings>,
    #[serde(default)]
    mojeek: Option<ProviderSettings>,
    #[serde(default)]
    searxng: Option<ProviderSettings>,
    #[serde(default)]
    kagi: Option<ProviderSettings>,
    #[serde(default)]
    google: Option<ProviderSettings>,
}

/// `[search]`: which engines the research job asks, in order. Unset means no engine, and expand mode says so.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchConfig {
    pub providers: Vec<Provider>,
    pub settings: BTreeMap<String, ProviderSettings>,
}

impl SearchConfig {
    /// Parse and check `[search]`. `gemini_configured` says whether an `[[endpoints]]` entry of the Gemini family exists, since
    /// Google is allowed only then (`specs/028` §10.8 Q2).
    pub fn parse(table: &toml::Value, gemini_configured: bool) -> Result<SearchConfig, Vec<String>> {
        let raw: Raw = table.clone().try_into().map_err(|e| vec![format!("[search]: {}", e)])?;
        let mut errs = Vec::new();
        let mut cfg = SearchConfig::default();
        for (name, s) in [("brave", raw.brave), ("mojeek", raw.mojeek), ("searxng", raw.searxng), ("kagi", raw.kagi), ("google", raw.google)] {
            if let Some(s) = s {
                cfg.settings.insert(name.into(), s);
            }
        }
        for name in &raw.providers {
            let Some(p) = Provider::parse(name) else {
                errs.push(format!("[search] providers: unknown engine `{}` (expected {})", name, Provider::ALL.map(|p| p.name()).join(", ")));
                continue;
            };
            if cfg.providers.contains(&p) {
                errs.push(format!("[search] providers: `{}` is listed twice", name));
                continue;
            }
            cfg.providers.push(p);
            let s = cfg.settings.get(name).cloned().unwrap_or_default();
            if p.needs_key() {
                match s.credential.as_deref() {
                    None | Some("") => errs.push(format!("[search.{}]: needs a `credential` (for example \"env:{}_API_KEY\")", name, name.to_uppercase())),
                    Some("none") | Some("harness-login") => errs.push(format!("[search.{}]: `{}` is not a key source", name, s.credential.as_deref().unwrap_or(""))),
                    Some(c) => {
                        if let Err(m) = CredentialRef::parse(c) {
                            errs.push(format!("[search.{}]: {}", name, m));
                        }
                    }
                }
            }
            match p {
                Provider::Searxng => match s.url.as_deref() {
                    Some(u) if u.starts_with("https://") || crate::config::is_loopback(u) => {}
                    Some(u) => errs.push(format!("[search.searxng]: url `{}` must be https (plain http only to loopback)", u)),
                    None => errs.push("[search.searxng]: needs the instance's `url`".into()),
                },
                Provider::Google => {
                    if s.cx.as_deref().unwrap_or("").is_empty() {
                        errs.push("[search.google]: needs `cx`, the Programmable Search engine id".into());
                    }
                    if !gemini_configured {
                        errs.push("[search] providers: `google` is allowed only when a Gemini endpoint is configured in [[endpoints]] (Google already sees your prompts then)".into());
                    }
                }
                _ => {}
            }
        }
        if errs.is_empty() {
            Ok(cfg)
        } else {
            Err(errs)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(t: &str, gemini: bool) -> Result<SearchConfig, Vec<String>> {
        SearchConfig::parse(&toml::from_str::<toml::Value>(t).unwrap()["search"], gemini)
    }

    #[test]
    fn the_recommended_order_parses() {
        let c = parse("[search]\nproviders = [\"brave\", \"mojeek\", \"searxng\"]\n[search.brave]\ncredential = \"env:BRAVE_API_KEY\"\n[search.mojeek]\ncredential = \"keychain:mojeek\"\n[search.searxng]\nurl = \"https://searx.example.org\"\n", false).unwrap();
        assert_eq!(c.providers, vec![Provider::Brave, Provider::Mojeek, Provider::Searxng]);
    }

    #[test]
    fn an_unset_search_table_is_no_engine() {
        assert!(parse("[search]\n", false).unwrap().providers.is_empty());
    }

    #[test]
    fn mistakes_are_named() {
        let e = |t: &str, g: bool| parse(t, g).unwrap_err().join(" | ");
        assert!(e("[search]\nproviders = [\"bing\"]\n", false).contains("unknown engine `bing`"));
        assert!(e("[search]\nproviders = [\"brave\", \"brave\"]\n[search.brave]\ncredential = \"env:K\"\n", false).contains("twice"));
        assert!(e("[search]\nproviders = [\"brave\"]\n", false).contains("needs a `credential`"));
        assert!(e("[search]\nproviders = [\"brave\"]\n[search.brave]\ncredential = \"none\"\n", false).contains("not a key source"));
        assert!(e("[search]\nproviders = [\"brave\"]\n[search.brave]\ncredential = \"pasted-secret\"\n", false).contains("unknown credential source"));
        assert!(!e("[search]\nproviders = [\"brave\"]\n[search.brave]\ncredential = \"pasted-secret-value\"\n", false).contains("pasted-secret-value"));
        assert!(e("[search]\nproviders = [\"searxng\"]\n[search.searxng]\nurl = \"http://searx.example.org\"\n", false).contains("must be https"));
        assert!(e("[search]\nproviders = [\"searxng\"]\n", false).contains("needs the instance's `url`"));
        assert!(e("[search]\nproviders = [\"brave\"]\nverbose = true\n", false).contains("[search]"));
    }

    #[test]
    fn a_local_searxng_over_plain_http_is_fine() {
        assert!(parse("[search]\nproviders = [\"searxng\"]\n[search.searxng]\nurl = \"http://127.0.0.1:8080\"\n", false).is_ok());
    }

    #[test]
    fn google_only_with_a_gemini_endpoint() {
        let t = "[search]\nproviders = [\"brave\", \"google\"]\n[search.brave]\ncredential = \"env:K\"\n[search.google]\ncredential = \"env:G\"\ncx = \"abc\"\n";
        assert!(parse(t, false).unwrap_err().join(" ").contains("only when a Gemini endpoint"));
        assert!(parse(t, true).is_ok());
        assert!(parse(&t.replace("cx = \"abc\"\n", ""), true).unwrap_err().join(" ").contains("needs `cx`"));
    }
}
