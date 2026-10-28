// Pictures in the reader (`specs/029` §9), drawn as half-block cells: pure data in, lines out. The daemon decodes the
// file and sends at most 160 pixels a side (`/v1/image`); this shrinks it to the window and paints it.
import { pixelCell, type Px } from './theme.ts';

/** Pixels, row by row, four bytes each (red, green, blue, alpha). */
export type Raster = { width: number; height: number; rgba: Uint8Array };

/** A raster from the daemon's reply (`{width, height, rgba}`, the pixels in base64); null when it is not one. */
export function rasterOf(body: any): Raster | null {
  const [width, height] = [Number(body?.width), Number(body?.height)];
  if (!Number.isInteger(width) || !Number.isInteger(height) || width < 1 || height < 1 || width > 512 || height > 512 || typeof body?.rgba !== 'string') return null;
  const rgba = new Uint8Array(Buffer.from(body.rgba, 'base64'));
  return rgba.length === width * height * 4 ? { width, height, rgba } : null;
}

/** `r` shrunk to fit `maxW` x `maxH` pixels, proportions kept, never enlarged; each pixel averages what it covers, weighted by alpha. */
export function fitRaster(r: Raster, maxW: number, maxH: number): Raster {
  if (r.width <= maxW && r.height <= maxH) return r;
  const scale = Math.min(maxW / r.width, maxH / r.height);
  const [w, h] = [Math.max(1, Math.round(r.width * scale)), Math.max(1, Math.round(r.height * scale))];
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

/**
 * `r` as rows of half-block cells, at most `cols` wide and `MAX_PICTURE_ROWS` high (a cell is one pixel wide and two
 * tall). A cell is twice as tall as it is wide, so the pixel grid keeps the picture's own proportions.
 */
export function paint(r: Raster, cols: number): string[] {
  const f = fitRaster(r, Math.max(1, cols), MAX_PICTURE_ROWS * 2);
  const px = (x: number, y: number): Px => {
    if (y >= f.height) return [0, 0, 0, 0];
    const i = (y * f.width + x) * 4;
    return [f.rgba[i]!, f.rgba[i + 1]!, f.rgba[i + 2]!, f.rgba[i + 3]!];
  };
  const rows: string[] = [];
  for (let y = 0; y < f.height; y += 2) {
    let row = '';
    for (let x = 0; x < f.width; x++) row += pixelCell(px(x, y), px(x, y + 1));
    rows.push(row);
  }
  return rows;
}
