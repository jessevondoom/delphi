// The screen as data: a pure function from the console's model to exactly `h` lines of `w` columns. Ink only paints
// what this returns, so the whole look is testable without a terminal (see test/screen.test.ts).
import { renderMarkdown } from './markdown.ts';
import { layout, type Composer } from './composer.ts';
import type { Item } from './complete.ts';
import { classify } from './attention.ts';
import { status, type Block, type State } from './fold.ts';
import * as T from './theme.ts';
import type { AgentState } from './theme.ts';
import * as I from './inbox.ts';
import * as P from './panel.ts';
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';
import stringWidth from 'string-width';

export type Model = {
  info: T.Info; pal: T.Pal; state: State; now: number; deck: 0 | 1 | 2;
  composer: Composer; back: number; help: boolean;
  menu: { items: Item[]; sel: number } | null;
  expandTools: boolean;
  search: { q: string; match: string | null } | null;
  queue: string[];
  inbox: I.Inbox;
  panel: P.Panel | null;
  /** permissions mode: default, accept-edits mission-auto or optimistic */
  mode: string;
  /** The daemon says the workspace has no model at all (`models.get`): the welcome becomes the no-model card, and the footer says so. */
  noModel?: boolean;
};

const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout'];
const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8);
const dots = (deck: number) => [0, 1, 2].map((i) => (i === deck ? T.accent('●') : T.dim('○'))).join(' ');
const nav = (deck: number, w = 100) => (w >= 64 ? T.dim('← / ? → · ') : '') + dots(deck);

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
    left = roles.map((r) => T.badge(r, faceOf(m, r), m.info, m.pal)).join(' ');
  }
  const l2 = chalk.bold(T.text(m.pal)('delphi')) + T.dim(` v${m.info.version} · ${T.tilde(m.info.cwd)} · ${PANE[m.deck]}`);
  return [T.clip(left, w), '', T.clip(l2, w)];
}

/** A role's state for its badge. The face of an escalation is let go after ten seconds (`005e` §6.2): the card carries it from then on. */
const ESCALATED_FACE_MS = 10_000;
function faceOf(m: Model, role: string): AgentState {
  const a = m.state.agents[role];
  if (!a) return 'idle';
  return a.state === 'escalated' && a.since !== undefined && m.now - a.since >= ESCALATED_FACE_MS ? 'idle' : a.state;
}

const PANE = ['activity', 'chat', 'decisions'] as const;
const MODE: Record<string, string> = { 'accept-edits': 'accept edits', 'mission-auto': 'auto', optimistic: 'auto · optimistic' };

export function footer(m: Model, w: number): string {
  const k = m.state.tokens >= 1000 ? (m.state.tokens / 1000).toFixed(1) + 'k' : String(m.state.tokens);
  const pending = Object.keys(m.state.approvals).length;
  const left = T.dim([`${k} tokens`, m.noModel ? 'no model' : '', MODE[m.mode] ?? (m.mode === 'default' ? '' : m.mode), pending ? `${pending} waiting` : ''].filter(Boolean).join(' · '));
  return T.spread(left, nav(m.deck, w), w);
}

const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true, trim: false }).split('\n');
/** Prose: wrapped at word boundaries, no space left at the start of a continuation line. */
const words = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true }).split('\n');

/**
 * A card: a label, a cap row, the rows (each already padded by one cell), a cap row. The caps are half blocks, so a
 * tinted card gets its fill from them; until the tint arrives (`005e` §7, §5) they are drawn dim, which reads as
 * the same card without the fill.
 */
function card(label: string, rows: string[], width: number, margin: number): string[] {
  const pad = ' '.repeat(margin);
  return [pad + label, pad + T.dim('▄'.repeat(width)), ...rows.map((r) => pad + r), pad + T.dim('▀'.repeat(width))];
}

/** A task that stopped and needs the operator (`005e` §6.4): not an answer, so it does not look like one. */
function attentionCard(b: Block, m: Model, w: number): string[] {
  const role = b.role ?? 'orchestrator';
  const a = classify(b.reason ?? b.text);
  const label = T.badge(role, 'escalated', m.info, m.pal) + ' ' + T.crimson(chalk.bold(`${T.roleName(role, m.info)} · needs you`));
  const widest = Math.max(10, Math.floor((w * 4) / 5));
  const room = widest - 2;
  const hints = a.hints.flatMap((h) => {
    const m = /^(\S.*?\S)(\s{2,})(\S.*)$/.exec(h); // `command   what it does`, as the copy table writes it
    return m ? pair(m[1]!, m[3]!, m[1]!.length + m[2]!.length, room, '  ') : words(h, room - 2).map((l) => '  ' + T.dim(l));
  });
  const lines = [...words(a.headline, room).map((l) => T.text(m.pal)(l)), '', ...hints];
  const width = Math.min(widest, Math.max(...lines.map((l) => 1 + stringWidth(l))) + 1);
  return ['', ...card(label, ['', ...lines.map((l) => (l ? ' ' + l : '')), ''], width, 0)];
}

/**
 * A command and what it does, the second column starting at `at`. When the pair does not fit `room` columns the
 * command takes a line and its words go under it, so a narrow window loses nothing.
 */
function pair(cmd: string, desc: string, at: number, room: number, lead = ''): string[] {
  const gap = Math.max(2, at - stringWidth(cmd));
  if (lead.length + stringWidth(cmd) + gap + stringWidth(desc) <= room) return [lead + T.link(cmd) + ' '.repeat(gap) + T.dim(desc)];
  return [lead + T.link(cmd), ...words(desc, Math.max(10, room - lead.length - 2)).map((l) => lead + '  ' + T.dim(l))];
}

/** No model at all (`005e` §6.3): what is wrong and the two commands that help. At most 76 columns, never stretched. */
function noModelCard(m: Model, w: number): string[] {
  const width = Math.min(76, w - 4);
  const inner = width - 2;
  const body = (l: string) => words(l, inner).map((x) => ' ' + T.text(m.pal)(x));
  const rows = [
    '',
    ...body('Welcome. This workspace has no model access yet, so nothing can answer.'),
    '',
    ...pair('delphi setup', 'connect Claude Code, agy or an API key (guided)', 18, inner, '   '),
    ...pair('delphi models', 'what is configured, and what is usable right now', 18, inner, '   '),
    '',
    ...body('When a model is connected: type a request below, @architect … sends it'),
    ...body('to one role, / lists commands and ? shows the keys.'),
    '',
  ];
  return card(chalk.bold(T.text(m.pal)('delphi · no model configured')), rows, width, 2);
}

/** A ready console with nothing said yet (`005e` §6.5): what to type, instead of a blank screen. */
function welcome(m: Model, w: number): string[] {
  if (m.noModel) return noModelCard(m, w);
  return [
    '',
    '  ' + T.text(m.pal)(`Ask anything about ${T.tilde(m.info.cwd)}, or hand out work:`),
    '',
    ...[['@architect draft a spec for …', 'one role answers'], ['/build implement @specs/011-x.md', 'a Builder task: worktree, tests, audit'], ['/council Should we …?', 'three seats argue it out blind']].flatMap(([c, d]) => pair(c!, d!, 36, w - 4, '    ')),
    '',
    '  ' + T.dim('/ lists commands · ? shows the keys · ← → move between decks'),
  ];
}

function render(b: Block, m: Model, w: number): string[] {
  if (b.kind === 'attention') return attentionCard(b, m, w);
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
/**
 * How many of the newest blocks are drawn. A session can run for days; the mission keeps every event, `/export` writes
 * them all, and scrolling back past this window shows where it stops.
 */
export const WINDOW = 1500;

/** Nothing said yet in this mission: the body shows the welcome (or the no-model card), from its top (`005e` §6). */
const fresh = (m: Model) => !m.state.blocks.length && !m.state.started && !m.state.live && !m.queue.length;

function parts(m: Model, w: number): string[][] {
  const blocks = m.state.blocks;
  const from = Math.max(0, blocks.length - WINDOW);
  const out: string[][] = from > 0 ? [['', T.dim(`  … ${from} earlier messages are not drawn; the mission keeps them and /export saves them all`)]] : [];
  if (fresh(m)) out.push(welcome(m, w));
  for (let i = from; i < blocks.length; i++) out.push(block(blocks[i]!, m, w));
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
  '  pgup pgdn or the mouse wheel scroll · ← → with an empty prompt (or shift+← shift+→ any time) move between decks',
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
    const top = fresh(m) && !m.help && back === 0; // a welcome is read from its first line
    body = top ? windowOf(ps, 0, bodyH) : windowOf(ps, Math.max(0, total - bodyH - back), total - back);
    if (top) while (body.length < bodyH) body.push('');
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
