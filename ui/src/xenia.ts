// `/xenia` in the console (`specs/011` §4.1, §7.3): the joined stores' status, and the steward's attestation card.
// Pure: what the operator typed in, the daemon's answers out as text. The daemon does the work (`xenia.status`,
// `xenia.attest.prepare`, `xenia.attest.decide`); this only words it. The card is the one the CLI shows: the exact bytes
// that will be signed, the flags and a short code. Nothing is signed until the operator types that code back, so an
// agent that prints "/xenia sign ..." cannot answer for them: only the operator's own keystrokes reach a slash command.
import { sanitize } from './theme.ts';

export const XENIA_ACTIONS = ['publish', 'endorse', 'dispute', 'retract', 'withdraw', 'restore', 'republish'] as const;

export type XeniaCmd =
  | { kind: 'status'; store?: string }
  | { kind: 'prepare'; params: { action: string; target?: string; options: Record<string, string>; store?: string } }
  | { kind: 'decide'; code: string; decision: 'sign' | 'cancel'; flags: boolean }
  | { kind: 'usage'; message: string };

export const USAGE =
  '/xenia · /xenia status [org/store]\n' +
  '/xenia endorse|dispute|retract|withdraw|restore <id> [note="why"] [reason=mistake] [scope=a,b]\n' +
  '/xenia publish <file> title="…" summary="…" promoted=true [tags=a,b] [type=finding]\n' +
  '/xenia republish old=<did>\n' +
  '/xenia sign <code> [flags] · /xenia cancel <code>      (the code is on the card)';

/** Words, with "double quoted" runs kept whole (a note has spaces). */
export function tokenize(s: string): string[] {
  const out: string[] = [];
  const re = /(?:[^\s"]+="[^"]*"|"[^"]*"|\S+)/g;
  for (const m of s.match(re) ?? []) out.push(m.replace(/^("?)([^"]*)\1$/, '$2').replace(/^([^=\s]+)="([^"]*)"$/, '$1=$2'));
  return out;
}

export function parseXenia(arg: string): XeniaCmd {
  const t = tokenize(arg.trim());
  const head = t[0] ?? 'status';
  if (head === 'status') return { kind: 'status', ...(t[1] ? { store: t[1] } : {}) };
  if (head === 'sign' || head === 'cancel') {
    if (!/^[0-9a-f]{12}$/.test(t[1] ?? '')) return { kind: 'usage', message: `/xenia ${head} needs the 12-character code on the card\n${USAGE}` };
    return { kind: 'decide', code: t[1]!, decision: head, flags: t.slice(2).includes('flags') };
  }
  if ((XENIA_ACTIONS as readonly string[]).includes(head)) {
    const options: Record<string, string> = {};
    let target: string | undefined;
    for (const w of t.slice(1)) {
      const i = w.indexOf('=');
      if (i > 0 && /^[a-z-]+$/.test(w.slice(0, i))) options[w.slice(0, i)] = w.slice(i + 1);
      else if (!target) target = w;
      else return { kind: 'usage', message: `unexpected \`${w}\` (put spaces inside quotes: note="the retry window is wrong")\n${USAGE}` };
    }
    const store = options['store'];
    delete options['store'];
    return { kind: 'prepare', params: { action: head, ...(target ? { target } : {}), options, ...(store ? { store } : {}) } };
  }
  return { kind: 'usage', message: `unknown /xenia action \`${head}\`\n${USAGE}` };
}

/** `xenia.status`: one block per joined store, as the CLI prints it. */
export function statusText(r: any): string {
  const stores: any[] = Array.isArray(r?.stores) ? r.stores : [];
  if (!stores.length) return 'no Xenia store is joined (delphi xenia join <url> ... in a terminal)';
  return stores.map((s) => sanitize(String(s.text ?? '')).trimEnd()).join('\n\n');
}

/** `xenia.attest.prepare`: the card, and how to answer it. */
export function cardText(r: any): string {
  if (!r?.ok) return `nothing to sign: ${sanitize(String(r?.message ?? r?.reason ?? 'refused'))}`;
  const flags = Number(r.flags) || 0;
  const answer = `Answer with /xenia sign ${r.code}${flags ? ' flags' : ''} to sign exactly this, or /xenia cancel ${r.code}. The card expires in ${Math.round((Number(r.expires_secs) || 900) / 60)} minutes.`;
  return `${sanitize(String(r.card ?? '')).trimEnd()}\n\n${flags ? `${flags} flag(s) above do not block; "flags" says you read them.\n` : ''}${answer}`;
}

/** `xenia.attest.decide`: what happened. */
export function resultText(r: any): string {
  if (r?.ok) return `${sanitize(String(r.action ?? 'published'))}: ${(r.files ?? []).length} file(s) in commit ${sanitize(String(r.commit ?? '')).slice(0, 12)}${r.already_there ? ' (the store already held exactly these bytes)' : ''}. Other installs accept it at their next sync.`;
  return `${r?.reason === 'denied' ? 'cancelled' : 'not published'}: ${sanitize(String(r?.message ?? r?.reason ?? 'refused'))}`;
}
