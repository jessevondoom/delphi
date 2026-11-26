---
section: Added
---
- **A retrieval change asks for a before-and-after run** (spec 140 F): `delphi forge finish` on a branch that changes the retrieval code (`kb.rs`, `retrieval.rs`, `kb_docs.rs`, `embed`, `research_index.rs`, `use_signals.rs`, or the `[retrieval]` keys of the config) asks the slice's tracker file for `eval_before` and `eval_after`, two `delphi kb eval run` stamps on one golden set, and puts their comparison in the pull request body; `eval_waived: <reason>` stands in for a change that cannot move ranking. It is advice, not a merge gate. `delphi kb eval` on the repository's own set now also prints recall@20 and nDCG@10, and `dev/kb-eval-baseline.json` holds its last numbers: a test fails when recall@10 or nDCG@10 falls more than 0.02 below them.
