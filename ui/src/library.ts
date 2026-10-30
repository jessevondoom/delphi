// The library's Sources and Claims views in the knowledge window (specs/055 SK2 and SK3, drawn in the 053 vocabulary).
// Pure, like `archives.ts`: the daemon's `delphi sources list` and `delphi claims list` output goes in as parsed JSON,
// lines come out. A claim's status is the daemon's own word (specs/052), never worked out here.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';

export type LibKind = 'sources' | 'claims';
export type Src = { id: string; title: string; type: string; licence: string; level: string; version: string; status: T.KnowledgeStatus; retrieved: string; checked: string; claims: string[]; private: boolean };
export type Anchor = { source: string; locator: string; quote: string; state: string };
export type Clm = { id: string; text: string; kind: string; scope: string; status: T.KnowledgeStatus; created: string; confirmed: string; anchors: Anchor[]; cited: number; contestedBy: string[] };
export type Excerpt = { id: string; locator: string; purpose: string; bytes: number; contested: boolean };
export type Detail = { k: 'source'; src: Src; excerpts: Excerpt[] | null } | { k: 'claim'; claim: Clm };
export type Lib = { sources: Src[] | null; claims: Clm[] | null; sel: number; detail: Detail | null; note: string; /** A source another view asked to open once the list arrives. */ want?: string };
export type LibAction = { t: 'none' } | { t: 'library'; what: LibKind; open?: string } | { t: 'excerpts'; id: string } | { t: 'leave'; to: 'search' };

export const emptyLib = (): Lib => ({ sources: null, claims: null, sel: 0, detail: null, note: '' });

const STATUSES: T.KnowledgeStatus[] = ['noted', 'checking', 'validated', 'contested', 'disproved', 'retired'];
const str = (v: unknown, max = 400): string => (typeof v === 'string' ? sanitize(v).slice(0, max) : '');
const list = (body: unknown): any[] => (Array.isArray(body) ? body.filter((x) => x && typeof x === 'object') : []);

/** A capture is noted until a claim says otherwise (its own meta still reads "not validated"); a retired source stays retired. */
const sourceStatus = (s: unknown): T.KnowledgeStatus => (/retired/i.test(str(s)) ? 'retired' : 'noted');

export function parseSources(body: unknown): Src[] {
  return list(body).map((x) => {
    const m = x.meta && typeof x.meta === 'object' ? x.meta : {};
    return {
      id: str(x.id, 120), title: str(m.title, 200) || str(x.id, 120), type: str(m.type, 40), licence: str(m.licence, 60), level: str(m.capture_level, 20), version: str(m.version, 20),
      status: sourceStatus(m.status), retrieved: str(m.retrieved, 40), checked: str(m.last_checked, 40),
      claims: Array.isArray(m.cited_by) ? m.cited_by.map((c: unknown) => str(c, 60)).filter(Boolean) : [], private: m.private === true || str(m.licence).toLowerCase() === 'private',
    };
  }).filter((s) => s.id);
}

export function parseClaims(body: unknown): Clm[] {
  return list(body).map((x) => ({
    id: str(x.id, 60), text: str(x.text, 2000), kind: str(x.kind, 20), scope: str(x.scope, 20),
    status: STATUSES.find((k) => k === x.status) ?? 'noted', created: str(x.created, 40), confirmed: str(x.last_confirmed, 40),
    anchors: list(x.anchors).map((a) => ({ source: str(a.source_id, 120), locator: str(a.locator, 80), quote: str(a.quote, 400), state: str(a.state, 20) })),
    cited: Array.isArray(x.cited_by) ? x.cited_by.length : 0, contestedBy: Array.isArray(x.contested_by) ? x.contested_by.map((c: unknown) => str(c, 60)).filter(Boolean) : [],
  })).filter((c) => c.id);
}

export function parseExcerpts(body: unknown): Excerpt[] {
  return list(body).map((e) => ({ id: str(e.id, 20), locator: str(e.locator, 80), purpose: str(e.purpose, 200), bytes: Number(e.bytes) || 0, contested: e.contested === true }));
}

export function gotSources(l: Lib, body: unknown): Lib {
  const sources = parseSources(body);
  const at = l.want ? sources.findIndex((s) => s.id === l.want) : -1;
  const { want: _want, ...rest } = l;
  return at >= 0 ? { ...rest, sources, sel: at, detail: { k: 'source', src: sources[at]!, excerpts: null }, note: '' } : { ...rest, sources, sel: 0, detail: null, note: l.want ? 'that source is not in the library' : '' };
}
export const gotClaims = (l: Lib, body: unknown): Lib => ({ ...l, claims: parseClaims(body), sel: 0, detail: null, note: '' });
export const gotExcerpts = (l: Lib, id: string, body: unknown): Lib => (l.detail?.k === 'source' && l.detail.src.id === id ? { ...l, detail: { ...l.detail, excerpts: parseExcerpts(body) } } : l);

export const items = (l: Lib, what: LibKind): (Src | Clm)[] | null => (what === 'sources' ? l.sources : l.claims);

/** One key in a library view. `used` is false for the keys the window keeps (← → change the view). */
export function key(l: Lib, what: LibKind, e: Ev, h: number): { l: Lib; action: LibAction; used: boolean } {
  const used = (next: Lib, action: LibAction = { t: 'none' }) => ({ l: next, action, used: true });
  const n = items(l, what)?.length ?? 0;
  const move = (to: number) => used({ ...l, sel: Math.max(0, Math.min(Math.max(0, n - 1), to)) });
  if (e.t === 'wheel') return l.detail ? used(l) : move(l.sel + (e.dir === 'up' ? -1 : 1));
  if (e.t !== 'key' || e.alt || e.ctrl) return { l, action: { t: 'none' }, used: false };
  if (l.detail) {
    if (e.k === 'esc' || e.k === 'backspace' || e.k === 'enter') return used({ ...l, detail: null });
    if (e.k === 'left' || e.k === 'right') return { l, action: { t: 'none' }, used: false };
    return used(l);
  }
  switch (e.k) {
    case 'down': return move(l.sel + 1);
    case 'up': return move(l.sel - 1);
    case 'pgdn': return move(l.sel + Math.max(1, Math.floor(h / 3)));
    case 'pgup': return move(l.sel - Math.max(1, Math.floor(h / 3)));
    case 'enter': {
      const x = items(l, what)?.[l.sel];
      if (!x) return used(l);
      return what === 'sources'
        ? used({ ...l, detail: { k: 'source', src: x as Src, excerpts: null } }, { t: 'excerpts', id: x.id })
        : used({ ...l, detail: { k: 'claim', claim: x as Clm } });
    }
    case 'esc': case 'shift-tab': return used(l, { t: 'leave', to: 'search' });
    case 'left': case 'right': return { l, action: { t: 'none' }, used: false };
    default: return used(l);
  }
}

/** `r` reloads the list the daemon holds; asked for as a text key so it never fights the cursor keys. */
export const wantsReload = (e: Ev): boolean => e.t === 'text' && e.s === 'r';

const word = (s: T.KnowledgeStatus) => T.dim(s.padEnd(10));
const dateOnly = (iso: string) => iso.slice(0, 10);
const hint = (s: string) => T.dim(' ' + s);
const plural = (n: number, one: string, many: string) => (n === 1 ? `1 ${one}` : `${n} ${many}`);

function tabs(what: LibKind, w: number): string {
  const t = (k: LibKind, label: string) => (k === what ? chalk.bold(T.heading(label)) : T.dim(label));
  return T.clip(` ${t('sources', 'sources')}  ${t('claims', 'claims')}   ${T.dim('ctrl+l switch')}`, w);
}

function sourceRow(s: Src, on: boolean, w: number, now: number, ago: (iso: string, now: number) => string): string {
  const cited = s.claims.length ? plural(s.claims.length, 'claim', 'claims') : '';
  const tail = (w >= 70 ? T.dim(`${s.level || '—'} · ${s.licence || '—'}`.padEnd(24)) : '') + T.dim(ago(s.checked || s.retrieved, now).padStart(6)) + (w >= 90 ? T.dim(cited.padStart(10)) : '');
  const head = ` ${on ? T.accent('›') : ' '} ${T.statusMark(s.status)} ${on ? chalk.bold(s.title) : s.title}${s.private ? T.dim(' · private') : ''}`;
  return T.spread(head, tail + ' ', w);
}

function claimRow(c: Clm, on: boolean, w: number, now: number, ago: (iso: string, now: number) => string): string {
  const tail = (w >= 80 ? word(c.status) : '') + T.dim(ago(c.confirmed || c.created, now).padStart(6));
  const head = ` ${on ? T.accent('›') : ' '} ${T.statusMark(c.status)} ${on ? chalk.bold(c.text.replace(/\s+/g, ' ')) : c.text.replace(/\s+/g, ' ')}`;
  return T.spread(T.clip(head, Math.max(10, w - T.width(tail) - 2)), tail + ' ', w);
}

function sourceDetail(d: Extract<Detail, { k: 'source' }>, w: number, now: number, ago: (iso: string, now: number) => string): string[] {
  const s = d.src;
  const out = [` ${T.statusMark(s.status)} ${chalk.bold(s.title)}`, '', T.dim(`   ${s.id}`)];
  const row = (k: string, v: string) => (v ? `   ${T.dim(k.padEnd(11))}${v}` : '');
  out.push(row('kept as', `${s.level || '—'}${s.version ? ` · version ${s.version}` : ''}${s.private ? ' · private' : ''}`), row('licence', s.licence), row('checked', s.checked ? `${dateOnly(s.checked)} (${ago(s.checked, now)})` : ''), row('claims', s.claims.length ? s.claims.join(', ') : 'none cite it yet'));
  out.push('', ` ${T.dim('excerpts')}`);
  if (d.excerpts === null) out.push(T.dim('   reading…'));
  else if (!d.excerpts.length) out.push(T.dim('   none kept'));
  else for (const e of d.excerpts) out.push(T.clip(`   ${e.id.padEnd(4)} ${e.locator.padEnd(18)} ${T.dim(`${e.bytes} bytes`)}${e.contested ? '  ' + T.statusMark('contested') + ' ' + T.dim('contested') : ''}  ${T.dim(e.purpose)}`, w));
  return out.filter((l, i, a) => l !== '' || a[i - 1] !== '');
}

function claimDetail(d: Extract<Detail, { k: 'claim' }>, w: number, now: number, ago: (iso: string, now: number) => string): string[] {
  const c = d.claim;
  const out = [` ${T.statusMark(c.status)} ${chalk.bold(c.status)}  ${T.dim(`${c.kind} · ${c.scope} · confirmed ${ago(c.confirmed || c.created, now)}`)}`, '', ...c.text.split('\n').map((l) => '   ' + l), '', T.dim(`   ${c.id}`)];
  out.push('', ` ${T.dim(plural(c.anchors.length, 'anchor', 'anchors'))}`);
  for (const a of c.anchors) out.push(T.clip(`   ${a.state === 'active' ? T.dim('·') : T.statusMark('contested')} ${a.source}${a.locator ? ' · ' + a.locator : ''}${a.state && a.state !== 'active' ? T.dim(` · ${a.state}`) : ''}`, w), T.clip(`       ${T.dim('“' + a.quote.replace(/\s+/g, ' ') + '”')}`, w));
  const more = [c.cited ? `handed to ${plural(c.cited, 'mission', 'missions')}` : '', c.contestedBy.length ? `contested by ${c.contestedBy.join(', ')}` : ''].filter(Boolean).join(' · ');
  if (more) out.push('', T.dim(`   ${more}`));
  return out;
}

/** The view's body: `h` lines of at most `w` columns, the last one the key hint. */
export function view(l: Lib, what: LibKind, w: number, h: number, now: number, ago: (iso: string, now: number) => string): string[] {
  const top = ['', tabs(what, w), T.dim('─'.repeat(Math.max(0, w)))];
  const all = items(l, what);
  const room = Math.max(1, h - top.length - 2);
  let body: string[];
  let keys: string;
  if (l.detail) {
    body = l.detail.k === 'source' ? sourceDetail(l.detail, w, now, ago) : claimDetail(l.detail, w, now, ago);
    keys = 'esc back · ← → views';
  } else if (all === null) body = ['', T.dim(`   reading the ${what}…`)], keys = 'esc back';
  else if (!all.length) {
    body = ['', T.dim(what === 'sources' ? '   no sources yet: `delphi sources add <url | file | doi>` captures one' : '   no claims yet: `delphi claims add <text>` records one'), ''];
    keys = 'r reload · esc back · ctrl+l switch';
  } else {
    const first = Math.max(0, Math.min(l.sel - Math.floor(room / 2), all.length - room));
    body = all.slice(first, first + room).map((x, i) => (what === 'sources' ? sourceRow(x as Src, first + i === l.sel, w, now, ago) : claimRow(x as Clm, first + i === l.sel, w, now, ago)));
    keys = `↑ ↓ pick · enter open · r reload · esc back · ${plural(all.length, what === 'sources' ? 'source' : 'claim', what)}`;
  }
  const lines = [...top, ...body.slice(0, room)];
  while (lines.length < h - 2) lines.push('');
  if (l.note) lines.push(T.dim(' ' + sanitize(l.note)));
  else lines.push('');
  lines.push(hint(keys));
  return lines.slice(0, h).map((s) => T.clip(s, w));
}
