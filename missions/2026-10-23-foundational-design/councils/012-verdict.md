---
id: council-012-verdict
title: "Council 012: Performance and LLM-efficiency work plan"
author: orchestrator
timestamp: 2026-10-30T00:00:00Z
source_type: internal_generated
confidence: medium
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-012, verdict, efficiency, search-pages, project-index]
---

# Council 012: Performance and LLM-efficiency work plan

**Under review:** a five-item plan (search-page gather reuse of the knowledge index; deterministic project `_index.md`; deterministic `/promote` preflight; deterministic trace mining; context trimming and offload) plus two secondary items (dirty-set `Kb::refresh`, progressive tool definitions). Repository at `main` after PR #381.

**Seats (spec 003 §4; blind Round 1, cross-examination Round 2, verdict Round 3):** `red-team` Ares on Gemini; `strategist` Odysseus on GPT; `architect` Daedalus on Claude Opus; `builder` Hephaestus on Codex; `validator` Horkos on Kimi. The plan's author is a Claude model, so the red-team seat is on another family. All seats were read-only. All five returned `concur_after_fixes: true`. Odysseus could not read the Round 1 stance file in Round 2 and answered from the question text.

## Agreement

- Item 5 is mostly built: 045 H2 / 049 CX1 merged (#375), off by default and unmeasured. CX2 to CX6 remain.
- Spec 048 is a draft. Work that measures against it, or enables a default from it, waits for the operator. Progressive tools is approved slice 045 H5, not started.
- Item 1 is not a drop-in `Kb` swap. The kb library zone holds only non-private, full-capture sources of 512 KiB or less, includes retired sources, holds no claims, and prunes unseen paths. `gather` covers claims, excerpt and pointer sources, private internal sources and documents up to 64 MiB, and excludes retired ones.
- Gather's cost is whole-file I/O: `stored_text` reads the whole `source.md` and, for documents, hashes it, before `SCAN_MAX` (characters, not bytes) is applied. The hash is also the `SourceAltered` integrity check and must survive.
- PS1 and PS3 of spec 058 are built on `main`; the tracker was stale. The `_index.md` stub records no generated hash.
- Spec 004 §3.7 requires a refresh before every search. A dirty set is allowed only with a proven synchronous freshness guarantee or an approved amendment.
- ML5 does not depend on 048.
- Promotion eligibility must be checked at preflight and again inside acceptance, under the compare-and-swap.

## Disagreements and resolutions

- First slice: Ares first said ML5, Hephaestus and Horkos said gather. Resolved after Round 2: baseline and parity fixtures, then the sidecar, then ML5.
- Index approach: kb dual path (Hephaestus), library-native index (Ares), sidecar word-set cache (Daedalus), baseline-gated (Odysseus). Resolved: sidecar, gated on measurement, kept outside source directories; the kb path and a library-native index are deferred.
- Whether 058 index and promotion work is efficiency work. Odysseus: feature completion, off the efficiency critical path. Daedalus: keep it in the ordered list. Resolved by keeping it in the build plan, labelled feature work.

## Verdict (recommended order)

1. Correct the plan and the tracker (docs).
2. Instrument gather and add parity fixtures; one small PR, no new dependencies.
3. Word-set sidecar for `gather` and `page_stale`, behind a flag, with oracle parity; build only if read, hash and tokenize account for 70% or more of gather time.
4. ML5 trace mining.
5. 058 PS4, then PS5 with the promotion preflight re-run inside acceptance, then PS6, PS7.
6. Operator decision on 048; then CX2 to CX6 and H5, only if measurements support them.
7. A library-native postings file or a kb dirty set only if measured (enumeration and meta parsing above about 50 ms p95 at 10k sources, a trigger and not a gate); a dirty set needs a 004 amendment or a synchronous guarantee.

## Minority Report

- **Ares (red-team):** "I maintain my R1 findings on the Kb recall and privacy gap, the 048 draft gate, PS4 `_index.md` hash-lock with stub adoption, and PS5/PS6 `retro::accept` CAS re-validation. The sidecar is acceptable only in a separate wordcache keyed on `source_print` patched to include `excerpts_hash`, with eviction on purge and revoke. I still reject any `Kb`-backed first step."
- **Odysseus (strategist):** "Relabel PS4/PS5 as feature completion and remove them from the efficiency critical path. Dirty sets are not forbidden by 004 §3.7, but unproven eventual freshness is unacceptable. The 50 ms threshold is a trigger, not a ship gate."
- **Daedalus (architect):** "The sidecar is conditional on the measurement: at least 70% of time in read, hash and tokenize. If enumeration and meta parsing still exceed about 50 ms p95 at 10k, a library-native postings file derived from the sidecars follows. Watchers stay off without a 004 amendment."
- **Hephaestus (builder):** "A kb candidate experiment is optional and belongs in a separate guarded PR, and only after zone-scoped sync exists."
- **Horkos (validator):** "Any indexed candidate path must keep the `stored_text` sha256 integrity check, or consciously amend it. A naive kb swap is blind in both directions. Enablement and acceptance of CX4 to CX6 need 048."

## Open questions

- Approve 048 now, or measure with 037 / 045 H9 suites meanwhile?
- Does the operator want 058 PS4 to PS7 tracked as efficiency or feature work?
- What hardware and latency budget would turn the refresh threshold into a service-level objective?
