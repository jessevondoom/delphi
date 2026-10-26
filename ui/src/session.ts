// Sessions in the console (`specs/030`): the `/context` report and the whole-mission export. Pure: the daemon's answers in, text out.
import type { Info } from './theme.ts';
import { sanitize } from './theme.ts';

/** What `session.context` returns: what the next task would be told of the conversation. */
export type ContextReport = { session: string; budget_tokens: number; turns: number; digested: number; omitted: number; earlier: number; tokens: number; trimmed: boolean; ceiling: string; text: string };

const kilo = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(1)}k` : String(n));
/** `s_0003` as the operator sees it: `session 3`. */
export const sessionName = (id: string) => `session ${Number(id.replace(/\D/g, '')) || id}`;

/** The note `/context` prints. `full` adds the text itself. */
export function contextReport(r: ContextReport, full: boolean): string {
  if (r.budget_tokens === 0) return 'the team is told nothing of the conversation: [session] conversation_tokens is 0';
  if (!r.turns && !r.earlier) return `${sessionName(r.session)}: nothing is carried, the next task starts clean`;
  const whole = Math.max(0, r.turns - r.digested - r.omitted);
  const parts = [
    `${r.turns} turn${r.turns === 1 ? '' : 's'}`,
    ...(r.digested || r.omitted ? [`${whole} whole, ${r.digested} as lines, ${r.omitted} left out`] : []),
    `about ${kilo(r.tokens)} of ${kilo(r.budget_tokens)} tokens`,
    ...(r.earlier ? [`${r.earlier} earlier session${r.earlier === 1 ? '' : 's'} in a line each`] : []),
    ...(r.ceiling !== 'L5' ? [`starts below full trust (${r.ceiling}): an earlier reply read untrusted content; /new clears it`] : []),
  ];
  const head = `${sessionName(r.session)} · the next task is told ${parts.join(' · ')}`;
  return full && r.text ? `${head}\n\n${r.text}` : head + (r.text ? '\n(/context full shows the text)' : '');
}

/** The front matter every export carries. */
export const exportHead = (mission: string, title = 'Conversation export', now = new Date()) =>
  `---\nid: export-${mission}\ntitle: "${title}"\nauthor: operator\ntimestamp: ${now.toISOString()}\nsource_type: internal_generated\nconfidence: medium\nstatus: active\nsource_mission: ${mission}\ntags: [export]\n---\n`;

/** The whole mission as markdown, a section per session (`/export all`), from the events the daemon sends. */
export function exportAll(events: any[], info: Info, mission: string, now = new Date()): string {
  const roleOf: Record<string, string> = {};
  for (const e of events) if (e.type === 'dag.node.added' && e.task_id && !(Number(e.payload?.depth) > 0) && !roleOf[e.task_id]) roleOf[e.task_id] = String(e.payload?.role ?? '');
  const out = [exportHead(mission, 'Mission export (all sessions)', now)];
  let sessions = 0;
  for (const e of events) {
    const at = e.timestamp ?? '';
    if (e.type === 'session.started') {
      sessions++;
      out.push(`\n## ${sessionName(String(e.payload?.session_id ?? ''))} · began ${at} (${sanitize(String(e.payload?.reason ?? ''))})\n`);
      continue;
    }
    if (!sessions && out.length === 1) { sessions++; out.push('\n## Before sessions were recorded\n'); }
    if (e.type === 'task.created' && e.payload?.prompt && e.payload?.flow !== 'build' && e.payload?.flow !== 'council') out.push(`**You** · ${at}\n\n${sanitize(String(e.payload.prompt))}\n`);
    else if (e.type === 'task.output') {
      const role = roleOf[e.task_id] || 'orchestrator';
      out.push(`**${info.roles[role]?.name ?? role}** (${role}) · ${at}${e.payload?.status === 'escalated' ? ' · stopped' : ''}\n\n${sanitize(String(e.payload?.text ?? ''))}\n`);
    }
  }
  return out.join('\n');
}
