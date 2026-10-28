// Pictures in the reader (spec 029 §9): half-block painting, the shrink, and what stands in when one cannot be drawn.
import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import { fitRaster, MAX_PICTURE_ROWS, paint, rasterOf, type Raster } from '../src/image.ts';
import { emptyArch, gotDoc, gotImage, layout } from '../src/archives.ts';
import { imageRefs, renderMarkdown, renderMarkdownLinks } from '../src/markdown.ts';
import { currentCapabilities, useCapabilities } from '../src/theme.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const SGR = /\x1b\[([0-9;]*)m/g;

/** Run `f` with the colour depth asked for, and put the old one back. */
function withColour<T>(color: 'none' | 'truecolor', f: () => T): T {
  const [prev, level] = [currentCapabilities(), chalk.level];
  useCapabilities({ color, attributes: true });
  try { return f(); } finally { useCapabilities(prev); chalk.level = level; }
}

const solid = (w: number, h: number, c: [number, number, number, number]): Raster => ({ width: w, height: h, rgba: Uint8Array.from({ length: w * h * 4 }, (_, i) => c[i % 4]!) });

test('a picture is painted two pixels to a cell: the upper as the glyph colour, the lower as the background', () => {
  withColour('truecolor', () => {
    const r: Raster = { width: 1, height: 2, rgba: Uint8Array.from([255, 0, 0, 255, 0, 0, 255, 255]) };
    const [row] = paint(r, 10);
    assert.equal(plain(row!), '▀');
    assert.ok(row!.includes('38;2;255;0;0') && row!.includes('48;2;0;0;255'), JSON.stringify(row));
  });
});

test('transparent pixels leave the terminal background showing, in every combination', () => {
  withColour('truecolor', () => {
    const [clear, red] = [[0, 0, 0, 0], [255, 0, 0, 255]];
    const cell = (a: number[], b: number[]) => paint({ width: 1, height: 2, rgba: Uint8Array.from([...a, ...b]) }, 4)[0]!;
    assert.equal(cell(clear, clear), ' ');
    assert.equal(plain(cell(red, clear)), '▀');
    assert.equal(plain(cell(clear, red)), '▄');
    assert.ok(![...cell(red, clear).matchAll(SGR)].some((m) => m[1]!.startsWith('48')), 'no background colour where the lower pixel is clear');
  });
});

test('an odd number of pixel rows ends in a half cell, not a missing row', () => {
  withColour('truecolor', () => {
    assert.equal(paint(solid(3, 3, [9, 9, 9, 255]), 10).length, 2);
  });
});

test('a picture is never wider than the window nor taller than the cap, and keeps its proportions', () => {
  withColour('truecolor', () => {
    const wide = paint(solid(160, 40, [1, 2, 3, 255]), 30);
    assert.equal(plain(wide[0]!).length, 30);
    assert.equal(wide.length, 4, '160x40 at 30 wide is 30x8 pixels: 4 cell rows');
    const tall = paint(solid(40, 160, [1, 2, 3, 255]), 80);
    assert.equal(tall.length, MAX_PICTURE_ROWS);
    assert.equal(plain(tall[0]!).length, 10, '40x160 at 40 pixels high is 10x40');
  });
});

test('shrinking averages what a pixel covers and weighs colour by opacity; it never enlarges', () => {
  const two: Raster = { width: 2, height: 1, rgba: Uint8Array.from([0, 0, 0, 255, 200, 100, 50, 255]) };
  assert.deepEqual([...fitRaster(two, 1, 1).rgba], [100, 50, 25, 255]);
  const mixed: Raster = { width: 2, height: 1, rgba: Uint8Array.from([255, 0, 0, 0, 0, 0, 255, 255]) };
  assert.deepEqual([...fitRaster(mixed, 1, 1).rgba].slice(0, 3), [0, 0, 255]);
  const small = solid(4, 4, [1, 1, 1, 255]);
  assert.equal(fitRaster(small, 100, 100), small);
});

test('the daemon\'s reply is accepted only if its pixels are as many as it says', () => {
  const ok = { width: 2, height: 1, rgba: Buffer.from([1, 2, 3, 255, 4, 5, 6, 255]).toString('base64') };
  assert.deepEqual(rasterOf(ok)?.width, 2);
  for (const bad of [null, {}, { ...ok, width: 3 }, { ...ok, rgba: 'AAAA' }, { ...ok, width: 0 }, { ...ok, width: 5000 }, { ...ok, rgba: 7 }]) assert.equal(rasterOf(bad), null, JSON.stringify(bad));
});

test('only relative image files are asked for: once each, without fragments, never the web or an absolute path', () => {
  const md = '![a](img/a.png)\n\n![again](img/a.png#x)\n\n![b](b.jpg?v=1)\n\n![web](https://example.com/c.png)\n\n![abs](/etc/d.png)\n\n![odd](file:///e.png)\n\n```\n![code](f.png)\n```';
  assert.deepEqual(imageRefs(md), ['img/a.png', 'b.jpg']);
});

const pic = solid(8, 4, [10, 20, 30, 255]);

test('a picture on a line of its own is drawn where it is written, and the text around it is kept', () => {
  withColour('truecolor', () => {
    const { lines } = renderMarkdownLinks('before\n\n![chart](img/a.png)\n\nafter', 60, { images: { 'img/a.png': pic } });
    const text = lines.map(plain);
    assert.deepEqual(text.filter((l) => l.trim()).map((l) => l.trim()).filter((l) => !/^▀+$/.test(l)), ['before', 'after']);
    assert.ok(lines.some((l) => l.includes('48;2;10;20;30')), 'the pixels are painted');
    assert.ok(!text.join('\n').includes('[image'), 'no note where the picture is shown');
  });
});

test('a picture that is not there, failed, or cannot be shown for lack of colour stands in as its alt text', () => {
  withColour('truecolor', () => {
    for (const images of [undefined, {}, { 'img/a.png': null }] as (Record<string, Raster | null> | undefined)[]) {
      assert.deepEqual(renderMarkdownLinks('![chart of x](img/a.png)', 60, { images }).lines.map(plain).filter(Boolean), ['[image: chart of x]']);
    }
    assert.deepEqual(renderMarkdown('![chart of x](img/a.png)', 60).map(plain).filter(Boolean), ['[image: chart of x]'], 'outside the reader nothing is fetched');
  });
  withColour('none', () => {
    assert.deepEqual(renderMarkdownLinks('![chart of x](img/a.png)', 60, { images: { 'img/a.png': pic } }).lines.filter(Boolean).map(plain), ['[image: chart of x]'], 'NO_COLOR draws no picture');
  });
});

test('a web image is a note with its address and is not drawn even if someone supplies pixels for it', () => {
  withColour('truecolor', () => {
    const out = renderMarkdownLinks('![logo](https://example.com/l.png)', 80, { images: { 'https://example.com/l.png': pic } }).lines.map(plain).filter(Boolean);
    assert.deepEqual(out, ['[image: logo <https://example.com/l.png>]']);
  });
});

test('an image inside a sentence is a note in the sentence, not a picture', () => {
  withColour('truecolor', () => {
    const out = renderMarkdownLinks('See ![the chart](img/a.png) above.', 80, { images: { 'img/a.png': pic } }).lines.map(plain).filter(Boolean);
    assert.deepEqual(out, ['See [image: the chart] above.']);
  });
});

test('links beside a picture keep their rows and columns', () => {
  withColour('truecolor', () => {
    const { lines, links } = renderMarkdownLinks('![p](img/a.png)\n\nsee [beta](beta.md)', 60, { base: 'specs/a.md', images: { 'img/a.png': pic } });
    assert.equal(links.length, 1);
    const s = links[0]!.spans[0]!;
    assert.equal(plain(lines[s.line]!).slice(s.col, s.col + s.width), 'beta');
  });
});

test('the reader gets a picture after the text, and shows it where the note was', () => {
  withColour('truecolor', () => {
    let a = gotDoc(emptyArch(), { path: 'specs/a.md', text: '# T\n\n![chart](img/a.png)\n' }, null, 80);
    const before = layout(a.trail[0]!, 80).lines;
    assert.ok(before.map(plain).includes('[image: chart]'));
    a = gotImage(a, 'specs/a.md', 'img/a.png', pic);
    const after = layout(a.trail[0]!, 80).lines;
    assert.ok(!after.map(plain).includes('[image: chart]') && after.some((l) => l.includes('48;2;10;20;30')));
    a = gotImage(a, 'specs/other.md', 'img/a.png', null);
    assert.equal(a.trail[0]!.images?.['img/a.png'], pic, 'another document\'s picture does not touch this one');
  });
});
