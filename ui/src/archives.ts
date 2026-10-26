// The knowledge window: the narrator's `archives` pane (specs 028 and 029). A search box, the shape of the store under
// it, results in relevance order, and an inline reader whose links are followed like hyperlinks. Pure: state and key
// events in, state and an action out; the observer does the calls (`kb.get`) and the window draws from `view`.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import { copyText, describe, firstVisible, stepFocus, type Link, type Target } from './links.ts';
import { renderMarkdownLinks } from './markdown.ts';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';

export type Hit = { path: string; collection: string; trust: string; heading: string; preview: string; start_line?: number; end_line?: number };
export type Shelf = {
  collections: { name: string; documents: number; updated: string }[];
  recent: { path: string; collection: string; updated: string }[];
  stats: { documents: number; passages: number; vectors: number } | null;
};
/** One document in the reader, with where the operator is in it. */
export type Entry = { path: string; collection: string; trust: string; stale: boolean; text: string; scroll: number; focus: number | null };
export type Arch = {
  q: string;
  /** Where the keys go: the search box, or the list under it. */
  focus: 'box' | 'list';
  sel: number;
  /** The query the results are for; null while the resting state shows. */
  searched: string | null;
  hits: Hit[];
  took: number | null;
  engine: string | null;
  /** The collection being browsed, when the results are its documents. */
  browse: string | null;
  note: string;
  shelf: Shelf | null;
  /** The reader's history and the place in it; `at` is -1 when the reader is closed. */
  trail: Entry[];
  at: number;
};

export const emptyArch = (): Arch => ({ q: '', focus: 'box', sel: 0, searched: null, hits: [], took: null, engine: null, browse: null, note: '', shelf: null, trail: [], at: -1 });

export type Action =
  | { t: 'none' }
  | { t: 'search'; q: string }
  | { t: 'browse'; name: string }
  | { t: 'open'; target: Extract<Target, { k: 'doc' | 'pointer' }> }
  | { t: 'browser'; url: string }
  | { t: 'copy'; text: string };

type Row = { k: 'hit'; hit: Hit } | { k: 'doc'; path: string; collection: string; updated: string } | { k: 'collection'; name: string; documents: number; updated: string };

/** What the list under the box holds: the results, or (at rest) the collections and the recently changed documents. */
export function rows(a: Arch): Row[] {
  if (a.searched !== null) return a.hits.map((hit) => ({ k: 'hit', hit }));
  const s = a.shelf;
  return s ? [...s.collections.map((c) => ({ k: 'collection' as const, name: c.name, documents: c.documents, updated: c.updated })), ...s.recent.map((d) => ({ k: 'doc' as const, ...d }))] : [];
}

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const slug = (s: string) => s.toLowerCase().replace(/[^\w\s-]/g, '').trim().replace(/\s+/g, '-');
const day = (iso: string) => (iso || '').slice(0, 10);
const READER_MARGIN = 4;

/** The reader's body: the document rendered at `w`, and the links in it (focus is drawn by the caller's `focus`). */
export const layout = (e: Entry, w: number, focus: number | null = null) => renderMarkdownLinks(e.text, Math.max(10, w - READER_MARGIN), { base: e.path, focus });
const bodyRows = (h: number) => Math.max(1, h - 5); // the header, two rules, the status row and the hint

/** The row a section or a line range starts on, in the rendered document: by heading text, or by how far down the source it is. */
function startRow(e: Entry, w: number, target: Target | null): number {
  if (!target || target.k === 'external') return 0;
  const { lines } = layout(e, w);
  if (target.k === 'doc' && target.anchor) {
    const i = lines.findIndex((l) => slug(plain(l)) === slug(target.anchor!));
    return Math.max(0, i - 1);
  }
  if (target.k === 'pointer') {
    const total = Math.max(1, e.text.split('\n').length);
    return Math.max(0, Math.floor(((target.from - 1) / total) * lines.length) - 1);
  }
  return 0;
}

// ── results from the daemon ─────────────────────────────────────────────────

const asHit = (r: any): Hit => ({ path: String(r.path), collection: String(r.collection ?? ''), trust: String(r.trust ?? ''), heading: String(r.heading ?? ''), preview: sanitize(String(r.preview ?? '')), start_line: r.start_line, end_line: r.end_line });

export function gotHits(a: Arch, q: string, body: any): Arch {
  if (a.q.trim() !== q) return a; // the box has moved on
  return { ...a, searched: q, hits: (body.results ?? []).map(asHit), sel: 0, focus: (body.results ?? []).length ? 'list' : 'box', took: body.took_ms ?? null, engine: body.mode ?? null, browse: null, note: '' };
}

export function gotBrowse(a: Arch, name: string, docs: any[]): Arch {
  return { ...a, searched: '', browse: name, hits: docs.map((d) => ({ path: d.rel_path, collection: d.zone, trust: d.source_type, heading: '', preview: day(d.updated_at) })), sel: 0, focus: 'list', took: null, engine: null, note: '' };
}

export function gotShelf(a: Arch, collections: any[], recent: any[], health: any): Arch {
  const shelf: Shelf = {
    collections: collections.map((c) => ({ name: String(c.name), documents: Number(c.documents), updated: String(c.updated ?? '') })),
    recent: recent.map((d) => ({ path: String(d.rel_path), collection: String(d.zone), updated: String(d.updated_at ?? '') })),
    stats: health?.ok ? { documents: health.documents, passages: health.passages, vectors: health.vectors } : null,
  };
  return { ...a, shelf };
}

/** A document arrived: show it, after the entry now on screen (anything forward of it in the history is dropped). */
export function gotDoc(a: Arch, body: any, target: Target | null, w: number): Arch {
  const entry: Entry = { path: String(body.path), collection: String(body.collection ?? ''), trust: String(body.trust ?? ''), stale: !!body.stale, text: String(body.text ?? ''), scroll: 0, focus: null };
  entry.scroll = startRow(entry, w, target);
  const trail = [...a.trail.slice(0, a.at + 1), entry];
  return { ...a, trail, at: trail.length - 1, note: '' };
}

export const withNote = (a: Arch, note: string): Arch => ({ ...a, note });

// ── keys ────────────────────────────────────────────────────────────────────

const none: Action = { t: 'none' };
const NOT_USED = (a: Arch) => ({ a, action: none, used: false });

/**
 * One key event. `used` is false for the keys this pane leaves to the window (← → change the view), so the caller
 * knows whether to go on. `w` and `h` are the body's size, which the reader needs to scroll and to find links.
 */
export function key(a: Arch, e: Ev, w: number, h: number): { a: Arch; action: Action; used: boolean } {
  const used = (next: Arch, action: Action = none) => ({ a: next, action, used: true });
  if (a.at >= 0) return reader(a, e, w, h, used);
  const list = rows(a);
  if (e.t === 'text' || e.t === 'paste') {
    const s = e.t === 'paste' ? e.s.replace(/\s+/g, ' ') : e.s;
    if (/[\x00-\x1f\x7f]/.test(s)) return NOT_USED(a);
    return used({ ...a, focus: 'box', q: (a.q + s).slice(0, 200), note: '' });
  }
  if (e.t === 'ctrl' && e.c === 'u') return used({ ...a, q: '', focus: 'box' });
  if (e.t === 'wheel') return used({ ...a, sel: Math.max(0, Math.min(list.length - 1, a.sel + (e.dir === 'up' ? -1 : 1))), focus: list.length ? 'list' : 'box' });
  if (e.t !== 'key' || e.alt || e.ctrl) return NOT_USED(a);
  switch (e.k) {
    case 'backspace': return used({ ...a, focus: 'box', q: [...a.q].slice(0, -1).join(''), note: '' });
    case 'enter': {
      if (a.focus === 'list' && list[a.sel]) return open(a, list[a.sel]!, used);
      const q = a.q.trim();
      return q ? used({ ...a, note: 'searching…' }, { t: 'search', q }) : used(a);
    }
    case 'down': return list.length ? used({ ...a, focus: 'list', sel: a.focus === 'list' ? Math.min(list.length - 1, a.sel + 1) : 0 }) : used(a);
    case 'up': return a.focus === 'list' ? (a.sel > 0 ? used({ ...a, sel: a.sel - 1 }) : used({ ...a, focus: 'box' })) : used(a);
    case 'pgdn': return list.length ? used({ ...a, focus: 'list', sel: Math.min(list.length - 1, a.sel + 5) }) : used(a);
    case 'pgup': return used({ ...a, sel: Math.max(0, a.sel - 5) });
    case 'tab': case 'shift-tab': return used({ ...a, note: 'online search is not set up: offline is the only mode for now' });
    case 'esc':
      if (a.searched !== null) return used({ ...a, searched: null, hits: [], browse: null, note: '', sel: 0, focus: 'box', took: null });
      return a.q ? used({ ...a, q: '' }) : NOT_USED(a);
    default: return NOT_USED(a); // left and right change the view
  }
}

function open(a: Arch, r: Row, used: (n: Arch, act?: Action) => ReturnType<typeof key>) {
  if (r.k === 'collection') return used({ ...a, note: 'opening…' }, { t: 'browse', name: r.name });
  const path = r.k === 'hit' ? r.hit.path : r.path;
  const target: Target = r.k === 'hit' && r.hit.start_line ? { k: 'pointer', path, from: r.hit.start_line, to: r.hit.end_line ?? r.hit.start_line } : { k: 'doc', path };
  return used({ ...a, note: 'opening…' }, { t: 'open', target });
}

function reader(a: Arch, e: Ev, w: number, h: number, used: (n: Arch, act?: Action) => ReturnType<typeof key>) {
  const cur = a.trail[a.at]!;
  const { lines, links } = layout(cur, w);
  const rowsHigh = bodyRows(h), max = Math.max(0, lines.length - rowsHigh);
  const set = (p: Partial<Entry>): Arch => ({ ...a, trail: a.trail.map((x, i) => (i === a.at ? { ...x, ...p } : x)) });
  const scrollTo = (n: number) => used(set({ scroll: Math.max(0, Math.min(max, n)) }));
  const back = () => used(a.at > 0 ? { ...a, at: a.at - 1 } : { ...a, at: -1, trail: [] });
  const forward = () => used(a.at < a.trail.length - 1 ? { ...a, at: a.at + 1 } : a);
  const moveFocus = (dir: 1 | -1) => {
    const f = cur.focus === null ? firstVisible(links, cur.scroll, rowsHigh) ?? stepFocus(links, null, dir) : stepFocus(links, cur.focus, dir);
    const l = links.find((x) => x.id === f);
    const row = l?.spans[0]?.line ?? cur.scroll;
    const scroll = row < cur.scroll ? row : row >= cur.scroll + rowsHigh ? Math.min(max, row - Math.floor(rowsHigh / 2)) : cur.scroll;
    return used(set({ focus: f, scroll }));
  };
  if (e.t === 'wheel') return scrollTo(cur.scroll + (e.dir === 'up' ? -3 : 3));
  if (e.t === 'text') {
    if (e.s === '[') return back();
    if (e.s === ']') return forward();
    if (e.s === 'y') return used(a, { t: 'copy', text: cur.focus !== null && links[cur.focus] ? copyText(links[cur.focus]!.target) : cur.path });
    return used(a); // the reader has no input: a stray letter changes nothing
  }
  if (e.t === 'ctrl' && e.c === 'c') return NOT_USED(a);
  if (e.t !== 'key' || e.alt || e.ctrl) return used(a);
  switch (e.k) {
    case 'tab': return moveFocus(1);
    case 'shift-tab': return moveFocus(-1);
    case 'enter': {
      const l = cur.focus === null ? undefined : links[cur.focus];
      if (!l) return used(a);
      return l.target.k === 'external' ? used(a, { t: 'browser', url: l.target.url }) : used({ ...a, note: 'opening…' }, { t: 'open', target: l.target });
    }
    case 'esc': case 'backspace': return back();
    case 'up': return scrollTo(cur.scroll - 1);
    case 'down': return scrollTo(cur.scroll + 1);
    case 'pgup': return scrollTo(cur.scroll - Math.max(1, rowsHigh - 2));
    case 'pgdn': return scrollTo(cur.scroll + Math.max(1, rowsHigh - 2));
    case 'home': return scrollTo(0);
    case 'end': return scrollTo(max);
    default: return NOT_USED(a);
  }
}

// ── drawing ─────────────────────────────────────────────────────────────────

const center = (s: string, w: number) => ' '.repeat(Math.max(0, Math.floor((w - T.width(s)) / 2))) + s;
const pad = (lines: string[], h: number) => [...lines.slice(0, h), ...Array(Math.max(0, h - lines.length)).fill('')];
const hint = (s: string) => T.dim(' ' + s);

function box(a: Arch, w: number): string[] {
  const cursor = a.focus === 'box' ? chalk.inverse(' ') : '';
  const inner = Math.min(58, Math.max(10, w - 8));
  const q = [...a.q].slice(-(inner - 6)).join('');
  if (w < 40) return [' ' + T.accent('›') + ' ' + (q || T.dim('search')) + cursor, T.dim('─'.repeat(Math.max(0, w - 2)).padStart(w - 1))];
  const body = ` ${T.accent('›')}  ${q || (a.focus === 'box' ? '' : '')}${q ? '' : T.dim('search what Delphi knows…')}${cursor}`;
  const fill = ' '.repeat(Math.max(0, inner - 1 - T.width(body)));
  return [center('╭' + '─'.repeat(inner) + '╮', w), center('│' + body + fill + ' │', w), center('╰' + '─'.repeat(inner) + '╯', w)];
}

const modeLine = (w: number) => center(`${T.accent('●')} offline    ${T.dim('○ online (not set up)')}`, w);

function mark(sel: boolean, a: Arch) { return sel && a.focus === 'list' ? T.accent('›') : ' '; }

function home(a: Arch, w: number, h: number): string[] {
  const out: string[] = ['', center(chalk.bold('knowledge'), w), ...box(a, w), modeLine(w), ''];
  const list = rows(a);
  if (!a.shelf) out.push(T.dim(center('reading the index…', w)));
  else if (!list.length) out.push(T.dim(center('the knowledge base is empty', w)));
  else {
    const cols = list.filter((r) => r.k === 'collection') as Extract<Row, { k: 'collection' }>[];
    const docs = list.filter((r) => r.k === 'doc') as Extract<Row, { k: 'doc' }>[];
    out.push(T.dim('  collections'));
    cols.forEach((r, i) => out.push(` ${mark(a.sel === i, a)} ${a.sel === i && a.focus === 'list' ? chalk.bold(r.name.padEnd(12)) : r.name.padEnd(12)} ${T.dim(`${r.documents} documents · changed ${day(r.updated)}`)}`));
    if (docs.length) {
      out.push('', T.dim('  recently changed'));
      docs.forEach((r, j) => { const i = cols.length + j; out.push(T.clip(` ${mark(a.sel === i, a)} ${a.sel === i && a.focus === 'list' ? chalk.bold(r.path) : r.path}  ${T.dim(day(r.updated))}`, w)); });
    }
    const s = a.shelf.stats;
    if (s) out.push('', T.dim(`  ${s.documents} documents · ${s.passages} passages · ${s.vectors > 0 ? 'hybrid (keyword + vector)' : 'keyword'} search`));
  }
  if (a.note) out.push('', T.dim(center(a.note, w)));
  return pad(out, h - 1).concat(hint(a.focus === 'list' ? '↑ ↓ pick · enter open · esc back · ← → views' : 'type to search · enter search · ↓ browse · ← → views'));
}

function results(a: Arch, w: number, h: number): string[] {
  const out: string[] = [...box(a, w).slice(w < 40 ? 0 : 1, w < 40 ? 2 : 2)];
  const head = a.browse
    ? `${a.hits.length} documents in ${a.browse}, newest first`
    : a.hits.length ? `${a.hits.length} result${a.hits.length === 1 ? '' : 's'}${a.took !== null ? ` · ${(a.took / 1000).toFixed(1)} s` : ''}${a.engine ? ` · ${a.engine}` : ''}` : 'nothing in the knowledge base matches';
  out.push(T.dim('  ' + head), '');
  const per = 4, room = Math.max(1, Math.floor((h - out.length - 1) / per));
  const top = Math.max(0, Math.min(a.sel - room + 1, a.hits.length - room), a.sel >= room ? a.sel - room + 1 : 0);
  a.hits.slice(top, top + room).forEach((x, k) => {
    const i = top + k, on = i === a.sel && a.focus === 'list';
    const title = `${x.path}${x.heading ? ' › ' + x.heading.split(' > ').pop() : ''}`;
    out.push(T.spread(` ${mark(i === a.sel, a)} ${String(i + 1).padStart(2)}  ${on ? chalk.bold(title) : title}`, T.dim(x.collection + ' '), w));
    // A preview opens with the section's own heading, which the title row already says.
    const prev = sanitize(x.preview).split('\n').filter((l) => !/^#+\s/.test(l)).map((l) => l.trim()).filter(Boolean).slice(0, 2);
    for (let n = 0; n < 2; n++) out.push(prev[n] ? T.dim('       ' + T.clip(prev[n]!, w - 9)) : '');
    out.push('');
  });
  if (a.note) out.push(T.dim('  ' + a.note));
  return pad(out, h - 1).concat(hint(a.focus === 'list' ? '↑ ↓ pick · enter open · esc clear · ← → views' : 'enter search · ↓ pick · esc clear · ← → views'));
}

function readerView(a: Arch, w: number, h: number, pal: T.Pal): string[] {
  const cur = a.trail[a.at]!;
  const { lines, links } = layout(cur, w, cur.focus);
  const rowsHigh = bodyRows(h), top = Math.min(cur.scroll, Math.max(0, lines.length - rowsHigh));
  const where = `${cur.path}`;
  const tags = [cur.collection, cur.trust, cur.stale ? 'changed since indexed' : ''].filter(Boolean).join(' · ');
  const f = cur.focus === null ? null : links.find((l) => l.id === cur.focus);
  const body = lines.slice(top, top + rowsHigh).map((l) => '  ' + l);
  const pct = lines.length <= rowsHigh ? 100 : Math.round(((top + rowsHigh) / lines.length) * 100);
  const status = f ? `${describe(f.target)}${f.target.k === 'external' ? '  (enter opens it in your browser)' : ''}` : links.length ? `${links.length} link${links.length === 1 ? '' : 's'} · tab to focus` : 'no links in this document';
  return [
    T.spread(' ' + chalk.bold(where), T.dim(tags + ' '), w),
    T.hairline(pal, w),
    ...pad(body, rowsHigh),
    T.hairline(pal, w),
    T.spread(' ' + T.dim(status), T.dim(`${a.note ? a.note + ' · ' : ''}${a.trail.length > 1 ? `${a.at + 1}/${a.trail.length} · ` : ''}${pct}% `), w),
  ].concat(hint('tab link · enter open · [ back · ] forward · y copy · esc close'));
}

/** The pane's body: `h` rows of at most `w` columns. */
export function view(a: Arch, w: number, h: number, pal: T.Pal): string[] {
  const lines = a.at >= 0 ? readerView(a, w, h, pal) : a.searched !== null ? results(a, w, h) : home(a, w, h);
  return pad(lines, h).map((l) => T.clip(l, w));
}

export type { Link };
