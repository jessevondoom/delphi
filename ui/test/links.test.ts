// Links as data (spec 029): targets, plain-text pointers, spans after wrapping, focus.
import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import stringWidth from 'string-width';
import { renderMarkdown, renderMarkdownLinks } from '../src/markdown.ts';
import { copyText, describe, firstVisible, isPointerText, normalize, parseTarget, POINTER_RE, stepFocus } from '../src/links.ts';
import { currentCapabilities, useCapabilities } from '../src/theme.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

test('targets: documents, passages, the web; relative links resolve against the document and cannot leave the tree', () => {
  assert.deepEqual(parseTarget('beta.md#intro', 'specs/alpha.md'), { k: 'doc', path: 'specs/beta.md', anchor: 'intro' });
  assert.deepEqual(parseTarget('../REVIEW.md', 'specs/alpha.md'), { k: 'doc', path: 'REVIEW.md' });
  assert.deepEqual(parseTarget('#top', 'specs/alpha.md'), { k: 'doc', path: 'specs/alpha.md', anchor: 'top' });
  assert.deepEqual(parseTarget('context/x.md#L51-L68'), { k: 'pointer', path: 'context/x.md', from: 51, to: 68 });
  assert.deepEqual(parseTarget('context/x.md#L7'), { k: 'pointer', path: 'context/x.md', from: 7, to: 7 });
  assert.deepEqual(parseTarget('https://example.com/a?b=1'), { k: 'external', url: 'https://example.com/a?b=1' });
  assert.equal(parseTarget('../../etc/passwd.md', 'specs/a.md'), null, 'leaves the tree');
  assert.equal(parseTarget('javascript:alert(1)'), null);
  assert.equal(parseTarget('file:///etc/passwd'), null);
  assert.equal(parseTarget('image.png', 'specs/a.md'), null, 'only Markdown opens in the reader');
  assert.equal(parseTarget('#x'), null, 'an anchor needs a document');
  assert.equal(normalize('a/./b/../c'), 'a/c');
  assert.equal(normalize('../a'), null);
});

test('plain-text pointers: a directory and a .md file, not prose, URLs or bare file names', () => {
  const found = (s: string) => [...s.matchAll(POINTER_RE)].map((m) => m[0]);
  assert.deepEqual(found('see specs/004-x.md and context/lessons/a.md#L10-L20, or `memory/b.md#sec`.'), ['specs/004-x.md', 'context/lessons/a.md#L10-L20', 'memory/b.md#sec']);
  assert.deepEqual(found('read README.md first'), []);
  assert.deepEqual(found('https://example.com/docs/page.md is the web'), []);
  assert.deepEqual(found('user@host/docs/a.md'), []);
  assert.ok(isPointerText('specs/a.md') && !isPointerText('see specs/a.md'));
});

test('rendering finds links, in prose, in markdown links and in code spans, but not in fenced code', () => {
  const src = 'Read [the card](beta.md#intro) and specs/004-x.md, or `context/a.md#L3-L9`. Also [web](https://example.com).\n\n```\nspecs/ignored.md\n```\n';
  const { lines, links } = renderMarkdownLinks(src, 100, { base: 'specs/alpha.md' });
  assert.deepEqual(links.map((l) => [l.label, l.target.k]), [['the card', 'doc'], ['specs/004-x.md', 'doc'], ['context/a.md#L3-L9', 'pointer'], ['web', 'external']]);
  assert.deepEqual(links.map((l) => l.id), [0, 1, 2, 3]);
  assert.ok(!lines.join('\n').includes('\x1b]8'), 'the markers are gone');
  const row = plain(lines[0]!);
  for (const l of links) {
    const [s] = l.spans;
    assert.equal(row.slice(s!.col, s!.col + s!.width), l.label, l.label);
  }
  assert.ok(plain(lines.join('\n')).includes('specs/ignored.md') && links.every((l) => l.label !== 'specs/ignored.md'));
  assert.equal(links[0]!.target.k === 'doc' && links[0]!.target.path, 'specs/beta.md');
});

test('a link that wraps has a span on each row, and the spans read back as the label', () => {
  const src = 'Before the link, [a rather long label that has to wrap onto more rows](gamma.md) and after.';
  const { lines, links } = renderMarkdownLinks(src, 24, { base: 'specs/alpha.md' });
  assert.equal(links.length, 1);
  const rows = lines.map(plain);
  assert.ok(links[0]!.spans.length >= 3, JSON.stringify(links[0]!.spans));
  const read = links[0]!.spans.map((s) => rows[s.line]!.slice(s.col, s.col + s.width)).join(' ').replace(/\s+/g, ' ').trim();
  assert.equal(read, 'a rather long label that has to wrap onto more rows');
  for (const l of lines) assert.ok(stringWidth(l) <= 24, l);
});

test('with no links the rendering is the same as renderMarkdown, line for line', () => {
  const src = '# Title\n\n- one\n- two with `code`\n\n> quoted\n\n```js\nconst x = 1;\n```\n\nA paragraph that is long enough to need wrapping when the width is narrow, like this one.';
  assert.deepEqual(renderMarkdownLinks(src, 30).lines, renderMarkdown(src, 30));
  assert.equal(renderMarkdownLinks(src, 30).links.length, 0);
});

test('focus draws one link reversed and nothing else changes', () => {
  const saved = currentCapabilities(), level = chalk.level;
  useCapabilities({ color: 'truecolor', attributes: true });
  try {
    const src = 'one specs/a.md and two specs/b.md';
    const plainLines = renderMarkdownLinks(src, 80).lines;
    const second = renderMarkdownLinks(src, 80, { focus: 1 }).lines[0]!;
    assert.ok(second.includes('\x1b[7m') && !plainLines[0]!.includes('\x1b[7m'), 'reverse video on the focused link only');
    assert.equal(plain(second), plain(plainLines[0]!), 'same text, same columns');
    assert.equal((second.match(/\x1b\[7m/g) ?? []).length, 1);
  } finally { useCapabilities(saved); chalk.level = level; }
});

test('moving focus: tab and shift+tab stop at the ends, and start from the visible links', () => {
  const links = [0, 1, 2].map((id) => ({ id, label: String(id), target: { k: 'external' as const, url: 'https://e.x' }, spans: [{ line: id * 10, col: 0, width: 1 }] }));
  assert.equal(stepFocus([], null, 1), null);
  assert.equal(stepFocus(links, null, 1), 0);
  assert.equal(stepFocus(links, null, -1), 2);
  assert.equal(stepFocus(links, 0, 1), 1);
  assert.equal(stepFocus(links, 2, 1), 2, 'stops at the end');
  assert.equal(stepFocus(links, 0, -1), 0);
  assert.equal(firstVisible(links, 5, 10), 1);
  assert.equal(firstVisible(links, 31, 5), null);
});

test('describing a target for the status line and for copying', () => {
  assert.equal(describe({ k: 'pointer', path: 'a/b.md', from: 3, to: 9 }), 'a/b.md L3-L9');
  assert.equal(describe({ k: 'doc', path: 'a/b.md', anchor: 'x' }), 'a/b.md § x');
  assert.equal(copyText({ k: 'pointer', path: 'a/b.md', from: 3, to: 9 }), 'a/b.md#L3-L9');
  assert.equal(copyText({ k: 'doc', path: 'a/b.md' }), 'a/b.md');
});
