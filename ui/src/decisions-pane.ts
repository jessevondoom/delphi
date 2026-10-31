// The decisions pane's list (specs/044 §5): every decision the operator owes, one entry each, tier-coloured titles, a
// filter by kind, and tick boxes only where the daemon says a default is safe. Pure: the list from `decision.list`
// and a key press go in, lines and an action come out. The daemon decides what is tickable; nothing here does.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import * as T from './theme.ts';

type Style = (s: string) => string;
export type Choice = { id: string; label: string; effect: string; reversible: boolean };
export type Brief = { summary: string; changes: string[]; recommendation: { choice_id: string; why: string } | null; state: string; /** Tokens a model spent writing it (0 when it was not written by one). */ tokens?: number };
/** One line of what was said about a decision (the daemon's thread, specs/044 §7.3). */
export type Entry = { role: string; kind: string; text: string; choice_id: string | null; conditions: string[]; tokens: number };
/** A model's reading of what the operator typed: one of the decision's own choices and the notes that go with it. Nothing is decided by it. */
export type Reading = { choice_id: string; label: string; conditions: string[] };
export const TYPED_MAX = 600;
export type Decision = {
  id: string; source: string; source_id: string; label: string; kind: string; tier: 'red' | 'amber' | 'open'; title: string;
  requester: string; task: string; raised_at: string | null; payload_blake3: string; brief: Brief; choices: Choice[];
  tickable: boolean; tickable_reason: string;
};
/**
 * Where the operator is in the pane. A tick names the payload it was given for, so a changed payload loses it. `open`
 * is the decision whose detail view shows (null: the list); `opt` is the choice the focus is on (null: none, which is
 * how a decision with no recommendation starts); `answered` is the decision an answer has been sent for, so a second
 * press cannot send it twice; `note` is the line that confirms the last answer.
 */
export type Pane = { /** What is being typed in the detail view's prompt, the saved thread of the open decision, a model's reading awaiting confirmation, a reply in flight, and the cost line of the last reply. */ draft: string; thread: Entry[]; reading: Reading | null; busy: boolean; rnote: string; sel: number; filter: string | null; ticks: string[]; open: string | null; opt: number | null; payload: boolean; openedAt: number; answered: string | null; note: string; /** The confirm page for the ticked decisions is showing, since `confirmedAt`. */ confirm: boolean; confirmedAt: number; /** Decisions (id@payload hash) whose detail view has been shown: only these can be ticked, so a bulk accept never covers what was not rendered. */ seen: string[] };
export const newPane = (): Pane => ({ draft: '', thread: [], reading: null, busy: false, rnote: '', sel: 0, filter: null, ticks: [], open: null, opt: null, payload: false, openedAt: 0, answered: null, note: '', confirm: false, confirmedAt: 0, seen: [] });

/** `answer` is the operator choosing; `accept-all` is the ticked recommendations accepted together (specs/044 §5.5). The console turns each into the existing call for the decision's source. */
export type Action =
  | { t: 'none' } | { t: 'chat' }
  /** `conditions` is set when the operator confirmed a reading of what they typed: the notes are saved and delivered, never executed. */
  | { t: 'answer'; d: Decision; choice: Choice; conditions?: string[] }
  | { t: 'accept-all'; ds: Decision[] }
  /** A decision was opened: load its saved thread. `reply`: the operator typed words about it. `retry`: write its brief again. */
  | { t: 'thread'; d: Decision } | { t: 'reply'; d: Decision; text: string } | { t: 'retry'; d: Decision };

/** The keys that answer are ignored for this long after a decision opens (spec 005's guard). */
export const GUARD_MS = 300;

const KIND_ORDER = ['action', 'spec', 'release', 'build', 'council', 'budget', 'question', 'learn'];
const KIND_LABEL: Record<string, string> = { action: 'actions', spec: 'specs', release: 'releases', build: 'builds', council: 'councils', budget: 'budgets', question: 'questions', learn: 'to learn' };
const TIER_WORD = { red: 'RED', amber: 'AMBER', open: 'OPEN' } as const;
const KIND_WORD: Record<string, string> = { action: 'action', spec: 'spec', release: 'release', build: 'build', council: 'council', budget: 'budget', question: 'question', learn: 'learn' };

const str = (v: unknown, n = 400): string => T.sanitize(typeof v === 'string' ? v : '').slice(0, n);

/** The list from `decision.list`, text cleaned of terminal escapes, anything malformed dropped. */
export function parse(r: any): Decision[] {
  const out: Decision[] = [];
  for (const d of Array.isArray(r?.decisions) ? r.decisions : []) {
    if (!d || typeof d.id !== 'string' || typeof d.kind !== 'string' || !d.brief || typeof d.brief !== 'object') continue;
    const rec = d.brief.recommendation;
    out.push({
      id: str(d.id, 120), source: str(d.source, 20), source_id: str(d.source_id, 120), label: str(d.label, 40), kind: str(d.kind, 20),
      tier: d.tier === 'red' ? 'red' : d.tier === 'amber' ? 'amber' : 'open', title: str(d.title, 100),
      requester: str(d.requester, 40), task: str(d.task, 40), raised_at: typeof d.raised_at === 'string' ? d.raised_at : null, payload_blake3: str(d.payload_blake3, 128),
      brief: {
        tokens: typeof d.brief.tokens === 'number' ? d.brief.tokens : 0,
        summary: str(d.brief.summary, 400), changes: (Array.isArray(d.brief.changes) ? d.brief.changes : []).slice(0, 5).map((c: unknown) => str(c, 120)),
        recommendation: rec && typeof rec.choice_id === 'string' ? { choice_id: str(rec.choice_id, 40), why: str(rec.why, 200) } : null, state: str(d.brief.state, 20),
      },
      choices: (Array.isArray(d.choices) ? d.choices : []).map((c: any) => ({ id: str(c?.id, 40), label: str(c?.label, 80), effect: str(c?.effect, 40), reversible: c?.reversible === true })),
      tickable: d.tickable === true, tickable_reason: str(d.tickable_reason, 120),
    });
  }
  return out;
}

export const visible = (items: Decision[], filter: string | null): Decision[] => (filter ? items.filter((d) => d.kind === filter) : items);

/** The kinds present, in the order of specs/044 §5.1, with how many of each. */
export function chips(items: Decision[]): { kind: string; n: number }[] {
  const n = new Map<string, number>();
  for (const d of items) n.set(d.kind, (n.get(d.kind) ?? 0) + 1);
  const known = KIND_ORDER.filter((k) => n.has(k));
  const other = [...n.keys()].filter((k) => !KIND_ORDER.includes(k)).sort();
  return [...known, ...other].map((kind) => ({ kind, n: n.get(kind)! }));
}

const tickKey = (d: Decision) => `${d.id}@${d.payload_blake3}`;
/** A decision can be ticked when the daemon says so and its payload has been shown in this pane (Red is never tickable). */
export const canTick = (p: Pane, d: Decision): boolean => d.tickable && p.seen.includes(tickKey(d));
export const isTicked = (p: Pane, d: Decision): boolean => canTick(p, d) && p.ticks.includes(tickKey(d));
/** Record that the detail view of these decisions has been shown. */
export const markSeen = (p: Pane, ds: Decision[]): Pane => ({ ...p, seen: [...new Set([...p.seen, ...ds.map(tickKey)])] });
/** How many tickable decisions in `rows` have not been opened yet. */
export const unopened = (p: Pane, rows: Decision[]): number => rows.filter((d) => d.tickable && !canTick(p, d)).length;
export const ticked = (p: Pane, items: Decision[]): Decision[] => items.filter((d) => isTicked(p, d));

/** After the list changes: keep the selection in range, drop a filter nothing matches, and drop ticks whose decision
 * is gone, is no longer tickable, or whose payload changed (specs/044 §5.4). */
export function reconcile(p: Pane, items: Decision[]): Pane {
  const filter = p.filter && items.some((d) => d.kind === p.filter) ? p.filter : null;
  const live = new Set(items.filter((d) => d.tickable).map(tickKey));
  const rows = visible(items, filter);
  const sel = Math.max(0, Math.min(p.sel, rows.length - 1));
  const ticks = p.ticks.filter((t) => live.has(t));
  const base = { ...p, filter, ticks, sel, confirm: p.confirm && ticks.length > 0, seen: p.seen.filter((t) => live.has(t)) };
  if (p.open === null) return base;
  const at = rows.findIndex((d) => d.id === p.open);
  if (at >= 0) return { ...markSeen(base, [rows[at]!]), sel: at };
  // The decision being read is gone (it was answered): move to the next one in this filter, or leave the detail view.
  const next = rows[sel];
  return next ? { ...markSeen(base, [next]), ...fresh, open: next.id, opt: initialOpt(next), payload: false, answered: null } : { ...base, ...fresh, open: null, opt: null, payload: false, answered: null };
}

/** Typing state of a decision that was just opened. */
const fresh = { draft: '', thread: [] as Entry[], reading: null as Reading | null, busy: false, rnote: '' };
const recIndex = (d: Decision): number => (d.brief.recommendation ? d.choices.findIndex((c) => c.id === d.brief.recommendation!.choice_id) : -1);
/** The focus starts on the recommended choice; a decision with no recommendation starts with none selected. */
export const initialOpt = (d: Decision): number | null => { const i = recIndex(d); return i >= 0 ? i : null; };
const openAt = (p: Pane, rows: Decision[], i: number, now: number): Pane => ({ ...markSeen(p, [rows[i]!]), ...fresh, sel: i, open: rows[i]!.id, opt: initialOpt(rows[i]!), payload: false, openedAt: now, answered: null });

/** Clear the sent mark after a failed answer so the operator can try again. */
export const unsent = (p: Pane): Pane => ({ ...p, answered: null, note: '', busy: false });

const entries = (v: unknown): Entry[] => (Array.isArray(v) ? v : []).slice(-40).filter((e: any) => e && typeof e.text === 'string').map((e: any) => ({
  role: str(e.role, 10), kind: str(e.kind, 12), text: str(e.text, 700), choice_id: typeof e.choice_id === 'string' ? str(e.choice_id, 40) : null,
  conditions: (Array.isArray(e.conditions) ? e.conditions : []).slice(0, 3).map((c: unknown) => str(c, 120)), tokens: typeof e.tokens === 'number' ? e.tokens : 0,
}));
/** The saved thread of decision `id` arrived (from `decision.get`); ignored if another decision is open now. */
export const withThread = (p: Pane, id: string, thread: unknown): Pane => (p.open === id ? { ...p, thread: entries(thread) } : p);
/** The daemon's answer to typed words: a reply to a question, or a reading to confirm. The thread now includes both. */
export function replied(p: Pane, id: string, r: any): Pane {
  if (p.open !== id) return p;
  const reading = r?.intent === 'answer' && r.reading && typeof r.reading.choice_id === 'string'
    ? { choice_id: str(r.reading.choice_id, 40), label: str(r.reading.label, 80), conditions: (Array.isArray(r.reading.conditions) ? r.reading.conditions : []).slice(0, 3).map((c: unknown) => str(c, 120)) }
    : null;
  const tokens = typeof r?.tokens === 'number' ? r.tokens : 0;
  return { ...p, busy: false, thread: entries(r?.thread), reading, rnote: `that reply used ${tokens} tokens and is saved with the decision` };
}

export function key(p: Pane, items: Decision[], e: Ev, now = 0): { pane: Pane; action: Action } {
  const none = (q: Pane = p) => ({ pane: q, action: { t: 'none' } as Action });
  if (e.t !== 'key' && e.t !== 'text' && e.t !== 'ctrl' && e.t !== 'paste') return none();
  const rows = visible(items, p.filter);
  const sel = Math.max(0, Math.min(p.sel, rows.length - 1));
  const d = rows[sel];
  const k = e.t === 'key' ? e.k : e.t === 'text' ? e.s : '';
  if (p.confirm) {
    // The confirm page (specs/044 §5.5): enter accepts what was ticked, esc goes back. The guard applies to the accept.
    if (k === 'esc' || (e.t === 'key' && k === 'left' && !e.alt && !e.ctrl)) return none({ ...p, confirm: false });
    if (k === 'enter' || k === 'y') {
      const ds = ticked(p, items);
      if (!ds.length) return none({ ...p, confirm: false });
      if (now - p.confirmedAt < GUARD_MS) return none();
      return { pane: { ...p, confirm: false, ticks: [], note: `✔ accepting ${ds.length} recommendation${ds.length === 1 ? '' : 's'}…` }, action: { t: 'accept-all', ds } };
    }
    return none();
  }
  if (p.open !== null) {
    const at = rows.findIndex((x) => x.id === p.open);
    const cur = rows[at];
    if (!cur) return none({ ...p, open: null });
    const answer = (c: Choice | undefined, conditions?: string[]) => {
      if (!c || p.answered === cur.id || now - p.openedAt < GUARD_MS) return none();
      return { pane: { ...p, answered: cur.id, draft: '', reading: null, note: `✔ ${cur.label} decided: ${c.label.toLowerCase()}` }, action: { t: 'answer', d: cur, choice: c, ...(conditions ? { conditions } : {}) } as Action };
    };
    const leave = (): Pane => ({ ...p, ...fresh, open: null, opt: null, payload: false });
    if (e.t === 'ctrl') {
      if (e.c === 'd') return none({ ...p, payload: !p.payload });
      if (e.c === 'u') return none({ ...p, draft: '' });
      if (e.c === 'r' && cur.source === 'card' && cur.brief.state === 'plain') return { pane: p, action: { t: 'retry', d: cur } };
      return none();
    }
    if (e.t === 'paste') return none({ ...p, draft: (p.draft + e.s.replace(/\s+/g, ' ')).slice(0, TYPED_MAX) });
    // Esc takes back what was started: the words, then the reading, then the view.
    if (k === 'esc') return p.draft ? none({ ...p, draft: '' }) : p.reading ? none({ ...p, reading: null }) : none(leave());
    if (e.t === 'key' && k === 'left' && !e.alt && !e.ctrl) return p.draft ? none() : none(leave());
    if (k === 'up' || k === 'down') {
      const to = at + (k === 'up' ? -1 : 1);
      return to < 0 || to >= rows.length ? none() : { pane: openAt(p, rows, to, now), action: { t: 'thread', d: rows[to]! } };
    }
    if (k === 'tab' || k === 'shift-tab') {
      const n = cur.choices.length;
      if (!n) return none();
      const back = k === 'shift-tab';
      return none({ ...p, reading: null, opt: p.opt === null ? (back ? n - 1 : 0) : (p.opt + (back ? n - 1 : 1)) % n });
    }
    if (k === 'backspace') return none({ ...p, draft: p.draft.slice(0, -1) });
    if (k === 'enter') {
      const text = p.draft.trim();
      if (text) {
        // A bare choice (`2`, `accept`, a choice's own name) is the choice itself, not a question for a model.
        const low = text.toLowerCase();
        const rec = cur.brief.recommendation;
        const bare = /^[1-9]$/.test(low) ? cur.choices[Number(low) - 1] : (low === 'accept' || low === 'accept recommendation') && rec ? cur.choices.find((c) => c.id === rec.choice_id) : cur.choices.find((c) => c.id === low || c.label.toLowerCase() === low);
        if (bare) return answer(bare);
        if (p.busy) return none();
        return { pane: { ...p, draft: '', busy: true, reading: null, rnote: '' }, action: { t: 'reply', d: cur, text } };
      }
      if (p.reading) return answer(cur.choices.find((c) => c.id === p.reading!.choice_id), p.reading.conditions);
      return answer(p.opt === null ? undefined : cur.choices[p.opt]);
    }
    if (e.t === 'text') {
      if (!p.draft && /^[1-9]$/.test(k)) return answer(cur.choices[Number(k) - 1]);
      return none({ ...p, draft: (p.draft + k).slice(0, TYPED_MAX) });
    }
    return none();
  }
  if (k === 'esc') return p.filter ? none({ ...p, filter: null, sel: 0 }) : { pane: p, action: { t: 'chat' } };
  if (e.t === 'key' && k === 'left' && !e.alt && !e.ctrl) return { pane: p, action: { t: 'chat' } };
  if (k === 'up' || k === 'down') return none({ ...p, sel: Math.max(0, Math.min(rows.length - 1, sel + (k === 'up' ? -1 : 1))) });
  if (k === 'enter') return d ? { pane: openAt(p, rows, sel, now), action: { t: 'thread', d } } : none();
  if (k === 'f') {
    const kinds = chips(items).map((c) => c.kind);
    const at = p.filter ? kinds.indexOf(p.filter) : -1;
    const next = at + 1 >= kinds.length ? null : kinds[at + 1]!;
    return none({ ...p, filter: next, sel: 0 });
  }
  if (k === ' ' && d) {
    if (!d.tickable) return none();
    if (!canTick(p, d)) return none({ ...p, sel });
    const t = tickKey(d);
    return none({ ...p, sel, ticks: p.ticks.includes(t) ? p.ticks.filter((x) => x !== t) : [...p.ticks, t] });
  }
  if (k === 'y') return ticked(p, items).length ? none({ ...p, confirm: true, confirmedAt: now }) : none();
  if (k === 'a') {
    // Every tickable decision in the filter, never more: a filter hides entries, it does not widen the batch.
    const ts = rows.filter((x) => canTick(p, x));
    if (!ts.length) return none();
    const keys = ts.map(tickKey);
    const all = keys.every((x) => p.ticks.includes(x));
    return none({ ...p, ticks: all ? p.ticks.filter((x) => !keys.includes(x)) : [...new Set([...p.ticks, ...keys])] });
  }
  return none();
}

const tierStyle = (t: Decision['tier']): Style => (t === 'red' ? T.crimson : t === 'amber' ? T.auto : (s: string) => chalk.bold(s));

function wrap(s: string, w: number): string[] {
  const out: string[] = [];
  let cur = '';
  for (const word of s.split(/\s+/).filter(Boolean)) {
    if (cur && T.width(cur + ' ' + word) > w) { out.push(cur); cur = word; } else cur = cur ? cur + ' ' + word : word;
  }
  if (cur) out.push(cur);
  return out;
}

/** `9 m`, `2 h`, `3 d`: how long ago. */
export function age(ms: number): string {
  const m = Math.max(0, Math.round(ms / 60000));
  return m < 60 ? `${m} m` : m < 1440 ? `${Math.round(m / 60)} h` : `${Math.round(m / 1440)} d`;
}

export type Ctx = {
  pal: T.Pal; now: number; w: number; h: number;
  /** When a decision was raised, in ms, if the console knows. */
  at: (d: Decision) => number | null;
  /** The diff or payload lines of a decision, when the console holds them (a card's preview); otherwise its listed changes show. */
  payload?: (d: Decision) => string[] | null;
};

const recLine = (d: Decision): string => {
  const rec = d.brief.recommendation;
  if (!rec) return T.dim('no recommendation: your call.' + (d.tier === 'red' ? ' Red actions are decided one at a time.' : ''));
  const label = (d.choices.find((c) => c.id === rec.choice_id)?.label ?? rec.choice_id).toLowerCase();
  return T.ok(`✔ recommend ${label}`) + T.dim(` · ${rec.why}` + (d.tier === 'red' ? '. Red: decide individually.' : ''));
};

/** The list's body: exactly `h` lines. */
export function view(items: Decision[], p: Pane, c: Ctx): string[] {
  const { w, h } = c;
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  const rows = visible(items, p.filter);
  const sel = Math.max(0, Math.min(p.sel, rows.length - 1));
  const compact = w < 90;
  const tickable = rows.filter((d) => d.tickable).length;
  const count = p.filter
    ? chalk.bold(T.text(c.pal)(`${rows.length} ${KIND_LABEL[p.filter] ?? p.filter}`)) + T.dim(` · hiding ${items.length - rows.length} other decision${items.length - rows.length === 1 ? '' : 's'}`)
    : chalk.bold(T.text(c.pal)(`${items.length} decision${items.length === 1 ? '' : 's'}`)) + T.dim(tickable ? ` · ${tickable} can take the recommendation` : ' · none can take a default');
  const nTicked = ticked(p, items).length;
  const tickHint = nTicked ? T.accent('y') + '  ' + T.text(c.pal)(`review ${nTicked} ticked`) : tickable ? T.accent('a') + '  ' + T.text(c.pal)(p.filter ? 'tick every recommended item here' : 'tick every recommended item') : '';
  const chipRow = (() => {
    const all = [{ kind: '', n: items.length }, ...chips(items)];
    const parts = all.map(({ kind, n }) => {
      const on = (p.filter ?? '') === kind;
      const label = `${on ? '●' : '○'} ${kind ? KIND_LABEL[kind] ?? kind : 'all'} ${n}`;
      const ds = kind ? items.filter((d) => d.kind === kind) : [];
      const tier = ds.some((d) => d.tier === 'red') ? 'red' : ds.some((d) => d.tier === 'amber') ? 'amber' : 'open';
      return kind ? (on ? chalk.bold(tierStyle(tier)(label)) : tier === 'open' ? T.dim(label) : tierStyle(tier)(label)) : on ? chalk.bold(T.text(c.pal)(label)) : T.dim(label);
    });
    return T.dim('show  ') + parts.join('   ');
  })();
  const unread = unopened(p, rows);
  const head = ['', '  ' + T.spread(count, tickHint, w - 4), '  ' + T.spread(chipRow, T.dim('f change'), w - 4), unread ? '  ' + T.warn(T.clip(`${unread} not opened yet: open one (enter) to read it before it can be ticked`, w - 4)) : ''];
  const per = compact ? 4 : 5;
  const cap = Math.max(1, Math.floor((h - head.length + (compact ? 0 : 1)) / per));
  const start = Math.min(Math.max(0, sel - cap + 1), Math.max(0, rows.length - cap));
  const body: string[] = [];
  rows.slice(start, start + cap).forEach((d, i) => {
    const on = start + i === sel;
    const style = tierStyle(d.tier);
    const box = d.tickable ? (isTicked(p, d) ? T.ok(chalk.bold('[x]')) : canTick(p, d) ? T.dim('[ ]') : T.dim('[?]')) : '   ';
    const when = c.at(d);
    const tag = (d.tier === 'open' ? T.dim : style)(`${TIER_WORD[d.tier]} · ${KIND_WORD[d.kind] ?? d.kind}`) + (when === null ? '' : '  ' + T.dim(age(c.now - when)));
    const left = (on ? T.accent('❯') : ' ') + ' ' + box + ' ' + T.dim(d.label) + '  ' + (on ? chalk.bold(style(d.title)) : style(d.title));
    body.push('  ' + T.spread(left, tag, w - 4));
    const sum = wrap(d.brief.summary, w - 12);
    const shown = compact ? sum.slice(0, 1) : sum.slice(0, 2);
    shown.forEach((l, j) => body.push('        ' + T.clip(l + (j === shown.length - 1 && sum.length > shown.length ? ' …' : ''), w - 10)));
    body.push('        ' + T.clip(recLine(d), w - 10));
    if (!compact) body.push('');
  });
  return pad([...head, ...body]);
}


const card = (c: Ctx, lines: string[]): string[] => {
  const t = T.tint(c.pal.bg ?? null, c.pal.dark);
  return lines.map((l) => {
    const row = ' ' + l + ' '.repeat(Math.max(0, c.w - 7 - T.width(l))) + ' ';
    return '  ' + (t ? t.fill(row) : row);
  });
};

/** The detail view of the open decision (specs/044 §6.2): what it is, what it changes, the recommendation and the choices. */
export function detail(items: Decision[], p: Pane, c: Ctx): string[] {
  const { w, h } = c;
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  const rows = visible(items, p.filter);
  const at = rows.findIndex((x) => x.id === p.open);
  const d = rows[at];
  if (!d) return view(items, { ...p, open: null }, c);
  const style = tierStyle(d.tier);
  const when = c.at(d);
  const rec = d.brief.recommendation;
  const ri = recIndex(d);
  const L: string[] = [
    '',
    '  ' + T.spread(T.dim('‹ decisions'), T.dim(`${at + 1} of ${rows.length} · ↑ previous · ↓ next`), w - 4),
    '',
    '  ' + T.dim(d.label) + '  ' + chalk.bold(style(d.title)),
    '  ' + (d.tier === 'open' ? T.dim : style)(`${TIER_WORD[d.tier]} · ${KIND_WORD[d.kind] ?? d.kind}`) + T.dim(` · requested by ${d.requester || 'an agent'}${d.task ? ` · task ${d.task}` : ''}${when === null ? '' : ` · waiting ${age(c.now - when)}`}`),
    '  ' + T.dim(T.clip(`ctrl+d shows ${d.source === 'card' ? 'the diff or payload' : 'the changes'}${d.brief.state === 'plain' ? ' · written from the card’s own facts, not by a model' + (d.source === 'card' ? ' · ctrl+r asks a model once' : '') : d.brief.state === 'generated' ? ` · summary cached, written once (${d.brief.tokens ?? 0} tokens)` : ''}`, w - 4)),
    '',
  ];
  if (p.payload) {
    const lines = c.payload?.(d) ?? d.brief.changes;
    L.push('  ' + chalk.bold(d.source === 'card' ? 'PAYLOAD' : 'CHANGES'));
    for (const l of lines.slice(0, Math.max(1, h - L.length - 9))) L.push('  ' + T.dim(T.clip(l, w - 4)));
  } else {
    L.push('  ' + chalk.bold('WHAT IT IS'));
    for (const l of wrap(d.brief.summary, w - 4)) L.push('  ' + l);
    L.push('', '  ' + chalk.bold('WHAT IT ADDS OR CHANGES'));
    if (d.brief.changes.length) for (const ch of d.brief.changes) L.push('   ' + T.dim('▪') + ' ' + T.clip(ch, w - 8));
    else L.push('   ' + T.dim('nothing listed'));
  }
  L.push('');
  if (rec && ri >= 0) {
    const why = rec.why ? rec.why[0]!.toUpperCase() + rec.why.slice(1) + '.' : '';
    const btn = p.opt === ri ? chalk.inverse('[ Accept recommendation ]') : chalk.bold('[ Accept recommendation ]');
    const others = d.choices.map((ch, i) => ({ ch, i })).filter((x) => x.i !== ri).map((x) => T.accent(String(x.i + 1)) + ' ' + (p.opt === x.i ? chalk.bold(x.ch.label) : x.ch.label)).join('   ');
    L.push(...card(c, [
      chalk.bold('RECOMMENDATION') + (d.tier === 'red' ? '  ' + T.crimson('Red: you decide, nothing is ticked for you') : ''),
      '',
      ...wrap(`${d.choices[ri]!.label}. ${why}`, w - 10),
      '',
      btn + T.dim(`  enter, or ${ri + 1}`),
      ...(others ? [T.dim('other choices') + '   ' + others] : []),
    ]));
  } else {
    L.push(...card(c, [
      chalk.bold('NO RECOMMENDATION') + T.dim('  your call'),
      '',
      ...wrap('There is no default for this one, so there is no accept button and nothing is preselected.' + (d.tier === 'red' ? ' Red actions are decided one at a time.' : ''), w - 10),
      '',
      ...d.choices.map((ch, i) => (p.opt === i ? T.accent('❯ ') : '  ') + T.accent(String(i + 1)) + '  ' + (p.opt === i ? chalk.bold(ch.label) : ch.label)),
    ]));
  }
  if (p.reading) {
    const ci = d.choices.findIndex((x) => x.id === p.reading!.choice_id);
    L.push('', ...card(c, [
      chalk.bold('READ AS') + T.dim('  nothing is decided yet'),
      '',
      T.accent(String(ci + 1)) + '  ' + chalk.bold(p.reading.label),
      ...p.reading.conditions.map((x) => T.accent('+') + '  ' + T.clip(x, w - 14)),
      '',
      chalk.inverse('[ Confirm this ]') + T.dim('  enter') + T.dim('   keep typing to change it   esc discard'),
    ]));
  }
  if (p.rnote) L.push('', '  ' + T.dim(p.rnote));
  if (p.thread.length) {
    L.push('', '  ' + chalk.bold('EARLIER ON THIS DECISION') + T.dim(' · saved with it, 0 tokens to reopen'));
    for (const e of p.thread.slice(-4)) {
      const who = e.role === 'you' ? T.accent('you  ') : T.dim('model');
      const what = e.kind === 'reading' ? `read as: ${e.text}${e.conditions.length ? ' + ' + e.conditions.join(' + ') : ''}` : e.kind === 'confirmed' ? 'confirmed the reading' : e.text;
      L.push('   ' + who + ' ' + T.clip(what, w - 12));
    }
  }
  if (p.answered === d.id) L.push('', '  ' + T.dim('sent…'));
  const out = pad(L);
  out[out.length - 1] = '  ' + T.dim(T.clip(hint(items, p), w - 4));
  return out;
}

/** Nothing is owed: the last answer, if this console just gave one, and the plain statement of the spec (§6.2). */
export function empty(p: Pane, c: Ctx): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, c.h - a.length)).fill('')].slice(0, c.h);
  return pad(['', p.note ? '  ' + T.ok(p.note) : '', '', '', '  ' + chalk.bold(T.text(c.pal)('Nothing needs you.')), '', '  ' + T.dim('Decisions from every agent arrive here; the header shows an alert when one is waiting.')]);
}

/** What accepting the recommendation of `d` does, in one line. */
const effectLine = (d: Decision): string => {
  const rec = d.brief.recommendation;
  const label = (d.choices.find((x) => x.id === rec?.choice_id)?.label ?? rec?.choice_id ?? 'accept').toLowerCase();
  return `${label} · ${d.requester || 'an agent'}'s ${KIND_WORD[d.kind] ?? d.kind}${d.task ? ` on task ${d.task}` : ''}`;
};

/** The confirm page (specs/044 §5.5): exactly what will be accepted, and what is not included. */
export function confirm(items: Decision[], p: Pane, c: Ctx): string[] {
  const { w, h } = c;
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  const yes = ticked(p, items);
  const rest = items.filter((d) => !yes.includes(d));
  const rule = '  ' + T.dim('─'.repeat(Math.max(0, w - 4)));
  const L: string[] = ['', '  ' + chalk.bold(T.text(c.pal)(`Accept ${yes.length} recommendation${yes.length === 1 ? '' : 's'}?`)), rule, ''];
  for (const d of yes) {
    L.push('  ' + T.ok('✔') + ' ' + T.dim(d.label) + '  ' + chalk.bold(tierStyle(d.tier)(T.clip(d.title, w - 16))));
    L.push('        ' + T.dim(T.clip(effectLine(d), w - 10)), '');
  }
  L.push(rule, '  ' + T.dim('not included: these need you one at a time'));
  const shown = rest.slice(0, Math.max(1, h - L.length - 7));
  for (const d of shown) L.push('   ' + T.dim(d.label) + '  ' + tierStyle(d.tier)(T.clip(d.title, w - 16)));
  if (rest.length > shown.length) L.push('   ' + T.dim(`+ ${rest.length - shown.length} more`));
  L.push('');
  for (const l of wrap('Each acceptance is signed as you and logged. You can still change a review in /decisions until the build is merged.', w - 4)) L.push('  ' + T.dim(l));
  L.push('');
  L.push('  ' + chalk.inverse(` Accept ${yes.length} `) + T.dim('  enter') + '      ' + T.dim('go back  esc'));
  return pad(L);
}

/** The pane's body: the confirm page, the detail of the open decision, the list, or the empty state. */
export const render = (items: Decision[], p: Pane, c: Ctx): string[] => (!items.length ? empty(p, c) : p.confirm ? confirm(items, p, c) : p.open !== null ? detail(items, p, c) : view(items, p, c));

/** The prompt bar of the detail view, bound to the open decision (specs/044 §6.3); null in the list and everywhere else. */
export const promptLine = (p: Pane, w: number): string | null => {
  if (p.open === null || p.confirm) return null;
  if (p.busy) return T.dim('reading your words… (a few hundred tokens, then it is saved with the decision)');
  const room = Math.max(8, w - 6);
  if (!p.draft) return T.dim('❯ ') + chalk.inverse(' ') + T.dim('or type here to answer in your own words');
  const shown = p.draft.length > room ? '…' + p.draft.slice(-(room - 1)) : p.draft;
  return T.dim('❯ ') + shown + chalk.inverse(' ');
};

export const hint = (items: Decision[], p: Pane): string => {
  if (!items.length) return 'nothing to decide · esc for chat';
  if (p.confirm) return `enter accept ${ticked(p, items).length} · esc back to the list`;
  if (p.open !== null) {
    const d = visible(items, p.filter).find((x) => x.id === p.open);
    if (p.reading) return 'enter confirm the reading · keep typing to change it · tab back to the choices · esc discard';
    return `${d && d.brief.recommendation ? 'tab / enter choose · 1-9 pick' : 'tab / 1-9 choose'} · or type in your own words · ctrl+d diff · ↑ ↓ other decisions · esc back`;
  }
  return `↑ ↓ move · enter open · space tick · a tick recommended${p.ticks.length ? ' · y review ticked' : ''} · f filter${p.filter ? ' · esc clear the filter' : ''}`;
};
