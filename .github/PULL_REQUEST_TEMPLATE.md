## Summary & Intent

<!-- Brief description of what this PR accomplishes -->

- **Implements-Spec:** `specs/NNN-<slug>.md` *(Required by `[REV-ARCH-01]` for all non-trivial changes)*
- **Mission ID:** `YYYY-MM-DD-<slug>`
- **Author Agent / Operator:** `hephaestus` / `@operator`

---

## Pre-Merge Verification Gates (`GOVERNANCE.md` & `REVIEW.md`)

- [ ] **`gate/spec-linkage`:** Linked specification in `specs/` has `status: approved` on `main` (or PR is a Stage 1 Spec PR authored by `Daedalus`).
- [ ] **`gate/deterministic-ci`:** All unit tests, integration tests, schema linters (`lint_manifests.py`), and `delphi doctor` checks pass with exit code `0`.
- [ ] **`gate/governance-zones`:** Zero unauthorized writes to Zone 3 operator-private paths or unratified edits to `GOVERNANCE.md`.
- [ ] **`gate/odysseus-alignment`:** Audited against `REVIEW.md` §3 for scope creep (*The Siren Protocol*).
- [ ] **`gate/ares-redteam`:** Audited against base-branch `origin/main:REVIEW.md` (`[REV-SEC-04]`) with `0` open `P0-BLOCKING` or `P1-MAJOR` findings.

---

## Accepted Risks & Minority Report (`Athena` / `Ares`)

> *Per `GOVERNANCE.md` §2.1 & §3.4, any unresolved `P2` warnings or accepted trade-offs from Council review must be preserved verbatim below:*

- None / See attached Council Verdict.
