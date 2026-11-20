// Asking from a page (spec 120 §4.2, §6; slice K5). A window whose key holds the `request` grant may ask the daemon to
// start four things; this file says what each request carries, whether this window may ask, and where a request stands.
//
// The rule every function here keeps: a request is made of ids (a topic's id, a page's slug, a claim's id, a section's
// anchor) and of what the operator typed into a field. It never carries a heading, a statement, a quotation or any
// other text taken from a page, a source or an agent. The builders below take the typed words as their own argument and
// have no parameter through which page text could arrive.
import { NOTE_MAX } from './ency.ts';
import type { Conn, Envelope } from './types.ts';

export type AskWhat = 'research.continue' | 'research.ask' | 'page.rebuild' | 'page.dispute' | 'update.apply';
/** The fields a request may carry; the bridge passes on only the ones its request takes (`src-tauri/src/ask.rs`). */
export type AskArg = { topic_id?: string; section?: string; note?: string; question?: string; follows?: string; slug?: string; claim?: string };
export type Request = { what: AskWhat; arg: AskArg };
/** What the daemon answers a request it counted: its id and where it stands. */
export type Asked = { request?: string; state?: string };

/** The most a follow-up question may hold: what the console's `/research` takes. */
export const QUESTION_MAX = 200;
/** The most a dispute's note may hold. */
export const DISPUTE_MAX = 500;

const SLUG = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const ID = /^[A-Za-z0-9][A-Za-z0-9._-]*$/;
/** A heading's anchor as the daemon makes one: lowercase letters, digits and hyphens. */
const ANCHOR = /^[\p{Ll}\p{Lo}\p{Lm}\p{N}-]{1,80}$/u;

/** What was typed, as one line of at most `max` characters. */
export function typedLine(typed: string, max: number): string {
  return Array.from(typed.replace(/[\p{Cc}\p{Zl}\p{Zp}]+/gu, ' ').replace(/\s+/g, ' ').trim()).slice(0, max).join('');
}

/** How many characters of a note (at most `NOTE_MAX`, what `delphi research continue --note` takes) are left once the daemon has named the section in it (`… (section: <anchor>)`). */
export const sectionNoteRoom = (anchor: string): number => Math.max(0, NOTE_MAX - Array.from(` (section: ${anchor})`).length);

/** "Rebuild now": the page's slug. */
export const rebuildRequest = (slug: string): Request | null => (SLUG.test(slug) && slug.length <= 60 ? { what: 'page.rebuild', arg: { slug } } : null);

/** "Run it again": a further run on the topic, with nothing added. */
export const againRequest = (topicId: string): Request | null => (SLUG.test(topicId) && topicId.length <= 60 ? { what: 'research.continue', arg: { topic_id: topicId } } : null);

/** "Ask a follow-up": the question the operator typed, researched in the project of the topic it follows. */
export function followUpRequest(topicId: string, typed: string): Request | null {
  const question = typedLine(typed, QUESTION_MAX);
  return SLUG.test(topicId) && topicId.length <= 60 && question ? { what: 'research.ask', arg: { question, follows: topicId } } : null;
}

/**
 * "Research this section further": the topic's id, the section's anchor, and what the operator typed (which may be
 * nothing). The section's heading is not sent: the daemon finds the heading the anchor names on the page it holds.
 */
export function sectionRequest(topicId: string, anchor: string, typed: string): Request | null {
  if (!SLUG.test(topicId) || topicId.length > 60 || !ANCHOR.test(anchor)) return null;
  const note = typedLine(typed, sectionNoteRoom(anchor));
  return { what: 'research.continue', arg: note ? { topic_id: topicId, section: anchor, note } : { topic_id: topicId, section: anchor } };
}

/** "Dispute this statement": the claim's id and the operator's words, which a dispute needs. */
export function disputeRequest(claimId: string, typed: string): Request | null {
  const note = typedLine(typed, DISPUTE_MAX);
  return ID.test(claimId) && !claimId.includes('..') && claimId.length <= 120 && note ? { what: 'page.dispute', arg: { claim: claimId, note } } : null;
}

/** "Update now" (spec 120 §13.2): the one request of the `update` grant. It takes no parameters at all: not a version, not a channel. */
export const updateRequest = (): Request => ({ what: 'update.apply', arg: {} });

/** Whether this window may ask for an update: attached with a key it proved, and that key holds `update`. */
export const canUpdate = (conn: Conn): boolean => conn.connected && !!conn.paired && (conn.grants ?? []).includes('update');

/** Whether this window may ask: attached with a key it proved, and that key holds `request` (spec 120 §6 item 1). */
export const canAsk = (conn: Conn): boolean => conn.connected && !!conn.paired && (conn.grants ?? []).includes('request');

/**
 * The title row (spec 120 §6 item 2): whether the window is read-only or paired, and the grants it holds. `why` is the
 * reason a window that has a key is reading without it (the daemon refused the key, the key could not be read); it is
 * shown while connected, since that is when the operator can do something about it.
 */
export function connWords(conn: Conn, fixture: boolean): { text: string; why: string } {
  if (!conn.connected || fixture) return { text: conn.detail, why: '' };
  if (conn.paired) {
    const grants = (conn.grants ?? []).filter((g) => /^[a-z]{1,20}$/.test(g));
    return { text: `daemon connected · paired as ${conn.paired} · ${grants.length ? grants.join(', ') : 'no grants'}`, why: '' };
  }
  return { text: 'daemon connected · read-only', why: (conn.unpaired_because ?? '').trim() };
}

/** Where one request stands, from the daemon's `app.request` events. */
export type RequestState = { request: string; method: string; state: string; why: string; ids: Record<string, unknown> };

const STATES = ['waiting', 'queued', 'running', 'done', 'failed', 'cancelled', 'refused'];

/** Each request's latest state, from the events the window already reads (spec 120 §6 item 3). */
export function requestStates(events: Envelope[]): Map<string, RequestState> {
  const out = new Map<string, RequestState>();
  for (const e of events) {
    if (e.type !== 'app.request') continue;
    const p = e.payload;
    const [request, state] = [p.request, p.state];
    if (typeof request !== 'string' || typeof state !== 'string' || !STATES.includes(state)) continue;
    const ids = p.ids && typeof p.ids === 'object' && !Array.isArray(p.ids) ? (p.ids as Record<string, unknown>) : {};
    out.set(request, { request, method: typeof p.method === 'string' ? p.method : '', state, why: typeof p.why === 'string' ? typedLine(p.why, 200) : '', ids });
  }
  return out;
}

/** Whether a request has ended, one way or another. */
export const ended = (state: string): boolean => state === 'done' || state === 'failed' || state === 'cancelled' || state === 'refused';

/**
 * A request's state in words. `waits` is how many approval cards wait on the operator: a request that is queued or
 * running while one waits may be the thing waiting, and the card is answered in the console, never here.
 */
export function stateWords(s: { state: string; why?: string }, waits = 0): string {
  const why = s.why ? ` ${s.why.replace(/[.\s]+$/, '')}.` : '';
  const card = waits > 0 ? ` ${waits === 1 ? 'A decision is' : `${waits} decisions are`} waiting on you in the console; this window cannot answer it.` : '';
  switch (s.state) {
    case 'sending': return 'Asking the daemon.';
    case 'waiting': return 'Waiting for approval at your terminal: run `delphi app approve` and type the version shown there. This window cannot approve it.';
    case 'queued': return `Queued: the daemon will start it when the run before it ends.${card}`;
    case 'running': return `Running.${card}`;
    case 'done': return 'Done.';
    case 'failed': return `It failed.${why}`;
    case 'cancelled': return 'It was cancelled.';
    case 'refused': return `Refused.${why}`;
    default: return 'Asked.';
  }
}

/** What the daemon or the bridge said when it would not take a request, as one sentence to show. */
export function refusalWords(e: unknown): string {
  const said = typedLine(e instanceof Error ? e.message : String(e), 300).replace(/^the daemon refused \(-?\d+\):\s*/, '');
  return said || 'the daemon did not take the request';
}
