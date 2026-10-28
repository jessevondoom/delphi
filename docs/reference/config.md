# Configuration reference

A workspace's settings are one TOML file, `$DELPHI_HOME/workspaces/<name>/config.toml`, seeded by `delphi init` from [`crates/delphi/config.example.toml`](../../crates/delphi/config.example.toml) (which is annotated line by line and is the better place to read examples). The machine-readable form is [`schemas/config.schema.json`](../../schemas/config.schema.json); it checks shape and allowed values, while `delphi doctor` is authoritative.

**Ratified** sections (marked ◆) pin what Delphi may reach or run. An edit to them is a pending Red-tier proposal and changes nothing until you approve it with `/ratify`. **API keys never appear in this file**: `credential` fields name a place (the macOS Keychain or Linux Secret Service, an environment variable, `aws:<profile>`, `gcloud-adc`, or a command).

Defaults are those of the template; "—" means no default.

## Workspace and display

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[workspace] name` | — | the workspace's name |
| `[workspace] projects` | — | project repositories this workspace operates on; `~/` expands. Delphi never writes into them except through reviewed builds |
| `[workspace] untrusted_paths` | `vendor/**`, `third_party/**`, `node_modules/**` | tracked files here are read as `external_untrusted` |
| `[workspace] isolated` | `false` | `true`: nothing is ever promoted out of this workspace |
| `[privacy] identifiers` | `[]` | repo names, hosts and terms that must never appear in operator-scope rules |
| `[privacy] default_mode` | `orchestrator` | `orchestrator` or `council` |
| `[theme] name` | `greek` | display names, window labels and voice only; behavior never depends on it |
| `[tui] auto_spawn_narrator` | `false` | open the second window on launch |
| `[tui] frame_rate_hz` | `60` | |
| `[agents] enabled` | orchestrator, architect, builder, red-team, strategist | the roster; nine roles exist |
| `[events] thinking_retention_days` | `30` | how long thinking is kept |

## Models, routing and budget

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[models.<family>] fast`, `workhorse`, `frontier` | — | tier to model ID, per family (`claude`, `gemini`, `local`, …) |
| `[models.catalog."<id>"]` | — | per-model `max_output_tokens`, `min_thinking_budget`, `max_thinking_budget`, `context_tokens`, `thinking_in_max_tokens`, and prices `input_usd_per_mtok`, `cache_read_usd_per_mtok`, `cache_write_usd_per_mtok`, `output_usd_per_mtok`. Prices drive budget reservation |
| `[routing] enable_dynamic_scaling` | `true` | |
| `[routing] cross_provider_builder_critic` | `true` | pair a Builder on one family with a critic on another |
| `[routing] max_attempts_per_node` | `3` | retry ladder: upscale, reset, escalate |
| `[routing] max_parallel_agents` | `4` | |
| `[routing] degrade_prompt_secs` | `300` | how long the "run on a weaker model?" card waits |
| `[routing] max_park_secs` | wait forever | how long a task waits for a model; `0` escalates at once |
| `[routing.pool] order` | every configured tier, family by family | models in preference order |
| `[routing.pool] throttle_attempts`, `throttle_window_secs` | `3`, `600` | this many throttles in a row over at least this long count as a spent quota |
| `[routing.pool] reprobe_first_secs`, `reprobe_max_secs` | `900`, `14400` | when an exhausted model with no known reset time is probed again, doubling up to the maximum |
| `[routing.targets] <role>` | — | a target model per role; a similar model is substituted if it is unavailable, a weaker one only with your confirmation |
| `[budget] mission_budget_usd` | `5.0` | ceiling for L1-L3 work, enforced by reserving cost before each call |
| `[budget] council_budget_usd` | `20.0` | ceiling for a Council |
| `[budget] on_budget_exhausted` | `escalate` | halt the node and ask you; never silently downscale |
| `[budget] per_watch_max_wakes_per_day` | `10` | |
| `[hitl] approval_timeout_sec` | `600` | an unanswered card is denied after this long |

## Model access ◆

| Section | Meaning |
| :-- | :-- |
| `[[endpoints]]` | one per way of reaching a model: `id`, `family`, `path` (`api`, `cloud`, `gateway-native`, `gateway-compat`, `harness`), `credential`, and as the path needs `platform`, `region`, `project`, `base_url`, `models`, `headers`, `cost_model` (`subscription` or `local`), `harness` (`claude` or `agy`), `betas`. `credential = "none"` is accepted only for a loopback `base_url` |
| `[access]` | `order` (family to endpoint IDs, in preference order) and `allowed_endpoints` |
| `[auth.<name>]` | an OAuth flow: `flow` (`device_code`, `client_credentials`, `loopback_pkce`), `client_id`, `issuer` or explicit URLs, `scopes`, `audience`, `client_secret` (a reference such as `keychain:…`), `redirect_port`, `authorization_params` |
| `[harness.claude]`, `[harness.agy]` | narrow a subscription CLI: `bridge`, `approvals` (`delegate` or `delphi`), `native_tools`, `claudeai_connectors`, `connector_reads`, `env_allow`, and CLI features such as `effort`, `max_turns`, `max_budget_usd`, `fallback_models` (see [`specs/021`](../../specs/021-harness-cli-features.md)). Unknown keys are refused |
| `[harness] connector_settle_secs` | seconds a span waits for the account's connectors to load before its first turn |
| `[connectors.<name>]` | an MCP connector: `url`, `hosts`, `login`, `account`, `roles`, `tools` (each `read`, `draft` or `write`), `max_arg_chars`, `first_use` (`ask` or `allow`), `calls_per_minute`, `max_result_chars`, `identity` |

## Commands and updates

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[exec] sandbox` ◆ | `off` | `off`, `prefer` or `required` |
| `[exec] allow`, `deny` ◆ | — | command patterns over the argument list; `*` matches anything. `allow` needs no approval, `deny` is refused outright |
| `[exec] test_command`, `check_command` ◆ | — | the two merge gates, as argument lists |
| `[update] require_signed` ◆ | `false` | refuse any build that is not a signed release |

## Away mode and hooks ◆

| Section | Meaning |
| :-- | :-- |
| `[relay]` | `channel` (`email` or `telegram`), `detail` (`summary` or `full`), `step_up = "totp"` with `totp_credential`, `require_auth`, `notify_tasks`, `max_outstanding`, `max_per_hour`, `poll_secs`; with `[relay.email]` (`mailbox`, `operator`, `also_allow`, `imap_host`, `smtp_host`, `imap_port`, `smtp_port`, `username`, `tls`, `authserv_id`, `credential`) or `[relay.telegram]` (`chat_id`, `also_allow`, `api_base`, `credential`). See [Away mode](../guides/away-mode.md) |
| `[[hooks.observe]]`, `[[hooks.deny]]` | your commands on events, or as vetoes: `name`, `events` or `tools`, `command` (an argument list), `timeout_secs`. They can only watch or refuse ([`specs/018`](../../specs/018-operator-hooks.md)) |
| `[away] approval_park_max_sec` | the longest a parked approval waits |

## Sessions, learning, optimistic builds

| Key | Meaning |
| :-- | :-- |
| `[session] summary` | `off`, `local` or `fast`: how a finished console session is summarized for `delphi sessions` ([`specs/030`](../../specs/030-console-sessions-and-conversation-context.md)) |
| `[session] conversation_tokens` | how much of your session a task is told; `0` tells nothing |
| `[learning] capture`, `max_notes`, `triggers` | whether notes are staged from denials, corrections, retries and escalations, how many, and which triggers. Staged notes are never loaded into a prompt ([`specs/031`](../../specs/031-layered-learning-and-training-mode.md)) |
| `[loop] nudge`, `nudge_edits`, `budget_warnings`, `verify_gate`, `workspace_bootstrap` | what the engine adds around a role's model calls: a nudge when it goes round in circles (and a stop if it carries on), a warning at 75 % and 90 % of its turns, one check against the task before a role that writes or runs commands may finish, and a map of its worktree. All on by default; `nudge_edits` is 4 ([`specs/045`](../../specs/045-harness-loop-techniques.md)) |
| `[optimistic] approver`, `second_seat`, `max_decisions`, `max_approver_calls`, `unseconded`, `review_wait_sec` | how `/build --optimistic` records and reviews provisional decisions ([`specs/015`](../../specs/015-provisional-decisions.md)) |

## Search and federation

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[rag] engine` | `sqlite-hybrid` | informational: keywords, vectors and a reranker. What runs depends on the models installed |
| `[rag] max_pointer_results` | `10` | |
| `[rag] exclude_untrusted_by_default` | `true` | |
| `[federation] enabled` | `false` | deferred ([`specs/007`](../../specs/007-federated-mesh-and-sync.md)) |
| `[federation] transport` | `tailscale` | `tailscale`, `ssh-uds` or `mdns` |
| `[federation] default_peer_trust_ceiling` | `federated_peer` | |

## Error log (operator scope)

Not in a workspace `config.toml`: these keys live in `$DELPHI_HOME/operator/config.toml`, which no agent can read or change. See [Debugging with the error log](../guides/debugging.md).

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[logging] level` | `error` | `off`, `error`, `warn` or `debug`; `DELPHI_LOG` overrides it |
| `[logging] max_mb` | `10` | size of each log file |
| `[logging] files` | `3` | the log and its rotated copies |

## Environment variables

`DELPHI_HOME`, `DELPHI_WORKSPACE`, `DELPHI_LOG` (`off`, `error`, `warn`, `debug`), `DELPHI_UI`, `DELPHI_THEME`, `DELPHI_NO_ONBOARD`, `DELPHI_NO_MOUSE`, `DELPHI_REDUCE_MOTION`, `NO_COLOR`, `DELPHI_EMBED_MODEL` / `DELPHI_RERANK_MODEL` / `DELPHI_SEED_VECTORS` (a file path, or `off`), `DELPHI_SKIP_ATTESTATION` (installer), `DELPHI_REQUIRE_SIGNED` and `DELPHI_NO_AUTO_UPDATE` (see [`specs/036`](../../specs/036-signed-releases-and-start-time-refresh.md)). `delphi --help` lists the ones that matter at the command line.
