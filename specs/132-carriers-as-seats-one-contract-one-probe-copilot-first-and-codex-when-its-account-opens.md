---
id: spec-132-carriers-as-seats-one-contract-one-probe-copilot-first-and-codex-when-its-account-opens
title: "132: Carriers as seats: one contract, one probe, Copilot first, and Codex when its account opens"
author: architect
timestamp: 2026-11-20T00:00:00Z
status: approved   # by the operator, 2026-11-20, IN ADVANCE, on the plan this spec carries out ("i approve this plan in full", given to the carrier-parity plan before this text existed), and the operator's later instruction the same day ("prioritize copilot even if we have to wait on codex for the account license to open up. copilot is extremely useful thanks to its variety of models"). This text was written afterwards by an architect agent from the plan, the P0 probe results and the code; the operator has not read it line by line. No Council has read it. Open decisions (section 14) take their recommendations unless the operator says otherwise. Slices that need a live run or a Council say so, and each live run spends the operator's tokens only on the operator's yes for it.
approved_by: operator (2026-11-20, in advance, on the plan)
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: medium   # code anchors were read on 2026-11-20 at main 56fe81ac (section 2 marks the two that were inferred, not run). The Copilot evidence is ONE probe run per size and model (section 3); no Codex measurement exists because the account is at its usage limit. No model was run and no Council was convened to write this.
source_mission: 2026-11-20-carrier-parity
tags: [spec, carriers, seats, copilot, codex, agy, council, panel, ask-family, structured-output, tools, families, quota]
---

# Spec 132: Carriers as seats: one contract, one probe, Copilot first, and Codex when its account opens

> **Status: approved by the operator in advance, on the plan, 2026-11-20** (see the frontmatter). Nothing is built. No code, schema or config changed with this spec.
> **Extends:** spec 131 (Gemini seats: a bounded prompt, the task at both ends, and a Council that counts who was heard; pull request #794, approved, not yet on `main` when this was written, so not linked). Spec 131 OWNS the per-carrier input budget (`max_prompt_bytes`, its section 3.2), the task at both ends (3.3), the carrier-neutral truncation check and "who was heard" (3.4), reading tools only for Council seats (3.5) and the operator-run input-ceiling probe (its S6, section 8.2). This spec takes all of those as given and adds the rest of what a carrier must do to be a seat. It also meets spec 128 (Round 2 without tools, one retry for an empty finish; pull request #792, not on `main`) at one point: the retry budget (section 6.4).
> **Builds on:** [`specs/047`](047-codex-harness-and-coverage.md) (Codex harness), [`specs/051`](051-copilot-cli-harness.md) (Copilot harness), [`specs/091`](091-seats-families-and-outside-launch.md) (receipts, families), [`specs/086`](086-outside-council-panel-and-host-subagents.md) (the Panel and its foreign seats), [`specs/104`](104-responsive-foreign-seats-jobs-progress-concurrency-standing-consent-and-a-four-call-panel.md) section 5 (standing consent), [`specs/020`](020-harness-tool-bridge.md) (the tool bridge), [`specs/046`](046-harness-and-openai-coverage.md) section 6 (coverage manifests and drift), [`specs/123`](123-the-council-and-panel-from-a-host-who-sat-what-it-cost-a-tail-and-the-loop.md) section 6 (`require_families`).
> **Number:** 132. On 2026-11-20: 127 to 131 are taken (128 #792, 129 #793, 130 on branch `worktree-app-onboarding-127`, 131 #794); `specs/` on `main` ends at 126. Verify again before merging.

---

## 1. Scope and objectives

The operator asked on 2026-11-20 for Delphi to "support codex and copilot equally as well as gemini" as seats in a Council, a Panel, a red-team pass and `ask_family`, then said: "prioritize copilot even if we have to wait on codex for the account license to open up. copilot is extremely useful thanks to its variety of models". This spec defines:

1. **One carrier contract** (section 4): what any carrier (`claude`, `agy`, `codex`, `copilot`) must do to be a seat, row by row, and **one conformance probe** that checks every row, run by the operator, extending spec 131's input-ceiling probe in `delphi endpoints test --probe`. The contract and the probe are carrier-neutral. **The order of proof is Copilot, then agy, then Codex.**
2. **Copilot as the multi-family carrier** (section 5): one endpoint per family it serves, the per-account model catalog read per run, a seat's model chosen by family and cost class, Council and Panel seats spread across every configured family so a third and fourth family is used, and Gemini through Copilot as a candidate second Gemini carrier.
3. **Copilot's gaps first** (section 6): a validated JSON-in-text answer with one retry (it has no schema flag), read-only Delphi tools (L1) behind `--additional-mcp-config` and its tool flags, Copilot in `delphi capabilities`, the hard-coded family in `delphi setup`, premium-request accounting, and the opt-in `gh` quota read.
4. **Codex, specified in full but waiting** (section 7): every Codex slice that needs a live run waits for the Codex account (usage limit until Nov 27, 2026 2:49 AM) or an API key, and none of them blocks a Copilot slice.
5. **The endpoints as a proposal** (section 8): exact TOML for the operator to ratify. This spec edits no config.

### 1.1 Non-goals

- No change to spec 131's budget table, its truncation check, its `heard` rule or its probe's size ladder; this spec adds rows to the probe, not a second probe.
- No Builder on Codex or Copilot. Both stay readers (spec 047 OD-1; spec 051 S2's ACP Builder path is unchanged and out of scope).
- Codex and Copilot as *hosts* (spec 110) are a different subject: here they are carriers Delphi starts.
- No change to `config.toml`, `[access]` or `[[endpoints]]` (Red tier, ratified by the operator). Section 8 proposes; the operator decides.
- No new families in code beyond `claude`, `gemini`, `openai` and `local` without the operator's decision (OD-5).
- No claim about any provider's true input limit, price or policy beyond the observations in section 3.

### 1.2 Simpler alternative considered

Add the endpoints to `config.toml` and change nothing else. Section 2 shows why that is not enough: Copilot would sit only when a role is mapped to its family (`council.rs:53-126`), a Copilot reader with the default `bridge = true` is told about tools it never receives (O10), its fractional premium requests are probably read as zero (O13), a Codex seat's family is always `unknown` (O7), and Codex's real out-of-quota text is not recognised as exhaustion (O12). The endpoints are still the first thing the operator can do (section 8) and the slices below assume they exist.

## 2. Observations tied to code

All anchors are `file:line` at `main` `56fe81ac`, read on 2026-11-20. "Inferred" means read in the code and not run.

| # | Observation | Anchor |
| :-- | :-- | :-- |
| O1 | **Both CLIs are accepted harnesses.** `Cli::from_harness` maps `codex` and `copilot`; `[harness.*]` accepts the four names. The validation message for a harness endpoint is stale: it lists `"claude"\|"agy"\|"codex"` and omits `copilot`. | `harness.rs:40-48`, `config.rs:1637-1638`, `config.rs:1581` |
| O2 | **Launch.** Codex: `exec --json --ephemeral --ignore-user-config --skip-git-repo-check --sandbox read-only --model <m> --config features.shell_tool=false`, `--output-schema <file>` when a schema is asked, prompt on stdin. Copilot: `--output-format json --model <m>` with eight hygiene flags, a log dir, and `--available-tools delphi-no-tools` (no tools) or `view grep glob`; prompt on stdin. | `harness.rs:994-1008`, `harness.rs:1014-1027`, `harness.rs:793`, `799` |
| O3 | **Readers only; both start untrusted.** A Codex Builder is refused; a Copilot reader and every Codex span are tainted to L0 ("its tools are not gated by Delphi"). | `harness_run.rs:570-593` |
| O4 | **Structured output.** Codex passes `--output-schema` (spec 047 §5 records one S0 probe on `codex-cli 0.165.0` whose answer conformed; no Delphi seat has run it). Copilot has no schema flag: the Council critique schema is dropped and read from the line format; any other schema is refused, so another seat takes the span. | `harness.rs:1003-1005`, `harness_run.rs:666-674` |
| O5 | **No Delphi tools for Codex.** The bridge is skipped for Codex; the `app-server` transport (which reports the model and carries approvals) exists but nothing sets `Opts.app_server` for a reader. | `harness_run.rs:700-701`, `harness.rs:481-496`, `harness.rs:990-993` |
| O6 | **Copilot's receipt names the served model, and a different model is refused** (`model_mismatch`). | `harness.rs:822-827`, `harness.rs:866-869`, `harness_run.rs:1082`, `1097` |
| O7 | **Codex `exec` reports no model**, so the receipt resolves family `unknown`, which never counts toward `require_families` or diversity; only `app-server`'s `thread/start` reports one. | `harness.rs:492-496`, `seat.rs:162-169`, `seat.rs:306-318` |
| O8 | **The family is the served model's, not the endpoint's.** `resolve` maps the reported model through the family table; `gemini-3.8-flash` (as Copilot reports it) falls through to `gemini-*`; `kimi-*` and `mai-*` have no entry and resolve `unknown`. | `seat.rs:162-169`, `model_families.toml:72-94` |
| O9 | **Family names are a closed set in config.** `role_family`, `primary` and `failover` accept only `claude`, `gemini`, `openai`, `local`. | `config.rs:1731-1741` |
| O10 | **A Copilot reader is told about tools it never gets (inferred).** The bridge is computed for every carrier except Codex when `bridge = true` (the default); for a Copilot reader the bridge starts and `lp.mcp_config` is set, but the Copilot launch never passes it (no `--additional-mcp-config`), while `harness_note` tells the model "Delphi's own tools are available to you as ..." and the first turn also says "Do not call tools". The receipt's `tools_offered` lists the bridge names. `receipt_toolless` papers over it for the `unverified-against-repo` label by treating every Copilot receipt as tool-less. | `harness_run.rs:701`, `734-764`, `harness_run.rs:211-230`, `harness.rs:912-914`, `harness.rs:1014-1027`, `harness_run.rs:1086`, `seat.rs:386-389`, `config.rs:231-232`, `291` |
| O11 | **Size estimates and rank.** `estimate` has rows for `copilot-acp`, `claude-cli`, `agy`; `codex-cli` takes the default (12,000, 24,000). `rank` puts `copilot-acp` first for a seat with no tools and `codex-cli` last for both. | `carrier_plan.rs:46-56`, `59-66`, `79-87` |
| O12 | **Codex's real out-of-quota text is not classified as exhaustion (inferred).** P0 captured "You've hit your usage limit. Upgrade to Plus ... or try again at Nov 27th, 2026 2:49 AM." with exit 1 in a normal event stream. `classify_cli_error` looks for "quota reached", "usage limit reached", "reached your usage limit", "hit your limit", "hit your session limit": none is a substring of "hit your usage limit". `parse_reset` reads only "resets in ..." and "regain access on YYYY-MM-DD". So the run fails as an ordinary CLI error: no `model.exhausted`, no reset time, no failover. The only Codex test with this phrase checks the parser's text, not the classifier. | `exhaust.rs:130-139`, `exhaust.rs:85-96`, `harness_run.rs:986-996`, `harness_run.rs:1104-1107`, `harness.rs:1865-1866` |
| O13 | **Fractional premium requests are probably read as zero (inferred).** `CopilotParser` reads `totalPremiumRequests` with `as_u64()`; `Event::Meter` holds `u64`. P0 saw `premiumRequests` 0.33 (claude-haiku-4.5) in the final usage, and spec 051 §6.4 calls the unit fractional. The test fixtures use whole numbers (2, 1). The nano-AIU count is unaffected. | `harness.rs:858`, `harness.rs:223`, `harness.rs:877-879`, `harness_run.rs:2020`, `2043` |
| O14 | **Availability lines.** `delphi capabilities` discovers claude, agy, codex, gemini and ollama, not Copilot. Copilot quota through `gh` is opt-in (`quota_source = "gh"`). The inventory text labels both carriers READ-ONLY, UNPROVEN. | `host_caps.rs:307` (in `crates/delphi/src`), `quota.rs:181-183`, `inventory.rs:177-178` |
| O15 | **Setup hard-codes Copilot as Claude.** `delphi setup` adds `copilot-sub` with family `claude` and Claude models only; its question still says "Builders and reviewers". | `setup.rs:386-395` |
| O16 | **Coverage pins are stale.** `codex.cov` says 0.165.0 (installed 0.166.0) and marks `cmd exec` `planned` while its flags are `used`; `copilot.cov` says 1.0.105 (installed 1.0.112) and its header says "Nothing is used yet". `copilot.cov:47` records that a stdio MCP server loaded through `--additional-mcp-config` in the S0 probe. `delphi doctor` warns on drift. | `codex.cov:1-2`, `27-28`, `copilot.cov:1-2`, `47`, `doctor.rs:799-818` |
| O17 | **The probe knows only agy.** `endpoints test --probe` reads a CLI version only for `agy` and saves a gate record only for `agy`. The receipt's `carrier.cli_version` is always `None`. | `endpoints_cli.rs:101-118` (104, 111-115), `harness_run.rs:1092` |
| O18 | **A third family is not used unless asked.** `assign` gives each role its `role_family` and moves a seat only to reach two families or to keep a critic off the author's family; `spread_families` runs only for `require_families`. Nothing blocks a configured endpoint from being planned. | `council.rs:53-126`, `council.rs:131-158`, `council.rs:1402-1411` |
| O19 | **No live seat run.** Live round trips exist as ignored tests; no Codex or Copilot seat has sat in a Council, Panel, red-team pass or `ask_family`. | `harness.rs:2238-2239`, `2274-2275`, `2298-2299`, `inventory.rs:177-178` |
| O20 | **Tracker contradictions.** Spec 091 S4 (`ask_family`) is "not started" while S6 says `ask_family` is built; spec 047's intro still says `codex` is not installed and nothing is built; spec 046's rows S1 to S7 all say "not started" (the parity map judged this understated; not re-checked row by row here). | `spec-tracker.md:200`, `860`, `169`, `158-165` |

Not read here: how the pool picks between two allowed endpoints of the same family for one seat (for example `agy-sub` and a Copilot Gemini endpoint). `engine.rs:988` passes only a family and a tier; slice C4 reads the router and specifies the rule (section 5.4).

## 3. P0 evidence (2026-11-20, operator's consent, raw results kept with the session)

Probe: a filler block of N bytes with an instruction at the very end to reply with a token `TAIL-OK-N`; the Copilot launch was Delphi's own (scratch `COPILOT_HOME`, `--available-tools delphi-no-tools`, explicit `--model`).

| Carrier, model | What was measured | Result | What was NOT measured |
| :-- | :-- | :-- | :-- |
| Copilot 1.0.112, `claude-haiku-4.5` | tail at 100, 200, 400 KB | the exact token each time; exit 0; 3.6 to 4.4 s; events name `claude-haiku-4.5`; `premiumRequests` 0.33 per call | anything above 400 KB; the real ceiling |
| Copilot, `gpt-6-luna` | tail at 200 KB | delivered; exit 0; model reported; `premiumRequests` 1 | 400 KB and above |
| Copilot, `gemini-3.8-flash` | tail at 200 KB | delivered; exit 0; model reported; `premiumRequests` **14** per call | 400 KB and above; `gemini-4-argon` (never run on Copilot) |
| Copilot, `claude-haiku-4.5` | "reply with only this JSON object" (one sample) | the object came back wrapped in a ```` ```json ```` fence | any schema-bound Delphi seat; a second sample |
| Codex 0.166.0 (ChatGPT plan) | bare, schema, 100/200/400 KB | every call exit 1 after 4 events: "You've hit your usage limit ... try again at Nov 27th, 2026 2:49 AM"; no model reported | everything: size, `--output-schema`, served model. **Codex slices begin with this probe** (X2) |
| agy, Gemini Flash (spec 131) | seven files, about 255 KB | the carrier cut 69,110 bytes from the tail; about 186 KB of file text arrived | the limit itself (spec 131 section 8) |

**A methodology lesson for the probe.** The first Copilot check passed falsely: Copilot echoes the user prompt in its own events (`user.message`), so a search of the whole stream for the tail token found it even when nothing was proven. The rerun read only `assistant.message` content. **The conformance probe reads the assistant's reply only, never the stream as a whole** (contract row C3).

**Gemini through Copilot is a candidate second Gemini carrier.** It returned the 200 KB tail where agy cut a 255 KB prompt at about 186 KB. Only 200 KB was tried, on one model that costs 14 premium requests a call; that is one data point, not a ceiling.

Other things seen in the Copilot stream and not yet used: one usage field read `tokenCount: 0` for the Gemini calls where Claude's read 29,588 to 113,893 (which field Delphi's parser reads, `accountingSnapshot.tokenDetails.input`, was not captured separately); each Copilot run injects a system reminder saying it is "*not* operating in a sandboxed environment". Both are probe rows (C3, C6).

## 4. The carrier contract

A **carrier** is a CLI or API path that runs one seat. A carrier is a **seat carrier** for a given installed version when it meets rows C1 to C9 below and the conformance probe has recorded that on that version. A row that is not met lowers what the seat may do; it never raises trust.

| Row | Requirement | How the probe checks it | claude | agy | copilot | codex |
| :-- | :-- | :-- | :-- | :-- | :-- | :-- |
| C1 configuration | An allowed endpoint names the harness, a family equal to its model's vendor, a credential and per-tier models. `delphi setup` and `delphi doctor` describe it correctly. | config check; the doctor line | met | met | O1 stale text, O15 setup family | O1 |
| C2 launch | Started headless, prompt on stdin (never argv), nothing of the operator's own config, a scratch working directory and home, no write or shell. | the CLI version, the args seen by a fake (tests); the live bare call (probe) | met | met (gate) | met (O2) | met (O2) |
| C3 input budget | Spec 131 sections 3.2 to 3.4 in full. The probe's ladder reads **the assistant's reply only**. | spec 131 S6 ladder | 131 | 131 | 131; P0 200 to 400 KB | 131; unmeasured |
| C4 structured answer | Native schema flag when the CLI has one; otherwise the validated JSON-in-text answer with one retry (section 6.1). Either way the answer is validated by Delphi against the schema before it is accepted. | one schema call and one deliberately fenced reply | native | native (`--json-schema`) | fallback (O4) | native, unproven live |
| C5 served model and family | The receipt carries the model the carrier reported and the family resolved from it. A carrier that reports none yields `unknown`, which never counts toward `require_families`. A mismatch is refused. | the bare call's events | met | met | met (O6) | not met on `exec` (O7) |
| C6 tool level | The seat is at a declared level: **L0** no tools, or **L1** read-only Delphi tools behind Delphi's gate (section 6.2). The receipt's `tools_offered` is what the CLI reported it offered, never what Delphi computed. | the stream's tool list; at L1, an allowed read succeeds, a write tool and a native tool are refused | L1 and above | L1 (hook gate) | L0 today; L1 unproven | L0 today; L1 unproven |
| C7 availability and quota | Out-of-quota output is classified as exhaustion with its reset time when the text gives one, so the pool marks the endpoint and fails over; quota is readable without a model call where the vendor allows. | a captured fixture per carrier (no live call); the doctor quota line | met | met | opt-in `gh` read (O14) | not met (O12) |
| C8 cost accounting | Each span records the vendor's own unit (tokens; Copilot premium requests and nano-AIU) exactly, fractions included. | parser tests on captured lines | met | met | O13 | tokens |
| C9 version pin | The coverage manifest is pinned to the installed version, the probe record names the version it ran on, and a newer version is "unprobed": its seat falls back to spec 131's default budget and L0 until probed again. | `--version`; manifest header; record key | pinned | pinned | stale (O16) | stale (O16) |

### 4.1 The conformance probe

`delphi endpoints test --probe [<endpoint>] [--rows C3,C4,...]` extends spec 131 S6; it is the one probe. Per allowed harness endpoint it:

1. Reads `<cli> --version` for every carrier (today only `agy`, O17) and refuses to reuse a record from another version.
2. Prints, before anything runs, the calls it will make and the estimated cost in the carrier's own unit: input tokens for every carrier, plus premium requests for Copilot from the catalog multiplier (for example, a four-step ladder on `gemini-3.8-flash` is about 56 premium requests at 14 each, on `claude-haiku-4.5` about 1.3). It runs only on the operator's yes for that run (spec 104 section 5's standing consent does not apply: the probe is not a foreign seat).
3. Runs, in order, stopping a row at its first failure: C2 bare call; C5 served model from the events; C3 the spec 131 ladder (assistant reply only); C4 one schema answer (native or fallback, including one reply that the carrier is asked to fence); C6 at the configured level (L0: the stream reports zero tools; L1: one allowed read, one refused write, one refused native tool, each seen in Delphi's bridge log).
4. Writes `local/carrier-conformance.json`: one record per endpoint, model and CLI version, row by row (`met`, `not_met`, `unmeasured`, with counts and the measured ceiling, never prompt text), with a time limit like the agy gate record. The existing agy gate record (`local/agy-gate.json`) becomes the agy C6 row of this record; the file stays readable for one release.
5. `delphi doctor` prints one line per endpoint: version, rows met, budget and its source (from spec 131), tool level, and "unprobed version" when the CLI moved.

C7 and C8 are proven with captured fixtures in tests, not live calls: a probe never spends tokens to reach a quota limit.

**Order of proof: Copilot, then agy, then Codex.** Copilot first because it works today and carries three families; agy second because spec 131 changes its budget and the probe must re-run afterwards; Codex when its account opens or an API key exists.

## 5. Copilot, the multi-family carrier

### 5.1 One endpoint per family

Copilot's one login serves Claude, OpenAI and Gemini models (and kimi and mai-code models whose families are `unknown`, O8). The family of an answer is the served model's (O8), and planning reads the endpoint's family, so **one Copilot endpoint per family**, each naming only models of that vendor, is the shape (section 8 has the TOML). An endpoint whose `models` mix vendors is refused by the config check (new, slice C3). Planning then sees the true family; the receipt still resolves it from the served model, and a mismatch is refused as today (O6).

### 5.2 The catalog, read per run

Spec 051 §6.4 records that `copilot --acp`'s `session/new` returns `models.availableModels[]` with `modelId`, `name` and `_meta.copilotUsage` (the multiplier, for example `0.33x`, `1x`, `14x`), and that the catalog is per account (23 models on 2026-10-28). Slice C3:

- reads the catalog through `--acp` `session/new` with no prompt, at most once per daemon start and per CLI version, cached with a time limit; **whether `session/new` alone spends a premium request is unverified** and is C3's premise check (if it does, the catalog is read only by `delphi doctor` and the probe, never per span);
- validates every Copilot endpoint's `models` against it: a model not in the catalog makes that tier unusable, said in `doctor` and in the plan, never silently swapped;
- records each model's multiplier and its **cost class**: `low` (under 1x), `standard` (1x), `premium` (over 1x). These are the vendor's labels, read, not computed.

### 5.3 Choosing a seat's model by family and cost class

A seat names a family and a tier, as today. On a Copilot endpoint the tier's model is the one the operator ratified in `models`; the plan (`carrier_plan`) shows its cost class and multiplier beside the token estimate, and a plan that would put a `premium` model in a seat says so before anything runs. Delphi never moves a seat to a cheaper model by itself (the operator's earlier rule: no cost-based lowering); the multiplier is information for the operator's choice of `models` (OD-2 recommends `gemini-4-argon`, listed at 1x, over `gemini-3.8-flash`, measured at 14 premium requests a call, for the Gemini endpoint).

### 5.4 Spreading seats across every configured family

Today a third family sits only if a role is mapped to it or `require_families` asks (O18). Slice C4 adds one rule to `assign`, carrier-neutral:

1. **When three or more families are usable**, after the existing rules (critics off the author's family; at least two families), each seat that shares a family with another seat is moved, red-team first, then strategist, the architect last and only if it is not the author, onto a usable family no seat holds yet, until every seat is on a distinct family or no unused family is left. This is `spread_families` with `want` equal to the number of seats, called always when three or more families are usable, not only for `require_families`. Deterministic; it never lowers a tier; every move is a note in the plan.
2. A setting `[seats] spread = "all" | "two"` (default `"all"` once C4 is built; `"two"` is today's behaviour) lets the operator keep two-family Councils. It lives in a ratified block, so it changes only by the operator.
3. **Two endpoints, one family** (for example `agy-sub` and `copilot-gemini`): C4 first reads how the pool picks today (not read here). The rule to build: prefer the endpoint whose spec 131 budget holds the assembled prompt with no cut; on a tie, the one whose conformance record has the higher measured ceiling; then config order. The receipt records which was used, as today.
4. The Panel's foreign seats (`carrier_plan`) follow the same rule: N foreign seats go to N distinct usable families when that many exist.

With the section 8 endpoints, a three-seat Council would sit on three families (claude, gemini, openai), where today it sits on two.

### 5.5 Gemini through Copilot

Gemini through Copilot is a **candidate** second Gemini carrier: it took a 200 KB tail where agy cut at about 186 KB. It is not preferred over agy by default until the probe's ladder (C7 slice) has measured both on the same sizes; then rule 5.4.3 picks the one that fits. Cost: `gemini-3.8-flash` through Copilot cost 14 premium requests a call in P0; `gemini-4-argon` is listed at 1x and has never run through Copilot.

## 6. Copilot's gaps, first

### 6.1 A validated JSON-in-text answer with one retry (C5)

For a carrier with no schema flag (Copilot today; any carrier whose probe row C4 is `not_met`):

1. The prompt's contract asks for one JSON object that matches the schema, and nothing else (the schema's text is already in the contract for other carriers).
2. Delphi extracts a candidate from the reply deterministically: the whole reply if it parses; else the content of exactly one fenced block (```` ```json ```` or a bare ```` ``` ````); else the single top-level `{...}` span when there is exactly one. Two candidates or none is "not found"; nothing is guessed.
3. The candidate is validated against the schema with the same validator the native path's result goes through. Valid: it is the structured answer (`structured` on the result, as a native one).
4. Invalid or not found: **one** retry in the same span with a fixed message ("Your answer was not one JSON object matching the schema: <the validator's first error path, no reply text>. Reply with only that object."). A second failure is `AnswerNotFound` with the counts.
5. The Council critique keeps its line-format reader (`harness_run.rs:666-670`) as today; the fallback replaces only the refusal at `harness_run.rs:671-674`.
6. **Shared retry budget.** Spec 128 S2 allows one retry for an empty finish; spec 131 S4 rule 7 halves the budget on that retry for a cut complaint. A seat gets at most ONE retry in total, whichever cause comes first (OD-3).

The same function serves an agy seat whose native `--json-schema` result is missing, if the operator wants that (OD-4); it is off for agy by default.

### 6.2 Tool levels: L0 now, L1 staged (C1, C8)

The operator accepted "tool-less or only some-tool seats". Levels:

- **L0, no tools.** Works today for both carriers (O2). Slice C1 makes it honest (O10): a Copilot or Codex reader with no MCP transport gets no bridge, so the prompt names no Delphi tools, the "do not call tools" sentence is true, and the receipt's `tools_offered` is empty. The taint stays L0 (O3).
- **L1, read-only Delphi tools behind a gate. Unproven on both carriers.** For Copilot: the bridge's MCP config is passed with `--additional-mcp-config`; the offered tools are exactly spec 131 section 3.5's reading set; `--available-tools` lists only those bridge tools by their Copilot names (`<server>-<tool>`, per `copilot.cov:47`), so no native tool (view, grep, glob, shell, write) is offered; `--deny-tool` names every native write and shell tool as a second wall (deny beats allow, `copilot.cov:26`); `--allow-tool 'delphi(<tool>)'` is passed only for the reading set; and every call still reaches Delphi's own bridge, which applies the call-time allowlist (a call outside the set is denied there even if the CLI let it through). The flags exist in 1.0.112; **none of them has been exercised by Delphi**; the S0 probe on 1.0.105 only saw a stdio server load. `receipt_toolless` (`seat.rs:386-389`) stops treating every Copilot receipt as tool-less and reads the stream's own tool list instead. The taint at L1 follows the agy rule: L0 until the probe's C6 row is `met` on the installed version, then trusted until the seat reads outside.
- For Codex, L1 would need a `-c mcp_servers.<name>...` override, which `codex.cov` refuses today ("any other key could widen authority"), and Codex `exec` has no per-tool allow or deny flag. Slice X5 starts with a probe of whether any deny-capable gate exists (the `app-server` protocol's `item/tool/call` and `mcpServer/elicitation/request`, spec 047 §5.1) and **stops with a recorded "not feasible" if none does**.

### 6.3 Copilot in `delphi capabilities` and `delphi doctor` (C1)

`discover_copilot` beside `discover_codex` (`host_caps.rs:271-275`, `307`): the version, whether a login exists (from the config file name only, never a token, per spec 051 §2.3), its MCP servers if a listing command exists without a model call (unverified; otherwise none), and the catalog only if C3 shows reading it is free. `delphi doctor` shows each Copilot endpoint's family, models found or missing in the catalog, cost class, and the `gh` quota line when `quota_source = "gh"` is set (opt-in, unchanged).

### 6.4 Premium-request accounting (C2)

- `premium_requests` becomes exact for fractions (`Event::Meter` and `harness.cost` carry thousandths as an integer, `premium_milli`, or an `f64`; the slice decides, the test is 0.33 in and 0.33 out). The fixture gains the 0.33 line seen in P0.
- Both the checkpoint's `totalPremiumRequests` and the final `result.usage.premiumRequests` are read; when both exist and differ, the final one is kept and the difference is an error-log note.
- The Council and Panel plans show premium requests beside tokens; spec 104's standing consent stays in tokens (OD-6 asks whether a premium-request cap is wanted too). Spec 051 §6.4 notes the account bills nano-AIU, so nano-AIU stays the ledger; premium requests are shown because the operator's plan screen shows them.

### 6.5 `delphi setup` (C1)

`copilot_harness` (`setup.rs:386-395`) stops writing a Claude-only `copilot-sub`. It asks once, then offers one endpoint per family whose models the operator's catalog lists, each with that family and that vendor's models; the question text says "read-only seats (no Builder)". Setup writes config only on the operator's answers in the wizard, as it does today; it never changes an existing endpoint.

## 7. Codex, specified and waiting

**Every slice in this section that needs a live run waits for the Codex account (usage limit until Nov 27, 2026 2:49 AM) or an API key, and none blocks a Copilot slice.** X1 needs no live run and may be built at any time.

- **X1 (no live run).** `codex.cov` pinned to the installed version, `cmd exec` marked `used`; `carrier_plan` gives `codex-cli` its own estimate row (the default until X2 measures) and a rank above "unknown kind"; the exhaustion classifier recognises "hit your usage limit" and parses "try again at <Month> <day>th, <year> <h:mm> AM|PM" as a reset time (the text names no zone: it is read as the machine's local time and the doctor line says so; unverified); the captured P0 text is the fixture; the run then emits `model.exhausted`, the pool marks the endpoint, and failover applies (`harness_run.rs:1104-1107`). `delphi doctor` prints "codex: out of quota until <time>".
- **X2 (live, operator-run).** The P0 Codex probe through the generalized probe: bare call, the ladder (100, 200, 400 KB, assistant reply only: the `agent_message` item), a schema call through `--output-schema` as a Delphi seat, and whether any `exec --json` event names the model.
- **X3 (served model).** If X2 finds a model field in `exec` events, read it. If not, the recommendation (OD-7) is to run Codex reader seats over `app-server`, which reports the model at `thread/start` (`harness.rs:494-495`) and is already built read-only; its `json_schema` refusal (spec 047 §5.1) then needs `turn/start` `outputSchema` probed, or the C5 fallback. Until then a Codex seat's family is `unknown` and the plan says "family not counted".
- **X4 (structured output, live).** `--output-schema` on a Council Round 1 shape and a red-team findings shape; if the answer does not validate, the C5 fallback applies.
- **X5 (L1, probe first).** As section 6.2; stops if no deny-capable gate exists.
- **X6 (live proofs).** As C9 for Codex.

## 8. The endpoints: a proposal for the operator to ratify

The operator answered "all families each carrier supports". **This spec changes no config.** Changing `[[endpoints]]` or `[access]` is a Red-tier ratified proposal; the operator edits or ratifies it. Example (model names from spec 051's catalog and the family table; `gemini-4-argon` and `gpt-6.1-terra` have never run through Copilot; the Codex model is the one `codex debug models` lists first and is unverified for this account):

```toml
[[endpoints]]
id = "codex-sub"
family = "openai"
path = "harness"
harness = "codex"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "gpt-6-luna", workhorse = "gpt-6-luna", frontier = "gpt-6-luna" }

[[endpoints]]
id = "copilot-claude"
family = "claude"
path = "harness"
harness = "copilot"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "claude-haiku-4.5", workhorse = "claude-sonnet-5.5", frontier = "claude-opus-5.5" }

[[endpoints]]
id = "copilot-openai"
family = "openai"
path = "harness"
harness = "copilot"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "gpt-6-luna", workhorse = "gpt-6-luna", frontier = "gpt-6.1-terra" }

[[endpoints]]
id = "copilot-gemini"
family = "gemini"
path = "harness"
harness = "copilot"
credential = "harness-login"
cost_model = "subscription"
models = { fast = "gemini-4-argon", workhorse = "gemini-4-argon", frontier = "gemini-4-argon" }

[access]
allowed_endpoints = ["claude-sub", "agy-sub", "copilot-claude", "copilot-openai", "copilot-gemini", "codex-sub"]
```

Notes for the operator:

- `copilot-claude` adds no family beside `claude-sub`; it is a second Claude path (failover, a cheap `claude-haiku-4.5` seat at 0.33x). Leave it out if not wanted.
- `codex-sub` can be ratified now and will fail over cleanly once X1 is built; until Nov 27 every call is exhausted.
- Copilot's kimi and mai-code models resolve to family `unknown` (O8) and `config.rs:1731-1741` accepts only `claude`, `gemini`, `openai` and `local`. Seating them as their own families needs a family-table entry and a config change: OD-5.
- With C4 built, nothing more is needed for a three-family Council; before C4, a third family sits only through `role_family` or `[seats] require_families = 3`.

## 9. Authority invariants and what the tests prove

1. **No widening.** L0 removes tools (C1). L1 offers only spec 131's reading set, each call still judged by Delphi's bridge, and the taint stays L0 until the probe's C6 row is `met` on the installed version. No slice adds a write tool, a shell, a Builder path, `--allow-all-tools`, or a `codex -c` key outside the allow-list without the X5 probe and a ratified change. Test: offer and call-time denial tests per carrier; a fake Copilot that calls a write tool is denied at the bridge.
2. **Words never raise trust.** The JSON-in-text extractor is deterministic and validates against the schema; a model's text cannot mark itself structured, select a family, or count as heard. Test: table tests of the extractor, including two fences, prose around JSON, and an answer that contains the schema's own text.
3. **The family is the served model's.** Planning reads the endpoint's family; counting reads the receipt's served family; `unknown` never counts. Test: a Copilot endpoint whose stream reports another vendor's model is `model_mismatch`; a Codex `exec` receipt counts as `unknown`.
4. **Config is read, never written.** Endpoints, `[seats] spread`, `quota_source` and the multiplier display come from ratified config; the catalog is a cache in `local/`, never config. Setup writes only on the operator's answers.
5. **Live calls need the operator's yes.** The probe and every live proof slice print the calls and estimated cost first and run only on the operator's yes for that run. C7 and C8 rows use fixtures.
6. **Records hold counts and labels, never prompt or reply text** (spec 039 section 3).

## 10. Slices

One pull request per slice, or several slices of THIS spec in one pull request under the same-spec trial in `AGENTS.md`. Hours are estimates. **Copilot slices come first; no Codex slice that needs a live run comes before them, and none blocks them.** The first slice that touches software (C1) begins with a premise check: re-read O1, O10, O14, O15, O16 at the then-current `main` and confirm each by a failing test before changing code; a premise that does not hold is written into the tracker row and that part is dropped.

| Slice | Content | Live run? | Hours | Depends on | Exit criteria |
| :-- | :-- | :-- | --: | :-- | :-- |
| S0 | this spec, compile, tracker rows | no | 0 (done) | none | merged |
| C1 | **PA quick wins (Copilot and shared).** Premise check. `config.rs:1581` text; setup per family (6.5); `copilot.cov` header and snapshot for 1.0.112; Copilot in `delphi capabilities` (6.3); `carrier_plan` estimate row naming; L0 honesty (6.2: no bridge for a reader without a transport; prompt, note and receipt agree); `docs/guides/model-access.md` gains Copilot and Codex; tracker fixes for 046, 047 and 091 S4 versus S6 (O20) | no | 6 | none | AC-1, AC-2, AC-3 |
| C2 | **Premium-request accounting** (6.4) and the doctor's `gh` quota line | no | 3 | none | AC-4 |
| C3 | **Catalog and per-family endpoints** (5.1 to 5.3): catalog read and cache (premise: is `session/new` free?), models validated, cost class shown, mixed-vendor endpoint refused | one `session/new` call, operator-run, to answer the premise | 6 | C1 | AC-5, AC-6 |
| C4 | **Spread across all families and pick within a family** (5.4), carrier-neutral | no | 6 | C1; spec 131 S2 for rule 5.4.3 (until then, config order) | AC-7, AC-8 |
| C5 | **Validated JSON-in-text with one shared retry** (6.1) | no | 6 | spec 128 S2 merged (shared retry) | AC-9, AC-10 |
| C6 | **The conformance probe** (4.1): every carrier's version, rows C2 to C6, cost printed first, `local/carrier-conformance.json`, doctor lines, agy gate record folded in | no (built with fakes) | 8 | spec 131 S6 | AC-11, AC-12 |
| C7 | **Copilot probe run (operator)**: per family (`claude-haiku-4.5`, `gpt-6-luna`, `gemini-4-argon`, and `gemini-3.8-flash` if the operator wants the comparison), a ladder of 200, 400, 600, 800 KB, assistant reply only, stopping at the first failure; C4 and C5 rows | **yes**, operator's tokens and premium requests | 1 | C6 | a record per model; the result written into this spec's tracker row |
| C8 | **Copilot L1** (6.2): `--additional-mcp-config`, `--available-tools`, `--deny-tool`, `--allow-tool` for the reading set, bridge-side denial, `receipt_toolless` from the stream | no (built with fakes); its C6 row needs a live probe run | 8 | C6; spec 131 S5 (the reading set) | AC-13, AC-14; then an operator probe run sets C6 `met` or `not_met` |
| C9 | **Copilot live proofs (operator)**: one Council with a Copilot seat on each of two families, one Panel foreign seat, one red-team pass, one `ask_family`, each with the operator's yes; context under spec 131's budget | **yes** | 2 | C5, C7 (C8 optional) | AC-15 |
| A1 | **agy through the same probe (operator)** after spec 131 S2 and S6 | **yes** | 1 | C6, spec 131 S6 | an agy record; doctor line |
| X1 | **Codex without a live run**: manifest, `exec` used, estimate and rank, exhaustion text and reset parse, doctor line | no | 4 | none (ordered after C1, may be built in parallel) | AC-16 |
| X2 | **Codex probe (operator)**: section 7 | **yes**; waits for the account or a key | 1 | C6, the account | a record; the answers to the three P0 questions |
| X3 | Codex served model: `exec` field or `app-server` readers (OD-7) | after X2 | 6 | X2 | AC-17 |
| X4 | Codex structured output live; fallback if not | **yes** | 2 | X2, C5 | a C4 row `met` |
| X5 | Codex L1: probe for a deny-capable gate, build only if found | probe **yes** | 2 to 8 | X2 | a recorded `feasible` or `not feasible` |
| X6 | Codex live proofs (operator) | **yes** | 2 | X3, X4 | AC-15 for Codex |
| F1 | **Drift and meters.** A newer CLI than the record is "unprobed" (C9 row); doctor drift for all carriers from the record; Copilot meter and Codex rate-limit reads in `capabilities` | no | 4 | C6 | AC-18 |

Order: S0, C1, C2, C3, C4, C5, C6, C7, C8, C9, A1, F1; X1 any time after C1; X2 to X6 after the Codex account opens, in that order. About 51 hours of building that does not wait (C1 to C6, C8, F1, X1), about 4 operator-run hours for Copilot and agy (C7, C9, A1), and 13 to 19 hours of Codex work (X2 to X6) that waits for the account or a key.

## 11. Acceptance criteria

1. **Text and setup (C1).** The harness-endpoint error lists `copilot`; `delphi setup` with a fake Copilot catalog offers one endpoint per family with that vendor's models and never a mixed one; `copilot.cov`'s header names the installed version and no longer says "Nothing is used yet"; `docs/reference/` regenerates without drift.
2. **Capabilities (C1).** `delphi capabilities` lists `copilot` with its version and login presence, and no token.
3. **L0 honesty (C1).** A Copilot reader with `bridge = true` and no MCP transport: the bridge is not started, the prompt contains no `mcp__delphi__` name, the first turn's "Do not call tools" is the only tool sentence, and the receipt's `tools_offered` is empty. The same for Codex (already true; asserted).
4. **Fractions (C2).** A stream with `totalPremiumRequests: 0.33` yields 0.33 in `harness.cost`; the existing whole-number fixture is unchanged.
5. **Catalog (C3).** With a fake `--acp` `session/new` answer, the catalog is cached with its version; a configured model missing from it makes that tier unusable with a doctor line; a multiplier of 14x shows cost class `premium` in the plan.
6. **Mixed vendors refused (C3).** A Copilot endpoint with `family = "gemini"` and a `claude-*` model fails the config check with a sentence naming the model and family.
7. **Three families used (C4).** With usable `claude`, `gemini`, `openai` and roles architect, strategist, red-team all mapped to `claude`, the plan seats three families, red-team moved first, the author's family avoided by critics; with `[seats] spread = "two"` the plan is today's.
8. **Two endpoints, one family (C4).** With `agy-sub` and `copilot-gemini` and a prompt over agy's budget but under Copilot's, the seat runs on `copilot-gemini` and the receipt says so; under both budgets, config order decides.
9. **Extractor (C5).** Table: bare JSON, one ```` ```json ```` fence (the P0 reply), one bare fence, prose around one object: accepted; two fences, two objects, none, invalid against the schema: not accepted. A valid answer has `structured` set exactly as a native one.
10. **One retry in total (C5).** A fake seat whose first reply is invalid JSON gets one retry with the fixed message and no reply text; a second invalid reply is `AnswerNotFound`; a seat that already used spec 128's retry for an empty finish gets no second retry.
11. **Probe (C6).** With fake CLIs: the version is read for every carrier; a fake that echoes the prompt in a `user.message` event and replies wrongly is `not_met` on C3 (the echo lesson); the cost line is printed before any call and nothing runs without the yes; the record holds no prompt or reply text.
12. **Version pin (C6, F1).** A record for another CLI version is ignored; the seat uses spec 131's default budget and L0, and doctor says "unprobed version".
13. **L1 offer (C8).** A Copilot L1 seat is launched with `--additional-mcp-config`, `--available-tools` listing only the reading set's bridge names, `--deny-tool` for native write and shell tools, and `--allow-tool` only for the reading set; a fake that calls `delphi-write_file` is denied by the bridge; the receipt's tools come from the stream.
14. **L1 trust (C8).** Without a `met` C6 row for the installed version the seat is L0; with it, trusted until it reads outside.
15. **Live proofs (C9, X6).** Operator-run; each records a receipt with the served model and family, `heard` per spec 131, and the cost; the tracker row states what ran and what failed. No acceptance criterion depends on a live result being good: the criterion is that it was run and recorded.
16. **Codex exhaustion (X1).** The captured P0 text classifies as exhaustion with a reset at Nov 27, 2026 02:49 local; `model.exhausted` is emitted; a text without a date still classifies, with no reset.
17. **Codex family (X3).** A Codex reader's receipt carries a served model and family `openai` from whichever source X2 found; if none, the plan says "family not counted".
18. **Drift (F1).** A CLI newer than its manifest prints the drift line and the "unprobed version" line for that carrier.

## 12. Verification Plan

| Item | How it is verified |
| :-- | :-- |
| C1 | Config-check message test; setup tests with a fake catalog; `cargo test -p delphi --test docs` after regenerating `docs/reference/`; a host-caps test with a fake `copilot --version`; harness tests with a fake Copilot asserting prompt, note and receipt for L0. |
| C2 | Parser test with the 0.33 line; `harness.cost` assertion beside the existing one (`harness_run.rs:2043`). |
| C3 | Fake `--acp` server answering `session/new`; config-check test for mixed vendors; plan text test. The premise call is operator-run once and its answer written into the tracker. |
| C4 | Pure tests of `assign` and `spread_families` over role and family tables; a pool-selection test for two endpoints of one family. |
| C5 | Extractor table tests; council tests with fake seats for retry and shared budget. |
| C6 | Fake CLIs of each carrier, including a prompt-echoing Copilot; record and doctor-line tests; no network. |
| C7, C9, A1, X2, X4, X6 | Operator-run. The command prints the calls and cost and waits for the yes; results are written to the conformance record and the tracker. |
| C8 | Launch-args test; bridge denial test; receipt-from-stream test; the L1 C6 row by an operator probe run. |
| X1 | Exhaustion classifier tests with the captured text; doctor line test. |
| F1 | Drift test with a fake newer `--help` and `--version`. |
| Gates | `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `delphi lint --repo .`, `delphi specs compile`, `delphi check spec` on this file. For this S0 (markdown only) only the last three plus `specs compile --check` were run; CI runs the rest. |

## 13. Known limits and what is not verified

- **Every Copilot number is one run.** One call per size and model; one fenced-JSON sample. 400 KB and above are untested for `gpt-6-luna` and `gemini-3.8-flash`; nothing above 400 KB is tested for any model.
- **No Codex number exists.** Size, schema and served model on Codex are all unmeasured; the only schema evidence is spec 047's S0 probe on 0.165.0, outside a Delphi seat.
- **O10, O12 and O13 are inferred from the code**, not run. Each slice's premise check confirms or drops them.
- **Whether reading Copilot's catalog costs a premium request** is unknown (C3 premise).
- **L1 is unproven on both carriers.** The Copilot flags exist; Delphi has never passed them. Codex may have no deny-capable gate at all.
- **How the pool picks between two endpoints of one family** was not read (C4 reads it).
- **The Codex reset time has no zone in its text**; reading it as local time is a guess.
- **The Copilot `tokenCount: 0` on Gemini calls** may mean Delphi's `tokens_in` is zero for Gemini-through-Copilot seats, which would trip spec 131's token-ratio truncation check (its section 3.4 item 0 (b)). Not checked; C7 records it.
- **Copilot's own system reminder says the session is not sandboxed.** It is Copilot's text, not a measurement; Delphi's scratch home and `--available-tools` narrowing are what bound a seat, and L1 adds a bridge-side check.
- **Costs and multipliers are the vendor's labels** on 2026-10-28 and 2026-11-20; they change without notice.
- A seat being heard does not make it right.

## 14. Decisions for the operator (with recommendations)

- **OD-1 (endpoints).** *Recommendation:* ratify section 8 as written, or without `copilot-claude`. *Alternative:* Copilot for `openai` and `gemini` only.
- **OD-2 (Gemini model on Copilot).** *Recommendation:* `gemini-4-argon` (listed 1x) for every tier, and run C7 on it before relying on it. *Alternative:* `gemini-3.8-flash` (measured, 14 premium requests a call).
- **OD-3 (one retry in total).** *Recommendation:* one retry per seat whatever the cause (empty finish, cut complaint, invalid JSON). *Alternative:* one per cause, up to three.
- **OD-4 (fallback for agy).** *Recommendation:* off; turn on only if agy's `--json-schema` results are seen missing.
- **OD-5 (kimi and mai-code families).** *Recommendation:* not now; they stay `unknown` and never count. *Alternative:* add families `moonshot` and `microsoft` to the family table and to `config.rs:1731-1741`, a separate change with its own tests.
- **OD-6 (a premium-request cap).** *Recommendation:* show premium requests in plans and doctor, no cap, since the account bills nano-AIU. *Alternative:* a `[access] standing_premium_requests` beside spec 104's token figure.
- **OD-7 (Codex served model).** *Recommendation:* after X2, if `exec` reports no model, run Codex readers over `app-server`. *Alternative:* keep `exec` and accept `unknown`.
- **OD-8 (spread default).** *Recommendation:* `[seats] spread = "all"` once C4 is built. *Alternative:* `"two"`, today's behaviour, and use `require_families` when more are wanted.
- **OD-9 (Gemini carrier preference).** *Recommendation:* by measured fit (5.4.3), agy first on a tie. *Alternative:* Copilot first for every Gemini seat.

## 15. Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | One carrier contract with rows, one operator-run conformance probe that writes a version-keyed record, and per-carrier slices that close each row; Copilot first (sections 4 to 7). | Fake-CLI tests per row and carrier; the probe's record and doctor lines; operator-run probe and live proofs. |
| B | constraint removed: "a seat runs through a vendor CLI" | Drop the CLIs for seats and call each vendor's API directly (`path = "api"`), where schemas, model ids and errors are uniform. | Run the same Council over API endpoints and compare receipts. Rejected as the main path: the operator's access is through subscriptions (Copilot's many models on one login, ChatGPT for Codex); API keys bill separately. Kept as the existing fallback the probe already treats as "none" for budgets. |
| C | constraint removed: "Delphi must parse each CLI's own events" | Treat every carrier as text in, text out, and parse only the final reply with the C5 extractor, ignoring event streams. | Fake CLIs that print only text. Rejected: the served model (C5), quota (C7) and cost (C8) live in the events; without them a family cannot be counted. Kept in part: C5's extractor is exactly this, for the answer only. |
| D | conformance testing (a named method: a test suite every implementation must pass, as protocol conformance suites do) | The probe of section 4.1 as the single gate: a carrier's rows are what it has passed on that version, and nothing is assumed. | The probe's fake-CLI tests, including a carrier that echoes the prompt (the P0 lesson) and one that cuts the tail. Chosen with A: A gives the work, D the gate. |
| E | adapter per vendor | A thin adapter per carrier that maps its quirks onto one internal seat interface, with no shared contract table. | Per-adapter tests. Rejected: it is roughly what exists (O2 to O7), and the gaps in section 2 are exactly the places where no shared row said what "done" means. |

Chosen: A with D. Flagged by the guard: none.

## 16. Council record

No Council has read this text. It was written by an architect agent from the operator-approved plan, the P0 results and the code at `main` `56fe81ac`, without calling any model. The approval is the operator's advance approval of the plan plus the instruction to put Copilot first. Slices C7, C9, A1, X2, X4, X5 and X6 are live runs that need the operator's yes; none of them is a Council of this text. A Council on this spec, if the operator wants one, should keep its context under spec 131's interim limit (about 150 KB) while a Gemini seat sits through agy.

## 17. Amendment (operator, 2026-11-21): Copilot ACP seats are confined (slice C10)

Approved by the operator in a host session ("6. make the copilot fixes"), on a recommendation drawn from a live probe of Copilot's ACP permissions on 2026-11-21 (about 2.6 premium requests; the evidence is summarised in `missions/2026-11-20-forge/F4-REPORT.md`'s follow-ups and below). Written afterwards by an architect agent; the operator has not read it line by line.

### 17.1 What the probe found

Copilot CLI (`--version` 1.0.114; ACP `initialize` reported 1.0.107; the installed link pointed at 1.0.105) in `--acp` mode:

1. **Delphi's ACP launch passes no `--available-tools`** (`harness.rs`, the `--acp` argv). Print-mode readers already pass `--available-tools delphi-no-tools` and get no tools; an ACP seat (a Copilot Builder) is offered every native tool, including `bash`, `sql`, `web_fetch` and the agent tools.
2. **The operator's user-level MCP configuration loads into the seat** even with `--disable-builtin-mcps`: the operator's `delphi-*` tools were offered.
3. **`bash` runs commands it classes as read-only without asking** (for example `cat` inside its working directory).
4. **A `view` of a file in the system temporary directory, outside the working directory, ran without a `session/request_permission`**, even with `--available-tools view grep glob --disallow-temp-dir`. Reads under `$HOME` outside the working directory did ask first, as did writes.
5. A rejected permission request ends the agent's turn.

### 17.2 The change

1. **An explicit tool list on every ACP launch.** `--available-tools` names exactly the tools the seat's role is meant to have: Delphi's bridge tools (by their Copilot names, `<server>-<tool>`) and, for a Builder, the native edit and create tools that reach Delphi's gate as permission requests. `bash` is never offered (a Builder runs commands through Delphi's own `run_command`, sandboxed), nor `sql`, `web_fetch` or the agent and workflow tools.
2. **No unasked reads outside the seat's places.** Each ACP launch passes `--deny-tool` rules for reads of the system temporary directories (`$TMPDIR`, `/tmp`, `/private/var/folders`, `/private/tmp`) except the seat's own scratch, and Delphi's ACP gate refuses every `kind:"read"` permission request whose path is outside the seat's worktree. Whether the deny rules close the unasked temp-directory read is checked live (17.3); if they do not, the seat's working directory and scratch are moved outside the system temporary directory and the remaining exposure is stated in `delphi doctor`.
3. **The operator's own Copilot configuration stays out.** The seat runs with a Copilot configuration home inside its scratch (the CLI's config-directory setting or environment variable, whichever the installed version honours), so user-level MCP servers, custom instructions and trusted folders do not load. Delphi's bridge is passed explicitly (`--additional-mcp-config`) where the role needs it.
4. **The watchdog sees reads.** For a Copilot seat, every `tool_call` of `kind:"read"` is checked against the seat's places (as spec 135 §14.7 rule 4 does for other carriers); a read outside them that no permission request preceded stops the seat and is logged.
5. **The version Delphi records is the one the ACP server reports** (`agentInfo.version`), not `--version` alone; a mismatch is shown in `delphi doctor`.

### 17.3 Verification

- Unit tests: the ACP argv carries `--available-tools` with no `bash` and the deny rules; the config home is the seat's scratch; the read watchdog stops a seat on an unasked out-of-place read (scripted stream).
- **A live re-probe, at most 3 premium requests**, run by the builder on the operator's standing approval for this slice: a `view` of a file in the system temporary directory and one under `$HOME`, both outside the working directory, `bash` not offered, no `delphi-*` user tools offered. The result is recorded in the tracker row. If the temporary-directory read still happens unasked, 17.2 item 2's fallback applies and the row says so.

Copilot review seats stay tool-less (spec 135 §14.7); this amendment does not give them read tools.
