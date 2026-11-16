import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fixtureBridge } from '../src/bridge.ts';
import { parts, passages, type Topic, type TopicPage } from '../src/ency.ts';
import { EMPTY, SITTING_MS, annotate, arrive, editNote, exportMarkdown, fresh, lastHere, markRead, newSince, parseMarks, passageKey, pinned, placed, removeNote, togglePin, type Marks } from '../src/marks.ts';

const NOW = '2026-11-15T12:00:00.000Z';

test('AC-6: a page version opened to its end is not listed as new, and a later version is', async () => {
  const topics = ((await (await fixtureBridge()).ency('pages.list')) as { topics: Topic[] }).topics;
  const tides = topics.find((t) => t.slug === 'tides')!;
  // Never read: new. A topic with no page built is neither new nor changed.
  assert.deepEqual(newSince(EMPTY, topics).map((x) => [x.topic.slug, x.state]), [['lighthouse-lamps', 'new'], ['tides', 'new']]);
  assert.equal(fresh(EMPTY, topics.find((t) => t.slug === 'harbour-ledger')!), null);
  // Version 2 read to its end: not listed.
  const read = markRead(EMPTY, 'tides', tides.version, NOW);
  assert.equal(fresh(read, tides), null);
  assert.deepEqual(newSince(read, topics).map((x) => x.topic.slug), ['lighthouse-lamps']);
  // The daemon builds version 3: listed again, as changed.
  const rebuilt = { ...tides, version: 3, built: (tides.built as number) + 3600 };
  assert.equal(fresh(read, rebuilt), 'changed');
  assert.deepEqual(newSince(read, [rebuilt]).map((x) => x.state), ['changed']);
  // Reading an earlier version afterwards does not make the later one unread, and a version that is not one is not marked.
  assert.equal(markRead(read, 'tides', 1, NOW), read);
  for (const bad of [0, -1, 1.5, Number.NaN]) assert.equal(markRead(EMPTY, 'tides', bad, NOW), EMPTY);
  assert.equal(markRead(EMPTY, '../x', 1, NOW), EMPTY);
});

test('"since you were last here" is since the sitting before this one', () => {
  const first = arrive(EMPTY, '2026-11-14T09:00:00.000Z');
  assert.deepEqual([first.here, first.before, lastHere(first)], ['2026-11-14T09:00:00.000Z', undefined, 0]);
  // Coming back within the sitting changes nothing; after a while away, the earlier sitting is "last here".
  assert.equal(arrive(first, '2026-11-14T09:10:00.000Z'), first);
  const next = arrive(first, new Date(Date.parse('2026-11-14T09:00:00.000Z') + SITTING_MS).toISOString());
  assert.deepEqual([next.here, next.before], ['2026-11-14T09:30:00.000Z', '2026-11-14T09:00:00.000Z']);
  assert.equal(lastHere(next), Date.parse('2026-11-14T09:00:00.000Z') / 1000);
});

test('AC-9: an annotation survives a rebuild that keeps its passage, and is listed as orphaned when the passage is gone', async () => {
  const b = await fixtureBridge();
  const v1 = passages(parts(((await b.ency('pages.get', { slug: 'tides', version: '20261114T000000Z' })) as TopicPage).markdown));
  const v2 = passages(parts(((await b.ency('pages.get', { slug: 'tides' })) as TopicPage).markdown));
  const moon = v1.find((p) => p.startsWith('The moon pulls'))!;
  const clock = v1.find((p) => p.startsWith('Older tables'))!;
  let m = annotate(EMPTY, { slug: 'tides', version: 1, passage: moon, text: 'Check this against the almanac.' }, NOW, 'n1');
  m = annotate(m, { slug: 'tides', version: 1, passage: clock, text: 'Which church?' }, NOW, 'n2');
  // On version 1 both notes sit on their passages.
  const on1 = placed(m, 'tides', v1);
  assert.deepEqual([[...on1.on.values()].flat().map((n) => n.id), on1.orphaned.length], [['n1', 'n2'], 0]);
  // Version 2 kept the first passage (it now cites one more source, which does not change its words) and dropped the second.
  assert.notEqual(v2.find((p) => p.startsWith('The moon pulls')), moon, 'the markers changed');
  const on2 = placed(m, 'tides', v2);
  assert.deepEqual(on2.on.get(passageKey(moon))?.map((n) => n.text), ['Check this against the almanac.']);
  assert.deepEqual(on2.orphaned.map((n) => [n.id, n.passage]), [['n2', 'Older tables give the times by the church clock.']]);
  // Notes of another page are not this page's.
  assert.deepEqual(placed(m, 'lighthouse-lamps', v2), { on: new Map(), orphaned: [] });
  // Editing changes the words; emptying or removing takes the note away.
  assert.equal(editNote(m, 'n1', '  Checked.  ').notes[0]!.text, 'Checked.');
  assert.deepEqual(editNote(m, 'n1', '   ').notes.map((n) => n.id), ['n2']);
  assert.deepEqual(removeNote(m, 'n2').notes.map((n) => n.id), ['n1']);
  // An empty note, or one on nothing, is not kept.
  assert.equal(annotate(EMPTY, { slug: 'tides', version: 1, passage: moon, text: '  ' }, NOW, 'x'), EMPTY);
  assert.equal(annotate(EMPTY, { slug: 'tides', version: 1, passage: ' [c-1] ', text: 'note' }, NOW, 'x'), EMPTY);
});

test('pins are a list of topics, and what was kept is read back or dropped, never guessed at', () => {
  let m = togglePin(EMPTY, 'tides');
  assert.ok(pinned(m, 'tides') && !pinned(m, 'lighthouse-lamps'));
  m = togglePin(m, 'tides');
  assert.deepEqual(m.pins, []);
  assert.equal(togglePin(EMPTY, 'Not A Slug'), EMPTY);

  const kept: Marks = { v: 1, read: { tides: { version: 2, at: NOW } }, notes: [{ id: 'n1', slug: 'tides', version: 2, passage: 'The moon pulls the sea.', text: 'note', at: NOW }], pins: ['tides'], here: NOW, before: '2026-11-14T09:00:00.000Z' };
  assert.deepEqual(parseMarks(JSON.parse(JSON.stringify(kept))), kept);
  // Damaged, or not what this app wrote: each bad part is dropped and the rest stands.
  assert.deepEqual(parseMarks(null), EMPTY);
  assert.deepEqual(parseMarks('text'), EMPTY);
  assert.deepEqual(parseMarks([1]), EMPTY);
  assert.deepEqual(parseMarks({ read: { '../etc': { version: 1 }, tides: { version: 'two' }, lamps: { version: 3, at: 'never' } }, notes: [{ id: 'a' }, null, { id: 'b', slug: 'tides', passage: 'p', text: 't', version: 1, at: NOW }, { id: 'c', slug: '../x', passage: 'p', text: 't' }], pins: ['tides', 'tides', 7, '../x'], here: 'not a time' }),
    { v: 1, read: { lamps: { version: 3, at: '' } }, notes: [{ id: 'b', slug: 'tides', version: 1, passage: 'p', text: 't', at: NOW }], pins: ['tides'] });
});

test('marks are kept through the bridge and read back; with no app they stay in the window', async () => {
  const b = await fixtureBridge();
  assert.deepEqual(parseMarks(await b.marks()), EMPTY);
  const m = markRead(togglePin(EMPTY, 'tides'), 'tides', 2, NOW);
  await b.saveMarks(m);
  assert.deepEqual(parseMarks(await b.marks()), m);
});

test('an exported page is the stored page with its banner and its bibliography, and a page without a banner is not exported', async () => {
  const page = (await (await fixtureBridge()).ency('pages.get', { slug: 'tides' })) as TopicPage;
  const out = exportMarkdown(page, '2026-11-15')!;
  assert.equal(out.name, 'tides-v2');
  assert.ok(out.text.startsWith('Generated page. Not authoritative. Written by the writer role on 2026-11-15'), 'the banner comes first');
  assert.ok(out.text.includes(page.markdown.trim()), 'the stored page, unchanged');
  assert.ok(out.text.includes('## Bibliography and citations') && out.text.includes('| s-local-tides-md-3277c799 |'), 'its attributions');
  assert.match(out.text, /Exported from Delphi on 2026-11-15: the topic page "Tides" \(tides\), version 2\. Generated, not authoritative/);
  // An earlier version says which it is, in its name too.
  const old = (await (await fixtureBridge()).ency('pages.get', { slug: 'tides', version: '20261114T000000Z' })) as TopicPage;
  const older = exportMarkdown(old, '2026-11-15')!;
  assert.equal(older.name, 'tides-20261114T000000Z');
  assert.ok(older.text.includes('the version of 20261114T000000Z'));
  // No banner, no file; and a name is only ever made from a slug.
  assert.equal(exportMarkdown({ ...page, markdown: '# Tides\n\nText.' }, '2026-11-15'), null);
  assert.equal(exportMarkdown({ ...page, slug: '../../etc/passwd' }, '2026-11-15'), null);
  assert.equal(exportMarkdown({ ...page, markdown: '' }, '2026-11-15'), null);
});
