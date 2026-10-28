// The decisions pane's list (specs/044 §5): every decision the operator owes, one entry each, tier-coloured titles, a
// filter by kind, and tick boxes only where the daemon says a default is safe. Pure: the list from `decision.list`
// and a key press go in, lines and an action come out. The daemon decides what is tickable; nothing here does.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import * as T from './theme.ts';

type Style = (s: string) => string;
export type Choice = { id: string; label: string; effect: string; reversible: boolean };
export type Brief = { summary: string; changes: string[]; recommendation: { choice_id: string; why: string } | null; state: string };
export type Decision = {
  id: string; source: string; source_id: string; label: string; kind: string; tier: 'red' | 'amber' | 'open'; title: string;
  requester: string; task: string; raised_at: string | null; payload_blake3: string; brief: Brief; choices: Choice[];
  tickable: boolean; tickable_reason: string;
};
/** Where the operator is in the list. A tick names the payload it was given for, so a changed payload loses it. */
export type Pane = { sel: number; filter: string | null; ticks: string[] };
export const newPane = (): Pane => ({ sel: 0, filter: null, ticks: [] });

export type Action = { t: 'none' } | { t: 'chat' } | { t: 'open'; d: Decision };

const KIND_ORDER = ['action', 'spec', 'release', 'build', 'council', 'budget', 'question'];
const KIND_LABEL: Record<string, string> = { action: 'actions', spec: 'specs', release: 'releases', build: 'builds', council: 'councils', budget: 'budgets', question: 'questions' };
const TIER_WORD = { red: 'RED', amber: 'AMBER', open: 'OPEN' } as const;
const KIND_WORD: Record<string, string> = { action: 'action', spec: 'spec', release: 'release', build: 'build', council: 'council', budget: 'budget', question: 'question' };

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
export const isTicked = (p: Pane, d: Decision): boolean => d.tickable && p.ticks.includes(tickKey(d));
export const ticked = (p: Pane, items: Decision[]): Decision[] => items.filter((d) => isTicked(p, d));

/** After the list changes: keep the selection in range, drop a filter nothing matches, and drop ticks whose decision
 * is gone, is no longer tickable, or whose payload changed (specs/044 §5.4). */
export function reconcile(p: Pane, items: Decision[]): Pane {
  const filter = p.filter && items.some((d) => d.kind === p.filter) ? p.filter : null;
  const live = new Set(items.filter((d) => d.tickable).map(tickKey));
  const rows = visible(items, filter);
  return { filter, ticks: p.ticks.filter((t) => live.has(t)), sel: Math.max(0, Math.min(p.sel, rows.length - 1)) };
}

export function key(p: Pane, items: Decision[], e: Ev): { pane: Pane; action: Action } {
  const none = (q: Pane = p) => ({ pane: q, action: { t: 'none' } as Action });
  if (e.t !== 'key' && e.t !== 'text') return none();
  const rows = visible(items, p.filter);
  const sel = Math.max(0, Math.min(p.sel, rows.length - 1));
  const d = rows[sel];
  const k = e.t === 'key' ? e.k : e.s;
  if (k === 'esc') return p.filter ? none({ ...p, filter: null, sel: 0 }) : { pane: p, action: { t: 'chat' } };
  if (e.t === 'key' && k === 'left' && !e.alt && !e.ctrl) return { pane: p, action: { t: 'chat' } };
  if (k === 'up' || k === 'down') return none({ ...p, sel: Math.max(0, Math.min(rows.length - 1, sel + (k === 'up' ? -1 : 1))) });
  if (k === 'enter') return d ? { pane: { ...p, sel }, action: { t: 'open', d } } : none();
  if (k === 'f') {
    const kinds = chips(items).map((c) => c.kind);
    const at = p.filter ? kinds.indexOf(p.filter) : -1;
    const next = at + 1 >= kinds.length ? null : kinds[at + 1]!;
    return none({ ...p, filter: next, sel: 0 });
  }
  if (k === ' ' && d) {
    if (!d.tickable) return none();
    const t = tickKey(d);
    return none({ ...p, sel, ticks: p.ticks.includes(t) ? p.ticks.filter((x) => x !== t) : [...p.ticks, t] });
  }
  if (k === 'a') {
    // Every tickable decision in the filter, never more: a filter hides entries, it does not widen the batch.
    const ts = rows.filter((x) => x.tickable);
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

export type Ctx = { pal: T.Pal; now: number; w: number; h: number; /** When a decision was raised, in ms, if the console knows. */ at: (d: Decision) => number | null };

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
  const tickHint = tickable ? T.accent('a') + '  ' + T.text(c.pal)(p.filter ? 'tick every recommended item here' : 'tick every recommended item') : '';
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
  const head = ['', '  ' + T.spread(count, tickHint, w - 4), '  ' + T.spread(chipRow, T.dim('f change'), w - 4), ''];
  const per = compact ? 4 : 5;
  const cap = Math.max(1, Math.floor((h - head.length + (compact ? 0 : 1)) / per));
  const start = Math.min(Math.max(0, sel - cap + 1), Math.max(0, rows.length - cap));
  const body: string[] = [];
  rows.slice(start, start + cap).forEach((d, i) => {
    const on = start + i === sel;
    const style = tierStyle(d.tier);
    const box = d.tickable ? (isTicked(p, d) ? T.ok(chalk.bold('[x]')) : T.dim('[ ]')) : '   ';
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


export const hint = (n: number, p: Pane): string =>
  n === 0 ? 'nothing to decide · esc for chat' : `↑ ↓ move · enter open · space tick · a tick recommended · f filter${p.filter ? ' · esc clear the filter' : ''} · red: open to decide`;
