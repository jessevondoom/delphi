//! `delphi narrator [view]` / `delphi observe [view]`: the read-only window (`specs/005` §4, `specs/005b` §4).
//!
//! One full-window view at a time (no tiled panes), cycled with `Shift+Left`/`Shift+Right`: Council
//! Chamber, Epic, Forge, Archives & Spend. A one-line agents strip stays in the header and the footer
//! carries mission totals. It attaches with a narrator-scope token, so the daemon refuses anything that
//! would start work or approve anything; killing and reopening it rebuilds the same screen from the
//! daemon's snapshot. Everything a model wrote is stripped of control sequences before it is drawn.
//!
//! The drawing code only reads [`App`], so tests render it into an in-memory buffer.

use crate::avatar;
use delphi_engine::observe::{sanitize, Build, Council, ObserverState};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Council,
    Epic,
    Forge,
    Archives,
}

pub const VIEWS: [View; 4] = [View::Council, View::Epic, View::Forge, View::Archives];

impl View {
    pub fn title(self) -> &'static str {
        match self {
            View::Council => "COUNCIL CHAMBER",
            View::Epic => "EPIC DAG & TASK GRAPH",
            View::Forge => "FORGE: WORKTREES & DIFFS",
            View::Archives => "ARCHIVES & SPEND",
        }
    }

    fn short(self) -> &'static str {
        match self {
            View::Council => "Council",
            View::Epic => "Epic DAG",
            View::Forge => "Forge Diffs",
            View::Archives => "Archives & Spend",
        }
    }

    pub fn parse(s: &str) -> Option<View> {
        match s.to_lowercase().as_str() {
            "council" | "chamber" | "1" => Some(View::Council),
            "epic" | "dag" | "2" => Some(View::Epic),
            "forge" | "diffs" | "3" => Some(View::Forge),
            "archives" | "spend" | "kb" | "4" => Some(View::Archives),
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
    pub hits: Vec<Value>,
    pub note: String,
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
}

impl App {
    pub fn new(roles: &[String], names: HashMap<String, String>) -> App {
        App { state: ObserverState::new(roles), view: View::Epic, names, mission: String::new(), cap_usd: 0.0, pending_proposals: 0, connected: true, pick: 0, scroll: 0, selected_node: 0, search: Search::default(), diff: None }
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
            K::Char(c @ '1'..='4') => self.go(VIEWS[c as usize - '1' as usize]),
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
}

// ── drawing ─────────────────────────────────────────────────────────────────

fn role_color(role: &str) -> Color {
    avatar::from_canonical_id(role).map(|r| avatar::get_theme(r).chassis_rgb).map(|(r, g, b)| Color::Rgb(r, g, b)).unwrap_or(Color::Gray)
}

fn money(v: f64) -> String {
    if v >= 100.0 { format!("${:.0}", v) } else if v >= 1.0 { format!("${:.2}", v) } else { format!("${:.3}", v) }
}

fn kilo(n: u64) -> String {
    if n >= 10_000 { format!("{:.1}k", n as f64 / 1000.0) } else { n.to_string() }
}

fn bar(frac: f64, width: usize) -> String {
    let f = frac.clamp(0.0, 1.0);
    let full = (f * width as f64).round() as usize;
    format!("{}{}", "▓".repeat(full), "░".repeat(width.saturating_sub(full)))
}

fn glyph(state: &str) -> &'static str {
    match state {
        "done" | "green" | "merged" => "✓",
        "running" => "●",
        "escalated" | "failed" | "red" | "refused" => "✗",
        "waiting on you" => "?",
        "pending" | "idle" => "○",
        _ => "·",
    }
}

fn state_color(state: &str) -> Color {
    match state {
        "done" | "green" | "merged" => Color::Green,
        "running" => Color::Cyan,
        "escalated" | "failed" | "red" | "refused" => Color::Red,
        "waiting on you" => Color::Yellow,
        _ => Color::DarkGray,
    }
}

fn clip(s: &str, n: usize) -> String {
    let s = sanitize(s).replace('\n', " ");
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>()) }
}

impl App {
    pub fn draw(&self, f: &mut Frame) {
        let area = f.area();
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(2)]).split(area);
        self.header(f, rows[0]);
        match self.view {
            View::Council => self.council(f, rows[1]),
            View::Epic => self.epic(f, rows[1]),
            View::Forge => self.forge(f, rows[1]),
            View::Archives => self.archives(f, rows[1]),
        }
        self.footer(f, rows[2]);
    }

    fn header(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1)]).split(area);
        let conn = if self.connected { "" } else { "  (disconnected)" };
        let title = format!(" DELPHI NARRATOR ── [{}] ── mission {}{}", self.view.title(), if self.mission.is_empty() { "…" } else { &self.mission }, conn);
        f.render_widget(Paragraph::new(title).style(Style::default().add_modifier(Modifier::BOLD)), rows[0]);
        // The agents strip: one badge per enabled role, in the viewer's theme names.
        let mut spans: Vec<Span> = vec![Span::raw(" ")];
        for (role, a) in &self.state.agents {
            let face = match a.state.as_str() {
                "waiting on you" => "x?x",
                "running" => "•_•",
                "escalated" => "x_x",
                _ => "•─•",
            };
            spans.push(Span::styled(format!("⟦{}⟧", face), Style::default().fg(role_color(role))));
            spans.push(Span::styled(format!("{} {}  ", self.name(role), a.state), Style::default().fg(state_color(&a.state))));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
    }

    fn footer(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1), Constraint::Length(1)]).split(area);
        let i = self.view.index();
        let dots: String = (0..VIEWS.len()).map(|n| if n == i { "●" } else { "○" }).collect::<Vec<_>>().join(" ");
        let (prev, next) = (self.view.step(-1).short(), self.view.step(1).short());
        f.render_widget(Paragraph::new(format!(" [{}] Panel {}/{} · Shift+Left: [{}] · Shift+Right: [{}] · 1-4 jump · q quit", dots, i + 1, VIEWS.len(), prev, next)).style(Style::default().fg(Color::DarkGray)), rows[0]);
        let st = &self.state;
        let frac = if self.cap_usd > 0.0 { st.spent_usd / self.cap_usd } else { 0.0 };
        let colour = if frac >= 0.9 { Color::Red } else if frac >= 0.75 { Color::Yellow } else { Color::Green };
        let line = Line::from(vec![
            Span::raw(format!(" spend {} / {} ", money(st.spent_usd), if self.cap_usd > 0.0 { money(self.cap_usd) } else { "?".into() })),
            Span::styled(bar(frac, 12), Style::default().fg(colour)),
            Span::raw(format!(" · tokens {} · cache hit {:.0}% · retries {}{}", kilo(st.tokens.total()), st.cache_hit_ratio() * 100.0, st.retries, if self.pending_proposals > 0 { format!(" · {} proposal(s) await /ratify", self.pending_proposals) } else { String::new() })),
        ]);
        f.render_widget(Paragraph::new(line), rows[1]);
    }

    fn council(&self, f: &mut Frame, area: Rect) {
        let Some(c) = self.selected_council() else {
            f.render_widget(Paragraph::new("\n  No Council yet.\n\n  Convene one in the console:  /council <proposition>").block(Block::default().borders(Borders::TOP)), area);
            return;
        };
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(if c.phase == "round 1" || c.phase == "round 2" { 3 } else { 9 })]).split(area);
        let n = self.state.councils.len();
        let head = vec![
            Line::from(format!(" Council {}/{} · {} · {}{}", self.pick.min(n - 1) + 1, n, c.task_id, c.phase, if c.homogeneous { " · homogeneous (one model family)" } else { "" })),
            Line::from(format!(" {}", clip(&c.proposition, area.width.saturating_sub(3) as usize))),
        ];
        f.render_widget(Paragraph::new(head).block(Block::default().borders(Borders::TOP)), rows[0]);
        // One column per seat: its Round 1 stance (or what it is writing), then its Round 2 critique.
        if c.seats.is_empty() {
            return;
        }
        let cols = Layout::default().direction(Direction::Horizontal).constraints(vec![Constraint::Ratio(1, c.seats.len() as u32); c.seats.len()]).split(rows[1]);
        for (seat, col) in c.seats.iter().zip(cols.iter()) {
            let mut lines: Vec<Line> = Vec::new();
            let stance = c.stances.iter().find(|s| s.role == seat.role);
            match stance {
                Some(s) if s.answered => {
                    lines.push(Line::styled("Round 1 (blind)", Style::default().add_modifier(Modifier::BOLD)));
                    lines.extend(sanitize(&s.text).lines().map(|l| Line::from(l.to_string())));
                }
                Some(s) => lines.push(Line::styled(format!("abstained: {}", clip(&s.text, 80)), Style::default().fg(Color::DarkGray))),
                None => {
                    let live = self.state.nodes.values().find(|n| n.task_id == c.task_id && n.role == seat.role).and_then(|n| self.state.live.get(&n.id));
                    // Live text of a blind round is shown to the operator only: agents never see it.
                    lines.push(Line::styled("writing… (blind)", Style::default().fg(Color::DarkGray)));
                    if let Some(t) = live {
                        lines.extend(sanitize(t).lines().rev().take(8).collect::<Vec<_>>().into_iter().rev().map(|l| Line::from(l.to_string())));
                    }
                }
            }
            if let Some(cr) = c.critiques.iter().find(|x| x.role == seat.role) {
                lines.push(Line::from(""));
                lines.push(Line::styled(format!("Round 2 → {}{}", if cr.recommendation.is_empty() { "unreadable" } else { &cr.recommendation }, match cr.concur { Some(true) => " · concurs", Some(false) => " · does not concur", None => "" }), Style::default().add_modifier(Modifier::BOLD)));
                for (sev, title) in &cr.findings {
                    lines.push(Line::styled(format!("{} {}", sev, title), Style::default().fg(if sev == "P0" { Color::Red } else if sev == "P1" { Color::Yellow } else { Color::Gray })));
                }
            }
            let title = format!(" {} ({}) ", self.name(&seat.role), seat.family);
            let block = Block::default().borders(Borders::ALL).title(Span::styled(title, Style::default().fg(role_color(&seat.role))));
            f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }).scroll((self.scroll, 0)), *col);
        }
        // The verdict: the Orchestrator's synthesis and the daemon-written Minority Report.
        let mut v: Vec<Line> = Vec::new();
        match c.phase.as_str() {
            "round 1" => v.push(Line::from(" Round 1 is blind: stances appear when every seat has answered.")),
            "round 2" => v.push(Line::from(" Round 2: seats are critiquing each other's stances.")),
            _ => {
                if let Some(why) = &c.escalation {
                    v.push(Line::styled(format!(" Escalated to you: {}", clip(why, 100)), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
                    for (label, seats) in &c.options {
                        v.push(Line::from(format!("   option: {} — {}", label, seats.join(", "))));
                    }
                } else {
                    v.push(Line::styled(" Verdict reached; no deterministic escalation.", Style::default().fg(Color::Green)));
                }
                v.push(Line::styled(" Minority report:", Style::default().add_modifier(Modifier::BOLD)));
                if c.unresolved.is_empty() {
                    v.push(Line::from("   none open"));
                }
                for (sev, seat, title) in c.unresolved.iter().take(4) {
                    v.push(Line::styled(format!("   [{}] {}: {}", sev, seat, clip(title, area.width.saturating_sub(16) as usize)), Style::default().fg(if sev == "P0" { Color::Red } else { Color::Yellow })));
                }
                if !c.path.is_empty() {
                    v.push(Line::styled(format!(" saved: {}", c.path), Style::default().fg(Color::DarkGray)));
                }
            }
        }
        f.render_widget(Paragraph::new(v).block(Block::default().borders(Borders::ALL).title(" Verdict ")).wrap(Wrap { trim: false }), rows[2]);
    }

    fn epic(&self, f: &mut Frame, area: Rect) {
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
            lines.push(Line::from("  No agents have run yet in this mission."));
        }
        let mut last_task = String::new();
        for (i, (n, depth)) in order.iter().enumerate() {
            if n.task_id != last_task {
                last_task = n.task_id.clone();
                let t = self.state.tasks.get(&n.task_id);
                lines.push(Line::styled(format!(" {} {}{}", n.task_id, t.map(|t| t.flow.as_str()).unwrap_or(""), t.map(|t| format!(" · {}", clip(&t.prompt, 60))).unwrap_or_default()), Style::default().add_modifier(Modifier::BOLD)));
            }
            let style = if i == sel { Style::default().add_modifier(Modifier::REVERSED) } else { Style::default() };
            lines.push(Line::from(vec![
                Span::raw(format!("  {}", "  ".repeat(*depth))),
                Span::styled(format!("{} ", glyph(&n.state)), Style::default().fg(state_color(&n.state))),
                Span::styled(format!("{:<12}", self.name(&n.role)), style.fg(role_color(&n.role))),
                Span::styled(format!(" {:<10} {:>7} tok  {}", n.state, kilo(n.tokens), money(n.cost_usd)), style),
            ]));
        }
        f.render_widget(Paragraph::new(lines).block(Block::default().borders(Borders::TOP)).scroll((self.scroll, 0)), rows[0]);
        // The story so far, from events, not from a model.
        let take = rows[1].height.saturating_sub(2) as usize;
        let story: Vec<Line> = self.state.narrative.iter().rev().take(take).rev().map(|l| Line::from(format!(" {}", clip(l, area.width.saturating_sub(3) as usize)))).collect();
        f.render_widget(Paragraph::new(story).block(Block::default().borders(Borders::ALL).title(" Narrative ")), rows[1]);
    }

    fn forge(&self, f: &mut Frame, area: Rect) {
        let Some(b) = self.selected_build() else {
            f.render_widget(Paragraph::new("\n  No builds yet.\n\n  Start one in the console:  /build <request>").block(Block::default().borders(Borders::TOP)), area);
            return;
        };
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(9), Constraint::Min(3)]).split(area);
        let n = self.state.builds.len();
        let mut head = vec![Line::styled(format!(" Build {}/{} · {} · branch {} · commit {} · {}", self.pick.min(n - 1) + 1, n, b.task_id, if b.branch.is_empty() { "…" } else { &b.branch }, if b.sha.is_empty() { "…" } else { &b.sha }, if b.merge.is_empty() { "in progress" } else { &b.merge }), Style::default().add_modifier(Modifier::BOLD))];
        for a in &b.attempts {
            head.push(Line::from(format!("   {}", a)));
        }
        for g in &b.gates {
            head.push(Line::from(vec![Span::styled(format!("   {} ", glyph(&g.state)), Style::default().fg(state_color(&g.state))), Span::raw(format!("{:<10} {}", g.name, clip(&g.detail, area.width.saturating_sub(18) as usize)))]));
        }
        for (sev, title) in &b.findings {
            head.push(Line::styled(format!("   audit {} {}", sev, clip(title, 100)), Style::default().fg(if sev == "P0" { Color::Red } else { Color::Yellow })));
        }
        if b.flaky {
            head.push(Line::styled("   a test failed once and passed on a rerun (flaky)", Style::default().fg(Color::Yellow)));
        }
        f.render_widget(Paragraph::new(head).block(Block::default().borders(Borders::TOP)), rows[0]);
        let mut diff: Vec<Line> = Vec::new();
        match &self.diff {
            Some(d) if d["available"] == true => {
                for l in sanitize(d["stat"].as_str().unwrap_or("")).lines() {
                    diff.push(Line::from(l.to_string()));
                }
                for u in d["untracked"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
                    diff.push(Line::styled(format!("?? {}", sanitize(u)), Style::default().fg(Color::Yellow)));
                }
                for l in sanitize(d["diff"].as_str().unwrap_or("")).lines() {
                    let colour = if l.starts_with('+') && !l.starts_with("+++") { Color::Green } else if l.starts_with('-') && !l.starts_with("---") { Color::Red } else if l.starts_with("@@") { Color::Cyan } else { Color::Reset };
                    diff.push(Line::styled(l.to_string(), Style::default().fg(colour)));
                }
            }
            _ => diff.push(Line::styled("  no live worktree for this build (it finished, or has not started). r refreshes.", Style::default().fg(Color::DarkGray))),
        }
        f.render_widget(Paragraph::new(diff).block(Block::default().borders(Borders::ALL).title(" Diff against the base commit ")).scroll((self.scroll, 0)), rows[1]);
    }

    fn archives(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(self.state.agents.len() as u16 + 3), Constraint::Min(4)]).split(area);
        let mut t: Vec<Line> = vec![Line::styled(format!(" {:<13}{:<15}{:<24}{:>8}{:>8}{:>8}{:>8}{:>9}", "agent", "state", "model", "in", "cache", "out", "think", "spend"), Style::default().add_modifier(Modifier::BOLD))];
        for (role, a) in &self.state.agents {
            let model = if a.model.is_empty() { "—".to_string() } else { format!("{} {}{}", a.tier, clip(&a.model, 14), if a.upscaled { " ⬆" } else { "" }) };
            t.push(Line::from(vec![
                Span::styled(format!(" {:<13}", self.name(role)), Style::default().fg(role_color(role))),
                Span::styled(format!("{:<15}", a.state), Style::default().fg(state_color(&a.state))),
                Span::raw(format!("{:<24}{:>8}{:>8}{:>8}{:>8}{:>9}", clip(&model, 23), kilo(a.tokens.input), kilo(a.tokens.cache_read), kilo(a.tokens.output), kilo(a.tokens.thinking), money(a.cost_usd))),
            ]));
        }
        f.render_widget(Paragraph::new(t).block(Block::default().borders(Borders::TOP)), rows[0]);
        let mut s: Vec<Line> = Vec::new();
        let prompt = if self.search.editing { format!(" search› {}▏", self.search.query) } else if self.search.query.is_empty() { " press / to search specs, context, REVIEW.md and lessons".to_string() } else { format!(" last search: {}", self.search.query) };
        s.push(Line::styled(prompt, Style::default().add_modifier(Modifier::BOLD)));
        if !self.search.note.is_empty() {
            s.push(Line::styled(format!(" {}", self.search.note), Style::default().fg(Color::DarkGray)));
        }
        for (i, h) in self.search.hits.iter().enumerate() {
            s.push(Line::from(format!(" {}. {}  [{}]", i + 1, sanitize(h["pointer"].as_str().unwrap_or("")), clip(h["heading"].as_str().unwrap_or(""), 60))));
            for l in sanitize(h["preview"].as_str().unwrap_or("")).lines().take(3) {
                s.push(Line::styled(format!("      {}", clip(l, area.width.saturating_sub(8) as usize)), Style::default().fg(Color::DarkGray)));
            }
        }
        f.render_widget(Paragraph::new(s).block(Block::default().borders(Borders::ALL).title(" Knowledge base ")).scroll((self.scroll, 0)), rows[1]);
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
    match e {
        ClientError::Rpc { message, .. } => message.clone(),
        other => other.to_string(),
    }
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
    let mut cmd = match attach(&layout.sock) {
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
    match attach(&layout.sock).and_then(|mut c| c.call("events.subscribe", json!({})).map(|_| c)) {
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
        a.on_key(KeyCode::Right, KeyModifiers::SHIFT);
        a.on_key(KeyCode::Right, KeyModifiers::SHIFT);
        assert_eq!(a.view, View::Council, "wraps around");
        a.on_key(KeyCode::Left, KeyModifiers::SHIFT);
        assert_eq!(a.view, View::Archives);
        a.on_key(KeyCode::Char('2'), KeyModifiers::NONE);
        assert_eq!(a.view, View::Epic);
        assert_eq!(a.on_key(KeyCode::Char('q'), KeyModifiers::NONE), Action::Quit);
        assert_eq!(a.on_key(KeyCode::Char('c'), KeyModifiers::CONTROL), Action::Quit);
        assert_eq!(View::parse("council"), Some(View::Council));
        assert_eq!(View::parse("nonsense"), None);
    }

    #[test]
    fn the_screen_always_shows_the_header_agents_totals_and_where_you_are() {
        let mut a = app();
        let s = render(&a, 110, 24);
        for want in ["DELPHI NARRATOR", "EPIC DAG", "2026-10-24-session", "Athena idle", "Daedalus idle", "Panel 2/4", "Shift+Left: [Council]", "Shift+Right: [Forge Diffs]", "spend $0.000 / $5.00"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        a.connected = false;
        assert!(render(&a, 110, 24).contains("(disconnected)"));
        a.state.spent_usd = 4.6;
        assert!(render(&a, 110, 24).contains("$4.60 / $5.00"));
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
        for want in ["t_1", "design the narrator", "Athena", "Daedalus", "Narrative", "architect is running"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
    }

    #[test]
    fn a_council_is_drawn_as_seat_columns_then_a_verdict_with_the_minority_report() {
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
        for want in ["Daedalus (claude)", "Nemesis (gemini)", "A queue survives restarts.", "Round 2 → DO-NOT-PROCEED", "does not concur", "Escalated to you", "[P0] red-team: Loses jobs on crash", "Minority report:", "001-x.md"] {
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
        for want in ["delphi/t_3", "0123456789ab", "attempt 1", "tests", "3 passed", "waiting for you", "audit P2 naming nit", "src/lib.rs | 2", "-old", "+new evil", "?? src/new.rs"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        assert_eq!(a.clone_action_for_r(), Action::RefreshDiff);
    }

    impl App {
        fn clone_action_for_r(&mut self) -> Action {
            self.on_key(KeyCode::Char('r'), KeyModifiers::NONE)
        }
    }

    #[test]
    fn the_archives_show_every_agent_with_its_model_and_tokens_and_search_the_kb() {
        let mut a = app();
        for e in [
            ev(1, "dag.node.added", "t_1", "architect", json!({"node_id": "n2", "role": "architect", "depth": 0})),
            {
                let mut u = ev(2, "agent.usage", "t_1", "architect", json!({"input_tokens": 1200, "cache_read_tokens": 30000, "cache_write_tokens": 0, "output_tokens": 800, "thinking_tokens": 100, "cost_usd": 0.12}));
                u["span_id"] = json!("n2");
                u["model_tier"] = json!("frontier");
                u["model_id"] = json!("opus-x");
                u
            },
        ] {
            a.state.apply(&e);
        }
        a.view = View::Archives;
        let s = render(&a, 120, 28);
        for want in ["Daedalus", "frontier opus-x", "30.0k", "$0.120", "press / to search"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        // Typing a query is captured by the box, not read as view keys, and Enter asks for the search.
        a.on_key(KeyCode::Char('/'), KeyModifiers::NONE);
        for c in "REV-SEC-04".chars() {
            assert_eq!(a.on_key(KeyCode::Char(c), KeyModifiers::NONE), Action::None);
        }
        assert_eq!(a.view, View::Archives, "typed characters do not switch views");
        assert!(render(&a, 120, 28).contains("search› REV-SEC-04"));
        assert_eq!(a.on_key(KeyCode::Enter, KeyModifiers::NONE), Action::Search("REV-SEC-04".into()));
        a.search.hits = vec![json!({"pointer": "REVIEW.md#L17-L28", "heading": "Security > Rubric", "preview": "- **[REV-SEC-04]** Ratified Rubric Loading\x1b[1m"})];
        let s = render(&a, 120, 28);
        assert!(s.contains("1. REVIEW.md#L17-L28") && s.contains("Ratified Rubric Loading") && !s.contains('\x1b'), "{}", s);
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
}
