---
id: council-015-spec-073-console-adaptive-contrast
title: "Council 015: Spec 073: Console Adaptive Contrast and Dual-Mode Palette"
author: orchestrator
timestamp: 2026-11-04T02:00:00Z
source_type: internal_generated
confidence: high
status: active
source_mission: 2026-10-23-foundational-design
tags: [council, council-015, spec-073, console, accessibility, wcag, contrast, dark-mode, light-mode, palette]
council_format: 1
---

# Council 015: Spec 073: Console Adaptive Contrast and Dual-Mode Palette

**Under review:** [`specs/073-console-adaptive-contrast-and-dual-mode-palette.md`](../../../specs/073-console-adaptive-contrast-and-dual-mode-palette.md) (draft), authored by `architect`.

**Seats (GOVERNANCE §3.2, specs/003 §4; blind Round 1, cross-examination Round 2, synthesis Round 3):**
- `architect` (Claude Opus family): author, structural integrity, ambient context contracts, Rust/TS parity.
- `advocate` (Gemini Flash/Pro family): human-centered accessibility, WCAG 2.1 AA evaluation, color perception, ergonomic legibility.
- `builder` (Codex/GPT family): implementation feasibility, relative luminance compute performance in TUI streaming, testability.
- `red-team` (Gemini/Llama family): adversarial testing, OSC 11 escape sequence injection, `NO_COLOR` / `TERM=dumb` invariant enforcement.
- `strategist` (GPT family): scope audit, ROI, slicing, non-goal enforcement.
- `orchestrator` (Claude Sonnet family): Council arbiter, synthesis, verdict.

**Proposition:** Should Spec 073 be approved, establishing a dual-mode semantic palette, ambient palette propagation in the console, avatar role chassis contrast adaptation in Rust and TypeScript, relative luminance contrast calculation, and automated WCAG 2.1 AA verification gates?

---

## 1. Round 1: Blind Stances

### architect

> OPTION: APPROVE WITH SPECIFIED GUARDS
>
> Spec 073 addresses a glaring visual defect in Delphi: on light-background terminals, essential semantic signifiers (`accent`, `warn`, `ok`, `alert`) and role chassis badges collapse below WCAG AA readability thresholds (dropping as low as 1.07:1).
>
> The architectural design centers on four structural choices:
> 1. **Zero-Churn Ambient Propagation (`ui/src/theme.ts`):** Module-level tracking (`activePal`, `usePal`, `currentPal`) allows existing semantic call sites (`T.accent()`, `T.ok()`, `T.warn()`) to remain zero-argument functions while dynamically responding to background polarity.
> 2. **Rust & TypeScript Parity:** `RoleTheme` in `crates/delphi/src/avatar.rs` adds `chassis_rgb_light` and `chassis_ansi_light`, mirrored in `avatar-spec.json` and `ui/src/theme.ts`.
> 3. **Mathematical Determinism:** Relative luminance calculation is grounded in the W3C sRGB formula, ensuring objective contrast evaluation across edge palettes (Solarized Light, Dracula, etc.).
>
> **Necessary Guards:**
> - To avoid unit test pollution where tests render frames across multiple palette polarities, `ui/src/theme.ts` must provide a clean state reset helper (`resetPal()`) and scoped runner (`withPal(pal, fn)`).
> - Synchronization between `avatar.rs` and `avatar-spec.json` must be strictly enforced via CI checks so the catalog and documentation never drift.

### advocate

> OPTION: APPROVE WITH ACCESSIBILITY ENHANCEMENTS
>
> From an accessibility and HCI perspective, this specification is mandatory. Terminal users on light backgrounds are currently subjected to severe visual fatigue; warning text in yellow (`#FFFF00`) on white is completely illegible at 1.07:1, violating the fundamental Gulf of Evaluation.
>
> **Strengths:**
> - Grounding the dual-mode palette in WCAG 2.1 / 2.2 Level AA ($\ge 4.5:1$ for normal text, $\ge 3.0:1$ for badges and headings) establishes an objective, falsifiable benchmark.
> - Tuning light mode semantic tokens to deep tones (Azure-700, Ochre-800, Emerald-700, Crimson-700) creates sharp readability while maintaining visceral harmony.
>
> **Defect in Spec 073:**
> - §4 provides ANSI 16-color fallbacks (`chassis_ansi_light`) for avatar chassis, but §3 **omits** ANSI 16-color fallback definitions for the semantic vocabulary (`accent`, `ok`, `warn`, `alert`, `auto`, `heading`).
> - When an operator runs inside a terminal without TrueColor support (`COLORTERM` unset), chalk downsamples RGB hex codes. If light-mode `warn` downsamples to standard ANSI bright yellow (ANSI 93), it will collapse on white backgrounds.
> - **Remedy:** The spec must explicitly define ANSI 16 fallbacks for all light-mode semantic vocabulary styles.

### builder

> OPTION: APPROVE WITH PERFORMANCE CONTROLS
>
> The proposed slicing (S0–S4, ~21 engineering hours) is modular, testable, and strictly respects Delphi's "One slice per pull request" rule.
>
> **Performance Concern:**
> - Relative luminance computation requires floating-point power operations ($C_{\text{lin}} = ((C + 0.055)/1.055)^{2.4}$).
> - Delphi's console streams tokens rapidly over UDS (`REV-TRAP-03`). If `ensureContrast` or relative luminance math is executed dynamically on every token or line render, it will introduce noticeable latency and frame drops in high-throughput streaming.
> - **Remedy:** All semantic palette tokens for dark and light modes must be precomputed constants. Dynamic `ensureContrast` recalculations must run **only** upon initial boot or when OSC 11 updates `pal.bg`, caching the resolved palette object in `activePal`. Zero floating-point math inside the per-frame render loop.

### red-team

> OPTION: CONCUR AFTER SECURITY FIXES
>
> While primarily a UI enhancement, dynamic palette adaptation interacts with raw terminal escape sequences and environment variables, presenting attack vectors:
>
> **Adversarial Vectors:**
> 1. **OSC 11 Terminal Query Escape Injection:** The console sends `\x1b]11;?\x07` and parses the response from stdin. An adversarial terminal emulator, malicious multiplexer, or hostile environment could inject malformed escape codes or terminal control characters to hijack console input.
> 2. **`NO_COLOR` / `TERM=dumb` Bypass:** Dynamic color adaptation must never emit ANSI SGR sequences when `NO_COLOR=1` or `TERM=dumb` is set.
> 3. **Reduce Motion Integrity:** Palette adaptation must remain pure functional token switching; no color fades, animated transitions, or private timers (`stillMotion` invariant in `motion.ts`).
>
> **Mandatory Requirements:**
> - `[RED-01]` **OSC 11 Response Sanitization:** `ui/src/input.ts` must strictly validate OSC 11 response strings against a tight hex regex (`^rgb:[0-9a-fA-F]{2,4}/[0-9a-fA-F]{2,4}/[0-9a-fA-F]{2,4}$`), sanitize control codes, and enforce a 100ms timeout failing closed to `dark: true, bg: null`.
> - `[RED-02]` **Strict `NO_COLOR` Invariant:** Automated contrast tests must explicitly verify that under `NO_COLOR=1`, zero ANSI color escapes are produced in either polarity.

### strategist

> OPTION: APPROVE WITH SCOPE DISCIPLINE
>
> Spec 073 delivers high ROI by resolving a persistent readability pain point without introducing architectural bloat:
>
> **Scope Audit:**
> - Non-goals (§1.1) correctly bar external CSS/theming engines and custom theme files. Delphi preserves its foundational philosophy: *"The terminal's theme is the palette"* (`specs/005e` §4.1).
> - Slices S0 through S4 are cleanly partitioned:
>   - S0: Spec approval and tracker entry (2h)
>   - S1: Contrast math, caching, and ambient context (4h)
>   - S2: Dual-mode semantic vocabulary and ANSI 16 fallbacks (6h)
>   - S3: Avatar role contrast adaptation in Rust & TS (5h)
>   - S4: Automated WCAG AA test matrix and doc updates (4h)
> - Total effort (~21h) is well within single-mission capacity.

---

## 2. Round 2: Cross-Examination & Critique

### architect
> Round 2 Response:
> Architect accepts all peer requirements:
> 1. Incorporates Advocate's requirement for deterministic ANSI 16-color fallbacks in the semantic palette (`[ADV-01]`).
> 2. Incorporates Builder's performance requirement: precompute palette styles at switch time and cache the resolved palette in `activePal` with zero per-token math (`[BLD-01]`).
> 3. Incorporates Architect's own test isolation requirement: export `resetPal()` and `withPal(pal, fn)` (`[ARC-01]`).
> 4. Incorporates Red-Team's OSC 11 regex sanitization, 100ms timeout, and `NO_COLOR` verification (`[RED-01]`, `[RED-02]`).
> 5. Incorporates `avatar-spec.json` synchronization guard (`[ARC-02]`).

### advocate
> Round 2 Response:
> With Architect's commitment to define ANSI 16-color fallbacks for the semantic vocabulary and Builder's caching discipline, the accessibility requirements are fully met.
> CONCUR: true

### builder
> Round 2 Response:
> Caching resolved palette objects on palette update eliminates all streaming overhead. Slicing S0-S4 is approved.
> CONCUR: true

### red-team
> Round 2 Response:
> With formal adoption of [RED-01] (strict OSC 11 regex validation & 100ms timeout) and [RED-02] (`NO_COLOR` zero-escape enforcement), Red-Team waives all blocking objections.
> CONCUR: true

### strategist
> Round 2 Response:
> Slices are realistic and scope is tight. Unanimously supported.
> CONCUR: true

## Round 2 structured output

- architect | recommendation | PROCEED-WITH-CHANGES
- architect | concur | true
- advocate | recommendation | PROCEED-WITH-CHANGES
- advocate | concur | true
- builder | recommendation | PROCEED-WITH-CHANGES
- builder | concur | true
- red-team | recommendation | PROCEED-WITH-CHANGES
- red-team | concur | true
- strategist | recommendation | PROCEED-WITH-CHANGES
- strategist | concur | true

## Verdict

**No deterministic escalation:** no unresolved P0 and the Round 2 recommendations agree. The decision is the operator's.

> **Council 015 Verdict: Unanimous Approval.**
>
> The Council recommends adopting Spec 073 with the ratified amendments:
> 1. **Dual-Mode Semantic Palette & ANSI 16 Fallbacks (`[ADV-01]`):** Implement dark and light mode semantic color tokens meeting WCAG 2.1 AA ($\ge 4.5:1$ text, $\ge 3.0:1$ badges/glyphs), including explicit ANSI 16-color fallbacks for terminals lacking TrueColor.
> 2. **Ambient Context & Test Isolation (`[ARC-01]`):** Provide module-level ambient palette propagation (`usePal`, `currentPal`) in `ui/src/theme.ts`, accompanied by `resetPal()` and `withPal(pal, fn)` to ensure test isolation.
> 3. **Luminance Caching (`[BLD-01]`):** Precompute palette tokens upon palette initialization or OSC 11 update; ensure zero relative luminance calculations run during active per-token 60Hz rendering.
> 4. **OSC 11 Sanitization & Safety (`[RED-01]`, `[RED-02]`):** Strictly validate OSC 11 responses in `ui/src/input.ts` with regex, enforce a 100ms timeout failing closed to dark mode, and verify that `NO_COLOR` / `TERM=dumb` produces zero color escapes.
> 5. **Avatar Parity (`[ARC-02]`):** Update `RoleTheme` in `crates/delphi/src/avatar.rs` and synchronize `avatar-spec.json` and `crates/delphi/src/observer.rs`.

## Decision and next steps

**Decision:** Adopt Spec 073 with ratified Council amendments for dual-mode palette, ANSI 16 fallbacks, palette caching, and OSC 11 sanitization.

1. `architect`: Update `specs/073-console-adaptive-contrast-and-dual-mode-palette.md` with ratified amendments and mark status as approved.
2. `builder`: Implement S1 (`ui/src/theme.ts` contrast math, palette caching, `usePal`, and `resetPal`).
3. `builder`: Implement S2 (dual-mode semantic vocabulary tokens and ANSI 16 fallback mappings).
4. `builder`: Implement S3 (avatar role chassis contrast adaptation in `avatar.rs`, `observer.rs`, and `ui/src/theme.ts`).
5. `validator`: Implement S4 (automated WCAG AA contrast matrix test suite in `ui/test/contrast.test.ts`).

## Accepted Risks & Minority Report (Red-Team / Dissent)

Unanimous — no unresolved P0/P1 (Red-Team concurs).
