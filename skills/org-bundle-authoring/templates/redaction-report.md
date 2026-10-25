# Redaction report: <org>/<name> <version>

Written by `delphi bundle validate --report` and completed by the author. The human approves this report before anything is packaged (`skills/org-bundle-authoring`, step 9).

## Summary
- Files scanned: <n>  ·  Findings: <n>  ·  Resolved: <n>  ·  Left for the human: <n>
- Sources distilled from: <list of allow-listed paths>
- Sources refused (outside the allow-list): <list or "none">

## Findings
| # | File:line | Class | Found | Action | Reason |
| :-: | :-- | :-- | :-- | :-- | :-- |
| 1 | knowledge/runbooks/rolling-deploy.md:41 | local-path | `/Users/…/deploy.sh` | replaced by `scripts/deploy.sh` (repo-relative) | path is machine-specific |
| 2 | knowledge/decisions/0042-….md:12 | person-name | <name> | replaced by role `architect` | roles, not people |
| 3 | behavior/goals.md:9 | identifier | `<customer id>` | removed; goal reworded | customer data never ships |
| 4 | knowledge/architecture/overview.md:77 | e-mail | <address> | kept | publisher contact (false positive) |

Classes: `secret`, `token`, `private-key`, `local-path`, `e-mail`, `person-name`, `identifier` (from `[privacy] identifiers`), `customer-data`, `transcript`.

## Pointers that replace removed detail
| Removed from | Pointer added | Access hint |
| :-- | :-- | :-- |
| knowledge/runbooks/incident-first-30-minutes.md | https://wiki.acme.example/oncall (pin: none) | on-call group |

## Author's attestation
- [ ] Every finding above has an action or a stated reason to keep.
- [ ] No file under `knowledge/` contains instructions aimed at an agent (validator warnings addressed).
- [ ] No secret was found in a previous published version; or, the secret has been rotated (date: <date>).

## Human approval
Approved by: <handle>  ·  Date: <date>  ·  Package may proceed: yes / no
