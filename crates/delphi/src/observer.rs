//! What the Rust consoles share with the narrator: the view names `delphi observe [view]` takes, the palette and the
//! badge, and the helpers for OSC 8 links.
//!
//! The narrator itself is the full-screen `delphi-ui --observe` window (`specs/005e` §9); the classic Rust Observer
//! that used to live here is retired. `delphi observe --once` prints one frame of the same window.

use crate::avatar;
use delphi_engine::observe::{sanitize, ObserverState};
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// The narrator's views, in order (`specs/005e` §9.2). There is no spend view: the narrator shows no money.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Council,
    Epic,
    Agents,
    Archives,
    Changes,
}

impl View {
    /// The name `delphi-ui --observe` and `delphi observe` take.
    pub fn name(self) -> &'static str {
        match self {
            View::Dashboard => "dashboard",
            View::Council => "council",
            View::Epic => "epic",
            View::Agents => "agents",
            View::Archives => "archives",
            View::Changes => "changes",
        }
    }

    pub fn parse(s: &str) -> Option<View> {
        match s.to_lowercase().as_str() {
            "dashboard" | "now" | "ops" | "monitor" => Some(View::Dashboard),
            "council" | "chamber" => Some(View::Council),
            "epic" | "forge" | "diffs" => Some(View::Epic), // the forge view folded into the epic pane (`005g`)
            "agents" | "roster" => Some(View::Agents),
            "archives" | "kb" => Some(View::Archives),
            "changes" | "changelog" | "log" | "thinking" | "reasoning" => Some(View::Changes), // the reasoning pane was dropped for the change log
            _ => None,
        }
    }
}

/// A piece of text on screen that should be a clickable link once the frame is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub url: String,
}

/// Dark unless the terminal says otherwise (`DELPHI_THEME=light`, or `COLORFGBG` ending in a light background).
pub fn detect_dark() -> bool {
    match std::env::var("DELPHI_THEME").ok().as_deref() {
        Some("light") => return false,
        Some("dark") => return true,
        _ => {}
    }
    match std::env::var("COLORFGBG").ok().and_then(|v| v.rsplit(';').next().and_then(|n| n.parse::<u8>().ok())) {
        Some(bg) => !(bg == 7 || bg >= 9),
        None => true,
    }
}



/// `file://` URL with everything that could end an OSC 8 sequence or a URL escaped.
pub fn file_url(p: &std::path::Path) -> String {
    let mut out = String::from("file://");
    for b in p.to_string_lossy().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            other => out.push_str(&format!("%{:02X}", other)),
        }
    }
    out
}

/// The escape sequences that turn already-drawn text into links: save the cursor, move to the text, print it
/// inside an OSC 8 pair, restore the cursor.
pub fn osc8(links: &[Link]) -> String {
    links.iter().map(|l| format!("\x1b7\x1b[{};{}H\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\\x1b8", l.y + 1, l.x + 1, l.url, l.text)).collect()
}

// ── palette, hairlines, avatars ─────────────────────────────────────────────

#[derive(Clone, Copy)]
pub(crate) struct Pal {
    pub(crate) dark: bool,
}

impl Pal {
    pub(crate) fn text(self) -> Color {
        if self.dark { Color::Rgb(226, 232, 240) } else { Color::Rgb(15, 23, 42) }
    }
    pub(crate) fn dim(self) -> Color {
        Color::Rgb(100, 116, 139)
    }
    pub(crate) fn rule(self) -> Color {
        if self.dark { Color::Rgb(51, 65, 85) } else { Color::Rgb(203, 213, 225) }
    }
    /// Eyes on a badge: bright on dark terminals, deep slate on light ones (`skills/tui-design` §2.1).
    pub(crate) fn eye(self) -> Color {
        if self.dark { Color::Rgb(255, 255, 255) } else { Color::Rgb(15, 23, 42) }
    }
}

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

pub(crate) fn role_color(role: &str) -> Color {
    avatar::from_canonical_id(role).map(|r| rgb(avatar::get_theme(r).chassis_rgb)).unwrap_or(Color::Gray)
}

fn face_for(role: &str, state: &str) -> &'static str {
    let t = avatar::from_canonical_id(role).map(avatar::get_theme);
    match (state, role) {
        ("waiting on you", _) => "x?x",
        ("escalated", _) => "x_x",
        ("running", "scout") => "···",
        ("running", _) => "•─•",
        (_, _) => t.map(|t| t.badge_face).unwrap_or("•_•"),
    }
}

/// `⟦•─•⟧` in the role's colour with bright eyes; the red-team's alert badge has gold eyes and crimson brackets.
pub(crate) fn badge(role: &str, state: &str, pal: Pal) -> Vec<Span<'static>> {
    let chassis = role_color(role);
    let alert = role == "red-team" && state == "waiting on you";
    let eye = if alert { Color::Rgb(250, 204, 21) } else { pal.eye() };
    let mut out = vec![Span::styled("⟦", Style::default().fg(chassis))];
    for ch in face_for(role, state).chars() {
        let c = if matches!(ch, '─' | '_' | '-') { chassis } else { eye };
        out.push(Span::styled(ch.to_string(), Style::default().fg(c)));
    }
    out.push(Span::styled("⟧", Style::default().fg(chassis)));
    out
}


pub(crate) fn kilo(n: u64) -> String {
    if n >= 10_000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}


pub(crate) fn state_color(state: &str) -> Color {
    match state {
        "done" | "green" | "merged" => Color::Green,
        "running" => Color::Cyan,
        "escalated" | "failed" | "red" | "refused" => Color::Red,
        "waiting on you" => Color::Yellow,
        _ => Color::DarkGray,
    }
}

/// Roles in the order people think of them: the orchestrator first, then the rest as the roster lists them.
pub(crate) fn ordered(st: &ObserverState) -> Vec<(&String, &delphi_engine::observe::AgentRow)> {
    const ORDER: [&str; 7] = ["orchestrator", "architect", "builder", "red-team", "strategist", "scout", "investigator"];
    let mut v: Vec<_> = st.agents.iter().collect();
    v.sort_by_key(|(r, _)| ORDER.iter().position(|o| o == &r.as_str()).unwrap_or(ORDER.len()));
    v
}

pub(crate) fn clip(s: &str, n: usize) -> String {
    let s = sanitize(s).replace('\n', " ");
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

/// `── title ────────────` across `width`: a section break without a box.
pub(crate) fn hairline(title: &str, width: u16, pal: Pal) -> Line<'static> {
    let head = if title.is_empty() { String::new() } else { format!("── {} ", title) };
    let fill = (width as usize).saturating_sub(head.chars().count());
    Line::from(Span::styled(format!("{}{}", head, "─".repeat(fill)), Style::default().fg(pal.rule())))
}


/// The text of a drawn buffer, one line per row (for tests and `--once`).
#[cfg(test)]
pub fn buffer_text(buf: &ratatui::buffer::Buffer) -> String {
    let area = buf.area;
    (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn views_parse_by_name_and_the_retired_ones_do_not() {
        assert_eq!(View::parse("Council"), Some(View::Council));
        assert_eq!(View::parse("roster"), Some(View::Agents));
        assert_eq!(View::parse("kb"), Some(View::Archives));
        for gone in ["spend", "telemetry", "dag", "1", "5"] {
            assert_eq!(View::parse(gone), None, "{}", gone);
        }
        assert_eq!(View::parse("forge"), Some(View::Epic), "the forge view became part of the epic pane");
    }

    #[test]
    fn badges_follow_the_eye_contrast_rule_in_dark_and_light() {
        let colours = |role: &str, state: &str, dark: bool| badge(role, state, Pal { dark }).into_iter().map(|s| (s.content.to_string(), s.style.fg)).collect::<Vec<_>>();
        let dark = colours("orchestrator", "idle", true);
        assert_eq!(dark.iter().map(|(c, _)| c.as_str()).collect::<String>(), "⟦•─•⟧");
        assert_eq!(dark[1].1, Some(Color::Rgb(255, 255, 255)), "eyes are bright white on dark");
        assert_eq!(dark[2].1, Some(Color::Rgb(168, 85, 247)), "the mouth keeps the role colour");
        assert_eq!(colours("orchestrator", "idle", false)[1].1, Some(Color::Rgb(15, 23, 42)), "eyes are deep slate on light");
        let alert = colours("red-team", "waiting on you", true);
        assert_eq!(alert.iter().map(|(c, _)| c.as_str()).collect::<String>(), "⟦x?x⟧");
        assert_eq!((alert[1].1, alert[0].1), (Some(Color::Rgb(250, 204, 21)), Some(Color::Rgb(239, 68, 68))), "gold eyes, crimson brackets");
        // Every badge is exactly five display columns.
        for role in ["orchestrator", "builder", "architect", "red-team", "strategist", "scout"] {
            for st in ["idle", "running", "waiting on you", "escalated"] {
                assert_eq!(colours(role, st, true).iter().map(|(c, _)| c.chars().count()).sum::<usize>(), 5, "{} {}", role, st);
            }
        }
    }

    #[test]
    fn links_are_placed_over_drawn_text_and_paths_are_escaped() {
        let seq = osc8(&[Link { x: 2, y: 4, text: "REVIEW.md".into(), url: "file:///ws/REVIEW.md".into() }]);
        assert!(seq.contains("\x1b[5;3H") && seq.contains("\x1b]8;;file:///ws/REVIEW.md\x1b\\REVIEW.md\x1b]8;;\x1b\\") && seq.starts_with("\x1b7") && seq.ends_with("\x1b8"));
        assert_eq!(file_url(std::path::Path::new("/a/b c/é.md")), "file:///a/b%20c/%C3%A9.md");
    }

    #[test]
    fn the_palette_follows_the_terminal() {
        // SAFETY: this is the only test that touches these variables.
        unsafe {
            std::env::remove_var("DELPHI_THEME");
            std::env::set_var("COLORFGBG", "0;15");
            assert!(!detect_dark(), "a white background is light");
            std::env::set_var("COLORFGBG", "15;0");
            assert!(detect_dark());
            std::env::set_var("DELPHI_THEME", "light");
            assert!(!detect_dark(), "an explicit choice wins");
            std::env::remove_var("DELPHI_THEME");
            std::env::remove_var("COLORFGBG");
            assert!(detect_dark(), "dark by default");
        }
        let (d, l) = (Pal { dark: true }, Pal { dark: false });
        assert_ne!(d.text(), l.text());
        assert_ne!(d.eye(), l.eye());
    }
}
