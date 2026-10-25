import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import { renderMarkdown, wrapLine } from '../src/markdown.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

test('a tight list item renders its bold and code spans instead of the raw markers', () => {
  const prev = chalk.level;
  chalk.level = 3;
  try {
    const out = renderMarkdown('- **macOS terminal:** `sleep 300`\n- plain', 80).join('\n');
    assert.ok(!plain(out).includes('**') && !plain(out).includes('`'), plain(out));
    assert.ok(out.includes('\x1b[1m'), 'bold is styled');
    assert.equal(plain(out).split('\n')[0], '• macOS terminal: sleep 300');
  } finally { chalk.level = prev; }
});

test('a wrapped line starts no continuation row with a space, and keeps its indent', () => {
  const rows = wrapLine('If you run one of these and paste the output', 20);
  assert.ok(rows.length > 1);
  assert.ok(rows.every((r) => !r.startsWith(' ')), JSON.stringify(rows));
  const list = renderMarkdown('- alpha beta gamma delta epsilon zeta eta theta', 20).map(plain);
  assert.ok(list.length > 1 && list.slice(1).filter(Boolean).every((r) => /^ {2}\S/.test(r)), JSON.stringify(list));
});
