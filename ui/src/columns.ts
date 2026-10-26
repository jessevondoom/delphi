// Two columns side by side (`specs/030` §6.5): the left rows padded to their width, a dim bar, then the right rows. Both are lists of
// already-coloured lines; widths are measured with the colour codes out. Pure.
import * as T from './theme.ts';

/** Does a window this wide have room for two columns? Below it a view stays one column. */
export const TWO_COLUMNS_AT = 116;

/** The width of the left column in a window `w` wide: about half, never more than 64, and the right column gets the rest less the bar. */
export const leftWidth = (w: number) => Math.min(64, Math.floor(w / 2));

/**
 * `left` and `right` joined into rows `w` wide. Rows past the shorter column are padded with blanks; a row longer than its column is
 * clipped to it, so neither column can push the other.
 */
export function joinColumns(left: string[], right: string[], w: number, pal: T.Pal): string[] {
  const lw = leftWidth(w), rw = Math.max(0, w - lw - 3);
  const bar = T.rule(pal)(' │ ');
  const rows = Math.max(left.length, right.length);
  const out: string[] = [];
  for (let i = 0; i < rows; i++) {
    const l = T.clip(left[i] ?? '', lw);
    const pad = ' '.repeat(Math.max(0, lw - T.width(l)));
    out.push(l + pad + bar + T.clip(right[i] ?? '', rw));
  }
  return out;
}
