import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { PICTURE_SIDE_MAX, Rich, pictureSide, pictureTarget } from '../src/Rich.tsx';
import { inline, isWeb, parse, resolvePath } from '../src/md.ts';
import { STILL_CSS, asksToLoad, isMermaid, mermaidConfig } from '../src/diagram.ts';

test('emphasis, code, links and pictures are found in a line, and anything unbalanced stays as written', () => {
  assert.deepEqual(inline('a **b** and `c` d'), [{ t: 'text', s: 'a ' }, { t: 'strong', c: [{ t: 'text', s: 'b' }] }, { t: 'text', s: ' and ' }, { t: 'code', s: 'c' }, { t: 'text', s: ' d' }]);
  assert.deepEqual(inline('*it* _too_ ~~gone~~ __bold__'), [
    { t: 'em', c: [{ t: 'text', s: 'it' }] }, { t: 'text', s: ' ' }, { t: 'em', c: [{ t: 'text', s: 'too' }] }, { t: 'text', s: ' ' },
    { t: 'del', c: [{ t: 'text', s: 'gone' }] }, { t: 'text', s: ' ' }, { t: 'strong', c: [{ t: 'text', s: 'bold' }] },
  ]);
  assert.deepEqual(inline('2 ** 3, snake_case_name and a ` tick'), [{ t: 'text', s: '2 ** 3, snake_case_name and a ` tick' }]);
  assert.deepEqual(inline('see [the **plan**](plan.md "t") and ![a chart](img/c.png)'), [
    { t: 'text', s: 'see ' }, { t: 'link', href: 'plan.md', c: [{ t: 'text', s: 'the ' }, { t: 'strong', c: [{ t: 'text', s: 'plan' }] }] }, { t: 'text', s: ' and ' }, { t: 'image', src: 'img/c.png', alt: 'a chart' },
  ]);
  assert.deepEqual(inline('<https://a.example/x> or https://b.example/y.'), [
    { t: 'link', href: 'https://a.example/x', c: [{ t: 'text', s: 'https://a.example/x' }] }, { t: 'text', s: ' or ' }, { t: 'link', href: 'https://b.example/y', c: [{ t: 'text', s: 'https://b.example/y' }] }, { t: 'text', s: '.' },
  ]);
  assert.deepEqual(inline('\\*not em\\* and `a **b**`'), [{ t: 'text', s: '*not em* and ' }, { t: 'code', s: 'a **b**' }]);
  assert.deepEqual(inline(''), []);
});

test('a document becomes headings, paragraphs, lists, quotes, tables, rules, pictures and fenced code', () => {
  const b = parse(['# Title', '', 'One line', 'and the next.', '', '- one', '  - two **deep**', '- [x] done', '- [ ] not done', '', '3. three', '4. four', '', '> quoted', '> more', '', '| a | b |', '| :-- | --: |', '| 1 | `x|y` |', '', '---', '', '![chart](img/c.png)', '', '```rust', 'let x = "**not bold**";', '```', ''].join('\n'));
  assert.deepEqual(b.map((x) => x.t), ['h', 'p', 'list', 'list', 'quote', 'table', 'hr', 'image', 'code']);
  assert.deepEqual(b[1], { t: 'p', c: [{ t: 'text', s: 'One line and the next.' }] });
  const ul = b[2] as Extract<typeof b[number], { t: 'list' }>;
  assert.equal(ul.ordered, false);
  assert.deepEqual(ul.items.map((i) => i.task), [null, true, false]);
  assert.equal((ul.items[0]!.c[1] as { t: string }).t, 'list', 'an indented item nests under the one above');
  const ol = b[3] as Extract<typeof b[number], { t: 'list' }>;
  assert.deepEqual([ol.ordered, ol.start, ol.items.length], [true, 3, 2]);
  assert.deepEqual((b[4] as { c: unknown[] }).c, [{ t: 'p', c: [{ t: 'text', s: 'quoted more' }] }]);
  const table = b[5] as Extract<typeof b[number], { t: 'table' }>;
  assert.deepEqual(table.align, ['left', 'right']);
  assert.deepEqual(table.rows, [[[{ t: 'text', s: '1' }], [{ t: 'code', s: 'x|y' }]]]);
  assert.deepEqual(b[8], { t: 'code', lang: 'rust', s: 'let x = "**not bold**";' });
  // A fence that never closes takes the rest as code; a comment is dropped; a line with a bar is not a table without its rule.
  assert.deepEqual(parse('```\nabc'), [{ t: 'code', lang: '', s: 'abc' }]);
  assert.deepEqual(parse('<!-- hidden -->\nshown'), [{ t: 'p', c: [{ t: 'text', s: 'shown' }] }]);
  assert.equal(parse('a | b\nc | d')[0]!.t, 'p');
});

test('nothing an agent or a document wrote is read as HTML', () => {
  const html = renderToStaticMarkup(<Rich text={'**<img src=x onerror=alert(1)>** and `<script>x</script>`\n\n<b>plain</b> <iframe src="x"></iframe>\n\n[click](javascript:alert(1)) ![p](javascript:alert(1))\n\n| <u>h</u> |\n| --- |\n| <s>c</s> |'} />);
  for (const bad of ['<img', '<script', '<b>', '<iframe', '<u>', '<s>', 'href=', 'javascript:alert(1)"']) assert.ok(!html.includes(bad), `${bad} in ${html}`);
  assert.ok(html.includes('<strong><span>&lt;img src=x onerror=alert(1)&gt;</span></strong>'));
  assert.ok(html.includes('<code>&lt;script&gt;x&lt;/script&gt;</code>'));
  // With nothing to follow it with, a link is text with its address as a title, and a picture is its words.
  assert.ok(html.includes('class="md-ref"') && html.includes('class="md-alt"'));
});

test('a link or a picture in a document resolves inside the workspace, and only a web address is a web address', () => {
  assert.equal(resolvePath('missions/m/report.md', 'img/chart.png'), 'missions/m/img/chart.png');
  assert.equal(resolvePath('missions/m/report.md', '../other/plan.md#part'), 'missions/other/plan.md');
  assert.equal(resolvePath('missions/m/report.md', './deck%20one.html?x=1'), 'missions/m/deck one.html');
  assert.equal(resolvePath('report.md', '../outside.md'), null, 'a path above the workspace is not followed');
  assert.equal(resolvePath('a/b.md', '/context/c.md'), 'context/c.md');
  for (const none of ['https://example.com/x.png', 'mailto:a@example.com', '#section', '//example.com/x', 'javascript:alert(1)', '']) assert.equal(resolvePath('a/b.md', none), null, none);
  assert.equal(resolvePath(undefined, 'img/c.png'), null);
  assert.ok(isWeb('https://example.com/a?b=c') && isWeb('http://example.com'));
  for (const no of ['javascript:alert(1)', 'file:///etc/hosts', 'https://exa mple.com', 'ftp://x', '']) assert.ok(!isWeb(no), no);
});

test('a picture is asked for at the size of the space it is drawn in, at most 1,600 pixels a side (spec 118 §4 item 3)', () => {
  assert.equal(PICTURE_SIDE_MAX, 1600);
  assert.equal(pictureSide(640, 1), 640);
  assert.equal(pictureSide(640, 2), 1280, 'in device pixels, so it is sharp on a dense screen');
  assert.equal(pictureSide(860.4, 1.5), 1291);
  assert.equal(pictureSide(1200, 2), 1600, 'never past the ceiling');
  for (const none of [0, -10, Number.NaN, Number.POSITIVE_INFINITY]) assert.equal(pictureSide(none, 2), 160, `nothing to measure (${none}): the daemon's default`);
  assert.equal(pictureSide(500, Number.NaN), 500, 'a nonsense ratio is 1');
  assert.equal(pictureSide(500, 40), 1600, 'a ratio is at most 4');
  assert.equal(pictureTarget('missions/m/report.md', 'img/a b.png?x=1#y', 900), '/v1/image?path=missions%2Fm%2Freport.md&src=img%2Fa%20b.png&max=900');
});

test('a Mermaid block is its text until the window draws it, and its text is never HTML (spec 118 §5 item 4)', () => {
  const hostile = 'graph TD\n  A["<img src=x onerror=alert(1)>"] --> B\n  click A "javascript:alert(2)"';
  const html = renderToStaticMarkup(<Rich text={`Before\n\n\`\`\`Mermaid\n${hostile}\n\`\`\`\n\n\`\`\`js\nnot a diagram\n\`\`\``} />);
  assert.ok(html.includes('<figure class="md-diagram" aria-busy="true"><pre class="code"><span class="md-lang">mermaid</span>graph TD'), html);
  assert.ok(html.includes('&lt;img src=x onerror=alert(1)&gt;'));
  for (const bad of ['<img', '<svg', 'href=', '<script']) assert.ok(!html.includes(bad), `${bad} in ${html}`);
  assert.ok(html.includes('<pre class="code"><span class="md-lang">js</span>not a diagram</pre>'), 'another language is code, as before');
  assert.ok(isMermaid(' mermaid ') && isMermaid('MERMAID') && !isMermaid('mermaidjs') && !isMermaid(''));
});

test('Mermaid runs strict, and a diagram cannot change that or anything that decides how its labels are cleaned', () => {
  const c = mermaidConfig(true);
  assert.equal(c.securityLevel, 'strict');
  assert.equal(c.startOnLoad, false);
  assert.equal(c.htmlLabels, false);
  assert.equal(mermaidConfig(false).theme, 'default');
  for (const k of ['securityLevel', 'secure', 'dompurifyConfig', 'htmlLabels', 'flowchart', 'themeCSS', 'maxTextSize']) assert.ok((c.secure as string[]).includes(k), k);
  // A diagram whose text could make a style load something is shown as its text, not drawn.
  for (const t of ['style A fill:url(http://x/y)', 'classDef c background:image-set("http://x" 1x)', 'style A fill:\\75rl(http://x)', '{"primaryColor": "\\u0075rl(x)"}', 'a: "\\x75rl(x)"', '@import "x";', '@font-face{}']) assert.ok(asksToLoad(t), t);
  for (const t of ['graph TD\n  A-->B', 'style A fill:#f9f,stroke:#333', 'A["a (b) url"]', 'A["C:\\\\path"]']) assert.ok(!asksToLoad(t), t);
  assert.equal(STILL_CSS, '*{animation:none!important;transition:none!important}');
});
