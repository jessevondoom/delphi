---
state: built
date: 2026-10-28
title: "`check fact`, `ratify_fact`, stamp L3 `web_corroborated`"
---

Moved from `spec-tracker.md` by `delphi forge tracker migrate` (spec 135 §14.14) on 2026-11-24; the date is the first one in the state cell. The state cell did not map clearly to a state; `built` is the closest, and the cell is kept below as it was.

The row (line 304, under `The validator role and fact ratification (spec 040, approved 2026-10-28)`):

| Slice | Content | State |
| :-- | :-- | :-- |
| V2b | `check fact`, `ratify_fact`, stamp L3 `web_corroborated` | V2b-1 built 2026-10-28 (`delphi check fact`: pure check over stored snapshots; `fact_sources`, `fact_check`, `fact_judge` (second-model entailment judge, tokens reserved before dispatch), `fact_run` (advisory dry-run: fetch, check, judge, contradiction search), `fact_mode` (`min(config, latch)`, PROMOTE gate), `[validator]` config; advisory writes nothing). V2b-2 built 2026-10-28 (`fact_store` (L3 `web_corroborated`, never L4; immutable `attr-` kind; contested/disproved demotion, disproved recorded not deleted), `fact_intake` (daemon-normalised claims, per-caller/install rolling quotas, duplicate collapse, budget reserved before dispatch, receipts status + id only), `fact_ratify` (advisory records a dry-run only; ratify stamps via provenance), `fact_tools` (`ratify_fact`, `validate_claims` as event requests), kb `include_attributed`, `knowledge_status::derive_fact`). Wired 2026-10-29 (`fact_serve`: the daemon consumer of `validator.request`, mode re-read as `min(config, latch)` per request, daemon-run search for `validate_claims`, verdicts and stamps as `validator.verdict`/`provenance.stamp`, refusals as status-only `validator.event`; `ratify_fact`/`validate_claims` registered at engine boot unless mode is off; advisory by default, ratify needs PROMOTE). PROMOTE report and latch raise are V2c. Stand-in Council pass done (040 §24, REVISE); amendments are acceptance tests |
