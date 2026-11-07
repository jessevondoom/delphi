//! The ASCII fallback for badges (`specs/087` §6.2).
//!
//! A badge is `⟦` + three cells + `⟧`. Some terminals and fonts draw its non-ASCII characters at the wrong width or not at
//! all, so the operator can ask for a plain-ASCII badge of the same five columns. This is the one table: the Rust
//! renderers, the outside identity strings and (through the data `ui_spec` sends) the console all read it, and the
//! name-pool lint checks that no two faces collapse onto the same ASCII form (`crates/delphi-engine/src/agent_pool.rs`).
//!
//! The setting is `DELPHI_ASCII_BADGES=1` everywhere, and `[mcp] badge_ascii` for what a host is sent, because the process
//! that builds an identity for a host does not always inherit the operator's shell.

/// The ASCII stand-in for each non-ASCII glyph a face uses, chosen so that every distinct face keeps a distinct ASCII form
/// (no two core roles collapse onto one, except the twins, who share a face to begin with). `…` and `●` are the activity
/// faces' (thinking, surprise), not any role's.
pub const FACE_GLYPHS: [(char, char); 18] = [
    ('•', '0'),
    ('─', '-'),
    ('·', '.'),
    ('¬', '~'),
    ('‿', 'v'),
    ('ᴥ', 'Y'),
    ('ω', 'W'),
    ('◉', '@'),
    ('◔', 'c'),
    ('■', '#'),
    ('▪', '#'),
    ('☆', '*'),
    ('✦', '+'),
    ('×', 'x'),
    ('≈', '%'),
    ('°', '\''),
    ('…', '.'),
    ('●', 'O'),
];

/// The badge's brackets. They are not part of a face (a theme's `ascii_glyphs` does not list them).
pub const BRACKET_GLYPHS: [(char, char); 2] = [('⟦', '['), ('⟧', ']')];

/// The environment switch. Same convention as `DELPHI_REDUCE_MOTION`: set to `1`.
pub const ENV: &str = "DELPHI_ASCII_BADGES";

/// Whether the environment asks for ASCII badges.
pub fn requested_by_env() -> bool {
    std::env::var(ENV).map(|v| v == "1").unwrap_or(false)
}

/// `s` with every non-ASCII character replaced through the tables (faces and brackets), so a badge or a face comes out as
/// ASCII of the same width. A character the tables do not know becomes `?`: the result is always ASCII.
pub fn ascii_cells(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                c
            } else {
                FACE_GLYPHS.iter().chain(BRACKET_GLYPHS.iter()).find(|(g, _)| *g == c).map(|(_, a)| *a).unwrap_or('?')
            }
        })
        .collect()
}

/// Free text (a one-off agent's label, `specs/087` §4.3 item 4) as ASCII: every non-ASCII character becomes `?`. The face
/// tables above are for badges; a label can hold any character, and a wide or ambiguous-width one must not move the columns
/// of a row when the operator asked for ASCII, so it is not run through them (a `•` in a label would otherwise read as `0`).
pub fn ascii_text(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii() { c } else { '?' }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn free_text_becomes_question_marks_not_face_glyphs() {
        assert_eq!(ascii_text("flaky-test-hunter"), "flaky-test-hunter");
        assert_eq!(ascii_text("測試•x"), "???x");
        assert_eq!(ascii_cells("•"), "0", "the badge table would have turned the bullet into a digit");
    }

    #[test]
    fn a_badge_keeps_five_columns_and_becomes_ascii() {
        assert_eq!(ascii_cells("⟦•─•⟧"), "[0-0]");
        assert_eq!(ascii_cells("⟦¬_¬⟧"), "[~_~]");
        assert_eq!(ascii_cells("⟦•‿•⟧"), "[0v0]");
        for b in ["⟦•─•⟧", "⟦·w·⟧", "⟦×_×⟧", "⟦…  ⟧", "⟦●_●⟧"] {
            let a = ascii_cells(b);
            assert!(a.is_ascii() && a.chars().count() == b.chars().count(), "{b} -> {a}");
        }
    }

    #[test]
    fn ascii_stays_as_it_is_and_an_unknown_glyph_becomes_a_question_mark() {
        assert_eq!(ascii_cells("-_-"), "-_-");
        assert_eq!(ascii_cells(">=<"), ">=<");
        assert_eq!(ascii_cells("♥_♥"), "?_?");
    }

    #[test]
    fn the_table_maps_each_glyph_once_to_a_printable_ascii_character() {
        let mut seen = std::collections::HashSet::new();
        for (g, a) in FACE_GLYPHS.iter().chain(BRACKET_GLYPHS.iter()) {
            assert!(!g.is_ascii() && a.is_ascii_graphic(), "{g} -> {a}");
            assert!(seen.insert(*g), "{g} is listed twice");
        }
    }
}
