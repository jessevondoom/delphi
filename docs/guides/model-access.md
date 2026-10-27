# Model access

Roles ask for a capability and a tier (`fast`, `workhorse`, `frontier`), never for a named model. Which models answer is decided by your **endpoints**, in the order you set, with automatic failover when one runs out.

## The quick way

```sh
delphi setup            # finds claude and agy, asks only for what it cannot find, ratifies the config
delphi setup --check    # report only, change nothing
delphi endpoints test   # does each endpoint answer?
delphi models           # which models are usable, out of quota, or cooling down
```

`delphi setup --yes` accepts the defaults. The first `delphi` in a new workspace runs the same setup when there is no model access.

## The five paths

Each `[[endpoints]]` entry in `config.toml` has a `path`:

| `path` | What it reaches | Credential |
| :-- | :-- | :-- |
| `api` | the Anthropic or Gemini API directly | macOS Keychain or Linux Secret Service, an `*_env` variable, or a command |
| `cloud` | AWS Bedrock or Google Vertex | `aws:<profile>`, `gcloud-adc` |
| `gateway-native`, `gateway-compat` | a corporate gateway or any OpenAI-compatible endpoint | OAuth2 device code or PKCE, or none for a loopback address |
| `harness` | your own `claude` (Claude Code) or `agy` (Antigravity) program, as a subscription seat | the program's own login; Delphi never reads it |

API keys never go in `config.toml`. The `credential` field names where the key is kept.

```toml
[[endpoints]]
id = "bedrock-us"
family = "claude"
path = "cloud"
platform = "bedrock"
region = "us-east-1"
credential = "aws:work-sso"

[access]
order = { claude = ["bedrock-us"] }
allowed_endpoints = ["bedrock-us"]
```

`[[endpoints]]`, `[access]`, `[auth.*]`, `[connectors.*]`, `[harness.*]` and `[exec]` are **ratified**: Delphi will not act on an edited copy until you approve the change in the console with `/ratify` (it is a Red-tier proposal). The schema is [`schemas/config.schema.json`](../../schemas/config.schema.json); every key is in the [configuration reference](../reference/config.md).

## Signing in

```sh
delphi auth login <name>         # OAuth flows you declared under [auth.<name>]; --manual for a copy-paste flow
delphi auth status
delphi auth logout <name>
```

Subscription seats (`claude`, `agy`) sign in through their own programs.

## Subscription seats

A harness seat gives Delphi's tools (timers, dispatch, scoped file access, connectors) to the program over a per-span MCP bridge. By default the Claude seat delegates approvals to the CLI and your organization's policy, and uses the account's own connectors and web tools. To narrow that, add a `[harness.claude]` block (`bridge`, `approvals`, `native_tools`, `claudeai_connectors`, `connector_reads`, `env_allow`); the template in `config.example.toml` explains each. The `agy` seat starts untrusted and may act as a Builder only after:

```sh
delphi endpoints test --probe
```

has watched its gate deny every kind of tool on the installed version. `delphi doctor` and the probe also tell you when your organization's MCP policy stops the bridge from loading.

## When a model runs out

The pool keeps your models in preference order. When one is out of quota or cooling down the next takes over and work continues. When none is usable a task waits, and says so, instead of failing; a weaker model substitutes only after you confirm it for that task. To set the order explicitly:

```toml
[routing.pool]
order = ["<claude frontier model id>", "<gemini frontier model id>", "<claude workhorse model id>"]
```

`delphi models --probe` re-checks every model now.

## A local fallback

A slow, free, private fallback for when paid quota is gone: declare an Ollama endpoint after your paid ones.

```toml
[[endpoints]]
id = "ollama"
family = "local"
path = "gateway-compat"
base_url = "http://localhost:11434"
credential = "none"        # accepted only for a loopback base_url
cost_model = "local"
models = { fast = "qwen3:8b", workhorse = "qwen3:8b", frontier = "qwen3:8b" }

[models.catalog."qwen3:8b"]
max_output_tokens = 8192
```

Small local models are noticeably weaker; the README's [Status and limitations](../../README.md#status-and-limitations) says what has been proven.

## Spending

Each model call reserves its cost before it runs against `[budget] mission_budget_usd` (default 5.0) and `council_budget_usd` (default 20.0). Per-model prices live in `[models.catalog."<id>"]`; subscription seats are metered by the subscription, not in dollars. `/usage` shows spend by mission, role, task or model, and `/budget <dollars>` changes the mission ceiling.
