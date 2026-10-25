//! `delphi observe [view]` (also `delphi narrator`): the read-only window (`specs/005b` §4, `skills/tui-design`).
//!
//! One full-window view at a time, no tiled panes and no enclosing boxes: structure comes from whitespace and
//! hairline rules. `Shift+Left`/`Shift+Right` cycle the views: Council Chamber, Epic, Forge, Agents (the
//! mascot roster), Spend, and Archives (knowledge-base search). It attaches with a narrator-scope token, so
//! the daemon refuses anything that would start work or approve anything, and a window that is killed and
//! reopened rebuilds the same screen from the daemon's snapshot. Everything a model wrote is stripped of
//! control sequences before it is drawn, and file paths are emitted as OSC 8 hyperlinks after each draw.
//!
//! The drawing code only reads [`App`], so tests render it into an in-memory buffer.

use crate::avatar;
use delphi_engine::observe::{sanitize, Build, Council, ObserverState};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Council,
    Epic,
    Forge,
    Agents,
    Spend,
    Archives,
}

pub const VIEWS: [View; 6] = [View::Council, View::Epic, View::Forge, View::Agents, View::Spend, View::Archives];

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Council => "COUNCIL CHAMBER",
            View::Epic => "EPIC DAG",
            View::Forge => "FORGE DIFFS",
            View::Agents => "MASCOT ROSTER",
            View::Spend => "SPEND TELEMETRY",
            View::Archives => "ARCHIVES",
        }
    }

    fn short(self) -> &'static str {
        match self {
            View::Council => "Council",
            View::Epic => "Epic DAG",
            View::Forge => "Forge Diffs",
            View::Agents => "Agents",
            View::Spend => "Spend",
            View::Archives => "Archives",
        }
    }

    /// The name `delphi-ui --observe` and `delphi observe` take.
    pub fn name(self) -> &'static str {
        match self {
            View::Council => "council",
            View::Epic => "epic",
            View::Forge => "forge",
            View::Agents => "agents",
            View::Spend => "spend",
            View::Archives => "archives",
        }
    }

    pub fn parse(s: &str) -> Option<View> {
        match s.to_lowercase().as_str() {
            "council" | "chamber" | "1" => Some(View::Council),
            "epic" | "dag" | "2" => Some(View::Epic),
            "forge" | "diffs" | "3" => Some(View::Forge),
            "agents" | "roster" | "4" => Some(View::Agents),
            "spend" | "telemetry" | "5" => Some(View::Spend),
            "archives" | "kb" | "6" => Some(View::Archives),
            _ => None,
        }
    }

    fn index(self) -> usize {
        VIEWS.iter().position(|v| *v == self).unwrap_or(0)
    }

    fn step(self, delta: isize) -> View {
        VIEWS[(self.index() as isize + delta).rem_euclid(VIEWS.len() as isize) as usize]
    }
}

/// What a key press asks the loop to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    /// Run a knowledge-base search.
    Search(String),
    /// Fetch the live diff for the selected build.
    RefreshDiff,
}

#[derive(Default)]
pub struct Search {
    pub editing: bool,
    pub query: String,
    /// Each hit is the daemon's JSON, with a `url` added when the file's place on disk is known.
    pub hits: Vec<Value>,
    pub note: String,
}

/// A piece of text on screen that should be a clickable link once the frame is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub url: String,
}

pub struct App {
    pub state: ObserverState,
    pub view: View,
    pub names: HashMap<String, String>,
    pub mission: String,
    pub cap_usd: f64,
    pub pending_proposals: u64,
    pub connected: bool,
    /// Which council or build is selected (`[` and `]`).
    pub pick: usize,
    pub scroll: u16,
    pub selected_node: usize,
    pub search: Search,
    /// The last `observe.diff` answer for the selected build.
    pub diff: Option<Value>,
    /// Dark or light terminal: chooses text and eye colours. The background is never painted.
    pub dark: bool,
    /// The workspace directory, for turning logical paths into file links.
    pub ws_dir: Option<PathBuf>,
    /// Links found while drawing, written as OSC 8 after the frame.
    pub links: RefCell<Vec<Link>>,
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

impl App {
    pub fn new(roles: &[String], names: HashMap<String, String>) -> App {
        App { state: ObserverState::new(roles), view: View::Epic, names, mission: String::new(), cap_usd: 0.0, pending_proposals: 0, connected: true, pick: 0, scroll: 0, selected_node: 0, search: Search::default(), diff: None, dark: true, ws_dir: None, links: RefCell::new(Vec::new()) }
    }

    fn name(&self, role: &str) -> String {
        self.names.get(role).cloned().unwrap_or_else(|| role.to_string())
    }

    fn go(&mut self, v: View) {
        if self.view != v {
            self.view = v;
            self.scroll = 0;
            self.pick = usize::MAX; // newest
        }
    }

    pub fn on_key(&mut self, code: ratatui::crossterm::event::KeyCode, mods: ratatui::crossterm::event::KeyModifiers) -> Action {
        use ratatui::crossterm::event::{KeyCode as K, KeyModifiers as M};
        if self.search.editing {
            match code {
                K::Esc => self.search.editing = false,
                K::Enter => {
                    self.search.editing = false;
                    if !self.search.query.trim().is_empty() {
                        return Action::Search(self.search.query.clone());
                    }
                }
                K::Backspace => {
                    self.search.query.pop();
                }
                K::Char(c) if !mods.contains(M::CONTROL) && self.search.query.chars().count() < 200 => self.search.query.push(c),
                _ => {}
            }
            return Action::None;
        }
        match code {
            K::Char('c') if mods.contains(M::CONTROL) => return Action::Quit,
            K::Char('q') | K::Esc => return Action::Quit,
            K::Right if mods.contains(M::SHIFT) => self.go(self.view.step(1)),
            K::Left if mods.contains(M::SHIFT) => self.go(self.view.step(-1)),
            K::Tab => self.go(self.view.step(1)),
            K::BackTab => self.go(self.view.step(-1)),
            K::Char(c @ '1'..='6') => self.go(VIEWS[c as usize - '1' as usize]),
            K::Char('/') if self.view == View::Archives => self.search.editing = true,
            K::Char('[') => self.pick = self.pick.min(usize::MAX - 1).saturating_sub(1),
            K::Char(']') => self.pick = self.pick.saturating_add(1),
            K::Down | K::Char('j') => {
                if self.view == View::Epic {
                    self.selected_node = self.selected_node.saturating_add(1);
                } else {
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
            K::Up | K::Char('k') => {
                if self.view == View::Epic {
                    self.selected_node = self.selected_node.saturating_sub(1);
                } else {
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            K::PageDown | K::Char(' ') => self.scroll = self.scroll.saturating_add(10),
            K::PageUp => self.scroll = self.scroll.saturating_sub(10),
            K::Home => self.scroll = 0,
            K::Char('r') if self.view == View::Forge => return Action::RefreshDiff,
            _ => {}
        }
        Action::None
    }

    fn selected_council(&self) -> Option<&Council> {
        let n = self.state.councils.len();
        (n > 0).then(|| &self.state.councils[self.pick.min(n - 1)])
    }

    pub fn selected_build(&self) -> Option<&Build> {
        let n = self.state.builds.len();
        (n > 0).then(|| &self.state.builds[self.pick.min(n - 1)])
    }

    /// A logical workspace path as a `file://` URL, if it can be placed on disk.
    pub fn url_for(&self, logical: &str) -> Option<String> {
        let ws = self.ws_dir.as_ref()?;
        let rel = logical.split('#').next().unwrap_or(logical);
        let phys = if rel.starts_with("missions/") || rel.starts_with("memory/") { ws.join("local").join(rel) } else { delphi_engine::kb::sources(ws).into_iter().find(|s| s.rel_path == rel)?.path };
        Some(file_url(&phys))
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

/// The 3-line solid mascot, seven columns wide (`specs/005a`): crown in the bottom half of row one, the face as
/// a painted row, a flush chin.
fn mascot(role: &str, state: &str) -> [Line<'static>; 3] {
    let Some(r) = avatar::from_canonical_id(role) else { return [Line::from("       "), Line::from("       "), Line::from("       ")] };
    let t = avatar::get_theme(r);
    let crown: Vec<Span> = t
        .crown_slice
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if c != '▄' {
                return Span::raw(" ");
            }
            let colour = if r == avatar::AgentRole::Builder && (1..=5).contains(&i) { (255, 255, 255) } else { t.crown_rgb };
            Span::styled("▄", Style::default().fg(rgb(colour)))
        })
        .collect();
    let face = if r == avatar::AgentRole::Scout {
        match state {
            "waiting on you" => "x ? x".to_string(),
            "running" => "· · ·".to_string(),
            _ => "• • •".to_string(),
        }
    } else if r == avatar::AgentRole::Builder && state != "waiting on you" && state != "escalated" {
        "•───•".to_string()
    } else {
        face_for(role, state).to_string()
    };
    let pad = 7usize.saturating_sub(face.chars().count());
    let text = format!("{}{}{}", " ".repeat(pad / 2), face, " ".repeat(pad - pad / 2));
    [
        Line::from(crown),
        Line::from(Span::styled(text, Style::default().bg(rgb(t.chassis_rgb)).fg(Color::Rgb(15, 23, 42)))),
        Line::from(Span::styled("▀▀▀▀▀▀▀", Style::default().fg(rgb(t.chassis_rgb)))),
    ]
}

pub(crate) fn money(v: f64) -> String {
    if v >= 100.0 { format!("${:.0}", v) } else if v >= 1.0 { format!("${:.2}", v) } else { format!("${:.3}", v) }
}

pub(crate) fn kilo(n: u64) -> String {
    if n >= 10_000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}

fn bar(frac: f64, width: usize) -> String {
    let f = frac.clamp(0.0, 1.0);
    let full = (f * width as f64).round() as usize;
    format!("{}{}", "▓".repeat(full), "░".repeat(width.saturating_sub(full)))
}

fn sparkline(values: &[f64], width: usize) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let recent: Vec<f64> = values.iter().rev().take(width).rev().copied().collect();
    let max = recent.iter().cloned().fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return "▁".repeat(recent.len());
    }
    recent.iter().map(|v| BLOCKS[((v / max) * 7.0).round() as usize]).collect()
}

fn glyph(state: &str) -> &'static str {
    match state {
        "done" | "green" | "merged" => "✔",
        "running" => "●",
        "escalated" | "failed" | "red" | "refused" => "✖",
        "waiting on you" => "►",
        "pending" | "idle" => "○",
        _ => "·",
    }
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

fn dim_line(s: impl Into<String>, pal: Pal) -> Line<'static> {
    Line::styled(s.into(), Style::default().fg(pal.dim()))
}

// ── drawing ─────────────────────────────────────────────────────────────────

impl App {
    fn pal(&self) -> Pal {
        Pal { dark: self.dark }
    }

    pub fn draw(&self, f: &mut Frame) {
        self.links.borrow_mut().clear();
        let area = f.area();
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3), Constraint::Min(3), Constraint::Length(3)]).split(area);
        self.header(f, rows[0]);
        match self.view {
            View::Council => self.council(f, rows[1]),
            View::Epic => self.epic(f, rows[1]),
            View::Forge => self.forge(f, rows[1]),
            View::Agents => self.agents(f, rows[1]),
            View::Spend => self.spend(f, rows[1]),
            View::Archives => self.archives(f, rows[1]),
        }
        self.footer(f, rows[2]);
    }

    fn header(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).split(area);
        let conn = if self.connected { "" } else { "   (disconnected)" };
        f.render_widget(Paragraph::new(Line::from(vec![Span::styled(" DELPHI OBSERVER", Style::default().fg(pal.text()).add_modifier(Modifier::BOLD)), Span::styled(format!("  ·  {}  ·  mission {}{}", self.view.title(), if self.mission.is_empty() { "…" } else { &self.mission }, conn), Style::default().fg(pal.dim()))])), rows[0]);
        // One badge per enabled role, in the viewer's theme names.
        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        for (role, a) in ordered(&self.state) {
            spans.extend(badge(role, &a.state, pal));
            // Idle agents are just a badge and a name; only a state worth noticing is spelled out.
            let text = if a.state == "idle" { format!(" {}   ", self.name(role)) } else { format!(" {} {}   ", self.name(role), a.state) };
            spans.push(Span::styled(text, Style::default().fg(if a.state == "idle" { pal.dim() } else { state_color(&a.state) })));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
        f.render_widget(Paragraph::new(hairline("", area.width, pal)), rows[2]);
    }

    fn footer(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).split(area);
        f.render_widget(Paragraph::new(hairline("", area.width, pal)), rows[0]);
        let i = self.view.index();
        // Bare dots, never bracketed, so they cannot be mistaken for an avatar badge.
        let dots: String = (0..VIEWS.len()).map(|n| if n == i { "●" } else { "○" }).collect::<Vec<_>>().join(" ");
        let left = format!(" {} {}/{}  ·  ← {}  ·  → {}  ·  1-{} jump  ·  q quit", self.view.short(), i + 1, VIEWS.len(), self.view.step(-1).short(), self.view.step(1).short(), VIEWS.len());
        let right = format!("navigate: shift+arrows · {} ", dots);
        let gap = (area.width as usize).saturating_sub(left.chars().count() + right.chars().count()).max(2);
        f.render_widget(Paragraph::new(format!("{}{}{}", left, " ".repeat(gap), right)).style(Style::default().fg(pal.dim())), rows[1]);
        let st = &self.state;
        let frac = if self.cap_usd > 0.0 { st.spent_usd / self.cap_usd } else { 0.0 };
        let colour = if frac >= 0.9 { Color::Red } else if frac >= 0.75 { Color::Yellow } else { Color::Green };
        let line = Line::from(vec![
            Span::styled(format!(" spend {} / {} ", money(st.spent_usd), if self.cap_usd > 0.0 { money(self.cap_usd) } else { "?".into() }), Style::default().fg(pal.text())),
            Span::styled(bar(frac, 12), Style::default().fg(colour)),
            Span::styled(format!(" · tokens {} · cache hit {:.0}% · retries {}{}", kilo(st.tokens.total()), st.cache_hit_ratio() * 100.0, st.retries, if self.pending_proposals > 0 { format!(" · {} proposal(s) await /ratify", self.pending_proposals) } else { String::new() }), Style::default().fg(pal.text())),
        ]);
        f.render_widget(Paragraph::new(line), rows[2]);
    }

    fn council(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let Some(c) = self.selected_council() else {
            f.render_widget(Paragraph::new(vec![Line::from(""), dim_line("  No Council yet.", pal), Line::from(""), dim_line("  Convene one in the console:  /council <proposition>", pal)]), area);
            return;
        };
        let live = matches!(c.phase.as_str(), "round 1" | "round 2");
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(if live { 3 } else { 9 })]).split(area);
        let n = self.state.councils.len();
        let head = vec![
            Line::from(Span::styled(format!(" Council {}/{} · {} · {}{}", self.pick.min(n - 1) + 1, n, c.task_id, c.phase, if c.homogeneous { " · homogeneous (one model family)" } else { "" }), Style::default().fg(pal.text()).add_modifier(Modifier::BOLD))),
            Line::from(Span::styled(format!(" {}", clip(&c.proposition, area.width.saturating_sub(3) as usize)), Style::default().fg(pal.text()))),
        ];
        f.render_widget(Paragraph::new(head), rows[0]);
        if c.seats.is_empty() {
            return;
        }
        // One open column per seat: a coloured name over a hairline, then what it said. No boxes.
        let cols = Layout::default().direction(Direction::Horizontal).constraints(vec![Constraint::Ratio(1, c.seats.len() as u32); c.seats.len()]).spacing(2).split(rows[1]);
        for (seat, col) in c.seats.iter().zip(cols.iter()) {
            let mut lines: Vec<Line> = vec![Line::from(Span::styled(format!("{} ({})", self.name(&seat.role), seat.family), Style::default().fg(role_color(&seat.role)).add_modifier(Modifier::BOLD))), hairline("", col.width, pal)];
            let stance = c.stances.iter().find(|s| s.role == seat.role);
            match stance {
                Some(s) if s.answered => {
                    lines.push(Line::styled("Round 1 (blind)", Style::default().fg(pal.dim())));
                    lines.extend(sanitize(&s.text).lines().map(|l| Line::styled(l.to_string(), Style::default().fg(pal.text()))));
                }
                Some(s) => lines.push(dim_line(format!("abstained: {}", clip(&s.text, 80)), pal)),
                None => {
                    let live_text = self.state.nodes.values().find(|n| n.task_id == c.task_id && n.role == seat.role).and_then(|n| self.state.live.get(&n.id));
                    // The blind round is shown to the operator only: agents never see it.
                    lines.push(dim_line("writing… (blind)", pal));
                    if let Some(t) = live_text {
                        lines.extend(sanitize(t).lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().map(|l| Line::styled(l.to_string(), Style::default().fg(pal.text()))));
                    }
                }
            }
            if let Some(cr) = c.critiques.iter().find(|x| x.role == seat.role) {
                lines.push(Line::from(""));
                lines.push(Line::styled(format!("Round 2 → {}{}", if cr.recommendation.is_empty() { "unreadable" } else { &cr.recommendation }, match cr.concur { Some(true) => " · concurs", Some(false) => " · does not concur", None => "" }), Style::default().fg(pal.text()).add_modifier(Modifier::BOLD)));
                for (sev, title) in &cr.findings {
                    lines.push(Line::styled(format!("{} {}", sev, title), Style::default().fg(if sev == "P0" { Color::Red } else if sev == "P1" { Color::Yellow } else { pal.dim() })));
                }
            }
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((self.scroll, 0)), *col);
        }
        // The verdict: the Orchestrator's synthesis and the daemon-written Minority Report.
        let mut v: Vec<Line> = vec![hairline("Verdict", area.width, pal)];
        let mut link_row = None;
        match c.phase.as_str() {
            "round 1" => v.push(dim_line(" Round 1 is blind: stances appear when every seat has answered.", pal)),
            "round 2" => v.push(dim_line(" Round 2: seats are critiquing each other's stances.", pal)),
            _ => {
                if let Some(why) = &c.escalation {
                    v.push(Line::styled(format!(" Escalated to you: {}", clip(why, 100)), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
                    for (label, seats) in &c.options {
                        v.push(Line::styled(format!("   option: {} — {}", label, seats.join(", ")), Style::default().fg(pal.text())));
                    }
                } else {
                    v.push(Line::styled(" Verdict reached; no deterministic escalation.", Style::default().fg(Color::Green)));
                }
                v.push(Line::styled(" Minority report:", Style::default().fg(pal.text()).add_modifier(Modifier::BOLD)));
                if c.unresolved.is_empty() {
                    v.push(dim_line("   none open", pal));
                }
                for (sev, seat, title) in c.unresolved.iter().take(3) {
                    v.push(Line::styled(format!("   [{}] {}: {}", sev, seat, clip(title, area.width.saturating_sub(16) as usize)), Style::default().fg(if sev == "P0" { Color::Red } else { Color::Yellow })));
                }
                if !c.path.is_empty() {
                    link_row = Some(v.len());
                    v.push(dim_line(format!(" saved: {}", c.path), pal));
                }
            }
        }
        if let (Some(row), Some(url)) = (link_row, self.url_for(&c.path)) {
            self.links.borrow_mut().push(Link { x: rows[2].x + " saved: ".chars().count() as u16, y: rows[2].y + row as u16, text: c.path.clone(), url });
        }
        f.render_widget(Paragraph::new(v), rows[2]);
    }

    fn epic(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(4), Constraint::Length(9)]).split(area);
        let mut lines: Vec<Line> = Vec::new();
        let mut order: Vec<(&delphi_engine::observe::Node, usize)> = Vec::new();
        fn walk<'a>(st: &'a ObserverState, n: &'a delphi_engine::observe::Node, depth: usize, out: &mut Vec<(&'a delphi_engine::observe::Node, usize)>) {
            out.push((n, depth));
            for c in st.children(&n.id) {
                walk(st, c, depth + 1, out);
            }
        }
        for r in self.state.roots() {
            walk(&self.state, r, 0, &mut order);
        }
        let sel = self.selected_node.min(order.len().saturating_sub(1));
        if order.is_empty() {
            lines.push(dim_line("  No agents have run yet in this mission.", pal));
        }
        let mut last_task = String::new();
        for (i, (n, depth)) in order.iter().enumerate() {
            if n.task_id != last_task {
                last_task = n.task_id.clone();
                let t = self.state.tasks.get(&n.task_id);
                lines.push(Line::styled(format!(" {} {}{}", n.task_id, t.map(|t| t.flow.as_str()).unwrap_or(""), t.map(|t| format!(" · {}", clip(&t.prompt, 60))).unwrap_or_default()), Style::default().fg(pal.text()).add_modifier(Modifier::BOLD)));
            }
            let style = if i == sel { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
            lines.push(Line::from(vec![
                Span::raw(format!("  {}", "  ".repeat(*depth))),
                Span::styled(format!("{} ", glyph(&n.state)), Style::default().fg(state_color(&n.state))),
                Span::styled(format!("{:<12}", self.name(&n.role)), style.fg(role_color(&n.role))),
                Span::styled(format!(" {:<10} {:>7} tok  {}", n.state, kilo(n.tokens), money(n.cost_usd)), style.fg(pal.text())),
            ]));
        }
        f.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), rows[0]);
        // The story so far, from events, not from a model.
        let take = rows[1].height.saturating_sub(1) as usize;
        let mut story: Vec<Line> = vec![hairline("Narrative", area.width, pal)];
        story.extend(self.state.narrative.iter().rev().take(take).rev().map(|l| Line::styled(format!(" {}", clip(l, area.width.saturating_sub(3) as usize)), Style::default().fg(pal.text()))));
        f.render_widget(Paragraph::new(story), rows[1]);
    }

    fn forge(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let Some(b) = self.selected_build() else {
            f.render_widget(Paragraph::new(vec![Line::from(""), dim_line("  No builds yet.", pal), Line::from(""), dim_line("  Start one in the console:  /build <request>", pal)]), area);
            return;
        };
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(9), Constraint::Min(3)]).split(area);
        let n = self.state.builds.len();
        let mut head = vec![Line::styled(format!(" Build {}/{} · {} · branch {} · commit {} · {}", self.pick.min(n - 1) + 1, n, b.task_id, if b.branch.is_empty() { "…" } else { &b.branch }, if b.sha.is_empty() { "…" } else { &b.sha }, if b.merge.is_empty() { "in progress" } else { &b.merge }), Style::default().fg(pal.text()).add_modifier(Modifier::BOLD))];
        for a in &b.attempts {
            head.push(Line::styled(format!("   {}", a), Style::default().fg(pal.text())));
        }
        for g in &b.gates {
            head.push(Line::from(vec![Span::styled(format!("   {} ", glyph(&g.state)), Style::default().fg(state_color(&g.state))), Span::styled(format!("{:<10} {}", g.name, clip(&g.detail, area.width.saturating_sub(18) as usize)), Style::default().fg(pal.text()))]));
        }
        for (sev, title) in &b.findings {
            head.push(Line::styled(format!("   audit {} {}", sev, clip(title, 100)), Style::default().fg(if sev == "P0" { Color::Red } else { Color::Yellow })));
        }
        if b.flaky {
            head.push(Line::styled("   a test failed once and passed on a rerun (flaky)", Style::default().fg(Color::Yellow)));
        }
        f.render_widget(Paragraph::new(head), rows[0]);
        let mut diff: Vec<Line> = vec![hairline("Diff against the base commit", area.width, pal)];
        match &self.diff {
            Some(d) if d["available"] == true => {
                for l in sanitize(d["stat"].as_str().unwrap_or("")).lines() {
                    diff.push(Line::styled(l.to_string(), Style::default().fg(pal.text())));
                }
                for u in d["untracked"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
                    diff.push(Line::styled(format!("?? {}", sanitize(u)), Style::default().fg(Color::Yellow)));
                }
                for l in sanitize(d["diff"].as_str().unwrap_or("")).lines() {
                    let colour = if l.starts_with('+') && !l.starts_with("+++") { Color::Green } else if l.starts_with('-') && !l.starts_with("---") { Color::Red } else if l.starts_with("@@") { Color::Cyan } else { pal.text() };
                    diff.push(Line::styled(l.to_string(), Style::default().fg(colour)));
                }
            }
            _ => diff.push(dim_line("  no live worktree for this build (it finished, or has not started). r refreshes.", pal)),
        }
        f.render_widget(Paragraph::new(diff).scroll((self.scroll, 0)), rows[1]);
    }

    /// The roster: a mascot beside three lines about each agent.
    fn agents(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let mut lines: Vec<Line> = Vec::new();
        // Who is running right now and the model each is on: one row per live span, not per role.
        lines.push(hairline("Running now", area.width, pal));
        let running = self.state.running_agents();
        if running.is_empty() {
            lines.push(dim_line("  Nothing is running.", pal));
        }
        for n in running {
            let what = if n.waiting.is_empty() { "working".to_string() } else { n.waiting.clone() };
            let what_colour = if n.waiting.is_empty() { pal.dim() } else { Color::Yellow };
            lines.push(Line::from(vec![
                Span::raw(format!("  {}", "  ".repeat(n.depth as usize))),
                Span::styled(self.name(&n.role), Style::default().fg(role_color(&n.role)).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {} · ", n.task_id), Style::default().fg(pal.dim())),
                Span::styled(delphi_engine::observe::ObserverState::model_label(n), Style::default().fg(pal.text())),
                Span::styled(format!(" · {}", clip(&what, 70)), Style::default().fg(what_colour)),
            ]));
        }
        lines.push(Line::from(""));
        lines.push(hairline("Roster", area.width, pal));
        for (role, a) in ordered(&self.state) {
            let m = mascot(role, &a.state);
            let model = if a.model.is_empty() { "no call yet".to_string() } else { format!("{} · {}{} · {}", a.tier, clip(&a.model, 28), if a.upscaled { " ⬆" } else { "" }, if a.endpoint.is_empty() { "—" } else { &a.endpoint }) };
            let meta = [
                vec![Span::styled(self.name(role), Style::default().fg(role_color(role)).add_modifier(Modifier::BOLD)), Span::styled(format!("  {}{} {}", if self.name(role) == *role { String::new() } else { format!("{}  ", role) }, glyph(&a.state), a.state), Style::default().fg(state_color(&a.state)))],
                vec![Span::styled(model, Style::default().fg(pal.text()))],
                vec![Span::styled(format!("in {} · cache {} · out {} · think {} · {} · {} call(s)", kilo(a.tokens.input), kilo(a.tokens.cache_read), kilo(a.tokens.output), kilo(a.tokens.thinking), money(a.cost_usd), a.calls), Style::default().fg(pal.dim()))],
            ];
            for (mline, meta_spans) in m.into_iter().zip(meta) {
                let mut spans = vec![Span::raw("  ")];
                spans.extend(mline.spans);
                spans.push(Span::raw("   "));
                spans.extend(meta_spans);
                lines.push(Line::from(spans));
            }
            lines.push(Line::from(""));
        }
        f.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), area);
    }

    /// Where the money and the tokens went.
    fn spend(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let st = &self.state;
        let series: Vec<f64> = st.series.iter().copied().collect();
        let mut lines: Vec<Line> = vec![
            hairline("Spend per call", area.width, pal),
            Line::from(vec![Span::styled(format!("  {}", sparkline(&series, area.width.saturating_sub(4) as usize)), Style::default().fg(Color::Cyan))]),
            dim_line(format!("  {} call(s) · total {} · {}", series.len(), money(st.spent_usd), if self.cap_usd > 0.0 { format!("{:.0}% of the {} ceiling", st.spent_usd / self.cap_usd * 100.0, money(self.cap_usd)) } else { "no ceiling known".to_string() }), pal),
            Line::from(""),
            hairline("Tokens", area.width, pal),
        ];
        let tk = &st.tokens;
        let total = tk.total().max(1);
        for (label, n) in [("input", tk.input), ("cache read", tk.cache_read), ("cache write", tk.cache_write), ("output", tk.output), ("thinking", tk.thinking)] {
            lines.push(Line::from(vec![Span::styled(format!("  {:<12}{:>9}  ", label, kilo(n)), Style::default().fg(pal.text())), Span::styled(bar(n as f64 / total as f64, 30), Style::default().fg(Color::Cyan))]));
        }
        lines.push(dim_line(format!("  prompt cache hit {:.0}% of input-side tokens · retries {}", st.cache_hit_ratio() * 100.0, st.retries), pal));
        lines.push(Line::from(""));
        lines.push(hairline("By agent", area.width, pal));
        let spent = st.spent_usd.max(f64::MIN_POSITIVE);
        for (role, a) in ordered(st) {
            lines.push(Line::from(vec![Span::styled(format!("  {:<13}", self.name(role)), Style::default().fg(role_color(role))), Span::styled(bar(a.cost_usd / spent, 24), Style::default().fg(role_color(role))), Span::styled(format!("  {}  {} tok", money(a.cost_usd), kilo(a.tokens.total())), Style::default().fg(pal.text()))]));
        }
        f.render_widget(Paragraph::new(lines).scroll((self.scroll, 0)), area);
    }

    fn archives(&self, f: &mut Frame, area: Rect) {
        let pal = self.pal();
        let mut s: Vec<Line> = vec![hairline("Knowledge base", area.width, pal)];
        let prompt = if self.search.editing { format!(" search› {}▏", self.search.query) } else if self.search.query.is_empty() { " press / to search specs, context, REVIEW.md and lessons".to_string() } else { format!(" last search: {}", self.search.query) };
        s.push(Line::styled(prompt, Style::default().fg(pal.text()).add_modifier(Modifier::BOLD)));
        if !self.search.note.is_empty() {
            s.push(dim_line(format!(" {}", self.search.note), pal));
        }
        for (i, h) in self.search.hits.iter().enumerate() {
            let pointer = sanitize(h["pointer"].as_str().unwrap_or(""));
            let lead = format!(" {}. ", i + 1);
            if self.scroll == 0 {
                if let Some(url) = h["url"].as_str() {
                    self.links.borrow_mut().push(Link { x: area.x + lead.chars().count() as u16, y: area.y + s.len() as u16, text: pointer.split('#').next().unwrap_or("").to_string(), url: url.to_string() });
                }
            }
            s.push(Line::from(vec![Span::styled(lead, Style::default().fg(pal.text())), Span::styled(pointer, Style::default().fg(pal.text())), Span::styled(format!("  {}", clip(h["heading"].as_str().unwrap_or(""), 60)), Style::default().fg(pal.dim()))]));
            for l in sanitize(h["preview"].as_str().unwrap_or("")).lines().take(3) {
                s.push(dim_line(format!("      {}", clip(l, area.width.saturating_sub(8) as usize)), pal));
            }
        }
        f.render_widget(Paragraph::new(s).scroll((self.scroll, 0)), area);
    }
}

// ── attaching and running ───────────────────────────────────────────────────

use delphi_core::client::{Client, ClientError};
use delphi_core::proto::Scope;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serde_json::json;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn attach(sock: &Path) -> Result<Client, ClientError> {
    let mut c = Client::connect(sock).map_err(ClientError::from)?;
    // A narrator-scope token: the daemon will refuse anything that changes state.
    let tok = c.issue_token(Scope::Narrator)?;
    c.hello(&tok)?;
    Ok(c)
}

fn rpc_error(e: &ClientError) -> String {
    crate::chat::rpc_error(e)
}

/// Load the daemon's snapshot into `app`.
fn load_snapshot(cmd: &mut Client, app: &mut App) -> Result<(), ClientError> {
    let snap = cmd.call("observe.snapshot", json!({}))?;
    let roles: Vec<String> = snap["enabled_agents"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
    app.state = ObserverState::new(&roles);
    app.mission = snap["mission_id"].as_str().unwrap_or("").to_string();
    app.cap_usd = snap["budget"]["cap_usd"].as_f64().unwrap_or(0.0);
    app.pending_proposals = snap["pending_proposals"].as_u64().unwrap_or(0);
    for e in snap["events"].as_array().into_iter().flatten() {
        app.state.apply(e);
    }
    Ok(())
}

/// `delphi narrator [view]`. `once` draws the current state a single time as text and exits.
pub fn run(home: &Path, ws_dir: &Path, view: Option<View>, names: HashMap<String, String>, once: bool) -> i32 {
    let layout = match delphi_core::spawn::layout(home, ws_dir) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("delphi narrator: {}", e);
            return 1;
        }
    };
    let mut cmd = match attach(&layout.narrator_sock) {
        Ok(c) => c,
        Err(ClientError::Io(_)) | Err(ClientError::Closed) => {
            eprintln!("delphi narrator: no daemon is running for this workspace. Start the console (`delphi chat`) first: the Narrator only watches.");
            return 1;
        }
        Err(e) => {
            eprintln!("delphi narrator: {}", rpc_error(&e));
            return 1;
        }
    };
    let mut app = App::new(&[], names);
    if let Err(e) = load_snapshot(&mut cmd, &mut app) {
        eprintln!("delphi narrator: {}", rpc_error(&e));
        return 1;
    }
    app.ws_dir = Some(ws_dir.to_path_buf());
    app.dark = detect_dark();
    app.view = view.unwrap_or_else(|| match app.state.default_view() {
        "council" => View::Council,
        "forge" => View::Forge,
        _ => View::Epic,
    });
    app.pick = usize::MAX;
    if once {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 34)).expect("in-memory terminal");
        t.draw(|f| app.draw(f)).expect("draw");
        println!("{}", buffer_text(t.backend().buffer()));
        return 0;
    }
    // A second connection carries the live stream so a slow redraw never blocks the daemon's queue.
    let (tx, rx) = mpsc::channel::<Value>();
    match attach(&layout.narrator_sock).and_then(|mut c| c.call("events.subscribe", json!({})).map(|_| c)) {
        Ok(mut ev) => {
            std::thread::spawn(move || {
                while let Ok(n) = ev.next_notification() {
                    if tx.send(n).is_err() {
                        return;
                    }
                }
                let _ = tx.send(json!({"method": "closed"}));
            });
        }
        Err(e) => {
            eprintln!("delphi narrator: {}", rpc_error(&e));
            return 1;
        }
    }
    let mut term = ratatui::init();
    let code = event_loop(&mut term, &mut cmd, &mut app, &rx);
    ratatui::restore();
    code
}

fn event_loop(term: &mut ratatui::DefaultTerminal, cmd: &mut Client, app: &mut App, rx: &mpsc::Receiver<Value>) -> i32 {
    let (mut dirty, mut last_poll, mut last_draw) = (true, Instant::now(), Instant::now() - Duration::from_secs(1));
    loop {
        while let Ok(n) = rx.try_recv() {
            match n["method"].as_str() {
                Some("event") => app.state.apply(&n["params"]),
                Some("delta.frame") => app.state.apply_frame(&n["params"]),
                Some("resync_required") => {
                    // The stream dropped frames: rebuild from the daemon's snapshot.
                    let _ = load_snapshot(cmd, app);
                }
                Some("closed") => app.connected = false,
                _ => {}
            }
            dirty = true;
        }
        if last_poll.elapsed() >= Duration::from_secs(2) && app.connected {
            last_poll = Instant::now();
            if let Ok(b) = cmd.call("budget.get", json!({})) {
                app.cap_usd = b["cap_usd"].as_f64().unwrap_or(app.cap_usd);
            }
            if app.view == View::Forge {
                refresh_diff(cmd, app);
            }
            dirty = true;
        }
        if dirty && last_draw.elapsed() >= Duration::from_millis(33) {
            if term.draw(|f| app.draw(f)).is_err() {
                return 1;
            }
            // Paths become clickable (OSC 8) by re-printing them over the cells just drawn.
            let links = app.links.borrow().clone();
            if !links.is_empty() {
                use std::io::Write;
                let mut out = std::io::stdout();
                let _ = out.write_all(osc8(&links).as_bytes());
                let _ = out.flush();
            }
            dirty = false;
            last_draw = Instant::now();
        }
        if event::poll(Duration::from_millis(50)).unwrap_or(false) {
            match event::read() {
                Ok(Event::Key(k)) if k.kind != KeyEventKind::Release => {
                    match app.on_key(k.code, k.modifiers) {
                        Action::Quit => return 0,
                        Action::Search(q) => match cmd.call("kb.search", json!({"query": q})) {
                            Ok(v) => {
                                app.search.hits = v.as_array().cloned().unwrap_or_default();
                                let sources = app.ws_dir.as_ref().map(|w| delphi_engine::kb::sources(w)).unwrap_or_default();
                                for h in app.search.hits.iter_mut() {
                                    let rel = h["pointer"].as_str().unwrap_or("").split('#').next().unwrap_or("").to_string();
                                    if let Some(s) = sources.iter().find(|s| s.rel_path == rel) {
                                        h["url"] = Value::String(file_url(&s.path));
                                    }
                                }
                                app.search.note = if app.search.hits.is_empty() { "no results".into() } else { String::new() };
                            }
                            Err(e) => app.search.note = rpc_error(&e),
                        },
                        Action::RefreshDiff => refresh_diff(cmd, app),
                        Action::None => {}
                    }
                    dirty = true;
                }
                Ok(Event::Resize(..)) => dirty = true,
                _ => {}
            }
        }
    }
}

fn refresh_diff(cmd: &mut Client, app: &mut App) {
    if let Some(b) = app.selected_build() {
        let task = b.task_id.clone();
        app.diff = cmd.call("observe.diff", json!({"task_id": task})).ok();
    }
}

/// The text of a drawn buffer, one line per row (for tests and `--once`).
pub fn buffer_text(buf: &Buffer) -> String {
    let area = buf.area;
    (0..area.height).map(|y| (0..area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>().trim_end().to_string()).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::Terminal;
    use serde_json::json;

    fn app() -> App {
        let roles: Vec<String> = ["orchestrator", "architect", "builder", "red-team", "strategist"].iter().map(|s| s.to_string()).collect();
        let names: HashMap<String, String> = [("orchestrator", "Athena"), ("architect", "Daedalus"), ("red-team", "Nemesis")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let mut a = App::new(&roles, names);
        a.mission = "2026-10-24-session".into();
        a.cap_usd = 5.0;
        a
    }

    fn ev(seq: u64, kind: &str, task: &str, agent: &str, payload: Value) -> Value {
        json!({"seq": seq, "type": kind, "task_id": task, "agent": agent, "payload": payload})
    }

    fn render(a: &App, w: u16, h: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| a.draw(f)).unwrap();
        buffer_text(t.backend().buffer())
    }

    #[test]
    fn views_cycle_with_shift_arrows_and_numbers_and_wrap() {
        let mut a = app();
        assert_eq!(a.view, View::Epic);
        a.on_key(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(a.view, View::Forge);
        for _ in 0..4 {
            a.on_key(KeyCode::Right, KeyModifiers::SHIFT);
        }
        assert_eq!(a.view, View::Council, "wraps around after six views");
        a.on_key(KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(a.view, View::Archives);
        a.on_key(KeyCode::Char('4'), KeyModifiers::NONE);
        assert_eq!(a.view, View::Agents);
        a.on_key(KeyCode::Char('5'), KeyModifiers::NONE);
        assert_eq!(a.view, View::Spend);
        assert_eq!(a.on_key(KeyCode::Char('q'), KeyModifiers::NONE), Action::Quit);
        assert_eq!(a.on_key(KeyCode::Char('c'), KeyModifiers::CONTROL), Action::Quit);
        assert_eq!((View::parse("council"), View::parse("roster"), View::parse("spend"), View::parse("nonsense")), (Some(View::Council), Some(View::Agents), Some(View::Spend), None));
    }

    #[test]
    fn no_view_is_drawn_inside_a_box_and_the_footer_dots_are_bare() {
        let mut a = app();
        a.state.apply(&ev(1, "council.started", "t_2", "", json!({"proposition": "Queue or cron?", "seats": [{"role": "architect", "family": "claude"}, {"role": "red-team", "family": "gemini"}], "homogeneous": false})));
        a.state.apply(&ev(2, "worktree.created", "t_3", "builder", json!({"branch": "b"})));
        for v in VIEWS {
            a.view = v;
            let s = render(&a, 110, 34);
            for corner in ['┌', '┐', '└', '┘', '│'] {
                assert!(!s.contains(corner), "{:?} draws a box ({}):\n{}", v, corner, s);
            }
            assert!(s.contains("────"), "{:?} uses hairline rules", v);
            assert!(!s.contains("[●") && !s.contains("[○"), "footer dots are not bracketed in {:?}", v);
        }
        a.view = View::Agents;
        assert!(render(&a, 140, 34).contains("navigate: shift+arrows · ○ ○ ○ ● ○ ○"));
    }

    #[test]
    fn the_agents_view_lists_every_running_agent_and_its_model_above_the_roster() {
        let mut a = app();
        a.view = View::Agents;
        let s = render(&a, 140, 40);
        assert!(s.contains("Running now") && s.contains("Nothing is running.") && s.contains("Roster"), "{}", s);
        let env = |seq: u64, kind: &str, role: &str, payload: serde_json::Value| serde_json::json!({"seq": seq, "type": kind, "task_id": "t_1", "agent": role, "payload": payload});
        for (seq, span, role, depth) in [(1u64, "n1", "orchestrator", 0u64), (3, "n2", "architect", 1)] {
            a.state.apply(&env(seq, "dag.node.added", role, serde_json::json!({"node_id": span, "role": role, "depth": depth})));
            a.state.apply(&env(seq + 1, "dag.node.state", role, serde_json::json!({"node_id": span, "state": "running"})));
        }
        let mut reserve = env(7, "budget.reserve", "architect", serde_json::json!({"reservation_id": "r_1", "amount": 1}));
        reserve["span_id"] = serde_json::json!("n2");
        reserve["endpoint_id"] = serde_json::json!("backup");
        reserve["model_id"] = serde_json::json!("sonnet-x");
        reserve["model_tier"] = serde_json::json!("workhorse");
        a.state.apply(&reserve);
        let s = render(&a, 140, 40);
        assert!(!s.contains("Nothing is running."), "{}", s);
        assert!(s.contains("sonnet-x via backup (workhorse)") && s.contains("no call yet"), "the architect's model, and the orchestrator that has not called yet: {}", s);
        assert!(s.contains("Running now") && s.find("Running now").unwrap() < s.find("Roster").unwrap(), "{}", s);
    }

    #[test]
    fn the_screen_always_shows_the_header_agents_totals_and_where_you_are() {
        let mut a = app();
        let s = render(&a, 120, 24);
        for want in ["DELPHI OBSERVER", "EPIC DAG", "2026-10-24-session", "Athena", "Daedalus", "Epic DAG 2/6", "← Council", "→ Forge Diffs", "spend $0.000 / $5.00"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        a.connected = false;
        assert!(render(&a, 120, 24).contains("(disconnected)"));
        a.state.spent_usd = 4.6;
        assert!(render(&a, 120, 24).contains("$4.60 / $5.00"));
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
    fn mascots_are_seven_columns_with_a_crown_a_painted_face_and_a_flush_chin() {
        for role in ["orchestrator", "builder", "architect", "red-team", "strategist", "scout"] {
            for st in ["idle", "running", "waiting on you", "escalated"] {
                let m = mascot(role, st);
                for (i, l) in m.iter().enumerate() {
                    assert_eq!(l.spans.iter().map(|s| s.content.chars().count()).sum::<usize>(), 7, "{} {} line {}", role, st, i);
                }
                assert_eq!(m[2].spans[0].content, "▀▀▀▀▀▀▀", "the chin is flush");
                assert!(m[1].spans[0].style.bg.is_some(), "the face is painted, not outlined");
                assert!(m[0].spans.iter().all(|s| s.content == " " || s.content == "▄"), "the crown uses only the bottom-half block");
            }
        }
        assert_eq!(mascot("scout", "idle")[1].spans[0].content, " • • • ", "the scout has three eyes and no mouth");
    }

    #[test]
    fn the_epic_shows_the_tree_with_states_costs_and_a_plain_language_story() {
        let mut a = app();
        for e in [
            ev(1, "task.created", "t_1", "", json!({"prompt": "design the narrator", "target": "orchestrator"})),
            ev(2, "dag.node.added", "t_1", "orchestrator", json!({"node_id": "n1", "role": "orchestrator", "depth": 0})),
            ev(3, "dag.node.added", "t_1", "architect", json!({"node_id": "n2", "role": "architect", "depth": 1, "parent": "n1"})),
            ev(4, "dag.node.state", "t_1", "architect", json!({"node_id": "n2", "state": "running"})),
        ] {
            a.state.apply(&e);
        }
        let s = render(&a, 110, 28);
        for want in ["t_1", "design the narrator", "Athena", "Daedalus", "Narrative", "architect is running", "● Daedalus"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        assert!(!s.contains('✗') && !s.contains('⚠'), "no emoji-width status glyphs");
    }

    #[test]
    fn a_council_is_drawn_as_open_seat_columns_then_a_verdict_with_the_minority_report() {
        let mut a = app();
        a.view = View::Council;
        a.state.apply(&ev(1, "council.started", "t_2", "", json!({"proposition": "Queue or cron for retries?", "seats": [{"role": "architect", "family": "claude"}, {"role": "red-team", "family": "gemini"}], "homogeneous": false})));
        let s = render(&a, 120, 30);
        assert!(s.contains("Queue or cron for retries?") && s.contains("round 1") && s.contains("writing… (blind)") && s.contains("Round 1 is blind"), "{}", s);
        for e in [
            ev(2, "council.barrier.released", "t_2", "", json!({"round": 1})),
            ev(3, "council.stance", "t_2", "", json!({"role": "architect", "status": "answered", "text": "OPTION: queue\nA queue survives restarts.\x1b[2J"})),
            ev(4, "council.stance", "t_2", "", json!({"role": "red-team", "status": "answered", "text": "OPTION: queue\nIt drops jobs on crash."})),
            ev(5, "council.critique", "t_2", "", json!({"role": "red-team", "recommendation": "DO-NOT-PROCEED", "concur": false, "findings": [{"severity": "P0", "title": "Loses jobs on crash"}]})),
            ev(6, "council.verdict", "t_2", "", json!({"path": "missions/m/councils/001-x.md", "escalated": "1 unresolved P0 finding(s)", "unresolved": [{"severity": "P0", "seat": "red-team", "title": "Loses jobs on crash"}], "options": [{"label": "DO-NOT-PROCEED", "seats": ["red-team"]}]})),
        ] {
            a.state.apply(&e);
        }
        let s = render(&a, 120, 34);
        for want in ["Daedalus (claude)", "Nemesis (gemini)", "A queue survives restarts.", "Round 2 → DO-NOT-PROCEED", "does not concur", "Escalated to you", "[P0] red-team: Loses jobs on crash", "Minority report:", "001-x.md", "── Verdict"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        assert!(!s.contains('\x1b') && !s.contains("[2J"), "escape sequences never reach the screen");
    }

    #[test]
    fn the_forge_shows_gates_findings_and_the_live_diff() {
        let mut a = app();
        a.view = View::Forge;
        assert!(render(&a, 100, 24).contains("No builds yet"));
        for e in [
            ev(1, "worktree.created", "t_3", "builder", json!({"branch": "delphi/t_3"})),
            ev(2, "build.attempt", "t_3", "", json!({"attempt": 1, "tier": "workhorse", "family": "claude", "homogeneous_retry": false})),
            ev(3, "gates.evaluated", "t_3", "", json!({"sha": "0123456789abcdef", "gates": [{"name": "tests", "state": "green", "detail": "3 passed"}, {"name": "operator", "state": "red", "detail": "waiting for you"}]})),
            ev(4, "audit.recorded", "t_3", "red-team", json!({"findings": [{"severity": "P2", "title": "naming nit"}]})),
        ] {
            a.state.apply(&e);
        }
        a.diff = Some(json!({"available": true, "stat": " src/lib.rs | 2 +-", "diff": "@@ -1 +1 @@\n-old\n+new\x1b[31m evil", "untracked": ["src/new.rs"]}));
        let s = render(&a, 110, 32);
        for want in ["delphi/t_3", "0123456789ab", "attempt 1", "✔ tests", "3 passed", "✖ operator", "waiting for you", "audit P2 naming nit", "src/lib.rs | 2", "-old", "+new evil", "?? src/new.rs", "── Diff against the base commit"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        assert_eq!(a.on_key(KeyCode::Char('r'), KeyModifiers::NONE), Action::RefreshDiff);
    }

    #[test]
    fn the_roster_pairs_every_mascot_with_three_lines_of_facts() {
        let mut a = app();
        for e in [
            ev(1, "dag.node.added", "t_1", "architect", json!({"node_id": "n2", "role": "architect", "depth": 0})),
            {
                let mut u = ev(2, "agent.usage", "t_1", "architect", json!({"input_tokens": 1200, "cache_read_tokens": 30000, "cache_write_tokens": 0, "output_tokens": 800, "thinking_tokens": 100, "cost_usd": 0.12}));
                u["span_id"] = json!("n2");
                u["model_tier"] = json!("frontier");
                u["model_id"] = json!("opus-x");
                u["endpoint_id"] = json!("direct");
                u
            },
        ] {
            a.state.apply(&e);
        }
        a.view = View::Agents;
        let s = render(&a, 120, 40);
        for want in ["MASCOT ROSTER", "Daedalus  architect  ● running", "frontier · opus-x · direct", "in 1200 · cache 30.0k · out 800 · think 100 · $0.120 · 1 call(s)", "▀▀▀▀▀▀▀", "no call yet"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        assert!(s.lines().filter(|l| l.contains("▀▀▀▀▀▀▀")).count() >= 5, "one mascot per agent");
    }

    #[test]
    fn spend_shows_a_sparkline_tokens_by_kind_and_each_agents_share() {
        let mut a = app();
        for (i, cost) in [0.01, 0.02, 0.08, 0.04].iter().enumerate() {
            let mut u = ev(i as u64 + 1, "agent.usage", "t_1", "builder", json!({"input_tokens": 1000, "cache_read_tokens": 3000, "cache_write_tokens": 0, "output_tokens": 200, "thinking_tokens": 0, "cost_usd": cost}));
            u["span_id"] = json!("n1");
            a.state.apply(&u);
        }
        a.view = View::Spend;
        let s = render(&a, 110, 30);
        for want in ["SPEND TELEMETRY", "── Spend per call", "4 call(s) · total $0.150", "── Tokens", "cache read", "prompt cache hit 75%", "── By agent", "builder"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        assert!(s.contains('█') && s.contains('▂'), "the sparkline rises to its largest call: {}", s);
        assert_eq!(sparkline(&[0.0, 0.0], 10), "▁▁");
        assert_eq!(sparkline(&[1.0, 2.0, 4.0, 8.0], 3), "▃▅█", "only the newest `width` points");
    }

    #[test]
    fn archives_search_the_kb_and_typed_text_is_not_read_as_view_keys() {
        let mut a = app();
        a.view = View::Archives;
        assert!(render(&a, 120, 28).contains("press / to search"));
        a.on_key(KeyCode::Char('/'), KeyModifiers::NONE);
        for c in "REV-SEC-04".chars() {
            assert_eq!(a.on_key(KeyCode::Char(c), KeyModifiers::NONE), Action::None);
        }
        assert_eq!(a.view, View::Archives, "typed characters do not switch views");
        assert!(render(&a, 120, 28).contains("search› REV-SEC-04"));
        assert_eq!(a.on_key(KeyCode::Enter, KeyModifiers::NONE), Action::Search("REV-SEC-04".into()));
        a.search.hits = vec![json!({"pointer": "REVIEW.md#L17-L28", "heading": "Security > Rubric", "preview": "- **[REV-SEC-04]** Ratified Rubric Loading\x1b[1m", "url": "file:///ws/team/REVIEW.md"})];
        let s = render(&a, 120, 28);
        assert!(s.contains("1. REVIEW.md#L17-L28") && s.contains("Ratified Rubric Loading") && !s.contains('\x1b'), "{}", s);
    }

    #[test]
    fn file_paths_become_osc8_links_placed_over_the_text_that_was_drawn() {
        let mut a = app();
        a.view = View::Archives;
        a.search.query = "x".into();
        a.search.hits = vec![json!({"pointer": "REVIEW.md#L1-L2", "heading": "h", "preview": "p", "url": "file:///ws/REVIEW.md"})];
        let text = render(&a, 100, 24);
        let links = a.links.borrow().clone();
        assert_eq!(links.len(), 1);
        let l = &links[0];
        assert_eq!((l.text.as_str(), l.url.as_str()), ("REVIEW.md", "file:///ws/REVIEW.md"));
        let row: Vec<&str> = text.lines().collect();
        assert_eq!(row[l.y as usize].chars().skip(l.x as usize).take(l.text.len()).collect::<String>(), l.text, "the link sits exactly on the drawn text");
        let seq = osc8(&links);
        assert!(seq.contains(&format!("\x1b[{};{}H", l.y + 1, l.x + 1)) && seq.contains("\x1b]8;;file:///ws/REVIEW.md\x1b\\REVIEW.md\x1b]8;;\x1b\\") && seq.starts_with("\x1b7") && seq.ends_with("\x1b8"));
        // Council artifact paths resolve inside the workspace; spaces and other bytes are escaped.
        a.ws_dir = Some(std::path::PathBuf::from("/srv/delphi/my ws"));
        assert_eq!(a.url_for("missions/m1/councils/001-x.md").unwrap(), "file:///srv/delphi/my%20ws/local/missions/m1/councils/001-x.md");
        assert_eq!(file_url(std::path::Path::new("/a/b c/é.md")), "file:///a/b%20c/%C3%A9.md");
        assert!(App::new(&[], HashMap::new()).url_for("missions/x.md").is_none(), "no workspace, no link");
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

    #[test]
    fn a_tiny_terminal_does_not_panic() {
        let mut a = app();
        a.state.apply(&ev(1, "council.started", "t_2", "", json!({"proposition": "x", "seats": [{"role": "architect", "family": "claude"}], "homogeneous": true})));
        for v in VIEWS {
            a.view = v;
            for (w, h) in [(1, 1), (10, 3), (30, 6), (200, 60)] {
                let _ = render(&a, w, h);
            }
        }
    }

    /// A developer aid: print every view of a populated mission. `cargo test -p delphi print_views -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_views() {
        let mut a = app();
        for e in [
            ev(1, "task.created", "t_1", "", json!({"prompt": "Review the retry design", "target": "orchestrator"})),
            ev(2, "dag.node.added", "t_1", "orchestrator", json!({"node_id": "n1", "role": "orchestrator", "depth": 0})),
            ev(3, "dag.node.added", "t_1", "red-team", json!({"node_id": "n2", "role": "red-team", "depth": 1, "parent": "n1"})),
            ev(4, "approval.requested", "t_1", "red-team", json!({"approval_id": "ap_1", "role": "red-team", "summary": "merge abc"})),
            ev(5, "council.started", "t_2", "", json!({"proposition": "Queue or cron for retries?", "seats": [{"role": "architect", "family": "claude"}, {"role": "red-team", "family": "gemini"}, {"role": "strategist", "family": "claude"}], "homogeneous": false})),
        ] {
            a.state.apply(&e);
        }
        for (i, c) in [0.01, 0.03, 0.02, 0.09, 0.05].iter().enumerate() {
            let mut u = ev(10 + i as u64, "agent.usage", "t_1", "orchestrator", json!({"input_tokens": 4000, "cache_read_tokens": 22000, "cache_write_tokens": 0, "output_tokens": 600, "thinking_tokens": 200, "cost_usd": c}));
            u["span_id"] = json!("n1");
            u["model_tier"] = json!("frontier");
            u["model_id"] = json!("opus-x");
            u["endpoint_id"] = json!("direct");
            a.state.apply(&u);
        }
        for v in VIEWS {
            a.view = v;
            println!("{}\n", render(&a, 112, 30));
        }
    }
}
