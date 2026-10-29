// The screen as data: a pure function from the console's model to exactly `h` lines of `w` columns. Ink only paints
// what this returns, so the whole look is testable without a terminal (see test/screen.test.ts).
import { renderMarkdown, wrapLine } from './markdown.ts';
import { blockWidths as outerWidths, hugWidth, tintBlock } from './blocks.ts';
import { layout, type Composer } from './composer.ts';
import type { Item } from './complete.ts';
import { classify } from './attention.ts';
import { speakerOf, type Block, type Done, type Span, type State } from './fold.ts';
import { faceOf, spinner, STRIP_ROLES, typingFrame } from './motion.ts';
import * as T from './theme.ts';
import { kilo } from './meters.ts';
import * as I from './inbox.ts';
import * as P from './panel.ts';
import * as A from './archives.ts';
import { describe as describeLink, paintFocus, scanLinks, type Link } from './links.ts';
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
  /** No animation (`DELPHI_REDUCE_MOTION=1`, `NO_COLOR`, `TERM=dumb`): every badge on its static frame. */
  still?: boolean;
  /** Ctrl+C was pressed once and the two seconds to confirm have not passed: the title-bar badges show crossed eyes. */
  quitArmed?: boolean;
  /** Candidates waiting in the decisions pane's `learn` kind, and the org bundles added: the quiet scope line (`specs/053` §5). */
  review?: number; bundles?: number;
  /** The knowledge window on the left deck (`archives.ts`); the app owns it and makes the `kb.get` calls. */
  arch?: A.Arch;
  /** Link mode in the chat (`specs/029`, `ctrl+o`): the pointers and addresses in the transcript can be focused and followed. */
  links?: { focus: number | null; note: string };
  /** A document opened from a link; while one is open it takes the chat's body, and closing it returns to the transcript. */
  reader?: A.Arch;
};

const clock = (ms: number) => new Date(ms).toTimeString().slice(0, 8);
const SEP = '  ·  ';
const dots = (deck: number) => [0, 1, 2].map((i) => (i === deck ? T.accent('●') : T.dim('○'))).join(' ');
const nav = (deck: number, w = 100) => (w >= 64 ? T.dim('← / ? →' + SEP) : '') + dots(deck);

const PANE = ['knowledge', 'chat', 'decisions'] as const;

/** `delphi v0.5 · ~/projects/app · chat`. The path gives way from the left and the pane name never does (`005e` §10.2). */
export function titleRow(m: Model, w: number): string {
  const pane = PANE[m.deck];
  const name = chalk.bold(T.text(m.pal)('delphi'));
  const line = (path: string) => name + T.dim(` v${m.info.version}${path ? ` · ${path}` : ''} · ${pane}`);
  if (w < 48) return T.clip(name + T.dim(` · ${pane}`), w);
  const path = T.tilde(m.info.cwd);
  if (T.width(line(path)) <= w) return line(path);
  const room = w - T.width(line('')) - 3; // ` · ` before the path
  if (room < 4) return T.clip(line(''), w);
  const chars = [...path];
  let tail = '';
  for (let i = chars.length - 1; i >= 0 && T.width('…' + chars.slice(i).join('')) <= room; i--) tail = chars.slice(i).join('');
  return line('…' + tail);
}

/**
 * The window's top: the agent strip, then the title row directly under it (`005e` §10.2). The strip is badges only, in
 * the fixed role order, and each badge's animation says what its agent is doing. One blank row follows, added by `frame`.
 */
export function header(m: Model, w: number): string[] {
  const roles = STRIP_ROLES.filter((r) => m.info.roles[r]);
  const strip = roles.map((r) => { const f = faceOf(m.state.agents[r], r, m.now, m.still ?? false); return T.badge(r, f.state, m.info, m.pal, f.frame, m.quitArmed ?? false); }).join(' ');
  return [T.clip(strip, w), titleRow(m, w)];
}

const modeLabel = (mode: string): string => (mode === 'mission-auto' ? T.auto('auto') : mode === 'optimistic' ? T.auto('auto · optimistic') : mode === 'accept-edits' ? T.body('accept edits') : T.dim(mode === 'default' ? 'default' : mode));

/** What this session added to the library, in the quiet words of `specs/053` §5: `+3 noted · 1 to review`; empty when nothing was. Never alert-coloured. */
export const libraryCounter = (noted: number, review: number): string => [noted ? `+${noted} noted` : '', review ? `${review} to review` : ''].filter(Boolean).join(' · ');

export function footer(m: Model, w: number): string {
  const k = m.state.tokens >= 1000 ? (m.state.tokens / 1000).toFixed(1) + 'k' : String(m.state.tokens);
  const pending = Object.keys(m.state.approvals).length;
  const asks = m.state.blocks.filter((b) => b.kind === 'user').length;
  const scope = [m.info.workspace_name, m.info.mission].filter(Boolean).join('/');
  const counter = libraryCounter(m.state.noted.length, m.review ?? 0);
  const base = [T.dim(`${k} tokens`), modeLabel(m.mode), m.state.session ? T.dim(`session ${m.state.session} · ${asks} ask${asks === 1 ? '' : 's'}`) : '', m.noModel ? T.dim('no model') : '', pending ? T.dim(`${pending} waiting`) : ''];
  const extras = [scope ? T.dim(scope) : '', m.bundles ? T.dim(`${m.bundles} bundle${m.bundles === 1 ? '' : 's'}`) : '', counter ? T.dim(counter) : ''];
  const room = w - T.width(nav(m.deck, w)) - 2;
  const join = (xs: string[]) => xs.filter(Boolean).join(T.dim(SEP));
  // The scope and the counter give way, the bundles first, when the window is narrow; the tokens and the mode never do.
  const tries = [[0, 1, 2], [0, 2], [2], []].map((keep) => join([...base, ...keep.map((i) => extras[i]!)]));
  const left = tries.find((t) => T.width(t) <= room) ?? tries[tries.length - 1]!;
  return T.spread(left, nav(m.deck, w), w);
}

const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true, trim: false }).split('\n');
/** Prose: wrapped at word boundaries, no space left at the start of a continuation line. */
const words = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true }).split('\n');

const tintOf = (m: Model): T.Tint | null => T.tint(m.pal.bg ?? null, m.pal.dark);

/** A card (no-model, attention, help): a label, then rows that carry their own padding, in a tinted block or on a rail. */
function card(m: Model, label: string, rows: string[], width: number, margin: number): string[] {
  return tintBlock({ label, rows, width, col: margin, tint: tintOf(m) });
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
  return ['', ...card(m, label, ['', ...lines.map((l) => (l ? ' ' + l : '')), ''], width, 0)];
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
  return card(m, chalk.bold(T.text(m.pal)('delphi · no model configured')), rows, width, 2);
}

/** A ready console with nothing said yet (`005e` §6.5): what to type, instead of a blank screen. */
function welcome(m: Model, w: number): string[] {
  if (m.noModel) return noModelCard(m, w);
  return [
    '',
    '  ' + T.text(m.pal)(`Ask anything about ${T.tilde(m.info.cwd)}, or hand out work:`),
    '',
    ...[['@architect draft a spec for …', 'one role answers'], ['/build implement @specs/011-x.md', 'a Builder task: worktree, tests, audit'], ['/council Should we …?', 'three seats argue it out blind']].flatMap(([c, d]) => pair(c!, d!, 36, w - 2, '    ')),
    '',
    '  ' + T.dim('/ lists commands · ? shows the keys · ← → move between decks'),
  ];
}

const trimBlank = (a: string[]): string[] => {
  let i = 0, j = a.length;
  while (i < j && a[i] === '') i++;
  while (j > i && a[j - 1] === '') j--;
  return a.slice(i, j);
};

/** The status gutter (`053` §4): a one-column bullet at the left of a block, then one pad column; below 60 columns just the glyph. */
export const gutterWidth = (w: number): number => (w >= 60 ? 2 : 1);
function gutter(lines: string[], glyph: string, w: number): string[] {
  const g = gutterWidth(w);
  const pad = ' '.repeat(g);
  const first = lines.findIndex((l) => l !== '');
  return lines.map((l, i) => (l === '' ? l : (i === first && glyph ? glyph + ' '.repeat(g - 1) : pad) + l));
}
/** Block widths inside the gutter: the gutter and the block together keep to the window's 80 % (`005e` §11). */
function blockWidths(inner: number): { max: number; text: number } {
  const g = inner >= 58 ? 2 : 1;
  const max = Math.max(10, outerWidths(inner + g).max - g);
  return { max, text: max - 2 };
}
/** A step that is under way: the spinner while motion is allowed, the still mark otherwise. */
const working = (m: Model): string => ((m.still ?? false) ? T.mark('working') : T.accent(spinner(m.now, false)));
const toolMark = (t: Block, m: Model): string => (t.status === 'error' ? T.mark('disproved') : t.status === 'ok' ? T.mark('done') : working(m));

/** A message: the label row, then the block, hugging its text and flush left (an agent) or right (the operator). */
function message(m: Model, w: number, label: string, text: string[], right = false): string[] {
  const rows = text.map((l) => (l ? ' ' + l : ' '));
  const width = hugWidth(rows, label, blockWidths(w).max);
  return tintBlock({ label, rows, width, col: right ? Math.max(0, w - width) : 0, tint: tintOf(m) });
}

/** A tool call inside an agent's block: one dim line led by its status mark (`Ctrl+O` shows what it returned). Alone, the gutter carries the mark. */
function toolRows(t: Block, m: Model, tw: number, inline = true): string[] {
  const head = inline ? T.clip(toolMark(t, m) + ' ' + T.dim(t.text), tw) : T.clip(T.dim(t.text), tw);
  if (!m.expandTools || !t.detail) return [head];
  return [head, ...t.detail.split('\n').flatMap((l) => wrap(l, tw - 4)).map((l) => '    ' + T.dim(l))];
}

const MARK = { info: () => T.dim('·'), error: () => T.mark('disproved'), ok: () => T.mark('done') };

const roleBullet = (role: string, m: Model): string => T.roleColor(role, m.info).bold('●');

function render(b: Block, m: Model, w: number, tools: Block[] = []): string[] {
  const inner = w - gutterWidth(w);
  const lines = renderIn(b, m, inner, tools);
  if (b.kind === 'attention') return gutter(lines, T.mark('needs-you'), w);
  if (b.kind === 'card') return gutter(lines, T.mark('needs-you'), w);
  if (b.kind === 'tool') return gutter(lines, toolMark(b, m), w);
  if (b.kind === 'user') return gutter(lines, T.dim('●'), w);
  if (b.kind === 'note') return gutter(lines, /^[·✖✔■]/.test(b.text) ? '' : MARK[b.tone ?? 'info'](), w);
  return gutter(lines, b.status === 'escalated' ? T.mark('needs-you') : roleBullet(b.role ?? 'orchestrator', m), w);
}

function renderIn(b: Block, m: Model, w: number, tools: Block[]): string[] {
  const { text: tw } = blockWidths(w);
  if (b.kind === 'attention') return attentionCard(b, m, w);
  if (b.kind === 'user') {
    const lines = b.text.split('\n').flatMap((l) => wrapLine(l, tw));
    return ['', ...message(m, w, T.dim(`you · ${clock(b.at)}`), lines.map((l) => T.operator(l)), true)];
  }
  if (b.kind === 'note') {
    const own = /^[·✖✔■]/.test(b.text); // a note that brings its own mark keeps it, in place of the gutter's
    const lines = b.text.split('\n').flatMap((l) => wrapLine(l, w - 2));
    return ['', ...lines.map((l) => T.dim(l))];
  }
  if (b.kind === 'card') {
    const red = b.status === 'red';
    const who = T.roleName(b.role ?? 'orchestrator', m.info);
    const head = (red ? T.crimson : T.warn)(`approval needed [${red ? 'RED — never auto-approved' : 'amber'}]`) + T.dim(` · ${who}`);
    const keys = T.dim(`/y approve once${red ? '' : ' · /a allow similar this task'} · /n reject · /d view all`);
    return ['', head, ...b.text.split('\n').flatMap((l) => wrap(l, w - 2)), keys];
  }
  if (b.kind === 'tool') return ['', ...toolRows(b, m, w - 2, false)];
  const role = b.role ?? 'orchestrator';
  const label = T.badge(role, b.status === 'escalated' ? 'escalated' : 'idle', m.info, m.pal) + ' ' + T.roleColor(role, m.info).bold(T.roleName(role, m.info)) + T.dim(` · ${clock(b.at)}`);
  return ['', ...message(m, w, label, [...trimBlank(renderMarkdown(b.text, tw)), ...tools.flatMap((t) => toolRows(t, m, tw))])];
}

// Finished blocks never change, so each is rendered once per width, palette and tint. A long session stays cheap.
const cache = new WeakMap<Block, { key: string; lines: string[] }>();
const rolesKey = new WeakMap<object, string>();
function block(b: Block, m: Model, w: number, tools: Block[] = []): string[] {
  let rk = rolesKey.get(m.info.roles);
  if (rk === undefined) { rk = JSON.stringify(m.info.roles); rolesKey.set(m.info.roles, rk); }
  const spin = [b, ...tools].some((t) => t.kind === 'tool' && t.status !== 'ok' && t.status !== 'error') && spinner(m.now, m.still ?? false);
  const tk = tools.map((t) => `${t.id}${t.status}${t.detail?.length ?? 0}`).join(',') + spin;
  const key = `${w}:${m.pal.dark}:${m.pal.bg?.join(',') ?? '-'}:${T.currentCapabilities().color}:${m.expandTools}:${b.status}:${b.detail?.length ?? 0}:${rk}:${tk}`;
  const hit = cache.get(b);
  if (hit && hit.key === key) return hit.lines;
  const lines = render(b, m, w, tools);
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

/** The reply being written: the lines so far, with the cursor `▊` after the last (`005e` §7.2, streaming). */
function liveBlock(m: Model, outer: number, tools: Block[]): string[] {
  const w = outer - gutterWidth(outer);
  const { text: tw } = blockWidths(w);
  const role = speakerOf(m.state);
  const label = T.badge(role, 'running', m.info, m.pal) + ' ' + T.roleColor(role, m.info).bold(T.roleName(role, m.info)) + T.dim(' · writing');
  const body = trimBlank(renderMarkdown(m.state.live.slice(-3000), tw));
  const last = body.length - 1;
  if (last >= 0 && T.width(body[last]!) < tw) body[last] += T.accent('▊'); else body.push(T.accent('▊'));
  return gutter(['', ...message(m, w, label, [...body, ...tools.flatMap((t) => toolRows(t, m, tw))])], working(m), outer);
}

/**
 * The prompt the operator just sent, while it waits for its reply to begin: the block's `at` and id when the typing
 * bubble belongs on screen (a task runs, nothing is streaming, and nothing but tool calls followed the prompt).
 */
export function typingOf(s: State): { since: number; id: number } | null {
  if (s.live || !Object.keys(s.running).length) return null;
  for (let i = s.blocks.length - 1; i >= 0; i--) {
    const b = s.blocks[i]!;
    if (b.kind === 'user') return { since: b.at, id: b.id };
    if (b.kind !== 'tool') return null;
  }
  return null;
}

/** Three dots in an agent's bubble, like a messaging app's "typing": the page-indicator dots, one lit at a time. The reply's own block replaces it. */
function typingBlock(m: Model, outer: number, tools: Block[], frame: number): string[] {
  const w = outer - gutterWidth(outer);
  const { text: tw } = blockWidths(w);
  const role = speakerOf(m.state);
  const label = T.badge(role, 'running', m.info, m.pal) + ' ' + T.roleColor(role, m.info).bold(T.roleName(role, m.info)) + T.dim(' · typing');
  const dots = [0, 1, 2].map((i) => (m.still || frame === i ? T.accent('●') : T.dim('○'))).join(' ');
  return gutter(['', ...message(m, w, label, [dots, ...tools.flatMap((t) => toolRows(t, m, tw))])], working(m), outer);
}

type Laid = { parts: string[][]; reply: { part: number; id: 'live' | number } | null };

/** The transcript as pieces (each finished block is cached), and which piece is the newest reply. */
function layout_(m: Model, w: number, dense: boolean): Laid {
  const blocks = m.state.blocks;
  const from = Math.max(0, blocks.length - WINDOW);
  const out: string[][] = from > 0 ? [['', T.dim(`  … ${from} earlier messages are not drawn; the mission keeps them and /export saves them all`)]] : [];
  let reply: Laid['reply'] = null;
  if (fresh(m)) out.push(welcome(m, w));
  // Tool calls belong to the reply that follows them: they are drawn inside its block.
  let tools: Block[] = [];
  const flush = () => { for (const t of tools) out.push(block(t, m, w)); tools = []; };
  for (let i = from; i < blocks.length; i++) {
    const b = blocks[i]!;
    if (b.kind === 'tool') { tools.push(b); continue; }
    if (b.kind === 'reply') { out.push(block(b, m, w, tools)); tools = []; reply = { part: out.length - 1, id: b.id }; continue; }
    flush();
    out.push(block(b, m, w));
    if (b.kind === 'attention') reply = { part: out.length - 1, id: b.id };
  }
  if (m.state.live) { out.push(liveBlock(m, w, tools)); tools = []; reply = { part: out.length - 1, id: 'live' }; }
  else {
    const ty = typingOf(m.state);
    const frame = ty ? typingFrame(ty.since, ty.id, m.now, m.still ?? false) : -1;
    if (ty && frame >= 0) { out.push(typingBlock(m, w, tools, frame)); tools = []; reply = { part: out.length - 1, id: 'live' }; }
    else flush();
  }
  for (const q of m.queue) out.push(['', T.dim('❯ ' + q.split('\n')[0] + '  (queued)')]);
  // The frame puts one blank row under the title, so the first message does not bring its own; a short window drops them all.
  const lead = (p: string[], i: number) => p[0] === '' && !(fresh(m) && i === 0) && (dense || i === 0);
  return { parts: out.map((p, i) => (lead(p, i) ? p.slice(1) : p)), reply };
}

const parts = (m: Model, w: number, dense = false): string[][] => layout_(m, w, dense).parts;

export function transcript(m: Model, w: number, dense = false): string[] {
  return parts(m, w, dense).flat();
}

/** How many lines the transcript has at this width. */
export function transcriptCount(m: Model, w: number, dense = false): number {
  return parts(m, w, dense).reduce((n, p) => n + p.length, 0);
}

/**
 * Where the newest reply begins: its identity (`live` while it streams, then its block) and the transcript line of its
 * label row. The app pins that row at the top of the body when a long reply starts (`005e` §7.2, finding 6).
 */
export function replyAnchor(m: Model, w: number, dense = false): { id: 'live' | number; row: number } | null {
  const { parts: ps, reply } = layout_(m, w, dense);
  if (!reply) return null;
  const row = ps.slice(0, reply.part).reduce((n, p) => n + p.length, 0);
  return { id: reply.id, row: row + (ps[reply.part]![0] === '' ? 1 : 0) };
}

/** The transcript as one list of rows, and the links in it (found by their text, so rails and tinted blocks need no help). */
export function chatLinks(m: Model, w: number, dense = false): { lines: string[]; links: Link[] } {
  const lines = transcript(m, w, dense);
  return { lines, links: scanLinks(lines) };
}

/** The scroll offset (rows hidden below the viewport) that shows transcript row `row`, changing `back` as little as it can. */
export function backShowing(total: number, row: number, bodyH: number, back: number): number {
  const top = total - bodyH - back;
  if (row < top) return Math.max(0, total - bodyH - row);
  if (row >= top + bodyH) return Math.max(0, total - row - 1);
  return back;
}

/** The status row in link mode: the focused target and what the keys do. */
function linkStatus(m: Model, w: number, links: Link[]): string {
  const f = m.links?.focus == null ? undefined : links.find((l) => l.id === m.links!.focus);
  if (m.links?.note) return T.clip(T.dim(m.links.note + ' · esc leaves link mode'), w);
  if (!links.length) return T.dim('no links in the chat · esc leaves link mode');
  if (!f) return T.clip(T.dim(`${links.length} link${links.length === 1 ? '' : 's'} · tab chooses · esc leaves link mode`), w);
  const what = f.target.k === 'external' ? 'enter opens it in your browser' : 'enter reads it here · p pins it';
  return T.clip(T.accent('link ') + T.text(m.pal)(describeLink(f.target)) + T.dim(` · ${what} · y copy · esc leaves`), w);
}

/** How many lines to hide below the viewport so that line `row` is the first one shown (0 while the rest still fits). */
export const anchorBack = (total: number, row: number, bodyH: number): number => Math.max(0, total - row - bodyH);

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

/** The keys, as the help overlay shows them (`005e` §10.1). Every row is a key that does what it says. */
const KEYS = [
  'enter  send                alt+enter / ctrl+j  new line',
  'esc    clear the prompt; with it empty, stop what is running',
  '↑ ↓    history             ctrl+r  search history      tab  complete',
  '← →    decks (empty prompt) · shift+← →  decks any time',
  'drag  select and copy text   double-click a word · triple-click a line',
  'pgup pgdn / wheel  scroll  ctrl+t  tool details        ctrl+l  to end',
  'ctrl+o  follow links (tab next · enter read here · p pin · esc leave)',
  '/  commands   @role …  one role   @specs/…  a file   ?  this help',
  'shift+tab  approval mode (default → accept edits → auto → optimistic)',
  'decisions:  enter open · space tick · a tick all · f filter · 1-9 choose',
  'ctrl+k  search knowledge  ctrl+s  sources  ctrl+c  leave (daemon stays)',
];

/** The help overlay: a card up to 76 columns wide, centred, over the transcript, which it never scrolls. */
function helpOverlay(m: Model, w: number): string[] {
  const width = Math.min(76, w - 4);
  const rows = ['', ...KEYS.flatMap((k) => words(k, width - 2).map((l, i) => ' ' + (i ? '  ' : '') + T.text(m.pal)(l))), ''];
  return card(m, T.dim('keys · esc closes'), rows, width, Math.floor((w - width) / 2));
}

function deckBody(m: Model, w: number, h: number): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  if (m.deck === 0) return A.view(m.arch ?? A.emptyArch(), w, h, m.pal, m.info, m.now > 0 ? m.now : undefined);
  if (m.deck === 2) return I.view(m.state, m.inbox, m.info, m.pal, m.now, w, h);
  return pad([]);
}

export const MIN_W = 28, MIN_H = 9;

/** A window too small to draw in: say so, in as few columns as it has, and nothing else. */
export function tooSmall(w: number, h: number): string[] {
  const msg = [`delphi needs at least ${MIN_W}x${MIN_H}`, `this window is ${w}x${h}`];
  const out = Array.from({ length: Math.max(0, h) }, () => '');
  msg.forEach((l, i) => { if (i < out.length) out[i] = [...l].slice(0, Math.max(0, w)).join(''); });
  return out;
}

function statusRow(m: Model, w: number): string {
  if (m.search) {
    const hit = m.search.match;
    return T.clip(T.dim('(search) ') + T.text(m.pal)(m.search.q) + T.dim(hit ? '  →  ' + hit.split('\n')[0] : m.search.q ? '  no match' : '  type to search history · ctrl+r older · enter use · esc cancel'), w);
  }
  const n = Object.keys(m.state.running).length;
  if (!n) return '';
  const since = Math.min(...Object.values(m.state.running).map((r) => r.since));
  const secs = Math.max(0, Math.round((m.now - since) / 1000));
  if (m.state.live && m.deck === 1) { // in the chat, a reply is being written: say who and what
    const pending = [...m.state.blocks].reverse().find((b) => b.kind === 'tool' && b.status === 'pending');
    const what = pending ? ` ${pending.text}` : '';
    return T.spread(T.accent(spinner(m.now, m.still ?? false)) + T.dim(` ${T.roleName(speakerOf(m.state), m.info)} · writing${what}`), T.dim('esc to stop'), w);
  }
  const left = T.accent(spinner(m.now, m.still ?? false)) + T.dim(` working · ${secs}s${n > 1 ? ` · ${n} tasks` : ''}${m.queue.length ? ` · ${m.queue.length} queued` : ''}`);
  return T.spread(left, T.dim('esc to stop'), w);
}

/** The prompt's rows (or the key hint of the deck that owns the keyboard). */
function promptOf(m: Model, w: number): string[] {
  const lay = layout(m.composer, Math.max(8, w - 2));
  const promptRows = Math.min(lay.rows.length, 6);
  const first = Math.max(0, Math.min(lay.cursorRow - promptRows + 1, lay.rows.length - promptRows));
  const prompt = lay.rows.slice(first, first + promptRows).map((r, i) =>
    (first + i === 0 ? T.dim('❯ ') : '  ') + T.text(m.pal)(r.before) + (r.at ? chalk.inverse(r.at) : '') + T.text(m.pal)(r.after));
  if (m.links && m.deck === 1) { prompt.length = 0; prompt.push(T.dim('link mode: tab next · shift+tab previous · enter open · y copy · p pin · esc leave')); }
  if (m.deck === 0) { prompt.length = 0; prompt.push(T.dim('type to search the knowledge base · enter opens · tab narrows to a collection · ← → move between decks')); }
  if (m.deck === 2) { prompt.length = 0; prompt.push(I.promptLine(m.inbox, w) ?? T.dim(I.hint(m.inbox, I.cards(m.state).length))); }
  if (m.panel && m.deck === 1) { prompt.length = 0; prompt.push(T.dim(P.hint(m.panel))); }
  return prompt;
}

/** Rows left for the body: the window less the strip, the title, the blank row, the status row, two hairlines, the prompt and the footer. */
export function bodyHeight(m: Model, w: number, h: number): number {
  return Math.max(1, h - 2 - (h < 16 ? 0 : 1) - 1 - 1 - promptOf(m, w).length - 1 - 1);
}

/** The whole screen: exactly `h` lines. */
export function frame(m: Model, w: number, h: number): string[] {
  if (w < MIN_W || h < MIN_H) return tooSmall(w, h);
  const head = header(m, w);
  const prompt = promptOf(m, w);
  const dense = h < 16; // a short window gives up the blank rows between messages
  const bodyH = bodyHeight(m, w, h);
  let body: string[];
  let links: Link[] | null = null;
  if (m.deck !== 1) body = deckBody(m, w, bodyH);
  else if (m.reader && m.reader.at >= 0) body = A.view(m.reader, w, bodyH, m.pal);
  else if (m.panel) body = P.view(m.panel, m.state.sources, m.pal, w, bodyH);
  else {
    const ps = parts(m, w, dense);
    const total = ps.reduce((n, p) => n + p.length, 0);
    let flat: string[] | null = null;
    if (m.links) { const all = ps.flat(); links = scanLinks(all); flat = paintFocus(all, links, m.links.focus); }
    const slice = (a: number, b: number) => (flat ? flat.slice(a, b) : windowOf(ps, a, b));
    const maxBack = Math.max(0, total - bodyH);
    const back = Math.min(m.back, maxBack);
    const top = back === 0 && total <= bodyH; // what fits is read from its first line, as in `less`; what does not follows the end
    body = top ? slice(0, bodyH) : slice(Math.max(0, total - bodyH - back), total - back);
    if (top) while (body.length < bodyH) body.push('');
    while (body.length < bodyH) body.unshift('');
    if (m.help) body = [...helpOverlay(m, w), ...Array(bodyH).fill('')].slice(0, bodyH);
    if (back > 0 && !m.help) body[bodyH - 1] = T.spread('', T.warn(`↓ ${back} more lines · pgdn or space · enter jumps to the end`), w);
    if (m.menu) {
      // Up to 8 rows, fewer when the body is short; a last dim row says how many more there are (`005e` §10.4).
      const items = m.menu.items, sel = Math.min(m.menu.sel, items.length - 1);
      const MAX = Math.max(1, Math.min(8, bodyH - 2));
      const start = Math.max(0, Math.min(sel - MAX + 1, items.length - MAX));
      const shown = items.slice(start, start + MAX);
      const more = items.length - shown.length;
      const rows = shown.map((c, i) => {
        const on = start + i === sel;
        const label = (on ? T.accent('❯ ') : '  ') + (on ? chalk.bold(T.link(c.label)) : T.link(c.label));
        // Below 100 columns a long description wraps to a second dim line instead of clipping mid-word.
        const room = w - 1 - T.width(label) - 3;
        const wrapped = w < 100 && T.width(c.desc) > room ? wrap(c.desc, Math.max(10, w - 6)) : [c.desc];
        const first = w < 100 && wrapped.length > 1 ? [label] : [label + T.dim(` · ${c.desc}`)];
        return [...first, ...(w < 100 && wrapped.length > 1 ? wrapped.slice(0, 2).map((l) => '    ' + T.dim(l)) : [])];
      });
      const lines = rows.flat();
      if (more > 0) lines.push('  ' + T.dim(`· ${more} more`));
      lines.slice(-Math.max(1, bodyH - 1)).forEach((l, i, a) => { body[bodyH - a.length + i] = T.clip(' ' + l, w); });
    }
  }
  const out = [...head, ...(h < 16 ? [] : ['']), ...body.map((l) => T.clip(l, w)), links && m.links ? linkStatus(m, w, links) : statusRow(m, w), T.hairline(m.pal, w), ...prompt.map((l) => T.clip(l, w)), T.hairline(m.pal, w), footer(m, w)];
  while (out.length < h) out.push('');
  return out.slice(0, h);
}
