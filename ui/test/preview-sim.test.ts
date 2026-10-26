// The preview simulator (`ui/dev/preview-sim.ts`) behind `ui/dev/preview.sh`: it must stay a believable, renderable mission.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { emptyArch } from '../src/archives.ts';
import { observeFrame, VIEWS, type OModel } from '../src/observe-screen.ts';
import { Preview, shiftTimes } from '../dev/preview-sim.ts';
import { monitorsWorld } from './monitors-fixture.ts';
import { info, NOW } from './epic-fixture.ts';

const T = Date.parse('2026-10-26T12:00:00Z');
const model = (p: Preview, now: number, view: OModel['view']): OModel => ({ info, pal: { dark: true }, st: p.state(), view, pick: 0, scroll: 0, selNode: 0, arch: emptyArch(), live: {}, connected: true, still: true, now, lastSeen: {}, since: now - 60_000 } as OModel);

test('the fixture\'s times move to the present and every clock in the state moves together', () => {
  const by = T - NOW, w = monitorsWorld(), s = shiftTimes(w, by);
  assert.equal(s.nodes['n3']!.started_ms, w.nodes['n3']!.started_ms + by);
  assert.equal(s.changes![0]!.at_ms, w.changes![0]!.at_ms + by);
  assert.equal(s.approvals!['ap_1']!.since_ms, w.approvals!['ap_1']!.since_ms + by);
  assert.equal(s.tools!['builder']!.open!.since_ms, w.tools!['builder']!.open!.since_ms + by);
  assert.equal(w.nodes['n3']!.started_ms, monitorsWorld().nodes['n3']!.started_ms, 'the fixture itself is not touched');
});

test('two minutes and more of the mission: tokens climb below the cap, changes are recorded, an approval is raised and answered, the scout\'s turns end', () => {
  const p = new Preview(T, { seed: 1 });
  let live = 0, est = 0, rises = 0;
  const kinds: string[] = [];
  for (let i = 1; i <= 130; i++) {
    const { frames, events } = p.tick(T + i * 1000);
    assert.ok(frames['n3']!.tokens! >= live, 'the subscription total never falls');
    assert.ok(frames['n3']!.tokens! < p.state().token_cap!, 'and stays under the cap');
    live = frames['n3']!.tokens!;
    if (frames['n7']!.est! < est) rises++;
    est = frames['n7']!.est!;
    kinds.push(...events.map((e) => e.type));
    for (const e of events) assert.ok(e.timestamp.startsWith('2026-10-26T12:'), 'events carry the simulated time');
  }
  assert.ok(kinds.filter((k) => k === 'change.recorded').length >= 6, 'a change about every fifteen seconds');
  assert.deepEqual([kinds.includes('approval.requested'), kinds.includes('approval.resolved')], [true, true]);
  assert.ok(rises >= 3 && kinds.includes('agent.usage'), 'the estimate drops each time a turn is logged');
  assert.equal(Object.keys(p.state().approvals ?? {}).filter((k) => k.startsWith('ap_p')).length, 0, 'the approval it raised was answered');
});

test('the same seed gives the same mission, and calm leaves out the approval, the loop and the clash', () => {
  const run = (o: ConstructorParameters<typeof Preview>[1]) => { const p = new Preview(T, o); const out: unknown[] = []; for (let i = 1; i <= 60; i++) out.push(p.tick(T + i * 1000)); return { p, out }; };
  assert.deepEqual(run({ seed: 3 }).out, run({ seed: 3 }).out);
  assert.notDeepEqual(run({ seed: 3 }).out, run({ seed: 4 }).out);
  const calm = run({ calm: true, seed: 3 });
  assert.ok(calm.out.every((o) => (o as { events: { type: string }[] }).events.every((e) => e.type !== 'approval.requested')));
  assert.ok(Object.keys(calm.p.state().approvals ?? {}).length === 0 && calm.p.state().files!.every((f) => !f.clash));
  assert.ok(calm.p.state().tools!['builder']!.streak!.n < 6, 'no loop is forming');
});

test('every pane renders the simulated mission at every size without overflowing, as it moves', () => {
  const p = new Preview(T, { seed: 2 });
  for (let i = 1; i <= 90; i++) {
    p.tick(T + i * 1000);
    if (i % 30 !== 0) continue;
    for (const view of VIEWS) for (const [w, h] of [[28, 9], [60, 20], [110, 34]] as const) {
      const lines = observeFrame(model(p, T + i * 1000, view), w, h);
      assert.equal(lines.length, h, `${view} ${w}x${h}`);
      for (const l of lines) assert.ok(stringWidth(l) <= w, `${view} ${w}x${h}: ${l}`);
    }
  }
});
