// Pictures in the reader (`specs/029` §9, §11), drawn in quadrant cells (2x2 pixels) or half-block cells (1x2): pure
// data in, lines out. The daemon decodes the file and sends at most 160 pixels a side (`/v1/image`); this shrinks it to
// the window and paints it.
import { paintedCell, pixelCell, type Px } from './theme.ts';

/** How a picture is drawn: quadrant blocks (the default; four pixels a cell) or half blocks (two pixels a cell). */
export type PictureStyle = 'quadrant' | 'half';
/** The style a word names (`DELPHI_PICTURE`, `/pictures`); anything unknown is the default. */
export const styleOf = (word: string | undefined): PictureStyle => (/^half(-block)?$/i.test((word ?? '').trim()) ? 'half' : 'quadrant');
let style: PictureStyle = styleOf(process.env.DELPHI_PICTURE);
export const pictureStyle = (): PictureStyle => style;
/** Adopt `s`: what `/pictures` does (and tests). The reader draws again the next time it is shown. */
export function usePictureStyle(s: PictureStyle): void { style = s; }

/** Pixels, row by row, four bytes each (red, green, blue, alpha). */
export type Raster = { width: number; height: number; rgba: Uint8Array };

/** A raster from the daemon's reply (`{width, height, rgba}`, the pixels in base64); null when it is not one. */
export function rasterOf(body: any): Raster | null {
  const [width, height] = [Number(body?.width), Number(body?.height)];
  if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1 || width > 512 || height > 512 || typeof body?.rgba !== 'string') return null;
  const rgba = new Uint8Array(Buffer.from(body.rgba, 'base64'));
  return rgba.length === width * height * 4 ? { width, height, rgba } : null;
}

/** The size `r` takes to fit `maxW` x `maxH` pixels with its proportions kept, never enlarged. */
export function fitSize(r: Raster, maxW: number, maxH: number): [number, number] {
  if (r.width <= maxW && r.height <= maxH) return [r.width, r.height];
  const scale = Math.min(maxW / r.width, maxH / r.height);
  return [Math.max(1, Math.round(r.width * scale)), Math.max(1, Math.round(r.height * scale))];
}

/** `r` shrunk to fit `maxW` x `maxH` pixels, proportions kept, never enlarged; each pixel averages what it covers, weighted by alpha. */
export function fitRaster(r: Raster, maxW: number, maxH: number): Raster {
  const [w, h] = fitSize(r, maxW, maxH);
  return w === r.width && h === r.height ? r : resample(r, w, h);
}

/** `r` at exactly `w` x `h`: each new pixel averages the old ones it covers (alpha-weighted), or repeats the one under it when enlarging. */
export function resample(r: Raster, w: number, h: number): Raster {
  const out = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) {
    const y0 = Math.floor((y * r.height) / h), y1 = Math.min(r.height, Math.max(y0 + 1, Math.floor(((y + 1) * r.height) / h)));
    for (let x = 0; x < w; x++) {
      const x0 = Math.floor((x * r.width) / w), x1 = Math.min(r.width, Math.max(x0 + 1, Math.floor(((x + 1) * r.width) / w)));
      let cr = 0, cg = 0, cb = 0, a = 0, n = 0;
      for (let yy = y0; yy < y1; yy++) for (let xx = x0; xx < x1; xx++) {
        const i = (yy * r.width + xx) * 4, al = r.rgba[i + 3]!;
        cr += r.rgba[i]! * al; cg += r.rgba[i + 1]! * al; cb += r.rgba[i + 2]! * al; a += al; n++;
      }
      const o = (y * w + x) * 4;
      if (a > 0) { out[o] = Math.floor(cr / a); out[o + 1] = Math.floor(cg / a); out[o + 2] = Math.floor(cb / a); out[o + 3] = Math.floor(a / n); }
    }
  }
  return { width: w, height: h, rgba: out };
}

/** The most rows a picture takes, so one never fills the screen. */
export const MAX_PICTURE_ROWS = 20;

/** The quadrant glyph for a set of covered quadrants: bit 1 upper left, 2 upper right, 4 lower left, 8 lower right. */
const QUADRANT = [' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█'];
type C3 = [number, number, number];

const mean = (ps: Px[]): C3 => [0, 1, 2].map((k) => Math.round(ps.reduce((a, p) => a + p[k]!, 0) / ps.length)) as C3;
const spread = (ps: Px[], m: C3): number => ps.reduce((a, p) => a + (p[0] - m[0]) ** 2 + (p[1] - m[1]) ** 2 + (p[2] - m[2]) ** 2, 0);

/**
 * One cell of four pixels (upper left, upper right, lower left, lower right). Transparent pixels (alpha under 128) are
 * left to the terminal's background: the glyph covers the opaque ones, in their mean colour. With all four opaque the cell
 * takes the split into two colours (of the seven there are) that fits best, as a glyph in the one colour over the other;
 * a cell the split does not improve, or that is one colour, is a space on a background, which cannot leave a seam.
 */
export function quadrantCell(q: Px[]): string {
  const opaque = q.map((p, i) => (p[3] >= 128 ? i : -1)).filter((i) => i >= 0);
  if (opaque.length === 0) return ' ';
  if (opaque.length < 4) return paintedCell(QUADRANT[opaque.reduce((m, i) => m | (1 << i), 0)]!, mean(opaque.map((i) => q[i]!)), null);
  const whole = mean(q), flat = spread(q, whole);
  const group = (mask: number, inside: boolean) => q.filter((_, i) => !!(mask & (1 << i)) === inside);
  let best = { mask: 0, error: flat };
  for (let mask = 1; mask < 8; mask++) { // the lower right pixel stays in the background group: a split and its mirror are one
    const [a, b] = [group(mask, true), group(mask, false)];
    const error = spread(a, mean(a)) + spread(b, mean(b));
    if (error < best.error) best = { mask, error };
  }
  if (best.mask === 0 || best.error > flat * 0.9) return paintedCell(' ', null, whole);
  return paintedCell(QUADRANT[best.mask]!, mean(group(best.mask, true)), mean(group(best.mask, false)));
}

/**
 * `r` as rows of cells, at most `cols` wide and `MAX_PICTURE_ROWS` high, in `how` (the current style by default). A cell
 * is about twice as tall as it is wide. Half-block style has 1x2 square pixels to the cell. Quadrant style has 2x2, each
 * half a cell wide and a whole pixel of the other style tall: the same picture area with twice the horizontal detail
 * (the vertical detail is the same), so its grid is the half-block grid made twice as wide.
 */
export function paint(r: Raster, cols: number, how: PictureStyle = style): string[] {
  const [cw, ch] = [how === 'quadrant' ? 2 : 1, 2];
  const [fw, fh] = fitSize(r, Math.max(1, cols), MAX_PICTURE_ROWS * 2);
  const f = resample(r, fw * cw, fh);
  const px = (x: number, y: number): Px => {
    if (x >= f.width || y >= f.height) return [0, 0, 0, 0];
    const i = (y * f.width + x) * 4;
    return [f.rgba[i]!, f.rgba[i + 1]!, f.rgba[i + 2]!, f.rgba[i + 3]!];
  };
  const rows: string[] = [];
  for (let y = 0; y < f.height; y += ch) {
    let row = '';
    for (let x = 0; x < f.width; x += cw) {
      row += how === 'quadrant'
        ? quadrantCell([px(x, y), px(x + 1, y), px(x, y + 1), px(x + 1, y + 1)])
        : pixelCell(px(x, y), px(x, y + 1));
    }
    rows.push(row);
  }
  return rows;
}
