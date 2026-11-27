---
state: run
date: 2026-11-27
title: "the live proof again, with the live canary"
eval_waived: "the commits with this entry change the writer's brief, the page validator and what a seed holds, not ranking, chunking or the index; the diff base counts knowledge-v2's own retrieval changes"
---

Slice E of spec 143. **Run 2026-11-27** by the operator with the coordinating session, on a copy of the operator's install. The build was `knowledge-v2` at `892f85bd`, installed as `delphi-kv2` on the cloned home `~/.delphi-kv2`; the operator's own install and home were not touched. No shim; the copy's API endpoint (`ollama`) was taken out of `allowed_endpoints` and ratified, so single calls had only the bridge.

## AC-9: a topic page written through the bridge (outcome and tokens, whatever they are)

- **Setup.** `delphi bridge setup claude` stored the token (`keychain:bridge-claude`); its test call answered.
- **Topic 1** (`how-are-software-architect-or-planner-ai`), started by `pages rebuild`.
  - Three bridged calls on `claude-sub`, job `pages`, origin `command`, all `ok`: 34,510, then 19,870, then 22,258 tokens. That is **76,638 tokens** for one attempt.
  - The writer was Sonnet (`workhorse`; the receipts say the CLI served `claude-sonnet-5-5`). The page validator was the `fast` tier.
  - Most of each count is the CLI's prompt cache. The usage events report 4 to 10 fresh input tokens and 850 to 3,200 output tokens per call.
  - **Outcome: the seed stands.** The validator judged `fabricated citation`, twice, on "The retrieved papers are MetaGPT, ChatDev, AgentMesh and …", citing the MetaGPT paper. The sentence is about the research, not the subject, and came from the scout's answer through the seed.
- **Topic 2** (`brand-strategy-and-product-strategy-prac`): **the seed stands.** `fabricated citation` on "Brand and product strategy practice is deciding what a product is …", citing a passage that says "Positioning says what a product is …". The writer widened it into a definition of the whole topic, because the brief made the lead start "**Title** is …".
- **What it showed.** The bridge, the writer, the validator and "a failure keeps the seed" (139 C) all worked; the failures were the writer saying more than its citation. One attempt costs about 77,000 tokens, so `[library.pages] week_tokens` (600,000) allows seven or eight a week.
- **Fixed on `knowledge-v2` the same day**, so the retry measures the policy rather than these faults:
  - The brief: no sentences about the research; a definition only when a record gives one.
  - Seeds leave out answers that drew no claim.
  - The validator (from the architect's review, help `t_0001`):
    - its reply is sized to the page and must end with `END ASSERTIONS`, so a list cut off by its cap is no verdict;
    - records shown in part, or left out for length, are marked, and a `fabricated` verdict against one is unchecked;
    - the prompt states how far a citation reaches;
    - every verdict is logged with the rejected draft.
- **The policy question** (one failing statement throws the whole page away) is spec 144, a draft.

## AC-12 (e): the live canary

`DELPHI_LIVE_CANARY=1 cargo test -p delphi-engine --lib live_canary -- --ignored --nocapture` (`harness_single::live_canary`). It planted a random marker in a fresh directory outside the call's home, then asked the bridge's own launch, with the operator's token, for that file.

- **By absolute path:** pass. The answer (368 characters) did not hold the marker. `claude-haiku-5-5`, 3,061 tokens.
- **By relative path** (`../` up to the root and down): pass. The answer (25 characters) did not hold the marker. 8,586 tokens.

## Also seen

- **A daemon's own idle rebuild reached Ollama** on the copy before the access change was ratified. `[bridge.jobs] pages = "manual"` governs bridged calls only; an API-served page follows `[library.pages] auto_rebuild`, as 143 §2.8 item 4 says. It was stopped by hand.
- **The copied page counter** (`library/pages/budget.json`) came from the real home, whose 0.6.7 daemon had spent all 20 of `per_day` on idle claims-only pages that day.

## Not verified

A page that passes. That needs the fixed build and the retry; spec 144 sets the thresholds the retry is judged by.
