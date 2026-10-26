// /org: the organization's bundles, goals, perspectives and open items, and the operator's own actions on them
// (`specs/026` §7.1). A panel over the chat area like search and sources (`panel.ts` dispatches here): it owns the
// keyboard while open and Esc closes it. Nothing here raises an approval card: each action is a key the operator presses
// and a path or name they type, and the daemon runs the same `delphi bundle` command the shell does (`org.action`).
import { isAbsolute, resolve } from 'node:path';
import chalk from 'chalk';
import type { Ev } from './input.ts';
import * as T from './theme.ts';

export type OrgBundle = {
  id: string; title: string; version: string; provenance: string; order: number; review_by: string;
  stale: boolean; matched: boolean; source: { kind: string; path: string };
  /** `null` until something can say (an update check against the recorded source is not built) */
  update_available: string | boolean | null;
  /** how far the bundle's vectors are, e.g. `42%` or `100% (shipped, verified)`; `null` from a daemon that predates slice B3 */
  vectors: string | null;
};
export type OrgGoal = { bundle: string; id: string; title: string; horizon: string; measure: string; status: string };
export type OrgPerspective = { bundle: string; id: string; title: string; statement: string };
export type OrgView = {
  bundles: OrgBundle[]; goals: OrgGoal[]; perspectives: OrgPerspective[]; open: string[];
  slice: { bytes: number; cap: number; bundles: number; trimmed: string[] };
  unavailable: string[];
};

export type Verb = 'add' | 'update' | 'rollback' | 'remove';
export type OrgPanel = {
  kind: 'org';
  view: OrgView | null;
  /** why the view could not be read */
  error: string;
  scroll: number;
  /** the verb being typed for, or `null` */
  asking: Verb | null;
  input: string;
  busy: boolean;
  /** what the last action printed */
  result: { title: string; ok: boolean; lines: string[] } | null;
  flash?: string;
};

export type OrgAction =
  | { t: 'none' }
  | { t: 'close' }
  | { t: 'refresh' }
  | { t: 'run'; body: { action: Verb; path?: string; name?: string; to?: string } };

export const openOrg = (): OrgPanel => ({ kind: 'org', view: null, error: '', scroll: 0, asking: null, input: '', busy: false, result: null });

const clean = (s: unknown): string => T.sanitize(String(s ?? '')).replace(/\s+/g, ' ').trim();

/** What the operator typed as a directory, as the absolute path the daemon wants: `~` and relative paths follow the console. */
export function resolveDir(typed: string, cwd: string, home: string | undefined): string {
  const t = typed.trim();
  const p = t === '~' ? (home ?? t) : t.startsWith('~/') && home ? home + t.slice(1) : t;
  return isAbsolute(p) ? resolve(p) : resolve(cwd, p);
}

/** The bundle a typed name means, for the prefilled answer to `update` and `remove`: the only one, or none. */
const onlyOne = (v: OrgView | null): string => (v && v.bundles.length === 1 ? v.bundles[0]!.id : '');

export function key(p: OrgPanel, e: Ev): { panel: OrgPanel; action: OrgAction } {
  const keep = (panel: OrgPanel = p, action: OrgAction = { t: 'none' }) => ({ panel, action });
  if (e.t === 'ctrl' && e.c === 'c') return keep(p, { t: 'close' });
  const k = e.t === 'key' ? e.k : e.t === 'text' ? e.s : '';
  if (p.asking) {
    if (p.busy) return keep();
    if (k === 'esc') return keep({ ...p, asking: null, input: '' });
    if (e.t === 'text' && [...p.input].length < 400) return keep({ ...p, input: p.input + e.s.replace(/[\r\n]/g, '') });
    if (e.t === 'paste') return keep({ ...p, input: (p.input + e.s.replace(/\s*\n\s*/g, ' ')).slice(0, 400) });
    if (k === 'backspace') return keep({ ...p, input: [...p.input].slice(0, -1).join('') });
    if (k === 'enter') {
      const v = p.input.trim();
      if (!v) return keep({ ...p, flash: p.asking === 'add' ? 'type the directory of the bundle' : 'type the name of an added bundle' });
      // rollback takes `name` or `name <hash>`: the second word is the version to go back to (default: the previous one)
      const [name, to] = v.split(/\s+/);
      const body = p.asking === 'add' ? { action: 'add' as const, path: v } : p.asking === 'rollback' && to ? { action: 'rollback' as const, name: name!, to } : { action: p.asking, name: v };
      return keep({ ...p, asking: null, input: '', busy: true, flash: undefined }, { t: 'run', body });
    }
    return keep();
  }
  if (k === 'esc' || k === 'q') return keep(p, { t: 'close' });
  if (k === 'up' || k === 'k') return keep({ ...p, scroll: Math.max(0, p.scroll - 1) });
  if (k === 'down' || k === 'j') return keep({ ...p, scroll: p.scroll + 1 });
  if (k === 'pgup') return keep({ ...p, scroll: Math.max(0, p.scroll - 10) });
  if (k === 'pgdn' || k === ' ') return keep({ ...p, scroll: p.scroll + 10 });
  if (p.busy) return keep();
  if (k === 'g') return keep({ ...p, flash: undefined }, { t: 'refresh' });
  if (k === 'a') return keep({ ...p, asking: 'add', input: '' });
  if (k === 'u' || k === 'x' || k === 'r') {
    if (!p.view?.bundles.length) return keep({ ...p, flash: 'no bundle is added yet' });
    return keep({ ...p, asking: k === 'u' ? 'update' : k === 'r' ? 'rollback' : 'remove', input: onlyOne(p.view) });
  }
  return keep();
}

export const hint = (p: OrgPanel): string =>
  p.flash ? p.flash
    : p.busy ? 'working…'
      : p.asking === 'add' ? 'type the directory of the bundle · enter add · esc cancel'
        : p.asking === 'rollback' ? 'type the bundle name, and a version hash to go back to if not the previous one · enter rollback · esc cancel'
          : p.asking ? `type the name of the bundle to ${p.asking} · enter ${p.asking} · esc cancel`
          : 'a add <dir> · u update <name> · x remove <name> · r rollback <name> · g reload · ↑↓ scroll · esc close';

/** A fixed-width cell: cut with an ellipsis, padded with spaces. */
const pad = (s: string, n: number): string => s + ' '.repeat(Math.max(0, n - T.width(s)));
const fit = (s: string, n: number): string => pad(T.clip(s, n), n);

/** The deck body as lines, before scrolling. Colour is never the only signal: stale, unmatched and the like are words. */
export function lines(p: OrgPanel, pal: T.Pal, w: number): string[] {
  const out: string[] = [];
  const head = (t: string, n?: number) => out.push('', '  ' + chalk.bold(T.text(pal)(t)) + (n === undefined ? '' : T.dim(` (${n})`)));
  const row = (s: string) => out.push('  ' + T.clip(s, w - 4));
  const v = p.view;
  out.push('', '  ' + T.clip(chalk.bold(T.text(pal)('Organization')) + T.dim('  what the added bundles say, and what you can do about it'), w - 4));
  if (p.result) {
    out.push('', '  ' + (p.result.ok ? T.ok : T.alert)(T.clip(p.result.title, w - 4)));
    for (const l of p.result.lines) out.push('    ' + T.dim(T.clip(l, w - 6)));
  }
  if (!v) {
    out.push('', '  ' + T.dim(T.clip(p.error ? `could not read the organization: ${clean(p.error)}` : 'reading…', w - 4)));
    return out;
  }
  head('Bundles', v.bundles.length);
  if (!v.bundles.length) row(T.dim('none added. `a` adds a bundle from a directory; `delphi bundle add <dir>` does the same from a shell'));
  else {
    const idW = Math.min(30, Math.max(8, ...v.bundles.map((b) => T.width(clean(b.id)))));
    out.push('  ' + T.dim(T.clip(`${fit('bundle', idW)}  ${fit('version', 9)}  ${fit('provenance', 10)}  ${fit('scope', 9)}  ${fit('review', 18)}  ${fit('update', 6)}  vectors`, w - 4)));
    for (const b of v.bundles) {
      const review = b.stale ? T.warn(fit(`stale (${clean(b.review_by)})`, 18)) : fit(`fresh (${clean(b.review_by)})`, 18);
      row(`${fit(clean(b.id), idW)}  ${fit(clean(b.version), 9)}  ${fit(clean(b.provenance), 10)}  ${fit(b.matched ? 'matched' : 'unmatched', 9)}  ${review}  ${fit(b.update_available === null || b.update_available === false ? '-' : clean(b.update_available === true ? 'yes' : b.update_available), 6)}  ${b.vectors === null ? '-' : clean(b.vectors)}`);
    }
  }
  head('Goals', v.goals.length);
  if (!v.goals.length) row(T.dim('none'));
  else {
    out.push('  ' + T.dim(T.clip(`${fit('id', 14)}  ${fit('status', 10)}  ${fit('horizon', 18)}  title · measure`, w - 4)));
    for (const g of v.goals) row(`${fit(clean(g.id), 14)}  ${fit(clean(g.status), 10)}  ${fit(clean(g.horizon), 18)}  ${clean(g.title)}${g.measure ? T.dim(' · ' + clean(g.measure)) : ''}`);
  }
  head('Perspectives', v.perspectives.length);
  if (!v.perspectives.length) row(T.dim('none'));
  for (const q of v.perspectives) row(`${fit(clean(q.id), 14)}  ${clean(q.statement || q.title)}`);
  head('Open items', v.open.length);
  if (!v.open.length) row(T.dim('nothing needs attention'));
  for (const o of v.open) row('- ' + clean(o));
  row(T.dim(`the prompt slice is ${v.slice.bytes} of ${v.slice.cap} bytes${v.slice.trimmed.length ? `, trimmed for ${v.slice.trimmed.map(clean).join(', ')}` : ''}`));
  head('Actions');
  row('a  add <dir>       check a bundle directory again here, then copy and index it');
  row('u  update <name>   add it again from where it came from (a new publisher key needs `delphi bundle add --replace-publisher`)');
  row('r  rollback <name> [hash]   go back to an earlier version still in the cache; no approval, the lock history is the record');
  row('x  remove <name>   take it out of the lock and the index');
  row('g  reload          read the organization again');
  row(T.dim(`not available yet: ${v.unavailable.map((u) => clean(u).replace('_', ' ')).join(', ')}. No action asks for an approval: you pressing the key is the decision.`));
  return out;
}

/** The panel as exactly `h` lines. */
export function view(p: OrgPanel, pal: T.Pal, w: number, h: number): string[] {
  const body = lines(p, pal, w);
  const ask = p.asking ? ['', '  ' + T.link(p.asking === 'add' ? 'add directory› ' : `${p.asking} bundle› `) + T.text(pal)(p.input) + '▏'] : [];
  const room = Math.max(1, h - ask.length);
  const scroll = Math.min(p.scroll, Math.max(0, body.length - room));
  const shown = [...body.slice(scroll, scroll + room), ...ask];
  return [...shown, ...Array(Math.max(0, h - shown.length)).fill('')].slice(0, h);
}

/** The result of `org.action` as what the panel shows under its title. */
export function resultOf(verb: Verb, target: string, r: { ok?: boolean; output?: string }): NonNullable<OrgPanel['result']> {
  const text = String(r.output ?? '').split('\n').map((l) => T.sanitize(l).trimEnd()).filter((l, i, a) => l || (i > 0 && a[i - 1]));
  return { title: `${r.ok ? '' : 'did not '}${verb} ${clean(target)}${r.ok ? ': done' : ''}`, ok: !!r.ok, lines: text.length ? text : ['(no output)'] };
}
