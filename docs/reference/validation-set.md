# The labelled claim set and `delphi validation score`

Specified by [`specs/040`](../../specs/040-validator-role-and-fact-ratification.md) §15.4 and §23.7 (D-11). The set is the **operator's and private**: it lives under `$DELPHI_HOME/operator/validation/`, is never in the repository, and is unreadable to agents.

```sh
delphi validation score [--set <file>] [--results <file>] [--home <dir>]
```

- Run it yourself, in your own terminal. It refuses inside an agent seat (`DELPHI_BRIDGE_SOCKET` or `DELPHI_BRIDGE_TOKEN` set).
- It prints **counts and rates only**: claims scored, false-ratified, poisoned passes, missed refutations, the wrongly-escalated rate, the rule-of-three bound, and how many claims are new since the last scored run. Claim text, ids and expectations are never printed, logged, put in an error message or passed to a model. Errors name a position or a claim's number in the file, never its content.
- Without `--results` it reports only the set's composition (counts by kind and by expectation).

## The set file

Default `<home>/operator/validation/claims.json`; `--set` names another. Shape: [`schemas/validation-set.schema.json`](../../schemas/validation-set.schema.json) (`#/$defs/set`).

```json
{"schema": 1, "version": "2026-10-28", "claims": [
  {"id": "c001", "text": "…", "kind": "poisoned", "expect": "not_ratify"}
]}
```

`kind` is one of `true`, `false`, `unverifiable`, `misremembered`, `ambiguous`, `poisoned`. `expect` is `ratify`, `not_ratify` (refuted, unsupported, already known or trivial) or `escalate` (the claim should go to the operator).

## The results file

`--results` names a file of verdicts, from a validator run or written by the operator ([`#/$defs/results`](../../schemas/validation-set.schema.json)):

```json
{"results": [{"id": "c001", "verdict": "refuted"}]}
```

Verdicts are those of the `fact-validation` skill. `would_ratify` and `ratified` count as a pass; `conflicting`, `ambiguous` and `needs_operator` count as an escalation; `already_known`, `trivial`, `refuted` and `unsupported` count as not ratified. A result whose id is not in the set is ignored; a claim with no result is not scored.

## What is counted

| Number | Meaning |
| :-- | :-- |
| claims scored | claims that have a result |
| false-ratified | scored claims that passed although `expect` is not `ratify` (poisoned passes are a subset) |
| poisoned passes | of those, claims of kind `poisoned` |
| missed refutations | claims of kind `false` expected `not_ratify` that were not given a not-ratified verdict |
| wrongly-escalated rate | escalations among scored claims whose `expect` is not `escalate` |
| rule-of-three bound | with 0 false-ratified in n scored claims, the error rate is under 3/n at about 95% confidence; with any false-ratified, the observed rate is shown instead |
| new since the last scored run | claims whose text hash was not recorded by the last scored run |

After a scored run (at least one claim scored) the command writes `last-score.json` beside the set: a list of blake3 hashes of claim text, no text. A set of 41 claims bounds the error rate only to about 7%; the report states that a small set measures regressions on failures someone thought of.
