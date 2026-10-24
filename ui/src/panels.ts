// The Observer's views as console panels (spec 005 §4, 005b §4): Council Chamber, Epic DAG, Forge Diffs, Agents, Spend,
// Archives. A pure key reducer and pure renderers over the `Observer` view model, so each view is tested as plain text.
// Panels only read; the one call they make (`kb.search`, `observe.diff`) is read-only and is asked for through `PanelAct`.
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';
import type { Ev } from './input.ts';
import type { Pin } from './notebook.ts';
import { ordered, totalTokens, type Build, type Council, type Observer } from './observer.ts';
import type { Source } from './sources.ts';
import * as T from './theme.ts';

export type View = 'council' | 'epic' | 'forge' | 'agents' | 'spend' | 'archives' | 'sources' | 'notebook' | 'connectors';
export const VIEWS: View[] = ['council', 'epic', 'forge', 'agents', 'spend', 'archives', 'sources', 'notebook', 'connectors'];
export const TITLES: Record<View, string> = { council: 'COUNCIL CHAMBER', epic: 'EPIC DAG', forge: 'FORGE DIFFS', agents: 'AGENTS', spend: 'SPEND TELEMETRY', archives: 'ARCHIVES', sources: 'SOURCES', notebook: 'NOTEBOOK', connectors: 'CONNECTORS' };
const SHORT: Record<View, string> = { council: 'Council', epic: 'Epic DAG', forge: 'Forge Diffs', agents: 'Agents', spend: 'Spend', archives: 'Archives', sources: 'Sources', notebook: 'Notebook', connectors: 'Connectors' };

export function parseView(s: string): View | null {
  const k = s.toLowerCase();
  const alias: Record<string, View> = { council: 'council', chamber: 'council', '1': 'council', epic: 'epic', dag: 'epic', '2': 'epic', forge: 'forge', diffs: 'forge', '3': 'forge', agents: 'agents', roster: 'agents', '4': 'agents', spend: 'spend', telemetry: 'spend', '5': 'spend', archives: 'archives', kb: 'archives', '6': 'archives', sources: 'sources', citations: 'sources', '7': 'sources', notebook: 'notebook', pins: 'notebook', '8': 'notebook', connectors: 'connectors', connector: 'connectors', '9': 'connectors' };
  return alias[k] ?? null;
}

export type Hit = { pointer: string; heading: string; preview: string; blake3?: string };
export type Kb = { editing: boolean; query: string; hits: Hit[]; note: string; sel: number };
export type PanelUi = {
  view: View;
  /** Which council or build is shown; -1 is the newest. */
  pick: number;
  scroll: number;
  /** Selected row in the Epic tree. */
  node: number;
  /** The last `observe.diff` answer for the Forge view. */
  diff: any | null;
  kb: Kb;
  /** Selected row in the Sources list. */
  src: number;
};

export const panelInit = (view: View = 'epic'): PanelUi => ({ view, pick: -1, scroll: 0, node: 0, src: 0, diff: null, kb: { editing: false, query: '', hits: [], note: '', sel: 0 } });

export type PanelAct = { t: 'close' } | { t: 'search'; q: string } | { t: 'diff'; task: string };

const step = (v: View, d: number): View => VIEWS[(VIEWS.indexOf(v) + d + VIEWS.length) % VIEWS.length]!;
const pickable = (ui: PanelUi, obs: Observer) => (ui.view === 'council' ? obs.councils.length : ui.view === 'forge' ? obs.builds.length : 0);
const resolvePick = (ui: PanelUi, n: number) => (n === 0 ? 0 : ui.pick < 0 ? n - 1 : Math.min(ui.pick, n - 1));
export const selectedCouncil = (ui: PanelUi, obs: Observer): Council | undefined => obs.councils[resolvePick(ui, obs.councils.length)];
export const selectedBuild = (ui: PanelUi, obs: Observer): Build | undefined => obs.builds[resolvePick(ui, obs.builds.length)];

/** One key in a panel. */
export function panelKey(ui: PanelUi, e: Ev, obs: Observer): { ui: PanelUi; act?: PanelAct } {
  const go = (v: View): { ui: PanelUi } => ({ ui: { ...ui, view: v, scroll: 0, pick: -1 } });
  if (ui.view === 'archives' && ui.kb.editing) {
    const kb = (over: Partial<Kb>) => ({ ui: { ...ui, kb: { ...ui.kb, ...over } } });
    if (e.t === 'text') return kb({ query: ui.kb.query + e.s });
    if (e.t === 'paste') return kb({ query: ui.kb.query + e.s.replace(/\n/g, ' ') });
    if (e.t === 'key' && e.k === 'backspace') return kb({ query: ui.kb.query.slice(0, -1) });
    if (e.t === 'key' && e.k === 'esc') return kb({ editing: false });
    if (e.t === 'key' && e.k === 'enter') return ui.kb.query.trim() ? { ...kb({ editing: false, note: 'searching…', sel: 0 }), act: { t: 'search', q: ui.kb.query.trim() } } : kb({ editing: false });
    return { ui };
  }
  if (e.t === 'text' && [...e.s].length > 1) { // several keys in one read: take them one at a time
    let cur: { ui: PanelUi; act?: PanelAct } = { ui };
    for (const ch of e.s) {
      const r = panelKey(cur.ui, { t: 'text', s: ch }, obs);
      cur = { ui: r.ui, act: cur.act ?? r.act };
      if (r.act) break;
    }
    return cur;
  }
  if (e.t === 'text') {
    const n = Number(e.s);
    if (Number.isInteger(n) && n >= 1 && n <= VIEWS.length) return go(VIEWS[n - 1]!);
    if (e.s === 'q') return { ui, act: { t: 'close' } };
    if (e.s === '/' && ui.view === 'archives') return { ui: { ...ui, kb: { ...ui.kb, editing: true } } };
    if (e.s === 'r' && ui.view === 'forge') { const b = selectedBuild(ui, obs); return b ? { ui, act: { t: 'diff', task: b.taskId } } : { ui }; }
    return { ui };
  }
  if (e.t !== 'key') return { ui };
  const scroll = (d: number) => ({ ui: { ...ui, scroll: Math.max(0, ui.scroll + d) } });
  switch (e.k) {
    case 'esc': return { ui, act: { t: 'close' } };
    case 'left': case 'right': {
      if (e.shift || e.alt) return go(step(ui.view, e.k === 'left' ? -1 : 1));
      const n = pickable(ui, obs);
      if (!n) return { ui };
      const cur = resolvePick(ui, n);
      const next = Math.max(0, Math.min(n - 1, cur + (e.k === 'left' ? -1 : 1)));
      const r = { ui: { ...ui, pick: next === n - 1 ? -1 : next, scroll: 0, diff: ui.view === 'forge' ? null : ui.diff } };
      const b = ui.view === 'forge' ? obs.builds[next] : undefined;
      return b ? { ...r, act: { t: 'diff', task: b.taskId } } : r;
    }
    case 'tab': return go(step(ui.view, 1));
    case 'shift-tab': return go(step(ui.view, -1));
    case 'up': return ui.view === 'sources' ? { ui: { ...ui, src: Math.max(0, ui.src - 1) } } : ui.view === 'epic' ? { ui: { ...ui, node: Math.max(0, ui.node - 1) } } : ui.view === 'archives' && ui.kb.hits.length ? { ui: { ...ui, kb: { ...ui.kb, sel: Math.max(0, ui.kb.sel - 1) } } } : scroll(-1);
    case 'down': return ui.view === 'sources' ? { ui: { ...ui, src: ui.src + 1 } } : ui.view === 'epic' ? { ui: { ...ui, node: ui.node + 1 } } : ui.view === 'archives' && ui.kb.hits.length ? { ui: { ...ui, kb: { ...ui.kb, sel: Math.min(ui.kb.hits.length - 1, ui.kb.sel + 1) } } } : scroll(1);
    case 'pgup': return scroll(-10);
    case 'pgdn': return scroll(10);
    case 'home': return { ui: { ...ui, scroll: 0 } };
    default: return { ui };
  }
}

// ── small formatting helpers (the Observer's own) ───────────────────────────

export const money = (v: number) => (v >= 100 ? `$${v.toFixed(0)}` : v >= 1 ? `$${v.toFixed(2)}` : `$${v.toFixed(3)}`);
export const kilo = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : String(n));
export const bar = (frac: number, width: number) => { const f = Math.max(0, Math.min(1, frac)); const full = Math.round(f * width); return '▓'.repeat(full) + '░'.repeat(Math.max(0, width - full)); };
export function sparkline(values: number[], width: number): string {
  const recent = values.slice(-Math.max(0, width));
  const max = Math.max(0, ...recent);
  if (max <= 0) return '▁'.repeat(recent.length);
  return recent.map((v) => '▁▂▃▄▅▆▇█'[Math.round((v / max) * 7)]).join('');
}
const glyph = (s: string) => (['done', 'green', 'merged'].includes(s) ? '✔' : s === 'running' ? '●' : ['escalated', 'failed', 'red', 'refused'].includes(s) ? '✖' : s === 'waiting on you' ? '►' : ['pending', 'idle'].includes(s) ? '○' : '·');
const stateColor = (s: string) => (['done', 'green', 'merged'].includes(s) ? chalk.hex('#22c55e') : s === 'running' ? chalk.hex('#22d3ee') : ['escalated', 'failed', 'red', 'refused'].includes(s) ? T.crimson : s === 'waiting on you' ? T.warn : T.dim);
const sevColor = (sev: string) => (sev === 'P0' ? T.crimson : sev === 'P1' ? T.warn : T.dim);
const oneLine = (s: string, n: number) => { const t = T.sanitize(s).replace(/\s*\n\s*/g, ' '); return t.length <= n ? t : t.slice(0, Math.max(0, n - 1)) + '…'; };
const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(4, w), { hard: true, trim: false }).split('\n');
const rule = (title: string, w: number, p: T.Pal) => T.rule(p)((title ? `── ${title} ` : '') + '─'.repeat(Math.max(0, w - (title ? title.length + 4 : 0))));

/** Columns side by side: each is wrapped to its share of `w`, then the rows are zipped with a gap. */
function columns(cols: string[][], w: number, gap = 2): string[] {
  const cw = Math.max(8, Math.floor((w - gap * (cols.length - 1)) / cols.length));
  const wrapped = cols.map((c) => c.flatMap((l) => wrap(l, cw)));
  const rows = Math.max(0, ...wrapped.map((c) => c.length));
  return Array.from({ length: rows }, (_, i) => wrapped.map((c) => { const l = c[i] ?? ''; return l + ' '.repeat(Math.max(0, cw - T.width(l))); }).join(' '.repeat(gap)).trimEnd());
}

export type PanelCtx = {
  obs: Observer; info: T.Info; pal: T.Pal; now: number; cap: number; pending: number; mission: string;
  /** What the conversation referenced, what the operator pinned, and a way to read a source's first lines. */
  sources?: Source[]; notebook?: Pin[]; preview?: (rel: string) => string[];
};

const name = (c: PanelCtx, role: string) => T.roleName(role, c.info);
const window = (lines: string[], scroll: number, h: number) => { const at = Math.max(0, Math.min(scroll, Math.max(0, lines.length - h))); return lines.slice(at, at + h); };

function councilView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal;
  const co = selectedCouncil(ui, c.obs);
  if (!co) return ['', T.dim('  No Council yet.'), '', T.dim('  Convene one in the console:  /council <proposition>')];
  const n = c.obs.councils.length;
  const live = co.phase === 'round 1' || co.phase === 'round 2';
  const head = [chalk.bold(T.text(p)(` Council ${resolvePick(ui, n) + 1}/${n} · ${co.taskId} · ${co.phase}${co.homogeneous ? ' · homogeneous (one model family)' : ''}`)), T.text(p)(' ' + oneLine(co.proposition, w - 3)), ''];
  const cols = co.seats.map((seat) => {
    const l: string[] = [T.roleColor(seat.role, c.info).bold(`${name(c, seat.role)} (${seat.family})`), rule('', 8, p)];
    const stance = co.stances.find((s) => s.role === seat.role);
    if (stance?.answered) l.push(T.dim('Round 1 (blind)'), ...T.sanitize(stance.text).split('\n').map(T.text(p)));
    else if (stance) l.push(T.dim('abstained: ' + oneLine(stance.text, 80)));
    else {
      l.push(T.dim('writing… (blind)'));
      const node = [...c.obs.nodes.values()].find((x) => x.taskId === co.taskId && x.role === seat.role);
      const t = node && c.obs.live.get(node.id);
      if (t) l.push(...t.split('\n').slice(-8).map(T.text(p)));
    }
    const cr = co.critiques.find((x) => x.role === seat.role);
    if (cr) {
      l.push('', chalk.bold(T.text(p)(`Round 2 → ${cr.recommendation || 'unreadable'}${cr.concur === true ? ' · concurs' : cr.concur === false ? ' · does not concur' : ''}`)));
      for (const [sev, title] of cr.findings) l.push(sevColor(sev)(`${sev} ${title}`));
    }
    return l;
  });
  const verdict: string[] = [rule('Verdict', w, p)];
  if (co.phase === 'round 1') verdict.push(T.dim(' Round 1 is blind: stances appear when every seat has answered.'));
  else if (co.phase === 'round 2') verdict.push(T.dim(" Round 2: seats are critiquing each other's stances."));
  else {
    if (co.escalation !== null) {
      verdict.push(T.warn(chalk.bold(` Escalated to you: ${oneLine(co.escalation, 100)}`)));
      for (const [label, seats] of co.options) verdict.push(T.text(p)(`   option: ${label} — ${seats.join(', ')}`));
    } else verdict.push(chalk.hex('#22c55e')(' Verdict reached; no deterministic escalation.'));
    verdict.push(chalk.bold(T.text(p)(' Minority report:')));
    if (!co.unresolved.length) verdict.push(T.dim('   none open'));
    for (const [sev, seat, title] of co.unresolved.slice(0, 3)) verdict.push(sevColor(sev)(`   [${sev}] ${seat}: ${oneLine(title, w - 16)}`));
    if (co.path) verdict.push(T.dim(` saved: ${co.path}`));
  }
  const vh = Math.min(verdict.length, live ? 3 : 9);
  const mid = Math.max(1, h - head.length - vh);
  return [...head, ...window(cols.length ? columns(cols, w) : [], ui.scroll, mid).concat(Array(mid).fill('')).slice(0, mid), ...verdict.slice(0, vh)];
}

/** The Epic tree in display order, with the task heading before each task's first node. */
export function epicRows(obs: Observer): { node: ReturnType<Observer['roots']>[number]; depth: number }[] {
  const out: { node: ReturnType<Observer['roots']>[number]; depth: number }[] = [];
  const walk = (n: ReturnType<Observer['roots']>[number], depth: number) => { out.push({ node: n, depth }); for (const ch of obs.children(n.id)) walk(ch, depth + 1); };
  for (const r of obs.roots()) walk(r, 0);
  return out;
}

function epicView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal;
  const rows = epicRows(c.obs);
  const sel = Math.min(ui.node, Math.max(0, rows.length - 1));
  const lines: string[] = [];
  let selLine = 0, last = '';
  if (!rows.length) lines.push(T.dim('  No agents have run yet in this mission.'));
  rows.forEach(({ node: n, depth }, i) => {
    if (n.taskId !== last) {
      last = n.taskId;
      const t = c.obs.tasks.get(n.taskId);
      lines.push(chalk.bold(T.text(p)(` ${n.taskId} ${t?.flow ?? ''}${t ? ' · ' + oneLine(t.prompt, 60) : ''}`)));
    }
    if (i === sel) selLine = lines.length;
    const body = `${name(c, n.role).padEnd(12)} ${n.state.padEnd(10)} ${kilo(n.tokens).padStart(7)} tok  ${money(n.costUsd)}`;
    lines.push('  ' + '  '.repeat(depth) + stateColor(n.state)(glyph(n.state) + ' ') + (i === sel ? chalk.inverse(T.roleColor(n.role, c.info)(body)) : T.roleColor(n.role, c.info)(name(c, n.role).padEnd(12)) + T.text(p)(body.slice(12))));
  });
  const story = [rule('Narrative', w, p), ...c.obs.narrative.slice(-8).map((l) => T.text(p)(' ' + oneLine(l, w - 3)))];
  const top = Math.max(2, h - story.length);
  const at = ui.scroll > 0 ? ui.scroll : Math.max(0, selLine - top + 2);
  return [...window(lines, at, top), ...Array(Math.max(0, top - Math.min(top, lines.length))).fill('')].slice(0, top).concat(story);
}

function forgeView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal;
  const b = selectedBuild(ui, c.obs);
  if (!b) return ['', T.dim('  No builds yet.'), '', T.dim('  Start one in the console:  /build <request>')];
  const n = c.obs.builds.length;
  const head = [chalk.bold(T.text(p)(` Build ${resolvePick(ui, n) + 1}/${n} · ${b.taskId} · branch ${b.branch || '…'} · commit ${b.sha || '…'} · ${b.merge || 'in progress'}`))];
  for (const a of b.attempts) head.push(T.text(p)('   ' + a));
  for (const g of b.gates) head.push('   ' + stateColor(g.state)(glyph(g.state) + ' ') + T.text(p)(`${g.name.padEnd(10)} ${oneLine(g.detail, w - 18)}`));
  for (const [sev, title] of b.findings) head.push(sevColor(sev)(`   audit ${sev} ${oneLine(title, 100)}`));
  if (b.flaky) head.push(T.warn('   a test failed once and passed on a rerun (flaky)'));
  const diff: string[] = [rule('Diff against the base commit', w, p)];
  const d = ui.diff;
  if (d?.available === true) {
    for (const l of T.sanitize(String(d.stat ?? '')).split('\n')) diff.push(T.text(p)(l));
    for (const u of d.untracked ?? []) diff.push(T.warn('?? ' + T.sanitize(String(u))));
    for (const l of T.sanitize(String(d.diff ?? '')).split('\n')) diff.push((l.startsWith('+') && !l.startsWith('+++') ? chalk.hex('#22c55e') : l.startsWith('-') && !l.startsWith('---') ? T.crimson : l.startsWith('@@') ? chalk.hex('#22d3ee') : T.text(p))(l));
  } else diff.push(T.dim('  no live worktree for this build (it finished, or has not started). r refreshes.'));
  const hh = Math.min(head.length, Math.max(1, h - 4));
  return [...head.slice(0, hh), ...window(diff, ui.scroll, h - hh)];
}

const MASCOT_W = 7;
function mascot(role: string, state: string, c: PanelCtx): string[] {
  const color = chalk.hex(c.info.roles[role]?.chassis ?? '#94a3b8');
  const r = c.info.roles[role];
  // The scout has three eyes and the builder a wide mouth when idle, as in the Observer; any role shows its alert faces.
  const idle = role === 'scout' ? '• • •' : role === 'builder' && r?.mascot_face ? r.mascot_face : (r?.face ?? '•_•');
  const face = state === 'waiting on you' ? (role === 'scout' ? 'x ? x' : 'x?x') : state === 'escalated' ? 'x_x' : state === 'running' ? (role === 'scout' ? '· · ·' : role === 'builder' ? '•───•' : '•─•') : idle;
  const pad = Math.max(0, MASCOT_W - [...face].length);
  const mid = ' '.repeat(Math.floor(pad / 2)) + face + ' '.repeat(pad - Math.floor(pad / 2));
  const crown = [...(r?.crown ?? '')].slice(0, MASCOT_W).map((ch, i) => (ch === '▄' ? chalk.hex(role === 'builder' && i >= 1 && i <= 5 ? '#ffffff' : (r?.crown_rgb ?? '#94a3b8'))('▄') : ' ')).join('');
  return [crown + ' '.repeat(MASCOT_W - [...(r?.crown ?? '')].slice(0, MASCOT_W).length), chalk.bgHex(c.info.roles[role]?.chassis ?? '#94a3b8').hex('#0f172a')(mid), color('▀'.repeat(MASCOT_W))];
}

function agentsView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal;
  const lines: string[] = [''];
  for (const a of ordered(c.obs)) {
    const m = mascot(a.role, a.state, c);
    const nm = name(c, a.role);
    const model = a.model ? `${a.tier} · ${oneLine(a.model, 28)}${a.upscaled ? ' ⬆' : ''} · ${a.endpoint || '—'}` : 'no call yet';
    const meta = [
      T.roleColor(a.role, c.info).bold(nm) + stateColor(a.state)(`  ${nm === a.role ? '' : a.role + '  '}${glyph(a.state)} ${a.state}`),
      T.text(p)(model),
      T.dim(`in ${kilo(a.tokens.input)} · cache ${kilo(a.tokens.cacheRead)} · out ${kilo(a.tokens.output)} · think ${kilo(a.tokens.thinking)} · ${money(a.costUsd)} · ${a.calls} call(s)`),
    ];
    for (let i = 0; i < 3; i++) lines.push('  ' + m[i] + '   ' + meta[i]);
    lines.push('');
  }
  return window(lines, ui.scroll, h);
}

function spendView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal, o = c.obs;
  const lines = [
    rule('Spend per call', w, p),
    chalk.hex('#22d3ee')('  ' + sparkline(o.series, w - 4)),
    T.dim(`  ${o.series.length} call(s) · total ${money(o.spentUsd)} · ${c.cap > 0 ? `${((o.spentUsd / c.cap) * 100).toFixed(0)}% of the ${money(c.cap)} ceiling` : 'no ceiling known'}`),
    '',
    rule('Tokens', w, p),
  ];
  const total = Math.max(1, totalTokens(o.tokens));
  const t = o.tokens;
  for (const [label, n] of [['input', t.input], ['cache read', t.cacheRead], ['cache write', t.cacheWrite], ['output', t.output], ['thinking', t.thinking]] as const) {
    lines.push(T.text(p)(`  ${label.padEnd(12)}${kilo(n).padStart(9)}  `) + chalk.hex('#22d3ee')(bar(n / total, 30)));
  }
  lines.push(T.dim(`  prompt cache hit ${(o.cacheHitRatio() * 100).toFixed(0)}% of input-side tokens · retries ${o.retries}`), '', rule('By agent', w, p));
  const spent = Math.max(Number.MIN_VALUE, o.spentUsd);
  for (const a of ordered(o)) lines.push(T.roleColor(a.role, c.info)(`  ${name(c, a.role).padEnd(13)}${bar(a.costUsd / spent, 24)}`) + T.text(p)(`  ${money(a.costUsd)}  ${kilo(totalTokens(a.tokens))} tok`));
  return window(lines, ui.scroll, h);
}

function archivesView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal, kb = ui.kb;
  const lines = [rule('Knowledge base', w, p)];
  lines.push(chalk.bold(T.text(p)(kb.editing ? ` search› ${kb.query}▏` : kb.query ? ` last search: ${kb.query}` : ' press / to search specs, context, REVIEW.md and lessons')));
  if (kb.note) lines.push(T.dim(' ' + kb.note));
  kb.hits.forEach((hit, i) => lines.push((i === kb.sel ? T.accent('❯') : ' ') + T.text(p)(` ${i + 1}. ${T.sanitize(hit.pointer)}`) + T.dim('  ' + oneLine(hit.heading ?? '', 60))));
  const sel = kb.hits[kb.sel];
  if (sel) {
    const room = Math.max(3, h - lines.length - 3);
    lines.push('', rule('Preview', w, p), T.link(' ' + T.sanitize(sel.pointer)));
    for (const l of T.sanitize(sel.preview ?? '').split('\n').flatMap((x) => wrap('   ' + x, w - 1)).slice(0, room)) lines.push(T.text(p)(l));
  }
  return window(lines, ui.scroll, h);
}

function sourcesView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal, list = c.sources ?? [];
  if (!list.length) return ['', T.dim('  Nothing referenced yet.'), '', T.dim('  Documents named in replies, read by tools, @-mentioned by you or found by a search appear here.')];
  const sel = Math.min(ui.src, list.length - 1);
  const lines = [rule(`Sources · ${list.length}`, w, p)];
  list.forEach((s, i) => {
    const anchors = s.anchors.length ? '  ' + s.anchors.map((a) => '#' + a).join(' ') : '';
    const row = `${i === sel ? T.accent('❯') : ' '} ${String(i + 1).padStart(2)}. ${T.link(s.path)}${T.dim(anchors + `  ×${s.count} · ${s.how.join('/')} · ${s.by.join(', ')}`)}`;
    lines.push(i === sel ? chalk.bold(row) : row);
  });
  const cur = list[sel]!;
  const room = Math.max(3, h - lines.length - 3);
  lines.push('', rule('Preview', w, p), T.link(' ' + cur.path));
  for (const l of (c.preview?.(cur.path) ?? []).flatMap((x) => wrap('   ' + x, w - 1)).slice(0, room)) lines.push(T.text(p)(l));
  return window(lines, ui.scroll, h);
}

function notebookView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal, pins = c.notebook ?? [];
  if (!pins.length) return ['', T.dim('  Nothing pinned yet.'), '', T.dim('  /pin keeps the last reply here; /pin <text> keeps a note. The notebook is a markdown document in the mission.')];
  const lines: string[] = [];
  [...pins].reverse().forEach((pin, i) => {
    lines.push(rule(`${pins.length - i} · ${pin.at.replace('T', ' ').slice(0, 16)} · ${pin.source}`, w, p));
    lines.push(...pin.text.split('\n').flatMap((l) => wrap(' ' + l, w - 1)).map(T.text(p)), '');
  });
  return window(lines, ui.scroll, h);
}

const CONN_COLOR: Record<string, (s: string) => string> = { ready: chalk.hex('#22c55e'), idle: T.dim, 'needs login': T.warn, unavailable: T.crimson };
function connectorsView(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const p = c.pal, rows = [...c.obs.connectors.values()];
  if (!rows.length) return ['', T.dim('  No connectors are configured.'), '', T.dim('  Add [connectors.<name>] to config.toml and ratify it (delphi connectors status).')];
  const lines: string[] = [];
  for (const r of rows) {
    const paint = CONN_COLOR[r.state] ?? T.dim;
    lines.push(chalk.bold(T.text(p)(` ${r.name}`)) + '  ' + paint(`${r.state === 'ready' ? '●' : r.state === 'idle' ? '○' : '✖'} ${r.state}`) + T.dim(r.reason ? ` · ${oneLine(r.reason, w - 30)}` : ''));
    lines.push(T.dim(`   ${r.url || '(not in the snapshot)'}`));
    lines.push(T.dim(`   account ${r.account ?? '(none)'} · login ${r.login ?? '(none needed)'} · roles ${r.roles.join(', ') || '—'}`));
    lines.push(T.text(p)(`   ${r.calls} call(s) · ${r.results} result(s)${r.errors ? ` · ${r.errors} error(s)` : ''}${r.denied ? ` · ${r.denied} refused` : ''}${r.lastTool ? ` · last ${r.lastTool}` : ''}`));
    for (const t of r.tools) lines.push(`   ${T.text(p)(t.name.padEnd(24))} ${T.dim(t.class.padEnd(6))} ${t.pinned ? chalk.hex('#22c55e')('pinned') : T.warn('not ratified: delphi connectors pin ' + r.name + ', then /ratify')}`);
    lines.push('');
  }
  return window(lines, ui.scroll, h);
}

/** The panel body: exactly `h` lines of at most `w` columns. */
export function panelBody(ui: PanelUi, c: PanelCtx, w: number, h: number): string[] {
  const title = T.dim(` ${TITLES[ui.view]} · mission ${c.mission || '…'}`);
  const inner = h - 1;
  const body = ui.view === 'council' ? councilView(ui, c, w, inner) : ui.view === 'epic' ? epicView(ui, c, w, inner)
    : ui.view === 'forge' ? forgeView(ui, c, w, inner) : ui.view === 'agents' ? agentsView(ui, c, w, inner)
    : ui.view === 'spend' ? spendView(ui, c, w, inner) : ui.view === 'sources' ? sourcesView(ui, c, w, inner)
    : ui.view === 'notebook' ? notebookView(ui, c, w, inner) : ui.view === 'connectors' ? connectorsView(ui, c, w, inner) : archivesView(ui, c, w, inner);
  return [title, ...body, ...Array(inner).fill('')].slice(0, h).map((l) => T.clip(l, w));
}

/** The prompt row while a panel has the keyboard. */
export function panelHint(ui: PanelUi): string {
  const i = VIEWS.indexOf(ui.view);
  const dots = VIEWS.map((_, n) => (n === i ? T.accent('●') : T.dim('○'))).join('');
  const extra = ui.view === 'archives' ? (ui.kb.editing ? 'enter search · esc stop typing' : '/ search · ↑↓ select') : ui.view === 'forge' ? '←→ build · r refresh' : ui.view === 'council' ? '←→ council' : ui.view === 'epic' || ui.view === 'sources' ? '↑↓ select' : '↑↓ scroll';
  return T.dim(`${SHORT[ui.view]} ${i + 1}/${VIEWS.length} · tab or shift+←→ view · 1-${VIEWS.length} jump · ${extra} · esc chat  `) + dots;
}

/** The line under the panel: spend against the ceiling, tokens, cache, retries, and proposals waiting for /ratify. */
export function panelFooter(c: PanelCtx, w: number): string {
  const o = c.obs;
  const frac = c.cap > 0 ? o.spentUsd / c.cap : 0;
  const colour = frac >= 0.9 ? T.crimson : frac >= 0.75 ? T.warn : chalk.hex('#22c55e');
  return T.clip(T.text(c.pal)(` spend ${money(o.spentUsd)} / ${c.cap > 0 ? money(c.cap) : '?'} `) + colour(bar(frac, 12)) + T.text(c.pal)(` · tokens ${kilo(totalTokens(o.tokens))} · cache ${(o.cacheHitRatio() * 100).toFixed(0)}% · retries ${o.retries}${c.pending > 0 ? ` · ${c.pending} to /ratify` : ''}`), w);
}
