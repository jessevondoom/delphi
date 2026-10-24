//! The model-access part of `config.toml` (`specs/009` §5, `specs/002` §4): endpoints, the access
//! policy, family→tier maps and the per-model catalog.
//!
//! Parsing is strict and returns every problem at once. The `[[endpoints]]`, `[access]` and
//! `[auth.*]` blocks are what `local/ratified.lock` pins; this module only reads them.

use crate::credential::CredentialRef;
use delphi_core::budget::Price;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum AccessPath {
    #[serde(rename = "api")]
    Api,
    #[serde(rename = "cloud")]
    Cloud,
    #[serde(rename = "gateway-native")]
    GatewayNative,
    #[serde(rename = "gateway-compat")]
    GatewayCompat,
    #[serde(rename = "harness")]
    Harness,
}

impl AccessPath {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccessPath::Api => "api",
            AccessPath::Cloud => "cloud",
            AccessPath::GatewayNative => "gateway-native",
            AccessPath::GatewayCompat => "gateway-compat",
            AccessPath::Harness => "harness",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Endpoint {
    pub id: String,
    pub family: String,
    pub path: AccessPath,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    /// Vertex AI project.
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    pub credential: String,
    /// Tier → concrete model ID for this endpoint; overrides the global `[models.<family>]`.
    #[serde(default)]
    pub models: BTreeMap<String, String>,
    /// Extra request headers (for example a cost-center header). Values are treated as secrets for redaction.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub cost_model: Option<String>,
    #[serde(default)]
    pub harness: Option<String>,
    /// `anthropic-beta` flags to send. When empty and thinking is on, the interleaved-thinking flag is used.
    #[serde(default)]
    pub betas: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum AuthFlow {
    #[serde(rename = "device_code")]
    DeviceCode,
    #[serde(rename = "client_credentials")]
    ClientCredentials,
    /// Authorization code with PKCE, redirected to a loopback port (`specs/010` §5).
    #[serde(rename = "loopback_pkce")]
    LoopbackPkce,
}

/// One `[auth.<name>]` block (`specs/009` §4.1). Delphi ships no client ids: they come from the operator.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AuthBlock {
    pub flow: AuthFlow,
    /// OIDC discovery root; or give the URLs below directly.
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub token_url: Option<String>,
    #[serde(default)]
    pub device_authorization_url: Option<String>,
    #[serde(default)]
    pub authorization_url: Option<String>,
    pub client_id: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub audience: Option<String>,
    /// `keychain:<name>` only; never an inline secret.
    #[serde(default)]
    pub client_secret: Option<String>,
    /// Loopback PKCE: the fixed port registered with the provider (an ephemeral one when unset).
    #[serde(default)]
    pub redirect_port: Option<u16>,
    /// Extra query parameters for the authorization request, for providers that need them to issue a refresh
    /// token (Google: `access_type = "offline"`, `prompt = "consent"`).
    #[serde(default)]
    pub authorization_params: BTreeMap<String, String>,
}

/// What a connector tool does, as the operator ratified it (`specs/010` §3, §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum ToolClass {
    /// Reads. Amber on first use per task unless `first_use = "allow"`; always asks when a tainted span puts text in a query.
    #[serde(rename = "read")]
    Read,
    /// Creates something private (a draft, a hold with no invitees). Amber, with the full payload.
    #[serde(rename = "draft")]
    Draft,
    /// Sends, posts, shares, invites or deletes. Red, bound to the payload's hash, never auto-approved.
    #[serde(rename = "write")]
    Write,
}

impl ToolClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolClass::Read => "read",
            ToolClass::Draft => "draft",
            ToolClass::Write => "write",
        }
    }
}

fn default_max_arg_chars() -> usize {
    128
}

fn default_calls_per_minute() -> u32 {
    30
}

fn default_max_result_chars() -> usize {
    20_000
}

fn default_transport() -> String {
    "http".into()
}

/// One `[connectors.<name>]` block (`specs/010` §3). Pinned by hash with `[access]`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Connector {
    /// `http` (Streamable HTTP). stdio is later.
    #[serde(default = "default_transport")]
    pub transport: String,
    pub url: String,
    /// The `[auth.<name>]` block whose login signs the operator in (`delphi auth login <name>`). `None` means the
    /// server needs no login.
    #[serde(default)]
    pub login: Option<String>,
    /// The identity the login must resolve to (an email, a Slack team id). Checked after login; a mismatch discards the token.
    #[serde(default)]
    pub account: Option<String>,
    /// Exact host names this connector may reach, redirects included. No wildcards.
    pub hosts: Vec<String>,
    /// Canonical role IDs offered these tools.
    pub roles: Vec<String>,
    /// Delphi's own allowlist and classification of the server's tools. Anything not listed is invisible.
    pub tools: BTreeMap<String, ToolClass>,
    #[serde(default = "default_max_arg_chars")]
    pub max_arg_chars: usize,
    /// `ask` (Amber once per task) or `allow` (Green) for read tools.
    #[serde(default)]
    pub first_use: Option<String>,
    #[serde(default = "default_calls_per_minute")]
    pub calls_per_minute: u32,
    /// A tool result longer than this goes to the payload store, and the model gets an excerpt and a pointer.
    #[serde(default = "default_max_result_chars")]
    pub max_result_chars: usize,
    /// How `account` is looked up after login: `google` (the OpenID userinfo email). Others arrive with their connectors.
    #[serde(default)]
    pub identity: Option<String>,
}

pub(crate) fn host_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
    let auth = rest.split(['/', '?', '#']).next()?;
    let auth = auth.rsplit('@').next()?;
    let host = if let Some(stripped) = auth.strip_prefix('[') { stripped.split(']').next()? } else { auth.split(':').next()? };
    (!host.is_empty()).then(|| host.to_lowercase())
}

impl Connector {
    pub fn host(&self) -> Option<String> {
        host_of(&self.url)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Access {
    #[serde(default)]
    pub order: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub allowed_endpoints: Option<Vec<String>>,
    /// Role → model family (`claude` or `gemini`) for roles that should not use the default, Claude.
    #[serde(default)]
    pub role_family: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CatalogEntry {
    pub max_output_tokens: u64,
    #[serde(default)]
    pub min_thinking_budget: u32,
    #[serde(default)]
    pub max_thinking_budget: u32,
    /// Claude: the thinking budget counts inside `max_tokens`; Gemini: it is additive.
    #[serde(default = "yes")]
    pub thinking_in_max_tokens: bool,
    #[serde(default)]
    pub input_usd_per_mtok: f64,
    #[serde(default)]
    pub cache_read_usd_per_mtok: f64,
    #[serde(default)]
    pub cache_write_usd_per_mtok: f64,
    #[serde(default)]
    pub output_usd_per_mtok: f64,
}

fn yes() -> bool {
    true
}

impl CatalogEntry {
    pub fn price(&self) -> Price {
        Price { input: self.input_usd_per_mtok, output: self.output_usd_per_mtok, cache_read: self.cache_read_usd_per_mtok, cache_write: self.cache_write_usd_per_mtok }
    }

    pub fn has_prices(&self) -> bool {
        self.input_usd_per_mtok > 0.0 && self.output_usd_per_mtok > 0.0
    }
}

/// The live configuration, replaceable while the daemon runs (for example when the operator ratifies new
/// endpoints). Readers take a snapshot with [`SharedConfig::get`] and use it for the whole operation.
#[derive(Clone)]
pub struct SharedConfig(std::sync::Arc<std::sync::RwLock<std::sync::Arc<Config>>>);

impl SharedConfig {
    pub fn new(c: Config) -> SharedConfig {
        SharedConfig(std::sync::Arc::new(std::sync::RwLock::new(std::sync::Arc::new(c))))
    }

    pub fn get(&self) -> std::sync::Arc<Config> {
        self.0.read().unwrap().clone()
    }

    pub fn set(&self, c: Config) {
        *self.0.write().unwrap() = std::sync::Arc::new(c);
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub endpoints: Vec<Endpoint>,
    pub access: Access,
    /// family → tier → model ID.
    pub models: BTreeMap<String, BTreeMap<String, String>>,
    pub catalog: BTreeMap<String, CatalogEntry>,
    pub auth: BTreeMap<String, AuthBlock>,
    pub connectors: BTreeMap<String, Connector>,
}

const TIERS: [&str; 3] = ["fast", "workhorse", "frontier"];

pub(crate) fn is_loopback(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or("");
    let host = rest.split(['/', ':']).next().unwrap_or("");
    host == "127.0.0.1" || host == "localhost" || host == "[::1]"
}

impl Config {
    /// Parse the model-access blocks of a `config.toml`. Returns all problems found.
    pub fn parse(text: &str) -> Result<Config, Vec<String>> {
        let root: toml::Value = toml::from_str(text).map_err(|e| vec![format!("config.toml is not valid TOML: {}", e)])?;
        let mut errs = Vec::new();
        let mut cfg = Config::default();

        if let Some(list) = root.get("endpoints") {
            match list.clone().try_into::<Vec<Endpoint>>() {
                Ok(v) => cfg.endpoints = v,
                Err(e) => errs.push(format!("[[endpoints]]: {}", e)),
            }
        }
        if let Some(a) = root.get("access") {
            match a.clone().try_into::<Access>() {
                Ok(v) => cfg.access = v,
                Err(e) => errs.push(format!("[access]: {}", e)),
            }
        }
        if let Some(a) = root.get("auth") {
            match a.clone().try_into::<BTreeMap<String, AuthBlock>>() {
                Ok(v) => cfg.auth = v,
                Err(e) => errs.push(format!("[auth.*]: {}", e)),
            }
        }
        if let Some(c) = root.get("connectors") {
            match c.clone().try_into::<BTreeMap<String, Connector>>() {
                Ok(v) => cfg.connectors = v,
                Err(e) => errs.push(format!("[connectors.*]: {}", e)),
            }
        }
        if let Some(models) = root.get("models").and_then(|m| m.as_table()) {
            for (k, v) in models {
                if k == "catalog" {
                    match v.clone().try_into::<BTreeMap<String, CatalogEntry>>() {
                        Ok(c) => cfg.catalog = c,
                        Err(e) => errs.push(format!("[models.catalog]: {}", e)),
                    }
                    continue;
                }
                match v.clone().try_into::<BTreeMap<String, String>>() {
                    Ok(tiers) => {
                        for t in tiers.keys() {
                            if !TIERS.contains(&t.as_str()) {
                                errs.push(format!("[models.{}]: unknown tier `{}` (expected fast, workhorse or frontier)", k, t));
                            }
                        }
                        // Template placeholders such as "<claude fast model id>" count as unset.
                        cfg.models.insert(k.clone(), tiers.into_iter().filter(|(_, id)| !id.trim().is_empty() && !id.starts_with('<')).collect());
                    }
                    Err(e) => errs.push(format!("[models.{}]: {}", k, e)),
                }
            }
        }
        errs.extend(cfg.validate());
        if errs.is_empty() {
            Ok(cfg)
        } else {
            Err(errs)
        }
    }

    fn validate(&self) -> Vec<String> {
        let mut errs = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.endpoints {
            if !seen.insert(e.id.clone()) {
                errs.push(format!("endpoint id `{}` is defined twice", e.id));
            }
            if e.id.trim().is_empty() || e.family.trim().is_empty() {
                errs.push("every endpoint needs a non-empty id and family".into());
            }
            if let Err(m) = CredentialRef::parse(&e.credential) {
                errs.push(format!("endpoint `{}`: {}", e.id, m));
            }
            match e.path {
                AccessPath::Cloud => {
                    match e.platform.as_deref() {
                        Some("bedrock") | Some("vertex") => {}
                        other => errs.push(format!("endpoint `{}`: cloud endpoints need platform = \"bedrock\" or \"vertex\" (got {:?})", e.id, other)),
                    }
                    if e.region.as_deref().unwrap_or("").is_empty() {
                        errs.push(format!("endpoint `{}`: cloud endpoints need a region", e.id));
                    }
                    if e.platform.as_deref() == Some("vertex") && e.project.as_deref().unwrap_or("").is_empty() {
                        errs.push(format!("endpoint `{}`: vertex endpoints need a project", e.id));
                    }
                    if let Some(u) = &e.base_url {
                        if !(u.starts_with("https://") || is_loopback(u)) {
                            errs.push(format!("endpoint `{}`: base_url `{}` must be https (plain http only to loopback)", e.id, u));
                        }
                    }
                }
                AccessPath::GatewayNative | AccessPath::GatewayCompat => match e.base_url.as_deref() {
                    Some(u) if u.starts_with("https://") || is_loopback(u) => {}
                    Some(u) => errs.push(format!("endpoint `{}`: base_url `{}` must be https (plain http only to loopback)", e.id, u)),
                    None => errs.push(format!("endpoint `{}`: gateway endpoints need a base_url", e.id)),
                },
                AccessPath::Harness => {
                    if e.harness.as_deref().unwrap_or("").is_empty() {
                        errs.push(format!("endpoint `{}`: harness endpoints need `harness = \"claude\"|\"agy\"|\"codex\"` (`gemini` is accepted but the Gemini CLI no longer supports personal sign-in)", e.id));
                    }
                }
                AccessPath::Api => {
                    if let Some(u) = &e.base_url {
                        if !(u.starts_with("https://") || is_loopback(u)) {
                            errs.push(format!("endpoint `{}`: base_url `{}` must be https (plain http only to loopback)", e.id, u));
                        }
                    }
                }
            }
        }
        let allowed = self.allowed();
        for id in &allowed {
            if !seen.contains(id) {
                errs.push(format!("[access] allowed_endpoints names unknown endpoint `{}`", id));
            }
        }
        for (family, order) in &self.access.order {
            for id in order {
                match self.endpoints.iter().find(|e| &e.id == id) {
                    None => errs.push(format!("[access] order.{} names unknown endpoint `{}`", family, id)),
                    Some(e) if &e.family != family => errs.push(format!("[access] order.{} lists `{}`, which belongs to family `{}`", family, id, e.family)),
                    Some(_) if !allowed.contains(id) => errs.push(format!("[access] order.{} lists `{}`, which is not in allowed_endpoints", family, id)),
                    Some(_) => {}
                }
            }
        }
        for (name, a) in &self.auth {
            let url_ok = |u: &str| u.starts_with("https://") || is_loopback(u);
            for (field, v) in [("issuer", &a.issuer), ("token_url", &a.token_url), ("device_authorization_url", &a.device_authorization_url), ("authorization_url", &a.authorization_url)] {
                if let Some(u) = v {
                    if !url_ok(u) {
                        errs.push(format!("[auth.{}] {} must be https", name, field));
                    }
                }
            }
            if a.client_id.trim().is_empty() {
                errs.push(format!("[auth.{}] needs a client_id (Delphi ships none; your identity provider's admin issues one)", name));
            }
            if a.issuer.is_none() && a.token_url.is_none() {
                errs.push(format!("[auth.{}] needs an issuer (OIDC discovery) or a token_url", name));
            }
            match a.flow {
                AuthFlow::DeviceCode if a.issuer.is_none() && a.device_authorization_url.is_none() => errs.push(format!("[auth.{}] the device_code flow needs an issuer or a device_authorization_url", name)),
                AuthFlow::LoopbackPkce if a.issuer.is_none() && a.authorization_url.is_none() => errs.push(format!("[auth.{}] the loopback_pkce flow needs an issuer or an authorization_url", name)),
                AuthFlow::ClientCredentials if a.client_secret.is_none() => errs.push(format!("[auth.{}] the client_credentials flow needs a client_secret = \"keychain:<name>\"", name)),
                _ => {}
            }
            if let Some(sec) = &a.client_secret {
                if !matches!(CredentialRef::parse(sec), Ok(CredentialRef::Keychain(_))) {
                    errs.push(format!("[auth.{}] client_secret must be a keychain reference (`keychain:<name>`), never an inline value", name));
                }
            }
        }
        for (name, c) in &self.connectors {
            let ok_name = !name.is_empty() && name.len() <= 32 && name.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') && !name.starts_with('-');
            if !ok_name {
                errs.push(format!("[connectors.{}] the name must be lowercase letters, digits and `-` (it prefixes every tool name)", name));
            }
            if c.transport != "http" {
                errs.push(format!("[connectors.{}] transport `{}` is not supported (only `http`)", name, c.transport));
            }
            if !(c.url.starts_with("https://") || is_loopback(&c.url)) {
                errs.push(format!("[connectors.{}] url must be https (plain http only to loopback)", name));
            }
            match c.host() {
                None => errs.push(format!("[connectors.{}] url has no host", name)),
                Some(h) if !c.hosts.iter().any(|x| x.to_lowercase() == h) => errs.push(format!("[connectors.{}] hosts must include the url's own host `{}`", name, h)),
                Some(_) => {}
            }
            if c.hosts.is_empty() || c.hosts.iter().any(|h| h.contains('*') || h.contains('/') || h.contains(':') || h.trim().is_empty()) {
                errs.push(format!("[connectors.{}] hosts are exact host names: no wildcards, schemes, paths or ports", name));
            }
            if c.roles.is_empty() {
                errs.push(format!("[connectors.{}] roles is empty, so no agent would be offered these tools", name));
            }
            if c.tools.is_empty() {
                errs.push(format!("[connectors.{}] tools is empty: a connector offers only the tools you list", name));
            }
            for t in c.tools.keys() {
                if t.is_empty() || t.len() > 48 || !t.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-') {
                    errs.push(format!("[connectors.{}] tool name `{}` must be letters, digits, `_` and `-`", name, t));
                }
            }
            if let Some(f) = &c.first_use {
                if f != "ask" && f != "allow" {
                    errs.push(format!("[connectors.{}] first_use must be \"ask\" or \"allow\"", name));
                }
            }
            if c.max_arg_chars == 0 || c.max_arg_chars > 4096 {
                errs.push(format!("[connectors.{}] max_arg_chars must be between 1 and 4096", name));
            }
            if c.identity.as_deref() == Some("google") && !c.hosts.iter().any(|h| h.eq_ignore_ascii_case("openidconnect.googleapis.com")) {
                errs.push(format!("[connectors.{}] identity = \"google\" asks openidconnect.googleapis.com who signed in, so that host must be in `hosts`", name));
            }
            if let Some(i) = &c.identity {
                if i != "google" {
                    errs.push(format!("[connectors.{}] identity `{}` is not supported (only \"google\")", name, i));
                }
            }
            if let Some(l) = &c.login {
                if !self.auth.contains_key(l) {
                    errs.push(format!("[connectors.{}] login names `{}`, but there is no [auth.{}] block", name, l, l));
                }
            }
            if c.account.is_some() && c.identity.is_none() && c.login.is_some() {
                errs.push(format!("[connectors.{}] `account` needs `identity` (how the signed-in identity is looked up, for example \"google\")", name));
            }
            if c.login.is_some() && c.account.is_none() {
                errs.push(format!("[connectors.{}] a connector with a login needs `account`: the identity it must resolve to, so the wrong browser profile cannot sign in", name));
            }
        }
        for e in &self.endpoints {
            if let Ok(CredentialRef::Oauth2(n)) = CredentialRef::parse(&e.credential) {
                if !self.auth.contains_key(&n) {
                    errs.push(format!("endpoint `{}`: credential oauth2:{} has no [auth.{}] block", e.id, n, n));
                }
            }
        }
        for (role, family) in &self.access.role_family {
            if !matches!(family.as_str(), "claude" | "gemini") {
                errs.push(format!("[access] role_family.{} names unknown family `{}` (use `claude` or `gemini`)", role, family));
            } else if !self.endpoints.iter().any(|e| &e.family == family && allowed.contains(&e.id)) {
                errs.push(format!("[access] role_family.{} = `{}`, but no allowed endpoint serves that family", role, family));
            }
        }
        errs
    }

    /// The model family a role is served by.
    pub fn family_for_role(&self, role: &str) -> &str {
        self.access.role_family.get(role).map(String::as_str).unwrap_or("claude")
    }

    /// The compliance pin: endpoints a request may be sent to. Without `allowed_endpoints`, all of them.
    pub fn allowed(&self) -> Vec<String> {
        self.access.allowed_endpoints.clone().unwrap_or_else(|| self.endpoints.iter().map(|e| e.id.clone()).collect())
    }

    pub fn is_allowed(&self, endpoint_id: &str) -> bool {
        self.allowed().iter().any(|a| a == endpoint_id)
    }

    pub fn endpoint(&self, id: &str) -> Option<&Endpoint> {
        self.endpoints.iter().find(|e| e.id == id)
    }

    /// Allowed endpoints for a family in policy order: the explicit `order` first, then any other
    /// allowed endpoint of that family in declaration order. Failover walks this list and never
    /// leaves it (`specs/009` §5).
    pub fn endpoints_for(&self, family: &str) -> Vec<&Endpoint> {
        let allowed = self.allowed();
        let mut out: Vec<&Endpoint> = Vec::new();
        for id in self.access.order.get(family).into_iter().flatten() {
            if let Some(e) = self.endpoint(id) {
                if allowed.contains(id) && !out.iter().any(|o| o.id == e.id) {
                    out.push(e);
                }
            }
        }
        for e in &self.endpoints {
            if e.family == family && allowed.contains(&e.id) && !out.iter().any(|o| o.id == e.id) {
                out.push(e);
            }
        }
        out
    }

    /// Concrete model ID for `tier` on `endpoint`: the endpoint's own map wins over `[models.<family>]`.
    pub fn resolve_model(&self, endpoint: &Endpoint, tier: &str) -> Option<String> {
        endpoint.models.get(tier).cloned().or_else(|| self.models.get(&endpoint.family).and_then(|m| m.get(tier)).cloned())
    }

    pub fn catalog_entry(&self, model_id: &str) -> Option<&CatalogEntry> {
        self.catalog.get(model_id)
    }

    /// Thinking budget to send for `model_id`, or `None` to omit `thinking` entirely
    /// (`specs/002` §2.1, §4): a request of 0 omits it, a budget under the model's minimum is raised
    /// to the minimum, one above the maximum is capped. Models whose catalog minimum is above 0 and
    /// that cannot disable thinking are handled by the caller via `min_thinking_budget`.
    pub fn clamp_thinking(&self, model_id: &str, requested: u32) -> Option<u32> {
        if requested == 0 {
            return None;
        }
        let entry = self.catalog.get(model_id);
        let min = entry.map(|e| e.min_thinking_budget).filter(|m| *m > 0).unwrap_or(1024);
        let max = entry.map(|e| e.max_thinking_budget).filter(|m| *m > 0).unwrap_or(u32::MAX);
        Some(requested.clamp(min, max.max(min)))
    }

    /// What `doctor` should flag: routable models without prices make budget reservation impossible.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        for e in &self.endpoints {
            if e.path == AccessPath::Harness {
                continue;
            }
            for tier in TIERS {
                if let Some(model) = self.resolve_model(e, tier) {
                    match self.catalog.get(&model) {
                        None => out.push(format!("endpoint `{}`: model `{}` ({}) has no [models.catalog] entry", e.id, model, tier)),
                        Some(c) if !c.has_prices() && e.cost_model.as_deref() != Some("subscription") => {
                            out.push(format!("endpoint `{}`: model `{}` has no prices; budget reservation is impossible", e.id, model))
                        }
                        Some(_) => {}
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub const SAMPLE: &str = r#"
[models.claude]
fast = "haiku-x"
workhorse = "sonnet-x"
frontier = "<claude frontier model id>"

[models.catalog."haiku-x"]
max_output_tokens = 8000
input_usd_per_mtok = 1.0
output_usd_per_mtok = 5.0

[models.catalog."sonnet-x"]
max_output_tokens = 64000
min_thinking_budget = 1024
max_thinking_budget = 32000
input_usd_per_mtok = 3.0
cache_read_usd_per_mtok = 0.3
cache_write_usd_per_mtok = 3.75
output_usd_per_mtok = 15.0

[[endpoints]]
id = "direct"
family = "claude"
path = "api"
credential = "keychain:anthropic"

[[endpoints]]
id = "bedrock-us"
family = "claude"
path = "cloud"
platform = "bedrock"
region = "us-east-1"
credential = "aws:work-sso"
models = { workhorse = "bedrock-sonnet-x" }

[[endpoints]]
id = "corp-gateway"
family = "claude"
path = "gateway-native"
base_url = "https://llm-gateway.example.internal/anthropic"
credential = "command:[\"corp-token\",\"--json\"]"
headers = { "X-Cost-Center" = "eng-123" }

[access]
order = { claude = ["corp-gateway", "bedrock-us"] }
allowed_endpoints = ["corp-gateway", "bedrock-us"]
"#;

    #[test]
    fn parses_and_resolves_models_with_endpoint_overrides() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.endpoints.len(), 3);
        let bedrock = c.endpoint("bedrock-us").unwrap();
        assert_eq!(c.resolve_model(bedrock, "workhorse").as_deref(), Some("bedrock-sonnet-x"), "endpoint map wins");
        assert_eq!(c.resolve_model(bedrock, "fast").as_deref(), Some("haiku-x"), "falls back to [models.claude]");
        assert_eq!(c.resolve_model(bedrock, "frontier"), None, "template placeholders are unset");
        assert!((c.catalog_entry("sonnet-x").unwrap().price().cache_write - 3.75).abs() < 1e-9);
    }

    #[test]
    fn the_pin_applies_to_ordering_and_failover() {
        let c = Config::parse(SAMPLE).unwrap();
        assert!(!c.is_allowed("direct"));
        let ids: Vec<&str> = c.endpoints_for("claude").iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["corp-gateway", "bedrock-us"], "policy order, and `direct` is outside the pin");
    }

    #[test]
    fn unpinned_config_allows_everything_in_declaration_order() {
        let text = SAMPLE.split("[access]").next().unwrap();
        let c = Config::parse(text).unwrap();
        let ids: Vec<&str> = c.endpoints_for("claude").iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["direct", "bedrock-us", "corp-gateway"]);
    }

    #[test]
    fn validation_reports_every_problem() {
        let bad = r#"
[[endpoints]]
id = "a"
family = "claude"
path = "cloud"
platform = "vertex"
credential = "gcloud-adc"
[[endpoints]]
id = "a"
family = "claude"
path = "gateway-native"
base_url = "http://evil.example.com"
credential = "whatever:x"
[[endpoints]]
id = "h"
family = "claude"
path = "harness"
credential = "harness-login"
[access]
allowed_endpoints = ["a", "ghost"]
order = { gemini = ["a"] }
"#;
        let errs = Config::parse(bad).unwrap_err().join("\n");
        for needle in ["defined twice", "need a region", "need a project", "must be https", "unknown credential source", "harness endpoints need", "unknown endpoint `ghost`", "belongs to family `claude`"] {
            assert!(errs.contains(needle), "missing `{}` in:\n{}", needle, errs);
        }
    }

    #[test]
    fn role_family_must_name_a_known_family_with_an_allowed_endpoint() {
        let base = "[[endpoints]]\nid = \"d\"\nfamily = \"claude\"\npath = \"api\"\ncredential = \"env:K\"\n";
        let gem = "[[endpoints]]\nid = \"g\"\nfamily = \"gemini\"\npath = \"api\"\ncredential = \"env:K\"\n";
        let ok = Config::parse(&format!("{}{}[access]\nrole_family = {{ architect = \"gemini\" }}\n", base, gem)).unwrap();
        assert_eq!((ok.family_for_role("architect"), ok.family_for_role("builder")), ("gemini", "claude"));
        let errs = Config::parse(&format!("{}[access]\nrole_family = {{ architect = \"gemini\", scribe = \"gpt\" }}\n", base)).unwrap_err().join("\n");
        assert!(errs.contains("no allowed endpoint serves that family") && errs.contains("unknown family `gpt`"), "{}", errs);
        let pinned = Config::parse(&format!("{}{}[access]\nallowed_endpoints = [\"d\"]\nrole_family = {{ architect = \"gemini\" }}\n", base, gem)).unwrap_err().join("\n");
        assert!(pinned.contains("no allowed endpoint serves that family"), "the pin counts: {}", pinned);
    }

    #[test]
    fn loopback_http_is_allowed_for_local_gateways_and_tests() {
        let t = "[[endpoints]]\nid=\"l\"\nfamily=\"claude\"\npath=\"gateway-native\"\nbase_url=\"http://127.0.0.1:9999\"\ncredential=\"env:K\"\n";
        assert!(Config::parse(t).is_ok());
    }

    #[test]
    fn thinking_budget_clamping() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.clamp_thinking("sonnet-x", 0), None, "0 omits thinking entirely");
        assert_eq!(c.clamp_thinking("sonnet-x", 500), Some(1024), "below the minimum is raised");
        assert_eq!(c.clamp_thinking("sonnet-x", 8192), Some(8192));
        assert_eq!(c.clamp_thinking("sonnet-x", 90_000), Some(32_000), "capped at the model maximum");
        assert_eq!(c.clamp_thinking("unknown-model", 100), Some(1024), "provider minimum when the catalog is silent");
    }

    #[test]
    fn doctor_flags_unpriced_and_uncataloged_models() {
        let c = Config::parse(SAMPLE).unwrap();
        let p = c.problems().join("\n");
        assert!(p.contains("bedrock-sonnet-x") && p.contains("no [models.catalog] entry"), "{}", p);
        let priced = SAMPLE.replace("input_usd_per_mtok = 1.0", "input_usd_per_mtok = 0.0");
        assert!(Config::parse(&priced).unwrap().problems().join("\n").contains("no prices"));
    }

    const GOOD_CONNECTOR: &str = r#"
[auth.google]
flow = "loopback_pkce"
issuer = "https://accounts.google.com"
client_id = "cid"
[connectors.docs]
url = "https://mcp.example.com/mcp"
hosts = ["mcp.example.com", "openidconnect.googleapis.com"]
login = "google"
account = "me@example.com"
identity = "google"
roles = ["orchestrator"]
tools = { search = "read", note = "draft", send = "write" }
"#;

    #[test]
    fn a_connector_catalog_parses_with_defaults_and_classes() {
        let c = Config::parse(GOOD_CONNECTOR).unwrap();
        let d = &c.connectors["docs"];
        assert_eq!((d.max_arg_chars, d.calls_per_minute, d.max_result_chars, d.transport.as_str()), (128, 30, 20_000, "http"));
        assert_eq!(d.tools["send"], ToolClass::Write);
        assert_eq!(d.tools["note"], ToolClass::Draft);
        assert_eq!(d.host().as_deref(), Some("mcp.example.com"));
    }

    #[test]
    fn a_connector_catalog_is_validated_strictly() {
        let bad = |edit: &dyn Fn(&str) -> String| Config::parse(&edit(GOOD_CONNECTOR)).unwrap_err().join("\n");
        assert!(bad(&|t| t.replace("https://mcp.example.com/mcp", "http://mcp.example.com/mcp")).contains("must be https"));
        assert!(bad(&|t| t.replace("\"mcp.example.com\", ", "\"other.example.com\", ")).contains("hosts must include the url's own host"));
        assert!(bad(&|t| t.replace(", \"openidconnect.googleapis.com\"", "")).contains("openidconnect.googleapis.com"));
        assert!(bad(&|t| t.replace("hosts = [\"mcp.example.com\", ", "hosts = [\"*.example.com\", \"mcp.example.com\", ")).contains("no wildcards"));
        assert!(bad(&|t| t.replace("roles = [\"orchestrator\"]", "roles = []")).contains("roles is empty"));
        assert!(bad(&|t| t.replace("tools = { search = \"read\", note = \"draft\", send = \"write\" }", "tools = {}")).contains("tools is empty"));
        assert!(bad(&|t| t.replace("search = \"read\"", "\"bad name\" = \"read\"")).contains("tool name"));
        assert!(bad(&|t| t.replace("search = \"read\"", "search = \"admin\"")).contains("[connectors.*]"), "an unknown class is a parse error");
        assert!(bad(&|t| t.replace("login = \"google\"", "login = \"nowhere\"")).contains("no [auth.nowhere] block"));
        assert!(bad(&|t| t.replace("account = \"me@example.com\"\n", "")).contains("needs `account`"), "a login without a pinned account is refused");
        assert!(bad(&|t| t.replace("[connectors.docs]", "[connectors.Docs_X]")).contains("lowercase"));
        assert!(bad(&|t| t.replace("[connectors.docs]", "[connectors.docs]\nfirst_use = \"sometimes\"")).contains("first_use"));
        assert!(bad(&|t| t.replace("[connectors.docs]", "[connectors.docs]\nmax_arg_chars = 100000")).contains("max_arg_chars"));
    }
}
