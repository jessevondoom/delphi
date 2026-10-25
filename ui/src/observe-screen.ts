// The Observer: six full-window views of the mission, drawn from the daemon's folded state (`observe.state`).
// Read-only, no boxes; structure comes from whitespace and hairlines. Pure: state in, `h` lines of `w` columns out.
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';
import { faceOf, STRIP_ROLES } from './motion.ts';

export type Tokens = { input: number; cache_read: number; cache_write: number; output: number; thinking: number };
export type AgentRow = { role: string; state: string; task_id: string; endpoint: string; tier: string; model: string; upscaled: boolean; tokens: Tokens; cost_usd: number; calls: number };
export type OState = {
  agents: Record<string, AgentRow>;
  nodes: Record<string, { id: string; role: string; parent: string | null; depth: number; task_id: string; state: string; tokens: number; cost_usd: number; started_ms: number; ended_ms: number | null; after: string[] }>;
  tasks: Record<string, { id: string; flow: string; prompt: string; status: string }>;
  councils: { task_id: string; proposition: string; seats: { role: string; family: string }[]; homogeneous: boolean; phase: string; stances: { role: string; answered: boolean; text: string }[]; critiques: { role: string; recommendation: string; concur: boolean | null; findings: [string, string][] }[]; escalation: string | null; path: string; unresolved: [string, string, string][]; options: [string, string[]][] }[];
  builds: { task_id: string; branch: string; attempts: string[]; sha: string; gates: { name: string; state: string; detail: string }[]; findings: [string, string][]; flaky: boolean; merge: string }[];
  spent_usd: number; tokens: Tokens; retries: number; narrative: string[]; series: number[]; pace: number[];
  pending_approvals?: Record<string, string>; cache_hit_ratio: number; default_view: string; mission_id: string; budget: { cap_usd: number }; pending_proposals: number;
};

/** The six views, in order (`005e` §9.2). The narrator shows no money, so there is no spend view. */
export const VIEWS = ['council', 'epic', 'forge', 'agents', 'archives', 'thinking'] as const;
export type View = (typeof VIEWS)[number];
/** The view one step left (-1) or right (+1) of `v`; it stops at the ends instead of wrapping. */
export const stepView = (v: View, d: -1 | 1): View => VIEWS[Math.max(0, Math.min(VIEWS.length - 1, VIEWS.indexOf(v) + d))]!;
export const parseView =(s: string): View | null => {
  const m: Record<string, View> = { council: 'council', chamber: 'council', epic: 'epic', forge: 'forge', diffs: 'forge', agents: 'agents', roster: 'agents', archives: 'archives', kb: 'archives', thinking: 'thinking', reasoning: 'thinking' };
  return m[s.toLowerCase()] ?? null;
};

export type Hit = { pointer: string; heading: string; preview: string };
export type OModel = {
  info: T.Info; pal: T.Pal; st: OState; view: View; pick: number; scroll: number; selNode: number;
  search: { editing: boolean; query: string; hits: Hit[]; note: string };
  diff: { available: boolean; stat?: string; diff?: string; untracked?: string[] } | null;
  live: Record<string, string>; connected: boolean;
  /** Reasoning per running span, in memory only: never written anywhere, not in the snapshot a reopened window receives. */
  think?: Record<string, string>;
  /** The clock for badge animation, and whether animation is off (`DELPHI_REDUCE_MOTION`, `NO_COLOR`); the strip is static without them. */
  now?: number; still?: boolean;
};

export const emptyState = (): OState => ({
  agents: {}, nodes: {}, tasks: {}, councils: [], builds: [], spent_usd: 0, tokens: { input: 0, cache_read: 0, cache_write: 0, output: 0, thinking: 0 },
  retries: 0, narrative: [], series: [], pace: [], cache_hit_ratio: 0, default_view: 'epic', mission_id: '', budget: { cap_usd: 0 }, pending_proposals: 0,
});

const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];
const ordered = (st: OState) => Object.values(st.agents).sort((a, b) => (ORDER.indexOf(a.role) + 99) % 99 - (ORDER.indexOf(b.role) + 99) % 99);
const kilo = (n: number) => (n >= 10_000 ? `${(n / 1000).toFixed(1)}k` : String(n));
const tokTotal = (t: Tokens) => t.input + t.cache_read + t.cache_write + t.output + t.thinking;
const bar = (f: number, w: number) => { const n = Math.round(Math.min(1, Math.max(0, f)) * w); return '▓'.repeat(n) + '░'.repeat(Math.max(0, w - n)); };
const spark = (v: number[], w: number) => {
  const r = v.slice(-w), max = Math.max(0, ...r);
  return max <= 0 ? '▁'.repeat(r.length) : r.map((x) => '▁▂▃▄▅▆▇█'[Math.round((x / max) * 7)]).join('');
};
const glyph = (s: string) => ({ done: '✔', green: '✔', merged: '✔', running: '●', escalated: '✖', failed: '✖', red: '✖', refused: '✖', 'waiting on you': '►', pending: '○', idle: '○' } as Record<string, string>)[s] ?? '·';
const stateColor = (s: string): Paint => ({ done: T.ok, green: T.ok, merged: T.ok, running: T.accent, escalated: T.alert, failed: T.alert, red: T.alert, refused: T.alert, 'waiting on you': T.warn } as Record<string, Paint>)[s] ?? T.muted;
type Paint = (s: string) => string;
const colored = (s: string, paint: Paint) => paint(s);
const clean = (s: string, n: number) => { const t = sanitize(s).replace(/\n/g, ' '); return [...t].length <= n ? t : [...t].slice(0, Math.max(0, n - 1)).join('') + '…'; };
const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(4, w), { hard: true, trim: false }).split('\n');
const rule = (m: OModel, w: number, title = '') => { const head = title ? `── ${title} ` : ''; return T.rule(m.pal)(head + '─'.repeat(Math.max(0, w - [...head].length))); };
const sevColor = (sev: string, _m: OModel): Paint => (sev === 'P0' ? T.alert : sev === 'P1' ? T.warn : T.muted);
const dim = (s: string) => T.dim(s);
const text = (m: OModel) => T.text(m.pal);

/** The 3-line solid mascot, seven columns (spec 005a): crown in the bottom half of row one, a painted face, a flush chin. */
export function mascot(role: string, state: string, info: T.Info, pal: T.Pal): [string, string, string] {
  const r = info.roles[role], lib = info.avatar?.roles[role];
  const slice = lib?.crown ?? r?.crown;
  if (!slice) return [' '.repeat(7), ' '.repeat(7), ' '.repeat(7)];
  const crownColor = lib?.crown_rgb ?? r?.crown_rgb ?? T.CROWN_DEFAULT;
  const chassis = lib?.chassis ?? r!.chassis;
  const crown = [...slice].map((c, i) => (c !== '▄' ? ' ' : T.tone(role === 'builder' && i >= 1 && i <= 5 ? T.CROWN_LIGHT : crownColor)('▄'))).join('');
  // The face row comes from the library's data; the built-in rows serve an older `delphi` that does not send it.
  let row = lib?.mascot[T.ACTIVITY[state as T.AgentState] ?? 'idle'];
  if (!row) {
    let face: string;
    if (role === 'scout') face = state === 'waiting on you' ? 'x ? x' : state === 'running' ? '· · ·' : '• • •';
    else if (role === 'builder' && state !== 'waiting on you' && state !== 'escalated') face = '•───•';
    else face = state === 'waiting on you' ? 'x?x' : state === 'escalated' ? 'x_x' : state === 'running' ? '•─•' : r!.face;
    const pad = 7 - [...face].length;
    row = ' '.repeat(Math.floor(pad / 2)) + face + ' '.repeat(pad - Math.floor(pad / 2));
  }
  return [crown, T.onChassis(chassis, row), T.tone(chassis)('▀▀▀▀▀▀▀')];
}

const name = (m: OModel, role: string) => T.roleName(role, m.info);
const roleCol = (m: OModel, role: string) => T.roleColor(role, m.info);

function council(m: OModel, w: number, h: number): string[] {
  const n = m.st.councils.length;
  if (!n) return ['', dim('  No Council yet.'), '', dim('  Convene one in the console:  /council <proposition>')];
  const idx = Math.min(m.pick, n - 1), c = m.st.councils[idx]!;
  const live = c.phase === 'round 1' || c.phase === 'round 2';
  const out: string[] = [
    chalk.bold(text(m)(` Council ${idx + 1}/${n} · ${c.task_id} · ${c.phase}${c.homogeneous ? ' · homogeneous (one model family)' : ''}`)),
    text(m)(' ' + clean(c.proposition, w - 3)),
    '',
  ];
  const cols = c.seats.length || 1, gap = 2, cw = Math.floor((w - gap * (cols - 1)) / cols);
  const columns = c.seats.map((seat) => {
    const L: string[] = [roleCol(m, seat.role).bold(`${name(m, seat.role)} (${seat.family})`), rule(m, cw)];
    const s = c.stances.find((x) => x.role === seat.role);
    if (s?.answered) { L.push(dim('Round 1 (blind)'), ...sanitize(s.text).split('\n').map((l) => text(m)(l))); }
    else if (s) L.push(dim(`abstained: ${clean(s.text, 80)}`));
    else {
      L.push(dim('writing… (blind)'));
      const node = Object.values(m.st.nodes).find((nd) => nd.task_id === c.task_id && nd.role === seat.role);
      const lt = node ? m.live[node.id] : undefined;
      if (lt) L.push(...sanitize(lt).split('\n').slice(-8).map((l) => text(m)(l)));
    }
    const cr = c.critiques.find((x) => x.role === seat.role);
    if (cr) {
      L.push('', chalk.bold(text(m)(`Round 2 → ${cr.recommendation || 'unreadable'}${cr.concur === true ? ' · concurs' : cr.concur === false ? ' · does not concur' : ''}`)));
      for (const [sev, title] of cr.findings) L.push(colored(`${sev} ${title}`, sevColor(sev, m)));
    }
    return L.flatMap((l) => wrap(l, cw));
  });
  const verdict: string[] = [rule(m, w, 'Verdict')];
  if (c.phase === 'round 1') verdict.push(dim(' Round 1 is blind: stances appear when every seat has answered.'));
  else if (c.phase === 'round 2') verdict.push(dim(" Round 2: seats are critiquing each other's stances."));
  else {
    if (c.escalation) {
      verdict.push(chalk.bold(colored(` Escalated to you: ${clean(c.escalation, 100)}`, T.warn)));
      for (const [label, seats] of c.options) verdict.push(text(m)(`   option: ${label} — ${seats.join(', ')}`));
    } else verdict.push(colored(' Verdict reached; no deterministic escalation.', T.ok));
    verdict.push(chalk.bold(text(m)(' Minority report:')));
    if (!c.unresolved.length) verdict.push(dim('   none open'));
    for (const [sev, seat, title] of c.unresolved.slice(0, 3)) verdict.push(colored(`   [${sev}] ${seat}: ${clean(title, w - 16)}`, sevColor(sev, m)));
    if (c.path) verdict.push(dim(` saved: ${c.path}`));
  }
  const room = Math.max(3, h - out.length - verdict.length);
  const rows = Math.min(room, Math.max(0, ...columns.map((x) => x.length)));
  const body = Array.from({ length: rows }, (_, i) => columns.map((col) => { const l = col[i + m.scroll] ?? ''; return l + ' '.repeat(Math.max(0, cw - T.width(l))); }).join(' '.repeat(gap)));
  void live;
  return [...out, ...body, ...Array(Math.max(0, room - rows)).fill(''), ...verdict];
}

export function epicOrder(st: OState): { n: OState['nodes'][string]; depth: number }[] {
  const kids = (id: string) => Object.values(st.nodes).filter((x) => x.parent === id);
  const roots = Object.values(st.nodes).filter((x) => !x.parent || !st.nodes[x.parent]);
  const out: { n: OState['nodes'][string]; depth: number }[] = [];
  const walk = (n: OState['nodes'][string], depth: number) => { out.push({ n, depth }); for (const c of kids(n.id)) walk(c, depth + 1); };
  for (const r of roots) walk(r, 0);
  return out;
}

function epic(m: OModel, w: number, h: number): string[] {
  const order = epicOrder(m.st);
  const lines: string[] = [];
  if (!order.length) lines.push(dim('  No agents have run yet in this mission.'));
  const sel = Math.min(m.selNode, Math.max(0, order.length - 1));
  let last = '';
  order.forEach(({ n, depth }, i) => {
    if (n.task_id !== last) {
      last = n.task_id;
      const t = m.st.tasks[n.task_id];
      lines.push(chalk.bold(text(m)(` ${n.task_id} ${t?.flow ?? ''}${t ? ` · ${clean(t.prompt, 60)}` : ''}`)));
    }
    const row = `${name(m, n.role).padEnd(12)} ${n.state.padEnd(10)} ${kilo(n.tokens).padStart(7)} tok`;
    lines.push('  ' + '  '.repeat(depth) + colored(glyph(n.state) + ' ', stateColor(n.state)) + (i === sel ? chalk.inverse(roleCol(m, n.role)(row)) : roleCol(m, n.role)(name(m, n.role).padEnd(12)) + text(m)(row.slice(12))));
  });
  const storyH = 9;
  const treeH = Math.max(3, h - storyH);
  const tree = lines.slice(m.scroll, m.scroll + treeH);
  while (tree.length < treeH) tree.push('');
  const story = [rule(m, w, 'Narrative'), ...m.st.narrative.slice(-(storyH - 1)).map((l) => text(m)(' ' + clean(l, w - 3)))];
  return [...tree, ...story];
}

function forge(m: OModel, w: number, h: number): string[] {
  const n = m.st.builds.length;
  if (!n) return ['', dim('  No builds yet.'), '', dim('  Start one in the console:  /build <request>')];
  const idx = Math.min(m.pick, n - 1), b = m.st.builds[idx]!;
  const head: string[] = [chalk.bold(text(m)(` Build ${idx + 1}/${n} · ${b.task_id} · branch ${b.branch || '…'} · commit ${b.sha || '…'} · ${b.merge || 'in progress'}`))];
  for (const a of b.attempts) head.push(text(m)(`   ${a}`));
  for (const g of b.gates) head.push(colored(`   ${glyph(g.state)} `, stateColor(g.state)) + text(m)(`${g.name.padEnd(10)} ${clean(g.detail, w - 18)}`));
  for (const [sev, title] of b.findings) head.push(colored(`   audit ${sev} ${clean(title, 100)}`, sevColor(sev, m)));
  if (b.flaky) head.push(colored('   a test failed once and passed on a rerun (flaky)', T.warn));
  const diff: string[] = [rule(m, w, 'Diff against the base commit')];
  const d = m.diff;
  if (d?.available) {
    for (const l of sanitize(d.stat ?? '').split('\n')) diff.push(text(m)(l));
    for (const u of d.untracked ?? []) diff.push(colored(`?? ${sanitize(u)}`, T.warn));
    for (const l of sanitize(d.diff ?? '').split('\n')) diff.push(l.startsWith('+') && !l.startsWith('+++') ? colored(l, T.ok) : l.startsWith('-') && !l.startsWith('---') ? colored(l, T.syntax.del) : l.startsWith('@@') ? colored(l, T.accent) : text(m)(l));
  } else diff.push(dim('  no live worktree for this build (it finished, or has not started). r refreshes.'));
  const headH = Math.min(head.length + 1, Math.max(3, h - 4));
  const body = diff.slice(m.scroll === 0 ? 0 : 1 + m.scroll);
  return [...head.slice(0, headH), ...(head.length < headH ? Array(headH - head.length).fill('') : []), ...body];
}

function agents(m: OModel, _w: number, _h: number): string[] {
  const out: string[] = [];
  for (const a of ordered(m.st)) {
    const mk = mascot(a.role, a.state, m.info, m.pal);
    const model = a.model ? `${a.tier} · ${clean(a.model, 28)}${a.upscaled ? ' ⬆' : ''} · ${a.endpoint || '—'}` : 'no call yet';
    const nm = name(m, a.role);
    const meta = [
      roleCol(m, a.role).bold(nm) + colored(`  ${nm === a.role ? '' : a.role + '  '}${glyph(a.state)} ${a.state}`, stateColor(a.state)),
      text(m)(model + (a.task_id && a.state !== 'idle' ? ` · task ${a.task_id}` : '')),
      dim(`in ${kilo(a.tokens.input)} · cache ${kilo(a.tokens.cache_read)} · out ${kilo(a.tokens.output)} · think ${kilo(a.tokens.thinking)} · ${a.calls} call(s)`),
    ];
    mk.forEach((line, i) => out.push('  ' + line + '   ' + meta[i]!));
    out.push('');
  }
  return out;
}

function archives(m: OModel, w: number, _h: number): string[] {
  const s = m.search;
  const out = [rule(m, w, 'Knowledge base')];
  out.push(chalk.bold(text(m)(s.query ? ` last search: ${s.query}` : ' Search specs, context, REVIEW.md and lessons from the console: ctrl+k')));
  if (s.note) out.push(dim(` ${s.note}`));
  s.hits.forEach((hit, i) => {
    out.push(text(m)(` ${i + 1}. ${sanitize(hit.pointer)}`) + dim(`  ${clean(hit.heading, 60)}`));
    for (const l of sanitize(hit.preview).split('\n').slice(0, 3)) out.push(dim(`      ${clean(l, w - 8)}`));
  });
  return out;
}

function thinking(m: OModel, w: number, h: number): string[] {
  const spans = Object.entries(m.think ?? {}).filter(([, t]) => t.trim());
  if (!spans.length) return ['', dim('  Nothing is thinking right now.'), '', dim('  Reasoning of running agents appears here as it streams. It lives only in this window:'), dim('  it is never saved, and a window opened later starts empty.')];
  const out: string[] = [];
  for (const [id, t] of spans) {
    const node = m.st.nodes[id];
    const role = node?.role ?? '?';
    const a = m.st.agents[role];
    out.push('', ' ' + T.badge(role, 'running', m.info, m.pal) + ' ' + roleCol(m, role).bold(name(m, role)) + dim(`  ${a?.tier ?? ''} · ${a?.model ? clean(a.model, 28) : 'model unknown'} · ${node?.task_id ?? ''}`), rule(m, w));
    out.push(...sanitize(t).split('\n').flatMap((l) => wrap(l, w - 4)).slice(-Math.max(4, Math.floor(h / spans.length) - 3)).map((l) => '  ' + chalk.italic(dim(l))));
  }
  return out.slice(-h);
}

/** The strip: the six badges only, in the fixed order, each animated by what its agent is doing (`005e` §10.2). */
function strip(m: OModel, w: number): string {
  const now = m.now ?? 0, still = m.still ?? true;
  const thinkingRoles = new Set(Object.entries(m.think ?? {}).filter(([, t]) => t.trim()).map(([id]) => m.st.nodes[id]?.role));
  const roles = STRIP_ROLES.filter((r) => m.info.roles[r]);
  return T.clip(roles.map((r) => {
    const a = m.st.agents[r];
    const f = faceOf(a ? { state: a.state as never, doing: '', thinkingAt: thinkingRoles.has(r) ? now : undefined } : undefined, r, now, still);
    return T.badge(r, f.state, m.info, m.pal, f.frame);
  }).join(' '), w);
}

/** `delphi v0.5 · epic`: directly under the strip, no path and no mission id. */
function titleRow(m: OModel, w: number): string {
  const name = chalk.bold(text(m)('delphi'));
  const conn = m.connected ? '' : ' · disconnected';
  return T.clip(w < 48 ? name + dim(` · ${m.view}${conn}`) : name + dim(` v${m.info.version} · ${m.view}${conn}`), w);
}

const header = (m: OModel, w: number): string[] => [strip(m, w), titleRow(m, w), ''];

const running = (m: OModel) => Object.values(m.st.agents).filter((a) => a.state === 'running').length;

function footer(m: OModel, w: number): string[] {
  const i = VIEWS.indexOf(m.view);
  const dots = VIEWS.map((_, n) => (n === i ? T.accent('●') : dim('○'))).join(' ');
  const seats = m.view === 'council' ? m.st.councils[Math.min(m.pick, m.st.councils.length - 1)]?.seats.length ?? 0 : 0;
  const left = dim(`${seats ? `${seats} agents` : `${running(m)} running`}  ·  ${kilo(tokTotal(m.st.tokens))} tokens`);
  const right = (w >= 64 ? dim('← →  ·  ') : '') + dots;
  return [T.hairline(m.pal, w), T.spread(left, right, w)];
}

/** What needs the operator, from the daemon's own lists; the count of the rest is appended. */
export function attention(st: OState): string | null {
  const items: string[] = [];
  for (const summary of Object.values(st.pending_approvals ?? {})) items.push(clean(summary, 80));
  for (const c of st.councils) if (c.escalation && c.phase !== 'round 1' && c.phase !== 'round 2') items.push(`council ${c.task_id} needs a decision`);
  if (st.pending_proposals > 0) items.push(`${st.pending_proposals} proposal(s) await /ratify`);
  if (!items.length) return null;
  return items[0] + (items.length > 1 ? `  (+${items.length - 1} more)` : '');
}

/** Body row 1 of every view, never removed so the body does not jump (`005e` §9.2). */
function slot(m: OModel, w: number): string {
  const what = attention(m.st);
  if (!what) return ' ' + colored('✔', T.ok) + dim(' nothing needs you');
  const left = ` ▼ needs you   ${what}`, right = w >= 64 ? 'answer in the console ' : '';
  const row = T.spread(left, right, w);
  return T.band(row + ' '.repeat(Math.max(0, w - T.width(row))), m.pal.bg);
}

export function observeFrame(m: OModel, w: number, h: number): string[] {
  if (w < 28 || h < 9) return [`narrator needs 28x9`, `window is ${w}x${h}`].map((l) => [...l].slice(0, Math.max(0, w)).join('')).concat(Array(Math.max(0, h - 2)).fill('')).slice(0, Math.max(0, h));
  const head = header(m, w), foot = footer(m, w);
  const bodyH = Math.max(1, h - head.length - foot.length);
  const inner = Math.max(1, bodyH - 2); // the attention slot and a blank row sit above every view
  const fn = { council, epic, forge, agents, archives, thinking }[m.view];
  let body = fn(m, w, inner);
  if (m.view !== 'council' && m.view !== 'epic' && m.view !== 'forge' && m.view !== 'thinking') body = body.slice(m.scroll);
  body = [slot(m, w), '', ...body.slice(0, inner)].slice(0, bodyH).map((l) => T.clip(l, w));
  while (body.length < bodyH) body.push('');
  return [...head, ...body, ...foot].slice(0, h);
}
