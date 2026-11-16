// Search pages in the library (specs/057 SK8d): a question, the records found for it at once, and, beside them, the page the writer
// composed from them once the daemon's checks pass. Pure, like `library.ts`: RPC answers go in as parsed JSON, lines come out.
// The banner on top and the bibliography table at the bottom are the daemon's own text, shown as it wrote them; the status glyphs
// are the daemon's words (specs/052), never worked out here. Anything that moves takes its answer from `motion.ts`.
import chalk from 'chalk';
import type { Ev } from './input.ts';
import { spinner } from './motion.ts';
import * as T from './theme.ts';
import { sanitize } from './theme.ts';
import type { Raster } from './image.ts';
import { renderMarkdownLinks } from './markdown.ts';
import { reverseSpan } from './links.ts';

export type PHit = { kind: 'claim' | 'source'; id: string; title: string; status: T.KnowledgeStatus; private: boolean };
export type PEntry = { key: string; query: string; pinned: boolean; outcome: string; stale: string; external: boolean; failed: boolean; created: number };
export type Statement = { text: string; ids: string[] };
export type Row = string[];
export type Open = {
  key: string;
  query: string;
  outcome: string;
  pinned: boolean;
  stale: string;
  external: boolean;
  cached: boolean;
  banner: string;
  body: Statement[];
  table: Row[];
  notes: string[];
  markdown?: string;
  title?: string;
  kind?: string;
  depth?: string;
  valid_until?: string;
  volatility?: string;
  ttl_reason?: string;
  images?: Record<string, Raster | null>;
};
export type Pages = {
  q: string; ext: boolean; focus: 'box' | 'results' | 'page' | 'note';
  hits: PHit[] | null; pages: PEntry[]; sel: number;
  /** What the console is waiting for, if anything: the instant results, or the page the writer is composing. */
  busy: 'results' | 'page' | null;
  open: Open | null; at: number; draft: string; note: string;
  /** The query whose page the operator cancelled: it is dropped if it still arrives. */
  drop: string | null;
  /** Which of the open page's links has the focus (an index into `navOf`), or none (specs/117 §7). */
  nav: number | null;
};
/** Something on a page that tab reaches and enter follows: a contents anchor, a `page:` link or a citation marker (specs/097 §4.6). */
export type Nav = { k: 'anchor'; anchor: string; label: string } | { k: 'page'; slug: string; label: string } | { k: 'cite'; ids: string[]; label: string };
export type PagesAction =
  | { t: 'none' }
  | { t: 'pages-search'; q: string; external: boolean }
  | { t: 'pages-expand'; q: string }
  | { t: 'pages-refresh'; q: string; external: boolean }
  | { t: 'pages-open'; key: string }
  | { t: 'pages-open-page'; slug: string }
  | { t: 'pages-pin'; key: string; pinned: boolean }
  | { t: 'pages-dispute'; claim: string; note: string }
  | { t: 'pages-promote'; page: string; statement: string }
  | { t: 'open-source'; id: string }
  | { t: 'open-claim'; id: string }
  | { t: 'leave' };

export const emptyPages = (): Pages => ({ q: '', ext: false, focus: 'box', hits: null, pages: [], sel: 0, busy: null, open: null, at: 0, draft: '', note: '', drop: null, nav: null });

const clean = (s: string, max: number): string => sanitize(s).slice(0, max);
const str = (v: unknown, max = 400): string => (typeof v === 'string' ? sanitize(v).slice(0, max) : '');
const list = (body: unknown): any[] => (Array.isArray(body) ? body.filter((x) => x && typeof x === 'object') : []);
const STATUSES: T.KnowledgeStatus[] = ['noted', 'checking', 'validated', 'contested', 'disproved', 'retired'];
const statusOf = (s: string): T.KnowledgeStatus => STATUSES.find((k) => s.includes(k)) ?? 'noted';

export function parseEntries(body: unknown): PEntry[] {
  return list(body).map((x) => ({ key: str(x.key, 60), query: str(x.query, 200), pinned: x.pinned === true, outcome: str(x.outcome, 20), stale: str(x.stale, 200), external: x.external === true, failed: x.failed === true, created: Number(x.created) || 0 })).filter((e) => e.key);
}

export function parseHits(body: unknown): PHit[] {
  return list(body).map((x) => ({ kind: x.kind === 'claim' ? 'claim' as const : 'source' as const, id: str(x.id, 120), title: str(x.title, 200), status: statusOf(str(x.status, 40)), private: x.private === true })).filter((h) => h.id);
}

/** The records a sentence cites: `[c-1]`, `[s-slug]`, `[c-1 ● validated]` and lists such as `[c-1, s-x]`. */
export function idsIn(text: string): string[] {
  const out: string[] = [];
  for (const m of text.matchAll(/\[([^\]]+)\]/g)) for (const seg of m[1]!.split(/[,;]/)) { const id = /^\s*([cs]-[^\s\]]+)/.exec(seg)?.[1]; if (id && !out.includes(id)) out.push(id); }
  return out;
}

/** The anchor a heading makes, by the daemon's rule (`library_page_check::slug_of`): what a page's `#section` links name. */
export function anchorOf(heading: string): string {
  let out = '';
  for (const ch of heading.toLowerCase()) out += /[\p{L}\p{N}]/u.test(ch) ? ch : ch === ' ' || ch === '-' ? '-' : '';
  return out;
}

const NAV = /\[([^\]\[]+)\]\(\s*(#[^)\s]+|page:[^)\s]+)\s*\)|\[([^\]\[]+)\](?!\()/g;
const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/**
 * What tab reaches on a page, in reading order (specs/117 §7): its contents anchors (`[Section](#section)`), its links to
 * other pages (`[Title](page:slug)`) and its citation markers (`[c-…]`, `[s-…]`). Read from the body only: the bibliography
 * table is the daemon's and holds no link. Fenced code is skipped. A `page:` target that is not a slug is not followed.
 */
export function navOf(md: string): Nav[] {
  const out: Nav[] = [];
  let fence = false;
  for (const line of md.split('\n')) {
    if (/^##\s+Bibliography and citations\s*$/.test(line)) break;
    if (/^ {0,3}(```|~~~)/.test(line)) { fence = !fence; continue; }
    if (fence) continue;
    for (const m of line.matchAll(NAV)) {
      if (m[2] !== undefined) {
        const label = clean(m[1]!, 200);
        if (m[2].startsWith('#')) out.push({ k: 'anchor', anchor: clean(m[2].slice(1), 200), label });
        else if (SLUG.test(m[2].slice(5))) out.push({ k: 'page', slug: m[2].slice(5), label });
      } else {
        const ids = idsIn(m[0]);
        // Every part of the bracket must be a record: `[c-1, see below]` is prose.
        if (ids.length && ids.length === m[3]!.split(/[,;]/).length) out.push({ k: 'cite', ids, label: clean(m[0], 200) });
      }
    }
  }
  return out;
}

/** The width a page is drawn at in a window `w` wide: beside the results when there is room, else across. */
const pageWidth = (w: number): number => (w >= 100 ? w - Math.floor(w * 0.4) - 3 : w);
/** Whether a page is drawn as Markdown (an encyclopedia page) or as its statements (an earlier search page). */
const asMarkdown = (o: Open): boolean => !!o.markdown && !!(o.kind || o.depth || o.markdown.includes('# ') || o.markdown.includes('## Contents') || o.markdown.includes('!['));
const ANSI_RE = /\x1b\[[0-9;]*m/g;

/** Where each of `navs` was drawn in `lines`, by its words, in order; null for one a wrap split across rows. */
export function navSpans(lines: string[], navs: Nav[]): ({ line: number; col: number; width: number } | null)[] {
  const plain = lines.map((l) => l.replace(ANSI_RE, ''));
  let row = 0, col = 0;
  return navs.map((n) => {
    for (let r = row; r < plain.length; r++) {
      const at = plain[r]!.indexOf(n.label, r === row ? col : 0);
      if (at >= 0) { row = r; col = at + n.label.length; return { line: r, col: T.width(plain[r]!.slice(0, at)), width: T.width(n.label) }; }
    }
    return null;
  });
}

/** What following a link will do, in the words the note line shows. */
const navWords = (n: Nav): string => (n.k === 'anchor' ? `enter goes to the section "${n.label}"` : n.k === 'page' ? `enter opens the page ${n.slug}` : n.ids[0]!.startsWith('s-') ? `enter opens the source ${n.ids[0]!.slice(2)}` : `enter opens the claim ${n.ids[0]}`);

const cells = (l: string): string[] => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => sanitize(c.trim()));

/** The page as the daemon wrote it: the banner line, the body's statements, the bibliography rows. */
export function parsePage(md: string): Pick<Open, 'banner' | 'body' | 'table'> {
  const lines = md.split('\n');
  const head = lines.findIndex((l) => /^##\s+Bibliography and citations\s*$/.test(l));
  const upto = head < 0 ? lines : lines.slice(0, head);
  const first = upto.findIndex((l) => l.trim() !== '');
  const banner = first < 0 ? '' : clean(upto[first]!.replace(/^>\s*/, ''), 600);
  const body: Statement[] = upto.slice(first + 1).map((l) => l.trim()).filter((l) => l && !/^#{1,6}\s/.test(l)).map((l) => ({ text: clean(l, 1500), ids: idsIn(l) }));
  const table = head < 0 ? [] : lines.slice(head + 1).filter((l) => l.trim().startsWith('|')).map(cells).filter((c) => !c.every((x) => /^:?-+:?$/.test(x)));
  return { banner, body, table };
}

export function parseFrontmatter(md: string): { meta: Record<string, string>; content: string } {
  const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/.exec(md);
  if (!m) return { meta: {}, content: md };
  const meta: Record<string, string> = {};
  for (const line of m[1]!.split('\n')) {
    const idx = line.indexOf(':');
    if (idx > 0) {
      const k = line.slice(0, idx).trim();
      const v = line.slice(idx + 1).trim().replace(/^['"]|['"]$/g, '');
      meta[k] = v;
    }
  }
  return { meta, content: md.slice(m[0].length) };
}

function opened(body: any, cached: boolean): Open {
  const e = parseEntries([body])[0];
  const rawMd = String(body?.markdown ?? '');
  const { meta, content } = parseFrontmatter(rawMd);
  const parsed = parsePage(content);
  return {
    key: e?.key ?? '',
    query: e?.query ?? '',
    outcome: e?.outcome ?? '',
    pinned: e?.pinned ?? false,
    stale: e?.stale ?? '',
    external: e?.external ?? false,
    cached,
    ...parsed,
    notes: Array.isArray(body?.notes) ? body.notes.map((n: unknown) => str(n, 200)) : [],
    markdown: content,
    title: meta['title'] || str(body?.title, 200),
    kind: meta['kind'] || str(body?.kind, 50),
    depth: meta['depth'] || str(body?.depth, 50),
    valid_until: meta['valid_until'] || str(body?.valid_until, 50),
    volatility: meta['volatility'] || str(body?.volatility, 50),
    ttl_reason: meta['ttl_reason'] || str(body?.ttl_reason, 200),
    images: body?.images ?? {},
  };
}

export const gotImage = (p: Pages, src: string, raster: Raster | null): Pages =>
  p.open ? { ...p, open: { ...p.open, images: { ...p.open.images, [src]: raster } } } : p;

export const gotList = (p: Pages, body: any): Pages => ({ ...p, pages: parseEntries(body?.pages ?? body) });
export const gotResults = (p: Pages, q: string, body: any): Pages => (q === p.q.trim() ? { ...p, hits: parseHits(body?.hits), pages: body?.pages ? parseEntries(body.pages) : p.pages, busy: p.busy === 'results' ? 'page' : p.busy, sel: 0 } : p);
/** Stop waiting for the page; the raw results stay. A page that still arrives for this query is dropped. */
export const cancel = (p: Pages): Pages => (p.busy ? { ...p, busy: null, drop: p.q.trim(), note: 'page cancelled' } : p);
export const building = (p: Pages, searched: string | null): boolean => p.busy === 'page' && searched !== null && searched === p.q.trim();
export const gotPage = (p: Pages, q: string, body: any): Pages => (q === p.q.trim() && p.drop === q ? { ...p, drop: null } : q === p.q.trim() ? { ...p, open: opened(body, body?.cached === true), at: 0, nav: null, busy: null, focus: 'results', note: '' } : p);
export const gotOpened = (p: Pages, body: any): Pages => ({ ...p, open: opened(body, true), at: 0, nav: null, busy: null, focus: 'page', note: '' });
export const failed = (p: Pages, why: string): Pages => ({ ...p, busy: null, note: clean(why, 300) });
export const withNote = (p: Pages, note: string): Pages => ({ ...p, note: clean(note, 300) });
export const gotPinned = (p: Pages, body: any): Pages => {
  const e = parseEntries([body])[0];
  return e ? { ...p, pages: p.pages.map((x) => (x.key === e.key ? e : x)), open: p.open?.key === e.key ? { ...p.open, pinned: e.pinned, stale: e.stale } : p.open, note: e.pinned ? 'saved: pinned pages are kept' : 'unpinned' } : p;
};

/** Page statements that cite a claim can be disputed or promoted; any that cites a source can open it. */
const claimOf = (s: Statement | undefined) => s?.ids.find((i) => i.startsWith('c-'));
const sourceOf = (s: Statement | undefined, o: Open) => {
  const id = s?.ids.find((i) => i.startsWith('s-')) ?? o.table.map((r) => r[0] ?? '').find((c) => c.startsWith('s-'));
  return id?.slice(2);
};

const none: PagesAction = { t: 'none' };

/**
 * Following the focused link of a page (specs/117 §7, specs/097 §4.6): a contents anchor moves the page to its section, a
 * `page:` link asks for that page, a citation opens the source it names or, for a claim, the claim's record.
 */
function follow(p: Pages, n: Nav, w: number): { p: Pages; action: PagesAction } {
  const o = p.open!;
  if (n.k === 'page') return { p: { ...p, note: 'opening page…', q: n.slug, busy: 'page', drop: null }, action: { t: 'pages-open-page', slug: n.slug } };
  if (n.k === 'cite') {
    const id = n.ids[0]!;
    if (/^s-kb-[0-9a-f]{16}$/.test(id) || id.startsWith('f-')) return { p: { ...p, note: `${id} is a passage of this workspace; its row is in the bibliography` }, action: none };
    return { p, action: id.startsWith('s-') ? { t: 'open-source', id: id.slice(2) } : { t: 'open-claim', id } };
  }
  if (asMarkdown(o)) {
    const lines = renderMarkdownLinks(o.markdown!, Math.max(10, pageWidth(w) - 4), { images: o.images }).lines;
    const row = lines.findIndex((l) => anchorOf(l.replace(ANSI_RE, '').trim()) === n.anchor);
    return { p: row < 0 ? { ...p, note: `this page has no section "${n.anchor}"` } : { ...p, at: row, note: '' }, action: none };
  }
  return { p: { ...p, note: 'this page has no sections' }, action: none };
}

/** One key in the Pages view. `used` is false for the keys the window keeps (← → change the view). `w` is the window's width, for moving to a section. */
export function key(p: Pages, e: Ev, h: number, w = 100): { p: Pages; action: PagesAction; used: boolean } {
  const used = (next: Pages, action: PagesAction = none) => ({ p: next, action, used: true });
  if (e.t === 'ctrl') return { p, action: none, used: false };
  if (p.focus === 'note') {
    if (e.t === 'text' || e.t === 'paste') return used({ ...p, draft: (p.draft + (e.t === 'paste' ? e.s.replace(/\s+/g, ' ') : e.s)).replace(/[\x00-\x1f\x7f]/g, '').slice(0, 500) });
    if (e.t !== 'key') return used(p);
    if (e.k === 'esc') return used({ ...p, focus: 'page', draft: '', note: '' });
    if (e.k === 'backspace') return used({ ...p, draft: [...p.draft].slice(0, -1).join('') });
    if (e.k === 'enter') {
      const claim = claimOf(p.open?.body[p.at]);
      if (!claim || !p.draft.trim()) return used({ ...p, note: 'say what is wrong first' });
      return used({ ...p, focus: 'page', draft: '', note: 'filing the dispute…' }, { t: 'pages-dispute', claim, note: p.draft.trim() });
    }
    return used(p);
  }
  if (e.t === 'wheel') return p.focus === 'page' && p.open ? used({ ...p, at: Math.max(0, Math.min(p.open.body.length - 1, p.at + (e.dir === 'up' ? -1 : 1))) }) : used(p);
  if (e.t === 'text' && p.focus === 'page' && p.open) {
    const st = p.open.body[p.at];
    switch (e.s) {
      case 'e': return used({ ...p, note: 'expanding page with reputable web research…' }, { t: 'pages-expand', q: p.open.query });
      case 'r': return used({ ...p, note: 'refreshing page…' }, { t: 'pages-refresh', q: p.open.query, external: p.open.external });
      case 's': return used({ ...p, note: '' }, { t: 'pages-pin', key: p.open.key, pinned: !p.open.pinned });
      case 'd': return claimOf(st) ? used({ ...p, focus: 'note', draft: '', note: '' }) : used({ ...p, note: 'this statement cites no claim to dispute' });
      case 'p': return st && st.text ? used({ ...p, note: 'making a noted claim…' }, { t: 'pages-promote', page: p.open.key, statement: st.text.replace(/\s*\[[^\]]*\]/g, '').trim() }) : used(p);
      case 'o': { const id = sourceOf(st, p.open); return id ? used(p, { t: 'open-source', id }) : used({ ...p, note: 'this statement cites no source' }); }
      default: return used(p);
    }
  }
  if (e.t === 'text' || e.t === 'paste') {
    if (p.focus !== 'box') return used(p);
    const s = (e.t === 'paste' ? e.s.replace(/\s+/g, ' ') : e.s).replace(/[\x00-\x1f\x7f]/g, '');
    return used({ ...p, q: (p.q + s).slice(0, 300), note: '' });
  }
  if (e.t !== 'key' || e.alt) return { p, action: none, used: false };
  if (p.focus === 'box') {
    switch (e.k) {
      case 'backspace': return used({ ...p, q: [...p.q].slice(0, -1).join('') });
      case 'tab': return used({ ...p, ext: !p.ext, note: p.ext ? 'internal: only what the library holds' : 'external: the web is searched too, and what it finds is noted, not validated' });
      case 'enter': {
        const q = p.q.trim();
        if (!q) return used(p);
        return used({ ...p, hits: null, open: null, busy: 'results', focus: 'box', note: '', drop: null }, { t: 'pages-search', q, external: p.ext });
      }
      case 'down': return p.hits?.length || p.pages.length ? used({ ...p, focus: 'results', sel: 0 }) : used(p);
      case 'esc': case 'shift-tab': return e.k === 'esc' && p.busy ? used(cancel(p)) : used(p, { t: 'leave' });
      case 'left': case 'right': return { p, action: none, used: false };
      default: return used(p);
    }
  }
  if (p.focus === 'page') {
    const o = p.open;
    // Tab and shift+tab move through the page's links; past either end they go back to the results, as tab always did.
    const navs = o?.markdown ? navOf(o.markdown) : [];
    const to = (i: number | null) => (i === null || i < 0 || i >= navs.length ? used({ ...p, nav: null, focus: 'results', note: '' }) : used({ ...p, nav: i, note: navWords(navs[i]!) }));
    switch (e.k) {
      // A Markdown page scrolls by rows and is clamped where it is drawn; a page of statements moves by statement.
      case 'down': return o ? used({ ...p, at: Math.min((asMarkdown(o) ? renderMarkdownLinks(o.markdown!, Math.max(10, pageWidth(w) - 4), { images: o.images }).lines.length : o.body.length) - 1, p.at + 1) }) : used(p);
      case 'up': return used({ ...p, at: Math.max(0, p.at - 1) });
      case 'tab': return to(p.nav === null ? (navs.length ? 0 : null) : p.nav + 1);
      case 'shift-tab': return to(p.nav === null ? null : p.nav - 1);
      case 'enter': { const n = p.nav === null ? undefined : navs[p.nav]; if (!n || !o) return used(p); const r = follow(p, n, w); return used(r.p, r.action); }
      case 'esc': return used({ ...p, nav: null, focus: 'results', note: '' });
      case 'left': case 'right': return { p, action: none, used: false };
      default: return used(p);
    }
  }
  const total = (p.hits?.length ?? 0) + p.pages.length;
  switch (e.k) {
    case 'down': return used({ ...p, sel: Math.min(Math.max(0, total - 1), p.sel + 1) });
    case 'up': return p.sel === 0 ? used({ ...p, focus: 'box' }) : used({ ...p, sel: p.sel - 1 });
    case 'pgdn': return used({ ...p, sel: Math.min(Math.max(0, total - 1), p.sel + Math.max(1, Math.floor(h / 3))) });
    case 'pgup': return used({ ...p, sel: Math.max(0, p.sel - Math.max(1, Math.floor(h / 3))) });
    case 'tab': return p.open ? used({ ...p, focus: 'page' }) : used(p);
    case 'enter': {
      const hit = p.hits?.[p.sel];
      if (hit) return hit.kind === 'source' ? used(p, { t: 'open-source', id: hit.id }) : used({ ...p, note: `${hit.id} is a claim; the page that cites it opens beside the results` });
      const entry = p.pages[p.sel - (p.hits?.length ?? 0)];
      return entry ? used({ ...p, busy: 'page', note: '' }, { t: 'pages-open', key: entry.key }) : used(p);
    }
    case 'esc': case 'shift-tab': return used({ ...p, focus: 'box' });
    case 'left': case 'right': return { p, action: none, used: false };
    default: return used(p);
  }
}

const hint = (s: string) => T.dim(' ' + s);
const plural = (n: number, one: string, many: string) => (n === 1 ? `1 ${one}` : `${n} ${many}`);

function tabs(w: number): string {
  const t = (on: boolean, label: string) => (on ? chalk.bold(T.heading(label)) : T.dim(label));
  return T.clip(` ${t(false, 'sources')}  ${t(false, 'claims')}  ${t(true, 'pages')}   ${T.dim('ctrl+l switch')}`, w);
}

function box(p: Pages, w: number): string {
  const cursor = p.focus === 'box' ? T.accent('▏') : '';
  const scope = p.ext ? T.accent('external') : T.dim('internal');
  return T.spread(` ${T.dim('ask')} ${p.q}${cursor}`, scope + ' ', w);
}

function hitRow(h: PHit, on: boolean, w: number): string {
  const head = ` ${on ? T.accent('›') : ' '} ${T.statusMark(h.status)} ${T.dim(h.kind === 'claim' ? 'claim ' : 'source')} ${on ? chalk.bold(h.title) : h.title}${h.private ? T.dim(' · private') : ''}`;
  return T.clip(head, w);
}

function entryRow(e: PEntry, on: boolean, w: number): string {
  const tail = T.dim([e.pinned ? 'pinned' : '', e.stale ? 'stale' : '', e.outcome === 'claims_only' ? 'claims only' : ''].filter(Boolean).join(' · '));
  return T.spread(T.clip(` ${on ? T.accent('›') : ' '} ${T.dim('page  ')} ${on ? chalk.bold(e.query) : e.query}`, Math.max(10, w - T.width(tail) - 2)), tail + ' ', w);
}

const wrap = (text: string, w: number): string[] => {
  const out: string[] = [];
  let line = '';
  for (const word of text.split(/\s+/).filter(Boolean)) {
    if (line && T.width(line + ' ' + word) > w) { out.push(line); line = word; } else line = line ? line + ' ' + word : word;
  }
  if (line) out.push(line);
  return out.length ? out : [''];
};

/** The glyph the daemon printed in the table for a record, so a statement can show the status of what it cites. */
function glyphFor(o: Open, ids: string[]): string {
  const row = o.table.find((r) => ids.includes(r[0] ?? ''));
  return row?.[1]?.split(' ')[0] ?? '';
}

function pageLines(o: Open, at: number, focused: boolean, w: number, h: number, nav: number | null = null): string[] {
  const out: string[] = [];
  out.push(...wrap(o.banner, w - 2).map((l) => ' ' + T.dim(l)));
  const flags = [
    o.kind ? `kind: ${o.kind}` : '',
    o.depth ? `depth: ${o.depth}` : '',
    o.valid_until ? `valid: ${o.valid_until}${o.volatility ? ` (${o.volatility})` : ''}` : '',
    o.pinned ? 'pinned' : '',
    o.stale ? `stale: ${o.stale}` : '',
    o.cached ? 'cached' : '',
    o.external ? 'external' : '',
  ].filter(Boolean).join(' · ');
  if (flags) out.push(' ' + T.dim(flags));
  out.push(T.dim('─'.repeat(Math.max(0, w))));

  if (asMarkdown(o)) {
    const rendered = renderMarkdownLinks(o.markdown!, Math.max(10, w - 4), { images: o.images });
    const room = Math.max(1, h - out.length - 1);
    // The focused link is drawn reversed, and the page is moved just far enough to show it.
    const span = focused && nav !== null ? navSpans(rendered.lines, navOf(o.markdown!))[nav] ?? null : null;
    const lines = span ? rendered.lines.map((l, i) => (i === span.line ? reverseSpan(l, span.col, span.width) : l)) : rendered.lines;
    let first = Math.max(0, Math.min(at, Math.max(0, lines.length - room)));
    if (span && (span.line < first || span.line >= first + room)) first = Math.max(0, Math.min(span.line - Math.floor(room / 3), Math.max(0, lines.length - room)));
    out.push(...lines.slice(first, first + room));
    return out;
  }

  const body: string[] = [];
  let focusAt = 0;
  o.body.forEach((s, i) => {
    if (i === at) focusAt = body.length;
    const g = glyphFor(o, s.ids);
    wrap(s.text, w - 6).forEach((l, j) => body.push(` ${i === at && focused ? T.accent('›') : ' '} ${j === 0 ? (g || ' ') : ' '} ${i === at && focused ? chalk.bold(l) : l}`));
  });
  const table: string[] = [T.dim('─'.repeat(Math.max(0, w))), ` ${T.dim('Bibliography and citations')}`];
  const widths = [8, 12, Math.max(10, Math.floor((w - 40) / 2))];
  for (const r of o.table) table.push(T.clip('  ' + r.slice(0, 4).map((c, i) => (i < 3 ? T.clip(c, widths[i]!).padEnd(widths[i]!) : c)).join(T.dim(' │ ')), w));
  const room = Math.max(1, h - out.length - table.length - 1);
  const first = Math.max(0, Math.min(focusAt - Math.floor(room / 3), body.length - room));
  out.push(...body.slice(first, first + room), ...Array(Math.max(0, room - Math.min(room, body.length - first))).fill(''), ...table.slice(0, Math.max(2, h - out.length - 1)));
  return out;
}

/** The view's body: `h` lines of at most `w` columns, the last one the key hint. `still` is the reduce-motion answer from `motion.ts`. */
export function view(p: Pages, w: number, h: number, now: number, still: boolean): string[] {
  const top = ['', tabs(w), box(p, w), T.dim('─'.repeat(Math.max(0, w)))];
  const room = Math.max(1, h - top.length - 2);
  const wide = w >= 100 && p.open !== null;
  const left = wide ? Math.floor(w * 0.4) : w;
  const lines: string[] = [];
  const results: string[] = [];
  if (p.busy === 'results') results.push(T.dim(`   ${spinner(now, still)} looking in the library…`));
  else if (p.hits === null && !p.pages.length) results.push('', T.dim('   ask a question: what the library holds appears at once, and a page is written from it'));
  if (p.hits) {
    results.push(T.dim(` ${plural(p.hits.length, 'record', 'records')} found`));
    p.hits.forEach((x, i) => results.push(hitRow(x, p.focus === 'results' && p.sel === i, left)));
    if (!p.hits.length) results.push(T.dim('   nothing in the library matches yet'));
  }
  if (p.pages.length && p.busy !== 'results') {
    results.push(T.dim(' earlier pages'));
    p.pages.forEach((x, i) => results.push(entryRow(x, p.focus === 'results' && p.sel === (p.hits?.length ?? 0) + i, left)));
  }
  const waiting = p.busy === 'page' ? [T.dim(` ${spinner(now, still)} writing the page, then checking it… · esc cancels`)] : [];
  if (wide && p.open) {
    const page = pageLines(p.open, p.at, p.focus === 'page', w - left - 3, room, p.nav);
    for (let i = 0; i < room; i++) lines.push(T.spread(T.clip(results[i] ?? '', left), T.dim('│ ') + (page[i] ?? ''), w));
  } else if (p.open) {
    lines.push(...pageLines(p.open, p.at, p.focus === 'page', w, room, p.nav));
  } else lines.push(...results.slice(0, room), ...waiting);
  const out = [...top, ...lines.slice(0, room)];
  while (out.length < h - 2) out.push('');
  const note = p.focus === 'note' ? ` ${T.accent('what is wrong?')} ${p.draft}${T.accent('▏')}` : p.note ? T.dim(' ' + sanitize(p.note)) : p.open?.notes.length ? T.dim(' ' + p.open.notes[0]) : p.open?.outcome === 'claims_only' ? T.dim(' the draft did not pass its checks: these are the claims alone') : '';
  out.push(note);
  out.push(hint(p.focus === 'note' ? 'enter file it · esc cancel' : p.focus === 'page' ? '↑ ↓ scroll · tab next link · enter follow · e expand · r refresh · s save/pin · d dispute · p promote · o source · esc back' : p.focus === 'box' ? 'enter ask · tab internal/external · ↓ results · esc back' : '↑ ↓ pick · enter open · tab page · esc ask again'));
  return out.slice(0, h).map((s) => T.clip(s, w));
}
