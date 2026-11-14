// Writes src/fixture/sim.json: the console simulator's `observe.state` and a minute of its events, at a fixed time.
// The simulator is the console's own (`ui/dev/preview-sim.ts`), so the app's screens are tested against the shape the
// console is tested against. Run from `app/`: `node --import tsx dev/make-fixture.ts`.
import { writeFileSync } from 'node:fs';
import { Preview } from '../../ui/dev/preview-sim.ts';

const now = Date.UTC(2026, 10, 14, 14, 30, 0);
const sim = new Preview(now, { seed: 7 });
const events: unknown[] = [];
let t = now;
for (let i = 0; i < 60; i++) {
  t += 1000;
  events.push(...sim.tick(t).events);
}
// The simulator's mission has no council. Two are added by hand in the daemon's shape (`observe::Council`): one decided,
// one in its second round, with the events a daemon would have sent for the running one.
const state = sim.state() as unknown as { councils: unknown[] };
const at = (s: number) => new Date(now - s * 1000).toISOString();
state.councils = [
  {
    task_id: 't_c1', proposition: 'Should the retry queue be a table or a file?', homogeneous: false, phase: 'verdict',
    seats: [{ role: 'architect', family: 'claude' }, { role: 'strategist', family: 'openai' }, { role: 'red-team', family: 'gemini' }],
    stances: [{ role: 'architect', answered: true, text: 'A table. Claims need a transaction.' }, { role: 'strategist', answered: true, text: 'A table, unless the file is already there.' }, { role: 'red-team', answered: true, text: 'A file loses rows when two workers claim at once.' }],
    critiques: [{ role: 'architect', recommendation: 'table', concur: true, findings: [], text: 'Nothing to add.' }, { role: 'strategist', recommendation: 'table', concur: true, findings: [], text: 'Agreed.' }, { role: 'red-team', recommendation: 'table', concur: true, findings: [['minor', 'the migration needs a rollback']], text: 'Agreed, with one condition.' }],
    escalation: null, path: 'missions/2026-10-25-rail/councils/t_c1.md', unresolved: [['minor', 'red-team', 'the migration needs a rollback']], options: [], synthesis: 'Use a table.',
    lenses: [['architect', 'structure'], ['strategist', 'cost'], ['red-team', 'failure']], lens_note: '',
  },
  {
    task_id: 't_c2', proposition: 'Should a claim that times out be retried by the same worker or handed to another?', homogeneous: false, phase: 'round 2',
    seats: [{ role: 'strategist', family: 'claude' }, { role: 'red-team', family: 'openai' }, { role: 'architect', family: 'gemini' }],
    stances: [{ role: 'strategist', answered: true, text: 'Hand it to another worker. The first one may be the reason it timed out.' }, { role: 'red-team', answered: true, text: 'The same worker retrying hides a stuck worker. Hand it on, and count the hand-offs.' }, { role: 'architect', answered: true, text: 'Retry once in place, then hand on. A hand-off costs a round trip.' }],
    critiques: [{ role: 'red-team', recommendation: 'hand on', concur: null, findings: [['major', 'a retry in place can loop']], text: 'One retry in place is one too many when the worker is the fault.' }],
    escalation: null, path: '', unresolved: [], options: [], synthesis: '',
    lenses: [['strategist', 'what finishes soonest'], ['red-team', 'how it fails'], ['architect', 'what it costs later']], lens_note: 'Each seat was given a different lens.',
  },
];
const c2 = (type: string, s: number, payload: Record<string, unknown>, agent?: string) => ({ type, timestamp: at(s), task_id: 't_c2', agent, payload });
events.push(
  c2('council.started', 420, { proposition: 'Should a claim that times out be retried by the same worker or handed to another?', seats: [], homogeneous: false }),
  c2('council.lens_assigned', 415, { assignments: [], note: 'Each seat was given a different lens.' }),
  c2('agent.block.complete', 400, { block: { type: 'tool_call', name: 'read_file', arguments: { path: 'crates/queue/src/claim.rs' } } }, 'red-team'),
  c2('agent.block.complete', 395, { block: { type: 'tool_call', name: 'search_kb', arguments: { query: 'visibility timeout' } } }, 'strategist'),
  c2('council.stance', 300, { role: 'strategist', status: 'answered', text: 'Hand it to another worker. The first one may be the reason it timed out.' }),
  c2('council.stance', 290, { role: 'red-team', status: 'answered', text: 'The same worker retrying hides a stuck worker. Hand it on, and count the hand-offs.' }),
  c2('council.stance', 280, { role: 'architect', status: 'answered', text: 'Retry once in place, then hand on. A hand-off costs a round trip.' }),
  c2('council.barrier.released', 275, {}),
  c2('council.critique', 120, { role: 'red-team', recommendation: 'hand on', text: 'One retry in place is one too many when the worker is the fault.' }),
);
writeFileSync(new URL('../src/fixture/sim.json', import.meta.url), JSON.stringify({ now: t, state, events }, null, 1) + '\n');
console.log(`wrote src/fixture/sim.json: ${events.length} events`);
