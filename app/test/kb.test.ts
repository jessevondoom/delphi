import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fixtureBridge } from '../src/bridge.ts';
import { dayOf, docInAddress, docTarget, frontmatter, headingOf, LIST_PAGE, listAll, listTarget, searchTarget, sourceWords, titleOf, type Doc, type SearchBody } from '../src/kb.ts';

test('a read is addressed the way the daemon expects, with the query encoded', () => {
  assert.equal(searchTarget('  narrator socket & more  '), '/v1/search?q=narrator%20socket%20%26%20more&limit=20&facets=1');
  assert.equal(searchTarget('x', 'skill', 5), '/v1/search?q=x&limit=5&facets=1&kind=skill');
  assert.equal(docTarget('missions/a b/status.md'), '/v1/doc?path=missions%2Fa%20b%2Fstatus.md');
  assert.equal(listTarget(8), '/v1/documents?limit=8');
});

test('front matter is read as fields, and the text after it is the body', () => {
  const { meta, body } = frontmatter('---\nid: x\ntitle: "Status: one"\nauthor: architect\nstatus: active   # a comment\ntags: [a, b]\n---\n\n# Heading\nText.\n');
  assert.deepEqual(meta, { id: 'x', title: 'Status: one', author: 'architect', status: 'active', tags: '[a, b]' });
  assert.equal(body, '# Heading\nText.\n');
  // No front matter, or one that never closes: the whole text is the body.
  assert.deepEqual(frontmatter('# Just text'), { meta: {}, body: '# Just text' });
  assert.equal(frontmatter('---\ntitle: x\nno end').body, '---\ntitle: x\nno end');
});

test('a document is called by its title, its first heading, or a name made from its path', () => {
  assert.equal(titleOf('a/b.md', '---\ntitle: "From the front"\n---\n# Other'), 'From the front');
  assert.equal(titleOf('a/b.md', '# Mission `one`\n'), 'Mission one');
  assert.equal(titleOf('context/domains/retry-queues.md'), 'retry queues');
  assert.equal(titleOf('skills/spec-driven-build/SKILL.md'), 'spec-driven-build (skill)');
  assert.equal(titleOf('missions/2026-10-25-rail/status.md'), '2026-10-25-rail (status)');
});

test('headings, days and sources are put in words', () => {
  assert.deepEqual(headingOf('Retry queues > Claims > Visibility timeout'), { title: 'Visibility timeout', above: 'Retry queues › Claims' });
  assert.deepEqual(headingOf('Alone'), { title: 'Alone', above: '' });
  assert.equal(dayOf('2026-11-12T09:00:00Z'), '2026-11-12');
  assert.equal(dayOf('not a time'), '');
  assert.equal(sourceWords('user_ratified'), 'ratified by the operator');
  assert.equal(sourceWords('something_new'), 'something new');
});

test('the fixture answers the five reads in the daemon\'s shapes, and nothing else', async () => {
  const b = await fixtureBridge();
  const found = (await b.kb(searchTarget('visibility timeout'))).body as SearchBody;
  assert.equal(found.results.length, 3);
  assert.deepEqual(found.results.map((h) => h.label), ['validated', 'not validated', 'not validated']);
  const doc = (await b.kb(docTarget(found.results[0]!.path))).body as Doc;
  assert.equal(titleOf(doc.path, doc.text), 'Retry queues');
  assert.equal((await b.kb('/v1/collections')).status, 200);
  assert.equal((await b.kb(listTarget(8))).status, 200);
  assert.equal((await b.kb(docTarget('no/such.md'))).status, 404);
  assert.equal((await b.kb('/v1/image?path=x')).status, 404);
});

test('a document named in the address must be a plain path inside the workspace', () => {
  assert.equal(docInAddress('?doc=missions/m/report.md'), 'missions/m/report.md');
  assert.equal(docInAddress('?q=x&doc=agents%2Fscout%2FAGENT.md'), 'agents/scout/AGENT.md');
  for (const bad of ['?doc=../secrets.md', '?doc=/etc/hosts', '?doc=a/../../b.md', '?doc=https://example.com/x.md', '?doc=', '']) assert.equal(docInAddress(bad), '', bad);
});

test('a listing is read to its end, a page at a time, by collection or by kind', async () => {
  assert.equal(listTarget(200, undefined, { collection: 'agents' }), '/v1/documents?limit=200&collection=agents');
  assert.equal(listTarget(200, 'skill', { offset: 200 }), '/v1/documents?limit=200&kind=skill&offset=200');
  // 450 documents in the collection: three reads, and the rows past the first page are there.
  const all = Array.from({ length: 450 }, (_, i) => ({ rel_path: `agents/r${i}/AGENT.md`, kind: 'other', zone: 'agents', status: 'active', source_type: 'user_ratified', updated_at: '' }));
  const asked: string[] = [];
  const read = async (target: string) => {
    asked.push(target);
    const q = new URLSearchParams(target.split('?')[1]);
    const at = Number(q.get('offset') ?? 0);
    return { status: 200, body: all.slice(at, at + Number(q.get('limit'))) };
  };
  const got = await listAll(read, { collection: 'agents' });
  assert.equal(got.status, 200);
  assert.equal(got.rows.length, 450);
  assert.deepEqual(asked, ['/v1/documents?limit=200&collection=agents', '/v1/documents?limit=200&collection=agents&offset=200', '/v1/documents?limit=200&collection=agents&offset=400']);
  // A full last page costs one more read, which comes back empty.
  asked.length = 0;
  assert.equal((await listAll(async (t) => (asked.push(t), { status: 200, body: asked.length === 1 ? all.slice(0, LIST_PAGE) : [] }), { kind: 'skill' })).rows.length, LIST_PAGE);
  assert.equal(asked.length, 2);
  // It stops at the cap, and a refusal ends it with the rows so far.
  assert.equal((await listAll(async () => ({ status: 200, body: all.slice(0, LIST_PAGE) }), {}, 400)).rows.length, 400);
  let n = 0;
  assert.deepEqual(await listAll(async () => (n++ === 0 ? { status: 200, body: all.slice(0, LIST_PAGE) } : { status: 503, body: {} }), {}).then((r) => [r.status, r.rows.length]), [503, 200]);
});
