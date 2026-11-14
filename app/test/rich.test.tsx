import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Rich } from '../src/Rich.tsx';
import { blocks, spans } from '../src/rich.ts';

test('bold and inline code are found in a line, and anything unbalanced stays as written', () => {
  assert.deepEqual(spans('a **b** and `c` d'), [
    { kind: 'text', text: 'a ' }, { kind: 'bold', text: 'b' }, { kind: 'text', text: ' and ' }, { kind: 'code', text: 'c' }, { kind: 'text', text: ' d' },
  ]);
  assert.deepEqual(spans('2 ** 3 and a ` tick'), [{ kind: 'text', text: '2 ** 3 and a ` tick' }]);
  assert.deepEqual(spans(''), []);
});

test('a reply becomes paragraphs, headings, list items, fenced code and single gaps', () => {
  const b = blocks('# Title\n\n\nFirst.\n- one\n  - two **deep**\n3. three\n```\nlet x = "**not bold**";\n```\n\n');
  assert.deepEqual(b.map((x) => x.kind), ['head', 'gap', 'para', 'item', 'item', 'item', 'code']);
  assert.deepEqual(b[3], { kind: 'item', marker: '•', depth: 0, spans: [{ kind: 'text', text: 'one' }] });
  assert.equal((b[4] as { depth: number }).depth, 1);
  assert.equal((b[5] as { marker: string }).marker, '3.');
  assert.deepEqual(b[6], { kind: 'code', text: 'let x = "**not bold**";' });
  // A fence that never closes takes the rest as code instead of losing it.
  assert.deepEqual(blocks('```\nabc'), [{ kind: 'code', text: 'abc' }]);
});

test('nothing an agent wrote is read as HTML', () => {
  const html = renderToStaticMarkup(<Rich text={'**<img src=x onerror=alert(1)>** and `<script>x</script>`\n<b>plain</b>'} />);
  assert.ok(!html.includes('<img') && !html.includes('<script') && !html.includes('<b>'), html);
  assert.ok(html.includes('<strong>&lt;img src=x onerror=alert(1)&gt;</strong>'));
  assert.ok(html.includes('<code>&lt;script&gt;x&lt;/script&gt;</code>'));
});
