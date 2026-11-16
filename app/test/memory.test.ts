import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fixtureBridge } from '../src/bridge.ts';
import { byDay, learned, lessonLines, roleInAddress, type RoleMemory } from '../src/ency.ts';
import { episodesOf } from '../src/screens/Memory.tsx';

test('AC-7: a role\'s memory page has its lessons, its provisional notes and its findings, and the operator is shown all of them', async () => {
  const b = await fixtureBridge();
  const m = (await b.ency('memory.role', { role: 'scout' })) as RoleMemory;
  assert.deepEqual(lessonLines(m.lessons).map((l) => [l.from, l.day]), [['operator ratified', ''], ['workspace ratified', ''], ['workspace ratified', '2026-11-10']]);
  // A note made inside a private project is the role's all the same, and is shown with its project.
  assert.deepEqual(m.notes.map((n) => [n.id, n.project, n.uses, n.valid_until]), [['pn-01', null, 3, '2026-12-14'], ['pn-02', 'Harbour & Co', 0, null]]);
  assert.equal(m.findings[0]!.label, 'agent finding, not validated');
  const episodes = episodesOf([
    { rel_path: 'memory/episodes/scout/2026-11-13-almanac.md', kind: 'memory', zone: 'episodes', status: 'active', source_type: 'internal_empirical', updated_at: '2026-11-13T10:00:00Z', author: null },
    { rel_path: 'memory/episodes/2026-11-12-run.md', kind: 'memory', zone: 'episodes', status: 'active', source_type: 'internal_empirical', updated_at: '2026-11-12T10:00:00Z', author: 'scout' },
    { rel_path: 'memory/episodes/builder/2026-11-14-x.md', kind: 'memory', zone: 'episodes', status: 'active', source_type: 'internal_empirical', updated_at: '2026-11-14T10:00:00Z', author: 'builder' },
  ], 'scout');
  assert.deepEqual(episodes.map((e) => e.rel_path), ['memory/episodes/scout/2026-11-13-almanac.md', 'memory/episodes/2026-11-12-run.md']);
  // "Learned lately": all four kinds in one dated list, newest first; a lesson with no date is not given one.
  const lately = learned(m, episodes);
  assert.deepEqual(lately.map((l) => [l.day, l.kind]), [['2026-11-15', 'note'], ['2026-11-15', 'finding'], ['2026-11-14', 'note'], ['2026-11-13', 'episode'], ['2026-11-12', 'episode'], ['2026-11-10', 'lesson']]);
  assert.equal(lately.length, m.notes.length + m.findings.length + episodes.length + 1, 'nothing dated is left out');
  // Across roles, by date.
  const architect = (await b.ency('memory.role', { role: 'architect' })) as RoleMemory;
  assert.deepEqual(byDay([...learned(architect), ...lately]).map(([d, r]) => [d, r.length]), [['2026-11-15', 2], ['2026-11-14', 1], ['2026-11-13', 1], ['2026-11-12', 1], ['2026-11-10', 1]]);
});

test('a role named in the address must be a role id', () => {
  assert.deepEqual([roleInAddress('?role=scout'), roleInAddress('?role=../x'), roleInAddress('?role=Scout'), roleInAddress('')], ['scout', '', '', '']);
});
