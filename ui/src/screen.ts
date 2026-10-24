// The screen as data: a pure function from the console's model to exactly `h` lines of `w` columns. Ink only paints
// what this returns, so the whole look is testable without a terminal (see test/screen.test.ts).
import { renderMarkdown } from './markdown.ts';
import { layout, type Composer } from './composer.ts';
import type { Item } from './complete.ts';
import { status, type Block, type State } from './fold.ts';
import * as T from './theme.ts';
import * as I from './inbox.ts';
import * as P from './panel.ts';
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';

export type Model = {
  info: T.Info; pal: T.Pal; state: State; now: number; deck: 0 | 1 | 2;
  composer: Composer; back: number; help: boolean;
  menu: { items: Item[]; sel: number } | null;
  expandTools: boolean;
  search: { q: string; match: string | null } | null;
  queue: string[];
  inbox: I.Inbox;
  panel: P.Panel | null;
  /** permissions mode: default, accept-edits or mission-auto */
  mode: string;
};

const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'];
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8);
const dots = (deck: number) => [0, 1, 2].map((i) => (i === deck ? T.accent('●') : T.dim('○'))).join(' ');
const nav = (deck: number, w = 100) => (w >= 72 ? T.dim('navigate: shift+arrows · ') : '') + dots(deck);

export function header(m: Model, w: number): string[] {
  const st = status(m.state, m.now);
  let left: string;
  if (st.kind === 'alert') {
    const who = m.info.roles['red-team'] ? 'red-team' : st.role;
    const n = Object.keys(m.state.approvals).length;
    left = T.badge(who, 'waiting on you', m.info, m.pal) + ' ' + T.crimson(chalk.bold(T.roleName(who, m.info) + (n > 1 ? `: ${n} decisions pending` : ': decision needed'))) + T.dim(` · ${String(Math.floor(st.secs / 60)).padStart(2, '0')}:${String(st.secs % 60).padStart(2, '0')} remaining`);
  } else if (st.kind === 'active') {
    left = T.badge(st.role, 'running', m.info, m.pal) + ' ' + T.roleColor(st.role, m.info).bold(T.roleName(st.role, m.info) + ':') + ' ' + T.text(m.pal)(st.doing || 'working');
  } else {
    const roles = ORDER.filter((r) => m.info.roles[r]);
    left = roles.map((r) => T.badge(r, m.state.agents[r]?.state ?? 'idle', m.info, m.pal)).join(' ') + T.dim('  all agents idle');
  }
  const l2 = chalk.bold(T.text(m.pal)('delphi')) + T.dim(` v${m.info.version} · ${T.tilde(m.info.cwd)}`);
  return [T.spread(left, nav(m.deck, w), w), T.clip(l2, w)];
}

const MODE: Record<string, string> = { default: 'approvals', 'accept-edits': 'accept edits', 'mission-auto': 'auto' };

export function footer(m: Model, w: number): string {
  const k = m.state.tokens >= 1000 ? (m.state.tokens / 1000).toFixed(1) + 'k' : String(m.state.tokens);
  const pending = Object.keys(m.state.approvals).length;
  const left = T.dim(`${k} tokens · ${MODE[m.mode] ?? m.mode}${pending ? ` · ${pending} waiting` : ''} · shortcuts: ?`);
  return T.spread(left, nav(m.deck, w), w);
}

const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true, trim: false }).split('\n');

function render(b: Block, m: Model, w: number): string[] {
  if (b.kind === 'user') {
    const lines = b.text.split('\n').flatMap((l) => wrap(l, w - 2));
    return ['', ...lines.map((l, i) => (i === 0 ? T.dim('❯ ') : '  ') + T.text(m.pal)(l))];
  }
  if (b.kind === 'note') return ['', ...b.text.split('\n').flatMap((l) => wrap(l, w - 2)).map((l) => '  ' + T.dim(l))];
  if (b.kind === 'card') {
    const red = b.status === 'red';
    const who = T.roleName(b.role ?? 'orchestrator', m.info);
    const head = (red ? T.crimson : T.warn)(`── approval needed [${red ? 'RED — never auto-approved' : 'amber'}]`) + T.dim(` · ${who}`);
    const keys = T.dim(`/y approve once${red ? '' : ' · /a allow similar this task'} · /n reject · /d view all`);
    return ['', head, ...b.text.split('\n').flatMap((l) => wrap(l, w - 2)).map((l) => '  ' + l), '  ' + keys];
  }
  if (b.kind === 'tool') {
    const mark = b.status === 'error' ? T.crimson('✖') : b.status === 'ok' ? T.dim('✔') : T.dim('…');
    const line = '  ' + T.dim('→ ' + b.text) + ' ' + mark;
    if (!m.expandTools || !b.detail) return [T.clip(line, w)];
    return [T.clip(line, w), ...b.detail.split('\n').flatMap((l) => wrap(l, w - 6)).map((l) => '      ' + T.dim(l))];
  }
  const role = b.role ?? 'orchestrator';
  const head = T.badge(role, b.status === 'escalated' ? 'escalated' : 'idle', m.info, m.pal) + ' ' + T.roleColor(role, m.info).bold(T.roleName(role, m.info)) + T.dim(` · ${role} · ${clock(b.at)}`);
  return ['', head, ...renderMarkdown(b.text, w - 2).map((l) => '  ' + l)];
}

// Finished blocks never change, so each is rendered once per width and palette. A long session stays cheap.
const cache = new WeakMap<Block, { key: string; lines: string[] }>();
const rolesKey = new WeakMap<object, string>();
function block(b: Block, m: Model, w: number): string[] {
  let rk = rolesKey.get(m.info.roles);
  if (rk === undefined) { rk = JSON.stringify(m.info.roles); rolesKey.set(m.info.roles, rk); }
  const key = `${w}:${m.pal.dark}:${m.expandTools}:${b.status}:${b.detail?.length ?? 0}:${rk}`;
  const hit = cache.get(b);
  if (hit && hit.key === key) return hit.lines;
  const lines = render(b, m, w);
  cache.set(b, { key, lines });
  return lines;
}

/** Every transcript line, oldest first. The unfinished reply is dim at the bottom. */
/** The transcript as pieces (each finished block is cached), so a long session costs a sum, not a copy. */
function parts(m: Model, w: number): string[][] {
  const out: string[][] = m.state.blocks.map((b) => block(b, m, w));
  for (const q of m.queue) out.push(['', T.dim('❯ ' + q.split('\n')[0] + '  (queued)')]);
  if (m.state.live) out.push(['', ...renderMarkdown(m.state.live.slice(-3000), w - 2).map((l) => '  ' + chalk.dim(l))]);
  return out;
}

export function transcript(m: Model, w: number): string[] {
  return parts(m, w).flat();
}

/** How many lines the transcript has at this width. */
export function transcriptCount(m: Model, w: number): number {
  return parts(m, w).reduce((n, p) => n + p.length, 0);
}

/** Lines [start, end) of the transcript without building the rest of it. */
function windowOf(ps: string[][], start: number, end: number): string[] {
  const out: string[] = [];
  let at = 0;
  for (const p of ps) {
    const next = at + p.length;
    if (next > start && at < end) out.push(...p.slice(Math.max(0, start - at), Math.min(p.length, end - at)));
    at = next;
    if (at >= end) break;
  }
  return out;
}

const HELP = [
  'keys',
  '  enter send · alt+enter or ctrl+j newline · esc clear · ↑↓ history',
  '  pgup pgdn or the mouse wheel scroll · shift+← shift+→ move between decks',
  '  ctrl+a ctrl+e line start/end · ctrl+u ctrl+k ctrl+w delete · ctrl+c leave',
  '  / opens the command menu · tab or enter completes',
  '  answer a waiting approval with /y /a /n /d · @architect … sends to one role · @specs/… points at a file',
  '  esc stops what is running (when the prompt is empty) · ctrl+o shows tool details · ctrl+r searches history',
  '  a message sent while something runs is queued and goes when it finishes',
];

function deckBody(m: Model, w: number, h: number): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  if (m.deck === 0) return activityView(m, w, h);
  if (m.deck === 2) return I.view(m.state, m.inbox, m.info, m.pal, m.now, w, h);
  return pad([]);
}

/** The whole screen: exactly `h` lines. */
/** The left deck: what the agents are thinking and calling, newest at the bottom. Only ever on screen; PgUp scrolls. */
export function activityLines(m: Model, w: number): string[] {
  const out: string[] = [];
  let last = '';
  for (const a of m.state.activity) {
    const who = a.role + ':' + a.span;
    if (who !== last && (a.kind === 'thinking' || a.kind === 'text')) {
      last = who;
      out.push('', '  ' + T.badge(a.role, 'running', m.info, m.pal) + ' ' + T.roleColor(a.role, m.info).bold(T.roleName(a.role, m.info)) + T.dim(` · ${a.role}`));
    }
    if (a.kind === 'thinking') out.push(...a.text.split('\n').flatMap((l) => wrap(l, w - 6)).map((l) => '    ' + chalk.italic(T.dim(l))));
    else if (a.kind === 'text') out.push(...a.text.split('\n').flatMap((l) => wrap(l, w - 6)).map((l) => '    ' + T.text(m.pal)(l)));
    else if (a.kind === 'tool') out.push('  ' + T.dim('→ ') + T.link(a.text));
    else out.push('    ' + T.crimson(a.text));
  }
  return out;
}

function activityView(m: Model, w: number, h: number): string[] {
  const lines = activityLines(m, w);
  if (!lines.length) return ['', '  ' + chalk.bold(T.text(m.pal)('Activity and thinking stream')), '', '  ' + T.dim('nothing running'), '', '  ' + T.dim('Reasoning, replies and tool calls appear here as they happen.'), '  ' + T.dim('They live only in this window: nothing is saved.')];
  const maxBack = Math.max(0, lines.length - h);
  const back = Math.min(m.back, maxBack);
  const body = lines.slice(Math.max(0, lines.length - h - back), lines.length - back);
  while (body.length < h) body.unshift('');
  if (back > 0) body[h - 1] = T.spread('', chalk.hex('#facc15')(`↓ ${back} lines below · pgdn`), w);
  return body;
}

export const MIN_W = 28, MIN_H = 9;

/** A window too small to draw in: say so, in as few columns as it has, and nothing else. */
export function tooSmall(w: number, h: number): string[] {
  const msg = [`delphi needs at least ${MIN_W}x${MIN_H}`, `this window is ${w}x${h}`];
  const out = Array.from({ length: Math.max(0, h) }, () => '');
  msg.forEach((l, i) => { if (i < out.length) out[i] = [...l].slice(0, Math.max(0, w)).join(''); });
  return out;
}

const SPIN = ['◐', '◓', '◑', '◒'];

function statusRow(m: Model, w: number): string {
  if (m.search) {
    const hit = m.search.match;
    return T.clip(T.dim('(search) ') + T.text(m.pal)(m.search.q) + T.dim(hit ? '  →  ' + hit.split('\n')[0] : m.search.q ? '  no match' : '  type to search history · ctrl+r older · enter use · esc cancel'), w);
  }
  const n = Object.keys(m.state.running).length;
  if (!n) return '';
  const since = Math.min(...Object.values(m.state.running).map((r) => r.since));
  const secs = Math.max(0, Math.round((m.now - since) / 1000));
  const left = T.accent(SPIN[Math.floor(m.now / 250) % 4]!) + T.dim(` working · ${secs}s${n > 1 ? ` · ${n} tasks` : ''}${m.queue.length ? ` · ${m.queue.length} queued` : ''}`);
  return T.spread(left, T.dim('esc to stop'), w);
}

/** The whole screen: exactly `h` lines. */
export function frame(m: Model, w: number, h: number): string[] {
  if (w < MIN_W || h < MIN_H) return tooSmall(w, h);
  const head = header(m, w);
  const lay = layout(m.composer, Math.max(8, w - 2));
  const promptRows = Math.min(lay.rows.length, 6);
  const first = Math.max(0, Math.min(lay.cursorRow - promptRows + 1, lay.rows.length - promptRows));
  const prompt = lay.rows.slice(first, first + promptRows).map((r, i) =>
    (first + i === 0 ? T.dim('❯ ') : '  ') + T.text(m.pal)(r.before) + (r.at ? chalk.inverse(r.at) : '') + T.text(m.pal)(r.after));
  if (m.deck === 2) { prompt.length = 0; prompt.push(T.dim(I.hint(m.inbox, I.cards(m.state).length))); }
  if (m.panel && m.deck === 1) { prompt.length = 0; prompt.push(T.dim(P.hint(m.panel))); }
  const bodyH = Math.max(1, h - head.length - 1 /*blank*/ - 1 /*status*/ - 1 - prompt.length - 1 - 1);
  let body: string[];
  if (m.deck !== 1) body = deckBody(m, w, bodyH);
  else if (m.panel) body = P.view(m.panel, m.state.sources, m.pal, w, bodyH);
  else {
    const ps = parts(m, w);
    if (m.help) ps.push(['', ...HELP.map(T.dim)]);
    const total = ps.reduce((n, p) => n + p.length, 0);
    const maxBack = Math.max(0, total - bodyH);
    const back = Math.min(m.back, maxBack);
    body = windowOf(ps, Math.max(0, total - bodyH - back), total - back);
    while (body.length < bodyH) body.unshift('');
    if (back > 0) body[bodyH - 1] = T.spread('', chalk.hex('#facc15')(`↓ ${back} lines below · pgdn · new output waits here`), w);
    if (m.menu) {
      const MAX = 5, items = m.menu.items, sel = Math.min(m.menu.sel, items.length - 1);
      const start = Math.max(0, Math.min(sel - MAX + 1, items.length - MAX));
      const shown = items.slice(start, start + MAX);
      shown.forEach((c, i) => {
        const on = start + i === sel;
        const row = (on ? T.accent('❯ ') : '  ') + (on ? chalk.bold(T.link(c.label)) : T.link(c.label)) + T.dim(` · ${c.desc}`);
        body[bodyH - shown.length + i] = T.clip(' ' + row, w);
      });
    }
  }
  const out = [...head, '', ...body.map((l) => T.clip(l, w)), statusRow(m, w), T.hairline(m.pal, w), ...prompt.map((l) => T.clip(l, w)), T.hairline(m.pal, w), footer(m, w)];
  while (out.length < h) out.push('');
  return out.slice(0, h);
}
