//! The inline console (`specs/005c`): `delphi chat` on a terminal.
//!
//! Delphi does not take over the screen. It clears it once, then prints finished lines of chat into the
//! terminal's ordinary scrollback (so selection, search, tmux and screen readers keep working) and keeps one
//! small block pinned at the bottom: a live line, the status header, the prompt and the footer. There is no
//! alternate screen, no boxes and no pager.
//!
//! Commands are the plain console's: both drive [`crate::chat::Session`], so they behave the same. This file is
//! the screen, the prompt editor and the event loop around it.

use crate::chat::{self, parse_input, Out, Reply, Session};
use crate::observer::{badge, clip, hairline, kilo, ordered, role_color, state_color, Pal};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame, TerminalOptions, Viewport,
};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Rows of the pinned block: live line, two header lines, hairline, prompt, hairline, footer.
const BLOCK_ROWS: u16 = 7;

// ── the prompt editor ───────────────────────────────────────────────────────

/// A single-line editor with history. A newline (`Alt+Enter`, or a pasted one) is kept in the text and shown as `↵`.
#[derive(Default)]
pub struct Editor {
    text: Vec<char>,
    cursor: usize,
    history: Vec<String>,
    /// While browsing history: the index shown, and what was being typed.
    browsing: Option<(usize, String)>,
}

impl Editor {
    pub fn insert(&mut self, c: char) {
        if c == '\r' {
            return;
        }
        self.text.insert(self.cursor, c);
        self.cursor += 1;
        self.browsing = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t') {
            self.insert(if c == '\t' { ' ' } else { c });
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.text.remove(self.cursor);
            self.browsing = None;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
            self.browsing = None;
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.text.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn kill_to_end(&mut self) {
        self.text.truncate(self.cursor);
    }

    pub fn kill_line(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.browsing = None;
    }

    /// Delete the word before the cursor (`Ctrl+W`).
    pub fn kill_word(&mut self) {
        while self.cursor > 0 && self.text[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
        while self.cursor > 0 && !self.text[self.cursor - 1].is_whitespace() {
            self.backspace();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// The submitted line; it goes into the history unless it repeats the last one.
    pub fn take(&mut self) -> String {
        let t = self.text();
        if !t.trim().is_empty() && self.history.last() != Some(&t) {
            self.history.push(t.clone());
            if self.history.len() > 500 {
                self.history.remove(0);
            }
        }
        self.kill_line();
        t
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let next = match &self.browsing {
            None => (self.history.len() - 1, self.text()),
            Some((i, draft)) => (i.saturating_sub(1), draft.clone()),
        };
        self.text = self.history[next.0].chars().collect();
        self.cursor = self.text.len();
        self.browsing = Some(next);
    }

    pub fn history_next(&mut self) {
        let Some((i, draft)) = self.browsing.clone() else { return };
        if i + 1 < self.history.len() {
            self.text = self.history[i + 1].chars().collect();
            self.browsing = Some((i + 1, draft));
        } else {
            self.text = draft.chars().collect();
            self.browsing = None;
        }
        self.cursor = self.text.len();
    }

    /// What to draw in a prompt `width` columns wide: the visible slice (scrolled so the cursor shows) and the
    /// cursor's column within it.
    pub fn view(&self, width: usize) -> (String, usize) {
        let shown: Vec<char> = self.text.iter().map(|c| if *c == '\n' { '↵' } else { *c }).collect();
        let w = |cs: &[char]| cs.iter().map(|c| c.width().unwrap_or(0)).sum::<usize>();
        let room = width.saturating_sub(1).max(1);
        // Drop characters from the left until the cursor fits.
        let mut start = 0;
        while start < self.cursor && w(&shown[start..self.cursor]) > room {
            start += 1;
        }
        let mut end = shown.len();
        while end > start && w(&shown[start..end]) > room + 1 && end > self.cursor {
            end -= 1;
        }
        (shown[start..end].iter().collect(), w(&shown[start..self.cursor]))
    }
}

// ── output: wrapping and line assembly ──────────────────────────────────────

/// Break `text` into screen lines no wider than `width` columns, at spaces where there is one.
pub fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.trim_end_matches('\r').replace('\t', "    ");
        if raw.width() <= width {
            out.push(raw);
            continue;
        }
        // Continuation lines keep the indent of the first.
        let indent: String = raw.chars().take_while(|c| *c == ' ').collect();
        let (mut line, mut used) = (String::new(), 0usize);
        for word in raw.split_inclusive(' ') {
            let ww = word.width();
            if used + ww > width && used > indent.width() {
                out.push(line.trim_end().to_string());
                line = indent.clone();
                used = indent.width();
            }
            // A word longer than a whole line is cut.
            for ch in word.chars() {
                let cw = ch.width().unwrap_or(0);
                if used + cw > width {
                    out.push(line.clone());
                    line = indent.clone();
                    used = indent.width();
                }
                line.push(ch);
                used += cw;
            }
        }
        out.push(line.trim_end().to_string());
    }
    out
}

/// Text arrives in pieces (live deltas have no trailing newline). Whole lines can be printed; the unfinished
/// tail waits, and is shown on the live row.
#[derive(Default)]
pub struct Assembler {
    partial: String,
}

impl Assembler {
    /// Add text; returns the lines it completed.
    pub fn push(&mut self, s: &str) -> Vec<String> {
        self.partial.push_str(s);
        let Some(cut) = self.partial.rfind('\n') else { return vec![] };
        let mut done: String = self.partial.drain(..=cut).collect();
        done.pop(); // the newline that ended the last line; blank lines before it stay
        done.split('\n').map(String::from).collect()
    }

    pub fn tail(&self) -> &str {
        &self.partial
    }

    /// The unfinished line, as a finished one (the task ended mid-line).
    pub fn flush(&mut self) -> Option<String> {
        (!self.partial.is_empty()).then(|| std::mem::take(&mut self.partial))
    }
}

// ── the pinned block ────────────────────────────────────────────────────────

/// Everything the pinned block shows.
pub struct View<'a> {
    pub editor: &'a Editor,
    pub live: &'a str,
    pub obs: &'a delphi_engine::observe::ObserverState,
    pub names: &'a HashMap<String, String>,
    pub mission: &'a str,
    pub cwd: &'a str,
    pub waiting: Option<&'a str>,
    pub running: usize,
    pub mode: &'a str,
    pub pal: Pal,
    pub notice: Option<&'a str>,
}

fn mode_label(mode: &str) -> &str {
    match mode {
        "accept-edits" => "edits auto",
        "mission-auto" => "auto",
        "optimistic" => "optimistic",
        _ => "approvals",
    }
}

/// `120k tokens` style, with the spec's wording.
fn tokens_label(n: u64) -> String {
    format!("{} tokens", kilo(n))
}

pub fn draw_block(f: &mut Frame, area: Rect, v: &View) {
    let pal = v.pal;
    let rows = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(1); BLOCK_ROWS as usize]).split(area);
    // The live row: the tail of whatever is being written, dim, so output never jumps while it streams.
    let tail: String = v.live.chars().rev().take(area.width.saturating_sub(2) as usize).collect::<Vec<_>>().into_iter().rev().collect();
    f.render_widget(Paragraph::new(Span::styled(format!(" {}", tail.replace('\n', " ")), Style::default().fg(pal.dim()))), rows[0]);

    // Header line 1: the priority line (decision alert, the working agent, or everyone idle).
    let name = |role: &str| v.names.get(role).cloned().unwrap_or_else(|| role.to_string());
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    let agents = ordered(v.obs);
    if let Some(who) = v.waiting {
        let role = agents.iter().find(|(_, a)| a.state == "waiting on you").map(|(r, _)| r.as_str()).unwrap_or("red-team");
        spans.extend(badge(role, "waiting on you", pal));
        spans.push(Span::styled(format!(" {}: decision needed · {}", name(role), who), Style::default().fg(Color::Rgb(239, 68, 68)).add_modifier(Modifier::BOLD)));
    } else if let Some((role, _)) = agents.iter().find(|(_, a)| a.state == "running") {
        spans.extend(badge(role, "running", pal));
        let more = if v.running > 1 { format!(" · {} tasks running", v.running) } else { String::new() };
        spans.push(Span::styled(format!(" {}: working{}", name(role), more), Style::default().fg(pal.text())));
    } else {
        for (role, a) in &agents {
            spans.extend(badge(role, &a.state, pal));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(" all agents idle", Style::default().fg(pal.dim())));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
    // Header line 2: branding.
    f.render_widget(Paragraph::new(Span::styled(format!(" delphi v{} · {} · mission {}", env!("CARGO_PKG_VERSION"), v.cwd, v.mission), Style::default().fg(pal.dim()))), rows[2]);
    f.render_widget(Paragraph::new(hairline("", area.width, pal)), rows[3]);

    // The prompt.
    let (text, col) = v.editor.view(area.width.saturating_sub(3) as usize);
    f.render_widget(Paragraph::new(Line::from(vec![Span::styled(" › ", Style::default().fg(Color::Cyan)), Span::styled(text, Style::default().fg(pal.text()))])), rows[4]);
    f.set_cursor_position((rows[4].x + 3 + col as u16, rows[4].y));
    f.render_widget(Paragraph::new(hairline("", area.width, pal)), rows[5]);

    // The footer: tokens, approval mode, the shortcut hint. No money here (that is `delphi observe spend`).
    let left = format!(" {} · {} · shortcuts: ?", tokens_label(v.obs.tokens.total()), mode_label(v.mode));
    let room = (area.width as usize).saturating_sub(left.width() + 2);
    let right = v.notice.map(|n| format!("{} ", clip(n, room.min(60)))).unwrap_or_default();
    let gap = (area.width as usize).saturating_sub(left.width() + right.width()).max(1);
    f.render_widget(Paragraph::new(Line::from(vec![Span::styled(left, Style::default().fg(pal.dim())), Span::raw(" ".repeat(gap)), Span::styled(right, Style::default().fg(Color::Yellow))])), rows[6]);
    let _ = (role_color, state_color); // reserved for the inbox slice
}

// ── the event loop ──────────────────────────────────────────────────────────

fn print_lines(term: &mut ratatui::DefaultTerminal, lines: &[String], width: usize, pal: Pal) -> std::io::Result<()> {
    let wrapped: Vec<String> = lines.iter().flat_map(|l| wrap_lines(l, width)).collect();
    if wrapped.is_empty() {
        return Ok(());
    }
    for chunk in wrapped.chunks(200) {
        let rows: Vec<Line> = chunk.iter().map(|l| style_line(l, pal)).collect();
        let n = rows.len() as u16;
        term.insert_before(n, |buf| Paragraph::new(rows).render(buf.area, buf))?;
    }
    Ok(())
}

use ratatui::widgets::Widget;

/// A little structure for plain text: section rules and the user's own echoed lines are dimmed.
fn style_line(l: &str, pal: Pal) -> Line<'static> {
    if l.starts_with("──") || l.starts_with("› ") {
        Line::styled(l.to_string(), Style::default().fg(pal.dim()))
    } else {
        Line::styled(l.to_string(), Style::default().fg(pal.text()))
    }
}

/// Run the inline console. Returns the process exit code.
/// A reply longer than this many screen lines is cut; the rest is saved and offered as a link and `/more`.
pub const REPLY_LINES: usize = 24;

/// Split `lines` into what is shown and what is held back.
pub fn cap_lines(lines: Vec<String>, cap: usize) -> (Vec<String>, Vec<String>) {
    if lines.len() <= cap {
        return (lines, vec![]);
    }
    let mut shown = lines;
    let rest = shown.split_off(cap);
    (shown, rest)
}

/// `HH:MM:SS` in local time.
fn clock() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// Keep a whole reply as a document in the mission (`missions/<mission>/replies/<task>.md`). It is the agent's own
/// output, so it is marked as generated, and it carries the frontmatter every workspace document has.
pub fn save_reply(ws_dir: &Path, mission: &str, r: &Reply, now_rfc3339: &str) -> std::io::Result<std::path::PathBuf> {
    let dir = ws_dir.join("local/missions").join(mission).join("replies");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.md", r.task_id));
    let doc = format!(
        "---\nid: reply-{task}\ntitle: \"Reply to {task}\"\nauthor: {role}\ntimestamp: {now}\nsource_type: internal_generated\nconfidence: medium\nstatus: active\nsource_mission: {mission}\ntags: [reply]\n---\n\n{text}\n",
        task = r.task_id,
        role = r.role,
        now = now_rfc3339,
        mission = mission,
        text = r.text.trim_end()
    );
    std::fs::write(&path, doc)?;
    Ok(path)
}

fn now_rfc3339() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as libc::time_t).unwrap_or(0);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::gmtime_r(&t, &mut tm) };
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday, tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// Print one finished reply: who said it and when, at most [`REPLY_LINES`] lines of it, how to get the rest,
/// and how the task ended.
fn print_reply(term: &mut ratatui::DefaultTerminal, session: &mut Session, r: &Reply, width: usize, pal: Pal) {
    let body: Vec<String> = wrap_lines(r.text.trim_end(), width);
    let (shown, rest) = cap_lines(body, REPLY_LINES);
    let mut rows: Vec<Line> = Vec::new();
    let mut badge_spans = vec![Span::raw("")];
    badge_spans.extend(badge(&r.role, if r.status == "escalated" { "waiting on you" } else { "idle" }, pal));
    badge_spans.push(Span::styled(format!(" {} · {}", r.name, clock()), Style::default().fg(pal.dim())));
    rows.push(Line::from(vec![Span::raw("")]));
    rows.push(Line::from(badge_spans));
    rows.extend(shown.iter().map(|l| Line::styled(l.clone(), Style::default().fg(pal.text()))));
    let mut link: Option<(usize, String, String, usize)> = None; // (line index, path text, url, column)
    if !rest.is_empty() {
        let logical = format!("missions/{}/replies/{}.md", session.mission_id, r.task_id);
        let prefix = format!("… {} more line{} · ", rest.len(), if rest.len() == 1 { "" } else { "s" });
        match save_reply(session.ws_dir(), &session.mission_id, r, &now_rfc3339()) {
            Ok(p) => {
                link = Some((rows.len(), logical.clone(), crate::observer::file_url(&p), prefix.width()));
                rows.push(Line::styled(format!("{}{}   /more prints them", prefix, logical), Style::default().fg(pal.dim())));
            }
            Err(_) => rows.push(Line::styled(format!("{}/more prints them", prefix), Style::default().fg(pal.dim()))),
        }
    }
    *session.shared.more.lock().unwrap() = rest;
    let end = if r.status == "escalated" { format!("! {} needs you", r.task_id) } else { format!("✓ {} done", r.task_id) };
    rows.push(Line::styled(end, Style::default().fg(if r.status == "escalated" { Color::Yellow } else { pal.dim() })));
    let n = rows.len() as u16;
    let total = rows.len();
    let _ = term.insert_before(n, |buf| Paragraph::new(rows).render(buf.area, buf));
    // The saved path becomes a link, written over the cells just printed.
    if let Some((index, text, url, col)) = link {
        let top = term.get_frame().area().y as usize;
        let y = top.saturating_sub(total - index);
        let seq = crate::observer::osc8(&[crate::observer::Link { x: col as u16, y: y as u16, text, url }]);
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(seq.as_bytes());
        let _ = out.flush();
    }
}

pub fn run(home: &Path, ws_dir: &Path, mission: Option<String>, names: HashMap<String, String>, notice: Option<&str>) -> i32 {
    let pal = Pal { dark: crate::observer::detect_dark() };
    // One clear at the start, then never again: from here on the screen only grows.
    {
        use std::io::Write;
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[2J\x1b[H");
        let _ = out.flush();
    }
    // A terminal that cannot report its cursor position cannot host a pinned block: use the plain console.
    let mut term = match ratatui::try_init_with_options(TerminalOptions { viewport: Viewport::Inline(BLOCK_ROWS) }) {
        Ok(t) => t,
        Err(e) => {
            let _ = ratatui::try_restore();
            eprintln!("delphi: this terminal cannot host the inline console ({}); using the plain console.", e);
            return chat::run(home, ws_dir, mission, names);
        }
    };
    let (tx, rx) = mpsc::channel::<Out>();
    chat::set_sink(Some(tx));
    let mut session = match chat::connect(home, ws_dir, mission, names.clone()) {
        Ok(s) => s,
        Err(code) => {
            chat::set_sink(None);
            let _ = term.clear();
            let _ = ratatui::try_restore();
            return code;
        }
    };
    if let Some(n) = notice {
        chat::say(&format!("{}\n", n));
    }
    {
        use ratatui::crossterm::{event::EnableBracketedPaste, execute};
        let _ = execute!(std::io::stdout(), EnableBracketedPaste);
    }
    let code = event_loop(&mut term, &mut session, &rx, &names, pal);
    {
        use ratatui::crossterm::{event::DisableBracketedPaste, execute};
        let _ = execute!(std::io::stdout(), DisableBracketedPaste);
    }
    let _ = term.clear();
    let _ = ratatui::try_restore();
    chat::set_sink(None);
    println!();
    code
}

fn event_loop(term: &mut ratatui::DefaultTerminal, session: &mut Session, rx: &mpsc::Receiver<Out>, names: &HashMap<String, String>, pal: Pal) -> i32 {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    let (mut editor, mut asm, mut live) = (Editor::default(), Assembler::default(), String::new());
    let cwd = std::env::current_dir().map(|p| p.display().to_string().replace(&std::env::var("HOME").unwrap_or_default(), "~")).unwrap_or_default();
    let mut mode = session.permission_mode();
    let (mut dirty, mut last_ctrl_c, mut notice): (bool, Option<Instant>, Option<(String, Instant)>) = (true, None, None);
    loop {
        // Output first: finished lines go above the block; the unfinished one stays on the live row.
        let width = term.size().map(|s| s.width as usize).unwrap_or(80).saturating_sub(1);
        let mut lines: Vec<String> = Vec::new();
        let mut replies: Vec<Reply> = Vec::new();
        while let Ok(o) = rx.try_recv() {
            match o {
                Out::Text(s) => lines.extend(asm.push(&s)),
                Out::Live(s) => {
                    live.push_str(&s);
                    if live.len() > 4000 {
                        let cut = live.len() - 2000;
                        let at = (cut..live.len()).find(|i| live.is_char_boundary(*i)).unwrap_or(live.len());
                        live.drain(..at);
                    }
                }
                Out::LiveClear => live.clear(),
                Out::Reply(r) => {
                    // Anything printed before the reply stays before it.
                    if !lines.is_empty() {
                        let _ = print_lines(term, &std::mem::take(&mut lines), width, pal);
                    }
                    replies.push(r);
                }
            }
            dirty = true;
        }
        // A task that has finished leaves nothing half-written behind.
        if session.shared.running.load(Ordering::SeqCst) == 0 {
            lines.extend(asm.flush());
        }
        if !lines.is_empty() && print_lines(term, &lines, width, pal).is_err() {
            return 1;
        }
        for r in replies {
            print_reply(term, session, &r, width, pal);
        }
        if session.shared.closed.load(Ordering::SeqCst) {
            let _ = print_lines(term, &["(the daemon connection closed)".to_string()], width, pal);
            return 1;
        }
        if dirty {
            let obs = session.shared.obs.lock().unwrap().clone();
            let waiting = session.shared.card.lock().unwrap().as_ref().map(|c| c.tier.clone()).map(|t| if t == "red" { "red-tier; /y /n /d".to_string() } else { "/y /a /n /d".to_string() });
            let running = session.shared.running.load(Ordering::SeqCst);
            let note = notice.as_ref().filter(|(_, t)| t.elapsed() < Duration::from_secs(4)).map(|(n, _)| n.as_str());
            let view = View { editor: &editor, live: if live.is_empty() { asm.tail() } else { &live }, obs: &obs, names, mission: &session.mission_id, cwd: &cwd, waiting: waiting.as_deref(), running, mode: &mode, pal, notice: note };
            if term.draw(|f| draw_block(f, f.area(), &view)).is_err() {
                return 1;
            }
            dirty = false;
        }
        if !event::poll(Duration::from_millis(40)).unwrap_or(false) {
            // Time passes even when nothing is typed: a spinner-free refresh keeps the header honest.
            dirty |= notice.as_ref().is_some_and(|(_, t)| t.elapsed() > Duration::from_secs(4));
            dirty |= session.shared.running.load(Ordering::SeqCst) > 0 && Instant::now().elapsed().as_millis() % 2 == 0;
            continue;
        }
        let ev = match event::read() {
            Ok(e) => e,
            Err(_) => return 1,
        };
        dirty = true;
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                let (ctrl, alt) = (k.modifiers.contains(KeyModifiers::CONTROL), k.modifiers.contains(KeyModifiers::ALT));
                match k.code {
                    KeyCode::Enter if alt => editor.insert('\n'),
                    KeyCode::Enter => {
                        let text = editor.take();
                        if text.trim().is_empty() {
                            continue;
                        }
                        // The line the operator sent stays in the scrollback as they typed it.
                        let echo: Vec<String> = text.lines().enumerate().map(|(i, l)| if i == 0 { format!("› {}", l) } else { format!("  {}", l) }).collect();
                        let _ = print_lines(term, &echo, width, pal);
                        let input = parse_input(&text);
                        let was_permissions = matches!(input, chat::Input::Permissions(Some(_)));
                        if !session.handle(input) {
                            return 0;
                        }
                        if was_permissions {
                            mode = session.permission_mode();
                        }
                    }
                    KeyCode::Char('c') if ctrl => {
                        if !editor.is_empty() {
                            editor.kill_line();
                        } else if last_ctrl_c.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
                            return 0;
                        } else {
                            last_ctrl_c = Some(Instant::now());
                            notice = Some(("press Ctrl+C again to quit".into(), Instant::now()));
                        }
                    }
                    KeyCode::Char('d') if ctrl && editor.is_empty() => return 0,
                    KeyCode::Char('a') if ctrl => editor.home(),
                    KeyCode::Char('e') if ctrl => editor.end(),
                    KeyCode::Char('u') if ctrl => editor.kill_line(),
                    KeyCode::Char('k') if ctrl => editor.kill_to_end(),
                    KeyCode::Char('w') if ctrl => editor.kill_word(),
                    KeyCode::Char('?') if editor.is_empty() => {
                        let _ = print_lines(term, &chat::HELP.lines().map(String::from).collect::<Vec<_>>(), width, pal);
                    }
                    KeyCode::Char(c) if !ctrl => editor.insert(c),
                    KeyCode::Backspace => editor.backspace(),
                    KeyCode::Delete => editor.delete(),
                    KeyCode::Left => editor.left(),
                    KeyCode::Right => editor.right(),
                    KeyCode::Home => editor.home(),
                    KeyCode::End => editor.end(),
                    KeyCode::Up => editor.history_prev(),
                    KeyCode::Down => editor.history_next(),
                    KeyCode::Esc => editor.kill_line(),
                    _ => {}
                }
            }
            Event::Paste(s) => editor.insert_str(&s),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use serde_json::json;

    fn typed(s: &str) -> Editor {
        let mut e = Editor::default();
        for c in s.chars() {
            e.insert(c);
        }
        e
    }

    #[test]
    fn the_editor_edits_where_the_cursor_is() {
        let mut e = typed("hello world");
        e.left();
        e.left();
        e.insert('X');
        assert_eq!(e.text(), "hello worXld");
        e.home();
        e.delete();
        assert_eq!(e.text(), "ello worXld");
        e.end();
        e.kill_word();
        assert_eq!(e.text(), "ello ");
        e.kill_word();
        assert_eq!(e.text(), "");
        let mut e = typed("abc def");
        e.left();
        e.left();
        e.left();
        e.kill_to_end();
        assert_eq!(e.text(), "abc ");
        e.backspace();
        e.backspace();
        assert_eq!(e.text(), "ab");
        let mut e = typed("é日本");
        e.backspace();
        assert_eq!(e.text(), "é日", "multi-byte characters delete whole");
    }

    #[test]
    fn history_recalls_earlier_lines_and_restores_the_draft() {
        let mut e = Editor::default();
        for l in ["one", "two", "two", "three"] {
            e.insert_str(l);
            assert_eq!(e.take(), l);
        }
        e.insert_str("draft");
        e.history_prev();
        assert_eq!(e.text(), "three");
        e.history_prev();
        assert_eq!(e.text(), "two", "a repeated line is stored once");
        e.history_prev();
        assert_eq!(e.text(), "one");
        e.history_prev();
        assert_eq!(e.text(), "one", "stops at the oldest");
        e.history_next();
        e.history_next();
        e.history_next();
        assert_eq!(e.text(), "draft", "past the newest comes the line being typed");
        assert!(Editor::default().take().is_empty());
    }

    #[test]
    fn pasted_text_keeps_newlines_shows_them_as_a_mark_and_drops_control_characters() {
        let mut e = Editor::default();
        e.insert_str("line one\r\nline\ttwo\x1b[2J\x07");
        assert_eq!(e.text(), "line one\nline two[2J", "escape and bell are dropped");
        assert!(e.view(80).0.contains('↵') && !e.view(80).0.contains('\n'));
    }

    #[test]
    fn the_prompt_scrolls_to_keep_the_cursor_visible() {
        let e = typed(&"x".repeat(100));
        let (shown, col) = e.view(20);
        assert!(shown.chars().count() <= 20 && col <= 19, "{} {}", shown.chars().count(), col);
        let mut e = typed("short");
        e.home();
        assert_eq!(e.view(20), ("short".to_string(), 0));
        let wide = typed(&"日".repeat(30));
        let (_, col) = wide.view(21);
        assert!(col <= 20, "double-width characters count as two columns: {}", col);
    }

    #[test]
    fn long_lines_wrap_at_spaces_keep_their_indent_and_never_exceed_the_width() {
        let text = "  the quick brown fox jumps over the lazy dog and keeps running far away";
        let lines = wrap_lines(text, 30);
        assert!(lines.len() >= 3 && lines.iter().all(|l| l.width() <= 30), "{:?}", lines);
        assert!(lines.iter().all(|l| l.starts_with("  ")), "continuations keep the indent: {:?}", lines);
        assert_eq!(wrap_lines("short", 30), vec!["short"]);
        assert_eq!(wrap_lines("a\n\nb", 30), vec!["a", "", "b"], "blank lines survive");
        let long = wrap_lines(&"x".repeat(70), 20);
        assert!(long.iter().all(|l| l.width() <= 20) && long.concat() == "x".repeat(70), "an unbreakable word is cut");
        assert!(wrap_lines(&"日".repeat(20), 10).iter().all(|l| l.width() <= 10));
    }

    #[test]
    fn text_is_printed_a_whole_line_at_a_time_and_the_tail_waits_on_the_live_row() {
        let mut a = Assembler::default();
        assert!(a.push("The file ").is_empty() && a.tail() == "The file ");
        assert_eq!(a.push("defines `needle`.\nIt retu"), vec!["The file defines `needle`."]);
        assert_eq!(a.tail(), "It retu");
        assert_eq!(a.push("rns 42.\n\nDone"), vec!["It returns 42.", "", ]);
        assert_eq!(a.flush(), Some("Done".to_string()));
        assert_eq!(a.flush(), None);
    }

    fn view<'a>(e: &'a Editor, obs: &'a delphi_engine::observe::ObserverState, names: &'a HashMap<String, String>, waiting: Option<&'a str>, running: usize) -> View<'a> {
        View { editor: e, live: "", obs, names, mission: "2026-10-24-session", cwd: "~/projects/my-project", waiting, running, mode: "default", pal: Pal { dark: true }, notice: None }
    }

    fn render(v: &View, w: u16) -> String {
        let mut t = Terminal::new(TestBackend::new(w, BLOCK_ROWS)).unwrap();
        t.draw(|f| draw_block(f, f.area(), v)).unwrap();
        crate::observer::buffer_text(t.backend().buffer())
    }

    fn obs(roles: &[&str]) -> delphi_engine::observe::ObserverState {
        delphi_engine::observe::ObserverState::new(&roles.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn the_pinned_block_has_a_header_a_prompt_and_a_footer_without_boxes_or_money() {
        let names: HashMap<String, String> = [("orchestrator", "Athena")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let mut o = obs(&["orchestrator", "architect", "builder", "red-team", "strategist"]);
        o.tokens.input = 42_100;
        let e = typed("what does @project/x do");
        let s = render(&view(&e, &o, &names, None, 0), 100);
        for want in ["all agents idle", "⟦•─•⟧", "delphi v", "~/projects/my-project", "mission 2026-10-24-session", "› what does @project/x do", "42.1k tokens · approvals · shortcuts: ?"] {
            assert!(s.contains(want), "missing `{}` in\n{}", want, s);
        }
        for bad in ['┌', '┐', '└', '┘', '│', '$'] {
            assert!(!s.contains(bad), "{} in\n{}", bad, s);
        }
        assert_eq!(s.lines().count(), BLOCK_ROWS as usize);
        assert!(s.lines().nth(3).unwrap().starts_with("────") && s.lines().nth(5).unwrap().starts_with("────"), "hairlines frame the prompt:\n{}", s);
    }

    #[test]
    fn the_header_shows_the_working_agent_or_a_waiting_decision() {
        let names: HashMap<String, String> = [("orchestrator", "Athena"), ("red-team", "Nemesis")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let mut o = obs(&["orchestrator", "red-team"]);
        o.apply(&json!({"seq": 1, "type": "dag.node.added", "task_id": "t_1", "agent": "orchestrator", "payload": {"node_id": "n1", "role": "orchestrator", "depth": 0}}));
        let e = Editor::default();
        let working = render(&view(&e, &o, &names, None, 2), 100);
        assert!(working.contains("Athena: working · 2 tasks running") && !working.contains("all agents idle"), "{}", working);
        o.apply(&json!({"seq": 2, "type": "approval.requested", "task_id": "t_1", "agent": "red-team", "payload": {"approval_id": "ap_1", "role": "red-team", "summary": "merge"}}));
        let alert = render(&view(&e, &o, &names, Some("red-tier; /y /n /d"), 1), 100);
        assert!(alert.contains("⟦x?x⟧ Nemesis: decision needed · red-tier; /y /n /d"), "{}", alert);
    }

    #[test]
    fn the_live_row_shows_the_tail_of_what_is_being_written_and_the_footer_shows_the_approval_mode() {
        let names = HashMap::new();
        let o = obs(&["orchestrator"]);
        let e = Editor::default();
        let mut v = view(&e, &o, &names, None, 1);
        let long = format!("{} and the very end", "words ".repeat(40));
        v.live = &long;
        v.mode = "accept-edits";
        v.notice = Some("press Ctrl+C again to quit");
        let s = render(&v, 80);
        assert!(s.lines().next().unwrap().ends_with("and the very end"), "{}", s);
        assert!(s.contains("edits auto") && s.contains("press Ctrl+C again to quit"), "{}", s);
    }

    #[test]
    fn nothing_panics_in_a_tiny_terminal() {
        let names = HashMap::new();
        let o = obs(&["orchestrator", "red-team"]);
        let e = typed(&"x".repeat(200));
        for w in [1u16, 5, 12, 40] {
            let _ = render(&view(&e, &o, &names, Some("x"), 3), w);
        }
    }

    #[test]
    fn a_long_reply_is_cut_at_the_cap_and_the_rest_is_kept() {
        let lines: Vec<String> = (1..=60).map(|i| format!("line {}", i)).collect();
        let (shown, rest) = cap_lines(lines.clone(), REPLY_LINES);
        assert_eq!((shown.len(), rest.len(), shown[23].as_str(), rest[0].as_str()), (24, 36, "line 24", "line 25"));
        assert_eq!([shown, rest].concat(), lines, "nothing is lost");
        let (all, none) = cap_lines(lines[..24].to_vec(), REPLY_LINES);
        assert_eq!((all.len(), none.len()), (24, 0), "exactly the cap is shown whole");
    }

    #[test]
    fn a_saved_reply_is_a_proper_workspace_document_with_the_whole_text() {
        let d = tempfile::tempdir().unwrap();
        let r = Reply { task_id: "t_0007".into(), role: "architect".into(), name: "Daedalus".into(), text: "# Plan\n\nstep one\nstep two\n".into(), status: "done".into() };
        let path = save_reply(d.path(), "m1", &r, "2026-10-24T12:00:00Z").unwrap();
        assert!(path.ends_with("local/missions/m1/replies/t_0007.md"));
        let text = std::fs::read_to_string(&path).unwrap();
        let (fm, body) = delphi_engine::workspace::split_frontmatter(&text);
        let fm = fm.expect("frontmatter");
        for (k, v) in [("author", "architect"), ("source_type", "internal_generated"), ("status", "active"), ("source_mission", "m1")] {
            assert_eq!(fm.get(k).and_then(|x| x.as_str()), Some(v), "{}", k);
        }
        assert!(body.contains("# Plan") && body.contains("step two"));
        assert!(clock().len() == 8 && now_rfc3339().ends_with('Z'));
    }

    #[test]
    fn model_text_cannot_reach_the_screen_with_control_sequences() {
        let (tx, rx) = std::sync::mpsc::channel();
        chat::set_sink(Some(tx));
        chat::say("ok \x1b[2J\x1b]8;;http://evil\x07link\x1b]8;;\x07 done\n");
        chat::set_sink(None);
        assert_eq!(rx.try_recv().unwrap(), Out::Text("ok link done\n".to_string()));
    }
}
