// A model against its role's default (`specs/035` D-14..D-16). The tier on a row is the tier of the model that actually served the call;
// the default is the tier the role starts on. ▲ is above it (the orchestrator raised the role for this task, or an upscale on the retry
// ladder), ▼ below it (a lesser stand-in, or the orchestrator lowered it). Nothing when they match or when either is unknown.
import * as T from './theme.ts';

const RANK: Record<string, number> = { fast: 0, workhorse: 1, frontier: 2 };

export type Versus = 'above' | 'below' | 'at' | '';
export function versus(tier: string | undefined, def: string | undefined): Versus {
  if (!tier || !def || RANK[tier] === undefined || RANK[def] === undefined) return '';
  return RANK[tier]! > RANK[def]! ? 'above' : RANK[tier]! < RANK[def]! ? 'below' : 'at';
}

/** `▲` or `▼`, painted, or an empty string. */
export function mark(tier: string | undefined, def: string | undefined): string {
  const v = versus(tier, def);
  return v === 'above' ? T.accent('▲') : v === 'below' ? T.warn('▼') : '';
}

/** In words, for the part of a row that has room: `above its default (workhorse)`. */
export function words(tier: string | undefined, def: string | undefined): string {
  const v = versus(tier, def);
  return v === 'above' ? `above its default (${def})` : v === 'below' ? `below its default (${def})` : '';
}
