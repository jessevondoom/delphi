// The Observer: six full-window views of the mission, drawn from the daemon's folded state (`observe.state`).
// Read-only, no boxes; structure comes from whitespace and hairlines. Pure: state in, `h` lines of `w` columns out.
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';
import { faceOf, STRIP_ROLES } from './motion.ts';
import { current as councilNow, phaseOf, seatsNow, weave } from './weave.ts';
import { epicThreads } from './epic-threads.ts';
import { dashboard, needs } from './dashboard.ts';
import { joinColumns, leftWidth, TWO_COLUMNS_AT } from './columns.ts';
import * as A from './archives.ts';
import { pulse, timeline, toolsBoard } from './monitors.ts';
import { bar, capMeter, colored, kilo, LOOP_CALLS, LOOP_FAILS, quietNote, spark, type Paint } from './meters.ts';

export type Tokens = { input: number; cache_read: number; cache_write: number; output: number; thinking: number };
export type AgentRow = { role: string; state: string; task_id: string; endpoint: string; tier: string; model: string; upscaled: boolean; tokens: Tokens; cost_usd: number; calls: number };
export type RoleTools = { calls: number; errors: number; last: string; streak?: { tool: string; n: number; failed: number }; open?: { tool: string; since_ms: number } | null; by_tool: Record<string, { calls: number; errors: number }> };
export type OState = {
  agents: Record<string, AgentRow>;
  nodes: Record<string, { id: string; role: string; parent: string | null; depth: number; task_id: string; state: string; waiting?: string; tokens: number; cost_usd: number; started_ms: number; ended_ms: number | null; after: string[] }>;
  /** `tokens` is everything the task's agents used, counted as the mission total is; a daemon that predates it sends none. */
  tasks: Record<string, { id: string; flow: string; prompt: string; status: string; output?: string; session?: string; tokens?: Tokens }>;
  /** The console sessions of the mission, oldest first; the last is the current one (`specs/030`). Work from every session stays on the screen while it runs. */
  sessions?: { id: string; reason: string; started_ms: number; turns: number }[];
  /** Models that are out right now, and tasks running on a stand-in for one (`specs/035` D-7). */
  models_out?: { endpoint_id: string; model_id: string; reason: string; back_ms: number | null }[];
  standins?: { task: string; role: string; from: string; to: string; why: string; by: 'operator' | 'policy' | 'automatic' }[];
  councils: { task_id: string; proposition: string; seats: { role: string; family: string }[]; homogeneous: boolean; phase: string; stances: { role: string; answered: boolean; text: string }[]; critiques: { role: string; recommendation: string; concur: boolean | null; findings: [string, string][] }[]; escalation: string | null; path: string; unresolved: [string, string, string][]; options: [string, string[]][]; lenses?: [string, string][]; lens_note?: string }[];
  builds: { task_id: string; branch: string; attempts: string[]; sha: string; gates: { name: string; state: string; detail: string }[]; findings: [string, string][]; flaky: boolean; merge: string }[];
  spent_usd: number; tokens: Tokens; retries: number; narrative: string[]; series: number[]; pace: number[];
  /** The orchestrator's own grouping of tasks into threads, when it sends one; otherwise the window groups by what the tasks say. */
  threads?: { id: string; label: string; tasks: string[] }[];
  /** Tool use per role over the whole mission: counts and names only (`observe::tool_stats`). */
  tools?: Record<string, RoleTools>;
  /** Approvals the operator has not answered, by id, with when each was raised. */
  approvals?: Record<string, { role: string; summary: string; since_ms: number }>;
  /** What happened since the operator last asked or answered something. */
  since_you?: { since_ms: number; changes: number; done: number; failed: number; approvals: number };
  /** Files the team has touched: who, how often, when last; `clash` is two roles on one file. */
  files?: { path: string; roles: Record<string, number>; writes: number; last_ms: number; clash: boolean }[];
  /** The most tokens one subscription-CLI span may use before Delphi stops it (`harness_token_cap`). */
  token_cap?: number;
  /** Which models report their work: per model, the runs that did work and the ones that recorded a change (`toolreport.rs`). */
  adoption?: { by_model: Record<string, { spans: number; calls: number; errors: number; worked: number; recorded: number }>; total: { spans: number; calls: number; errors: number; worked: number; recorded: number } };
  /** What agents recorded as finished (`record_change`), oldest first: their own plain words, never their reasoning. */
  changes?: { session: string; at_ms: number; role: string; task: string; summary: string; files: string[] }[];
  pending_approvals?: Record<string, string>; cache_hit_ratio: number; default_view: string; mission_id: string; budget: { cap_usd: number }; pending_proposals: number;
};

/** The views, in order (`005e` §9.2; the dashboard is `specs/030`; `changes` is `specs/032`). The narrator shows no money, so there is no spend view. */
export const VIEWS = ['dashboard', 'council', 'epic', 'agents', 'pulse', 'timeline', 'tools', 'archives', 'changes'] as const;
export type View = (typeof VIEWS)[number];
/** The view one step left (-1) or right (+1) of `v`; it stops at the ends instead of wrapping. */
export const stepView = (v: View, d: -1 | 1): View => VIEWS[Math.max(0, Math.min(VIEWS.length - 1, VIEWS.indexOf(v) + d))]!;
export const parseView =(s: string): View | null => {
  const m: Record<string, View> = { dashboard: 'dashboard', now: 'dashboard', ops: 'dashboard', monitor: 'dashboard', council: 'council', chamber: 'council', epic: 'epic', forge: 'epic', diffs: 'epic', agents: 'agents', roster: 'agents', archives: 'archives', kb: 'archives', pulse: 'pulse', meters: 'pulse', timeline: 'timeline', tools: 'tools', changes: 'changes', changelog: 'changes', log: 'changes', thinking: 'changes', reasoning: 'changes' };
  return m[s.toLowerCase()] ?? null;
};

export type OModel = {
  info: T.Info; pal: T.Pal; st: OState; view: View; pick: number; scroll: number; selNode: number;
  /** The knowledge window (`archives.ts`): the search box, the shelf, the results and the reader. */
  arch: A.Arch;
  live: Record<string, string>; connected: boolean;
  /** A running span's token total so far, from the live frames: a subscription CLI logs its usage only when it ends. In memory, never saved. */
  liveTokens?: Record<string, number>;
  /** An estimate of the tokens the call in flight has produced so far (an API seat), from what has streamed. Shown with the live total; it has no cap. */
  liveEst?: Record<string, number>;
  /** When each span was last heard from (a frame or an event), in ms, and when this window first looked: a running agent silent for a while is flagged. In memory. */
  lastSeen?: Record<string, number>;
  /** Tokens per sample for each running role, oldest first (the `pulse` pane's sparklines). In memory, never saved. */
  pulse?: Record<string, number[]>;
  since?: number;
  /** The clock for badge animation, and whether animation is off (`DELPHI_REDUCE_MOTION`, `NO_COLOR`); the strip is static without them. */
  now?: number; still?: boolean;
};

export const emptyState = (): OState => ({
  agents: {}, nodes: {}, tasks: {}, councils: [], builds: [], spent_usd: 0, tokens: { input: 0, cache_read: 0, cache_write: 0, output: 0, thinking: 0 },
  retries: 0, narrative: [], series: [], pace: [], cache_hit_ratio: 0, default_view: 'dashboard', mission_id: '', budget: { cap_usd: 0 }, pending_proposals: 0,
});

const ORDER = ['orchestrator', 'architect', 'builder', 'red-team', 'strategist', 'scout', 'investigator'];
const ordered = (st: OState) => Object.values(st.agents).sort((a, b) => (ORDER.indexOf(a.role) + 99) % 99 - (ORDER.indexOf(b.role) + 99) % 99);
const tokTotal = (t: Tokens) => t.input + t.cache_read + t.cache_write + t.output + t.thinking;
const glyph = (s: string) => ({ done: '✔', green: '✔', merged: '✔', running: '●', escalated: '✖', failed: '✖', red: '✖', refused: '✖', 'waiting on you': '►', pending: '○', idle: '○' } as Record<string, string>)[s] ?? '·';
const stateColor = (s: string): Paint => ({ done: T.ok, green: T.ok, merged: T.ok, running: T.accent, escalated: T.alert, failed: T.alert, red: T.alert, refused: T.alert, 'waiting on you': T.warn } as Record<string, Paint>)[s] ?? T.muted;
const clean = (s: string, n: number) => { const t = sanitize(s).replace(/\n/g, ' '); return [...t].length <= n ? t : [...t].slice(0, Math.max(0, n - 1)).join('') + '…'; };
const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(4, w), { hard: true, trim: false }).split('\n');
const rule = (m: OModel, w: number, title = '') => { const head = title ? `── ${title} ` : ''; return T.rule(m.pal)(head + '─'.repeat(Math.max(0, w - [...head].length))); };
const sevColor = (sev: string, _m: OModel): Paint => (sev === 'P0' ? T.alert : sev === 'P1' ? T.warn : T.muted);
const dim = (s: string) => T.dim(s);
const text = (m: OModel) => T.text(m.pal);

/**
 * The 3-line solid mascot, seven columns (spec 005a): crown in the top row, a painted face, a chin.
 * Every cell is painted with a background colour, never drawn as a half-block glyph: `▄` and `▀` leave seams between
 * neighbours and, where the terminal adds line spacing, a gap between rows. The crown is a cell whose upper half is
 * the terminal's own colour (a `▀` in the background colour on the crown's tint), so it keeps its half height; the
 * chin is a darker shade of the body. Without a known background the crown falls back to `▄`, and without colour the
 * whole figure to glyphs.
 */
export function mascot(role: string, state: string, info: T.Info, pal: T.Pal): [string, string, string] {
  const r = info.roles[role], lib = info.avatar?.roles[role];
  // A role the library has no sprite for (a generic sub-agent) gets a plain slate one.
  const generic = !r && !lib;
  const slice = generic ? '  ▄▄▄  ' : lib?.crown ?? r?.crown;
  if (!slice) return [' '.repeat(7), ' '.repeat(7), ' '.repeat(7)];
  const crownColor = generic ? T.GENERIC_CROWN : lib?.crown_rgb ?? r?.crown_rgb ?? T.CROWN_DEFAULT;
  const chassis = generic ? T.GENERIC_CHASSIS : lib?.chassis ?? r!.chassis;
  const bgc = pal.bg;
  const crown = [...slice].map((c, i) => {
    if (c !== '▄') return ' ';
    const hex = role === 'builder' && i >= 1 && i <= 5 ? T.CROWN_LIGHT : crownColor;
    return T.crownCell(hex, bgc);
  }).join('');
  // The face row comes from the library's data; the built-in rows serve an older `delphi` that does not send it.
  let row = lib?.mascot[T.ACTIVITY[state as T.AgentState] ?? 'idle'];
  if (!row) {
    let face: string;
    if (role === 'scout') face = state === 'waiting on you' ? 'x ? x' : state === 'running' ? '· · ·' : '• • •';
    else if (role === 'builder' && state !== 'waiting on you' && state !== 'escalated') face = '•───•';
    else face = state === 'waiting on you' ? 'x?x' : state === 'escalated' ? 'x_x' : state === 'running' ? '•─•' : r?.face ?? '•_•';
    const pad = 7 - [...face].length;
    row = ' '.repeat(Math.floor(pad / 2)) + face + ' '.repeat(pad - Math.floor(pad / 2));
  }
  return [crown, T.onChassis(chassis, row), T.chin(chassis)];
}

const name = (m: OModel, role: string) => T.roleName(role, m.info);
const roleCol = (m: OModel, role: string) => T.roleColor(role, m.info);

const council = weave;

export function epicOrder(st: OState): { n: OState['nodes'][string]; depth: number }[] {
  const kids = (id: string) => Object.values(st.nodes).filter((x) => x.parent === id);
  const roots = Object.values(st.nodes).filter((x) => !x.parent || !st.nodes[x.parent]);
  const out: { n: OState['nodes'][string]; depth: number }[] = [];
  const walk = (n: OState['nodes'][string], depth: number) => { out.push({ n, depth }); for (const c of kids(n.id)) walk(c, depth + 1); };
  for (const r of roots) walk(r, 0);
  return out;
}

/** The epic as threads of work grouped by what they are about, each with done, now and next (`epic-threads.ts`). */
function epic(m: OModel, w: number, h: number): string[] {
  return epicThreads(m, w, h);
}

/** A full-width tinted band for the row that is running or selected; bold where there is no tint (`005f` slice 10). */
const band = (m: OModel, line: string, w: number): string => {
  const t = T.tint(m.pal.bg ?? null, m.pal.dark);
  const padded = line + ' '.repeat(Math.max(0, w - T.width(line)));
  return t ? t.fill(padded) : chalk.bold(line);
};

/** An agent row for a DAG node whose role has no agent of its own: a generic sub-agent the work spawned. */
const NO_TOKENS: Tokens = { input: 0, cache_read: 0, cache_write: 0, output: 0, thinking: 0 };
const subAgent = (n: OState['nodes'][string]): AgentRow => ({ role: n.role, state: n.state, task_id: n.task_id, endpoint: '', tier: '', model: '', upscaled: false, tokens: { ...NO_TOKENS, input: n.tokens }, cost_usd: n.cost_usd, calls: 0 });

/** Who is at work right now: the agents that are not idle, and every running node whose role has no agent (sub-agents). */
export function activeAgents(st: OState): AgentRow[] {
  const rows = ordered(st).filter((a) => a.state !== 'idle');
  const seen = new Set<string>();
  for (const n of Object.values(st.nodes)) {
    if (n.state !== 'running' || st.agents[n.role] || seen.has(n.role)) continue;
    seen.add(n.role);
    rows.push(subAgent(n));
  }
  return rows;
}

/** Tools that change something: an agent that has called one is expected to record a change (`toolreport.rs`). */
const WORK_TOOLS = new Set(['write_file', 'edit_file', 'run_command', 'write_archive', 'write_html', 'write_chart', 'write_report', 'write_deck', 'write_sheet', 'write_diagram', 'Edit', 'Write', 'MultiEdit', 'NotebookEdit', 'Bash']);

/** `calls · ✎ changes recorded · ✗ failed · → the tool in use or used last` for one agent's role; nothing before its first call. */
function toolsLine(m: OModel, a: AgentRow, lead = true): string {
  const t = m.st.tools?.[a.role];
  if (!t || t.calls === 0) return '';
  const rec = t.by_tool['record_change']?.calls ?? 0;
  const parts = [`${t.calls} ${t.calls === 1 ? 'call' : 'calls'}`];
  // A running agent that has changed something and recorded nothing yet: the count turns to a warning.
  const unreported = !rec && a.state === 'running' && Object.keys(t.by_tool).some((k) => WORK_TOOLS.has(k));
  if (rec) parts.push(`✎ ${rec}`);
  const line = dim((lead ? '  ·  ' : '') + parts.join(' · ')) + (unreported ? colored(' · ✎ 0', T.warn) : '');
  // The same tool again and again, or failing again and again: going round in circles.
  const sk = t.streak;
  const loop = a.state === 'running' && sk && (sk.n >= LOOP_CALLS || sk.failed >= LOOP_FAILS) ? colored(` · ⟳ ${clean(sk.tool, 20)} ×${sk.n}${sk.failed >= LOOP_FAILS ? ` (${sk.failed} failed)` : ''}`, sk.failed >= LOOP_FAILS ? T.alert : T.warn) : '';
  return line + (t.errors ? colored(` · ✗ ${t.errors}`, T.alert) : '') + (a.state === 'running' && t.last ? dim(` · → ${clean(t.last, 24)}`) : '') + loop;
}

/** The roster: a mascot per agent that is working, with its name, role, state in words, model, task and tokens. With none working, the orchestrator alone, and a line saying the system is idle. */
function agents(m: OModel, w: number, _h: number, stacked = false): string[] {
  const out: string[] = [];
  const active = activeAgents(m.st);
  const idle = active.length === 0;
  const orch: AgentRow = m.st.agents.orchestrator ?? { role: 'orchestrator', state: 'idle', task_id: '', endpoint: '', tier: '', model: '', upscaled: false, tokens: NO_TOKENS, cost_usd: 0, calls: 0 };
  for (const a of idle ? [orch] : active) {
    const mk = mascot(a.role, a.state, m.info, m.pal);
    const model = a.model ? `${a.tier} · ${clean(a.model, 28)}${a.upscaled ? ' ⬆' : ''}` : idle ? 'the system is idle' : 'no call yet';
    const nm = name(m, a.role);
    const logged = a.tokens.input + a.tokens.cache_read + a.tokens.cache_write + a.tokens.output + a.tokens.thinking;
    // What the running spans of this role have used so far but not yet logged; shown as an estimate until the span ends.
    const live = Object.values(m.st.nodes).filter((n) => n.role === a.role && n.state === 'running').reduce((t, n) => t + (m.liveTokens?.[n.id] ?? m.liveEst?.[n.id] ?? 0), 0);
    const total = logged + live;
    const meta = [
      roleCol(m, a.role).bold(nm) + dim(`  ${nm === a.role ? '' : a.role + '  '}`) + colored(`${glyph(a.state)} ${a.state}`, stateColor(a.state)) + (a.state === 'running' ? quietNote(m, a.role) : ''),
      text(m)(model) + (a.task_id && a.state !== 'idle' ? dim(` · ${a.task_id}`) : ''),
      dim(`${live > 0 ? '~' : ''}${kilo(total)} tokens`) + capMeter(m, a.role) + (stacked ? '' : toolsLine(m, a)),
    ];
    // In a half-width column the counters wait for a fourth row under the mascot instead of running off the edge.
    const extra = stacked ? toolsLine(m, a, false) : '';
    // The running agent's facts sit on a tinted band; the mascot keeps its own colours.
    mk.forEach((line, i) => {
      const tail = '   ' + meta[i]!;
      out.push('  ' + line + (a.state === 'running' ? band(m, tail, Math.max(0, w - 9)) : tail));
    });
    if (extra) out.push(' '.repeat(12) + extra);
    out.push('');
  }
  if (idle) out.push(dim('  nothing is running: the system is idle. agents appear here while they work.'));
  return out;
}

/** The agents and the tools side by side, on a window wide enough for both (`specs/034`: what each agent does, and what all of them call). */
function agentsAndTools(m: OModel, w: number, h: number): string[] {
  const lw = leftWidth(w), rw = Math.max(0, w - lw - 3);
  return joinColumns(agents(m, lw, h, true), toolsBoard(m, rw, h), w, m.pal);
}

const archives = (m: OModel, w: number, h: number): string[] => A.view(m.arch, w, h, m.pal);

const hhmm = (ms: number) => { const d = new Date(ms); return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`; };

/**
 * The change log: what agents say they finished, one plain line each (`record_change`), grouped by session like the chat,
 * newest first so the latest is on screen without scrolling. Results only: the reasoning behind them is never shown.
 */
function changes(m: OModel, w: number, _h: number): string[] {
  const all = m.st.changes ?? [];
  if (!all.length) return ['', dim('  no changes recorded yet'), '', dim('  agents add a line here when they finish a piece of work: what changed, in plain words'), dim('  it is kept with the mission, so the chat summary and a pull request description can use it')];
  const sessions = m.st.sessions ?? [];
  const last = sessions[sessions.length - 1]?.id ?? '';
  const ids = [...new Set(all.map((c) => c.session))].reverse();
  const out: string[] = [];
  for (const id of ids) {
    const rows = all.filter((c) => c.session === id);
    const row = sessions.find((s) => s.id === id);
    const title = `${id ? `session ${id}` : 'earlier'}${row ? ` · ${hhmm(row.started_ms)}` : ''}${id === last ? ' · now' : ''} · ${rows.length} ${rows.length === 1 ? 'change' : 'changes'}`;
    out.push('', rule(m, w, title));
    for (const c of rows.slice().reverse()) {
      const who = name(m, c.role);
      const lead = `  ${hhmm(c.at_ms)}  `;
      const pad = ' '.repeat(lead.length);
      const body = wrap(clean(c.summary, 300), Math.max(8, w - lead.length - 1));
      out.push(dim(lead) + roleCol(m, c.role).bold(who) + dim(c.task ? `  ${c.task}` : ''));
      out.push(...body.map((l) => pad + text(m)(l)));
      if (c.files.length) out.push(pad + dim(clean(c.files.slice(0, 4).join(', ') + (c.files.length > 4 ? ` +${c.files.length - 4}` : ''), Math.max(8, w - lead.length - 1))));
    }
  }
  return out;
}

/** The strip: the six badges only, in the fixed order, each animated by what its agent is doing (`005e` §10.2). */
function strip(m: OModel, w: number): string {
  const now = m.now ?? 0, still = m.still ?? true;
  // The council view: seats still answering pan their badges; on a verdict every seat shows success (`005e` §9.2).
  const c = m.view === 'council' ? councilNow(m) : undefined;
  const seatsThinking = c ? seatsNow(m, c).thinking : new Set<string>();
  const seatsDone = c && phaseOf(c) === 'verdict' ? new Set(c.seats.map((x) => x.role)) : new Set<string>();
  const roles = STRIP_ROLES.filter((r) => m.info.roles[r]);
  return T.clip(roles.map((r) => {
    if (seatsDone.has(r)) return T.badge(r, 'idle', m.info, m.pal, 'success');
    const a = m.st.agents[r];
    const hot = seatsThinking.has(r);
    const f = faceOf(a || hot ? { state: (hot ? 'running' : a!.state) as never, doing: '', thinkingAt: hot ? now : undefined } : undefined, r, now, still);
    return T.badge(r, f.state, m.info, m.pal, f.frame);
  }).join(' '), w);
}

/** `session 3 · 5 asks`, plus how many tasks of earlier sessions are still running (they stay on the screen). Empty before any session began. */
export function sessionNote(st: OState): string {
  const cur = st.sessions?.[st.sessions.length - 1];
  if (!cur) return '';
  const older = Object.values(st.tasks).filter((t) => t.status === 'running' && t.session !== cur.id).length;
  return `session ${Number(cur.id.replace(/\D/g, '')) || cur.id} · ${cur.turns} ask${cur.turns === 1 ? '' : 's'}${older ? ` · ${older} from before` : ''}`;
}

/** `delphi v0.5 · epic`: directly under the strip, no path and no mission id. */
function titleRow(m: OModel, w: number): string {
  const name = chalk.bold(text(m)('delphi'));
  const conn = m.connected ? '' : ' · disconnected';
  const sess = sessionNote(m.st);
  return T.clip(w < 48 ? name + dim(` · ${m.view}${conn}`) : name + dim(` v${m.info.version} · ${m.view}${sess ? ` · ${sess}` : ''}${conn}`), w);
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
  const items = needs(st);
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
  const fn = { dashboard, council, epic, agents, pulse, timeline, tools: toolsBoard, archives, changes }[m.view];
  let body = m.view === 'agents' && w >= TWO_COLUMNS_AT ? agentsAndTools(m, w, inner) : fn(m, w, inner);
  if (m.view !== 'dashboard' && m.view !== 'council' && m.view !== 'epic' && m.view !== 'archives') body = body.slice(m.scroll);
  body = [slot(m, w), '', ...body.slice(0, inner)].slice(0, bodyH).map((l) => T.clip(l, w));
  while (body.length < bodyH) body.push('');
  return [...head, ...body, ...foot].slice(0, h);
}
