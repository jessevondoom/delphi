// Selecting and copying text. Mouse reporting is on so the wheel can scroll, and a terminal that has handed the mouse
// to a program no longer selects text itself. So the console does it: drag to select, release to copy; double-click
// takes a word and triple-click a line. The screen is a pure array of lines (see screen.ts), which is what makes this
// simple: a selection is two cells, the text is read from the lines, and highlighting is a reverse-video pass over them.
import { useEffect, useMemo, useRef, useState } from 'react';
import { spawn } from 'node:child_process';
import stringWidth from 'string-width';
import { graphemes } from './text.ts';
import type { Term } from './terminal.ts';
import type { Mouse } from './input.ts';

export type Pt = { x: number; y: number };
export type Sel = { a: Pt; b: Pt };

const ANSI_SRC = '\\x1b\\[[0-9;?]*[ -/]*[@-~]|\\x1b\\][^\\x07\\x1b]*(?:\\x07|\\x1b\\\\)';
const strip = (s: string) => s.replace(new RegExp(ANSI_SRC, 'g'), '');

type Cell = { g: string; x: number; w: number };

/** The characters of a line with the column each starts at (a wide character takes two). */
function cells(line: string): Cell[] {
  const out: Cell[] = [];
  let x = 0;
  for (const g of graphemes(strip(line))) {
    const w = stringWidth(g);
    out.push({ g, x, w });
    x += w;
  }
  return out;
}

const before = (p: Pt, q: Pt) => p.y < q.y || (p.y === q.y && p.x <= q.x);

/** The columns of `row` inside the selection, inclusive, or null when the row is outside it. */
function span(sel: Sel, row: number): [number, number] | null {
  const [s, e] = before(sel.a, sel.b) ? [sel.a, sel.b] : [sel.b, sel.a];
  if (row < s.y || row > e.y) return null;
  return [row === s.y ? s.x : 0, row === e.y ? e.x : Infinity];
}

export const isEmpty = (sel: Sel) => sel.a.x === sel.b.x && sel.a.y === sel.b.y;

/** The selected text, as the screen shows it: no colour codes, trailing blanks dropped from each line. */
export function extract(lines: string[], sel: Sel): string {
  const rows: string[] = [];
  const [s, e] = before(sel.a, sel.b) ? [sel.a, sel.b] : [sel.b, sel.a];
  for (let y = s.y; y <= Math.min(e.y, lines.length - 1); y++) {
    const [c0, c1] = span(sel, y)!;
    rows.push(cells(lines[y]!).filter((c) => c.x + Math.max(c.w, 1) - 1 >= c0 && c.x <= c1).map((c) => c.g).join('').trimEnd());
  }
  while (rows.length && rows[rows.length - 1] === '') rows.pop();
  return rows.join('\n');
}

/** The word under a cell: the run of non-blank characters around it. Null on a blank. */
export function wordAt(line: string, x: number): [number, number] | null {
  const cs = cells(line);
  const i = cs.findIndex((c) => x >= c.x && x < c.x + Math.max(c.w, 1));
  if (i < 0 || /^\s*$/.test(cs[i]!.g)) return null;
  let a = i, b = i;
  while (a > 0 && !/^\s*$/.test(cs[a - 1]!.g)) a--;
  while (b < cs.length - 1 && !/^\s*$/.test(cs[b + 1]!.g)) b++;
  return [cs[a]!.x, cs[b]!.x + Math.max(cs[b]!.w, 1) - 1];
}

const REVERSE = '\x1b[7m', NORMAL = '\x1b[27m';

/** The lines with the selection in reverse video. Colour codes inside it are kept; a reset cannot switch it off. */
export function paint(lines: string[], sel: Sel): string[] {
  if (isEmpty(sel)) return lines;
  const [s, e] = before(sel.a, sel.b) ? [sel.a, sel.b] : [sel.b, sel.a];
  const re = new RegExp(ANSI_SRC, 'y');
  return lines.map((line, y) => {
    if (y < s.y || y > e.y) return line;
    const [c0, c1] = span(sel, y)!;
    let out = '', col = 0, on = false, i = 0;
    while (i < line.length) {
      re.lastIndex = i;
      const m = re.exec(line);
      if (m) { out += m[0]; if (on && m[0].endsWith('m')) out += REVERSE; i += m[0].length; continue; }
      const g = graphemes(line.slice(i, i + 16))[0]!; // a grapheme is far shorter than 16 UTF-16 units
      const w = stringWidth(g);
      const inside = col + Math.max(w, 1) - 1 >= c0 && col <= c1;
      if (inside && !on) { out += REVERSE; on = true; } else if (!inside && on) { out += NORMAL; on = false; }
      out += g; col += w; i += g.length;
    }
    return on ? out + NORMAL : out;
  });
}

/** Put text on the clipboard: the terminal's own (OSC 52, which also works over ssh and tmux) and the machine's. */
const MACHINE_TOOLS: string[][] = process.platform === 'darwin' ? [['pbcopy']] : process.env.WAYLAND_DISPLAY ? [['wl-copy']] : [['xclip', '-selection', 'clipboard'], ['xsel', '--clipboard', '--input']];

export function copyToClipboard(text: string, write: (s: string) => void = (s) => { process.stdout.write(s); }, tools: string[][] = MACHINE_TOOLS) {
  if (!text) return;
  if (Buffer.byteLength(text) <= 100_000) write(`\x1b]52;c;${Buffer.from(text).toString('base64')}\x07`);
  const tryTool = (i: number) => {
    const t = tools[i];
    if (!t) return;
    try {
      const p = spawn(t[0]!, t.slice(1), { stdio: ['pipe', 'ignore', 'ignore'] });
      p.on('error', () => tryTool(i + 1));
      p.stdin.on('error', () => {});
      p.stdin.end(text);
    } catch { tryTool(i + 1); }
  };
  tryTool(0);
}

/**
 * Selection for a screen drawn from `lines`: returns the lines to paint. Mouse events come from the terminal; any key
 * or wheel turn clears the selection (the text under it is about to move).
 */
export function useSelection(term: Term, lines: string[], copy: (text: string) => void = (t) => copyToClipboard(t)): string[] {
  const [sel, setSel] = useState<Sel | null>(null);
  const linesRef = useRef(lines);
  linesRef.current = lines;
  const cur = useRef<{ sel: Sel | null; dragging: boolean; last: { t: number; x: number; y: number; n: number } | null }>({ sel: null, dragging: false, last: null });

  useEffect(() => {
    const set = (s: Sel | null) => { cur.current.sel = s; setSel(s); };
    const onMouse = (m: Mouse) => {
      const c = cur.current;
      const p = { x: m.x, y: m.y };
      if (m.k === 'down') {
        const now = Date.now();
        const n = c.last && now - c.last.t < 400 && c.last.x === p.x && c.last.y === p.y ? (c.last.n % 3) + 1 : 1;
        c.last = { t: now, ...p, n };
        const line = linesRef.current[p.y];
        if (line === undefined) { c.dragging = false; set(null); return; }
        if (n === 1) { c.dragging = true; set({ a: p, b: p }); return; }
        c.dragging = false;
        const w = n === 2 ? wordAt(line, p.x) : [0, Math.max(0, cells(line).reduce((x, k) => k.x + Math.max(k.w, 1), 0) - 1)] as [number, number];
        if (!w) { set(null); return; }
        const next = { a: { x: w[0], y: p.y }, b: { x: w[1], y: p.y } };
        set(next);
        copy(extract(linesRef.current, next));
      } else if (m.k === 'drag') {
        if (c.dragging && c.sel) set({ a: c.sel.a, b: p });
      } else if (c.dragging && c.sel) {
        c.dragging = false;
        const next = { a: c.sel.a, b: p };
        if (isEmpty(next)) { set(null); return; }
        set(next);
        copy(extract(linesRef.current, next));
      }
    };
    const onEv = (e: { t: string }) => { if (e.t !== 'bg' && cur.current.sel) { cur.current.dragging = false; setSel(null); cur.current.sel = null; } };
    term.on('mouse', onMouse);
    term.on('ev', onEv);
    return () => { term.off('mouse', onMouse); term.off('ev', onEv); };
  }, [term]);

  return useMemo(() => (sel ? paint(lines, sel) : lines), [lines, sel]);
}
