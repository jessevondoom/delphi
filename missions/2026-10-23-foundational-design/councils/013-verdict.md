---
id: council-013-verdict
title: "Council 013: Spec issues report and build plan (303 issues, 25 workstreams)"
author: orchestrator
timestamp: 2026-10-30T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-013, verdict, spec-issues, build-plan]
---

# Council 013: Spec issues report and build plan

**Under review:** `delphi-spec-issues-and-build-plan-2026-10-30.md` (303 issues, 27 operator decisions D1-D27, 25 workstreams, about 350 h), written against `main` at 5133763. Checked against `main` at adb0836.

**Seats (spec 003 §4; blind Round 1, cross-examination Round 2, verdict Round 3):** `red-team` Ares on Gemini; `strategist` Odysseus on GPT; `architect` Daedalus on Claude Opus; `builder` Hephaestus on Codex; `validator` Horkos on Kimi. All seats were read-only. All five returned `concur_after_fixes: true`; the fixes are folded in below. Odysseus first could not read `/tmp` and `~/Downloads`; he re-ran Round 1 from copies.

## 1. Is the report valid?

- **The findings are accurate.** Horkos sampled 43 issues across all ten areas and verdicts: 42 are fully valid at HEAD, 1 is partially stale (TI34: the tracker's PS2/PS4/PS5 rows changed in PRs #386-#388), none wrong. No spec file changed between 5133763 and adb0836. The counts (303; 21/106/176; kinds; verdicts) and the 303-row coverage table reconcile. This supports the sampled findings, not every one of the 303. Tracker-dependent claims (TI34, CI31, LI19) must be re-read when built, not quoted from the report.
- **The severities are overstated.** Horkos re-rates the 21 "high" issues to 9 (RI1, CI1, CI2, MI4, MI5, LI1, LI2, LI11, OI1). The rest are mostly text drift (UI1, UI2, TI1, TI3, MI1, MI2, MI3, KI4, KI7, XI1, XI2, CI3). That is a consistency judgement on the report's own labels, not a risk assessment: Ares's two P0s map to CI1 and TI4.
- **Several recommendations are wrong against the code** (below). The report's §8 unknowns are resolved: `optimistic` is a real mode (`ui/src/mode.ts`, `approval.rs`); thinking blocks are persisted and read for recovery (`engine.rs`, `recovery.rs`, `claude.rs`); the update setting is `[update] require_signed` (neither `--allow-unsigned*` flag exists); `policy.rs` covers MCP managed settings only, and tier logic lives in `approval.rs`, `provenance.rs` and `connector_run.rs`; relay codes and TOTP replay/lockout state are in memory.

## 2. Recommended approach

**Do not run the 25-workstream, 350 h programme.** Run an evidence-first programme in two increments. Increment 1 contains confirmed authority and trust defects. Increment 2 fixes correctness and gates. The rest is deferred or won't-fix.

- **Per-slice approval and ratification.** No omnibus Red-tier batch (GOVERNANCE §5.7: the operator approves the diff). Mechanical wording in one file and one slice may share a proposal.
- **Each slice:** approved spec amendment first (No Spec, No Build), `Implements-Spec` trailer, tracker row in the same PR, red-team review, the three gates.
- **Hours are admission limits, not forecasts.** Hephaestus puts WS10/WS15/WS24 at roughly 1.5-2x the report. If verification expands a slice, re-estimate and defer a whole slice; never trim review or acceptance gates.

## 3. Decisions, corrected

**Decide now (operator):**

| D | Council answer |
| :-- | :-- |
| D2 | (a): keep storage, reword 005c test 9 to "no projection, snapshot or view contains reasoning" and 005 §2.1. (b) would break crash recovery. |
| D3 | (b): remove `optimistic` from the mode cycle; per-build flag only; confirm on entering `auto`. |
| D5 | (a): `agent.usage` with a `system` role. Include 049 CX-5. |
| D6 | (a): 044 §5.1 amended (badge, footer, countdown in the pane). The report's reason was wrong: 044 is newer than 005e. |
| D8 | **Corrected.** Tainted spans may write only `missions/<current>/research/**` (and the role's other `missions/*/…` globs), and those globs get an origin default of L0 in the Local zone: lower-only, no frontmatter trust (frontmatter is forgeable and never consulted). `context/domains/**` stays barred at L1 or below. `gh` CLI stays denied at L1 or below. Integrator acts outbound only through connector writes (Red). Cost: untainted architect notes in `research/**` also read as L0 in later missions; accepted, since it only lowers trust. `precheck_write` must also block tainted writes to any path whose origin default is above L0. |
| D9 | 003's wording (the code matches). |
| D10 | Code already does (c): a rejection consumes an attempt, upscales at attempt 2 and changes family at 3 (`build.rs#L148-L163, L211-L219`). Record built behaviour in 015 §5.4, unless the operator wants (a), which is a code change. **Minority Report below.** |
| D11 | Document PS5's console-only, compare-and-swap, preflight-rerun acceptance as the control, **provided** it shows the payload and records explicit operator acceptance; otherwise budget the missing control. Add: refuse `external_untrusted` source records and cap entries and bytes under `local/promoted/` (the tier cap already exists, `promote.rs#L13, L40`). |
| D12 | (a): merges and ratification console-only even with step-up; persist TOTP replay/lockout state across restarts. |
| D13 | Static JSON checks cannot see a hook's runtime decision. Refuse ratified `PreToolUse`/`PermissionRequest` hooks in `[harness.claude.settings]` for Delphi-run spans; guard hooks belong to 018. Wording: "runtime-only enforceable". |
| D14, D20, D22 | Confirm as the report recommends. D14 marks D-22 superseded. D22: no daemon timer. |
| D15 | (b): refuse `suggest_rule` from spans at L1 or below (a short guard in `tools.rs#L1063`); belongs with Increment 1 slice 1. Operator may instead choose "stamp the origin tier". |
| D16 | (a), plus fix the retro stub frontmatter (it would fail `cap_errors`). |
| D21 | The built `NoModel` floor rule governs (`assign.rs#L149-L159`); amend 056 §3.1 step 5. |
| D23 | Every draft gets its own card with the payload and **no allow-similar lookup** (the cache key `"<conn> draft"` carries no arguments, `connector_run.rs#L129-L136`); a draft from a span at L1 or below is Red; invitees stay Red. |

**Per slice later:** D4 (a): bulk Amber only for rendered cards, enforced in both `decisions-pane.ts` and `inbox.ts`. D7, D18, D19 (decide when 049/050 reach their measuring slices; BM1-BM3 only), D24, D25, D26, D27.

- **D24:** rename the host command; one shared host list (`archives.toml` has a different schema); pointer rule; **defer** merging `knowledge/web/` into the library (the 013 build trigger is not met: 74 `.md` files against about 150); digest `keep` to claim must carry origin taint.
- **D25:** a `delphi needs` reminder plus a contested-age flag; defer the §11.8 CI key.
- **D26:** stop `package` rewriting `files.json` after `sign` (`bundle_pack.rs#L167-L175`); order manifest, sign, package; countersignature before a second publisher key.
- **D27:** stays advisory until the §24 amendments are built and tested and a council of two model families passes.

**Drop or reduce:** D1 is a text fix (006 Zone table and test 4; the code has `[update] require_signed`, `update.rs#L269-L289`). D17 is not a contradiction (both caps are enforced, `checks.rs#L302, L323`); record the answer in one line in GOVERNANCE §6.2 so CI17 does not resurface.

## 4. Cuts

- **Cut:** MI16 capability consolidation (the six stores differ in scope and lifetime); TI35 code refactor (keep one tier table in the spec, since `Broker::need` and the relay check already centralise); WS25 omnibus sweep (fix touched banners locally; an optional `built-banner` lint is 4-6 h later).
- **Defer:** WS4, WS19, WS22. Land the XI2 one-liner as a doc fix outside WS22.
- **Pull in:** WS20 becomes a must (D23); the D2(a) text fix (WS2); UI1 text fix (WS6); UI2 one-liner (WS8); MI2 and TI1 stale-status lines inside WS13 and WS14.
- **Already built; do not reschedule:** SK8e (SK8f closed), ML5, PS2/PS2b/PS4/PS5, council round machinery, relay step-up, cross-family failover.

## 5. Work plan

**Increment 1: containment (admission limit 80 h; planning allowance 70-90 h). Lane A: `delphi-core`/`delphi-engine` provenance. Lane B: connectors, UI, relay.**

| # | Slice | Est. | Depends on |
| :-- | :-- | --: | :-- |
| 1 | Taint containment (D8) plus the `suggest_rule` refusal (D15): origin default for `missions/*/research/**`; `is_gated_path` strips `@worktree/`/`@project/<n>/`; `precheck_write` rule; tests across a mission boundary. Check first how far stamps carry (index scope). | 18-24 h | D8, D15 amendment |
| 2 | Connector drafts (D23): per-draft card, no allow-similar, Red at L1 or below. | 10-12 h | D23 amendment |
| 3 | Promotion hardening (D11): refuse `external_untrusted`, caps; verify the payload-bound acceptance first. | 4-6 h | 1 |
| 4 | Mode and bulk consent (D3, D4): remove `optimistic` from the cycle, confirm on `auto`, `rendered_ids` in both panes. | 10-14 h | 2 |
| 5 | Harness hooks (D13): refuse guard hooks in ratified settings. | 8-10 h | 3 |
| 6 | Relay merge boundary (D12): console-only merge and ratify, persisted TOTP replay/lockout. | 8-12 h | 4 |
| 7 | Integration and live check: cross-mission taint, drafts, promotion, approval modes. | 8 h | 1-6 |

**Increment 2: correctness and gates.** Meter system calls (D5, CX-5); T/D/F and the retro stub (D16); barrier-glob lint (WS9); validator gate wiring, advisory (D27); release and signing text (D1); bundle package order and report approval (D26); text fixes for D2, D6, D9, D10, D14, D20, D21, D22, D25. BM1-BM3 only if efficiency work resumes. Each as its own slice with the same per-slice rules.

**Order:** rebase the inventory against the tracker → ratify the amendment for each slice → implement and test → red-team review → merge → live check → reauthorise the next slice from results.

**Gates per slice:** `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo test --workspace --locked`; `cargo clippy --workspace --all-targets --locked -- -D warnings`; `cargo run -q --locked -p delphi -- lint --repo .`; for UI slices the UI suite; regenerate docs if a command changes.

## 6. Disagreements and resolutions

- **Scope.** The report proposed 25 workstreams; Odysseus proposed 60-80 h. Resolved as the two increments above.
- **Frontmatter `source_type` (Ares) against a glob origin default (Daedalus).** Frontmatter is never consulted, by design. Glob default adopted. Ares concurred.
- **One batched ratification (Daedalus' Round 1) against per-slice.** Daedalus conceded. Per slice.
- **PS5 gaps.** Ares reduced his claim to a P2 after reading `promote.rs` and `service.rs`.
- **MI16 and TI35 code.** Cut by Daedalus, Hephaestus, Ares and Odysseus.

## 7. Open questions for the operator

1. D10: keep the built behaviour (c), or change to (a)?
2. D11: is PS5's acceptance enough, or do you want a Red card with the payload shown?
3. D15: refuse proposals from L1-or-below spans, or stamp the origin tier?
4. D2: confirm that reasoning text is kept in storage for recovery.
5. Does stamp scope in `provenance.rs` extend beyond one mission? (Slice 1 checks this first.)

## Minority Report (verbatim)

**Daedalus, D10:** "D10: Daedalus holds (a). An operator rejection says nothing about the model's capability, so climbing the ladder (upscale at attempt 2, family change at 3; build.rs#L148-163, #L211-219) spends more on a model that was not at fault. Recording (c) as the spec makes the cost escalation permanent."

**Odysseus, D11:** "Promotion hardening must preserve explicit, payload-bound operator approval. Console-only transport and compare-and-swap are necessary controls, not substitutes for that approval. I support documenting an existing equivalent control if demonstrated; otherwise budget the missing control before calling PS5 hardened."

**Horkos, severity:** "Horkos: the re-rate 21→9 highs is my consistency judgement on the report's own severity labels, not a risk assessment; severity remained 'reader judgement' in the source report. Do not cite the re-rate as evidence the nine surviving highs are the only safety-relevant items — Ares' P0s (taint laundering, draft bypass) map to CI1/TI4, which the report rated high and med respectively."

Ares, Hephaestus: none.
