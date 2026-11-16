import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fixtureBridge } from '../src/bridge.ts';
import { byDay, changeRows, keptWords, sourceFacts, statusWhy, type Changes, type ClaimView, type SourceView } from '../src/ency.ts';

test('source and claim pages say how a source is kept and what a status means, in the daemon\'s own terms', async () => {
  const b = await fixtureBridge();
  const pub = (await b.ency('library.source', { id: 's-local-tides-md-3277c799' })) as SourceView;
  assert.deepEqual(sourceFacts(pub), [
    { label: 'Status', value: 'not validated' }, { label: 'How it is kept', value: 'its whole text is kept' }, { label: 'Licence', value: 'CC0-1.0' },
    { label: 'Retrieved', value: '2026-11-13' }, { label: 'Last checked', value: '2026-11-15' },
  ]);
  const priv = (await b.ency('library.source', { id: 'local-ledger-md-cb15b5e9' })) as SourceView;
  assert.equal(keptWords(priv), 'its whole text is kept; private: shown to you, never given to another session or agent');
  assert.ok(priv.text.includes('lists every visitor'), 'the operator is shown a private source (spec 115 §11)');
  const kept = (await b.ency('library.source', { id: 'almanac-1911-5d6e7f80' })) as SourceView;
  assert.equal(keptWords(kept), 'its whole text is kept; kept privately for retrieval, so only the cited passage is shown here');
  assert.deepEqual(sourceFacts(kept).filter((f) => ['Author or origin', 'Address'].includes(f.label)).map((f) => f.value), ['The Harbour Almanac', 'https://almanac.example/1911/tides']);
  assert.equal(keptWords({ capture_level: 'excerpts', private: false, meta: {} }), 'the passages quoted from it are kept, not the whole text');

  const contested = (await b.ency('library.claim', { id: 'c-77aa88bb99cc' })) as ClaimView;
  assert.equal(statusWhy(contested.status, contested.claim), 'Contested: someone said it is wrong. It reads not validated until the contest is resolved. The daemon counts 0 independent origins, and something contradicts it.');
  const noted = (await b.ency('library.claim', { id: 'c-fbebef7001b9' })) as ClaimView;
  assert.equal(statusWhy(noted.status, noted.claim), 'Noted: recorded with 2 anchors in sources, and not yet confirmed by independent origins or the operator. The daemon counts 1 independent origin.');
  assert.equal(statusWhy('something-new', { anchors: [] }), 'Something-new.');
});

test('what changed is one dated list, newest first', async () => {
  const b = await fixtureBridge();
  const all = changeRows((await b.ency('changes.since', { time: 0 })) as Changes);
  assert.deepEqual(byDay(all).map(([day, rows]) => [day, rows.map((r) => r.kind).sort()]), [
    ['2026-11-15', ['finding', 'note', 'page', 'page', 'research', 'research']], ['2026-11-14', ['note']], ['2026-11-12', ['research']],
  ]);
  const tides = all.find((r) => r.title === 'Tides')!;
  assert.deepEqual([tides.detail, tides.open], ['version 2: 2 statements added, 1 statement removed, 2 sources added', { v: 'topic', slug: 'tides' }]);
  // Since a time: only what is dated after it.
  const late = changeRows((await b.ency('changes.since', { time: 1794700800 })) as Changes);
  assert.deepEqual(late.map((r) => r.title).sort(), ['Lighthouse lamps', 'The harbour office keeps a second ledger for pilots.', 'Who kept the harbour ledger?']);
});
