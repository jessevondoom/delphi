// The Decision Inbox (the right deck): every pending approval, reviewed one at a time (focus) or triaged as a list.
// Pure functions: the view is lines, a key press is (state, key) -> (state, what to do).
import chalk from 'chalk';
import type { Approval, State } from './fold.ts';
import type { Ev } from './input.ts';
import * as T from './theme.ts';
import { highlightLine } from './highlight.ts';
import * as D from './decisions-pane.ts';

/**
 * `items` is the daemon's `decision.list` (specs/044) and `loaded` says it has arrived, so an empty list means
 * "nothing is owed" and not "the daemon did not answer". Until it has, the inbox is the cards alone, as it was.
 */
export type Inbox = { mode: 'list' | 'focus'; sel: number; opt: number; payload: boolean; confirmAll: boolean; /** Approval ids whose card was shown in focus mode: only these join a resolve-all. */ seen: string[]; since: number; items: D.Decision[]; loaded: boolean; pane: D.Pane };
export const newInbox = (now: number, items: D.Decision[] = [], pane: D.Pane = D.newPane(), loaded = items.length > 0): Inbox => ({ mode: 'list', sel: 0, opt: 0, payload: false, confirmAll: false, seen: [], since: now, items, loaded, pane: D.reconcile(pane, items) });
/** The pane (list, detail, empty state) owns the deck once the daemon's list has arrived. */
const paned = (inb: Inbox): boolean => inb.loaded || inb.items.length > 0;

export type Decision = 'allow' | 'allow_similar' | 'allow_session' | 'deny';
export type Action =
  | { t: 'none' }
  | { t: 'chat' }
  | { t: 'resolve'; card: Approval; decision: Decision; /** Set when a reading of typed words was confirmed: the decision it belongs to, and its notes. */ d?: D.Decision; conditions?: string[] }
  | { t: 'review'; d: D.Decision; resolution: 'ratified' | 'rejected'; conditions?: string[] }
  /** A raised decision answered: the console records it with `decision.resolve`. */
  | { t: 'record'; d: D.Decision; choice_id: string; conditions?: string[] }
  | { t: 'thread'; d: D.Decision } | { t: 'reply'; d: D.Decision; text: string } | { t: 'retry'; d: D.Decision }
  | { t: 'accept'; ds: D.Decision[] }
  | { t: 'resolve-all'; cards: Approval[] };

/** The guard from spec 005: keys that answer a card are ignored for this long after it appears or the deck opens. */
export const GUARD_MS = 300;

/** Red first, then oldest first. */
export const cards = (s: State): Approval[] => Object.values(s.approvals).sort((a, b) => (a.tier === 'red' ? 0 : 1) - (b.tier === 'red' ? 0 : 1) || a.at - b.at);

export const options = (a: Approval): { d: Decision; label: string }[] =>
  a.tier === 'red' || a.noSimilar
    ? [{ d: 'allow', label: 'Approve once' }, { d: 'deny', label: 'Reject' }]
    : [{ d: 'allow', label: 'Approve once' }, { d: 'allow_similar', label: 'Allow similar actions this task' }, ...(a.sessionAllowed ? [{ d: 'allow_session' as const, label: 'Allow similar commands this session' }] : []), { d: 'deny', label: 'Reject' }];

const remaining = (a: Approval, now: number) => {
  const s = Math.max(0, Math.round((a.at + a.timeoutS * 1000 - now) / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
};

/** A key press; a card shown in focus mode counts as rendered, and only rendered cards join a resolve-all. */
export function key(inb: Inbox, list: Approval[], e: Ev, now: number): { inbox: Inbox; action: Action } {
  const r = press(inb, list, e, now);
  const next = r.inbox;
  if (paned(next) || next.mode !== 'focus') return r;
  const shown = list[Math.min(next.sel, Math.max(0, list.length - 1))];
  return shown && !next.seen.includes(shown.id) ? { ...r, inbox: { ...next, seen: [...next.seen, shown.id] } } : r;
}

function press(inb: Inbox, list: Approval[], e: Ev, now: number): { inbox: Inbox; action: Action } {
  const none = (i: Inbox = inb) => ({ inbox: i, action: { t: 'none' } as Action });
  if (e.t !== 'key' && e.t !== 'text' && !(paned(inb) && (e.t === 'ctrl' || e.t === 'paste'))) return none();
  const sel = Math.min(inb.sel, Math.max(0, list.length - 1));
  const card = list[sel];
  const guarded = now - inb.since < GUARD_MS || (card ? now - card.at < GUARD_MS : false);
  const k = e.t === 'key' ? e.k : e.t === 'text' ? e.s : '';
  const answer = (decision: Decision) => (card && !guarded ? { inbox: { ...inb, sel, confirmAll: false, payload: false, opt: 0 }, action: { t: 'resolve', card, decision } as Action } : none());
  // The pane of specs/044: the keys belong to it. An answer is the operator choosing in the detail view (never from the
  // list), and the console turns it into the call for the decision's source: a card's own resolution, or a review.
  if (paned(inb)) {
    const r = D.key(inb.pane, inb.items, e, now);
    if (r.action.t === 'chat') return { inbox: { ...inb, pane: r.pane }, action: { t: 'chat' } };
    if (r.action.t === 'accept-all') return { inbox: { ...inb, pane: r.pane }, action: { t: 'accept', ds: r.action.ds } };
    if (r.action.t === 'thread' || r.action.t === 'reply' || r.action.t === 'retry') return { inbox: { ...inb, pane: r.pane }, action: r.action };
    if (r.action.t === 'answer') {
      const { d, choice } = r.action;
      if (d.source === 'card') {
        const c = list.find((x) => x.id === d.source_id);
        if (c && (choice.id === 'allow' || choice.id === 'allow_similar' || choice.id === 'allow_session' || choice.id === 'deny') && now - c.at >= GUARD_MS) return { inbox: { ...inb, pane: r.pane }, action: { t: 'resolve', card: c, decision: choice.id, ...(r.action.conditions ? { conditions: r.action.conditions, d } : {}) } };
      } else if (d.source === 'raised' || d.source === 'digest') {
        return { inbox: { ...inb, pane: r.pane }, action: { t: 'record', d, choice_id: choice.id, ...(r.action.conditions ? { conditions: r.action.conditions } : {}) } };
      } else if (d.source === 'provisional' && (choice.id === 'ratified' || choice.id === 'rejected')) {
        return { inbox: { ...inb, pane: r.pane }, action: { t: 'review', d, resolution: choice.id, ...(r.action.conditions ? { conditions: r.action.conditions } : {}) } };
      }
      return none({ ...inb, pane: D.unsent(r.pane) });
    }
    return none({ ...inb, pane: r.pane });
  }
  if (k === 'esc') return { inbox: { ...inb, confirmAll: false }, action: { t: 'chat' } };
  if (k === 'tab') return none({ ...inb, mode: inb.mode === 'list' ? 'focus' : 'list', confirmAll: false });
  if (k === 'up' || k === 'down') {
    const d = k === 'up' ? -1 : 1;
    if (inb.mode === 'list') return none({ ...inb, sel: Math.max(0, Math.min(list.length - 1, sel + d)), confirmAll: false });
    const n = card ? options(card).length : 1;
    return none({ ...inb, opt: Math.max(0, Math.min(n - 1, inb.opt + d)) });
  }
  // `←` in the list goes back to the chat; in focus mode it and `→` walk through the cards.
  if (e.t === 'key' && k === 'left' && !e.alt && !e.ctrl && (inb.mode === 'list' || !list.length)) return { inbox: { ...inb, confirmAll: false }, action: { t: 'chat' } };
  if (inb.mode === 'focus' && (k === 'left' || k === 'right')) return none({ ...inb, sel: Math.max(0, Math.min(list.length - 1, sel + (k === 'left' ? -1 : 1))), opt: 0, payload: false });
  if (k === 'enter') {
    if (inb.mode === 'list') return card ? none({ ...inb, mode: 'focus', sel, opt: 0 }) : none();
    const o = card ? options(card)[Math.min(inb.opt, options(card).length - 1)] : undefined;
    return o ? answer(o.d) : none();
  }
  if (inb.mode === 'focus' && card && /^[1-9]$/.test(k)) {
    const o = options(card)[Number(k) - 1];
    return o ? answer(o.d) : none();
  }
  if (k === 'd' && inb.mode === 'focus') return none({ ...inb, payload: !inb.payload });
  if (k === 'y') return answer('allow');
  if (k === 'n') return answer('deny');
  if (k === 'a' && inb.mode === 'list') {
    const amber = list.filter((c) => c.tier !== 'red' && now - c.at >= GUARD_MS && inb.seen.includes(c.id));
    if (!amber.length || guarded) return none();
    if (!inb.confirmAll) return none({ ...inb, confirmAll: true });
    return { inbox: { ...inb, confirmAll: false }, action: { t: 'resolve-all', cards: amber } };
  }
  return none({ ...inb, confirmAll: false });
}

const tier = (a: Approval) => (a.tier === 'red' ? T.crimson('RED') : T.warn('AMB'));

/** The deck's body: exactly `h` lines. */
export function view(s: State, inb: Inbox, info: T.Info, pal: T.Pal, now: number, w: number, h: number): string[] {
  const list = cards(s);
  if (paned(inb)) {
    const at = (d: D.Decision): number | null => (d.source === 'card' ? s.approvals[d.source_id]?.at ?? null : d.raised_at ? Date.parse(d.raised_at) || null : null);
    const payload = (d: D.Decision): string[] | null => {
      const a = d.source === 'card' ? s.approvals[d.source_id] : undefined;
      return a ? a.preview.split('\n').slice(0, 200) : null;
    };
    return D.render(inb.items, inb.pane, { pal, now, w, h, at, payload });
  }
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  if (!list.length) return pad(['', '  ' + chalk.bold(T.text(pal)('Decision inbox')), '', '  ' + T.dim('nothing needs you'), '', '  ' + T.dim('Approvals from every agent arrive here; the header shows an alert when one is waiting.')]);
  const sel = Math.min(inb.sel, list.length - 1);
  const card = list[sel]!;
  if (inb.mode === 'list') {
    const rows = list.map((c, i) => {
      const on = i === sel;
      const left = (on ? T.accent('❯ ') : '  ') + `${i + 1}. ` + tier(c) + '  ' + (on ? chalk.bold(T.text(pal)(T.clip(c.summary || c.tool, Math.max(10, w - 40)))) : T.text(pal)(T.clip(c.summary || c.tool, Math.max(10, w - 40))));
      const right = T.badge(c.role, 'idle', info, pal) + ' ' + T.dim(T.roleName(c.role, info)) + '  ' + T.dim(remaining(c, now));
      return T.spread(left, right, w - 2);
    });
    const amber = list.filter((c) => c.tier !== 'red' && inb.seen.includes(c.id)).length;
    const unread = list.filter((c) => c.tier !== 'red' && !inb.seen.includes(c.id)).length;
    return pad([
      '', '  ' + chalk.bold(T.text(pal)('Pending decisions')) + T.dim(` (${list.length})`), '',
      ...rows.map((r) => '  ' + r), '',
      T.dim('  ' + '┈'.repeat(Math.max(0, w - 4))),
      '  ' + T.dim(`Selected: #${sel + 1} · ${card.tool}`),
      '  ' + T.dim(`Reason: ${card.tier === 'red' ? 'Red-tier gate: ' : ''}${card.summary}`),
      ...(inb.confirmAll ? ['', '  ' + T.warn(`press a again to approve ${amber} amber action(s), each bound to what it shows`)] : []),
      ...(unread ? ['', '  ' + T.warn(`${unread} amber action(s) not opened yet: open one (enter) to read it before \`a\` can approve it`)] : []),
    ]);
  }
  const opts = options(card);
  const head = (card.tier === 'red' ? T.crimson(chalk.bold('Approval required')) : T.warn(chalk.bold('Approval required'))) + T.dim(` · ${sel + 1} of ${list.length} pending · ${card.tier === 'red' ? 'Red' : 'Amber'}-tier gate · ${remaining(card, now)} left`);
  const preview = (inb.payload ? JSON.stringify(card.payload, null, 2) : card.preview).split('\n').slice(0, 200);
  const lines = [
    '', '  ' + head, '',
    '  ' + chalk.bold(T.link(T.clip(card.summary || card.tool, w - 4))),
    '  ' + T.dim('Requester: ') + T.badge(card.role, 'idle', info, pal) + ' ' + T.roleColor(card.role, info)(T.roleName(card.role, info)) + T.dim(` (${card.role}) · Task: ${card.task || '?'} · Tool: ${card.tool}`),
    '',
    ...opts.map((o, i) => (i === inb.opt ? T.accent('  ❯ ') + chalk.bold(T.text(pal)(`${i + 1}. ${o.label}`)) : '    ' + T.dim(`${i + 1}. ${o.label}`))),
    '',
    ...preview.map((l) => '  ' + (inb.payload ? T.dim(l) : highlightLine(T.clip(l, w - 4), 'diff'))),
  ];
  return pad(lines);
}

/** The detail view's own prompt bar (specs/044 §6.3), or null where the deck shows its key hint. */
export const promptLine = (inb: Inbox, w: number): string | null => (paned(inb) ? D.promptLine(inb.pane, w) : null);

export const hint = (inb: Inbox, n: number): string =>
  paned(inb) ? D.hint(inb.items, inb.pane)
    : n === 0 ? 'nothing to decide · esc for chat'
    : inb.mode === 'list' ? 'enter open card · y approve · n reject · a approve all amber · tab focus mode · esc chat'
      : 'enter select · 1-3 choose · ←→ other cards · d payload · tab list mode · esc chat';
