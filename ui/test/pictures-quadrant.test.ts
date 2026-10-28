// Quadrant pictures (spec 029 §11): the cell choice, the grid, and the setting that picks quadrant or half blocks.
import test from 'node:test';
import assert from 'node:assert/strict';
import chalk from 'chalk';
import { paint, pictureStyle, quadrantCell, styleOf, usePictureStyle, type Raster } from '../src/image.ts';
import { renderMarkdownLinks } from '../src/markdown.ts';
import { currentCapabilities, useCapabilities, type Px } from '../src/theme.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const RED: Px = [255, 0, 0, 255], BLUE: Px = [0, 0, 255, 255], CLEAR: Px = [0, 0, 0, 0];

function withColour<T>(f: () => T): T {
  const [prev, level, was] = [currentCapabilities(), chalk.level, pictureStyle()];
  useCapabilities({ color: 'truecolor', attributes: true });
  try { return f(); } finally { useCapabilities(prev); chalk.level = level; usePictureStyle(was); }
}

test('quadrant is the default, and the words that name a style are forgiving', () => {
  assert.equal(pictureStyle(), 'quadrant', 'with DELPHI_PICTURE unset');
  for (const w of ['half', 'HALF', ' half-block ']) assert.equal(styleOf(w), 'half', w);
  for (const w of ['quadrant', 'Quadrant', '', undefined, 'sextant', 'half-width']) assert.equal(styleOf(w), 'quadrant', String(w));
});

test('a cell of one colour is a space on that background, so it has no glyph to leave a seam', () => {
  withColour(() => {
    const c = quadrantCell([RED, RED, RED, RED]);
    assert.equal(plain(c), ' ');
    assert.ok(c.includes('48;2;255;0;0') && !c.includes('38;'), JSON.stringify(c));
  });
});

test('the glyph is the split that fits: left and right, top and bottom, one corner, the lower right on its own', () => {
  withColour(() => {
    const cases: [string, Px[], string][] = [
      ['left red, right blue', [RED, BLUE, RED, BLUE], '▌'],
      ['top red, bottom blue', [RED, RED, BLUE, BLUE], '▀'],
      ['upper left red', [RED, BLUE, BLUE, BLUE], '▘'],
      ['lower right red (it is the odd one out, so the three others are the glyph)', [BLUE, BLUE, BLUE, RED], '▛'],
      ['diagonal (the lower right stays the background, so blue is the glyph: the mirror of the other way round)', [RED, BLUE, BLUE, RED], '▞'],
    ];
    for (const [what, q, glyph] of cases) {
      const c = quadrantCell(q);
      assert.equal(plain(c), glyph, what);
      assert.ok(c.includes('38;2;') && c.includes('48;2;'), `${what}: a colour for the glyph and one behind it`);
    }
    const left = quadrantCell([RED, BLUE, RED, BLUE]);
    assert.ok(left.includes('38;2;255;0;0') && left.includes('48;2;0;0;255'), 'the glyph is the first group, the background the other');
  });
});

test('transparent pixels are left to the terminal: the glyph covers the opaque ones and nothing sits behind it', () => {
  withColour(() => {
    assert.equal(quadrantCell([CLEAR, CLEAR, CLEAR, CLEAR]), ' ');
    const corner = quadrantCell([RED, CLEAR, CLEAR, CLEAR]);
    assert.equal(plain(corner), '▘');
    assert.ok(corner.includes('38;2;255;0;0') && !corner.includes('48;'), JSON.stringify(corner));
    const right = quadrantCell([CLEAR, [10, 20, 30, 255], CLEAR, [30, 40, 50, 255]]);
    assert.equal(plain(right), '▐');
    assert.ok(right.includes('38;2;20;30;40'), 'the covered pixels are averaged');
  });
});

test('the grid is the half-block grid twice as wide: the same cells across and down', () => {
  withColour(() => {
    const big: Raster = { width: 160, height: 40, rgba: Uint8Array.from({ length: 160 * 40 * 4 }, (_, i) => (i % 4 === 3 ? 255 : 7)) };
    const [q, h] = [paint(big, 30, 'quadrant'), paint(big, 30, 'half')];
    assert.equal(q.length, h.length);
    assert.equal(plain(q[0]!).length, plain(h[0]!).length);
    assert.equal(plain(q[0]!).length, 30);
  });
});

test('quadrants keep detail across the cell that half blocks average away', () => {
  withColour(() => {
    // 6x4 with columns alternating red and blue: shrunk to 3 cells wide, quadrant keeps every column, half blends pairs
    const rgba: number[] = [];
    for (let y = 0; y < 4; y++) for (let x = 0; x < 6; x++) rgba.push(...(x % 2 === 0 ? RED : BLUE));
    const stripes: Raster = { width: 6, height: 4, rgba: Uint8Array.from(rgba) };
    const q = paint(stripes, 3, 'quadrant');
    assert.equal(q.map(plain).join('|'), '▌▌▌|▌▌▌'.slice(0, 3), 'one row of cells, each a red half beside a blue half');
    assert.ok(q[0]!.includes('38;2;255;0;0') && q[0]!.includes('48;2;0;0;255'));
    const h = paint(stripes, 3, 'half')[0]!;
    assert.ok(h.includes('38;2;127;0;127'), 'half blocks blend them to purple: ' + JSON.stringify(h));
  });
});

test('the reader draws in the current style; /pictures changes it without a restart', () => {
  withColour(() => {
    const flat: Raster = { width: 8, height: 4, rgba: Uint8Array.from({ length: 8 * 4 * 4 }, (_, i) => [10, 20, 30, 255][i % 4]!) };
    const draw = () => renderMarkdownLinks('![p](img/a.png)', 60, { images: { 'img/a.png': flat } }).lines;
    usePictureStyle('quadrant');
    const q = draw();
    assert.ok(q.some((l) => l.includes('48;2;10;20;30')) && !q.some((l) => plain(l).includes('▀')), 'flat cells are backgrounds');
    usePictureStyle('half');
    assert.ok(draw().some((l) => plain(l).includes('▀')), 'half blocks are glyphs');
  });
});
