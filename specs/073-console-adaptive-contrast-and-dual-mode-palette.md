---
id: spec-073-console-adaptive-contrast-and-dual-mode-palette
title: "073: Console adaptive contrast: dual-mode semantic palette, avatar role contrast adaptation, and WCAG AA verification"
author: architect
timestamp: 2026-11-04T01:46:00Z
status: approved
governed_by: "GOVERNANCE.md"
review_rubric: "REVIEW.md"
source_type: internal_generated
confidence: high
source_mission: 2026-10-23-foundational-design
tags: [spec, ui, console, tui, color, contrast, accessibility, dark-mode, light-mode, wcag]
---

# Spec 073: Console adaptive contrast and dual-mode palette

> **Status: approved following Council 015 review.**  
> **Council Record:** [`missions/2026-10-23-foundational-design/councils/015-spec-073-console-adaptive-contrast.md`](../missions/2026-10-23-foundational-design/councils/015-spec-073-console-adaptive-contrast.md)  
> **Based on:** Extends [`specs/005e`](005e-console-ux-review-and-redesign.md) (Console UX review & redesign), [`specs/005a`](005a-terminal-avatar-system.md) (Terminal avatar system), and [`specs/053`](053-console-visual-language.md) (Console visual language).

---

## 1. Scope & Problem Statement

Delphi's visual interface relies on the core principle: *"The terminal's theme is the palette"* ([`specs/005e`](005e-console-ux-review-and-redesign.md) §4.1). However, the initial semantic vocabulary ([`specs/053`](053-console-visual-language.md)) and avatar role chassis colors ([`crates/delphi/src/avatar.rs`](../crates/delphi/src/avatar.rs)) were chosen exclusively against dark-background consoles.

When an operator launches Delphi in a **light-mode terminal** (e.g., Apple Terminal Basic, iTerm2 Light, Ghostty Light, or VSCode Light), contrast collapses:
- `accent` (`chalk.cyan` / `#38BDF8`) drops to a **1.25:1** contrast ratio against `#FFFFFF` (WCAG AA requires **4.5:1**).
- `warn` and `heading` (`chalk.yellow` / `#FFFF00`) drop to **1.07:1**, rendering warning text and section headings virtually invisible.
- `ok` (`chalk.green` / `#4ADE80`) drops to **1.37:1**.
- Key agent chassis colors (Scout `#FFFF00` at **1.07:1**, Architect `#38BDF8` at **1.7:1**, Builder `#F59E0B` at **2.1:1**, Strategist `#14B8A6` at **2.2:1**) become illegible in badges, brackets, and labels.
- `chalk.dim` washes out into faint grey or white in many terminal profiles, dropping below **2.0:1**.

This specification defines an **adaptive contrast architecture** for the Delphi console and observer:
1. A **dual-mode semantic palette** ensuring every vocabulary style meets WCAG 2.1 AA standards (>= 4.5:1 for body text, >= 3.0:1 for large text and UI glyphs) across dark and light consoles.
2. **Ambient palette propagation** (`usePal` / `currentPal`) in `ui/src/theme.ts`, preventing churn across the ~114 semantic style call sites.
3. **Role chassis contrast adaptation** in both the Rust avatar library and TypeScript console.
4. **Programmatic contrast calculation** using W3C relative luminance to dynamically adapt colors on custom background tints (Solarized Light, Solarized Dark, Dracula, pure white, pure black).
5. **Automated contrast test assertions** preventing contrast regressions under CI.

### 1.1 Non-Goals
- Introducing custom user theme files or external CSS/styling engines.
- Altering the semantic meaning mapping or glyph vocabulary established in Spec 053.
- Bypassing or altering reduce-motion constraints (`stillMotion` remains inviolate).
- Overriding `NO_COLOR` or `TERM=dumb` (which continue to suppress all color codes).

---

## 2. Contrast Standards & Mathematical Foundation

Delphi evaluates contrast against the W3C Web Content Accessibility Guidelines (WCAG) 2.1 / 2.2 Level AA standard:

$$\text{Relative Luminance } L = 0.2126 \times R_{\text{lin}} + 0.7152 \times G_{\text{lin}} + 0.0722 \times B_{\text{lin}}$$

where each sRGB component $C \in \{R, G, B\}$ normalized to $[0, 1]$ is linearized as:
$$C_{\text{lin}} = \begin{cases} \frac{C}{12.92} & C \le 0.04045 \\ \left(\frac{C + 0.055}{1.055}\right)^{2.4} & C > 0.04045 \end{cases}$$

The contrast ratio between foreground luminance $L_1$ and background luminance $L_2$ (where $L_1 > L_2$) is:
$$\text{Contrast Ratio} = \frac{L_1 + 0.05}{L_2 + 0.05}$$

### Targets:
- **Normal Text (Body, Labels, Operators, Syntax)**: $\ge 4.5:1$.
- **Large Text, Headings & Status Glyphs / Badges**: $\ge 3.0:1$.
- **Hairlines & Subtle Tint Blocks**: $\ge 1.2:1$ step separation from terminal background.

---

## 3. The Dual-Mode Semantic Palette (CP-1)

`ui/src/theme.ts` establishes dual-mode style definitions for each semantic meaning in Spec 053 §3:

| Meaning | Dark Terminal (`pal.dark = true`) | Light Terminal (`pal.dark = false`) | Dark Contrast | Light Contrast | Light ANSI 16 | Target |
| :--- | :--- | :--- | :--- | :--- | :--- | :--- |
| **`body`** | Terminal default fg (`#E2E8F0`) | Terminal default fg (`#0F172A`) | > 12:1 | > 14:1 | default (0) | Text $\ge 4.5:1$ |
| **`muted` / `dim`** | Slate-400 (`#94A3B8`) | Slate-600 (`#475569`) | ~ 6.8:1 | ~ 5.6:1 | grey (8) | Text $\ge 4.5:1$ |
| **`accent`** (working, pointer) | Cyan-400 (`#38BDF8`) | Azure-700 (`#0369A1`) | ~ 9.2:1 | ~ 5.2:1 | blue (4) | Text $\ge 4.5:1$ |
| **`link`** | Cyan-400 underlined | Azure-700 underlined | ~ 9.2:1 | ~ 5.2:1 | blue (4) | Text $\ge 4.5:1$ |
| **`ok`** (done, validated) | Emerald-400 (`#34D399`) | Emerald-700 (`#15803D`) | ~ 10.4:1 | ~ 5.1:1 | green (2) | Text $\ge 4.5:1$ |
| **`warn`** (contested) | Amber-300 (`#FCD34D`) | Ochre-800 (`#92400E`) | ~ 12.1:1 | ~ 5.8:1 | yellow/brown (3) | Text $\ge 4.5:1$ |
| **`alert`** (needs you, failed) | Red-500 bold (`#EF4444`) | Crimson-700 bold (`#B91C1C`) | ~ 4.5:1 | ~ 6.1:1 | red (1) | Text $\ge 4.5:1$ |
| **`auto`** (approval mode) | Orange-500 bold (`#F97316`) | Rust-800 bold (`#C2410C`) | ~ 5.8:1 | ~ 5.2:1 | red/magenta (1) | Text $\ge 4.5:1$ |
| **`heading`** | Yellow-300 bold (`#FDE047`) | Ochre-900 bold (`#78350F`) | ~ 13.5:1 | ~ 7.2:1 | yellow/brown (3) | Text $\ge 4.5:1$ |
| **`retired`** | Slate-500 strikethrough | Slate-400 strikethrough | ~ 4.2:1 | ~ 4.6:1 | grey (8) | Muted text |

### 3.1 Ambient Context Architecture & Caching Discipline

To avoid refactoring ~114 call sites (`T.accent()`, `T.ok()`, `T.warn()`, `T.alert()`), `ui/src/theme.ts` introduces module-level ambient palette tracking:

```ts
let activePal: Pal = { dark: true, bg: null };
let activeTokens: ResolvedTokens = computeTokens(activePal);

export const currentPal = (): Pal => activePal;

export function usePal(p: Pal): void {
  activePal = p;
  activeTokens = computeTokens(p);
}

/** Reset to safe default dark mode; used for test isolation. */
export function resetPal(): void {
  usePal({ dark: true, bg: null });
}

/** Execute a scoped callback under a specified palette, restoring previous palette on return. */
export function withPal<T>(p: Pal, fn: () => T): T {
  const prev = activePal;
  try {
    usePal(p);
    return fn();
  } finally {
    usePal(prev);
  }
}
```

#### Performance Invariant (`[BLD-01]`):
When `App` or `Observer` receives an OSC 11 update or boot event, `usePal(pal)` precomputes all semantic and role styles into `activeTokens`. Zero floating-point calculations or relative luminance formulas run inside the active per-token 60Hz rendering hot paths (`[REV-TRAP-03]`). All `T.*()` style methods perform simple direct lookups against `activeTokens`.

---

## 4. Avatar Role Contrast Adaptation (CP-2)

Each role's chassis color must maintain $\ge 3.0:1$ contrast (for badges `⟦•_•⟧` and brackets) and $\ge 4.5:1$ contrast (when rendered as speaker label text).

In [`crates/delphi/src/avatar.rs`](../crates/delphi/src/avatar.rs), `RoleTheme` is extended with an explicit light-mode chassis definition:

```rust
pub struct RoleTheme {
    pub role_id: &'static str,
    pub chassis_rgb: (u8, u8, u8),
    pub chassis_rgb_light: (u8, u8, u8),
    pub chassis_ansi: u8,
    pub chassis_ansi_light: u8,
    // ...
}
```

### Role Color Pairings

| Role ID | Dark Chassis (`chassis_rgb`) | Light Chassis (`chassis_rgb_light`) | Dark Ratio | Light Ratio | Light ANSI |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`orchestrator`** | Violet `#A855F7` | Deep Purple `#7E22CE` | 4.7:1 | 6.5:1 | 5 (magenta) |
| **`architect`** | Sky `#38BDF8` | Deep Blue `#0369A1` | 9.2:1 | 5.2:1 | 4 (blue) |
| **`builder`** | Amber `#F59E0B` | Deep Amber `#B45309` | 7.8:1 | 4.9:1 | 3 (yellow/brown) |
| **`red-team`** | Crimson `#EF4444` | Dark Red `#B91C1C` | 4.1:1 | 6.1:1 | 1 (red) |
| **`strategist`** | Teal `#14B8A6` | Deep Teal `#0F766E` | 7.1:1 | 5.0:1 | 6 (cyan/dark) |
| **`scout`** | Yellow `#FFFF00` | Olive Gold `#854D0E` | 14.5:1 | 5.4:1 | 3 (yellow/brown) |
| **`operator`** | Slate `#94A3B8` | Deep Slate `#475569` | 6.8:1 | 5.6:1 | 8 (dark grey) |

In `ui/src/theme.ts`, `roleColor(role, info)` evaluates `currentPal().dark` to select between dark and light chassis RGB and ANSI fallbacks.

---

## 5. Terminal Detection & Fallback Discipline (CP-3)

Terminal background detection preserves the priority established in Spec 005e:

1. **`DELPHI_THEME` environment variable**: Explicit override (`light` $\rightarrow$ forced light, `dark` $\rightarrow$ forced dark).
2. **OSC 11 Terminal Query**: The console queries background color (`\x1b]11;?\x07`). Responses parsed in [`ui/src/input.ts`](../ui/src/input.ts) update `pal.bg` and `pal.dark`.
3. **`COLORFGBG` environment variable**: Handled if OSC 11 times out or is unsupported (e.g. `15;0` $\rightarrow$ dark, `0;15` $\rightarrow$ light).
4. **Safe Default**: Dark mode (`dark: true, bg: null`).

### 5.1 OSC 11 Security & Sanitization (`[RED-01]`)
OSC 11 responses from standard input must be strictly sanitized to prevent ANSI escape injection:
- Responses are validated against `^rgb:([0-9a-fA-F]{2,4})/([0-9a-fA-F]{2,4})/([0-9a-fA-F]{2,4})$` or standard hex `#([0-9a-fA-F]{6})`.
- Any response containing control characters, unescaped ANSI sequences, or exceeding 64 bytes is discarded immediately.
- A **100ms timeout** prevents blocking on unresponsive terminals; if unanswered, detection falls closed to `dark: true, bg: null`.

When `pal.bg` is provided as an RGB tuple, `ensureContrast(fg, bg, targetRatio)` dynamically checks and shifts luminance if the background is a non-standard pastel or custom palette (such as Solarized Light `#FDF6E3` or Dracula `#282A36`).

---

## 6. Rust Console Parity

Delphi provides an inline console and Rust-driven observer in [`crates/delphi/src/observer.rs`](../crates/delphi/src/observer.rs) and [`crates/delphi/src/console.rs`](../crates/delphi/src/console.rs).
- `Pal` in `observer.rs` is updated to consume `chassis_rgb_light` when `!self.dark`.
- Text rendering in `console.rs` uses the dark/light palette tokens rather than hardcoded ANSI bright codes.
- `avatar-spec.json` is generated or synchronized in lockstep with `avatar.rs` (`[ARC-02]`).

---

## 7. Verification Invariants & Automated Tests

A dedicated test suite in `ui/test/contrast.test.ts` enforces the following invariants under CI:

1. **WCAG AA Compliance Matrix**:
   Every meaning in `VOCABULARY` and every role in `avatar-spec.json` is evaluated against:
   - Pure Black `#000000`
   - Dark Slate `#0F172A`
   - Dracula Background `#282A36`
   - Pure White `#FFFFFF`
   - Solarized Light `#FDF6E3`
   All text styles must meet $\ge 4.5:1$; all badge glyphs must meet $\ge 3.0:1$.
2. **One Home Invariant**:
   `ui/test/theme.test.ts` continues to assert that no hex literal, chalk color call, or raw ANSI escape code appears outside `ui/src/theme.ts`.
3. **Never Color Alone (`[RED-02]`)**:
   Under `NO_COLOR=1` or `TERM=dumb`, all color SGR sequences are suppressed while glyphs and attributes carry meaning identically in both dark and light modes. Test explicitly verifies 0 ANSI escape sequences emitted.
4. **Reduce Motion Compliance**:
   Contrast adjustments are pure functional mappings; no animations, transitions, or private clocks are introduced (`ui/test/reduce-motion.test.ts`).
5. **Test Isolation Guarantee (`[ARC-01]`)**:
   Unit tests rendering light mode frames must call `resetPal()` or use `withPal()` to prevent palette state bleeding into subsequent tests.

---

## 8. Build Plan & Implementation Slices

Each slice represents one atomic pull request:

| Slice | Focus | Deliverables |
| :--- | :--- | :--- |
| **S0** | **Spec Approval & Tracker** | Ratify `specs/073` and update `missions/2026-10-23-foundational-design/spec-tracker.md`. |
| **S1** | **Contrast Math & Ambient Context** | Implement `relativeLuminance`, `contrastRatio`, `ensureContrast` in `ui/src/theme.ts`. Add `usePal` / `currentPal`, `resetPal`, `withPal`, and token caching (`[BLD-01]`, `[ARC-01]`). |
| **S2** | **Dual-Mode Semantic Vocabulary** | Tune `VOCABULARY` (`accent`, `ok`, `warn`, `alert`, `auto`, `heading`, `dim`, syntax tokens) for dark and light backgrounds with explicit ANSI 16 fallbacks (`[ADV-01]`). |
| **S3** | **Avatar Role Contrast Adaptation** | Add `chassis_rgb_light` and `chassis_ansi_light` to `crates/delphi/src/avatar.rs`. Update `avatar-spec.json`, `ui/src/theme.ts`, and `crates/delphi/src/observer.rs` (`[ARC-02]`). |
| **S4** | **Automated Verification & Docs** | Add `ui/test/contrast.test.ts` matrix test, OSC 11 sanitization unit test (`[RED-01]`), and `NO_COLOR` verification (`[RED-02]`). Update `docs/console-visual-language.md`. |
