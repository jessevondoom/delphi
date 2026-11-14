// An agent's reply is Markdown. The window shows a small part of it (bold, inline code, lists, headings, fenced code) and the
// rest as it was written. This turns text into plain data; the component makes elements from it, so nothing an agent wrote is
// ever read as HTML (spec 111 §2.5).

export type Span = { kind: 'text' | 'bold' | 'code'; text: string };
export type Block =
  | { kind: 'para'; spans: Span[] }
  | { kind: 'head'; spans: Span[] }
  | { kind: 'item'; marker: string; depth: number; spans: Span[] }
  | { kind: 'code'; text: string }
  | { kind: 'gap' };

/** `**bold**` and `` `code` `` within a line; anything unbalanced stays as written. */
export function spans(line: string): Span[] {
  const out: Span[] = [];
  const re = /(\*\*([^*\n]+)\*\*)|(`([^`\n]+)`)/g;
  let last = 0;
  for (let m = re.exec(line); m; m = re.exec(line)) {
    if (m.index > last) out.push({ kind: 'text', text: line.slice(last, m.index) });
    out.push(m[2] !== undefined ? { kind: 'bold', text: m[2] } : { kind: 'code', text: m[4]! });
    last = m.index + m[0].length;
  }
  if (last < line.length) out.push({ kind: 'text', text: line.slice(last) });
  return out;
}

export function blocks(text: string): Block[] {
  const out: Block[] = [];
  const lines = text.replace(/\r\n?/g, '\n').split('\n');
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!;
    if (/^\s*```/.test(line)) {
      const body: string[] = [];
      for (i++; i < lines.length && !/^\s*```/.test(lines[i]!); i++) body.push(lines[i]!);
      out.push({ kind: 'code', text: body.join('\n') });
      continue;
    }
    if (!line.trim()) {
      if (out.length && out[out.length - 1]!.kind !== 'gap') out.push({ kind: 'gap' });
      continue;
    }
    const head = /^#{1,6}\s+(.*)$/.exec(line);
    if (head) { out.push({ kind: 'head', spans: spans(head[1]!) }); continue; }
    const item = /^(\s*)([-*•]|\d+[.)])\s+(.*)$/.exec(line);
    if (item) { out.push({ kind: 'item', marker: /\d/.test(item[2]!) ? item[2]! : '•', depth: Math.min(3, Math.floor(item[1]!.length / 2)), spans: spans(item[3]!) }); continue; }
    out.push({ kind: 'para', spans: spans(line) });
  }
  while (out.length && out[out.length - 1]!.kind === 'gap') out.pop();
  return out;
}
