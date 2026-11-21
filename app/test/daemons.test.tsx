import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { fixtureBridge, fixtureDaemons } from '../src/bridge.ts';
import {
  INSTALL_COMMAND, START_COMMAND, selfInstallCommand, agoWords, cardOf, cardState, cards, orderDaemons, showStart, startCommand, summaryLines, switchable, tokenWords, versionNote,
  type Daemon, type DaemonList, type Summary,
} from '../src/daemons.ts';
import { Cards, Start } from '../src/screens/Start.tsx';

const NOW = Date.parse('2026-11-19T12:00:00Z');
const quiet: Summary = { running: 0, waiting: 0, escalated: 0, approvals: 0, oldest_approval_ms: null, mission: null, session: null, tokens: 0 };
const daemon = (hash: string, name: string, state: Daemon['state'], summary: Summary | null, version = '0.6.5'): Daemon => ({ hash, state, answers: state === 'running', workspace_name: name, workspace: `/Users/me/.delphi/workspaces/${name}`, home: '/Users/me/.delphi', version, pid: 1, started: '', summary });
const list = (daemons: Daemon[], held = false): DaemonList => ({ held, current: null, app_version: '0.6.5', daemons });

test('a card says needs you, working, idle, stopped or older daemon, and sorts in that order', () => {
  const sample = [
    daemon('000000000006', 'z-old', 'stopped', null),
    { ...daemon('000000000005', '', 'running', null), state: 'older' as const },
    daemon('000000000004', 'quiet', 'running', quiet),
    daemon('000000000003', 'busy', 'running', { ...quiet, running: 2 }),
    daemon('000000000002', 'b-asks', 'running', { ...quiet, running: 1, approvals: 1, oldest_approval_ms: NOW - 240_000 }),
    daemon('000000000001', 'a-waits', 'running', { ...quiet, waiting: 1 }),
    daemon('000000000007', 'escalated', 'running', { ...quiet, escalated: 1 }),
    daemon('000000000008', 'unread', 'running', null),
  ];
  assert.deepEqual(sample.map(cardState), ['stopped', 'older daemon', 'idle', 'working', 'needs you', 'needs you', 'needs you', 'running']);
  assert.deepEqual(orderDaemons(sample).map((d) => d.workspace_name), ['a-waits', 'b-asks', 'escalated', 'busy', 'quiet', 'unread', 'z-old', '']);
  // A refresh never shuffles equals: the same list in another order is drawn in the same order.
  assert.deepEqual(orderDaemons([...sample].reverse()).map((d) => d.hash), orderDaemons(sample).map((d) => d.hash));
});

test('the counts are words, tokens and never dollars', () => {
  assert.deepEqual(summaryLines({ running: 2, waiting: 1, escalated: 1, approvals: 3, oldest_approval_ms: NOW - 240_000, mission: 'm-1', session: 's-2', tokens: 482_100 }, NOW), [
    '3 approvals waiting, the oldest for 4 min', '1 agent waiting on you', '1 agent escalated', '2 agents running', 'session s-2 in mission m-1', '482k tokens',
  ]);
  assert.deepEqual(summaryLines(quiet, NOW), ['no agent running', '0 tokens']);
  assert.deepEqual(summaryLines(null, NOW), []);
  assert.deepEqual([1, 999, 1000, 12_300, 1_250_000].map(tokenWords), ['1 token', '999 tokens', '1k tokens', '12.3k tokens', '1.3M tokens']);
  assert.deepEqual([5_000, 125_000, 7_200_000, 200_000_000].map((ms) => agoWords(NOW - ms, NOW)), ['5 s', '2 min', '2 h', '2 d']);
  const text = JSON.stringify(cards(list(fixtureDaemons(NOW)), NOW));
  assert.ok(!/\$|usd|dollar/i.test(text), text);
});

test('a daemon of another version says both, and a build with no release version says nothing', () => {
  assert.equal(versionNote('0.6.4', '0.6.5'), 'daemon 0.6.4, app 0.6.5. `delphi update` restarts idle daemons.');
  assert.equal(versionNote('0.6.5', '0.6.5'), null);
  assert.equal(versionNote('0.6.4', '0.0.0'), null);
  assert.equal(versionNote('', '0.6.5'), null);
  const c = cardOf(daemon('1', 'old', 'running', quiet, '0.6.4'), NOW, '0.6.5');
  assert.match(c.versionNote ?? '', /daemon 0\.6\.4, app 0\.6\.5/);
});

test('the command that starts a stopped workspace is composed only from a plain name', () => {
  assert.equal(startCommand(daemon('1', 'client-work', 'stopped', null)), 'delphi --workspace client-work');
  for (const bad of ['', '-rf', 'a b', 'a;b', '$(id)', 'a\nb', '`x`', 'x'.repeat(65)]) assert.equal(startCommand(daemon('1', bad, 'stopped', null)), null, JSON.stringify(bad));
  assert.equal(START_COMMAND, 'delphi chat');
  assert.match(INSTALL_COMMAND, /releases\/latest\/download\/install\.sh \| sh$/);
});

test('with two daemons and none named the window shows the start screen, with one it attaches as before, with none the empty state (AC-6)', () => {
  const two = list([daemon('1', 'a', 'running', quiet), daemon('2', 'b', 'running', quiet)], true);
  const one = list([daemon('1', 'a', 'running', quiet), daemon('2', 'old', 'stopped', null)]);
  const none = list([]);
  assert.equal(showStart(two, false, false), true, 'held: the bridge waits for a choice');
  assert.equal(showStart(one, true, true), false);
  assert.equal(showStart(one, false, false), false, 'one runs, and is being attached');
  assert.equal(showStart(none, false, false), true, 'nothing runs, nothing is attached');
  assert.equal(showStart(list([daemon('2', 'old', 'stopped', null)]), false, false), true, 'only a stopped one is no daemon');
  assert.equal(showStart(none, true, true), false, 'what is shown stays while a daemon goes away');
  assert.equal(showStart(null, false, false), false, 'before the first list there is nothing to say');
  assert.deepEqual([two, one, none, null].map(switchable), [2, 1, 0, 0]);
  // A page of two, drawn: both cards, as buttons, and no empty-state words.
  const html = renderToStaticMarkup(<Start list={two} now={NOW} conn={null} onChoose={() => {}} error="" />);
  assert.match(html, /Choose a daemon/);
  assert.equal((html.match(/<button/g) ?? []).length, 2);
  assert.ok(!html.includes('Nothing is running'));
  // None: the empty state names the command and, beside it, how to install.
  const empty = renderToStaticMarkup(<Start list={none} now={NOW} conn={null} onChoose={() => {}} error="" />);
  assert.match(empty, /Nothing is running\. Start Delphi in a terminal/);
  assert.ok(empty.includes('delphi chat') && empty.includes(INSTALL_COMMAND));
  assert.equal((empty.match(/>Copy command</g) ?? []).length, 2, 'a copy button for each command');
});

test('a card that needs you sorts first, says where to answer, and has no button that answers (AC-8)', () => {
  const l = list([daemon('2', 'calm', 'running', { ...quiet, running: 1 }), daemon('1', 'asks', 'running', { ...quiet, approvals: 2, oldest_approval_ms: NOW - 60_000 })]);
  const html = renderToStaticMarkup(<Cards list={l} now={NOW} current={null} conn={null} onChoose={() => {}} />);
  assert.ok(html.indexOf('asks') < html.indexOf('calm'), 'needs you first');
  assert.ok(html.includes('Answer it in the console.') && html.includes('2 approvals waiting'));
  const buttons = [...html.matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g)].map((m) => m[1]!.replace(/<[^>]*>/g, ' '));
  assert.equal(buttons.length, 2, 'one button per card, and it attaches');
  assert.ok(!buttons.some((b) => /approve|deny|answer|allow|yes|reject/i.test(b.replace('Answer it in the console.', ''))), buttons.join('|'));
});

test('a stopped card offers a copy button and no button that starts a process (AC-9)', () => {
  const html = renderToStaticMarkup(<Cards list={list([daemon('4', 'client-work', 'stopped', null)])} now={NOW} current={null} conn={null} onChoose={() => {}} />);
  const buttons = [...html.matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g)].map((m) => m[1]);
  assert.deepEqual(buttons, ['Copy command']);
  assert.ok(html.includes('delphi --workspace client-work') && html.includes('stopped'));
  // Nothing in the window's source asks the bridge to start anything: no command of that name in either bridge.
  for (const file of ['../src/bridge.ts', '../src/bridge-app.ts', '../src/screens/Start.tsx']) {
    const src = readFileSync(new URL(file, import.meta.url), 'utf8');
    assert.ok(!/\b(start_daemon|spawn|startDaemon|exec)\b/.test(src.replace(/\/\/.*$/gm, '').replace(/\/\*[\s\S]*?\*\//g, '')), file);
  }
  // An older daemon that does not answer has no button either; one that answers is a button that attaches.
  const older = (answers: boolean) => renderToStaticMarkup(<Cards list={list([{ ...daemon('5', '', 'running', null), state: 'older', answers }])} now={NOW} current={null} conn={null} onChoose={() => {}} />);
  assert.equal((older(false).match(/<button/g) ?? []).length, 0);
  assert.equal((older(true).match(/<button/g) ?? []).length, 1);
  assert.match(older(true), /daemon 5/);
});

test('the sample daemons show the start screen in a browser and attach() takes only a hash of the list (AC-7)', async () => {
  const b = await fixtureBridge();
  const seen: boolean[] = [];
  b.subscribe({ event() {}, conn: (c) => seen.push(c.connected), resync() {} });
  assert.deepEqual(seen, [true], 'with no browser location the sample is one daemon, attached');
  const l = await b.daemons(true);
  assert.equal(l.daemons.length, 1);
  assert.equal(l.held, false);
  await assert.rejects(b.attach('ffffffffffff'), /not a daemon in the list/);
  await b.attach(l.daemons[0]!.hash);
  assert.equal((await b.daemons(false)).current, l.daemons[0]!.hash);
  // The full sample has every state the screen draws.
  assert.deepEqual(new Set(fixtureDaemons(NOW).map((d) => cardState(d))), new Set(['needs you', 'working', 'idle', 'stopped', 'older daemon']));
});

test('nothing on the start screen moves, and the reduce-motion rule says so (AC-10)', () => {
  const css = readFileSync(new URL('../src/theme.css', import.meta.url), 'utf8');
  const from = css.indexOf('/* The start screen and the switcher');
  const block = css.slice(from, css.indexOf('@media print', from));
  const outsideMotion = block.replace(/@media \(prefers-reduced-motion: reduce\)[^\n]*/g, '');
  assert.ok(!/@keyframes|animation\s*:|transition\s*:/.test(outsideMotion), 'no animation or transition is declared for it');
  assert.match(block, /@media \(prefers-reduced-motion: reduce\) \{ \.start \*, \.switcher \*[^}]*animation: none !important; transition: none !important/);
  const html = renderToStaticMarkup(<Start list={list(fixtureDaemons(NOW))} now={NOW} conn={null} onChoose={() => {}} error="" />);
  assert.ok(!/style="[^"]*(animation|transition)/.test(html) && !/class="[^"]*(spin|pulse|blink)/.test(html));
});

test('with no installed delphi and a bundled one, the empty state names its self-install command (spec 122 AC-11)', () => {
  const bundled = '/Applications/Delphi.app/Contents/Resources/delphi';
  const none = { ...list([]), program: { installed: false, bundled } };
  const html = renderToStaticMarkup(<Start list={none} now={NOW} conn={null} onChoose={() => {}} error="" />);
  assert.ok(html.includes('Install the delphi command'));
  assert.ok(html.includes(`&quot;${bundled}&quot; self-install`));
  assert.equal((html.match(/>Copy command</g) ?? []).length, 3, 'a copy button for each command');
  // An installed delphi, or no bundled copy: no offer.
  for (const program of [{ installed: true, bundled }, { installed: false, bundled: null }, undefined]) {
    assert.ok(!renderToStaticMarkup(<Start list={{ ...none, program }} now={NOW} conn={null} onChoose={() => {}} error="" />).includes('Install the delphi command'));
  }
  // A path a shell would read inside double quotes is single-quoted; a relative one is not offered.
  assert.equal(selfInstallCommand({ installed: false, bundled: '/a b/$x/delphi' }), "'/a b/$x/delphi' self-install");
  assert.equal(selfInstallCommand({ installed: false, bundled: 'delphi' }), null);
  // A control character, DEL or a line or paragraph separator in the path: no command is offered (V3-14).
  for (const odd of ['\n', '\u007f', '\u2028', '\u2029']) {
    assert.equal(selfInstallCommand({ installed: false, bundled: `/a${odd}b/delphi` }), null, JSON.stringify(odd));
  }
});
