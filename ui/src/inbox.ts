// The decision inbox (right deck, spec 005b §3.4 and 005c §2.1.6): list mode to triage, focus mode to review one card.
// A pure reducer and a pure renderer, so the whole thing is tested without a terminal. Answers are not sent from here:
// the reducer returns what the operator asked for and the app sends it through `answerCard` (bound to the payload hash).
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';
import type { Ev } from './input.ts';
import type { Approval, State } from './fold.ts';
import type { Decision } from './run.ts';
import * as T from './theme.ts';

export type InboxUi = {
  mode: 'list' | 'focus';
  idx: number;
  /** Focus mode showing the full payload instead of the summary. */
  view: boolean;
  scroll: number;
  /** List mode: `a` was pressed once and waits for a second press before approving every Amber card. */
  confirm: boolean;
};

export const inboxInit = (): InboxUi => ({ mode: 'list', idx: 0, view: false, scroll: 0, confirm: false });

/** Pending cards: Red first, then oldest first, so what needs the most care and has waited longest is on top. */
export const cardsOf = (s: State): Approval[] =>
  Object.values(s.approvals).sort((a, b) => Number(b.tier === 'red') - Number(a.tier === 'red') || a.at - b.at);

export type Act =
  | { t: 'answer'; id: string; d: Decision }
  | { t: 'amber-all' }
  | { t: 'leave' };

/** One key in the inbox: the new view state and, when the key asked for something, what it asked for. */
export function inboxKey(ui: InboxUi, e: Ev, cards: Approval[]): { ui: InboxUi; act?: Act } {
  const n = cards.length;
  const idx = n ? Math.max(0, Math.min(ui.idx, n - 1)) : 0;
  const cur = cards[idx];
  const keep = (over: Partial<InboxUi> = {}) => ({ ui: { ...ui, idx, confirm: false, ...over } });
  if (e.t === 'text') {
    const k = e.s.toLowerCase();
    if (!cur) return keep();
    if (ui.mode === 'list' && k === 'a') {
      const amber = cards.some((c) => c.tier !== 'red');
      if (!amber) return keep();
      return ui.confirm ? { ui: { ...ui, idx, confirm: false }, act: { t: 'amber-all' } } : { ui: { ...ui, idx, confirm: true } };
    }
    if (ui.mode === 'focus' && k === 'a') return { ...keep(), act: { t: 'answer', id: cur.id, d: 'allow_similar' } };
    if (k === 'y') return { ...keep(), act: { t: 'answer', id: cur.id, d: 'allow' } };
    if (k === 'n') return { ...keep(), act: { t: 'answer', id: cur.id, d: 'deny' } };
    if (k === 'd') return keep({ mode: 'focus', view: !(ui.mode === 'focus' && ui.view), scroll: 0 });
    if (k === 'j') return keep({ idx: Math.max(0, Math.min(n - 1, idx + 1)), scroll: 0 });
    if (k === 'k') return keep({ idx: Math.max(0, idx - 1), scroll: 0 });
    return keep();
  }
  if (e.t !== 'key') return keep();
  switch (e.k) {
    case 'esc': return ui.view ? keep({ view: false, scroll: 0 }) : { ui: { ...ui, idx, confirm: false }, act: { t: 'leave' } };
    case 'tab': return keep({ mode: ui.mode === 'list' ? 'focus' : 'list', view: false, scroll: 0 });
    case 'enter': return ui.mode === 'list' && cur ? keep({ mode: 'focus' }) : keep();
    case 'up': return ui.view ? keep({ scroll: Math.max(0, ui.scroll - 1) }) : keep({ idx: Math.max(0, idx - 1), scroll: 0 });
    case 'down': return ui.view ? keep({ scroll: ui.scroll + 1 }) : keep({ idx: Math.max(0, Math.min(n - 1, idx + 1)), scroll: 0 });
    case 'left': return ui.mode === 'focus' ? keep({ idx: Math.max(0, idx - 1), view: false, scroll: 0 }) : keep();
    case 'right': return ui.mode === 'focus' ? keep({ idx: Math.max(0, Math.min(n - 1, idx + 1)), view: false, scroll: 0 }) : keep();
    case 'pgup': return keep({ scroll: Math.max(0, ui.scroll - 8) });
    case 'pgdn': return keep({ scroll: ui.scroll + 8 });
    default: return keep();
  }
}

const tierTag = (a: Approval) => (a.tier === 'red' ? T.crimson(chalk.bold('RED')) : T.warn('AMB')) + ' ';
const mmss = (secs: number) => `${String(Math.floor(secs / 60)).padStart(2, '0')}:${String(secs % 60).padStart(2, '0')}`;
const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true, trim: false }).split('\n');

type View = { info: T.Info; pal: T.Pal; now: number };

/** What the prompt row says while the inbox has the keyboard. */
export function inboxHint(ui: InboxUi, cards: Approval[], v: View): string {
  if (!cards.length) return T.dim('nothing needs you · esc or shift+← back to chat');
  const i = Math.min(ui.idx, cards.length - 1);
  const c = cards[i]!;
  if (ui.mode === 'list') return T.dim(ui.confirm ? `approve ${cards.filter((x) => x.tier !== 'red').length} amber card(s) once each? press a again · any other key cancels` : `decision ${i + 1}/${cards.length}: ↑↓ select · enter inspect · y approve · n reject · a approve all amber · esc chat`);
  return T.dim(`decision ${i + 1}/${cards.length}: y approve once${c.tier === 'red' ? '' : ' · a allow similar'} · n reject · d ${ui.view ? 'summary' : 'full payload'} · tab list · ← → card · esc ${ui.view ? 'summary' : 'chat'}`);
}

/** The deck body: exactly `h` lines. */
export function inboxBody(ui: InboxUi, cards: Approval[], v: View, w: number, h: number): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h).map((l) => T.clip(l, w));
  if (!cards.length) return pad(['', T.dim('  DECISION INBOX · NOTHING PENDING'), '', T.dim('  When an agent needs you, the card appears here and in the chat.')]);
  const idx = Math.max(0, Math.min(ui.idx, cards.length - 1));
  if (ui.mode === 'list') {
    const rows = cards.map((a, i) => {
      const left = Math.max(0, Math.round((a.at + a.timeoutS * 1000 - v.now) / 1000));
      const on = i === idx;
      const desc = (a.summary || a.tool || 'approval').split('\n')[0]!;
      const line = `${on ? T.accent('►') : ' '} ${String(i + 1).padStart(2)}  ${tierTag(a)} ${T.roleColor(a.role, v.info)(T.roleName(a.role, v.info).padEnd(11).slice(0, 11))} ${T.dim((a.task || '—').padEnd(6).slice(0, 6))} ${on ? chalk.bold(desc) : desc}`;
      return T.spread(line, T.dim(mmss(left)), w - 1);
    });
    const head = `  ${T.dim(' #')}  ${T.dim('TIER')} ${T.dim('ROLE'.padEnd(11))} ${T.dim('TASK'.padEnd(6))} ${T.dim('DESCRIPTION')}`;
    return pad(['', chalk.bold(T.text(v.pal)(`  DECISION INBOX · ${cards.length} PENDING`)), '', head, ...rows]);
  }
  const a = cards[idx]!;
  const left = Math.max(0, Math.round((a.at + a.timeoutS * 1000 - v.now) / 1000));
  const title = (a.tier === 'red' ? T.crimson : T.warn)(chalk.bold(`APPROVAL REQUIRED [${idx + 1} of ${cards.length}] · ${a.tier === 'red' ? 'red-tier gate · never auto-approved' : 'amber'}`)) + T.dim(` · ${mmss(left)} until it is denied`);
  const field = (k: string, val: string) => wrap(`  ${T.dim(k.padEnd(10) + ':')} ${val}`, w - 2).map((l, i) => (i === 0 ? l : '    ' + l));
  const head = ['', title, ...field('Requester', `${T.roleColor(a.role, v.info)(T.roleName(a.role, v.info))} (${a.role})`), ...field('Task', a.task || '—'), ...field('Tool', a.tool || '—'), ...field('Reason', a.summary || '—')];
  let body: string[];
  if (ui.view) {
    const all = JSON.stringify(a.payload ?? null, null, 2).split('\n').flatMap((l) => wrap('    ' + T.sanitize(l), w - 2));
    const room = Math.max(1, h - head.length - 2);
    const at = Math.min(ui.scroll, Math.max(0, all.length - room));
    body = ['', T.dim(`  full payload · lines ${at + 1}-${Math.min(all.length, at + room)} of ${all.length} · ↑↓ scroll`), ...all.slice(at, at + room)];
  } else {
    const prev = a.preview ? ['', T.dim('  Preview:'), ...a.preview.split('\n').slice(0, Math.max(3, h - head.length - 6)).flatMap((l) => wrap('    ' + l, w - 2))] : [];
    body = [...prev, '', T.dim(`  answers are bound to payload ${a.blake3.slice(0, 12) || '—'}; a changed payload is a new approval`)];
  }
  return pad([...head, ...body]);
}
