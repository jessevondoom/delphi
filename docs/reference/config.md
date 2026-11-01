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
| `[routing] registry` | `"flat"` | `"flat"`: the orchestrator sees one line per bundle agent, up to 20, with a note of how many are not listed. `"auto"`: past 20 agents it sees a domain index and gets `search_agent_registry` (spec 041); becomes the default only after the labeled set meets the recall bar |
| `[routing] assignment` | `"static"` | `"static"`: a role's tier picks the model. `"discovered"`: a model is assigned by capability class, measured results and quota (spec 056); becomes the default only after a benchmark shows it no worse |
| `[routing] evidence_enough` | `8` | measured results per `(role, kind, model)` after which a model is scored by them alone |
| `[routing.reserve] <role>` | none | the share (0 to 1) of the mission budget kept for that role, so planning cannot starve the validator; applies with `assignment = "discovered"` |
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
| `[[endpoints]]` | one per way of reaching a model: `id`, `family`, `path` (`api`, `cloud`, `gateway-native`, `gateway-compat`, `harness`), `credential`, and as the path needs `platform`, `region`, `project`, `base_url`, `models`, `headers`, `cost_model` (`subscription` or `local`), `harness` (`claude`, `agy`, `codex` or `copilot`), `betas`. `credential = "none"` is accepted only for a loopback `base_url` |
| `[access]` | `order` (family to endpoint IDs, in preference order) and `allowed_endpoints` |
| `[auth.<name>]` | an OAuth flow: `flow` (`device_code`, `client_credentials`, `loopback_pkce`), `client_id`, `issuer` or explicit URLs, `scopes`, `audience`, `client_secret` (a reference such as `keychain:…`), `redirect_port`, `authorization_params` |
| `[harness.claude]`, `[harness.agy]`, `[harness.codex]`, `[harness.copilot]` | narrow a subscription CLI: `bridge`, `approvals` (`delegate` or `delphi`), `native_tools`, `claudeai_connectors`, `connector_reads`, `env_allow`, `quota_source` (copilot only: `"gh"` lets `delphi doctor` read the account's quota through your `gh` login; off by default), and CLI features such as `effort`, `max_turns`, `max_budget_usd`, `fallback_models` (see [`specs/021`](../../specs/021-harness-cli-features.md)). Unknown keys are refused |
| `[harness] connector_settle_secs` | seconds a span waits for the account's connectors to load before its first turn |
| `[connectors.<name>]` | an MCP connector: `url`, `hosts`, `login`, `account`, `roles`, `tools` (each `read`, `draft` or `write`), `max_arg_chars`, `first_use` (`ask` or `allow`), `calls_per_minute`, `max_result_chars`, `identity` |

## Promote and nerf ◆

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[grades] <role>` ◆ | `0` | a whole number from -2 to 2 that moves the class of model a role is assigned (spec 056 §4): positive promotes, negative nerfs, never past the top or bottom the pool has, never below the role's floor. `delphi agent promote\|nerf <role>` writes it as a pending proposal; `/ratify` applies it. Applies with `[routing] assignment = "discovered"` |
| `[grades.<role>] <kind>`, `default` ◆ | — | the same per task kind; `default` is the role's own grade. Stored and shown now; the engine applies the role's grade (a task's kind is not yet known at dispatch) |

## Commands and updates

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[exec] sandbox` ◆ | `off` | `off`, `prefer` or `required` |
| `[exec] allow`, `deny` ◆ | — | command patterns over the argument list; `*` matches anything. `allow` needs no approval, `deny` is refused outright |
| `[exec] test_command`, `check_command` ◆ | — | the two merge gates, as argument lists |
| `[update] require_signed` ◆ | `false` | refuse any build that is not a signed release |

## Research (the archives pane's expand mode) ◆

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[search] providers` ◆ | `["parallel"]` | the web search engines to ask, in order, one at a time: the next only if the last failed. `parallel` (Parallel's hosted MCP server: no key, no account, rate limited; it sees the query text), `brave`, `mojeek`, `searxng`, `kagi`, `google`. `google` needs a Gemini endpoint. `[]`: research cannot run ([`specs/028`](../../specs/028-knowledge-window.md) §12) |
| `[search.<engine>] credential` ◆ | — | where the key comes from, as for an endpoint (`env:BRAVE_API_KEY`, `keychain:…`, `command:…`); every engine but `parallel` and `searxng` needs one |
| `[search.searxng] url` ◆ | — | the SearXNG instance (https, or http to loopback) |
| `[search.google] cx` ◆ | — | the Programmable Search engine id |

Which sites the job may read is not here: it is `context/sources/trusted.toml` (ratified in a release) plus your own `$DELPHI_HOME/operator/trusted-sources.toml`.

## Fact validator ◆

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[validator] mode` ◆ | `advisory` | `off`: the validator fetches nothing, anywhere. `advisory`: the full fact path runs as a dry run (**it does use the network**: it fetches, snapshots, judges and records what it would ratify) and writes nothing validated. `ratify`: real writes to `context/facts/**`, honoured only while the daemon's latch allows it and the PROMOTE conditions hold. The effective mode is the lower of this value and the latch |
| `[validator] weak_sources` ◆ | `label` | a claim supported only by Weak sources: `label` keeps it labelled `unverified (weak)`, searchable and never quoted as fact; `off` refuses it. Weak sources never count toward the two that validate a claim, whatever this is set to ([`specs/040`](../../specs/040-validator-role-and-fact-ratification.md) §27) |
| `[validator.sources] primary`, `reputable`, `weak` ◆ | — | registrable domains the operator adds to the default source list (`specs/040` appendix A); they win over the defaults. Only the operator can edit them |

## Library acquisition ◆

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[library.acquire] enabled` ◆ | `false` | turns on the daily acquisition job (`specs/055` §8). It fetches from the archives in `context/sources/archives.toml` plus your `operator/archives.toml`, and only from those hosts. Proposals arrive as one batch in the digest; nothing is fetched until you accept or a standing rule (`delphi sources acquire rule add`) covers it |
| `[library.acquire] byte_budget_mib` ◆ | `2048` | what acquisition may hold; reaching it stops acquiring and evicts nothing |
| `[library.acquire] max_item_mib` ◆ | `16` | the largest text one acquired source may have; also the ceiling of a standing rule |
| `[library.acquire] batch_size` ◆ | `8` | sources in one batch (1 to 20) |
| `[library.acquire] token_share_percent` ◆ | `5` | the scout's share of the install's tokens over the trailing week (at most 10; `specs/040` §20.2). The scout only suggests topics; `0` keeps the job model-free |
| `[library.acquire] interests` ◆ | `[]` | topics you want, beside your projects' names (at most 50) |

## Generated pages

| Key | Default | Meaning |
| :-- | :-- | :-- |
| `[library.pages] enabled` | `true` | `false`: no page is written (`delphi pages search` still lists what the library holds) |
| `[library.pages] external` | `ask` | `ask`: the web is searched only when you ask (`--external`, or the console's key); `off`: never; `on`: every page also searches the web unless `--internal`. Queries go only to the engines in the ratified `[search]` table; `private` sources are never put on an external page or in its query ([`specs/057`](../../specs/057-search-pages-generated-and-checked.md)) |
| `[library.pages] max_tokens` | `3000` | the longest page the writer may write |
| `[library.pages] external_ttl` | `7d` | a page that used the web is stale after this (days as a number, or `12h`, `7d`) |
| `[library.pages] per_day` | `20` | pages built in one day (a cache hit costs nothing). The validator tokens a page uses also count against the validator's share ([`specs/040`](../../specs/040-validator-role-and-fact-ratification.md) §20) |
| `[library.pages] min_ttl_days` | `1` | the shortest cache lifetime, in days, the daemon accepts for a page ([`specs/061`](../../specs/061-encyclopedia-search-pages.md)) |
| `[library.pages] max_ttl_days` | `365` | the longest; the `writer` proposes a lifetime from how fast the topic changes and the daemon keeps it within these bounds |

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
| `[learning] capture`, `max_notes`, `triggers`, `retro_min_tasks` | whether notes are staged from denials, corrections, retries and escalations (default on), how many per mission (10 to 1000, default 200), which triggers, and how many finished tasks `delphi needs` waits for before calling the coaching pass worth building (3 to 200, default 10). A value outside its bounds is clamped and reported at boot. No key loads staged text into a prompt; turning capture off is allowed, loading unratified content is not ([`specs/031`](../../specs/031-layered-learning-and-training-mode.md) §3.6) |
| `[loop] nudge`, `nudge_edits`, `budget_warnings`, `verify_gate`, `workspace_bootstrap` | what the engine adds around a role's model calls: a nudge when it goes round in circles (and a stop if it carries on), a warning at 75 % and 90 % of its turns, one check against the task before a role that writes or runs commands may finish, and a map of its worktree. All on by default; `nudge_edits` is 4 ([`specs/045`](../../specs/045-harness-loop-techniques.md)) |
| `[loop] tool_result_limit_kb`, `clear_after_kb`, `trim_arguments_kb`, `keep_recent_turns` | keeping a long span's history small without losing any of it. A tool result over `tool_result_limit_kb` is stored and the history keeps a handle (`@payload/<id>`, readable with `read_file`) and a preview of at most 400 bytes; old results are replaced by a handle in batches of about `clear_after_kb`; old tool calls with large arguments keep a handle in place of the bulk (`trim_arguments_kb`); the newest `keep_recent_turns` (6) assistant turns stay whole. The first three are `0` (off) by default until measured; reading a handle counts as reading external content ([`specs/045`](../../specs/045-harness-loop-techniques.md) T5, [`specs/049`](../../specs/049-context-management-recoverable-compaction-and-handles.md) CX-1, CX-2) |
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
