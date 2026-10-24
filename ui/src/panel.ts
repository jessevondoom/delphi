// The research panels that take over the chat area: search the knowledge base, see where answers came from, read the
// notebook. Like the Decision Inbox they own the keyboard while open (Esc closes), so nothing typed here reaches the prompt.
import chalk from 'chalk';
import wrapAnsi from 'wrap-ansi';
import { renderMarkdown } from './markdown.ts';
import type { Source } from './research.ts';
import type { Ev } from './input.ts';
import * as T from './theme.ts';

export type Hit = { pointer: string; heading: string; preview: string };
export type Panel = (
  | { kind: 'search'; q: string; editing: boolean; hits: Hit[]; sel: number; note: string; lines: string[] }
  | { kind: 'sources'; sel: number; lines: string[] }
  | { kind: 'notes'; scroll: number; text: string }
) & { flash?: string };

export type PanelAction =
  | { t: 'none' }
  | { t: 'close' }
  | { t: 'search'; q: string }
  | { t: 'load'; path: string }        // fetch a preview for this logical path
  | { t: 'insert'; text: string }      // put this in the prompt (and close)
  | { t: 'pin'; from: string; text: string };

export const openSearch = (q = ''): Panel => ({ kind: 'search', q, editing: !q, hits: [], sel: 0, note: '', lines: [] });
export const openSources = (): Panel => ({ kind: 'sources', sel: 0, lines: [] });
export const openNotes = (text: string): Panel => ({ kind: 'notes', scroll: 0, text });

const pathOf = (pointer: string) => pointer.split('#')[0]!;
const sorted = (src: Record<string, Source>) => Object.values(src).sort((a, b) => (a.how === b.how ? b.last - a.last : a.how === 'read' ? -1 : 1));

export function key(p: Panel, e: Ev, src: Record<string, Source>): { panel: Panel; action: PanelAction } {
  const keep = (panel: Panel = p, action: PanelAction = { t: 'none' }) => ({ panel, action });
  if (e.t === 'ctrl' && e.c === 'c') return keep(p, { t: 'close' });
  const k = e.t === 'key' ? e.k : e.t === 'text' ? e.s : '';
  if (p.kind === 'search') {
    if (p.editing) {
      if (e.t === 'text' && [...p.q].length < 200) return keep({ ...p, q: p.q + e.s });
      if (e.t === 'paste') return keep({ ...p, q: (p.q + e.s.replace(/\n/g, ' ')).slice(0, 200) });
      if (k === 'backspace') return keep({ ...p, q: [...p.q].slice(0, -1).join('') });
      if (k === 'esc') return p.hits.length ? keep({ ...p, editing: false }) : keep(p, { t: 'close' });
      if (k === 'enter') return p.q.trim() ? keep({ ...p, note: 'searching…' }, { t: 'search', q: p.q.trim() }) : keep();
      return keep();
    }
    if (k === 'esc') return keep(p, { t: 'close' });
    if (k === '/' || k === 'tab') return keep({ ...p, editing: true });
    const move = (d: number) => { const sel = Math.max(0, Math.min(p.hits.length - 1, p.sel + d)); return keep({ ...p, sel, lines: [] }, p.hits[sel] ? { t: 'load', path: pathOf(p.hits[sel]!.pointer) } : { t: 'none' }); };
    if (k === 'up' || k === 'k') return move(-1);
    if (k === 'down' || k === 'j') return move(1);
    const hit = p.hits[p.sel];
    if (k === 'enter' && hit) return keep(p, { t: 'insert', text: '@' + hit.pointer.replace(/^@/, '') + ' ' });
    if (k === 'p' && hit) return keep(p, { t: 'pin', from: `search “${p.q}” → ${hit.pointer}`, text: `${hit.pointer} — ${hit.heading}\n\n${hit.preview}` });
    return keep();
  }
  if (p.kind === 'sources') {
    const list = sorted(src);
    if (k === 'esc' || k === 'q') return keep(p, { t: 'close' });
    const move = (d: number) => { const sel = Math.max(0, Math.min(list.length - 1, p.sel + d)); return keep({ ...p, sel, lines: [] }, list[sel] ? { t: 'load', path: list[sel]!.path } : { t: 'none' }); };
    if (k === 'up' || k === 'k') return move(-1);
    if (k === 'down' || k === 'j') return move(1);
    const s = list[Math.min(p.sel, list.length - 1)];
    if (k === 'enter' && s) return keep(p, { t: 'insert', text: '@' + s.path.replace(/^@/, '') + ' ' });
    if (k === 'p' && s) return keep(p, { t: 'pin', from: `source ${s.path}`, text: s.path });
    return keep();
  }
  if (k === 'esc' || k === 'q') return keep(p, { t: 'close' });
  if (k === 'up' || k === 'k') return keep({ ...p, scroll: Math.max(0, p.scroll - 1) });
  if (k === 'down' || k === 'j') return keep({ ...p, scroll: p.scroll + 1 });
  if (k === 'pgup') return keep({ ...p, scroll: Math.max(0, p.scroll - 10) });
  if (k === 'pgdn' || k === ' ') return keep({ ...p, scroll: p.scroll + 10 });
  return keep();
}

export const hint = (p: Panel): string =>
  p.flash ? p.flash : p.kind === 'search' ? (p.editing ? 'type a query · enter search · esc close' : '↑↓ choose · enter put @path in the prompt · p pin to notes · / new search · esc close')
    : p.kind === 'sources' ? '↑↓ choose · enter put @path in the prompt · p pin · esc close'
      : '↑↓ scroll · esc close';

const wrap = (s: string, w: number) => wrapAnsi(s, Math.max(10, w), { hard: true, trim: false }).split('\n');

/** The panel as exactly `h` lines. */
export function view(p: Panel, src: Record<string, Source>, pal: T.Pal, w: number, h: number): string[] {
  const pad = (a: string[]) => [...a, ...Array(Math.max(0, h - a.length)).fill('')].slice(0, h);
  const title = (t: string, n?: number) => '  ' + chalk.bold(T.text(pal)(t)) + (n === undefined ? '' : T.dim(` (${n})`));
  const rule = T.dim('  ' + '┈'.repeat(Math.max(0, w - 4)));
  const preview = (lines: string[], room: number) => lines.slice(0, room).map((l) => '  ' + T.dim(T.clip(l, w - 4)));
  if (p.kind === 'search') {
    const head = ['', title('Search the knowledge base'), '  ' + (p.editing ? T.link('search› ') + T.text(pal)(p.q) + '▏' : T.dim(`“${p.q}”`)), ''];
    if (!p.hits.length) return pad([...head, '  ' + T.dim(p.note || 'specs, context, REVIEW.md, lessons and mission notes; every hit is a pointer you can cite')]);
    const rows = p.hits.map((hit, i) => (i === p.sel ? T.accent('  ❯ ') + chalk.bold(T.link(pathOf(hit.pointer))) : '    ' + T.link(pathOf(hit.pointer))) + T.dim(`  ${T.clip(hit.heading, Math.max(10, w - pathOf(hit.pointer).length - 10))}`));
    const sel = p.hits[Math.min(p.sel, p.hits.length - 1)]!;
    const room = Math.max(3, h - head.length - rows.length - 3);
    return pad([...head, ...rows.slice(0, Math.max(3, h - 8)), rule, ...wrap(sel.preview, w - 4).map((l) => '  ' + T.text(pal)(l)).slice(0, 6), ...(p.lines.length ? ['', ...preview(p.lines, room)] : [])]);
  }
  if (p.kind === 'sources') {
    const list = sorted(src);
    if (!list.length) return pad(['', title('Sources'), '', '  ' + T.dim('Nothing yet. Files the agents read, and documents their replies cite, are listed here.')]);
    const sel = Math.min(p.sel, list.length - 1);
    const rows = list.map((s, i) => (i === sel ? T.accent('  ❯ ') + chalk.bold(T.link(s.path)) : '    ' + T.link(s.path)) + T.dim(`  ${s.how === 'read' ? 'read' : 'cited'} ${s.count}× · ${s.roles.join(', ')}`));
    const room = Math.max(3, h - 6 - Math.min(rows.length, h - 10));
    return pad(['', title('Sources', list.length), '', ...rows.slice(0, Math.max(3, h - 10)), '', rule, ...(p.lines.length ? preview(p.lines, room) : [])]);
  }
  const md = p.text.trim() ? renderMarkdown(p.text, w - 4).map((l) => '  ' + l) : ['  ' + T.dim('The notebook is empty. /pin keeps the last reply; in search, p pins a hit.')];
  const scroll = Math.min(p.scroll, Math.max(0, md.length - (h - 3)));
  return pad(['', title('Mission notebook'), '', ...md.slice(scroll)]);
}
