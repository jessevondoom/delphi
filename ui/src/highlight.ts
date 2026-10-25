// Small syntax highlighter for the code in replies: strings, comments, numbers, keywords, diffs. Not a parser; it
// only colours, so a wrong guess costs a colour, never text.
import chalk from 'chalk';
import { syntax } from './theme.ts';

const KW: Record<string, string> = {
  rust: 'fn let mut pub struct enum impl trait use mod match if else for while loop return self Self crate as in where async await move const static type unsafe true false',
  python: 'def class import from as return if elif else for while in not and or is None True False with try except finally raise lambda yield pass break continue async await',
  js: 'function const let var return if else for while of in new class extends import export from default async await try catch finally throw typeof null undefined true false this interface type enum',
  sh: 'if then else fi for do done case esac function in while export local return set',
  toml: 'true false',
  json: 'true false null',
};
const ALIAS: Record<string, string> = { rs: 'rust', py: 'python', javascript: 'js', typescript: 'js', ts: 'js', tsx: 'js', jsx: 'js', bash: 'sh', shell: 'sh', zsh: 'sh', yaml: 'toml', yml: 'toml' };
const COMMENT: Record<string, RegExp> = { rust: /^\/\/.*/, js: /^\/\/.*/, python: /^#.*/, sh: /^#.*/, toml: /^#.*/ };

const c = syntax;

export function highlightLine(line: string, lang: string): string {
  const l = ALIAS[lang.toLowerCase()] ?? lang.toLowerCase();
  if (l === 'diff' || l === 'patch') {
    if (/^\+(?!\+\+)/.test(line)) return c.add(line);
    if (/^-(?!--)/.test(line)) return c.del(line);
    if (/^@@/.test(line)) return c.hunk(line);
    return chalk.dim(line);
  }
  const kws = new Set((KW[l] ?? '').split(' '));
  const com = COMMENT[l];
  let out = '', i = 0;
  while (i < line.length) {
    const rest = line.slice(i);
    let m: RegExpExecArray | null;
    if (com && (m = com.exec(rest))) { out += c.com(m[0]); break; }
    if ((m = /^("(?:[^"\\]|\\.)*"?|'(?:[^'\\]|\\.)*'?|`[^`]*`?)/.exec(rest))) { out += c.str(m[0]); i += m[0].length; continue; }
    if ((m = /^\d[\d_.]*/.exec(rest)) && !/\w/.test(line[i - 1] ?? '')) { out += c.num(m[0]); i += m[0].length; continue; }
    if ((m = /^[A-Za-z_][A-Za-z0-9_]*/.exec(rest))) {
      out += kws.has(m[0]) ? c.kw(m[0]) : /^[A-Z]/.test(m[0]) && l !== 'sh' ? c.ty(m[0]) : m[0];
      i += m[0].length; continue;
    }
    out += line[i]; i++;
  }
  return out;
}
