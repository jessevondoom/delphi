// One-off agents in the Observer (spec 087 §4): a bundle agent's start event carries a name-pool slot; the daemon sends the
// pool (slot to name and face) with the start-up info; the console draws `⟦face⟧ Name (role)` in the role's colour, on its own
// roster row, after the six in the strip (blinking mechanically while it rests), and in the epic threads.
import test from 'node:test';
import assert from 'node:assert/strict';
import stringWidth from 'string-width';
import { activeAgents, observeFrame, type OModel, type OState } from '../src/observe-screen.ts';
import { mechanicalBlink, poolIdentity, roman, shownRole, useAsciiBadges, type Info } from '../src/theme.ts';
import { BLINK_SLOT_MS, blinkOffset } from '../src/motion.ts';
import { info as base, namedWorld, NOW } from './threads-fixture.ts';
import { snapshot } from './helpers.ts';

const plain = (s: string) => s.replace(/\x1b\[[0-9;]*m/g, '');
const info: Info = { ...base, pool: { 'pool-36': { name: 'Sisyphus', face: '×_×' }, 'pool-02': { name: 'Atlas', face: '•─•' } } };

/** The threads world with two one-off agents: the running builder wears pool-36, the queued red-team pool-02 (worn before, so `II`). */
function world(): OState {
  const s = namedWorld();
  s.nodes['d3']!.pool_slot = 'pool-36';
  // OD-6: the running one-off agent is a bundle agent whose own name the daemon sent as its label; the queued one has none to show
  // (the daemon refused it, or it had none), so it keeps its role.
  s.nodes['d3']!.bundle_agent = 'acme/plat:flaky-test-hunter';
  s.nodes['d3']!.label = 'flaky-test-hunter';
  s.nodes['d4']!.pool_slot = 'pool-02';
  s.nodes['d4']!.pool_numeral = 2;
  return s;
}
const model = (view: OModel['view'], over: Partial<OModel> = {}): OModel => ({ info, pal: { dark: true }, st: world(), view, pick: 0, scroll: 0, selNode: 0, live: {}, connected: true, still: true, now: NOW, ...over });
const frame = (view: OModel['view'], w: number, h: number, over: Partial<OModel> = {}) => observeFrame(model(view, over), w, h);

test('a slot is looked up in the theme the daemon sent; an unknown slot or a malformed face shows nothing', () => {
  assert.deepEqual(poolIdentity(info, 'pool-36'), { name: 'Sisyphus', face: '×_×' });
  assert.deepEqual(poolIdentity(info, 'pool-36', 2), { name: 'Sisyphus II', face: '×_×' });
  assert.equal(poolIdentity(info, 'pool-99'), undefined, 'a slot the theme does not have');
  assert.equal(poolIdentity(info, undefined), undefined);
  assert.equal(poolIdentity({ ...info, pool: undefined }, 'pool-36'), undefined, 'an older delphi sends no pool');
  assert.equal(poolIdentity({ ...info, pool: { 'pool-01': { name: 'X', face: 'toolong' } } }, 'pool-01'), undefined, 'a face is three cells');
  const hostile = poolIdentity({ ...info, pool: { 'pool-01': { name: 'Evil\x1b[2J\x07', face: '•_•' } } }, 'pool-01')!;
  assert.ok(!/[\x00-\x1f]/.test(hostile.name), 'control characters never reach the screen');
});

test('numerals: a name already worn gets a roman numeral', () => {
  assert.deepEqual([1, 2, 4, 9, 14, 36, 49, 99].map(roman), ['I', 'II', 'IV', 'IX', 'XIV', 'XXXVI', 'XLIX', 'XCIX']);
});

test('the roster shows each running one-off agent on its own row, and its role not a second time', () => {
  const st = world();
  const rows = activeAgents(st);
  const pooled = rows.filter((r) => r.pool);
  assert.deepEqual(pooled.map((r) => [r.pool!.slot, r.role]), [['pool-36', 'builder']], 'only the running one-off agent: the queued one is not at work');
  assert.ok(!rows.some((r) => !r.pool && r.role === 'builder' && r.state === 'running'), 'the builder\'s only running span is the one-off agent, so the role row is not shown again');
  const text = frame('agents', 100, 34).map(plain).join('\n');
  assert.ok(text.includes('⟦×_×⟧ Sisyphus (flaky-test-hunter)'), text);
  assert.ok(!text.includes('Sisyphus (builder)'), 'what it is for stands where the role was');
  assert.ok(text.includes('runs as builder'), 'the base role is on the detail line, where scope is shown');
  assert.ok(/× ?_ ?×/.test(text), 'the mascot wears the face of its name');
});

test('the strip adds the live one-off agents after the six, and a queued one blinks mechanically at a time of its own', () => {
  const strip = (over: Partial<OModel>) => plain(frame('agents', 100, 24, over)[0]!);
  const rest = strip({});
  assert.ok(rest.includes('⟦•─•⟧ ⟦×_×⟧'), `queued Atlas then running Sisyphus after the six: ${rest}`);
  // The queued agent's blink moment is a pure function of its node id, like a core role's.
  const slot = 5, at = slot * BLINK_SLOT_MS + blinkOffset('d4', slot) + 10;
  assert.equal(mechanicalBlink('•─•'), '-─-');
  assert.ok(strip({ now: at, still: false }).includes('⟦-─-⟧'), 'it blinks: the eyes close, the mouth stays');
  assert.ok(!strip({ now: at, still: true }).includes('⟦-─-⟧'), 'with reduce motion it never blinks');
  assert.ok(!strip({ now: at + 400, still: false }).includes('⟦-─-⟧'), 'and it opens again');
});

test('without the pool (an older delphi) the strip and the roster are what they were: the role, not a name', () => {
  const m = model('agents', { info: { ...info, pool: undefined } });
  const text = observeFrame(m, 100, 34).map(plain).join('\n');
  assert.ok(!text.includes('Sisyphus') && !text.includes('×_×'), text);
});

test('the epic threads name a one-off agent where they name the role', () => {
  const text = frame('epic', 100, 34).map(plain).join('\n');
  assert.ok(text.includes('⟦×_×⟧ Sisyphus (flaky-test-hunter)'), text);
});

test('every view fits the window with one-off agents on screen, at the sizes the Observer uses', () => {
  for (const view of ['agents', 'epic', 'dashboard'] as const) for (const [w, h] of [[110, 34], [70, 20], [40, 12]] as const) {
    const lines = frame(view, w, h);
    assert.equal(lines.length, h, `${view} ${w}x${h}`);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${view} ${w}x${h}: line wider than the window: ${plain(l)}`);
  }
});

test('golden: the roster and the epic threads with one-off agents', () => {
  for (const [name, view, w, h] of [['pool-agents-roster-100x34', 'agents', 100, 34], ['pool-agents-epic-100x34', 'epic', 100, 34]] as const) {
    const r = snapshot(name, frame(view, w, h));
    assert.ok(r.ok, r.message);
  }
});

// ---- OD-6: a one-off agent shows what it is for where its role would be (spec 087 §4.3 item 4) ----

test('shownRole: the label the daemon sent, cleaned, or the role when there is none', () => {
  assert.equal(shownRole('flaky-test-hunter', 'builder'), 'flaky-test-hunter');
  assert.equal(shownRole(undefined, 'builder'), 'builder', 'an older delphi sends no label');
  assert.equal(shownRole('', 'builder'), 'builder');
  assert.equal(shownRole('   ', 'builder'), 'builder');
  assert.equal(shownRole('  auth   flow\tattacker ', 'builder'), 'auth flow attacker');
  const hostile = shownRole('tester\x1b[2J\x07\u202e', 'builder');
  assert.ok(!/[\x00-\x1f\u202e]/.test(hostile), `no control or bidi character reaches the screen: ${JSON.stringify(hostile)}`);
});

test('a label the daemon did not send (refused, or none) leaves the role on the roster row, with no clipped-label detail', () => {
  const st = world();
  st.nodes['d4']!.state = 'running';
  st.nodes['d4']!.label = undefined;
  const text = observeFrame(model('agents', { st }), 100, 34).map(plain).join('\n');
  assert.ok(text.includes('⟦•─•⟧ Atlas II (red-team)'), text);
  assert.ok(text.includes('runs as red-team'), text);
});

test('a long label is clipped on the roster row and given whole on the detail line; every row still fits', () => {
  const long = 'reviews every migration for locking problems before it ships to production';
  const st = world();
  st.nodes['d3']!.label = long;
  for (const [w, h] of [[110, 34], [100, 34], [70, 20], [40, 12]] as const) {
    const lines = observeFrame(model('agents', { st }), w, h);
    for (const l of lines) assert.ok(stringWidth(l) <= w, `${w}x${h}: line wider than the window: ${plain(l)}`);
  }
  const text = observeFrame(model('agents', { st }), 110, 34).map(plain).join('\n');
  assert.ok(text.includes('⟦×_×⟧ Sisyphus (reviews every migration…)'), text);
  // The detail line gives the label as wide as the pane allows (110 columns holds 54 characters of it), with `…` when it cannot.
  assert.ok(text.includes('runs as builder · reviews every migration for locking problems before'), text);
  const wide = observeFrame(model('agents', { st: (() => { const s = world(); s.nodes['d3']!.label = 'x'.repeat(30); return s; })() }), 160, 34).map(plain).join('\n');
  assert.ok(wide.includes('runs as builder · ' + 'x'.repeat(30)), 'a wide pane shows the whole label');
  const epic = observeFrame(model('epic', { st }), 100, 34).map(plain).join('\n');
  assert.ok(epic.includes('Sisyphus (reviews every migration…)') || epic.includes('Sisyphus (reviews every migrati'), epic);
});

test('in ASCII mode a label with wide or non-ASCII characters is question marks, so no row moves', () => {
  const st = world();
  st.nodes['d3']!.label = '測試 runner •';
  const was = useAsciiBadges(true);
  try {
    assert.equal(shownRole('測試 runner •', 'builder'), '?? runner ?');
    const lines = observeFrame(model('agents', { st }), 100, 34);
    const text = lines.map(plain).join('\n');
    assert.ok(text.includes('(?? runner ?)'), text);
    assert.ok(!/[測試•]/.test(text.split('\n').filter((l) => l.includes('Sisyphus')).join('\n')), 'no wide character on the one-off agent\'s rows');
    for (const l of lines) assert.ok(stringWidth(l) <= 100);
  } finally { useAsciiBadges(was); }
  // With the switch off the label is drawn as sent.
  assert.equal(shownRole('測試 runner •', 'builder'), '測試 runner •');
});

test('the strip, the colours and the mascot of a one-off agent stay its base role\'s', () => {
  const a = frame('agents', 100, 34)[0]!;
  const b = observeFrame(model('agents', { st: (() => { const s = world(); s.nodes['d3']!.label = undefined; return s; })() }), 100, 34)[0]!;
  assert.equal(a, b, 'the strip does not depend on the label: it holds only the badge');
});
