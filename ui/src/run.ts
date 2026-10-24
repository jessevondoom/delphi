// Slash commands: each is a call to the daemon's socket and a few lines of output. The same commands as
// `delphi chat --inline`, ported so the full-screen console depends on nothing but the socket.
import type { Rpc } from './rpc.ts';
import type { Approval, State } from './fold.ts';
import type { Info } from './theme.ts';

export type Ctx = {
  cmd: Rpc; mission: string; info: Info; state: () => State;
  post: (text: string) => void;
  /** Called after a call that starts work, so the UI can show it as running. */
  started?: () => void;
};

const errText = (e: any): string => (e && typeof e === 'object' && 'message' in e ? String(e.message) : String(JSON.stringify(e)));
const usd = (n: unknown, d: number) => `$${Number(n ?? 0).toFixed(d)}`;

export function renderUsage(v: any): string {
  const row = (k: string, c: string, i: string, ca: string, o: string, co: string) => `${k.padEnd(22)} ${c.padStart(5)} ${i.padStart(10)} ${ca.padStart(10)} ${o.padStart(10)} ${co.padStart(9)}`;
  const lines = [`usage for mission ${v.mission_id ?? '?'}`, row('', 'calls', 'in', 'cache', 'out', 'cost')];
  let total = 0;
  for (const r of v.rows ?? []) {
    total += Number(r.cost_usd ?? 0);
    lines.push(row(String(r.key ?? '?'), String(r.calls), String(r.input_tokens), String((r.cache_read_tokens ?? 0) + (r.cache_write_tokens ?? 0)), String(r.output_tokens), usd(r.cost_usd, 4)));
  }
  const b = v.budget ?? {};
  lines.push(`total ${usd(total, 4)} of ${usd(b.cap_usd, 2)} ceiling (${usd(b.reserved_usd, 4)} reserved, ${usd(b.available_usd, 4)} available)`);
  return lines.join('\n');
}

export function parseCouncil(args: string): { proposition: string; seats: string[]; author: string | null } | string {
  const usage = '/council needs a proposition, for example `/council Should the retry queue be a table or a file?` (optional: --seats architect,strategist,red-team --author claude)';
  const words = args.split(/\s+/).filter(Boolean);
  const rest: string[] = [];
  let seats: string[] = [], author: string | null = null;
  for (let i = 0; i < words.length; i++) {
    const w = words[i]!;
    if (w === '--seats' && rest.length === 0) { const v = words[++i]; if (!v) return usage; seats = v.split(',').filter(Boolean); }
    else if (w === '--author' && rest.length === 0) { const v = words[++i]; if (!v) return usage; author = v; }
    else rest.push(w);
  }
  return rest.length ? { proposition: rest.join(' '), seats, author } : usage;
}

/** Answer a card: the daemon checks the payload hash, so what is approved is exactly what was shown. */
export async function resolveCard(cmd: Rpc, card: Approval, decision: 'allow' | 'allow_similar' | 'deny'): Promise<string | null> {
  try { await cmd.call('approval.resolve', { approval_id: card.id, payload_blake3: card.blake3, decision }); return null; }
  catch (e) { return errText(e); }
}

/** The newest approval that is still waiting. */
export const waiting = (s: State): Approval | undefined => Object.values(s.approvals).sort((a, b) => b.at - a.at)[0];

let lastRatify: any[] = [];

export async function runSlash(line: string, ctx: Ctx, now = Date.now()): Promise<void> {
  const [name = '', ...rest] = line.slice(1).trim().split(/\s+/);
  const arg = rest.join(' ');
  const call = async (method: string, params: object = {}) => {
    try { return await ctx.cmd.call(method, { mission_id: ctx.mission, ...params }); }
    catch (e) { ctx.post(errText(e)); return undefined; }
  };
  switch (name) {
    case 'mission': ctx.post(`mission ${ctx.mission}`); return;
    case 'usage': { const v = await call('usage.get', { group: rest[0] ?? 'role' }); if (v) ctx.post(renderUsage(v)); return; }
    case 'budget': {
      if (!rest[0]) { const b = await call('budget.get'); if (b) ctx.post(`ceiling ${usd(b.cap_usd, 2)} · spent ${usd(b.spent_usd, 4)} · reserved ${usd(b.reserved_usd, 4)} · available ${usd(b.available_usd, 4)}`); return; }
      const n = Number(rest[0].replace(/^\$/, ''));
      if (!Number.isFinite(n)) { ctx.post(`/budget takes a dollar amount, not \`${rest[0]}\``); return; }
      const b = await call('budget.set', { usd: n }); if (b) ctx.post(`ceiling is now ${usd(b.cap_usd, 2)}`); return;
    }
    case 'permissions': case 'perm': {
      if (!rest[0]) {
        const v = await call('permissions.get');
        if (v) ctx.post(`mode: ${v.mode ?? '?'} · allowed similar this task: ${(v.allow_similar ?? []).map((s: any) => `${s.pattern} (${s.task_id})`).join(', ') || 'none'}`);
        return;
      }
      const v = await call('permissions.set', { mode: rest[0] }); if (v) ctx.post(`mode is now ${v.mode ?? '?'}`); return;
    }
    case 'resume': { const v = await call('recovery.resume', { tasks: rest.length ? rest : null }); if (v) { const n = (v.resumed ?? []).length; ctx.post(n ? `resuming ${n} task(s)…` : 'nothing to resume'); if (n) ctx.started?.(); } return; }
    case 'pause': { if (await call('recovery.pause')) ctx.post('left as they are; they will be offered again after the next restart'); return; }
    case 'build': {
      if (!arg) { ctx.post('/build needs a request, for example `/build implement @specs/011-x.md in @project/app`'); return; }
      const v = await call('build.start', { prompt: arg }); if (v) { ctx.post(`(${v.task_id ?? 'task'} build started; you will be asked before anything is merged)`); ctx.started?.(); } return;
    }
    case 'council': {
      const p = parseCouncil(arg);
      if (typeof p === 'string') { ctx.post(p); return; }
      const v = await call('council.start', { proposition: p.proposition, seats: p.seats, author_family: p.author });
      if (v) { ctx.post(`(${v.task_id ?? 'task'} council convened; the seats answer blind, then critique each other)`); ctx.started?.(); } return;
    }
    case 'ratify': { ctx.post(await ratify(ctx, rest)); return; }
    case 'observe': case 'narrator': case 'visualize': {
      const view = rest[0] ?? '';
      ctx.post(`The Observer only watches. Run this in another terminal (or a tmux split):\n  delphi observe ${view}`.trimEnd()); return;
    }
    case 'y': case 'yes': case 'a': case 'always': case 'n': case 'no': case 'd': case 'diff': case 'approve': {
      const key = name === 'approve' ? rest[0] : name;
      const d = key === 'y' || key === 'yes' ? 'allow' : key === 'a' || key === 'always' ? 'allow_similar' : key === 'n' || key === 'no' ? 'deny' : key === 'd' || key === 'diff' ? 'view' : null;
      if (!d) { ctx.post('/approve takes y, a, n or d'); return; }
      const card = waiting(ctx.state());
      if (!card) { ctx.post('no approval is waiting'); return; }
      if (d === 'view') { ctx.post(JSON.stringify(card.payload, null, 2)); return; }
      if (d === 'allow_similar' && card.tier === 'red') { ctx.post('`allow similar` never applies to a Red action; use /y to approve this once or /n'); return; }
      if (now - card.at < 300) { ctx.post('that card only just appeared; look at it, then answer'); return; }
      const bad = await resolveCard(ctx.cmd, card, d);
      if (bad) ctx.post(bad);
      return;
    }
    default: ctx.post(`unknown command /${name} (try /help)`);
  }
}

async function ratify(ctx: Ctx, args: string[]): Promise<string> {
  const idx = (s?: string) => { const n = Number(s); return Number.isInteger(n) && n >= 1 ? n : null; };
  const act = args[0] ?? 'list';
  try {
    if (act === 'list') {
      lastRatify = (await ctx.cmd.call('ratify.list', {})) ?? [];
      if (!lastRatify.length) return 'no pending proposals';
      const rows = lastRatify.map((p, i) => `${String(i + 1).padStart(3)}. [${p.kind}]${p.kind === 'config-access' ? ' [RED]' : ''} ${p.summary}`);
      return [`${lastRatify.length} pending proposal(s). Nothing below takes effect until you ratify it.`, ...rows, '/ratify show <n> · apply <n> · reject <n>'].join('\n');
    }
    if (!['show', 'apply', 'ratify', 'reject'].includes(act)) return `unknown /ratify action \`${act}\` (list, show, apply, reject)`;
    const n = idx(args[1]);
    if (!n) return `usage: /ratify ${act === 'ratify' ? 'apply' : act} <number>`;
    if (!lastRatify.length) return `run /ratify first to list proposals (asked for #${n})`;
    const p = lastRatify[n - 1];
    if (!p) return `no proposal #${n}`;
    if (act === 'show') return `${p.id} — ${p.summary}\n${p.diff || '(no diff for this kind of change)'}`;
    const method = act === 'reject' ? 'ratify.reject' : 'ratify.apply';
    await ctx.cmd.call(method, { id: p.id, expected_blake3: p.live_blake3 ?? p.ratified_blake3 });
    if (p.live_blake3) lastRatify.splice(n - 1, 1);
    return `${method === 'ratify.apply' ? 'ratified' : 'rejected'} ${p.id}`;
  } catch (e) { return errText(e); }
}
