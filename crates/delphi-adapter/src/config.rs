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

impl Endpoint {
    /// Can a role that writes files or runs commands be served here? Every API path can. A subscription
    /// harness can only if it has a per-action approval bridge: `claude` does, `agy` does not (`specs/009` §7B),
    /// so `agy` is limited to read-only roles and Council seats. (The engine adds an `agy` seat whose gate it has seen
    /// hold, `specs/020` §8.7; that is a fact about the installed CLI, not about the configuration.)
    pub fn can_mutate(&self) -> bool {
        self.path != AccessPath::Harness || self.harness.as_deref().unwrap_or("claude") == "claude"
    }

    /// Is a call here paid for by a subscription metered in tokens, rather than priced from the catalog?
    pub fn is_metered_by_subscription(&self) -> bool {
        self.path == AccessPath::Harness || self.cost_model.as_deref() == Some("subscription")
    }

    /// A model running on this machine (`cost_model = "local"`): it costs nothing, so it needs no prices to be reserved against.
    pub fn is_local(&self) -> bool {
        self.cost_model.as_deref() == Some("local")
    }
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

/// Who answers the tool-permission questions a subscription CLI raises (`specs/020` §5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalSource {
    /// Delphi's own tiers, taint gates and cards decide every request.
    Delphi,
    /// For roles that do not edit files or run commands, the CLI's own policy decides (organization settings and
    /// its auto-mode classifier); Delphi is asked only about what that policy cannot settle. Builders always stay on
    /// Delphi's rules. This is the default: the organization's own policy is the stronger authority (operator, 2026-10-25).
    #[default]
    Delegate,
}

/// The CLI's own tools every role without a worktree gets unless a profile says otherwise. File access is not here on
/// purpose: Delphi's own file tools carry the role's scope and the provenance stamps the CLI's cannot.
pub const DEFAULT_NATIVE_TOOLS: [&str; 2] = ["WebSearch", "WebFetch"];

fn default_native_tools() -> Vec<String> {
    DEFAULT_NATIVE_TOOLS.iter().map(|s| s.to_string()).collect()
}

/// One `[harness.<name>]` block (`specs/020` §3). Optional: with no block every default below applies, so nothing has to
/// be ratified to use them. A block is pinned by hash with `[access]`, so a local edit cannot widen what it narrows.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HarnessProfile {
    /// Offer the role's Delphi tools to the CLI over a per-span MCP bridge. Off means the old text-only seat.
    #[serde(default = "yes")]
    pub bridge: bool,
    #[serde(default)]
    pub approvals: ApprovalSource,
    /// The CLI's own built-in tools given to roles that have no worktree. Builders always get the CLI's standard set.
    #[serde(default = "default_native_tools")]
    pub native_tools: Vec<String>,
    /// Let the CLI load the MCP servers its sign-in provides (claude.ai connectors the organization set up, and any the
    /// operator configured for the CLI). Off, the CLI is started with only the Delphi bridge (`--strict-mcp-config`).
    #[serde(default = "yes")]
    pub claudeai_connectors: bool,
    /// Exact names (`mcp__<server>__<tool>`) of the account's connector tools that only read. A listed tool is allowed
    /// without a card; any other connector tool the CLI asks about is treated as a possible write (a Red card).
    #[serde(default)]
    pub connector_reads: Vec<String>,
    /// Extra environment variable names passed through to the CLI, for setups that configure it that way
    /// (`ANTHROPIC_BASE_URL`, `AWS_PROFILE`, `CLAUDE_CODE_USE_BEDROCK`). Names only; the values come from the daemon's environment.
    #[serde(default)]
    pub env_allow: Vec<String>,
    /// Reserved for the strict (sandboxed) profile of `specs/009` §7; read and ignored today.
    #[serde(default)]
    pub profile: Option<String>,
    /// CLI features (`specs/021`): `effort`, `max_turns`, `max_budget_usd`, `fallback_models`, `partial_messages`, `session`,
    /// `json_schema`, `add_dirs`, `agents`, `settings`, and for agy `mode`, `sandbox`, `print_timeout_secs`. They are kept as
    /// raw values here so the block stays ratified and strict about unknown keys; the engine checks and reads them
    /// (`harness_opts.rs`), and reports a bad value on attach.
    #[serde(default)]
    pub effort: Option<toml::Value>,
    #[serde(default)]
    pub json_schema: Option<toml::Value>,
    #[serde(default)]
    pub session: Option<toml::Value>,
    #[serde(default)]
    pub add_dirs: Option<toml::Value>,
    #[serde(default)]
    pub sandbox: Option<toml::Value>,
    #[serde(default)]
    pub print_timeout_secs: Option<toml::Value>,
    #[serde(default)]
    pub max_turns: Option<toml::Value>,
    #[serde(default)]
    pub max_budget_usd: Option<toml::Value>,
    #[serde(default)]
    pub fallback_models: Option<toml::Value>,
    #[serde(default)]
    pub partial_messages: Option<toml::Value>,
    #[serde(default)]
    pub agents: Option<toml::Value>,
    #[serde(default)]
    pub settings: Option<toml::Value>,
    #[serde(default)]
    pub mode: Option<toml::Value>,
    /// Copilot only (`specs/051` §8): `"gh"` lets `delphi doctor` read the account's quota snapshot through the operator's `gh`
    /// login (a credential Delphi does not otherwise hold, on an undocumented endpoint). Off when absent or `"off"`.
    #[serde(default)]
    pub quota_source: Option<String>,
}

impl Default for HarnessProfile {
    fn default() -> Self {
        HarnessProfile { bridge: true, approvals: ApprovalSource::Delegate, native_tools: default_native_tools(), claudeai_connectors: true, connector_reads: vec![], env_allow: vec![], profile: None, effort: None, json_schema: None, session: None, add_dirs: None, sandbox: None, print_timeout_secs: None, max_turns: None, max_budget_usd: None, fallback_models: None, partial_messages: None, agents: None, settings: None, mode: None, quota_source: None }
    }
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
    /// Role → model family (`claude`, `gemini`, `openai` or `local`) for roles that should not use the default (`primary`).
    #[serde(default)]
    pub role_family: BTreeMap<String, String>,
    /// The family roles are served by unless `role_family` says otherwise (`specs/035` D-1). Unset: `claude` when an allowed
    /// Claude endpoint exists, else the first allowed family.
    #[serde(default)]
    pub primary: Option<String>,
    /// The families tried, in this order, when the primary has nothing usable (`specs/035` D-2). Unlisted ones follow in declaration order, `local` last.
    #[serde(default)]
    pub failover: Vec<String>,
    /// What to do when only a weaker or reduced model is left (`specs/035` D-4).
    #[serde(default)]
    pub on_weaker: OnWeaker,
}

/// `[access] on_weaker`: `ask` puts a card to the operator; `finish` lets a task already under way carry on without one;
/// `never` runs nothing weaker and waits for the primary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OnWeaker {
    #[default]
    Ask,
    Finish,
    Never,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct CatalogEntry {
    pub max_output_tokens: u64,
    #[serde(default)]
    pub min_thinking_budget: u32,
    #[serde(default)]
    pub max_thinking_budget: u32,
    /// The context window in tokens; 0 means unknown, which routing never treats as too small.
    #[serde(default)]
    pub context_tokens: u64,
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
    /// A reduced model is never a peer of one that is not, whatever tier it is mapped to (`specs/035` D-3). Unset: reduced
    /// when the endpoint is `cost_model = "local"`.
    #[serde(default)]
    pub reduced: Option<bool>,
    /// The lineage the model belongs to when that is not its endpoint's family: a Claude model served from Gemini's own pool is
    /// `lineage = "claude"` (`specs/035` D-10). Unset: the endpoint's family.
    #[serde(default)]
    pub lineage: Option<String>,
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

/// Thresholds of the model pool (`specs/002` §4.1). Every field is optional in `[routing.pool]`.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolTuning {
    /// Consecutive throttles of one `(endpoint, model)` that, over `throttle_window_secs`, count as exhaustion.
    pub throttle_attempts: u32,
    pub throttle_window_secs: i64,
    /// First re-probe of an exhausted model with no known reset time, then doubling up to `reprobe_max_secs`.
    pub reprobe_first_secs: i64,
    pub reprobe_max_secs: i64,
}

impl Default for PoolTuning {
    fn default() -> PoolTuning {
        PoolTuning { throttle_attempts: 3, throttle_window_secs: 600, reprobe_first_secs: 900, reprobe_max_secs: 4 * 3600 }
    }
}

/// How a dispatch picks its model (`specs/056`): `static` is the role's tier as before (the default, until a benchmark shows
/// `discovered` no worse, 056 OD-5); `discovered` assigns by capability class, evidence and quota.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Assignment {
    #[default]
    Static,
    Discovered,
}

impl Assignment {
    pub fn parse(s: &str) -> Option<Assignment> {
        match s {
            "static" => Some(Assignment::Static),
            "discovered" => Some(Assignment::Discovered),
            _ => None,
        }
    }
}

/// How the orchestrator learns of bundle agents (`specs/041` §4, §5.7): `flat` is the one-line-per-agent list as before (the
/// default, until the labeled set meets the recall bar); `auto` swaps in the domain index and `search_agent_registry` once
/// there are more agents than the list holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RegistryMode {
    #[default]
    Flat,
    Auto,
}

impl RegistryMode {
    pub fn parse(s: &str) -> Option<RegistryMode> {
        match s {
            "flat" => Some(RegistryMode::Flat),
            "auto" => Some(RegistryMode::Auto),
            _ => None,
        }
    }
}

/// Results per `(role, kind, model)` that make measured evidence "enough" (`specs/056` OD-3).
pub const DEFAULT_EVIDENCE_ENOUGH: u32 = 8;

/// Largest grade, up or down (`specs/056` §4).
pub const GRADE_LIMIT: i32 = 2;

/// One role's grades: the role's own and one per task kind (`specs/056` §4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoleGrade {
    pub role: i32,
    pub kinds: BTreeMap<String, i32>,
}

/// `[grades]` (ratified): a small signed integer per role, `architect = 1`, or per kind, `[grades.builder] docs = -1` with
/// `default` for the role itself. Positive promotes, negative nerfs, within ±2.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grades {
    pub roles: BTreeMap<String, RoleGrade>,
}

impl Grades {
    /// The grade for `role`, for `kind` when one is named and set, else the role's own; 0 when none.
    pub fn of(&self, role: &str, kind: Option<&str>) -> i32 {
        let Some(g) = self.roles.get(role) else { return 0 };
        kind.and_then(|k| g.kinds.get(k).copied()).unwrap_or(g.role)
    }

    pub fn parse(v: &toml::Value) -> Result<Grades, Vec<String>> {
        let Some(t) = v.as_table() else { return Err(vec!["[grades] must be a table".into()]) };
        let mut out = Grades::default();
        let mut errs = Vec::new();
        let mut one = |what: String, x: &toml::Value| match x.as_integer() {
            Some(n) if n.abs() <= GRADE_LIMIT as i64 => Some(n as i32),
            _ => {
                errs.push(format!("[grades] {what} must be a whole number from -{GRADE_LIMIT} to {GRADE_LIMIT}"));
                None
            }
        };
        for (role, x) in t {
            let mut g = RoleGrade::default();
            match x {
                toml::Value::Table(kinds) => {
                    for (k, n) in kinds {
                        let Some(n) = one(format!("{role}.{k}"), n) else { continue };
                        if k == "default" { g.role = n } else { g.kinds.insert(k.clone(), n); }
                    }
                }
                n => g.role = one(role.clone(), n).unwrap_or(0),
            }
            out.roles.insert(role.clone(), g);
        }
        if errs.is_empty() { Ok(out) } else { Err(errs) }
    }
}

/// `[routing]`: the model pool and per-role targets (`specs/002` §4.1, D-26). Other keys in the table
/// (`enable_dynamic_scaling`, …) are not read here.
#[derive(Debug, Clone, PartialEq)]
pub struct Routing {
    /// Model IDs in preference order, as written; `None` when `[routing.pool]` has no `order`.
    pub pool: Option<Vec<String>>,
    /// Role → target model ID.
    pub targets: BTreeMap<String, String>,
    pub tuning: PoolTuning,
    /// `[routing] assignment` (`specs/056`).
    pub assignment: Assignment,
    /// `[routing] evidence_enough`: measured results that make a model scored by them alone.
    pub evidence_enough: u32,
    /// `[routing.reserve] <role>`: the share (0 to 1) of the mission budget kept for that role; other roles cannot spend it.
    pub reserve: BTreeMap<String, f64>,
    /// `[routing] registry` (`specs/041`).
    pub registry: RegistryMode,
}

impl Default for Routing {
    fn default() -> Routing {
        Routing { pool: None, targets: BTreeMap::new(), tuning: PoolTuning::default(), assignment: Assignment::Static, evidence_enough: DEFAULT_EVIDENCE_ENOUGH, reserve: BTreeMap::new(), registry: RegistryMode::Flat }
    }
}

#[derive(Deserialize, Default)]
struct PoolRaw {
    order: Option<Vec<String>>,
    throttle_attempts: Option<u32>,
    throttle_window_secs: Option<i64>,
    reprobe_first_secs: Option<i64>,
    reprobe_max_secs: Option<i64>,
}

/// One slot of the pool: a model on one endpoint. Quotas belong to this pair, not to the model alone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PoolKey {
    pub endpoint_id: String,
    pub model_id: String,
    pub tier: String,
    pub family: String,
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub endpoints: Vec<Endpoint>,
    pub access: Access,
    pub routing: Routing,
    /// family → tier → model ID.
    pub models: BTreeMap<String, BTreeMap<String, String>>,
    pub catalog: BTreeMap<String, CatalogEntry>,
    pub auth: BTreeMap<String, AuthBlock>,
    pub connectors: BTreeMap<String, Connector>,
    /// `[harness.<cli>]` (`claude`, `agy`): how Delphi runs and equips each subscription CLI.
    pub harness: BTreeMap<String, HarnessProfile>,
    /// `[search]`: the engines the archives pane's research job asks, in order (`specs/028` §10).
    pub search: crate::research::SearchConfig,
    /// `[validator]`: the fact validator's mode and the operator's additions to the source list (`specs/040` §15.1, §6.1).
    pub validator: ValidatorConfig,
    /// `[grades]`: promote and nerf, per role and optionally per task kind (`specs/056` §4).
    pub grades: Grades,
    /// `[library]`: generated search pages (`specs/057` §5, §6) and the acquisition job (`specs/055` §8).
    pub library: LibraryConfig,
    /// `[learning]`: capture and the bounds on the learning loop (`specs/031` §3.6). The engine clamps each value to the core's bounds.
    pub learning: LearningConfig,
    /// `[mcp]`: the server for host agents (`specs/062` §3.4).
    pub mcp: McpConfig,
}

/// `[mcp]` (`specs/062` §3.2, §3.4). `enabled` is the operator's switch; `allow_remote` is reserved for a later spec and
/// cannot be turned on in this version.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct McpConfig {
    pub enabled: bool,
    pub allow_remote: bool,
}

impl Default for McpConfig {
    fn default() -> McpConfig {
        McpConfig { enabled: true, allow_remote: false }
    }
}

impl McpConfig {
    /// The `[mcp]` settings in a `config.toml`'s text, or `None` when they cannot be read as written or are out of range
    /// (the daemon then refuses, as the server does: fail closed).
    pub fn read(text: &str) -> Option<McpConfig> {
        let root = toml::from_str::<toml::Value>(text).ok()?;
        let c = match root.get("mcp") {
            None => McpConfig::default(),
            Some(v) => v.clone().try_into::<McpConfig>().ok()?,
        };
        (!c.allow_remote).then_some(c)
    }

    /// Whether the server answers tool calls, read from a `config.toml`'s text. Anything that cannot be read as the operator
    /// wrote it (invalid TOML, a wrong type under `[mcp]`) is "off": the switch fails closed.
    pub fn enabled_in(text: &str) -> bool {
        let Ok(root) = toml::from_str::<toml::Value>(text) else { return false };
        match root.get("mcp") {
            None => true,
            Some(v) => v.clone().try_into::<McpConfig>().map(|c| c.enabled && !c.allow_remote).unwrap_or(false),
        }
    }
}

/// `[library]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryConfig {
    pub pages: PagesConfig,
    pub acquire: AcquireConfig,
}

/// Whether a page may use external search: `ask` (the default) needs `--external` or the console's key each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PagesExternal {
    #[default]
    Ask,
    Off,
    On,
}

/// `[library.pages]` (`specs/057` §6). `external_ttl` is how long a page that used external search stays fresh.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PagesConfig {
    pub enabled: bool,
    #[serde(deserialize_with = "de_external")]
    pub external: PagesExternal,
    pub max_tokens: u64,
    #[serde(deserialize_with = "de_ttl", rename = "external_ttl")]
    pub external_ttl_secs: u64,
    /// Pages the daemon builds in one day; a cache hit is free. They also count against the validator's share (`specs/040` §20).
    pub per_day: u32,
}

impl Default for PagesConfig {
    fn default() -> Self {
        PagesConfig { enabled: true, external: PagesExternal::Ask, max_tokens: 3000, external_ttl_secs: 7 * 86_400, per_day: 20 }
    }
}

fn de_external<'de, D: serde::Deserializer<'de>>(d: D) -> Result<PagesExternal, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    match s.as_str() {
        "ask" => Ok(PagesExternal::Ask),
        "off" => Ok(PagesExternal::Off),
        "on" => Ok(PagesExternal::On),
        _ => Err(serde::de::Error::custom(format!("external `{}` is not one of ask, off, on", s))),
    }
}

/// `"7d"`, `"12h"`, `"90m"` or a bare number of days.
pub fn parse_ttl(s: &str) -> Option<u64> {
    let s = s.trim();
    let (n, unit) = match s.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((i, _)) => s.split_at(i),
        None => (s, "d"),
    };
    let n: u64 = n.parse().ok()?;
    let per = match unit.trim() {
        "d" => 86_400,
        "h" => 3_600,
        "m" => 60,
        _ => return None,
    };
    n.checked_mul(per).filter(|v| *v > 0)
}

fn de_ttl<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum V {
        N(u64),
        S(String),
    }
    let secs = match <V as serde::Deserialize>::deserialize(d)? {
        V::N(n) => parse_ttl(&n.to_string()),
        V::S(s) => parse_ttl(&s),
    };
    secs.ok_or_else(|| serde::de::Error::custom("external_ttl is a duration such as \"7d\", \"12h\" or \"90m\""))
}

/// `[learning]`. Types are checked here so `delphi doctor` names a wrong one; the floors and ceilings are the engine's
/// (`delphi_engine::learn::LearningSettings`). No key loads unratified content, and an unknown key is a boot notice.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct LearningConfig {
    pub capture: Option<bool>,
    pub max_notes: Option<i64>,
    pub triggers: Option<Vec<String>>,
    pub retro_min_tasks: Option<i64>,
}

/// `[validator] mode` (`specs/040` §15.1). Ordered so that `min` is the stricter of two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ValidatorMode {
    Off,
    #[default]
    Advisory,
    Ratify,
}

impl ValidatorMode {
    pub fn parse(s: &str) -> Option<ValidatorMode> {
        Some(match s {
            "off" => ValidatorMode::Off,
            "advisory" => ValidatorMode::Advisory,
            "ratify" => ValidatorMode::Ratify,
            _ => return None,
        })
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ValidatorMode::Off => "off",
            ValidatorMode::Advisory => "advisory",
            ValidatorMode::Ratify => "ratify",
        }
    }
}

/// `[validator]`. The domain lists extend Appendix A and are the operator's alone (a ratified setting, GOVERNANCE §5.7).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValidatorConfig {
    #[serde(deserialize_with = "de_mode")]
    pub mode: ValidatorMode,
    pub sources: ValidatorSources,
    /// What to do with a claim supported only by Weak sources (`specs/040` §27): keep it labelled, or refuse it.
    #[serde(deserialize_with = "de_weak")]
    pub weak_sources: WeakSources,
}

/// `[validator] weak_sources`. Weak sources never count as evidence; this only says whether a claim they alone support is kept.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WeakSources {
    #[default]
    Label,
    Off,
}

impl WeakSources {
    pub fn as_str(&self) -> &'static str {
        match self {
            WeakSources::Label => "label",
            WeakSources::Off => "off",
        }
    }
}

fn de_weak<'de, D: serde::Deserializer<'de>>(d: D) -> Result<WeakSources, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    match s.as_str() {
        "label" => Ok(WeakSources::Label),
        "off" => Ok(WeakSources::Off),
        _ => Err(serde::de::Error::custom(format!("weak_sources `{}` is not one of label, off", s))),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValidatorSources {
    pub primary: Vec<String>,
    pub reputable: Vec<String>,
    pub weak: Vec<String>,
}

fn de_mode<'de, D: serde::Deserializer<'de>>(d: D) -> Result<ValidatorMode, D::Error> {
    let s = <String as serde::Deserialize>::deserialize(d)?;
    ValidatorMode::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("mode `{}` is not one of off, advisory, ratify", s)))
}

/// `[library.acquire]` (`specs/055` §8): the acquisition job proposes public-domain and research sources, in one batch in the digest.
/// Off until the operator turns it on, because it reaches out to archives. Ratified, since it names where the daemon fetches from.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AcquireConfig {
    pub enabled: bool,
    /// Bytes acquisition may hold in the library, in MiB. The library budget (`specs/055` §3.6) still applies; nothing is evicted.
    pub byte_budget_mib: u64,
    /// The largest text one acquired source may have, in MiB.
    pub max_item_mib: u64,
    /// Sources in one proposed batch.
    pub batch_size: usize,
    /// The scout's share of the install's tokens over the trailing week, in percent (`specs/040` §20.2). 0 keeps the job model-free.
    pub token_share_percent: u64,
    /// Topics the operator wants, beside the workspace's project names.
    pub interests: Vec<String>,
}

impl Default for AcquireConfig {
    fn default() -> Self {
        AcquireConfig { enabled: false, byte_budget_mib: 2048, max_item_mib: 16, batch_size: 8, token_share_percent: 5, interests: Vec::new() }
    }
}

impl AcquireConfig {
    fn check(&self) -> Result<(), String> {
        if self.byte_budget_mib == 0 || self.max_item_mib == 0 {
            return Err("byte_budget_mib and max_item_mib must be at least 1".into());
        }
        if !(1..=20).contains(&self.batch_size) {
            return Err("batch_size must be between 1 and 20".into());
        }
        if self.token_share_percent > 10 {
            return Err("token_share_percent is at most 10 (specs/040 section 20.2)".into());
        }
        if self.interests.len() > 50 || self.interests.iter().any(|i| i.trim().is_empty() || i.chars().count() > 80) {
            return Err("interests: at most 50 topics of 1 to 80 characters".into());
        }
        Ok(())
    }
}

const TIERS: [&str; 3] = ["fast", "workhorse", "frontier"];

pub(crate) fn is_loopback(url: &str) -> bool {
    let rest = url.strip_prefix("http://").unwrap_or("");
    // A bracketed IPv6 literal contains colons, so it is taken whole before the port is split off.
    let host = if rest.starts_with('[') { rest.split_inclusive(']').next().unwrap_or("") } else { rest.split(['/', ':']).next().unwrap_or("") };
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
        if let Some(g) = root.get("grades") {
            match Grades::parse(g) {
                Ok(v) => cfg.grades = v,
                Err(es) => errs.extend(es),
            }
        }
        if let Some(s) = root.get("search") {
            match crate::research::SearchConfig::parse(s, cfg.endpoints.iter().any(|e| e.family == "gemini")) {
                Ok(v) => cfg.search = v,
                Err(es) => errs.extend(es),
            }
        }
        if let Some(v) = root.get("validator") {
            match v.clone().try_into::<ValidatorConfig>() {
                Ok(v) => cfg.validator = v,
                Err(e) => errs.push(format!("[validator]: {}", e)),
            }
        }
        if let Some(v) = root.get("library") {
            match v.clone().try_into::<LibraryConfig>() {
                Ok(v) => match v.acquire.check() {
                    Ok(()) => cfg.library = v,
                    Err(e) => errs.push(format!("[library.acquire]: {}", e)),
                },
                Err(e) => errs.push(format!("[library]: {}", e)),
            }
        }
        if let Some(v) = root.get("learning") {
            match v.clone().try_into::<LearningConfig>() {
                Ok(v) => cfg.learning = v,
                Err(e) => errs.push(format!("[learning]: {}", e)),
            }
        }
        if let Some(v) = root.get("mcp") {
            match v.clone().try_into::<McpConfig>() {
                Ok(v) if v.allow_remote => errs.push("[mcp] allow_remote: remote access is not supported in this version (specs/062 §3.2)".to_string()),
                Ok(v) => cfg.mcp = v,
                Err(e) => errs.push(format!("[mcp]: {}", e)),
            }
        }
        // `[harness]` also carries scalar settings of its own (`subscription_budget_tokens`); only its sub-tables are profiles.
        if let Some(h) = root.get("harness").and_then(|h| h.as_table()) {
            for (name, v) in h.iter().filter(|(_, v)| v.is_table()) {
                match v.clone().try_into::<HarnessProfile>() {
                    Ok(p) => {
                        cfg.harness.insert(name.clone(), p);
                    }
                    Err(e) => errs.push(format!("[harness.{}]: {}", name, e)),
                }
            }
        }
        if let Some(r) = root.get("routing").and_then(|r| r.as_table()) {
            if let Some(p) = r.get("pool") {
                match p.clone().try_into::<PoolRaw>() {
                    Ok(raw) => {
                        cfg.routing.pool = raw.order;
                        let d = PoolTuning::default();
                        cfg.routing.tuning = PoolTuning {
                            throttle_attempts: raw.throttle_attempts.unwrap_or(d.throttle_attempts),
                            throttle_window_secs: raw.throttle_window_secs.unwrap_or(d.throttle_window_secs),
                            reprobe_first_secs: raw.reprobe_first_secs.unwrap_or(d.reprobe_first_secs),
                            reprobe_max_secs: raw.reprobe_max_secs.unwrap_or(d.reprobe_max_secs),
                        };
                    }
                    Err(e) => errs.push(format!("[routing.pool]: {}", e)),
                }
            }
            if let Some(t) = r.get("targets") {
                match t.clone().try_into::<BTreeMap<String, String>>() {
                    // Template placeholders such as "<model id>" count as unset, like `[models.*]`.
                    Ok(v) => cfg.routing.targets = v.into_iter().filter(|(_, id)| !id.trim().is_empty() && !id.starts_with('<')).collect(),
                    Err(e) => errs.push(format!("[routing.targets]: {}", e)),
                }
            }
            if let Some(a) = r.get("assignment") {
                match a.as_str().and_then(Assignment::parse) {
                    Some(v) => cfg.routing.assignment = v,
                    None => errs.push("[routing] assignment must be \"static\" or \"discovered\"".into()),
                }
            }
            if let Some(a) = r.get("registry") {
                match a.as_str().and_then(RegistryMode::parse) {
                    Some(v) => cfg.routing.registry = v,
                    None => errs.push("[routing] registry must be \"flat\" or \"auto\"".into()),
                }
            }
            if let Some(n) = r.get("evidence_enough") {
                match n.as_integer().filter(|n| *n >= 1) {
                    Some(n) => cfg.routing.evidence_enough = n as u32,
                    None => errs.push("[routing] evidence_enough must be a whole number of at least 1".into()),
                }
            }
            if let Some(t) = r.get("reserve") {
                match t.clone().try_into::<BTreeMap<String, f64>>() {
                    Ok(v) if v.values().all(|x| (0.0..=1.0).contains(x)) && v.values().sum::<f64>() <= 1.0 => cfg.routing.reserve = v,
                    Ok(_) => errs.push("[routing.reserve] each share must be between 0 and 1, and together at most 1".into()),
                    Err(e) => errs.push(format!("[routing.reserve]: {}", e)),
                }
            }
            if let Some(list) = &cfg.routing.pool {
                // Placeholders in a template count as unset, so an untouched template parses.
                cfg.routing.pool = Some(list.iter().filter(|id| !id.trim().is_empty() && !id.starts_with('<')).cloned().collect()).filter(|v: &Vec<String>| !v.is_empty());
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
            if e.credential == "none" && !(e.path == AccessPath::GatewayCompat && e.base_url.as_deref().is_some_and(is_loopback)) {
                errs.push(format!("endpoint `{}`: credential \"none\" is only for a local server: a gateway-compat endpoint whose base_url is http://localhost, 127.0.0.1 or [::1]", e.id));
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
        for (name, h) in &self.harness {
            if !matches!(name.as_str(), "claude" | "agy" | "codex" | "copilot") {
                errs.push(format!("[harness.{}] only `claude`, `agy`, `codex` and `copilot` are known harnesses", name));
            }
            let ident = |s: &str| !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            for t in &h.native_tools {
                if !ident(t) {
                    errs.push(format!("[harness.{}] native_tools entry `{}` must be a tool name (letters, digits, `_`)", name, t));
                }
            }
            for t in &h.connector_reads {
                if !(t.starts_with("mcp__") && t.len() <= 128 && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))) {
                    errs.push(format!("[harness.{}] connector_reads entry `{}` must be an exact tool name like `mcp__<server>__<tool>`", name, t));
                }
            }
            for v in &h.env_allow {
                if !(ident(v) && v.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_') && !v.starts_with(|c: char| c.is_ascii_digit())) {
                    errs.push(format!("[harness.{}] env_allow entry `{}` must be an environment variable name (A-Z, 0-9, `_`); give names, never values", name, v));
                }
            }
            match h.quota_source.as_deref() {
                None | Some("off") => {}
                Some("gh") if name == "copilot" => {}
                Some(other) => errs.push(format!("[harness.{}] quota_source `{}` is not valid: only the copilot block takes it, as \"gh\" or \"off\"", name, other)),
            }
            // agy has no flag for an MCP config, a tool list or an approval bridge. Delphi gives it a plugin and a gate hook in the
            // span's own `.agents/` instead (`specs/020` §8), so from its block the engine reads `bridge`, `native_tools` (the
            // web tools the gate lets through; Claude names are translated) and `env_allow`. `approvals`, `claudeai_connectors`
            // and `connector_reads` mean nothing to it.
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
            if !matches!(family.as_str(), "claude" | "gemini" | "openai" | "local") {
                errs.push(format!("[access] role_family.{} names unknown family `{}` (use `claude`, `gemini`, `openai` or `local`)", role, family));
            } else if !self.endpoints.iter().any(|e| &e.family == family && allowed.contains(&e.id)) {
                errs.push(format!("[access] role_family.{} = `{}`, but no allowed endpoint serves that family", role, family));
            }
        }
        for (what, family) in self.access.primary.iter().map(|p| ("primary", p)).chain(self.access.failover.iter().map(|f| ("failover", f))) {
            if !matches!(family.as_str(), "claude" | "gemini" | "openai" | "local") {
                errs.push(format!("[access] {} names unknown family `{}` (use `claude`, `gemini`, `openai` or `local`)", what, family));
            } else if !self.endpoints.iter().any(|e| &e.family == family && allowed.contains(&e.id)) {
                errs.push(format!("[access] {} = `{}`, but no allowed endpoint serves that family", what, family));
            }
        }
        let t = &self.routing.tuning;
        if t.throttle_attempts == 0 || t.throttle_window_secs <= 0 || t.reprobe_first_secs <= 0 || t.reprobe_max_secs < t.reprobe_first_secs {
            errs.push("[routing.pool]: throttle_attempts, throttle_window_secs and reprobe_first_secs must be positive, and reprobe_max_secs at least reprobe_first_secs".into());
        }
        if let Some(order) = &self.routing.pool {
            let mut seen = std::collections::BTreeSet::new();
            for id in order {
                if !seen.insert(id.as_str()) {
                    errs.push(format!("[routing.pool] order lists `{}` twice", id));
                } else if self.endpoints_serving(id).is_empty() {
                    errs.push(format!("[routing.pool] order: `{}` is not served by any allowed endpoint (check [models.*], the endpoints' `models` and allowed_endpoints)", id));
                }
            }
        }
        for (role, id) in &self.routing.targets {
            if self.endpoints_serving(id).is_empty() {
                errs.push(format!("[routing.targets] {} = `{}` is not served by any allowed endpoint", role, id));
            } else if let Some(order) = &self.routing.pool {
                if !order.contains(id) {
                    errs.push(format!("[routing.targets] {} = `{}` is not in [routing.pool] order", role, id));
                }
            }
        }
        errs
    }

    /// Every allowed endpoint that serves `model_id` for some tier, grouped by family in policy order
    /// ([`Config::endpoints_for`]). Each is returned once, with the highest tier that names the model.
    pub fn endpoints_serving(&self, model_id: &str) -> Vec<PoolKey> {
        let mut families: Vec<&str> = Vec::new();
        let allowed = self.allowed();
        for e in &self.endpoints {
            if allowed.contains(&e.id) && !families.contains(&e.family.as_str()) {
                families.push(&e.family);
            }
        }
        let mut out = Vec::new();
        for fam in families {
            for ep in self.endpoints_for(fam) {
                if let Some(tier) = TIERS.iter().rev().find(|t| self.resolve_model(ep, t).as_deref() == Some(model_id)) {
                    out.push(PoolKey { endpoint_id: ep.id.clone(), model_id: model_id.to_string(), tier: tier.to_string(), family: fam.to_string() });
                }
            }
        }
        out
    }

    /// The pool in preference order. With `[routing.pool] order`, each model expands to its endpoints
    /// in policy order. Without it, every configured tier, family by family, highest tier first.
    pub fn pool_entries(&self) -> Vec<PoolKey> {
        let mut out: Vec<PoolKey> = Vec::new();
        let mut push = |k: PoolKey| {
            if !out.iter().any(|o| o.endpoint_id == k.endpoint_id && o.model_id == k.model_id) {
                out.push(k);
            }
        };
        if let Some(order) = &self.routing.pool {
            for id in order {
                for k in self.endpoints_serving(id) {
                    push(k);
                }
            }
            return out;
        }
        let allowed = self.allowed();
        let mut families: Vec<&str> = Vec::new();
        for e in &self.endpoints {
            if allowed.contains(&e.id) && !families.contains(&e.family.as_str()) {
                families.push(&e.family);
            }
        }
        for fam in families {
            for tier in TIERS.iter().rev() {
                for ep in self.endpoints_for(fam) {
                    if let Some(model_id) = self.resolve_model(ep, tier) {
                        push(PoolKey { endpoint_id: ep.id.clone(), model_id, tier: tier.to_string(), family: fam.to_string() });
                    }
                }
            }
        }
        out
    }

    /// The model family a role is served by: its `role_family`, else the primary.
    pub fn family_for_role(&self, role: &str) -> &str {
        self.access.role_family.get(role).map(String::as_str).unwrap_or_else(|| self.primary_family())
    }

    /// The family roles are served by unless told otherwise (`specs/035` D-1). Claude stays the default where it is configured; a
    /// workspace without it is not sent looking for it.
    pub fn primary_family(&self) -> &str {
        if let Some(p) = self.access.primary.as_deref() {
            return p;
        }
        let allowed = self.allowed();
        if self.endpoints.iter().any(|e| e.family == "claude" && allowed.contains(&e.id)) {
            return "claude";
        }
        self.endpoints.iter().find(|e| allowed.contains(&e.id)).map(|e| e.family.as_str()).unwrap_or("claude")
    }

    /// Where a family stands in the failover order (lower is tried first): its place in `[access] failover`, then the
    /// others in declaration order, `local` last (`specs/035` D-2).
    pub fn failover_rank(&self, family: &str) -> usize {
        if let Some(i) = self.access.failover.iter().position(|f| f == family) {
            return i;
        }
        if family == "local" {
            return usize::MAX;
        }
        self.access.failover.len() + self.endpoints.iter().position(|e| e.family == family).unwrap_or(usize::MAX / 2 - 1)
    }

    /// The lineage of a pool pair: the catalog's `lineage`, else the endpoint's family (`specs/035` D-10).
    pub fn lineage_of(&self, key: &PoolKey) -> String {
        self.catalog_entry(&key.model_id).and_then(|c| c.lineage.clone()).unwrap_or_else(|| key.family.clone())
    }

    /// Is this model reduced (`specs/035` D-3): marked so in the catalog, or served by a local endpoint?
    pub fn is_reduced(&self, endpoint_id: &str, model_id: &str) -> bool {
        match self.catalog_entry(model_id).and_then(|c| c.reduced) {
            Some(r) => r,
            None => self.endpoint(endpoint_id).is_some_and(|e| e.is_local()),
        }
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
                        Some(c) if !c.has_prices() && !matches!(e.cost_model.as_deref(), Some("subscription" | "local")) => {
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

    #[test]
    fn mcp_defaults_on_and_refuses_remote_access() {
        assert!(Config::parse("").unwrap().mcp.enabled);
        assert!(!Config::parse("[mcp]\nenabled = false\n").unwrap().mcp.enabled);
        assert!(Config::parse("[mcp]\nallow_remote = true\n").unwrap_err().iter().any(|e| e.contains("allow_remote")));
        assert!(Config::parse("[mcp]\nsurprise = 1\n").is_err());
        // There is no limit or quota on host calls: the switch is the only setting (`specs/062` §9).
        assert!(Config::parse("[mcp]\nmax_help_per_session = 5\n").is_err() && Config::parse("[mcp]\ncooldown_seconds = 5\n").is_err());
        assert!(McpConfig::read("[mcp]\nenabled = 3\n").is_none() && McpConfig::read("").is_some());
    }

    #[test]
    fn mcp_switch_reads_fail_closed() {
        assert!(McpConfig::enabled_in(""));
        assert!(!McpConfig::enabled_in("[mcp]\nenabled = false\n"));
        assert!(!McpConfig::enabled_in("[mcp]\nenabled = \"no\"\n"));
        assert!(!McpConfig::enabled_in("[mcp]\nallow_remote = true\n"));
        assert!(!McpConfig::enabled_in("[[["));
    }

    #[test]
    fn library_pages_defaults_and_ttl_forms() {
        let d = Config::parse(SAMPLE).unwrap().library.pages;
        assert!(d.enabled && d.external == PagesExternal::Ask);
        assert_eq!((d.max_tokens, d.external_ttl_secs, d.per_day), (3000, 7 * 86_400, 20));
        for (v, secs) in [("3", 3 * 86_400), ("\"12h\"", 12 * 3600), ("\"90m\"", 5400), ("\"7d\"", 7 * 86_400)] {
            let c = Config::parse(&format!("{}\n[library.pages]\nexternal = \"off\"\nexternal_ttl = {}\n", SAMPLE, v)).unwrap().library.pages;
            assert_eq!((c.external, c.external_ttl_secs), (PagesExternal::Off, secs), "{v}");
        }
        assert!(Config::parse(&format!("{}\n[library.pages]\nexternal = \"maybe\"\n", SAMPLE)).is_err());
        assert!(Config::parse(&format!("{}\n[library.pages]\nexternal_ttl = \"soon\"\n", SAMPLE)).is_err());
    }

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
    fn validator_defaults_to_advisory_and_modes_order_by_strictness() {
        assert_eq!(Config::parse("").map(|c| c.validator.mode).unwrap_or(ValidatorMode::Off), ValidatorMode::Advisory);
        let c = Config::parse("[validator]\nmode = \"ratify\"\n[validator.sources]\nprimary = [\"example.org\"]\n").unwrap();
        assert_eq!(c.validator.mode.min(ValidatorMode::Advisory), ValidatorMode::Advisory);
        assert!(Config::parse("[validator]\nmode = \"yes\"\n").is_err());
    }

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

    fn with_routing(extra: &str) -> Result<Config, Vec<String>> {
        Config::parse(&format!("{}\n{}", SAMPLE, extra))
    }

    fn slots(c: &Config) -> Vec<String> {
        c.pool_entries().iter().map(|k| format!("{}/{}", k.endpoint_id, k.model_id)).collect()
    }

    #[test]
    fn only_a_harness_without_an_approval_bridge_is_read_only_and_context_is_optional() {
        let text = format!(
            "{}\n[[endpoints]]\nid = \"c\"\nfamily = \"claude\"\npath = \"harness\"\nharness = \"claude\"\ncredential = \"harness-login\"\n[[endpoints]]\nid = \"a\"\nfamily = \"gemini\"\npath = \"harness\"\nharness = \"agy\"\ncredential = \"harness-login\"\ncost_model = \"subscription\"\n[models.catalog.\"big\"]\nmax_output_tokens = 1\ncontext_tokens = 1000000\n",
            SAMPLE
        );
        let c = Config::parse(&text).unwrap();
        let can = |id: &str| c.endpoint(id).unwrap().can_mutate();
        assert_eq!((can("direct"), can("bedrock-us"), can("corp-gateway"), can("c"), can("a")), (true, true, true, true, false));
        assert!(c.endpoint("a").unwrap().is_metered_by_subscription() && c.endpoint("c").unwrap().is_metered_by_subscription() && !c.endpoint("direct").unwrap().is_metered_by_subscription());
        assert_eq!((c.catalog_entry("big").unwrap().context_tokens, c.catalog_entry("sonnet-x").unwrap().context_tokens), (1_000_000, 0), "unset means unknown");
    }

    #[test]
    fn harness_profiles_default_to_full_capability_and_a_partial_block_keeps_the_other_defaults() {
        assert_eq!(Config::parse(SAMPLE).unwrap().harness.len(), 0, "no block is the normal case: the defaults apply in the engine, nothing to ratify");
        let d = HarnessProfile::default();
        assert_eq!((d.bridge, d.approvals, d.claudeai_connectors, d.native_tools.as_slice(), d.connector_reads.len()), (true, ApprovalSource::Delegate, true, &["WebSearch".to_string(), "WebFetch".to_string()][..], 0), "the operator's 2026-10-25 decision: full capability out of the box");
        // A block that names one thing changes only that thing.
        // `[harness]` has scalar settings of its own, which must not be mistaken for a profile.
        let mixed = Config::parse(&format!("{}\n[harness]\nsubscription_budget_tokens = 1000\n[harness.claude]\nbridge = false\n", SAMPLE)).unwrap();
        assert_eq!(mixed.harness.keys().collect::<Vec<_>>(), ["claude"]);
        let narrow = Config::parse(&format!("{}\n[harness.claude]\napprovals = \"delphi\"\n", SAMPLE)).unwrap();
        assert_eq!(narrow.harness["claude"], HarnessProfile { approvals: ApprovalSource::Delphi, ..Default::default() });
        let off = Config::parse(&format!("{}\n[harness.claude]\nbridge = false\nclaudeai_connectors = false\nnative_tools = []\n", SAMPLE)).unwrap();
        assert_eq!((off.harness["claude"].bridge, off.harness["claude"].claudeai_connectors, off.harness["claude"].native_tools.len(), off.harness["claude"].approvals), (false, false, 0, ApprovalSource::Delegate));
        let ok = format!("{}\n[harness.claude]\nconnector_reads = [\"mcp__claude_ai_Slack__slack_search\"]\nenv_allow = [\"AWS_PROFILE\", \"CLAUDE_CODE_USE_BEDROCK\"]\nprofile = \"default\"\n[harness.agy]\nenv_allow = [\"HTTPS_PROXY\"]\n", SAMPLE);
        let c = Config::parse(&ok).unwrap();
        let h = &c.harness["claude"];
        assert_eq!((h.connector_reads.len(), h.env_allow.len()), (1, 2));
        assert_eq!(c.harness["agy"], HarnessProfile { env_allow: vec!["HTTPS_PROXY".into()], ..Default::default() });
        assert_eq!(Config::parse(&format!("{}\n[harness.copilot]\nquota_source = \"gh\"\n", SAMPLE)).unwrap().harness["copilot"].quota_source.as_deref(), Some("gh"));
        for (bad, needle) in [
            ("[harness.claude]\nenv_allow = [\"aws_profile\"]\n", "environment variable name"),
            ("[harness.claude]\nenv_allow = [\"TOKEN=abc\"]\n", "give names, never values"),
            ("[harness.claude]\nnative_tools = [\"Web Search\"]\n", "must be a tool name"),
            ("[harness.claude]\nconnector_reads = [\"slack_search\"]\n", "exact tool name"),
            ("[harness.claude]\nsurprise = 1\n", "unknown field"),
            ("[harness.gemini]\n", "only `claude`, `agy`, `codex` and `copilot`"),
            ("[harness.codex]\nquota_source = \"gh\"\n", "only the copilot block takes it"),
            ("[harness.copilot]\nquota_source = \"yes\"\n", "only the copilot block takes it"),
        ] {
            let errs = Config::parse(&format!("{}\n{}", SAMPLE, bad)).unwrap_err().join("; ");
            assert!(errs.contains(needle), "{} -> {}", bad, errs);
        }
    }

    #[test]
    fn without_a_pool_every_configured_tier_is_used_family_by_family_highest_tier_first() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.routing, Routing::default());
        assert_eq!(slots(&c), vec!["corp-gateway/sonnet-x", "bedrock-us/bedrock-sonnet-x", "corp-gateway/haiku-x", "bedrock-us/haiku-x"]);
        assert_eq!(c.routing.tuning, PoolTuning { throttle_attempts: 3, throttle_window_secs: 600, reprobe_first_secs: 900, reprobe_max_secs: 14_400 });
    }

    #[test]
    fn grades_are_per_role_or_kind_and_bounded() {
        let c = with_routing("[grades]\narchitect = 1\n[grades.builder]\ndefault = -1\ndocs = -2").unwrap();
        assert_eq!((c.grades.of("architect", None), c.grades.of("builder", None), c.grades.of("builder", Some("docs")), c.grades.of("builder", Some("code")), c.grades.of("writer", None)), (1, -1, -2, -1, 0));
        assert!(with_routing("[grades]\narchitect = 3").is_err());
        assert!(with_routing("[grades]\narchitect = \"high\"").is_err());
    }

    #[test]
    fn the_agent_registry_is_flat_unless_asked_for() {
        assert_eq!(with_routing("").unwrap().routing.registry, RegistryMode::Flat);
        assert_eq!(with_routing("[routing]\nregistry = \"auto\"").unwrap().routing.registry, RegistryMode::Auto);
        assert!(with_routing("[routing]\nregistry = \"on\"").is_err());
    }

    #[test]
    fn assignment_defaults_to_static_and_its_keys_are_checked() {
        let c = with_routing("").unwrap();
        assert_eq!((c.routing.assignment, c.routing.evidence_enough), (Assignment::Static, 8));
        let c = with_routing("[routing]\nassignment = \"discovered\"\nevidence_enough = 3\n[routing.reserve]\nvalidator = 0.2").unwrap();
        assert_eq!((c.routing.assignment, c.routing.evidence_enough, c.routing.reserve["validator"]), (Assignment::Discovered, 3, 0.2));
        for bad in ["[routing]\nassignment = \"auto\"", "[routing]\nevidence_enough = 0", "[routing.reserve]\nvalidator = 1.5", "[routing.reserve]\nvalidator = 0.6\nbuilder = 0.6"] {
            assert!(with_routing(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_pool_order_expands_each_model_to_the_allowed_endpoints_that_serve_it_in_policy_order() {
        let c = with_routing("[routing.pool]\norder = [\"haiku-x\", \"sonnet-x\"]\nthrottle_attempts = 5\n[routing.targets]\nbuilder = \"sonnet-x\"").unwrap();
        // `direct` serves both models but is outside allowed_endpoints; bedrock-us names a different workhorse model.
        assert_eq!(slots(&c), vec!["corp-gateway/haiku-x", "bedrock-us/haiku-x", "corp-gateway/sonnet-x"]);
        let first = &c.pool_entries()[0];
        assert_eq!((first.tier.as_str(), first.family.as_str()), ("fast", "claude"));
        assert_eq!((c.routing.tuning.throttle_attempts, c.routing.tuning.throttle_window_secs), (5, 600), "unset keys keep their defaults");
        assert_eq!(c.routing.targets.get("builder").map(String::as_str), Some("sonnet-x"));
    }

    #[test]
    fn a_bad_pool_or_target_is_reported_in_full() {
        let errs = with_routing("[routing.pool]\norder = [\"sonnet-x\", \"sonnet-x\", \"nope-x\"]\nreprobe_first_secs = 100\nreprobe_max_secs = 10\n[routing.targets]\nbuilder = \"haiku-x\"\nred-team = \"ghost-x\"").unwrap_err().join("\n");
        for want in ["lists `sonnet-x` twice", "`nope-x` is not served by any allowed endpoint", "reprobe_max_secs at least reprobe_first_secs", "builder = `haiku-x` is not in [routing.pool] order", "red-team = `ghost-x` is not served"] {
            assert!(errs.contains(want), "missing `{}` in:\n{}", want, errs);
        }
        assert!(with_routing("[routing.pool]\norder = \"sonnet-x\"").is_err(), "order must be a list");
    }

    #[test]
    fn template_placeholders_and_unrelated_routing_keys_are_ignored() {
        let c = with_routing("[routing]\nenable_dynamic_scaling = true\nmax_parallel_agents = 4\n[routing.pool]\norder = [\"<claude frontier model id>\"]\n[routing.targets]\nred-team = \"<model id>\"").unwrap();
        assert_eq!((c.routing.pool.clone(), c.routing.targets.clone()), (None, BTreeMap::new()));
        assert_eq!(slots(&c).len(), 4, "an untouched template behaves as no pool");
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
    fn credential_none_is_only_for_a_loopback_compat_endpoint() {
        let ep = |path: &str, url: &str| format!("[[endpoints]]\nid=\"o\"\nfamily=\"local\"\npath=\"{}\"\nbase_url=\"{}\"\ncredential=\"none\"\n", path, url);
        assert!(Config::parse(&ep("gateway-compat", "http://localhost:11434")).is_ok());
        assert!(Config::parse(&ep("gateway-compat", "http://[::1]:11434")).is_ok());
        for bad in [ep("gateway-compat", "https://gw.example.internal"), ep("gateway-native", "http://127.0.0.1:1"), ep("api", "http://127.0.0.1:1")] {
            let errs = Config::parse(&bad).unwrap_err().join("\n");
            assert!(errs.contains("only for a local server"), "{}", errs);
        }
    }

    #[test]
    fn a_local_family_is_declared_last_so_it_is_the_last_resort() {
        let cfg = "[[endpoints]]\nid=\"c\"\nfamily=\"claude\"\npath=\"harness\"\nharness=\"claude\"\ncredential=\"harness-login\"\ncost_model=\"subscription\"\n[endpoints.models]\nworkhorse=\"sonnet\"\n[[endpoints]]\nid=\"ollama\"\nfamily=\"local\"\npath=\"gateway-compat\"\nbase_url=\"http://localhost:11434\"\ncredential=\"none\"\n[endpoints.models]\nworkhorse=\"qwen3:8b\"\n[access]\nrole_family = { scribe = \"local\" }\n";
        let c = Config::parse(cfg).unwrap();
        let ids: Vec<_> = c.pool_entries().iter().map(|k| k.endpoint_id.clone()).collect();
        assert_eq!(ids, ["c", "ollama"]);
        assert_eq!(c.family_for_role("scribe"), "local");
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

    #[test]
    fn library_acquisition_is_off_by_default_and_its_keys_are_checked() {
        let off = Config::parse(SAMPLE).unwrap().library.acquire;
        assert!(!off.enabled && off.token_share_percent == 5 && off.interests.is_empty());
        let on = Config::parse(&format!("{}\n[library.acquire]\nenabled = true\ninterests = [\"siege engines\"]\nbatch_size = 4\n", SAMPLE)).unwrap().library.acquire;
        assert!(on.enabled && on.batch_size == 4 && on.interests == ["siege engines"]);
        for (bad, why) in [("batch_size = 0", "batch_size"), ("token_share_percent = 11", "token_share_percent"), ("interests = [\"\"]", "interests"), ("byte_budget_mib = 0", "byte_budget_mib"), ("licence = \"cc0\"", "licence")] {
            let errs = Config::parse(&format!("{}\n[library.acquire]\n{}\n", SAMPLE, bad)).unwrap_err().join("\n");
            assert!(errs.contains(why), "{}: {}", bad, errs);
        }
    }
}
