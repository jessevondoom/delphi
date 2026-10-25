// /decisions: review an optimistic build's provisional decisions (specs/015). The same text and rules as the Rust
// console (`crates/delphi/src/decisions.rs`); keep the two in step.
import type { Rpc } from './rpc.ts';

const short = (s: string, n: number) => { const t = (s ?? '').replace(/\s+/g, ' ').trim(); return t.length > n ? t.slice(0, n - 1) + '…' : t; };
const state = (d: any): string => d.outcome === 'deny' ? 'refused' : d.reverted === true ? 'reverted' : d.review?.resolution ?? 'PENDING';
const priority = (d: any): number => {
  if (state(d) !== 'PENDING') return 3;
  if (d.second === 'unseconded') return 0;
  return d.risk === 'elevated' ? 1 : 2;
};
const pad = (s: unknown, n: number) => String(s ?? '?').padEnd(n);

export function renderList(v: any): string {
  const ds: any[] = [...(v?.decisions ?? [])].sort((a, b) => priority(a) - priority(b));
  if (!ds.length) return 'no provisional decisions in this mission (they exist only for builds started with --optimistic)';
  const pending = v.pending ?? 0;
  const out = [`${ds.length} provisional decision(s), ${pending} awaiting your review`];
  for (const d of ds) {
    out.push(`  ${pad(d.decision_id, 14)} ${pad(d.task_id, 7)} ${pad(state(d), 8)} ${pad(d.risk, 9)} ${short(d.summary, 60)}`);
    if (state(d) === 'PENDING' || d.outcome === 'deny') {
      const by = d.decided_by ?? 'rule';
      const who = d.second ? `by ${by}; ${String(d.second).replace('concurred:', 'second seat: ')}` : `by ${by}`;
      out.push(`      why: ${short(d.rationale, 110)} (${who})`);
    }
  }
  if (pending > 0) out.push('review with: /decisions show <id> · /decisions ratify <id…|all-routine> · /decisions reject <id> [note]');
  return out.join('\n');
}

function renderOne(d: any): string {
  const out = [
    `${d.decision_id}  (${d.task_id}, ${d.outcome} by ${d.role})`,
    `  ${short(d.summary, 160)}`,
    `  tool: ${d.tool} · risk: ${d.risk} · decided: ${state(d)}`,
    `  why: ${short(d.rationale, 300)}`,
    `  payload: blake3 ${d.payload_blake3}`,
  ];
  for (const e of d.effects ?? []) out.push(`  effect: ${e.kind} ${e.path} (${e.bytes} bytes)`);
  if (d.review) out.push(`  review: ${d.review.resolution} by ${d.review.by}${d.review.note ? ` — ${d.review.note}` : ''}`);
  return out.join('\n');
}

const errText = (e: any): string => e?.message ?? String(e);

export async function decisions(cmd: Rpc, mission: string, args: string[]): Promise<string> {
  let list: any;
  try { list = await cmd.call('decisions.list', { mission_id: mission }); } catch (e) { return errText(e); }
  const ds: any[] = list?.decisions ?? [];
  const find = (id?: string) => ds.find((d) => d.decision_id === id);
  const review = async (ids: string[], resolution: string, note: string) => {
    const out: string[] = [];
    for (const id of ids) {
      const d = find(id);
      if (!d) { out.push(`${id}: no such decision`); continue; }
      // The review names the exact payload that was shown, so a decision that changed since cannot be ratified blind.
      try { await cmd.call('decisions.review', { mission_id: mission, decision_id: id, payload_blake3: d.payload_blake3, resolution, note }); out.push(`${id}: ${resolution}`); }
      catch (e) { out.push(`${id}: ${errText(e)}`); }
    }
    return out.join('\n');
  };
  switch (args[0] ?? 'list') {
    case 'list': return renderList(list);
    case 'show': { const d = find(args[1]); return d ? renderOne(d) : 'usage: /decisions show <decision id> (ids are listed by /decisions)'; }
    case 'ratify': {
      const ids = args[1] === 'all-routine'
        ? ds.filter((d) => d.outcome === 'allow' && !d.review && d.risk === 'routine').map((d) => d.decision_id)
        : args.slice(1);
      if (!ids.length) return 'nothing to ratify. Name decision ids, or `all-routine` for every pending ordinary edit (elevated ones are reviewed one by one).';
      return review(ids, 'ratified', '');
    }
    case 'reject': return args[1] ? review([args[1]], 'rejected', args.slice(2).join(' ')) : 'usage: /decisions reject <decision id> [why]';
    default: return `unknown /decisions command \`${args[0]}\` (list, show, ratify, reject)`;
  }
}
