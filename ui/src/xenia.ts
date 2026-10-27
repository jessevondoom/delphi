// /xenia: the organization's shared memory store, read-only (plan milestone X9b; `specs/011` §4.1, §7.3). A panel over the
// chat area like /org (`panel.ts` dispatches here): it owns the keyboard while open and Esc closes it.
//
// Nothing here changes anything. There is no key that approves, signs, syncs or publishes, and the daemon methods it
// calls (`xenia.status`, `xenia.pending`) are reads. A steward's card shows what they must see before attesting a record
// and tells them which terminal command to run: the signing prompt, with the exact record in front of it, is in the
// terminal and nowhere else.
//
// Every string in the answer came from a shared store and is hostile display text. It is drawn as text only: the
// theme's sanitize removes control and bidi characters, it is clipped, and nothing is parsed as markup or as a link.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import * as T from './theme.ts';

export type Capped<X> = { total: number; shown: X[] };
export type Store = {
  org: string; store: string;
  problems: { file: string; slug: string }[];
  accepted_head: string | null; roster_epoch: number | null; admins: number | null; members: number | null;
  freshness: { state: string; reason: string; issued_at: string; expires_at: string; age_secs: number; life_used_pct: number } | null;
  tiers: { l2: number; l1: number; l0: number; hidden: number };
  contested: Capped<string>;
  quarantined: Capped<{ path: string; reason: string }>;
  quarantine_by_reason: Record<string, number>;
  last_sync: { outcome: string; reason: string; findings: number; at: number } | null;
  unreachable_days: number | null;
  rewrite_pending: { fetched: string; command: string } | null;
  me: { admin: boolean; steward: boolean } | null;
};
export type Card = {
  org: string; store: string; id: string; rev: number; title: string; author_handle: string; tags: string[]; summary: string;
  scan_flags: { class: string; field: string }[]; disputed: boolean; held_by_staleness: boolean; command: string;
  similar: { id: string; title: string; tier: string }[];
};
export type XeniaView = { stores: Store[]; cards: Card[]; total: number };
export type XeniaPanel = { kind: 'xenia'; view: XeniaView | null; error: string; scroll: number };
export type XeniaAction = { t: 'none' } | { t: 'close' } | { t: 'refresh' };

export const openXenia = (): XeniaPanel => ({ kind: 'xenia', view: null, error: '', scroll: 0 });

/** Store text as one inert line: control and bidi characters removed, whitespace folded. The caller clips. */
const clean = (s: unknown): string => T.sanitize(String(s ?? '')).replace(/\s+/g, ' ').trim();

export function key(p: XeniaPanel, e: Ev): { panel: XeniaPanel; action: XeniaAction } {
  const keep = (panel: XeniaPanel = p, action: XeniaAction = { t: 'none' }) => ({ panel, action });
  if (e.t === 'ctrl' && e.c === 'c') return keep(p, { t: 'close' });
  const k = e.t === 'key' ? e.k : e.t === 'text' ? e.s : '';
  if (k === 'esc' || k === 'q') return keep(p, { t: 'close' });
  if (k === 'up' || k === 'k') return keep({ ...p, scroll: Math.max(0, p.scroll - 1) });
  if (k === 'down' || k === 'j') return keep({ ...p, scroll: p.scroll + 1 });
  if (k === 'pgup') return keep({ ...p, scroll: Math.max(0, p.scroll - 10) });
  if (k === 'pgdn' || k === ' ') return keep({ ...p, scroll: p.scroll + 10 });
  if (k === 'g') return keep(p, { t: 'refresh' });
  return keep();
}

export const hint = (_p: XeniaPanel): string => 'read-only · g reload · ↑↓ scroll · esc close';

const age = (secs: number): string => (secs < 3600 ? `${Math.max(0, Math.round(secs / 60))}m` : secs < 172800 ? `${Math.round(secs / 3600)}h` : `${Math.round(secs / 86400)}d`);
/** `s` cut into pieces of at most `n` characters (no ellipsis): for a command that must stay whole. */
const chunks = (s: string, n: number): string[] => {
  const cs = [...s];
  const out: string[] = [];
  for (let i = 0; i < cs.length; i += n) out.push(cs.slice(i, i + n).join(''));
  return out.length ? out : [''];
};
const num = (n: unknown): number => (typeof n === 'number' && Number.isFinite(n) ? n : 0);

/** The panel body as lines, before scrolling. Colour is never the only signal: stale, problem and the like are words. */
export function lines(p: XeniaPanel, pal: T.Pal, w: number): string[] {
  const out: string[] = [];
  const row = (s: string, indent = 4) => out.push(' '.repeat(indent) + T.clip(s, w - indent - 2));
  const head = (t: string, n?: number) => out.push('', '  ' + chalk.bold(T.text(pal)(t)) + (n === undefined ? '' : T.dim(` (${n})`)));
  out.push('', '  ' + T.clip(chalk.bold(T.text(pal)('Xenia')) + T.dim('  the organization\'s shared memory; read-only, nothing here signs or syncs'), w - 4));
  const v = p.view;
  if (!v) {
    out.push('', '  ' + T.dim(T.clip(p.error ? `could not read Xenia: ${clean(p.error)}` : 'reading…', w - 4)));
    return out;
  }
  if (!v.stores.length) {
    out.push('', '  ' + T.dim('no joined store. `delphi xenia join <url> ...` in a terminal joins one.'));
    return out;
  }
  head('Stores', v.stores.length);
  for (const s of v.stores) {
    out.push('', '  ' + chalk.bold(T.text(pal)(clean(`${s.org}/${s.store}`))) + (s.me ? T.dim(`  you: ${[s.me.admin ? 'admin' : '', s.me.steward ? 'steward' : ''].filter(Boolean).join(' + ') || 'member'}`) : ''));
    for (const pr of s.problems) row(T.alert(`PROBLEM ${clean(pr.file)}: ${clean(pr.slug)}`));
    row(s.accepted_head ? `head ${clean(s.accepted_head).slice(0, 12)} · roster epoch ${num(s.roster_epoch)} · ${num(s.admins)} admin(s) · ${num(s.members)} member(s)` : 'nothing accepted yet');
    const f = s.freshness;
    if (f) row(f.state === 'fresh' ? `freshness fresh · issued ${age(num(f.age_secs))} ago · ${num(f.life_used_pct)}% of its life used${num(f.life_used_pct) >= 50 ? T.warn('  · an admin should refresh it') : ''}` : T.alert(`freshness STALE (${clean(f.reason)}): no endorsement counts`));
    row(`records L2 ${num(s.tiers.l2)} · L1 ${num(s.tiers.l1)} · L0 ${num(s.tiers.l0)} · hidden ${num(s.tiers.hidden)}`);
    if (s.contested.total) row(T.warn(`contested ${s.contested.total}: `) + s.contested.shown.map(clean).join(', ') + (s.contested.total > s.contested.shown.length ? ` and ${s.contested.total - s.contested.shown.length} more` : ''));
    if (s.quarantined.total) {
      row(`quarantined ${s.quarantined.total} file(s): ${Object.entries(s.quarantine_by_reason).map(([k, n]) => `${clean(k)} ×${num(n)}`).join(', ')}`);
      for (const q of s.quarantined.shown.slice(0, 8)) row(T.dim(`${clean(q.reason)}  ${clean(q.path)}`), 6);
      if (s.quarantined.total > Math.min(8, s.quarantined.shown.length)) row(T.dim(`and ${s.quarantined.total - Math.min(8, s.quarantined.shown.length)} more (\`delphi xenia status\` lists more)`), 6);
    }
    if (s.last_sync) row(`last sync ${clean(s.last_sync.outcome)}${s.last_sync.reason ? ` (${clean(s.last_sync.reason)})` : ''}${s.last_sync.findings ? `, ${num(s.last_sync.findings)} path(s)` : ''}`);
    if (s.unreachable_days !== null) row(T.warn(`host unreachable for ${num(s.unreachable_days)} day(s); offline use continues`));
    if (s.rewrite_pending) row(T.alert(`a rewritten history waits for you: run \`${clean(s.rewrite_pending.command)}\` in a terminal`));
  }
  head('Waiting for your attestation', v.total);
  if (!v.cards.length) row(T.dim(v.total ? 'none shown' : 'nothing awaits you'), 4);
  for (const c of v.cards) {
    out.push('');
    row(chalk.bold(T.text(pal)(clean(c.title))) + T.dim(`  ${clean(c.org)}/${clean(c.store)} · ${clean(c.id)} rev ${num(c.rev)}`), 4);
    row(`author ${clean(c.author_handle)} · tags ${c.tags.map(clean).join(', ') || '(none)'}`, 6);
    row(clean(c.summary), 6);
    row(c.scan_flags.length ? T.warn(`scan flags: ${c.scan_flags.map((f) => `${clean(f.class)} in ${clean(f.field)}`).join(', ')}`) : T.dim('scan flags: none'), 6);
    if (c.disputed) row(T.warn('a member has disputed this record'), 6);
    if (c.held_by_staleness) row(T.warn('it has endorsements that cannot count while the freshness statement is stale'), 6);
    if (c.similar.length) { row(T.dim('similar records already in the store:'), 6); for (const s of c.similar) row(`${clean(s.tier)}  ${clean(s.title)}  ${T.dim(clean(s.id))}`, 8); }
    // The command is never clipped: the steward has to be able to run exactly what is shown.
    row(T.dim('to review and sign, in a terminal:'), 6);
    for (const part of chunks(clean(c.command), Math.max(20, w - 12))) row(T.link(part), 8);
  }
  if (v.total > v.cards.length) row(T.dim(`and ${v.total - v.cards.length} more`), 4);
  out.push('');
  for (const part of chunks('Approval is not available here by design: the terminal prompt shows the exact record and signs it, with no model in between.', Math.max(20, w - 6))) out.push('  ' + T.dim(part));
  return out;
}

/** The panel as exactly `h` lines. */
export function view(p: XeniaPanel, pal: T.Pal, w: number, h: number): string[] {
  const body = lines(p, pal, w);
  const scroll = Math.min(p.scroll, Math.max(0, body.length - h));
  const shown = body.slice(scroll, scroll + h);
  return [...shown, ...Array(Math.max(0, h - shown.length)).fill('')].slice(0, h);
}

/** `xenia.status` and `xenia.pending` answers as one view; a daemon that predates them gives an error the panel shows. */
export function viewOf(status: any, pending: any): XeniaView {
  const stores: Store[] = Array.isArray(status?.stores) ? status.stores : [];
  const cards: Card[] = Array.isArray(pending?.cards) ? pending.cards : [];
  return { stores, cards, total: num(pending?.total) };
}
