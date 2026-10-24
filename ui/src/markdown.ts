// Markdown to ANSI lines: the part of the UI where a library (marked) does the parsing and we only style.
import { Lexer, type Token, type Tokens } from 'marked';
import chalk from 'chalk';
import { sanitize } from './theme.ts';
import { highlightLine } from './highlight.ts';
import wrapAnsi from 'wrap-ansi';

const inline = (toks: Token[] | undefined): string =>
  (toks ?? []).map((t) => {
    switch (t.type) {
      case 'strong': return chalk.bold(inline((t as Tokens.Strong).tokens));
      case 'em': return chalk.italic(inline((t as Tokens.Em).tokens));
      case 'codespan': return chalk.hex('#38bdf8')((t as Tokens.Codespan).text);
      case 'link': return chalk.underline.hex('#60a5fa')((t as Tokens.Link).text) + chalk.dim(` <${(t as Tokens.Link).href}>`);
      case 'br': return '\n';
      case 'text': return (t as Tokens.Text).tokens ? inline((t as Tokens.Text).tokens) : (t as Tokens.Text).text;
      default: return (t as any).raw ?? '';
    }
  }).join('');

function block(t: Token, indent = ''): string[] {
  switch (t.type) {
    case 'heading': return ['', chalk.bold.hex('#facc15')(inline((t as Tokens.Heading).tokens)), ''];
    case 'paragraph': return inline((t as Tokens.Paragraph).tokens).split('\n').concat('');
    case 'code': {
      const c = t as Tokens.Code;
      const lang = (c.lang ?? '').split(/\s/)[0]!;
      return [chalk.dim(`  ${lang}`), ...c.text.split('\n').map((l) => '  ' + highlightLine(l, lang)), ''];
    }
    case 'list': {
      const l = t as Tokens.List;
      return l.items.flatMap((it, i) => {
        const mark = l.ordered ? `${(Number(l.start) || 1) + i}. ` : '• ';
        const body = it.tokens.flatMap((x) => block(x)).filter((x, j, a) => !(x === '' && j === a.length - 1));
        return body.map((ln, j) => (j === 0 ? mark : ' '.repeat(mark.length)) + ln);
      }).concat('');
    }
    case 'blockquote': return (t as Tokens.Blockquote).tokens.flatMap((x) => block(x)).map((l) => chalk.dim('│ ') + chalk.italic(l));
    case 'hr': return [chalk.dim('─'.repeat(40)), ''];
    case 'space': return [];
    case 'table': {
      const tb = t as Tokens.Table;
      const cells = (r: Tokens.TableCell[]) => r.map((c) => inline(c.tokens));
      return [chalk.bold(cells(tb.header).join('  │  ')), ...tb.rows.map((r) => cells(r).join('  │  ')), ''];
    }
    default: return [(t as any).raw ?? ''];
  }
}

/** Markdown to lines no wider than `width` (ANSI styling preserved across the wrap). */
export function renderMarkdown(src: string, width: number): string[] {
  const lines = new Lexer().lex(sanitize(src)).flatMap((t) => block(t));
  return lines.flatMap((l) => (l === '' ? [''] : wrapAnsi(l, Math.max(10, width), { hard: true, trim: false }).split('\n')));
}
