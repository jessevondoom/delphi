// What a user perceives as one character. An emoji with a joiner, a letter with an accent written as two code points
// and a flag are each several code points but one keystroke to delete and one cell group to draw.
const seg = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

export const graphemes = (s: string): string[] => Array.from(seg.segment(s), (x) => x.segment);
