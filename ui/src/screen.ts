// The screen as data: a pure function from the console's model to exactly `h` lines of `w` columns. Ink only paints
// what this returns, so the whole look is testable without a terminal (see test/screen.test.ts).
import { renderMarkdown } from './markdown.ts';
import { layout, type Composer } from './composer.ts';
import type { Item } from './complete.ts';
import { status, type Block, type State } from './fold.ts';
import { cardsOf, inboxBody, inboxHint, type InboxUi } from './inbox.ts';
import type { Pin } from './notebook.ts';
import type { Observer } from './observer.ts';
import type { Source } from './sources.ts';
import { panelBody, panelFooter, panelHint, type PanelCtx, type PanelUi } from './panels.ts';
import * as T from './theme.ts';
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';

export type Model = {
  info: T.Info; pal: T.Pal; state: State; now: number; deck: 0 | 1 | 2;
  composer: Composer; back: number; help: boolean;
  menu: { items: Item[]; sel: number } | null;
  expandTools: boolean;
  search: { q: string; match: string | null } | null;
  queue: string[];
  inbox: InboxUi;
  /** The approval mode the daemon is in: default, accept-edits or mission-auto. */
  mode: string;
  /** The Observer's view model: panels and the thinking deck read it. */
  obs: Observer;
  /** An open panel, over whatever deck is showing. */
  panel: { ui: PanelUi; cap: number; pending: number; mission: string; sources?: Source[]; notebook?: Pin[]; preview?: (rel: string) => string[] } | null;
};

const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'];
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8);
const dots = (deck: number) => [0, 1, 2].map((i) => (i === deck ? T.accent('●') : T.dim('○'))).join(' ');
const nav = (deck: number) => T.dim('navigate: shift+arrows · ') + dots(deck);

export function header(m: Model, w: number): string[] {
  const st = status(m.state, m.now);
  let left: string;
  if (st.kind === 'alert') {
    left = T.badge(st.role, 'waiting on you', m.info, m.pal) + ' ' + T.crimson(chalk.bold(Object.keys(m.state.approvals).length > 1 ? `${Object.keys(m.state.approvals).length} decisions pending` : T.roleName(st.role, m.info) + ': decision needed')) + T.dim(` · ${String(Math.floor(st.secs / 60)).padStart(2, '0')}:${String(st.secs % 60).padStart(2, '0')} remaining`);
  } else if (st.kind === 'active') {
    left = T.badge(st.role, 'running', m.info, m.pal) + ' ' + T.roleColor(st.role, m.info).bold(T.roleName(st.role, m.info) + ':') + ' ' + T.text(m.pal)(st.doing || 'working');
  } else {
    const roles = ORDER.filter((r) => m.info.roles[r]);
    left = roles.map((r) => T.badge(r, m.state.agents[r]?.state ?? 'idle', m.info, m.pal)).join(' ') + T.dim('  all agents idle');
  }
  const l2 = chalk.bold(T.text(m.pal)('delphi')) + T.dim(` v${m.info.version} · ${T.tilde(m.info.cwd)}`);
  return [T.spread(left, nav(m.deck), w), T.clip(l2, w)];
}

export function footer(m: Model, w: number): string {
  const k = m.state.tokens >= 1000 ? (m.state.tokens / 1000).toFixed(1) + 'k' : String(m.state.tokens);
  const pending = Object.keys(m.state.approvals).length;
  const left = T.dim(`${k} tokens · ${m.mode === 'default' ? 'approvals' : m.mode}${pending ? ` · ${pending} waiting` : ''} · shortcuts: ?`);
  return T.spread(left, nav(m.deck), w);
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
const rolesKeys = new WeakMap<object, string>();
const rolesKey = (info: T.Info) => { let k = rolesKeys.get(info.roles); if (k === undefined) { k = JSON.stringify(info.roles); rolesKeys.set(info.roles, k); } return k; };
function block(b: Block, m: Model, w: number, base: string): string[] {
  const key = `${base}:${b.status}:${b.detail?.length ?? 0}`;
  const hit = cache.get(b);
  if (hit && hit.key === key) return hit.lines;
  const lines = render(b, m, w);
  cache.set(b, { key, lines });
  return lines;
}

/**
 * How many of the newest blocks are drawn. A session can run for days; drawing (and re-wrapping on every resize) all
 * of it would make each keystroke slower than the last. The mission keeps every event, `/export` writes them all, and
 * scrolling back past this window shows where it stops.
 */
export const WINDOW = 1500;

/** The transcript lines, oldest first: the newest `WINDOW` blocks, the queue, and the unfinished reply dim at the bottom. */
export function transcript(m: Model, w: number): string[] {
  const blocks = m.state.blocks;
  const from = Math.max(0, blocks.length - WINDOW);
  const base = `${w}:${m.pal.dark}:${m.expandTools}:${rolesKey(m.info)}`;
  const out: string[] = from > 0 ? ['', T.dim(`  … ${from} earlier messages are not drawn; the mission keeps them and /export saves them all`)] : [];
  for (let i = from; i < blocks.length; i++) for (const l of block(blocks[i]!, m, w, base)) out.push(l);
  for (const q of m.queue) out.push('', T.dim('❯ ' + q.split('\n')[0] + '  (queued)'));
  if (m.state.live) out.push('', ...renderMarkdown(m.state.live.slice(-3000), w - 2).map((l) => '  ' + chalk.dim(l)));
  return out;
}

const HELP = [
  'keys',
  '  enter send · alt+enter or ctrl+j newline · esc clear · ↑↓ history',
  '  pgup pgdn or the mouse wheel scroll · shift+← shift+→ move between decks',
  '  ctrl+a ctrl+e line start/end · ctrl+u ctrl+k ctrl+w delete · ctrl+c leave',
  '  / opens the command menu · tab or enter completes',
  '  answer a waiting approval with /y /a /n /d · @architect … sends to one role · @specs/… points at a file',
  '  shift+→ or /operator opens the decision inbox (y n a d, tab list/focus, esc back) · shift+tab cycles approvals, accept-edits and mission-auto',
  '  /panel [council|epic|forge|agents|spend|archives|sources|notebook|connectors] opens an Observer view here (1-9 jump, shift+←→ cycle, esc back)',
  '  /pin keeps the last reply in the mission notebook · /pin <text> keeps a note',
  '  esc stops what is running (when the prompt is empty) · ctrl+o shows tool details · ctrl+r searches history',
  '  a message sent while something runs is queued and goes when it finishes',
];

/**
 * The left deck (spec 005b §3.3): what each agent is writing right now, span by span, and the tool calls so far.
 * It reads the Observer's live buffers, which live in memory only: reasoning is never saved and never enters the chat.
 */
export function thinkingBody(m: Model, w: number, h: number): string[] {
  const spans = [...m.obs.live.entries()].filter(([, t]) => t.trim());
  const out: string[] = ['', T.dim('  Activity and thinking stream · memory only, never saved')];
  const tools = m.state.blocks.filter((b) => b.kind === 'tool').slice(-4);
  if (tools.length) out.push('', ...tools.map((b) => '  ' + T.dim('→ ' + b.text) + ' ' + (b.status === 'error' ? T.crimson('✖') : b.status === 'ok' ? T.dim('✔') : T.dim('…'))));
  if (!spans.length) out.push('', T.dim(Object.keys(m.state.running).length ? '  waiting for the first words…' : '  nothing running'));
  for (const [id, t] of spans) {
    const node = m.obs.nodes.get(id);
    const role = node?.role ?? 'orchestrator';
    out.push('', '  ' + T.badge(role, 'running', m.info, m.pal) + ' ' + T.roleColor(role, m.info).bold(T.roleName(role, m.info)) + T.dim(` · ${id}`));
    out.push(...t.split('\n').flatMap((l) => wrapAnsi(l, Math.max(10, w - 4), { hard: true, trim: false }).split('\n')).map((l) => '    ' + chalk.dim(l)));
  }
  if (m.state.live && !spans.length) out.push('', ...renderMarkdown(m.state.live.slice(-2000), w - 2).map((l) => '  ' + chalk.dim(l)));
  const fit = out.length > h ? [...out.slice(0, 2), ...out.slice(out.length - (h - 2))] : [...out, ...Array(h - out.length).fill('')];
  return fit.map((l) => T.clip(l, w));
}

function deckBody(m: Model, w: number, h: number): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  if (m.deck === 0) return thinkingBody(m, w, h);
  return inboxBody(m.inbox, cardsOf(m.state), m, w, h);
}

/** The whole screen: exactly `h` lines. */
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
  const head = header(m, w);
  const lay = layout(m.composer, Math.max(8, w - 2));
  const promptRows = Math.min(lay.rows.length, 6);
  const first = Math.max(0, Math.min(lay.cursorRow - promptRows + 1, lay.rows.length - promptRows));
  const prompt = m.panel ? [panelHint(m.panel.ui)] : m.deck === 2 ? [inboxHint(m.inbox, cardsOf(m.state), m)] : lay.rows.slice(first, first + promptRows).map((r, i) =>
    (first + i === 0 ? T.dim('❯ ') : '  ') + T.text(m.pal)(r.before) + (r.at ? chalk.inverse(r.at) : '') + T.text(m.pal)(r.after));
  const bodyH = Math.max(1, h - head.length - 1 /*blank*/ - 1 /*status*/ - 1 - prompt.length - 1 - 1);
  let body: string[];
  if (m.panel) {
    const ctx: PanelCtx = { obs: m.obs, info: m.info, pal: m.pal, now: m.now, cap: m.panel.cap, pending: m.panel.pending, mission: m.panel.mission, sources: m.panel.sources, notebook: m.panel.notebook, preview: m.panel.preview };
    body = [...panelBody(m.panel.ui, ctx, w, Math.max(1, bodyH - 1)), panelFooter(ctx, w)];
  } else if (m.deck !== 1) body = deckBody(m, w, bodyH);
  else {
    const lines = m.help ? [...transcript(m, w), '', ...HELP.map(T.dim)] : transcript(m, w);
    const maxBack = Math.max(0, lines.length - bodyH);
    const back = Math.min(m.back, maxBack);
    body = lines.slice(Math.max(0, lines.length - bodyH - back), lines.length - back);
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
