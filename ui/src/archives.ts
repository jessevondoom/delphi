// The knowledge window: the narrator's `archives` pane (specs 028 and 029). A search box, the shape of the store under
// it, results in relevance order, and an inline reader whose links are followed like hyperlinks. Pure: state and key
// events in, state and an action out; the observer does the calls (`kb.get`) and the window draws from `view`.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import { copyText, describe, firstVisible, stepFocus, type Link, type Target } from './links.ts';
import { renderMarkdownLinks } from './markdown.ts';
import type { Raster } from './image.ts';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';

export type Hit = { path: string; collection: string; /** Where it lives, as a label (specs/043 §4): research, docs, spec, lesson, mission, skill, memory, other. */ kind: string; /** When the document last changed (ISO), or ''. */ updated: string; trust: string; status?: string; heading: string; preview: string; start_line?: number; end_line?: number };
export type Shelf = {
  /** The store counted by kind (`/v1/kinds`). */
  kinds: { kind: string; documents: number; updated: string }[];
  stats: { documents: number; passages: number; vectors: number } | null;
};
/** One document in the reader, with where the operator is in it. */
export type Entry = { path: string; collection: string; trust: string; stale: boolean; text: string; scroll: number; focus: number | null; /** The pictures fetched for it, by the file they were written as; null where one could not be drawn. */ images?: Record<string, Raster | null>; /** The lines a pointer cited, when the document was opened by one. */ range?: { from: number; to: number } };
/** One entry the research job wrote (`delphi-engine/src/research.rs`). */
export type Made = { path: string; title: string; publisher: string; url: string; form: string; licence: string };
export type Report = { new: Made[]; updated: Made[]; unchanged: number; notKept: [string, string][]; unlisted: string[]; provider: string | null; tokens: number; stopped: string | null };
/** The research job as the pane shows it (spec 028-A1 §10.6): progress while it runs, the report when it ends. */
/** One page of the job: queued, being read, written (with its form and licence), not readable, or read and not kept (specs/043 §3.7). */
export type Source = { url: string; host: string; status: 'queued' | 'reading' | 'done' | 'skipped' | 'not kept'; form: string | null; licence: string | null; why: string | null };
export type Research = { topic: string; phase: 'starting' | 'searching' | 'reading' | 'done' | 'failed' | 'cancelled'; found: number; read: number; skipped: number; hosts: string[]; sources: Source[]; tokens: number; error: string | null; report: Report | null; sel: number };
export const researchRunning = (r: Research | null): boolean => !!r && (r.phase === 'starting' || r.phase === 'searching' || r.phase === 'reading');

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
  /** Show only this kind (tab walks the chips; specs/043 §3.2); null is all of them. */
  kind: string | null;
  /** The number of results of each kind among the candidates of the last search (the chips), or null. */
  facets: Record<string, number> | null;
  /** Hide documents whose provenance is untrusted or inactive (ctrl+t). They show, tagged, by default: this is a person reading. */
  trustedOnly: boolean;
  /** The queries searched in this window, oldest first, and which one up and down have recalled (null: none). */
  history: string[];
  hist: number | null;
  /** The console can pin a passage to the operator's notebook (`p`); the narrator is read-only and cannot. */
  pins: boolean;
  /** `search` is the page as it was (local, zero-token); `expand` is the clean research page of spec 028-A1 (`shift+tab` switches). Remembered for the session. */
  mode: 'search' | 'expand';
  /** The research job started from the expand page, running or finished; null on the clean page. */
  research: Research | null;
};

/** How many results one search asks for. */
export const RESULTS = 30;

export const emptyArch = (pins = false): Arch => ({ q: '', focus: 'box', sel: 0, searched: null, hits: [], took: null, engine: null, browse: null, note: '', shelf: null, trail: [], at: -1, kind: null, facets: null, trustedOnly: false, history: [], hist: null, pins, mode: 'search', research: null });

/** What every request to the daemon adds: the trust filter. */
export const trustParam = (a: Arch): string => (a.trustedOnly ? '&trusted_only=1' : '');

export type Action =
  | { t: 'none' }
  | { t: 'search'; q: string }
  | { t: 'reload' }
  | { t: 'browse'; name: string }
  | { t: 'research'; topic: string }
  | { t: 'research-cancel' }
  | { t: 'open'; target: Extract<Target, { k: 'doc' | 'pointer' }> }
  | { t: 'browser'; url: string }
  | { t: 'copy'; text: string }
  | { t: 'pin'; target: Extract<Target, { k: 'doc' | 'pointer' }> };

type Row = { k: 'hit'; hit: Hit } | { k: 'kind'; kind: string; documents: number; updated: string };

/** What the list under the box holds: the results, or (at rest) the kinds of document the store holds. */
export function rows(a: Arch): Row[] {
  if (a.searched !== null) return a.hits.map((hit) => ({ k: 'hit', hit }));
  return a.shelf ? a.shelf.kinds.map((c) => ({ k: 'kind' as const, ...c })) : [];
}

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

const STOP = new Set(['a', 'an', 'and', 'are', 'as', 'at', 'be', 'by', 'can', 'do', 'does', 'for', 'from', 'how', 'in', 'is', 'it', 'of', 'on', 'or', 'the', 'to', 'was', 'what', 'when', 'where', 'which', 'who', 'why', 'with']);
/** The words of a query worth marking in a result, cut back a little so `escalate` also marks `escalates` and `escalating`. */
export function terms(q: string): string[] {
  const stem = (t: string) => (t.length > 6 ? t.replace(/(ing|ed|es|ly|s)$/, '') : t.length > 4 ? t.replace(/s$/, '') : t);
  return [...new Set(q.toLowerCase().split(/[^\p{L}\p{N}_-]+/u).filter((t) => t.length >= 2 && !STOP.has(t)).map(stem))];
}

/** `text` with the words of `q` in bold and the rest in `base`. */
export function emphasize(text: string, q: string, base: (s: string) => string = (s) => s): string {
  const ts = terms(q);
  if (!ts.length || !text) return base(text);
  const re = new RegExp(`(?<![\\p{L}\\p{N}_])(?:${ts.map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')})[\\p{L}\\p{N}_]*`, 'giu');
  let out = '', last = 0;
  for (const m of text.matchAll(re)) {
    out += (m.index! > last ? base(text.slice(last, m.index)) : '') + chalk.bold(m[0]);
    last = m.index! + m[0].length;
  }
  return out + (last < text.length ? base(text.slice(last)) : '');
}

const UNTRUSTED = new Set(['external_untrusted', 'federated_peer']);
/** What a result says about its provenance when it is not the usual: ` · untrusted`, ` · inactive`. */
export const caveat = (h: { trust: string; status?: string }): string => [UNTRUSTED.has(h.trust) ? 'untrusted' : '', h.status && h.status !== 'active' ? 'inactive' : ''].filter(Boolean).join(' · ');
const slug = (s: string) => s.toLowerCase().replace(/[^\w\s-]/g, '').trim().replace(/\s+/g, '-');
const day = (iso: string) => (iso || '').slice(0, 10);
const READER_MARGIN = 4;

/** The reader's body: the document rendered at `w`, and the links in it (focus is drawn by the caller's `focus`). */
export const layout = (e: Entry, w: number, focus: number | null = null) => renderMarkdownLinks(e.text, Math.max(10, w - READER_MARGIN), { base: e.path, focus, images: e.images });
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

const asHit = (r: any): Hit => ({ path: String(r.path), collection: String(r.collection ?? ''), kind: String(r.kind ?? 'other'), updated: String(r.updated ?? ''), trust: String(r.trust ?? ''), status: r.status ? String(r.status) : undefined, heading: String(r.heading ?? ''), preview: sanitize(String(r.preview ?? '')), start_line: r.start_line, end_line: r.end_line });

export function gotHits(a: Arch, q: string, body: any): Arch {
  if (a.q.trim() !== q) return a; // the box has moved on
  return { ...a, searched: q, hits: (body.results ?? []).map(asHit), sel: 0, focus: (body.results ?? []).length ? 'list' : 'box', took: body.took_ms ?? null, engine: body.mode ?? null, browse: null, note: '', facets: body.facets && typeof body.facets === 'object' ? Object.fromEntries(Object.entries(body.facets).map(([k, n]) => [String(k), Number(n)])) : a.facets };
}

export function gotBrowse(a: Arch, name: string, docs: any[]): Arch {
  return { ...a, searched: '', browse: name, hits: docs.map((d) => ({ path: d.rel_path, collection: d.zone, trust: d.source_type, status: d.status, kind: String(d.kind ?? 'other'), updated: String(d.updated_at ?? ''), heading: '', preview: day(d.updated_at) })), sel: 0, focus: 'list', took: null, engine: null, note: '' };
}

export function gotShelf(a: Arch, kinds: any[], health: any): Arch {
  const shelf: Shelf = {
    kinds: kinds.map((c) => ({ kind: String(c.kind), documents: Number(c.documents), updated: String(c.updated ?? '') })),
    stats: health?.ok ? { documents: health.documents, passages: health.passages, vectors: health.vectors } : null,
  };
  return { ...a, shelf };
}

/** A document arrived: show it, after the entry now on screen (anything forward of it in the history is dropped). */
export function gotDoc(a: Arch, body: any, target: Target | null, w: number): Arch {
  const entry: Entry = { path: String(body.path), collection: String(body.collection ?? ''), trust: String(body.trust ?? ''), stale: !!body.stale, text: String(body.text ?? ''), scroll: 0, focus: null, ...(target?.k === 'pointer' ? { range: { from: target.from, to: target.to } } : {}) };
  entry.scroll = startRow(entry, w, target);
  const trail = [...a.trail.slice(0, a.at + 1), entry];
  return { ...a, trail, at: trail.length - 1, note: '' };
}

/** A picture arrived (or could not be had) for the document `path`: every copy of it in the history is told. */
export const gotImage = (a: Arch, path: string, src: string, r: Raster | null): Arch => ({ ...a, trail: a.trail.map((x) => (x.path === path ? { ...x, images: { ...x.images, [src]: r } } : x)) });

/** The most pictures the reader fetches for one document. */
export const MAX_IMAGES = 8;

const asMade = (m: any): Made => ({ path: String(m.path), title: sanitize(String(m.title ?? m.path)), publisher: sanitize(String(m.publisher ?? '')), url: String(m.url ?? ''), form: String(m.form ?? ''), licence: String(m.licence ?? '') });

const STATUSES = ['queued', 'reading', 'done', 'skipped', 'not kept'] as const;
const asSource = (l: any): Source => ({ url: String(l.url ?? ''), host: sanitize(String(l.host ?? '')), status: STATUSES.find((x) => x === l.status) ?? 'queued', form: l.form ? String(l.form) : null, licence: l.licence ? sanitize(String(l.licence)) : null, why: l.why ? sanitize(String(l.why)) : null });

/** The daemon's `research.status` as the pane's state. A status for another topic than the one on screen is ignored. */
export function gotResearch(a: Arch, body: any): Arch {
  const r = a.research;
  if (!r || String(body?.topic ?? '') !== r.topic) return a;
  const rep = body.report;
  const report: Report | null = rep ? { new: (rep.new ?? []).map(asMade), updated: (rep.updated ?? []).map(asMade), unchanged: Number(rep.unchanged ?? 0), notKept: (rep.not_kept ?? []).map((p: any) => [String(p[0]), sanitize(String(p[1]))]), unlisted: (rep.unlisted_hosts ?? []).map(String), provider: rep.provider ?? null, tokens: Number(rep.tokens ?? 0), stopped: rep.stopped ?? null } : null;
  const phase = (['searching', 'reading', 'done', 'failed', 'cancelled'] as const).find((p) => p === body.phase) ?? r.phase;
  return { ...a, research: { ...r, phase, found: Number(body.found ?? 0), read: Number(body.read ?? 0), skipped: Number(body.skipped ?? 0), hosts: (body.hosts ?? []).map(String), sources: (body.sources ?? []).map(asSource), tokens: Number(body.tokens ?? 0), error: body.error ? sanitize(String(body.error)) : null, report } };
}

/** The job could not be started (no search engine configured, one already running): the page goes back to the box with the reason. */
export const researchRefused = (a: Arch, why: string): Arch => ({ ...a, research: null, note: sanitize(why) });

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
  if (a.mode === 'expand') return expandKey(a, e, used);
  if (e.t === 'text' || e.t === 'paste') {
    const s = e.t === 'paste' ? e.s.replace(/\s+/g, ' ') : e.s;
    if (/[\x00-\x1f\x7f]/.test(s)) return NOT_USED(a);
    return used({ ...a, focus: 'box', q: (a.q + s).slice(0, 200), note: '', hist: null });
  }
  if (e.t === 'ctrl' && e.c === 'u') return used({ ...a, q: '', focus: 'box', hist: null });
  if (e.t === 'ctrl' && e.c === 't') return used({ ...a, trustedOnly: !a.trustedOnly, note: a.trustedOnly ? 'showing every document, tagged by provenance' : 'trusted documents only' }, { t: 'reload' });
  if (e.t === 'wheel') return used({ ...a, sel: Math.max(0, Math.min(list.length - 1, a.sel + (e.dir === 'up' ? -1 : 1))), focus: list.length ? 'list' : 'box' });
  if (e.t !== 'key' || e.alt || e.ctrl) return NOT_USED(a);
  switch (e.k) {
    case 'backspace': return used({ ...a, focus: 'box', q: [...a.q].slice(0, -1).join(''), note: '', hist: null });
    case 'enter': {
      if (a.focus === 'list' && list[a.sel]) return open(a, list[a.sel]!, used);
      const q = a.q.trim();
      const history = q && a.history[a.history.length - 1] !== q ? [...a.history, q].slice(-50) : a.history;
      return q ? used({ ...a, note: 'searching…', history, hist: null }, { t: 'search', q }) : used(a);
    }
    case 'down':
      if (a.focus === 'box' && a.hist !== null) return used(a.hist + 1 < a.history.length ? { ...a, hist: a.hist + 1, q: a.history[a.hist + 1]! } : { ...a, hist: null, q: '' }); // newer, then the empty box again
      return list.length ? used({ ...a, focus: 'list', sel: a.focus === 'list' ? Math.min(list.length - 1, a.sel + 1) : 0 }) : used(a);
    case 'up':
      if (a.focus === 'box') { // older queries, from an empty box or while recalling
        if (!a.history.length || (a.q !== '' && a.hist === null)) return used(a);
        const hist = a.hist === null ? a.history.length - 1 : Math.max(0, a.hist - 1);
        return used({ ...a, hist, q: a.history[hist]! });
      }
      return a.sel > 0 ? used({ ...a, sel: a.sel - 1 }) : used({ ...a, focus: 'box' });
    case 'pgdn': return list.length ? used({ ...a, focus: 'list', sel: Math.min(list.length - 1, a.sel + 5) }) : used(a);
    case 'pgup': return used({ ...a, sel: Math.max(0, a.sel - 5) });
    case 'shift-tab': return used({ ...a, mode: 'expand', focus: 'box', note: '' });
    case 'tab': {
      // The kind chips: all, then each kind that has results (or, at rest, each kind the store holds), in turn.
      if (a.searched === null) return list.length ? used({ ...a, focus: 'list', sel: a.focus === 'list' ? (a.sel + 1) % list.length : 0 }) : used(a);
      const kinds: (string | null)[] = [null, ...chipKinds(a)];
      const kind = kinds[(Math.max(0, kinds.indexOf(a.kind)) + 1) % kinds.length] ?? null;
      const q = a.q.trim();
      const next = { ...a, kind };
      return !a.browse && q ? used({ ...next, note: 'searching…', sel: 0 }, { t: 'search', q }) : used(next);
    }
    case 'esc':
      if (a.searched !== null) return used({ ...a, searched: null, hits: [], browse: null, note: '', sel: 0, focus: 'box', took: null });
      return a.q ? used({ ...a, q: '' }) : NOT_USED(a);
    default: return NOT_USED(a); // left and right change the view
  }
}

/** The entries of a finished report, in the order the page lists them: new first, then updated. */
export const madeOf = (r: Research | null): Made[] => (r?.report ? [...r.report.new, ...r.report.updated] : []);

/** Expand mode (spec 028-A1): a box and a topic; with a job, its progress and then its results. */
function expandKey(a: Arch, e: Ev, used: (n: Arch, act?: Action) => ReturnType<typeof key>) {
  const r = a.research;
  if (r) {
    if (e.t === 'wheel') return used(r.phase === 'done' || r.phase === 'cancelled' ? { ...a, research: { ...r, sel: Math.max(0, Math.min(madeOf(r).length - 1, r.sel + (e.dir === 'up' ? -1 : 1))) } } : a);
    if (e.t !== 'key' || e.alt || e.ctrl) return e.t === 'ctrl' && e.c === 'c' ? NOT_USED(a) : used(a); // a stray letter changes nothing while a job is on screen
    const made = madeOf(r);
    switch (e.k) {
      case 'shift-tab': return used({ ...a, mode: 'search', focus: 'box', note: '' }, a.shelf ? none : { t: 'reload' });
      case 'esc':
        if (researchRunning(r)) return used({ ...a, note: 'cancelling…' }, { t: 'research-cancel' });
        return used({ ...a, research: null, q: '', note: '' });
      case 'down': return used({ ...a, research: { ...r, sel: Math.min(Math.max(0, made.length - 1), r.sel + 1) } });
      case 'up': return used({ ...a, research: { ...r, sel: Math.max(0, r.sel - 1) } });
      case 'enter': return made[r.sel] ? used({ ...a, note: 'opening…' }, { t: 'open', target: { k: 'doc', path: made[r.sel]!.path } }) : used(a);
      case 'left': case 'right': return NOT_USED(a); // the view keys still work
      default: return used(a);
    }
  }
  if (e.t === 'text' || e.t === 'paste') {
    const s = e.t === 'paste' ? e.s.replace(/\s+/g, ' ') : e.s;
    if (/[\x00-\x1f\x7f]/.test(s)) return NOT_USED(a);
    return used({ ...a, q: (a.q + s).slice(0, 200), note: '' });
  }
  if (e.t === 'ctrl' && e.c === 'u') return used({ ...a, q: '' });
  if (e.t !== 'key' || e.alt || e.ctrl) return NOT_USED(a);
  switch (e.k) {
    case 'shift-tab': return used({ ...a, mode: 'search', focus: 'box', note: '' }, a.shelf ? none : { t: 'reload' });
    case 'backspace': return used({ ...a, q: [...a.q].slice(0, -1).join(''), note: '' });
    case 'enter': {
      const topic = a.q.trim();
      if (!topic) return used({ ...a, note: '' });
      if (!a.pins) return used({ ...a, note: 'research runs from the console: this window only reads' }); // the narrator is read-only
      return used({ ...a, note: '', research: { topic, phase: 'starting', found: 0, read: 0, skipped: 0, hosts: [], sources: [], tokens: 0, error: null, report: null, sel: 0 } }, { t: 'research', topic });
    }
    case 'tab': case 'up': case 'down': case 'pgup': case 'pgdn': return used(a); // no scope, history or list here
    case 'esc': return a.q ? used({ ...a, q: '', note: '' }) : NOT_USED(a);
    default: return NOT_USED(a); // left and right change the view
  }
}

function open(a: Arch, r: Row, used: (n: Arch, act?: Action) => ReturnType<typeof key>) {
  if (r.k === 'kind') return used({ ...a, note: 'opening…' }, { t: 'browse', name: r.kind });
  const path = r.hit.path;
  const target: Target = r.hit.start_line ? { k: 'pointer', path, from: r.hit.start_line, to: r.hit.end_line ?? r.hit.start_line } : { k: 'doc', path };
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
    if (e.s === 'p' && a.pins) return used({ ...a, note: 'pinning…' }, { t: 'pin', target: cur.range ? { k: 'pointer', path: cur.path, from: cur.range.from, to: cur.range.to } : { k: 'doc', path: cur.path } });
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

function box(a: Arch, w: number, placeholder = 'search what Delphi knows…'): string[] {
  const cursor = a.focus === 'box' ? chalk.inverse(' ') : '';
  const inner = Math.min(58, Math.max(10, w - 8));
  const q = [...a.q].slice(-(inner - 6)).join('');
  if (w < 40) return [' ' + T.accent('›') + ' ' + (q || T.dim('search')) + cursor, T.dim('─'.repeat(Math.max(0, w - 2)).padStart(w - 1))];
  const body = ` ${T.accent('›')}  ${q || (a.focus === 'box' ? '' : '')}${q ? '' : T.dim(placeholder)}${cursor}`;
  const fill = ' '.repeat(Math.max(0, inner - 1 - T.width(body)));
  return [center('╭' + '─'.repeat(inner) + '╮', w), center('│' + body + fill + ' │', w), center('╰' + '─'.repeat(inner) + '╯', w)];
}

/** What narrows the search, when anything does. There are no offline/online indicators (spec 028-A1 §10.1): this page always searches the local store. */
const narrowLine = (a: Arch, w: number) => (a.trustedOnly ? center(T.dim('trusted only'), w) : '');

/** A role's badge, as the roster draws it, or its plain face when the console's roster is not at hand. */
const roleBadge = (role: string, face: string, info: T.Info | undefined, pal: T.Pal) => (info ? T.badge(role, 'idle', info, pal) : T.dim(`⟦${face}⟧`));
const titleRow = (w: number, info: T.Info | undefined, pal: T.Pal) => center(`${roleBadge('orchestrator', '•_•', info, pal)} ${chalk.bold('search the archives')}`, w);
const SWITCH = { search: 'shift+tab  expand', expand: 'shift+tab  search' } as const;

// ── kinds, the best match and the result list (specs/043) ───────────────────

export const KIND_ORDER = ['research', 'docs', 'spec', 'lesson', 'mission', 'skill', 'memory', 'other'];
const KIND_LABEL: Record<string, string> = { research: 'research', docs: 'docs', spec: 'specs', lesson: 'lessons', mission: 'missions', skill: 'skills', memory: 'memory', other: 'other' };
const kindStyle = (k: string): ((s: string) => string) => (k === 'research' ? T.warn : k === 'docs' ? T.ok : k === 'spec' ? T.accent : k === 'lesson' ? chalk.magenta : k === 'mission' ? chalk.blue : T.dim);

/** The kinds that have results, in the spec's order: the chips after `all`. */
export function chipKinds(a: Arch): string[] {
  const f = a.facets ?? {};
  const known = KIND_ORDER.filter((k) => (f[k] ?? 0) > 0);
  return [...known, ...Object.keys(f).filter((k) => !KIND_ORDER.includes(k) && f[k]! > 0).sort()];
}

/** How long ago, as the list shows it: `today`, `3 d`, `2 w`, `4 mo`. */
export function ago(iso: string, now: number): string {
  const t = Date.parse(iso);
  if (!t) return '';
  const d = Math.max(0, now - t) / 86_400_000;
  return d < 1 ? 'today' : d < 14 ? `${Math.round(d)} d` : d < 70 ? `${Math.round(d / 7)} w` : `${Math.round(d / 30)} mo`;
}

/** `trusted`, `listed` (from an allow-listed source), `untrusted` or `inactive` (specs/043 §4.3). */
export const trustWord = (h: { trust: string; status?: string }): string => caveat(h).split(' · ')[0] || (h.trust === 'trusted_source' ? 'listed' : 'trusted');

/**
 * The best-match summary (specs/043 §3.2): the passage's own words, trimmed by code. Up to three sentences that hold the
 * most words of the query, in document order, at most 300 characters. No model is called, and nothing is paraphrased.
 */
export function bestSummary(preview: string, q: string): string {
  const text = sanitize(preview).split('\n').filter((l) => !/^#+\s/.test(l)).map((l) => l.trim()).filter(Boolean).join(' ');
  const sentences = text.split(/(?<=[.!?])\s+/).filter(Boolean);
  if (!sentences.length) return '';
  const ts = terms(q);
  const score = (s: string) => ts.filter((t) => s.toLowerCase().includes(t)).length;
  const picked = sentences.map((s, i) => ({ s, i, n: score(s) })).sort((x, y) => y.n - x.n || x.i - y.i).slice(0, 3).sort((x, y) => x.i - y.i).map((x) => x.s);
  const out = picked.join(' ');
  return out.length > 300 ? out.slice(0, 299).replace(/\s+\S*$/, '') + '…' : out;
}

const wrapText = (s: string, w: number): string[] => {
  const out: string[] = [];
  let cur = '';
  for (const word of s.split(/\s+/).filter(Boolean)) {
    if (cur && T.width(cur + ' ' + word) > w) { out.push(cur); cur = word; } else cur = cur ? cur + ' ' + word : word;
  }
  return cur ? [...out, cur] : out;
};
const lastHeading = (h: string) => h.split(' > ').pop() ?? '';
const titleOf = (x: Hit) => `${x.path}${x.heading ? ' › ' + lastHeading(x.heading) : ''}`;

function mark(sel: boolean, a: Arch) { return sel && a.focus === 'list' ? T.accent('›') : ' '; }

/** The resting page (specs/043 §3.1): the title, the box, one line of counts by kind, and the totals. Nothing else. */
function home(a: Arch, w: number, h: number, info: T.Info | undefined, pal: T.Pal): string[] {
  const out: string[] = Array(Math.max(1, Math.floor((h - 14) / 3))).fill('');
  out.push(titleRow(w, info, pal), '', ...box(a, w), narrowLine(a, w), '');
  const list = rows(a);
  if (!a.shelf) out.push(T.dim(center('reading the index…', w)));
  else if (!list.length) out.push(T.dim(center('the knowledge base is empty', w)));
  else {
    const kinds = list.filter((r) => r.k === 'kind') as Extract<Row, { k: 'kind' }>[];
    const ordered = [...KIND_ORDER.flatMap((k) => kinds.filter((r) => r.kind === k)), ...kinds.filter((r) => !KIND_ORDER.includes(r.kind))];
    out.push('', center(ordered.map((r) => { const on = a.focus === 'list' && list[a.sel] === r; const t = `${KIND_LABEL[r.kind] ?? r.kind} ${r.documents}`; return on ? chalk.bold.underline(kindStyle(r.kind)(t)) : kindStyle(r.kind)(t); }).join('   '), w), '');
    const s = a.shelf.stats;
    if (s) out.push(T.dim(center(`${s.documents} documents · ${s.passages} passages · ${s.vectors > 0 ? 'hybrid (keyword + vector)' : 'keyword'} search`, w)));
  }
  if (a.note) out.push('', T.dim(center(a.note, w)));
  return pad(out, h - 1).concat(hint(a.focus === 'list' ? `↑ ↓ pick a kind · enter browse it · esc back · ${SWITCH.search} · ← → views` : `type to search · ↑ earlier searches · tab pick a kind · ctrl+t trusted only · ${SWITCH.search}`));
}

/** Expand mode (spec 028-A1 §10.1): the title, the scout's label and the box, and nothing else: no shelf, counts, recent list or totals. */
function expandView(a: Arch, w: number, h: number, info: T.Info | undefined, pal: T.Pal): string[] {
  if (a.research) return researchView(a, w, h, info, pal);
  const out: string[] = ['', titleRow(w, info, pal), '', center(`${roleBadge('scout', '•••', info, pal)} ${chalk.bold('expand knowledge base: research a topic')}`, w), '', ...box(a, w, 'a topic to research…')];
  if (a.note) out.push('', T.dim(center(a.note, w)));
  return pad(out, h - 1).concat(hint(`type a topic · enter research it · esc clear · ${SWITCH.expand}`));
}

const plural = (n: number, one: string, many: string) => (n === 1 ? `1 ${one}` : `${n} ${many}`);

/** The banner of a finished job (spec 028-A1 §10.6). */
export function banner(r: Research): string {
  const n = r.report?.new.length ?? 0;
  return n === 0 ? 'no new entries: the archive already covers this' : `the archive has been expanded to include ${plural(n, 'new entry', 'new entries')}`;
}

const FORM_WORD: Record<string, string> = { full: 'full text', summary: 'summary and facts' };
/** The licence as the page says it: unknown licences are summary-only sources. */
const licenceWord = (licence: string | null, form: string | null) => (!licence || licence === 'unknown' ? (form === 'summary' ? 'summary-only source' : 'licence unknown') : licence);

/** The clean page's top, kept while a job runs and after it: the title, the scout's label and the box holding the topic (specs/043 §3.7). */
function expandTop(a: Arch, w: number, info: T.Info | undefined, pal: T.Pal, topic: string): string[] {
  return ['', titleRow(w, info, pal), '', center(`${roleBadge('scout', '•••', info, pal)} ${chalk.bold('expand knowledge base: research a topic')}`, w), '', ...box({ ...a, q: topic, focus: 'list' }, w, 'a topic to research…'), ''];
}

/** What a skipped or not-kept page looked like, short: `britannica.com/…`. */
const shortUrl = (s: Source) => `${s.host}${/^https?:\/\/[^/]+\/./.test(s.url) ? '/…' : ''}`;

/** The job's page (specs/043 §3.7, spec 028-A1 §10.6): a live count and one line per source while it runs, then the banner and the entries it wrote. */
function researchView(a: Arch, w: number, h: number, info: T.Info | undefined, pal: T.Pal): string[] {
  const r = a.research!;
  const out = expandTop(a, w, info, pal, r.topic);
  const ind = '  ';
  if (researchRunning(r)) {
    out.push(ind + chalk.bold(`researching “${r.topic}”`));
    out.push(ind + (r.phase === 'reading' ? `reading ${chalk.bold(String(r.read))} ${r.read === 1 ? 'source' : 'sources'} · ${chalk.bold(String(r.found))} found · ${T.dim(`${r.skipped} skipped`)}` : T.dim('searching the trusted sources…')), '');
    const lines = r.sources.filter((x) => x.status !== 'queued');
    const queued = r.sources.filter((x) => x.status === 'queued');
    const room = Math.max(2, h - out.length - 4);
    const shown = lines.slice(Math.max(0, lines.length - (queued.length ? room - 1 : room)));
    for (const x of shown) {
      const mark = x.status === 'done' ? T.ok('✔') : x.status === 'reading' ? T.accent('●') : x.status === 'skipped' ? T.alert('✖') : T.dim('–');
      const what = x.status === 'done' ? `${FORM_WORD[x.form ?? ''] ?? x.form ?? ''}`.padEnd(19) + T.dim(licenceWord(x.licence, x.form)) : x.status === 'reading' ? 'reading…' : T.dim(`${x.status}: ${x.why ?? ''}`);
      out.push(T.clip(`${ind}${mark} ${T.warn(x.host.padEnd(24))} ${what}`, w));
    }
    if (queued.length) out.push(T.dim(T.clip(`${ind}○ ${queued.map((x) => x.host).join(' · ')}`, w)));
    if (a.note) out.push('', T.dim(ind + a.note));
    return pad(out, h - 1).concat(hint('esc cancel · entries already written stay · nothing leaves the trusted list'));
  }
  if (r.phase === 'failed' || !r.report) {
    out.push(ind + T.alert(`research failed: ${r.error ?? 'no answer from the daemon'}`));
    return pad(out, h - 1).concat(hint(`esc back · ${SWITCH.expand}`));
  }
  const rep = r.report, made = madeOf(r);
  out.push(ind + chalk.bold(T.ok(banner(r))));
  const extra: string[] = [];
  if (rep.updated.length) extra.push(`${rep.updated.length} updated`);
  if (rep.unchanged) extra.push(`${rep.unchanged} already there`);
  if (rep.stopped) extra.push(`stopped: ${rep.stopped}`);
  if (extra.length) out.push(T.dim(ind + extra.join(' · ')));
  out.push('');
  if (made.length) {
    const pw = Math.max(20, Math.min(Math.max(...made.map((m) => m.path.length)), w - 38));
    out.push(T.dim(`${ind}   #  ${'new entry'.padEnd(pw)}  ${'form'.padEnd(8)} licence`));
    const room = Math.max(1, h - out.length - 6);
    const top = Math.max(0, Math.min(r.sel - room + 1, made.length - room));
    made.slice(top, top + room).forEach((m, k) => {
      const i = top + k, on = i === r.sel;
      const path = T.clip(m.path, pw).padEnd(pw);
      out.push(T.clip(`${ind}${on ? T.accent('❯') : ' '} ${String(i + 1).padStart(2)}  ${on ? chalk.bold(path) : path}  ${T.warn(m.form.padEnd(8))} ${T.dim(licenceWord(m.licence, m.form))}`, w));
    });
  }
  const pages = r.sources.filter((x) => x.status === 'not kept' || x.status === 'skipped');
  out.push('');
  if (pages.length) out.push(T.dim(T.clip(`${ind}read, not kept: ${pages.map((x) => `${shortUrl(x)} (${x.why ?? x.status})`).join(' · ')}`, w)));
  if (rep.unlisted.length) out.push(T.dim(T.clip(`${ind}seen, not read: ${rep.unlisted.length} ${rep.unlisted.length === 1 ? 'host that is' : 'hosts that are'} not on your lists (${rep.unlisted.slice(0, 4).join(', ')}${rep.unlisted.length > 4 ? ', …' : ''})`, w)));
  if (a.note) out.push(T.dim(ind + a.note));
  return pad(out, h - 1).concat(hint(`${made.length ? '↑ ↓ pick · enter read it · ' : ''}esc back to the clean page · they are searchable now`));
}

/** One result of the list (specs/043 §5): a title row in fixed columns and one line of the passage. */
function listRow(a: Arch, x: Hit, i: number, w: number, now: number): string[] {
  const on = i === a.sel && a.focus === 'list';
  const mid = w >= 90, full = w >= 80;
  const tw = trustWord(x);
  const tail = kindStyle(x.kind)(x.kind.padEnd(8)) + (full ? '  ' + (tw === 'trusted' || tw === 'listed' ? T.dim : T.warn)(tw.padEnd(9)) : tw === 'trusted' || tw === 'listed' ? '' : T.warn('!')) + (mid ? ' ' + T.dim(ago(x.updated, now).padStart(6)) : '');
  const room = Math.max(10, w - 8 - 10 - (full ? 11 : 0) - (mid ? 7 : 0) - 1);
  const title = T.clip(titleOf(x), Math.min(64, room));
  const first = sanitize(x.preview).split('\n').filter((l) => !/^#+\s/.test(l)).map((l) => l.trim()).filter(Boolean)[0] ?? '';
  const q = a.browse ? '' : a.searched ?? '';
  return [
    T.spread(` ${mark(i === a.sel, a)} ${String(i + 1).padStart(2)}  ${a.browse ? (on ? chalk.bold(title) : title) : emphasize(title, q, on ? chalk.bold : (z) => z)}`, tail + ' ', w),
    first ? '       ' + emphasize(T.clip(first, w - 9), q, T.dim) : '',
    '',
  ];
}

/** The best match (specs/043 §3.2): a soft-fill card with the passage's own words, trimmed by code. */
function bestCard(a: Arch, x: Hit, w: number, h: number, now: number, pal: T.Pal): string[] {
  const t = T.tint(pal.bg ?? null, pal.dark);
  const fill = (s: string) => { const row = ' ' + s + ' '.repeat(Math.max(0, w - 7 - T.width(s))) + ' '; return '  ' + (t ? t.fill(row) : row); };
  const on = a.sel === 0 && a.focus === 'list';
  const edited = ago(x.updated, now);
  const compact = h < 30;
  const sum = wrapText(bestSummary(x.preview, a.searched ?? ''), w - 11).slice(0, compact ? 2 : 4);
  const L = [
    T.spread(chalk.bold(T.dim('BEST MATCH')), kindStyle(x.kind)(x.kind) + T.dim(` · ${trustWord(x)}${edited ? ` · edited ${edited}` : ''}`), w - 7),
    (on ? T.accent('❯ ') : '') + chalk.bold(T.accent(T.clip(titleOf(x), w - 11))),
    ...sum.map((l) => emphasize(l, a.searched ?? '')),
  ];
  if (!compact) L.push('', T.dim(x.start_line ? `lines ${x.start_line}–${x.end_line ?? x.start_line}` : x.collection));
  L.push(T.accent('enter') + ' read');
  return L.map(fill);
}

/** A search that found fewer than 3 reference documents (kinds `research` or `docs`) offers to research the topic (specs/043 §8.2). Only a window that can run research offers it. */
export function thinOnReference(a: Arch): boolean {
  if (!a.pins || a.browse || a.searched === null) return false;
  const n = a.facets ? (a.facets.research ?? 0) + (a.facets.docs ?? 0) : a.hits.filter((x) => x.kind === 'research' || x.kind === 'docs').length;
  return n < 3;
}

function results(a: Arch, w: number, h: number, now: number, pal: T.Pal): string[] {
  const out: string[] = [...box(a, w).slice(w < 40 ? 0 : 1, w < 40 ? 2 : 2)];
  const where = `${a.kind ? ` · ${KIND_LABEL[a.kind] ?? a.kind}` : ''}${a.trustedOnly ? ' · trusted only' : ''}`;
  const head = a.browse
    ? `${a.hits.length} documents · ${KIND_LABEL[a.browse] ?? a.browse}, newest first${a.trustedOnly ? ' · trusted only' : ''}`
    : a.hits.length ? `${a.hits.length >= RESULTS ? 'top ' : ''}${a.hits.length} result${a.hits.length === 1 ? '' : 's'}${a.took !== null ? ` · ${(a.took / 1000).toFixed(2)} s` : ''} · 0 tokens${where}`
      : `nothing in the knowledge base matches${where}`;
  out.push(T.dim('  ' + head + (thinOnReference(a) ? ' · thin on reference: shift+tab to research this topic' : '')));
  if (!a.browse && a.facets && Object.keys(a.facets).length) {
    const total = Object.values(a.facets).reduce((s, n) => s + n, 0);
    const chip = (k: string | null, label: string, n: number) => { const on = a.kind === k; const t = `${on ? '●' : '○'} ${label} ${n}`; return k === null ? (on ? chalk.bold(t) : T.dim(t)) : on ? chalk.bold(kindStyle(k)(t)) : kindStyle(k)(t); };
    out.push('  ' + T.dim('show  ') + [chip(null, 'all', total), ...chipKinds(a).map((k) => chip(k, KIND_LABEL[k] ?? k, a.facets![k]!))].join('   '));
  }
  out.push('');
  const useCard = !a.browse && a.hits.length > 0;
  if (useCard) out.push(...bestCard(a, a.hits[0]!, w, h, now, pal), '');
  if (a.hits.length > (useCard ? 1 : 0)) out.push(T.dim(`   #  title${' '.repeat(Math.max(1, w - 46))}kind${w >= 80 ? '      trust' : ''}${w >= 90 ? '    edited' : ''}`));
  const start = useCard ? 1 : 0, per = 3;
  const room = Math.max(1, Math.floor((h - out.length - 2) / per));
  const top = Math.max(start, Math.min(a.sel - room + 1, a.hits.length - room), a.sel >= start + room ? a.sel - room + 1 : start);
  a.hits.slice(top, top + room).forEach((x, k) => out.push(...listRow(a, x, top + k, w, now)));
  const rest = a.hits.length - (top + room);
  if (rest > 0) out.push(T.dim(`   + ${rest} more below`));
  if (a.note) out.push(T.dim('  ' + a.note));
  return pad(out, h - 1).concat(hint(a.focus === 'list' ? `↑ ↓ pick · enter open · tab filter · ctrl+t trust · esc clear · ${SWITCH.search}` : `enter search · ↓ pick · ↑ history · tab filter · ctrl+t trust · esc clear · ${SWITCH.search}`));
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
  ].concat(hint(`tab link · enter open · [ back · ] forward · y copy${a.pins ? ' · p pin' : ''} · esc close`));
}

/** The pane's body: `h` rows of at most `w` columns. */
export function view(a: Arch, w: number, h: number, pal: T.Pal, info?: T.Info, now: number = Date.now()): string[] {
  const lines = a.at >= 0 ? readerView(a, w, h, pal) : a.mode === 'expand' ? expandView(a, w, h, info, pal) : a.searched !== null ? results(a, w, h, now, pal) : home(a, w, h, info, pal);
  return pad(lines, h).map((l) => T.clip(l, w));
}

export type { Link };
