import React from 'react';
import { currentCapabilities, terminalAnsiColor } from './theme.ts';

type Style = { fg?: string; bg?: string; attributes: number; link?: { url: string } };
const BOLD = 1, DIM = 2, ITALIC = 4, UNDERLINE = 8, BLINK = 16, INVERSE = 32, HIDDEN = 64, STRIKE = 128;
const ANSI = '\x1b[';

function styledLine(line: string, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  const style: Style = { attributes: 0 };
  const colorEnabled = currentCapabilities().color !== 'none';
  const parts = line.split(/(\x1b\[[0-9;]*m|\x1b\]8;;[^\x07\x1b]*(?:\x07|\x1b\\))/g);
  let key = 0;
  const append = (text: string) => {
    if (!text) return;
    if (!colorEnabled) { nodes.push(text); return; }
    if (!style.attributes && !style.fg && !style.bg && !style.link) { nodes.push(text); return; }
    const props: Record<string, unknown> = { key: `${keyPrefix}-${key++}`, attributes: style.attributes };
    if (style.fg) props.fg = style.fg;
    if (style.bg) props.bg = style.bg;
    if (style.link) props.link = style.link;
    nodes.push(React.createElement('span' as any, props, text));
  };
  for (const part of parts) {
    if (!part) continue;
    if (part.startsWith(`${ANSI}`) && part.endsWith('m')) {
      const codes = part.slice(2, -1).split(';').map((value) => Number(value || 0));
      for (let i = 0; i < codes.length; i++) {
        const c = codes[i]!;
        if (c === 0) { style.fg = undefined; style.bg = undefined; style.attributes = 0; }
        else if (c === 1) style.attributes |= BOLD;
        else if (c === 2) style.attributes |= DIM;
        else if (c === 3) style.attributes |= ITALIC;
        else if (c === 4) style.attributes |= UNDERLINE;
        else if (c === 5 || c === 6) style.attributes |= BLINK;
        else if (c === 7) style.attributes |= INVERSE;
        else if (c === 8) style.attributes |= HIDDEN;
        else if (c === 9) style.attributes |= STRIKE;
        else if (c === 22) style.attributes &= ~(BOLD | DIM);
        else if (c === 23) style.attributes &= ~ITALIC;
        else if (c === 24) style.attributes &= ~UNDERLINE;
        else if (c === 25) style.attributes &= ~BLINK;
        else if (c === 27) style.attributes &= ~INVERSE;
        else if (c === 28) style.attributes &= ~HIDDEN;
        else if (c === 29) style.attributes &= ~STRIKE;
        else if (c === 39 && colorEnabled) style.fg = undefined;
        else if (c === 49 && colorEnabled) style.bg = undefined;
        else if (c >= 30 && c <= 37 && colorEnabled) style.fg = terminalAnsiColor(c - 30);
        else if (c >= 90 && c <= 97 && colorEnabled) style.fg = terminalAnsiColor(c - 90 + 8);
        else if (c >= 40 && c <= 47 && colorEnabled) style.bg = terminalAnsiColor(c - 40);
        else if (c >= 100 && c <= 107 && colorEnabled) style.bg = terminalAnsiColor(c - 100 + 8);
        else if ((c === 38 || c === 48) && codes[i + 1] === 5) {
          if (colorEnabled) { if (c === 38) style.fg = terminalAnsiColor(codes[i + 2]!); else style.bg = terminalAnsiColor(codes[i + 2]!); }
          i += 2;
        } else if ((c === 38 || c === 48) && codes[i + 1] === 2) {
          const rgb = `#${codes.slice(i + 2, i + 5).map((v) => Math.max(0, Math.min(255, v)).toString(16).padStart(2, '0')).join('')}`;
          if (colorEnabled) { if (c === 38) style.fg = rgb; else style.bg = rgb; }
          i += 4;
        }
      }
    } else if (part.startsWith('\x1b]8;;')) {
      const address = part.slice(5).replace(/(?:\x07|\x1b\\)$/, '');
      style.link = address ? { url: address } : undefined;
    } else {
      // Strip any non-SGR terminal controls; line layout and cursor movement belong to the screen model.
      append(part.replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, ''));
    }
  }
  return nodes;
}

export function Frame({ lines, width, height }: { lines: string[]; width: number; height: number }) {
  return React.createElement('box' as any, { flexDirection: 'column', width, height },
    ...lines.map((line, row) => React.createElement('text' as any, { key: row, height: 1, width }, ...styledLine(line, `row-${row}`))));
}
