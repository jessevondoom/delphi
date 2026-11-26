---
id: knowledge-v2-handoff
title: "Knowledge v2: handoff (2026-11-25)"
author: orchestrator
timestamp: 2026-11-25T23:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-11-25-knowledge-v2
tags: [handoff, knowledge-v2]
---

# Knowledge v2: handoff (2026-11-25)

Hand this plan to the next agent or machine that continues the knowledge-v2 build. It covers where the build stands, what remains in order, and the decisions and errata waiting for the architect. Delete this file when the program is finished.

## 1. The goal

The operator wants one central store of knowledge that serves two audiences:

- **The operator** reads full encyclopedia pages: prose, links, images, diagrams and a bibliography.
- **Agents** read markdown files with rich frontmatter.

The store also keeps local source copies for retrieval, serves as long-term memory, and improves over time. The 2026-11-25 audit found the machinery of specs 112-120 merged, but it produced nothing:

- 0 built pages;
- every source a pointer;
- every endpoint a harness, so the writer, the validator and the scout's own calls could never run;
- the failure logged below the default log level.

## 2. Branches

| Branch | What | Remote |
|---|---|---|
| `knowledge-v2` | the integration branch: approved specs 139-143, then every built slice merged | pushed, no PR |
| `fix/page-rewrite-directive` | PR #912: the rewrite kept the `<!-- page: -->` line, so every second attempt failed (spec 097 §4.4) | open; the operator merges |
| `build/139-a`, `build/140-a`, `build/142-a`, `build/143-a` | the builders' branches, already merged into `knowledge-v2` | local only, scratch |

The specs were approved on 2026-11-25. The operator wrote "approve 139–143 as recommended, waive §1.3".

- **Reviews:** Councils t_0002 and t_0003, Panel t_0004 and a red-team recheck. Every finding is applied; see each spec's §9. The redacted records are under `missions/2026-11-25-knowledge-v2/councils/`.
- **Operator rulings, quoted in specs 141 and 142:**
  - every configured model route may see the whole knowledge base, Council seats included;
  - web research is safe;
  - labels guard only the sinks that leave the operator's machine;
  - the shipped default is `private_to_models = "all"`, with a `deny` option.

## 3. Built

All of these are merged on `knowledge-v2`.

| Spec | Slices | Notes |
|---|---|---|
| 143 bridge | D, A, B | D: notices when a background job cannot run, an error-level log line, a health line and a doctor warning. A: routing for `single_call`, and `one_shot_raw` returns a `Result` with causes. B: the `claude` call in a per-launch empty home with the environment scrubbed, redaction, the persisted ledger, `job_may_run`, the yield, the caps, the idle share and pause, cooldown and the reaper. |
| 139 pages | A, B | `topic_seed.rs`, `pages seeds --count`, seed pages (outcome `seed`, no stub). The `HeldText` trait currently uses `NoHeldText`; 142 B fills it in. |
| 140 retrieval | A, B, D | Stage timings, the golden set (`kb_eval.rs`, `kb eval build/accept/run/compare/runs`) and the baseline. |
| 142 cache | A, E, D1 | The capture record (`library_capture.rs`), `sources verify`, `text_sha256` anchors, pinned excerpts, orphans, `text_cache_max_mib` (1 GiB) with LRU by use. |

### 3.1 Measurements on a copy of the operator's install (2026-11-25)

- **Seeds (139 A).**
  - Seven of the nine research answers say only "No claim could be drawn from the sources read". The scout's model calls were blocked by the harness-only routing.
  - One topic has real research text.
  - Seed pages were built for all nine topics (118-1,061 words), filled mostly by knowledge-base search hits.
  - The operator's real research is in about 80 project research files. Importing them is an **operator runbook for that install**, not a spec. The operator does not want their own data written into specs.
- **Retrieval baseline (140 D).** golden-v1, 90 real cases, 817 relevant passages.

  | Path | recall@10 | recall@20 | nDCG@10 |
  |---|---|---|---|
  | service and hybrid | 0.9076 | 0.9361 | 0.9092 |
  | lexical | 0.5964 | 0.6222 | 0.6142 |

  - The builder accepted the set on the copy only so that it could run. The operator's own acceptance (140 OD-2) is still due; any change to the cases means a new baseline.
- **Vector deadline (140 A).**
  - A warm search takes 44 ms at p50 and 88 ms at p95.
  - Misses come only from the cold model load (300-450 ms) plus the lock backlog behind it.
  - Spec 140 §2.5's claim that the models reload after every sync is wrong.
  - Slice E should be the warm-up. The choice is OD-4.
- **Bridge spike (143 B), on claude 2.1.332.**
  - A one-year subscription token in an empty per-launch home signs in and answers, and the operator's own login does not leak in.
  - These flags work: `--safe-mode`, `--tools ""`, `--disallowedTools "mcp__*"`, `--strict-mcp-config`, `--disable-slash-commands`, `--no-session-persistence`, `--permission-prompts none`, `--max-turns 1`.
  - The canaries held: an ancestor `CLAUDE.md` was not obeyed, and a read by absolute path returned nothing.
  - **There is no OS sandbox:** the CLI fails under a file profile. That is a stated limit.
  - The daemon's run directory fails the walk to `/` (a `~/.claude` sits above it), so the home is built under the per-user temp directory.
  - So far no live call has run through the Rust bridge; those wait for slice E.

## 4. Remaining build, in order

The hours are the specs' own estimates.

1. **143 E**: the live proof on the operator's install, with no shim: one topic page written through the bridge, plus the live canary. **The operator does this with the agent**, after installing a build of `knowledge-v2`.
   - It needs the token in Delphi's store, which 143 G provides; build G first, or store the token by hand.
   - The token is currently in the macOS keychain under service `delphi-bridge-claude`. The bridge reads `keychain:<name>` as service `delphi`, account `<name>`.
2. **143 F** (commands, `[bridge.jobs]` modes, the tty confirmation, the ceiling's commands; 5-7 h) and **143 G** (`delphi bridge setup claude`, docs lines on quota and API keys; 2-3 h). 143 C (codex/copilot) is optional.
3. **142 B**, the largest risk. Start with the sender inventory (141 §2.4 item 8). Then rule (a): `local_text`, `rights: held`, the `text_held` mark, held text in the index, and `body_may_leave` with the run bound and the aggregate cap (1,200 characters or a tenth of the source). Then the 64-character block index, filling `HeldText`, and the interim guard at every exit. AC-9 blocks merge. About 12-15 h.
4. **142 C** (`capture_source`, 13-16 h), then **142 G** (source summaries through the bridge).
5. **The pointer backfill** (142 §2.7): started by the operator and bounded.
6. **139 C, D, E, F**: long depth, concept titles, build after research, the page budget.
7. **140 C, E, F, G**: synthetic cases, the deadline fix, the advisory finisher check, the quality history.
8. **141 A-F**: labels at the sinks. Private research reaches local pages when its last slice lands.
9. **Late**, released when 5 of the last 10 written pages pass: 139 G and H (writer refresh and learning), 140 H, 142 D2 and F.

About 26 slices and 140-185 h remain.

After the build:
- **PRs:** one stack per spec, as before. Gate-changing paths (`app_grants.toml`, `spawn_sites.toml`) go in their own PRs, which the operator merges at a terminal.
- **A follow-up spec:** "refused inside an agent seat" is a check on two environment variables (`xenia_terminal.rs:123`), so a host agent's shell passes as the operator for every operator-only command. 143 bounds it only where quota is concerned.

## 5. For the architect to confirm

The builders decided these themselves. Confirm each, or amend the spec.

- **143:**
  - The yield can deadlock when a span's tool makes a single call on its own endpoint; the wait is bounded at the call's timeout.
  - `confirmed` also passes a job's weekly budget.
  - A switch that says no refuses with `no_route`.
  - The daily cap is counted by UTC day.
  - A wait mid-job fails that call (a page can end claims-only).
  - `harness_room` is unfilled, and the ledger is never pruned.
  - The new `[bridge.claude] credential` key is not ratification-pinned; no credential key is.
  - §2.9 item 4 moved into A: a harness is ineligible until a token is named.
  - The added causes are `launch`, `empty` and `provider`.
  - The sandbox limit is already in §7.
- **139:**
  - Row 5 (ingested files) always gives nothing, because 113 makes them private.
  - The 300-character cap covers summaries too.
  - "Waiting" depends on linked rows only.
  - Rows 6 and 7 go under `## Related notes` and `## Claims`.
  - Seed frontmatter still says `author: writer`, `depth: summary`.
  - No health count by outcome.
  - A seed page is not due just for lacking a writer pass; slice E queues it.
  - The cut must leave room for the marker's leading space.
- **140:**
  - The builder wrote into spec §7, as AC-1 says, which GOVERNANCE forbids for builders.
  - recall@k = found / min(passages, k), with linear nDCG gains.
  - "The operator's view" means untrusted passages, all shared projects, and no reranker.
  - `tier` stands in for the 141 label.
  - `fix` replaces the question text.
  - The spike's queries are made by code.
  - Cases whose passages are not indexed are skipped and counted.
  - The commit comes from `DELPHI_GIT_COMMIT`.
  - AC-7 is tested at unit level.
- **142:**
  - The real extractor is `fact_fetch::extract_text`, not `html.rs:160`.
  - AC-29's hourly limit needs C.
  - AC-11 conflicts with the two passes; the reading taken is that the text survives and `raw` goes first.
  - AC-14 is built as "unchecked", not as a failed page.
  - `sources private purge` removes bodies.
  - Pinned excerpts live in `pinned.json`; treat it in the sender inventory.
  - `raw` is kept only with text.
  - `library.source` now writes the use ledger.
  - Archived copies and workspace documents have no capture record.
  - The cap runs only where `.with_text_cache` is set, so 142 B must set it wherever held text is stored.
- **Integration, by the lead:**
  - **142 D1 against 117 §3 item 2.** An encyclopedia read (`library.source`) now writes `library/cache-use.json`, because an open counts as use. The service test `every_encyclopedia_read_leaves_the_workspace_byte_identical…` exempts that one file. The architect should amend one of the two specs.
  - **143 against 139.** With no writer route, a rebuild builds the seed page, and the idle rebuild still raises `no_route` (`page_run.rs`).
  - **Two service tests** made a hand-made topic with no linked row. Since 139 B such a topic waits and gets no page, so the tests now link a research record.
  - **The egress scan** matched `body_may_leave(` as `may_leave(`. It now matches the whole word only.
  - **143's idle-job tests** now use a topic with a linked research record (`topic_with_material` in `service.rs`), for the same reason.
  - **`retractions_grant_nothing`** now lists `library_topics.rs`, whose seed build copies the library handle.
  - **FIRST TASK for the next session: one test is `#[ignore]`d on purpose.** `an_idle_rebuild_with_no_route_is_one_notice_a_day_and_holds_no_entry_text` follows 143 §2.7 item 6, where a topic with no writer stays due and the failure is counted again each hour. Under 139 B the seed page is built and is not due, so nothing is counted again.
    - The architect decides. One way: a seed page counts as due for expansion, which is what 139 E's "seed now, expand at idle" wants anyway, without writing a new seed version when nothing has changed.
    - Then un-ignore the test.
  - **SECOND TASK: two deadline tests.** `mcp_host::tests::get_agent_with_a_task_ends_with_a_section_outside_the_pack_and_its_hash` and `mcp_project::tests::a_task_section_never_shows_another_roles_memory_or_another_projects_files` pass alone. They failed in all three full local runs of `cargo test --workspace` on this branch, with no other build running on the machine.
    - They run into the retrieval deadline. Find out whether 140 A's stage timings in the service path, or the merge, made the service path slower under load.
    - Compare a full run on `origin/main`, and the CI result for this branch's first PR.
- **Local gates at the push** (the last commit on this branch): clippy `--workspace --all-targets -D warnings` clean; `delphi lint` 0/0; the `delphi` crate's tests pass; `delphi-engine` lib 3,207 passed, 2 failed (the deadline tests above), 14 ignored.

## 6. Traps

- **Rust:** cargo is not on PATH. Run `export PATH="/opt/homebrew/opt/rustup/bin:$PATH"`. Each builder's `target/` is about 30 GB; remove it at hand-back. Use `CARGO_BUILD_JOBS=3` with three builders on 8 cores.
- **Scratch directories:** give every builder its own. A shared script was overwritten once. Unix socket paths under a long scratch path fail, so use a short symlink.
- **Never point a local build at the operator's real home.** It could migrate the database. Work on a `cp -cR` copy with `--home`. `forge claim` writes to the real home.
- **Flaky tests:** `train_run::routing_arms…`, the `council` abstention test, and the `mcp_host`/`mcp_project` deadline tests fail under load and pass alone.
- **Public repo:** Council records print the operator's project names; redact that line before committing. Never write an employer name, an internal project name or a local path into specs or commits.
- **Shell guard:** the worktree guard refuses compound commands that mix heredocs, `cd` or `export` with git. Write files with an editor tool and run git commands one at a time.
