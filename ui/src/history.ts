// `/history` in the console (`specs/102` §5.1, slice S5): the operator's read-only view of the exchange records and the project report.
// Pure: the daemon's `history.view` answer in, text out. There is no model here and no write path; nothing this draws goes anywhere but
// the screen. The daemon cleans every string before it sends it; this cleans again before drawing, and it also drops the C1 controls
// (a lone 0x9b is a CSI to some terminals) that `sanitize` leaves.
import { sanitize } from './theme.ts';

export type ViewRecord = {
  id: string; at: string; mission: string; trigger: string; kind: string; status: string;
  prompt?: { text: string; cut: boolean } | null;
  artifacts?: { artifact: string; log: string }[];
  pasted?: { hash: string; chars: number }[];
  reply?: { label: string; asked: string; did: string; open: string } | null;
  outcome?: { value: string; provenance: string };
  observed?: { ids: string[]; counts: Record<string, number> };
};
export type HistoryView = {
  state: 'off' | 'empty' | 'records' | 'unreadable';
  message?: string | null;
  project: string; capture: string; setting: string;
  records: ViewRecord[];
  total: number; hidden: number; verified: number; chain_broken: number; unverifiable: number; broken_at?: number | null;
  report?: { text: string | null; error: string | null };
};

/** Nothing a terminal acts on: the daemon's strip, then the C1 range, the line breaks as spaces, and a cap. */
export function plain(s: unknown, cap = 400): string {
  const t = sanitize(String(s ?? '')).replace(/[\x80-\x9f]/g, '').replace(/[\r\n\t]+/g, ' ');
  return t.length > cap ? t.slice(0, cap) + '…' : t;
}

/** The mark of a record's check. Only `verified` reads as valid; anything else is said in words. */
const mark = (status: string) => (status === 'verified' ? 'verified' : status === 'chain broken' ? 'CHAIN BROKEN' : status === 'unverifiable' ? 'UNVERIFIABLE (no key)' : `UNCHECKED (${plain(status, 20)})`);

export function recordLines(r: ViewRecord): string[] {
  const head = `${plain(r.id, 24)} · ${plain(r.at, 24)} · ${plain(r.trigger, 24)} · ${plain(r.kind, 12)} · ${mark(r.status)}`;
  const out = [head];
  if (r.prompt?.text) out.push(`  prompt${r.prompt.cut ? ' (cut)' : ''}: ${plain(r.prompt.text, 2000)}`);
  for (const a of r.artifacts ?? []) out.push(`  artifact: ${plain(a.artifact, 200)}`);
  for (const p of r.pasted ?? []) out.push(`  pasted block: ${plain(p.hash, 12)} · ${Number(p.chars) || 0} characters (text not kept)`);
  if (r.reply) out.push(`  host's account (${plain(r.reply.label, 40)}): asked: ${plain(r.reply.asked, 200)} · did: ${plain(r.reply.did, 200)} · open: ${plain(r.reply.open, 200)}`);
  if (r.outcome) out.push(`  outcome: ${plain(r.outcome.value, 12)} (${plain(r.outcome.provenance, 16)})`);
  const ids = (r.observed?.ids ?? []).map((i) => plain(i, 64)).join(', ');
  if (ids) out.push(`  ids: ${ids}`);
  return out;
}

/** The text `/history` prints. `report` adds the project report after the list. */
export function historyReport(v: HistoryView, withReport = true): string {
  const lines: string[] = [];
  if (v.state === 'off') return plain(v.message ?? 'history is off', 600) + `\n(the setting: ${plain(v.setting, 80)})`;
  if (v.state === 'unreadable') return `the history store could not be read: ${plain(v.message, 200)}`;
  const records = v.records ?? [];
  if (v.state === 'empty') lines.push(plain(v.message ?? 'no records yet', 300));
  else {
    lines.push(`project ${plain(v.project, 40)} · capture ${plain(v.capture, 24)} · ${v.total} record${v.total === 1 ? '' : 's'} · ${v.verified} verified${v.chain_broken ? ` · ${v.chain_broken} CHAIN BROKEN` : ''}${v.unverifiable ? ` · ${v.unverifiable} unverifiable` : ''}`);
    if (v.message) lines.push(plain(v.message, 300));
    if (v.chain_broken) lines.push(`the chain breaks at record ${Number(v.broken_at ?? 0) + 1}: that record and every later one is shown but is not counted anywhere. Nothing was repaired or deleted.`);
    lines.push('');
    for (const r of records) lines.push(...recordLines(r), '');
    if (v.hidden > 0) lines.push(`(${v.hidden} older record${v.hidden === 1 ? '' : 's'} not shown)`);
  }
  // The report is a fixed-template block of lines: each is cleaned alone so its line breaks survive.
  if (withReport && v.report) lines.push('', ...(v.report.text ? v.report.text.split('\n').slice(0, 200).map((l) => plain(l, 400)) : [`the report: ${plain(v.report.error ?? 'not available', 200)}`]));
  return lines.join('\n').trimEnd();
}
