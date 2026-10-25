// Keys and the help overlay (`005e` §10.1): `?` on an empty prompt opens it; `?` and Esc close it; paging does nothing
// (it never scrolls the transcript); any other key closes it and is then handled normally.
import type { Ev } from './input.ts';

export type HelpAction =
  | 'open'            // show the overlay; the key is consumed
  | 'close'           // hide it; the key is consumed
  | 'ignore'          // leave it open; the key is consumed
  | 'close-and-pass'  // hide it, then handle the key as usual
  | 'pass';           // not for the overlay

export function helpKey(open: boolean, e: Ev, promptEmpty: boolean, onChatDeck: boolean): HelpAction {
  if (!onChatDeck) return 'pass';
  if (!open) return e.t === 'text' && e.s === '?' && promptEmpty ? 'open' : 'pass'; // `?` inside text types `?`
  if (e.t === 'bg') return 'pass';
  if ((e.t === 'text' && e.s === '?') || (e.t === 'key' && e.k === 'esc')) return 'close';
  if (e.t === 'wheel' || (e.t === 'key' && (e.k === 'pgup' || e.k === 'pgdn'))) return 'ignore';
  return 'close-and-pass';
}
