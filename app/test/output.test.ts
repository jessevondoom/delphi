import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import type { DocRow } from '../src/kb.ts';
import { canOpen, fileKind, groupCounts, groupOf, iconOf, items } from '../src/output.ts';
import type { OState } from '../src/types.ts';

const sim = JSON.parse(readFileSync(new URL('../src/fixture/sim.json', import.meta.url), 'utf8')) as { state: OState };
const row = (rel_path: string, zone: string, updated_at: string, extra: Partial<DocRow> = {}): DocRow => ({ rel_path, zone, kind: 'other', status: 'active', source_type: 'internal_generated', updated_at, ...extra });

test('a document is grouped by where the daemon filed it, and what configures the team is left out', () => {
  assert.equal(groupOf(row('missions/m/report.md', 'missions', '')), 'mission');
  assert.equal(groupOf(row('summaries/x.md', 'summaries', '')), 'research');
  assert.equal(groupOf(row('context/a.md', 'context', '')), 'context');
  assert.equal(groupOf(row('memory/a.md', 'memory', '')), 'memory');
  for (const zone of ['skills', 'agents', 'core', 'something-new']) assert.equal(groupOf(row('x.md', zone, '')), null, zone);
});

test('research is listed once with its sources, and a status page gives way to a mission\'s real documents', () => {
  const rows = [
    row('summaries/deep-read.md', 'summaries', '2026-11-09T10:00:00Z'),
    row('summaries/deep-read.bib.md', 'summaries', '2026-11-09T10:00:00Z'),
    row('summaries/alone.bib.md', 'summaries', '2026-11-08T10:00:00Z'),
    row('missions/with-report/status.md', 'missions', '2026-11-12T10:00:00Z'),
    row('missions/with-report/report.md', 'missions', '2026-11-12T09:00:00Z'),
    row('missions/only-status/status.md', 'missions', '2026-11-11T10:00:00Z'),
    row('skills/a/SKILL.md', 'skills', '2026-11-13T10:00:00Z'),
  ];
  const all = items(rows, null);
  assert.deepEqual(all.map((x) => x.path), ['missions/with-report/report.md', 'missions/only-status/status.md', 'summaries/deep-read.md', 'summaries/alone.bib.md']);
  const research = all.find((x) => x.path === 'summaries/deep-read.md')!;
  assert.equal(research.detail, 'with its sources');
  assert.deepEqual(research.files, ['summaries/deep-read.bib.md']);
  assert.equal(all[0]!.mission, 'with-report');
});

test('what agents recorded is output too, with who recorded it, and groups with nothing are not offered', () => {
  const all = items([row('context/a.md', 'context', '2020-01-01T00:00:00Z')], sim.state);
  const changes = all.filter((x) => x.group === 'change');
  assert.equal(changes.length, sim.state.changes!.length);
  assert.ok(changes.every((x) => x.role && x.status === 'recorded'));
  for (let i = 1; i < all.length; i++) assert.ok(all[i - 1]!.at_ms >= all[i]!.at_ms);
  assert.deepEqual(groupCounts(all).map((g) => g.group), ['change', 'context']);
  assert.deepEqual(items([], null), []);
  assert.deepEqual(groupCounts([]), []);
});

test('a file is called what its name says it is', () => {
  const kinds = Object.fromEntries(['report.md', 'plan.pdf', 'deck.pptx', 'numbers.csv', 'shot.PNG', 'demo.mp4', 'talk.m4a', 'index.html', 'conf.toml', 'main.rs', 'Makefile'].map((f) => [f, fileKind(f)]));
  assert.deepEqual(kinds, { 'report.md': 'document', 'plan.pdf': 'document', 'deck.pptx': 'deck', 'numbers.csv': 'data', 'shot.PNG': 'image', 'demo.mp4': 'video', 'talk.m4a': 'audio', 'index.html': 'deck', 'conf.toml': 'config', 'main.rs': 'code', Makefile: 'file' });
});

test('a deck, a page, a document, a picture or a recording opens outside; a program or a script does not', () => {
  for (const f of ['deck.html', 'a/b/page.HTM', 'plan.pdf', 'shot.png', 'photo.JPEG', 'demo.mp4', 'talk.m4a', 'notes.md']) assert.ok(canOpen(f), f);
  for (const f of ['run.sh', 'tool.app', 'main.rs', 'archive.zip', 'Makefile', 'deck.html.sh']) assert.ok(!canOpen(f), f);
});

test('a listed document carries who its front matter names, and none when the daemon does not say', () => {
  const all = items([row('context/a.md', 'context', '2026-11-12T10:00:00Z', { author: 'architect' }), row('summaries/b.md', 'summaries', '2026-11-11T10:00:00Z', { author: 'operator-import' }), row('memory/c.md', 'memory', '2026-11-10T10:00:00Z', { author: null }), row('memory/d.md', 'memory', '2026-11-09T10:00:00Z')], null);
  assert.deepEqual(all.map((x) => x.role), ['architect', 'operator-import', undefined, undefined]);
});

test('an item carries the picture of what it is', () => {
  const doc = (path: string, group: 'mission' | 'research' | 'context' | 'memory') => iconOf({ group, path, files: [] });
  assert.equal(doc('missions/m/report.md', 'mission'), 'doc');
  assert.equal(doc('missions/m/councils/001-a-or-b.md', 'mission'), 'council');
  assert.equal(doc('summaries/deep-read.md', 'research'), 'research');
  assert.equal(doc('context/reference/board.pptx', 'context'), 'deck');
  assert.equal(doc('context/reference/chart.png', 'context'), 'image');
  assert.equal(doc('memory/lessons/a.md', 'memory'), 'doc');
  assert.equal(iconOf({ group: 'build', files: [] }), 'repo');
  // A change is drawn by its files only when they are all one kind of thing.
  const change = (...files: string[]) => iconOf({ group: 'change', files });
  assert.equal(change(), 'repo');
  assert.equal(change('src/a.rs', 'src/b.rs'), 'repo');
  assert.equal(change('out/a.png', 'out/b.jpg'), 'image');
  assert.equal(change('talk.pptx'), 'deck');
  assert.equal(change('demo/index.html'), 'app');
  assert.equal(change('walk.mp4'), 'video');
  assert.equal(change('report.md', 'chart.png'), 'repo');
});
