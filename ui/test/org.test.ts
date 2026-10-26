import test from 'node:test';
import assert from 'node:assert/strict';
import * as O from '../src/org.ts';
import * as P from '../src/panel.ts';
import { initial } from '../src/fold.ts';
import { frame, type Model } from '../src/screen.ts';
import * as C from '../src/composer.ts';
import { newInbox } from '../src/inbox.ts';
import { width, type Info } from '../src/theme.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const pal = { dark: true, bg: [15, 23, 42] as [number, number, number] };
const text = (s: string): any => ({ t: 'text', s });
const key = (k: string): any => ({ t: 'key', k, alt: false, shift: false, ctrl: false });

const bundle = (over: Partial<O.OrgBundle> = {}): O.OrgBundle => ({ id: 'acme/plat', title: 'Platform', version: '1.4.0', provenance: 'unsigned', order: 10, review_by: '2999-01-01', stale: false, matched: true, source: { kind: 'directory', path: '/srv/acme' }, update_available: null, vectors: null, ...over });
const view = (over: Partial<O.OrgView> = {}): O.OrgView => ({
  bundles: [bundle()],
  goals: [{ bundle: 'acme/plat', id: 'G-acme-01', title: 'Cut p99 checkout latency', horizon: '2026-Q4 to 2027-Q1', measure: 'p99 weekly', status: 'active' }],
  perspectives: [{ bundle: 'acme/plat', id: 'P-acme-01', title: 'Boring', statement: 'prefer boring technology.' }],
  open: [], slice: { bytes: 900, cap: 4096, bundles: 1, trimmed: [] }, unavailable: ['rollback', 'map connector'], ...over,
});
const loaded = (v: O.OrgView = view()): O.OrgPanel => ({ ...O.openOrg(), view: v });
const shown = (p: O.OrgPanel, w = 110) => O.lines(p, pal, w).map(plain).join('\n');

test('the deck lists bundles with version, provenance, scope and freshness, and shows - for what is not known yet', () => {
  const s = shown(loaded(view({ bundles: [bundle(), bundle({ id: 'beta/ops', version: '0.3.0', stale: true, matched: false, review_by: '2026-01-01' })] })));
  assert.match(s, /acme\/plat\s+1\.4\.0\s+unsigned\s+matched\s+fresh \(2999-01-01\)\s+-\s+-/);
  assert.match(s, /beta\/ops\s+0\.3\.0\s+unsigned\s+unmatched\s+stale \(2026-01-01\)\s+-\s+-/, 'stale and unmatched are words, not only colour');
});

test('the vectors column shows how far the bundle is, as the daemon reports it', () => {
  const s = shown(loaded(view({ bundles: [bundle({ vectors: '42%' }), bundle({ id: 'beta/ops', vectors: '100% (shipped, verified)' })] })));
  assert.match(s, /acme\/plat .*\s42%/);
  assert.match(s, /beta\/ops .*\s100% \(shipped, verified\)/);
});

test('goals are a table, perspectives are listed, and open items say what needs a look', () => {
  const s = shown(loaded(view({ open: ['acme/plat is past its review date (2026-01-01)'], slice: { bytes: 4000, cap: 4096, bundles: 1, trimmed: ['acme/plat'] } })));
  assert.match(s, /Goals \(1\)/);
  assert.match(s, /G-acme-01\s+active\s+2026-Q4 to 2027-Q1\s+Cut p99 checkout latency · p99 weekly/);
  assert.match(s, /P-acme-01\s+prefer boring technology\./);
  assert.match(s, /Open items \(1\)\n\s+- acme\/plat is past its review date/);
  assert.match(s, /prompt slice is 4000 of 4096 bytes, trimmed for acme\/plat/);
});

test('rollback and map connector are listed as unavailable, never as actions', () => {
  const s = shown(loaded());
  assert.match(s, /not available yet: rollback, map connector/);
  assert.doesNotMatch(s, /^\s+(r|m)\s+(rollback|map)/m);
  assert.match(s, /No action asks for an approval/);
});

test('an empty organization says how to add a bundle, and an unreadable one says why', () => {
  assert.match(shown(loaded(view({ bundles: [], goals: [], perspectives: [] }))), /none added\. `a` adds a bundle from a directory/);
  assert.match(shown({ ...O.openOrg(), error: 'the daemon connection closed' }), /could not read the organization: the daemon connection closed/);
  assert.match(shown(O.openOrg()), /reading…/);
});

test('words that came from a bundle cannot move the cursor or restyle the screen', () => {
  const s = O.lines(loaded(view({ goals: [{ bundle: 'a/b', id: 'G-1', title: 'Hi\x1b[2J\x1b]0;owned\x07 there\nnow', horizon: 'h', measure: '', status: 'active' }] })), pal, 100).join('\n');
  assert.ok(!s.includes('\x1b[2J') && !s.includes('owned') && !s.includes('\x07'), JSON.stringify(s));
  assert.match(plain(s), /Hi there now/);
});

test('every line fits the width, and the view is exactly the height asked for', () => {
  const many = view({ goals: Array.from({ length: 30 }, (_, i) => ({ bundle: 'a/b', id: `G-${i}`, title: 'A long goal title '.repeat(8), horizon: '2026-Q4 to 2999-Q1', measure: 'm'.repeat(80), status: 'active' })) });
  for (const w of [40, 80, 120]) {
    for (const l of O.view(loaded(many), pal, w, 24)) assert.ok(width(l) <= w, `${w}: ${plain(l)}`);
    assert.equal(O.view(loaded(many), pal, w, 24).length, 24);
  }
  const down = O.view({ ...loaded(many), scroll: 5 }, pal, 100, 24).map(plain).join('\n');
  assert.doesNotMatch(down, /Organization\s+what the added/, 'scrolled past the title');
});

test('a asks for a directory, enter runs add with it, and nothing runs before enter', () => {
  let r = O.key(loaded(), text('a'));
  assert.equal(r.panel.asking, 'add');
  r = O.key(r.panel, text('/srv/'));
  r = O.key(r.panel, text('acme'));
  r = O.key(r.panel, key('backspace'));
  assert.deepEqual([r.panel.input, r.action.t], ['/srv/acm', 'none']);
  r = O.key(r.panel, text('e'));
  r = O.key(r.panel, key('enter'));
  assert.deepEqual(r.action, { t: 'run', body: { action: 'add', path: '/srv/acme' } });
  assert.equal(r.panel.busy, true);
  assert.equal(O.key(r.panel, text('a')).panel.asking, null, 'keys wait while it runs');
});

test('update and remove ask for a name, prefilled when only one bundle is added, and esc cancels', () => {
  let r = O.key(loaded(), text('u'));
  assert.deepEqual([r.panel.asking, r.panel.input], ['update', 'acme/plat']);
  r = O.key(r.panel, key('enter'));
  assert.deepEqual(r.action, { t: 'run', body: { action: 'update', name: 'acme/plat' } });
  const two = loaded(view({ bundles: [bundle(), bundle({ id: 'beta/ops' })] }));
  r = O.key(two, text('x'));
  assert.deepEqual([r.panel.asking, r.panel.input], ['remove', ''], 'with two bundles the operator names one');
  assert.equal(O.key(r.panel, key('enter')).action.t, 'none', 'an empty name runs nothing');
  r = O.key(O.key(r.panel, text('beta/ops')).panel, key('enter'));
  assert.deepEqual(r.action, { t: 'run', body: { action: 'remove', name: 'beta/ops' } });
  assert.equal(O.key(O.key(two, text('x')).panel, key('esc')).panel.asking, null);
  assert.equal(O.key(loaded(view({ bundles: [] })), text('x')).panel.asking, null, 'nothing to remove');
});

test('g reloads, esc and q close, and the panel is reachable through the shared panel keys', () => {
  assert.deepEqual(O.key(loaded(), text('g')).action, { t: 'refresh' });
  assert.deepEqual(O.key(loaded(), key('esc')).action, { t: 'close' });
  assert.deepEqual(O.key(loaded(), text('q')).action, { t: 'close' });
  const viaPanel = P.key(P.key(loaded(), text('a'), {}).panel, text('/x'), {});
  assert.equal(P.key(viaPanel.panel, key('enter'), {}).action.t, 'org');
  assert.deepEqual(P.key(loaded(), text('g'), {}).action, { t: 'org', action: { t: 'refresh' } });
  assert.match(P.hint(loaded()), /a add <dir> · u update <name> · x remove <name>/);
});

test('typing in the answer never runs a command key, and a pasted newline does not submit', () => {
  let r = O.key(loaded(), text('a'));
  r = O.key(r.panel, text('q'));
  r = O.key(r.panel, { t: 'paste', s: '/srv/a\ncme' });
  assert.equal(r.panel.input, 'q/srv/a cme');
  assert.equal(r.action.t, 'none');
});

test('directories are made absolute against the console, with ~ for the home directory', () => {
  assert.equal(O.resolveDir('/srv/acme/', '/work', '/h/op'), '/srv/acme');
  assert.equal(O.resolveDir('bundles/acme', '/work/p', '/h/op'), '/work/p/bundles/acme');
  assert.equal(O.resolveDir('~/b', '/work', '/h/op'), '/h/op/b');
  assert.equal(O.resolveDir('../b', '/work/p', undefined), '/work/b');
});

test('what the command printed is shown under the title, success or not', () => {
  const ok = O.resultOf('add', '/srv/acme', { ok: true, output: 'Acme (platform) acme@1.4.0\n\n  brings:\n\nadded acme/platform@1.4.0' });
  assert.deepEqual([ok.ok, ok.title, ok.lines.length], [true, 'add /srv/acme: done', 5]);
  const bad = O.resultOf('remove', 'x', { ok: false, output: 'delphi bundle: no added bundle named `x`' });
  assert.deepEqual([bad.ok, bad.title, bad.lines], [false, 'did not remove x', ['delphi bundle: no added bundle named `x`']]);
  const s = shown({ ...loaded(), result: bad });
  assert.match(s, /did not remove x\n\s+delphi bundle: no added bundle named/);
});

test('the console draws the deck over the chat area with its own key hint', () => {
  const info: Info = { console_socket: '', narrator_socket: '', workspace: '/w', mission: null, version: '0.5', cwd: '/p', roles: { orchestrator: { name: 'athena', chassis: '#a855f7', face: '•─•' } } };
  const m: Model = { info, pal, state: initial(0), now: 1000, deck: 1, composer: C.empty(), back: 0, help: false, menu: null, expandTools: false, search: null, queue: [], inbox: newInbox(0), mode: 'default', panel: loaded() };
  const screen = frame(m, 100, 30).map(plain).join('\n');
  assert.match(screen, /Organization/);
  assert.match(screen, /acme\/plat\s+1\.4\.0\s+unsigned/);
  assert.match(screen, /a add <dir>/);
});
