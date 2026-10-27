// Moving between the three decks (left: knowledge base, centre: chat, right: decision inbox) with the arrow keys.
import type { Ev } from './input.ts';

export type Deck = 0 | 1 | 2;

/**
 * The deck an arrow key leads to, or `null` when the key is not for navigation.
 *
 * `←` and `→` with nothing typed in the prompt move one deck; with text in the prompt they move the cursor, as in any
 * editor. `Shift+←` and `Shift+→` always move a deck (the spec's keys, and the way to leave a half-written message
 * alone). Alt and Ctrl combinations are not used here.
 */
export function deckTarget(deck: Deck, e: Ev, promptEmpty: boolean): Deck | null {
  if (e.t !== 'key' || (e.k !== 'left' && e.k !== 'right') || e.alt || e.ctrl) return null;
  if (!e.shift && !promptEmpty) return null;
  return Math.max(0, Math.min(2, deck + (e.k === 'left' ? -1 : 1))) as Deck;
}
