// The council view, "Weave" (`005e` §9.2): the question, a three-step track, one tinted card per seat and a verdict card.
// The blind round is sealed: no stance text is ever read in it, only who has answered. Pure: model in, rows out.
import chalk from 'chalk';
import stringWidth from 'string-width';
import wrapAnsi from 'wrap-ansi';
import { tintCard } from './blocks.ts';
import { faceOf } from './motion.ts';
import type { OModel, OState } from './observe-screen.ts';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';

type Council = OState['councils'][number];
export type Phase = 'blind' | 'debate' | 'verdict';

/** The daemon's `Council.phase` in the view's three steps: `round 1` is blind, `round 2` is the debate, anything later is the verdict. */
export const phaseOf = (c: Council): Phase => (c.phase === 'round 1' ? 'blind' : c.phase === 'round 2' ? 'debate' : 'verdict');

const GAP = 2, MARGIN = 2, CARD = 24, VERDICT = 76;
const BAR = ['▰', '▱'] as const;

const clean = (s: string, n: number) => { const t = sanitize(s).replace(/\s+/g, ' ').trim(); return [...t].length <= n ? t : [...t].slice(0, Math.max(0, n - 1)).join('') + '…'; };
const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(4, w), { hard: true, trim: true }).split('\n');
const mmss = (ms: number) => { const s = Math.max(0, Math.floor(ms / 1000)); return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`; };
const meter = (done: number, of: number) => { const n = of > 0 ? Math.round((10 * done) / of) : 0; return BAR[0].repeat(n) + BAR[1].repeat(10 - n); };
const dim = (s: string) => T.dim(s);

/** A seat's position: the word after `OPTION:` in its stance, else the first line. Its reason is the rest. */
export function positionOf(text: string): { option: string; reason: string } {
  const lines = sanitize(text).split('\n').map((l) => l.trim()).filter(Boolean);
  const first = lines[0] ?? '';
  const m = /^option\s*:\s*(.+)$/i.exec(first);
  return { option: (m ? m[1]! : first).trim(), reason: lines.slice(1).join(' ') };
}

const nodesOf = (m: OModel, c: Council) => Object.values(m.st.nodes).filter((n) => n.task_id === c.task_id);
const seatNode = (m: OModel, c: Council, role: string) => nodesOf(m, c).find((n) => n.role === role);

/** Who is still answering (blind) or speaking (debate), and who has finished: the strip and the cards read the same facts. */
export function seatsNow(m: OModel, c: Council): { thinking: Set<string>; speaking: string | null } {
  const phase = phaseOf(c);
  const thinking = new Set<string>();
  let speaking: string | null = null;
  if (phase === 'blind') for (const s of c.seats) if (!c.stances.find((x) => x.role === s.role)?.answered && seatNode(m, c, s.role)?.state !== 'done') thinking.add(s.role);
  if (phase === 'debate') {
    const live = nodesOf(m, c).find((n) => n.state === 'running' && c.seats.some((s) => s.role === n.role) && !c.critiques.some((k) => k.role === n.role));
    if (live) { speaking = live.role; thinking.add(live.role); }
  }
  return { thinking, speaking };
}

/** The council the view shows, if any. */
export const current = (m: OModel): Council | undefined => m.st.councils[Math.min(m.pick, m.st.councils.length - 1)];

/** Each seat's final position (its critique's recommendation when it has one, else its stance) and the majority. */
export function positions(c: Council) {
  const by = new Map<string, string>();
  for (const s of c.seats) {
    const crit = c.critiques.find((k) => k.role === s.role)?.recommendation?.trim();
    const st = c.stances.find((x) => x.role === s.role);
    const opt = crit || (st?.answered ? positionOf(st.text).option : '');
    if (opt) by.set(s.role, opt);
  }
  const counts = new Map<string, number>();
  for (const o of by.values()) counts.set(o.toLowerCase(), (counts.get(o.toLowerCase()) ?? 0) + 1);
  const ranked = [...counts.entries()].sort((a, b) => b[1] - a[1]);
  const top = ranked[0];
  const display = (key: string) => [...by.values()].find((o) => o.toLowerCase() === key) ?? key;
  const majority = top && (ranked.length === 1 || top[1] > ranked[1]![1]) ? { option: display(top[0]), n: top[1] } : null;
  return { by, majority, tally: ranked.map(([k, n]) => `${display(k)} ${n}`) };
}

function elapsed(m: OModel, c: Council, phase: Phase): number {
  const ns = nodesOf(m, c).filter((n) => n.started_ms > 0);
  if (!ns.length) return 0;
  const start = Math.min(...ns.map((n) => n.started_ms));
  const ends = ns.map((n) => n.ended_ms);
  const end = phase === 'verdict' && ends.every((e) => e != null) ? Math.max(...(ends as number[])) : (m.now ?? start);
  return Math.max(0, end - start);
}

const pulse = (m: OModel) => (m.still ?? true) || Math.floor((m.now ?? 0) / 600) % 2 === 0;

function track(m: OModel, c: Council, w: number, phase: Phase): string {
  const idx = { blind: 0, debate: 1, verdict: 2 }[phase];
  const waiting = phase === 'verdict' && Boolean(c.escalation);
  const mark = (i: number) => (i < idx || (i === idx && phase === 'verdict' && !waiting) ? T.ok('✔') : i === idx ? T.accent(pulse(m) ? '●' : '○') : dim('○'));
  const name = (i: number, n: string) => (i === idx ? chalk.bold(T.text(m.pal)(n)) : i < idx ? T.text(m.pal)(n) : dim(n));
  const done = phase === 'verdict' && !waiting;
  const right = waiting ? T.warn('needs you') : done ? dim(`done ${mmss(elapsed(m, c, phase))}`) : dim(mmss(elapsed(m, c, phase)));
  if (w < 56) return T.clip(' ' + dim('round ') + mark(idx) + ' ' + name(idx, ['blind', 'debate', 'verdict'][idx]!) + dim(`  ${idx + 1} of 3`) + '  ' + right, w);
  const conn = Math.max(2, Math.min(9, Math.floor((w - 9 - 28 - 7) / 2)));
  const link = (i: number) => (i < idx ? T.ok('━'.repeat(conn)) : dim('╌'.repeat(conn)));
  const left = ' ' + dim('round') + '   ' + [0, 1, 2].map((i) => mark(i) + ' ' + name(i, ['blind', 'debate', 'verdict'][i]!)).reduce((a, b, i) => a + ' ' + link(i - 1) + ' ' + b);
  return T.spread(left, right + ' ', w);
}

/** The shutter: two rows of block characters standing for text that is sealed. */
function shutter(m: OModel, inner: number, answered: boolean): string[] {
  const a = Math.max(2, inner - 1), b = Math.max(2, Math.round(a * 0.7));
  const row = (n: number) => (answered ? dim('▓'.repeat(n)) : T.accent([...Array(n)].map((_, i) => ((i + (pulse(m) ? 0 : 1)) % 2 ? '▒' : '░')).join('')));
  return [row(a), row(b)];
}

type Stage = { text: string[]; tone: number };

function seatCard(m: OModel, c: Council, seat: Council['seats'][number], phase: Phase, inner: number, now: { speaking: string | null; thinking: Set<string> }, pos: ReturnType<typeof positions>): Stage {
  const role = seat.role;
  const st = c.stances.find((x) => x.role === role);
  const node = seatNode(m, c, role);
  const hot = now.thinking.has(role);
  const face = phase === 'verdict' ? { frame: 'success', state: 'idle' as const } : faceOf(hot ? { state: 'running', doing: '', thinkingAt: m.now ?? 0 } : undefined, role, m.now ?? 0, m.still ?? true);
  const head = T.badge(role, face.state, m.info, m.pal, face.frame) + ' ' + T.roleColor(role, m.info).bold(clean(T.roleName(role, m.info), inner - 6));
  const rows: string[] = [head];
  if (phase === 'blind') {
    const answered = Boolean(st?.answered);
    const took = node ? (node.ended_ms ?? m.now ?? node.started_ms) - node.started_ms : 0;
    rows.push(dim('sealed'), answered ? T.ok(`✔ answered ${mmss(took)}`) : T.accent(`${pulse(m) ? '●' : '○'} thinking ${mmss(took)}`), ...shutter(m, inner, answered));
    return { text: rows, tone: answered ? 1 : 1.7 };
  }
  const mine = st?.answered ? positionOf(st.text) : { option: '', reason: '' };
  const option = pos.by.get(role) ?? mine.option;
  rows.push(chalk.bold(T.text(m.pal)(clean(option || 'no position', inner))));
  if (phase === 'debate') {
    const speaking = now.speaking === role;
    const replied = c.critiques.some((k) => k.role === role);
    const other = c.seats.find((s) => s.role !== role && (pos.by.get(s.role) ?? '').toLowerCase() !== option.toLowerCase()) ?? c.seats.find((s) => s.role !== role);
    rows.push(speaking ? T.accent(clean(`${pulse(m) ? '●' : '○'} answering ${other ? T.roleName(other.role, m.info) : ''}`, inner)) : replied ? T.ok('✔ replied') : dim('waiting'));
    if (speaking) {
      const lt = node ? m.live[node.id] : '';
      const tail = wrap(sanitize(lt ?? '').replace(/\s+/g, ' ').trim(), inner).filter(Boolean).slice(-2);
      const lines = tail.length ? tail : [''];
      lines[lines.length - 1] = lines[lines.length - 1]! + T.accent('▊');
      rows.push(...lines.map((l) => chalk.italic(dim(l))));
    } else rows.push(...wrap(mine.reason, inner).slice(0, 2).map((l) => dim(l)));
    return { text: rows, tone: speaking ? 1.7 : 1 };
  }
  const agreed = pos.majority ? option.toLowerCase() === pos.majority.option.toLowerCase() : false;
  rows.push(c.escalation ? dim('holds its position') : agreed ? T.ok('✔ agreed') : T.warn('✔ dissent noted'));
  rows.push(...wrap(mine.reason, inner).slice(0, 2).map((l) => dim(l)));
  return { text: rows, tone: 1 };
}

function verdictCard(m: OModel, c: Council, phase: Phase, pos: ReturnType<typeof positions>, speaking: string | null): string[] {
  const n = c.seats.length, answered = c.seats.filter((s) => c.stances.find((x) => x.role === s.role)?.answered).length;
  const label = (s: string) => dim(s.padEnd(11));
  const b = (s: string) => chalk.bold(T.text(m.pal)(s));
  if (phase === 'blind') return [label('verdict') + dim(`sealed until all ${n === 3 ? 'three' : n} answer`), dim(`${answered} of ${n} answered`) + '  ' + T.accent(meter(answered, n))];
  if (phase === 'debate') return [label('verdict') + dim('none yet · round 2 of 3'), label('positions') + T.text(m.pal)(pos.tally.join(' · ') || 'none') + (speaking ? '   ' + dim(`${T.roleName(speaking, m.info)} is replying`) : '')];
  if (c.escalation) return [label('verdict') + T.warn('needs you'), label('options') + T.text(m.pal)(clean(c.options.map(([o, who]) => `${o} (${who.join(', ')})`).join(' · ') || c.escalation, 90))];
  const maj = pos.majority;
  const minority = c.seats.filter((s) => maj && (pos.by.get(s.role) ?? '').toLowerCase() !== maj.option.toLowerCase()).map((s) => T.roleName(s.role, m.info));
  return [
    label('verdict') + b(clean(maj?.option ?? 'no majority', 30)) + '   ' + dim(`${maj?.n ?? 0} of ${n} · one round`),
    label('agreement') + T.ok(meter(maj?.n ?? 0, n)) + '   ' + dim(minority.length ? `minority: ${minority.join(', ')}, noted` : 'unanimous'),
  ];
}

/** Cards side by side, each exactly `width` cells, `GAP` apart, after `MARGIN`. */
function side(cards: string[][], width: number): string[] {
  const h = Math.max(...cards.map((c) => c.length));
  return Array.from({ length: h }, (_, i) => ' '.repeat(MARGIN) + cards.map((c) => c[i] ?? ' '.repeat(width)).join(' '.repeat(GAP)));
}

/** The council view's body, `h` rows (the attention slot and the blank row under it are the frame's). */
export function weave(m: OModel, w: number, h: number): string[] {
  const c = current(m);
  if (!c) return ['', dim('  No Council yet.'), '', dim('  Convene one in the console:  /council <proposition>')];
  const phase = phaseOf(c);
  const now = seatsNow(m, c);
  const pos = positions(c);
  const stacked = w < 72;
  const per = stacked ? 1 : 3;
  const usable = w - 2 * MARGIN;
  const cardW = stacked ? Math.min(usable, 40) : Math.min(CARD, Math.floor((usable - GAP * (per - 1)) / per));
  const inner = Math.max(6, cardW - 3); // one cell of padding left, two right
  const bg = m.pal.bg ?? null;
  const tintFor = (k: number) => T.tint(bg, m.pal.dark, undefined, k);
  const cards = c.seats.map((seat) => { const s = seatCard(m, c, seat, phase, inner, now, pos); return tintCard(s.text.map((l) => ' ' + T.clip(l, inner)), cardW, tintFor(s.tone)); });
  const lower: string[] = [];
  for (let i = 0; i < cards.length; i += per) { if (i) lower.push(''); lower.push(...(stacked ? cards[i]!.map((l) => ' '.repeat(MARGIN) + l) : side(cards.slice(i, i + per), cardW))); }
  const vw = Math.min(VERDICT, usable);
  const v = tintCard(verdictCard(m, c, phase, pos, now.speaking).map((l) => ' ' + T.clip(l, vw - 2)), vw, tintFor(1)).map((l) => ' '.repeat(MARGIN) + l);
  const n = m.st.councils.length;
  const q = ' ' + dim('question') + '  ' + T.text(m.pal)(clean(c.proposition, Math.max(8, w - 12 - (n > 1 ? 10 : 0)))) + (n > 1 ? dim(`  (${Math.min(m.pick, n - 1) + 1} of ${n})`) : '');
  const top = [T.clip(q, w), track(m, c, w, phase), ''];
  const rest = [...lower, '', ...v];
  const room = Math.max(1, h - top.length);
  let shown = rest;
  if (rest.length > room) {
    // Not everything fits: the last row says what is out of view, and ↑ ↓ move (the question and the track stay).
    const avail = Math.max(1, room - 1);
    const start = Math.min(Math.max(0, m.scroll), rest.length - avail);
    const above = start, below = rest.length - start - avail;
    const note = [above ? `↑ ${above} above` : '', below ? `↓ ${below} more` : ''].filter(Boolean).join(' · ');
    shown = [...rest.slice(start, start + avail), T.spread('', dim(note + ' '), w)];
  }
  return [...top, ...shown].map((l) => T.clip(l, w)).map((l) => (stringWidth(l) > w ? T.clip(l, w) : l));
}
