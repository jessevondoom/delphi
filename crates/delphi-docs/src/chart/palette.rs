//! The chart palette (`specs/014` §8.2). Every value here is the validated reference palette of the dataviz method
//! (OKLab-based adjacent-pair CVD separation, normal-vision floor, lightness band, chroma floor), not a taste choice.
//! Validation on 2026-10-25 with the method's `validate_palette.js`, light surface `#fcfcfb`, dark `#1a1a19`:
//!
//! | run                          | CVD ΔE (≥8 target) | normal-vision ΔE (≥15 floor) | result |
//! |------------------------------|--------------------|------------------------------|--------|
//! | light, 8 slots, adjacent     | 9.1                | 19.6                         | pass; contrast warn on slots 3, 4, 5 |
//! | dark, 8 slots, adjacent      | 8.4                | 19.3                         | pass   |
//! | light, first 3, all pairs    | 9.2                | 24.0                         | pass   |
//! | dark, first 3, all pairs     | 9.4                | 20.9                         | pass   |
//!
//! The consequences are enforced in code, not left to the reader: forms where any two marks can touch (scatter)
//! carry at most three series; every other form at most four; and the light-mode slots under 3:1 contrast (aqua,
//! yellow, magenta) are the "relief required" set, so charts that use them put values where they fit.

/// Categorical series colours, in the fixed order of the method (never cycled, never generated).
pub const SERIES_LIGHT: [&str; 8] = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"];
pub const SERIES_DARK: [&str; 8] = ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"];

pub const SURFACE_LIGHT: &str = "#fcfcfb";
pub const SURFACE_DARK: &str = "#1a1a19";
pub const INK_LIGHT: [&str; 3] = ["#0b0b0b", "#52514e", "#898781"]; // primary, secondary, muted
pub const INK_DARK: [&str; 3] = ["#f0efec", "#c3c2b7", "#898781"];
pub const GRID_LIGHT: &str = "#e1e0d9";
pub const GRID_DARK: &str = "#2c2c2a";
pub const AXIS_LIGHT: &str = "#c3c2b7";
pub const AXIS_DARK: &str = "#383835";

/// Most series a chart may carry: three where any two marks can sit side by side (the first three slots validate
/// all-pairs in both modes), four elsewhere (the fourth puts yellow beside orange, so direct labels are mandatory).
pub const MAX_SERIES: usize = 4;
pub const MAX_SERIES_ALL_PAIRS: usize = 3;
/// A scatter plot cannot print values on its points, so it uses only the first two slots, which are over 3:1 on the
/// light surface and need no relief.
pub const MAX_SERIES_SCATTER: usize = 2;
/// Light-mode slots (0-based) below 3:1 contrast on the surface. A chart that uses them must show values.
pub const RELIEF_SLOTS_LIGHT: [usize; 3] = [2, 3, 4];

fn channel(c: u8) -> f64 {
    let v = c as f64 / 255.0;
    if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
    (p(0), p(2), p(4))
}

/// WCAG 2 relative luminance.
pub fn luminance(hex: &str) -> f64 {
    let (r, g, b) = rgb(hex);
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// WCAG 2 contrast ratio between two colours.
pub fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_matches_the_published_figures() {
        assert!((contrast("#000000", "#ffffff") - 21.0).abs() < 1e-9);
        // the reference palette's own numbers, from the method's validator
        let c = |hex: &str| (contrast(hex, SURFACE_LIGHT) * 100.0).round() / 100.0;
        assert_eq!((c("#1baf7a"), c("#eda100"), c("#e87ba4")), (2.74, 2.11, 2.62));
    }

    #[test]
    fn the_relief_set_is_exactly_the_slots_under_three_to_one_in_light_mode() {
        let under: Vec<usize> = (0..8).filter(|&i| contrast(SERIES_LIGHT[i], SURFACE_LIGHT) < 3.0).collect();
        assert_eq!(under, RELIEF_SLOTS_LIGHT.to_vec());
        for (i, c) in SERIES_DARK.iter().enumerate() {
            assert!(contrast(c, SURFACE_DARK) >= 3.0, "dark slot {} is under 3:1", i + 1);
        }
    }

    #[test]
    fn text_tokens_are_readable_on_their_surface() {
        for (surface, ink) in [(SURFACE_LIGHT, INK_LIGHT), (SURFACE_DARK, INK_DARK)] {
            assert!(contrast(ink[0], surface) >= 7.0, "primary ink on {}", surface);
            assert!(contrast(ink[1], surface) >= 4.5, "secondary ink on {}", surface);
            assert!(contrast(ink[2], surface) >= 3.0, "muted ink (axis labels) on {}", surface);
        }
        // gridlines and axes are meant to recede: visible, never loud
        assert!(contrast(GRID_LIGHT, SURFACE_LIGHT) < 1.4 && contrast(GRID_DARK, SURFACE_DARK) < 1.4);
    }

    #[test]
    fn the_series_are_distinct_hues_in_the_documented_order() {
        assert_eq!(SERIES_LIGHT.len(), 8);
        let mut seen = std::collections::HashSet::new();
        assert!(SERIES_LIGHT.iter().chain(SERIES_DARK.iter()).filter(|h| **h != "#008300").all(|h| seen.insert(*h)), "no colour repeated except the mode-invariant green");
        const { assert!(MAX_SERIES_SCATTER <= MAX_SERIES_ALL_PAIRS && MAX_SERIES_ALL_PAIRS < MAX_SERIES) };
        assert!((0..MAX_SERIES_SCATTER).all(|i| contrast(SERIES_LIGHT[i], SURFACE_LIGHT) >= 3.0), "scatter must not need relief");
    }
}
