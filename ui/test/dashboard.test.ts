// The dashboard view (`specs/030`): sessions as chips, everything running now grouped by session (earlier sessions'
// running work stays on screen), what needs the operator, the pace, and the latest happenings in the space left.
import test from 'node:test';
import assert from 'node:assert/strict';
import { emptyArch } from '../src/archives.ts';
import { groups } from '../src/dashboard.ts';
import { observeFrame, parseView, type OModel, type OState } from '../src/observe-screen.ts';
import { epicWorld, info, NOW } from './epic-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');

/** A mission in its third session: one task running in this one, one started in the second that is still going. */
function world(): OState {
  const s = epicWorld();
  s.sessions = [
    { id: 's_0001', reason: 'new', started_ms: NOW - 3_600_000, turns: 4 },
    { id: 's_0002', reason: 'restart', started_ms: NOW - 1_800_000, turns: 2 },
    { id: 's_0003', reason: 'new', started_ms: NOW - 300_000, turns: 3 },
  ];
  s.tasks = {
    t_1: { id: 't_1', flow: 'build', prompt: 'Add a retry queue to the claim loop', status: 'running', session: 's_0002' },
    t_2: { id: 't_2', flow: 'orchestrator', prompt: 'Which queue should the claim loop use?', status: 'running', session: 's_0003' },
    t_3: { id: 't_3', flow: 'orchestrator', prompt: 'summarise the design', status: 'done', output: 'Use a visibility timeout.\nSecond line.', session: 's_0003' },
    t_4: { id: 't_4', flow: 'orchestrator', prompt: 'an old one', status: 'done', session: 's_0001' },
  };
  s.nodes['n7'] = { id: 'n7', role: 'orchestrator', parent: null, depth: 0, task_id: 't_2', state: 'running', waiting: 'waiting for a model: claude is out of quota', tokens: 900, cost_usd: 0, started_ms: NOW - 134_000, ended_ms: null, after: [] };
  s.narrative = ['t_2 started (orchestrator): Which queue should the claim loop use?', 'session s_0003 began (new)', 'builder: tests green, 12 passed'];
  s.retries = 2;
  s.cache_hit_ratio = 0.42;
  return s;
}
const model = (st: OState = world(), over: Partial<OModel> = {}): OModel => ({
  info, pal: { dark: true }, st, view: 'dashboard', pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now: NOW, ...over,
} as OModel);

test('work is grouped by the session that asked for it: the current one first, earlier ones while they still run', () => {
  const gs = groups(world());
  assert.deepEqual(gs.map((g) => [g.id, g.current, g.running.length]), [['s_0001', false, 0], ['s_0002', false, 1], ['s_0003', true, 1]]);
  const text = observeFrame(model(), 100, 34).map(plain).join('\n');
  assert.match(text, /session 3 \(current\)[\s\S]*session 2 \(earlier, still running\)/);
  assert.match(text, /► .*t_2 .*2:14 .*waiting for a model: claude is out/, 'what is stuck says why, with how long it has run');
  assert.match(text, /t_1 .*build/, 'a build keeps its tag');
  assert.match(text, /✔ t_3 +s3 +summarise the design +→ +Use a visibility timeout\./, 'finished work says which session asked and what it said');
  assert.match(text, /● session 3 · 3 asks · 1 running/, 'the current session is a chip');
});

test('a mission with no sessions yet is one group; idle says so; the pace and the feed fill the space', () => {
  const quiet = world();
  quiet.sessions = []; quiet.tasks = {}; quiet.nodes = {}; quiet.pending_approvals = {}; quiet.councils = []; quiet.pending_proposals = 0;
  const text = observeFrame(model(quiet), 100, 34).map(plain).join('\n');
  assert.match(text, /nothing is running: the system is idle/);
  assert.match(text, /this mission · 0 asks/);
  assert.match(text, /tokens\/min/);
  assert.match(text, /cache 42%/);
  assert.match(text, /── recent/);
});

test('hostile text in a prompt or a feed line cannot widen the screen or move the cursor', () => {
  const st = world();
  st.tasks['t_2']!.prompt = 'x\x1b[2J\x1b]0;pwned\x07 ' + 'y'.repeat(300);
  st.narrative.push('\x1b[31mred\x1b[0m ' + 'z'.repeat(300));
  for (const [w, h] of [[80, 24], [40, 12]] as const) {
    const frame = observeFrame(model(st), w, h);
    assert.equal(frame.length, h);
    assert.ok(frame.every((l) => !/\x1b\[2J|\x1b\]/.test(l)), 'no control sequence from a model survives');
  }
});

test('the dashboard has names an operator would try', () => {
  for (const n of ['dashboard', 'now', 'ops', 'monitor']) assert.equal(parseView(n), 'dashboard', n);
});

test('goldens: the dashboard at 80x24, 100x34 and 40x12, busy and idle', () => {
  const idle = world(); idle.tasks = {}; idle.nodes = {}; idle.sessions = []; idle.pending_approvals = {}; idle.councils = []; idle.pending_proposals = 0; idle.narrative = [];
  for (const [w, h] of [[80, 24], [100, 34], [40, 12]] as const) {
    for (const [name, st] of [['busy', world()], ['idle', idle]] as const) {
      const r = snapshot(`dashboard-${name}-${w}x${h}`, observeFrame(model(st), w, h));
      assert.ok(r.ok, r.message);
    }
  }
});
