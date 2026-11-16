import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Rich } from '../src/Rich.tsx';
import { fixtureBridge } from '../src/bridge.ts';
import {
  anchorOf, byLetter, byProject, cited, contents, findQuote, kindOf, marker, matching, parts, passages, pieces, routeInAddress, unmarked,
  type ClaimView, type Research, type SourceView, type Topic, type TopicPage,
} from '../src/ency.ts';
import { SourceText } from '../src/screens/Encyclopedia.tsx';

test('a citation marker is read the way the daemon reads one, and anything else in brackets is text', () => {
  assert.deepEqual(marker('[c-01]'), [{ label: 'c-01' }]);
  assert.deepEqual(marker('[s-local-tides-md-3277c799]'), [{ label: 's-local-tides-md-3277c799' }]);
  assert.deepEqual(marker('[c-1, s-x; f-0123456789abcdef]'), [{ label: 'c-1' }, { label: 's-x' }, { label: 'f-0123456789abcdef' }]);
  assert.deepEqual(marker('[c-1 ● validated]'), [{ label: 'c-1', shown: '● validated' }]);
  // Not markers: another prefix, a status the daemon does not write, a path, an empty id.
  for (const not of ['[x-1]', '[c-1 certainly true]', '[c-1/../x]', '[s-]', '[note]', '[c-1', 'c-1]', '[ ]', '[c-1 ● validated, anything]']) assert.equal(marker(not), null, not);
  assert.deepEqual([kindOf('c-1'), kindOf('s-web-1'), kindOf('s-kb-0123456789abcdef'), kindOf('f-0123456789abcdef'), kindOf('s-kb-short')], ['claim', 'source', 'passage', 'finding', 'source']);

  assert.deepEqual(pieces('Tides come twice a day [c-1]. See [the plan](plan.md) and [c-2, s-x].'), [
    { t: 'text', s: 'Tides come twice a day ' }, { t: 'cite', cites: [{ label: 'c-1' }], raw: '[c-1]' }, { t: 'text', s: '. See [the plan](plan.md) and ' },
    { t: 'cite', cites: [{ label: 'c-2' }, { label: 's-x' }], raw: '[c-2, s-x]' }, { t: 'text', s: '.' },
  ]);
  assert.deepEqual(pieces('[c-1](not-a-marker.md) and an [unclosed'), [{ t: 'text', s: '[c-1](not-a-marker.md) and an [unclosed' }]);
  assert.deepEqual(cited('a [c-1] b [s-x, c-1] c [c-2]'), ['c-1', 's-x', 'c-2']);
  assert.equal(unmarked('Tides come   twice a day [c-1].\nAnd again [s-x] .'), 'Tides come twice a day. And again.');
});

test('a heading makes the anchor the daemon makes, so the page\'s own section links land', () => {
  assert.equal(anchorOf('Why twice a day'), 'why-twice-a-day');
  assert.equal(anchorOf('Spring & neap tides (2026)'), 'spring--neap-tides-2026');
  assert.equal(anchorOf('Élan vital: un résumé'), 'élan-vital-un-résumé');
});

async function page(slug: string, version?: string): Promise<TopicPage> {
  return (await (await fixtureBridge()).ency('pages.get', version ? { slug, version } : { slug })) as TopicPage;
}

test('a page is taken apart into its banner, lead, sections and bibliography, and nothing of it is rewritten', async () => {
  const p = parts((await page('tides')).markdown);
  assert.match(p.banner, /^Generated page\. Not authoritative\. Written by the writer role on 2026-11-15/);
  assert.equal(p.title, 'Tides');
  assert.match(p.lead, /^Tides are the regular rise and fall of the sea\..*\[s-kb-0123456789abcdef\]\.$/);
  assert.deepEqual(p.sections.map((s) => [s.heading, s.anchor]), [['Why twice a day', 'why-twice-a-day'], ['Spring and neap tides', 'spring-and-neap-tides']]);
  // The sections shown apart from the text are taken out of it, each as the page wrote it.
  assert.deepEqual(Object.keys(p.named).sort(), ['Contents', 'Key terms', 'Open questions', 'See also', 'Start here']);
  assert.match(p.named['Key terms']!, /^- \*\*Spring tide\*\*/);
  assert.deepEqual(contents(p), [
    { title: 'Why twice a day', anchor: 'why-twice-a-day', depth: 0 }, { title: 'What the tables add', anchor: 'what-the-tables-add', depth: 1 }, { title: 'Spring and neap tides', anchor: 'spring-and-neap-tides', depth: 0 },
  ]);
  assert.equal(p.bibliography.length, 7);
  assert.deepEqual(p.bibliography[0], { label: 'c-0a1b2c3d4e5f', status: '● validated', title: 'Spring tides are higher than usual and come near a new or full moon.', origin: 'tides.md', date: '2026-11-14', locator: '§1', citation: 'tides.md.' });
  // Every record the body cites has its row, as the daemon's check requires.
  const body = [p.lead, ...p.sections.map((s) => s.text)].join('\n');
  for (const label of cited(body)) assert.ok(p.bibliography.some((r) => r.label === label), label);
  // A page with nothing cited: the daemon's one row is not a record.
  const bare = parts((await page('lighthouse-lamps')).markdown);
  assert.deepEqual([bare.bibliography.length, bare.sections.length, bare.lead], [0, 0, 'No claims on this topic yet.']);
  // A `##` inside a code fence is not a section.
  assert.equal(parts('lead\n\n```\n## not a heading\n```\n\n## Real\n\ntext').sections.length, 1);
  assert.deepEqual(parts('').sections, []);
});

test('on a page, a marker is a button naming its records, a page link opens a topic and a section link moves to its heading', () => {
  const opened: string[] = [];
  const html = renderToStaticMarkup(<Rich text={'## Why twice a day\n\nThe moon pulls the sea [c-1, s-x]. See [Lamps](page:lighthouse-lamps), [above](#why-twice-a-day) and [a file](page:../etc).'} ctx={{ cite: (l) => opened.push(...l), openPage: () => {}, anchors: true }} />);
  assert.match(html, /<h3 class="md-h md-h2" id="sec-why-twice-a-day" tabindex="-1">/);
  assert.match(html, /<button type="button" class="cite"[^>]*aria-label="Citation: c-1, s-x"[^>]*>c-1, s-x<\/button>/);
  assert.match(html, /<button type="button" class="md-link" title="the topic page lighthouse-lamps"><span>Lamps<\/span><\/button>/);
  assert.match(html, /<button type="button" class="md-link" title="a section of this page"><span>above<\/span><\/button>/);
  // A `page:` link that is not a slug is shown, not followed.
  assert.match(html, /<span class="md-ref" title="page:\.\.\/etc"><span>a file<\/span><\/span>/);
  // Without the page's context a marker stays the text it is: a document of the knowledge base is not a generated page.
  const plain = renderToStaticMarkup(<Rich text={'The moon pulls the sea [c-1].'} />);
  assert.ok(!plain.includes('<button') && plain.includes('[c-1]'), plain);
  // What a page wrote is never HTML: a marker-shaped script stays text.
  const hostile = renderToStaticMarkup(<Rich text={'<img src=x onerror=alert(1)> [c-1<script>]'} ctx={{ cite: () => {} }} />);
  assert.ok(!hostile.includes('<img') && !hostile.includes('<script'), hostile);
});

test('AC-4: a citation marker leads to the claim, its anchor, and the source with the quoted passage highlighted', async () => {
  const b = await fixtureBridge();
  const p = parts((await page('tides')).markdown);
  // The marker on the page.
  const first = pieces(p.sections[0]!.text).find((x) => x.t === 'cite') as { cites: { label: string }[] };
  assert.deepEqual(first.cites.map((c) => c.label), ['c-fbebef7001b9', 's-local-tides-md-3277c799']);
  // The panel: the claim, its status, its anchors.
  const claim = (await b.ency('library.claim', { id: first.cites[0]!.label })) as ClaimView;
  assert.deepEqual([claim.claim.text, claim.shown, claim.claim.anchors.length], ['Tides come twice a day.', '○ not validated', 2]);
  // Choosing an anchor: the source, with the quote found in its text though it runs across a line end there.
  const anchor = claim.claim.anchors[1]!;
  const source = (await b.ency('library.source', { id: anchor.source_id, quote: anchor.quote })) as SourceView;
  const range = findQuote(source.text, anchor.quote!)!;
  assert.equal(source.text.slice(range.from, range.to), 'carries each coast\nthrough two bulges of water a day');
  const html = renderToStaticMarkup(<SourceText text={source.text} quote={anchor.quote!} />);
  assert.match(html, /turning earth <mark class="quote">carries each coast\nthrough two bulges of water a day<\/mark>\./);
  assert.equal((html.match(/<mark/g) ?? []).length, 1);
  // The whole text is still there, in order, around the mark.
  assert.equal(html.replace(/<[^>]+>/g, '').replace(/&#x27;/g, "'"), source.text);
});

test('a quotation is found whatever its spacing, and one that is not in the text is not invented', () => {
  const text = '# Tides\n\nThe tide comes in   twice a day.\nSpring tides are higher.';
  assert.deepEqual(findQuote(text, 'twice a day'), { from: 29, to: 40 });
  assert.equal(text.slice(29, 40), 'twice a day');
  const across = findQuote(text, 'a day. Spring  tides')!;
  assert.equal(text.slice(across.from, across.to), 'a day.\nSpring tides');
  assert.equal(findQuote(text, 'thrice a day'), null);
  assert.equal(findQuote(text, '   '), null);
  assert.equal(findQuote('', 'x'), null);
  // No mark when the quote is not in the text: the text alone.
  assert.ok(!renderToStaticMarkup(<SourceText text={text} quote="thrice a day" />).includes('<mark'));
});

test('the home page lists topics by letter and by project, and finds them by title, key term or open question', async () => {
  const b = await fixtureBridge();
  const topics = ((await b.ency('pages.list')) as { topics: Topic[] }).topics;
  const research = ((await b.ency('topics.list')) as { topics: Research[] }).topics;
  assert.deepEqual(byLetter(topics).map(([k, l]) => [k, l.map((t) => t.slug)]), [['H', ['harbour-ledger']], ['L', ['lighthouse-lamps']], ['T', ['tides']]]);
  assert.deepEqual(byLetter([{ ...topics[0]!, title: '2026 tide tables' }, { ...topics[0]!, title: 'élan' }]).map(([k]) => k), ['É', '#']);
  // A topic follows its research topic's project; one the operator named, or whose research has none, is the workspace's.
  assert.deepEqual(byProject(topics, research).map(([k, l]) => [k, l.map((t) => t.slug)]), [['Harbour & Co', ['harbour-ledger']], ['This workspace', ['lighthouse-lamps', 'tides']]]);
  assert.deepEqual(matching(topics, 'spring').map((t) => t.slug), ['tides'], 'a key term');
  assert.deepEqual(matching(topics, 'inner basin').map((t) => t.slug), ['tides'], 'an open question');
  assert.deepEqual(matching(topics, 'LAMPS').map((t) => t.slug), ['lighthouse-lamps']);
  assert.equal(matching(topics, '  ').length, 3);
  assert.deepEqual(topics.filter((t) => t.due).map((t) => [t.slug, t.due]), [['lighthouse-lamps', 'a claim it was built from changed'], ['harbour-ledger', 'never built']]);
});

test('the fixture answers the eleven reads in the daemon\'s shapes and refuses anything it does not hold', async () => {
  const b = await fixtureBridge();
  const reads: [Parameters<typeof b.ency>[0], Record<string, unknown>][] = [
    ['pages.list', {}], ['pages.get', { slug: 'tides' }], ['pages.versions', { slug: 'tides' }], ['pages.links', { slug: 'tides' }], ['topics.list', {}], ['topics.get', { id: 'when-do-tides-come-77eb208a' }],
    ['library.source', { id: 'local-tides-md-3277c799' }], ['library.claim', { id: 'c-fbebef7001b9' }], ['memory.roles', {}], ['memory.role', { role: 'scout' }], ['changes.since', { time: 0 }],
  ];
  assert.equal(reads.length, 11);
  for (const [read, arg] of reads) assert.ok(await b.ency(read, arg), read);
  assert.equal(((await b.ency('pages.get', { slug: 'tides', version: '20261114T000000Z' })) as TopicPage).version_shown, '20261114T000000Z');
  await assert.rejects(b.ency('pages.get', { slug: 'no-such' }), /no topic `no-such`/);
  await assert.rejects(b.ency('library.claim', { id: 'c-none' }), /no claim/);
  await assert.rejects(b.ency('kb.get' as never, {}), /not an encyclopedia read/);
  // The passages a note can be attached to are the page's prose, in order.
  assert.deepEqual(passages(parts(((await b.ency('pages.get', { slug: 'tides', version: '20261114T000000Z' })) as TopicPage).markdown)).map((t) => unmarked(t).slice(0, 24)), [
    'Tides are the regular ri', 'The moon pulls the sea t', 'Older tables give the ti', 'Spring tides are higher ',
  ]);
});

test('a page named in the address must be a slug or an id of the daemon\'s shapes', () => {
  assert.deepEqual(routeInAddress('?topic=tides'), { v: 'topic', slug: 'tides' });
  assert.deepEqual(routeInAddress('?topic=tides&version=20261114T000000Z'), { v: 'topic', slug: 'tides', version: '20261114T000000Z' });
  assert.deepEqual(routeInAddress('?source=s-local-tides-md-3277c799&quote=twice%20a%20day'), { v: 'source', id: 'local-tides-md-3277c799', quote: 'twice a day' });
  assert.deepEqual(routeInAddress('?claim=c-fbebef7001b9'), { v: 'claim', id: 'c-fbebef7001b9' });
  assert.deepEqual(routeInAddress('?research=when-do-tides-come-77eb208a'), { v: 'research', id: 'when-do-tides-come-77eb208a' });
  assert.deepEqual(routeInAddress('?changes'), { v: 'changes' });
  for (const bad of ['', '?topic=../x', '?topic=Tides', '?source=a/b', '?claim=..', '?claim=', '?research=x%20y', '?doc=a.md']) assert.equal(routeInAddress(bad), null, bad);
  assert.deepEqual(routeInAddress('?topic=tides&version=../x'), { v: 'topic', slug: 'tides' }, 'a version that is not a stamp is dropped');
});
