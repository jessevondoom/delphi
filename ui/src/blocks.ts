// Tinted blocks (`005e` §7.2): a label row above, then the text rows in a soft fill capped by half-block rows (the bottom one is `▄` on the tint, since some fonts draw `▀` thin and gapped), with
// square edges. Where there is no tint (no colour, or a terminal that did not say what its background is) the block
// is a rail: no fill, no caps, each row starting with a dim `│`. Shared by the chat, the attention cards, the
// no-model card and the help overlay.
import stringWidth from 'string-width';
import * as T from './theme.ts';

/** The widest a block may be and the width its text wraps at: 80 % of the window; narrower windows give up less (`005e` §11). */
export function blockWidths(w: number): { max: number; text: number } {
  const max = w >= 48 ? Math.floor((w * 4) / 5) : w >= 40 ? w - 6 : Math.max(10, w - 2);
  return { max, text: max - 2 };
}

export const MIN_TEXT = 12;

/**
 * A block hugs its longest row, at least twelve text columns, never wider than `max`; a longer label sets the width.
 * `rows` carry their one column of left padding already, so the right padding is one more.
 */
export function hugWidth(rows: string[], label: string, max: number): number {
  const widest = Math.max(MIN_TEXT + 1, ...rows.map((r) => stringWidth(r)));
  return Math.min(max, Math.max(widest + 1, stringWidth(label)));
}

export type TintBlock = {
  /** Above the block, starting at its first column. */
  label: string;
  /** Text rows, each starting with its column of padding and no wider than `width - 1`. */
  rows: string[];
  width: number;
  /** Columns to the left of the block (flush right is `w - width`). */
  col: number;
  tint: T.Tint | null;
};

/**
 * The block without its label: a cap row, the text rows, a cap row, all `width` cells, square (`005e` §7.2). The council
 * cards use it (`weave.ts`). Without a tint it is the rail: no caps, each row `│` and its padding, padded to `width`.
 */
export function tintCard(rows: string[], width: number, tint: T.Tint | null): string[] {
  const fit = (r: string) => r + ' '.repeat(Math.max(0, width - stringWidth(r)));
  if (!tint) return rows.map((r) => fit(T.dim('│') + r));
  return [tint.ink('▄'.repeat(width)), ...rows.map((r) => tint.fill(fit(r))), tint.flip('▄'.repeat(width))];
}

export function tintBlock(o: TintBlock): string[] {
  const pad = ' '.repeat(Math.max(0, o.col));
  if (!o.tint) return [pad + o.label, ...o.rows.map((r) => pad + T.dim('│') + r)]; // the row's own padding column follows the rail
  const { fill, ink, flip } = o.tint;
  const row = (r: string) => pad + fill(r + ' '.repeat(Math.max(0, o.width - stringWidth(r))));
  return [pad + o.label, pad + ink('▄'.repeat(o.width)), ...o.rows.map(row), pad + flip('▄'.repeat(o.width))];
}
