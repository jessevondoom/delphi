import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Badge, Who } from '../src/Badge.tsx';
import { BUILT_IN, MARK_ROLES, activityOf, colorOf, faceOf, motionOf, nameOf } from '../src/avatar.ts';
import { fixtureInfo } from '../src/bridge.ts';

const info = fixtureInfo();

test("the built-in avatar table is the console library's, unchanged", () => {
  const consoles = JSON.parse(readFileSync(new URL('../../ui/test/fixtures/avatar-spec.json', import.meta.url), 'utf8'));
  assert.deepEqual(BUILT_IN, consoles);
});

test('every role has a face of three cells for every activity, with at most one mouth', () => {
  const roles = Object.keys(BUILT_IN.roles);
  assert.ok(roles.length >= 13);
  for (const role of roles) {
    for (const activity of Object.keys(BUILT_IN.roles[role]!.badge)) {
      const f = faceOf(info, role, activity);
      assert.equal(f.cells.length, 3, `${role} ${activity}`);
      assert.ok(f.mouth === null || (f.mouth >= 0 && f.mouth <= 2), `${role} ${activity}`);
    }
    assert.match(colorOf(info, role, false), /^#[0-9a-f]{6}$/);
    assert.match(colorOf(info, role, true), /^#[0-9a-f]{6}$/);
  }
  assert.equal(colorOf(info, 'orchestrator', false), '#a855f7');
  assert.equal(colorOf(info, 'orchestrator', true), '#7e22ce');
  // The scout's cells are all eyes; a role the library does not know gets a neutral face and colour, never a crash.
  assert.equal(faceOf(info, 'scout', 'idle').mouth, null);
  assert.deepEqual(faceOf(info, 'someone-new', 'idle').cells, ['•', '_', '•']);
  assert.equal(colorOf(info, 'someone-new', true), '#475569');
  assert.equal(nameOf(info, 'someone-new'), 'someone-new');
  assert.deepEqual(MARK_ROLES, ['orchestrator', 'strategist', 'architect', 'scout', 'red-team', 'builder']);
});

test('a daemon state maps to an activity, and an activity to its motion', () => {
  assert.equal(activityOf('waiting on you'), 'waiting_approval');
  assert.equal(activityOf('running'), 'working');
  assert.equal(activityOf('idle'), 'idle');
  assert.equal(activityOf('anything else'), 'idle');
  assert.equal(motionOf('waiting_approval'), 'wait');
  assert.equal(motionOf('thinking_left'), 'think');
  assert.equal(motionOf('idle'), '');
  assert.equal(motionOf('success'), 'still');
});

test("a badge draws the mouth in the role's colour and the other cells as eyes; a plate has no eyes", () => {
  const html = renderToStaticMarkup(<Badge info={info} role="orchestrator" light={false} />);
  assert.match(html, /class="bdg"/);
  assert.match(html, /color:#a855f7/);
  assert.equal((html.match(/<i class="e">/g) ?? []).length, 2, html);
  assert.equal((html.match(/<i>/g) ?? []).length, 1, html);
  const scout = renderToStaticMarkup(<Badge info={info} role="scout" light={true} />);
  assert.equal((scout.match(/<i class="e">/g) ?? []).length, 3);
  assert.match(scout, /color:#854d0e/);
  const plate = renderToStaticMarkup(<Badge info={info} role="builder" light={false} plate activity="waiting_approval" />);
  assert.match(plate, /class="bdg plate wait"/);
  assert.ok(!plate.includes('class="e"'));
  // A one-off agent wears its own face in its base role's colour.
  const one = renderToStaticMarkup(<Badge info={info} role="builder" light={false} face="^‿^" />);
  assert.ok(one.includes('>^<') && one.includes('>‿<') && one.includes('color:#f59e0b'), one);
});

test("a name is in the role's colour, and the role follows only when the name is something else", () => {
  const plain = renderToStaticMarkup(<Who info={info} role="architect" light={false} />);
  assert.match(plain, /--c:#38bdf8/);
  assert.ok(!plain.includes('class="role"'));
  const named = renderToStaticMarkup(<Who info={info} role="architect" light={false} label="A one-off" />);
  assert.ok(named.includes('A one-off') && named.includes('(architect)'));
  // Text from the daemon is text: markup in a name is escaped.
  assert.ok(renderToStaticMarkup(<Who info={info} role="architect" light={false} label="<b>x</b>" />).includes('&lt;b&gt;'));
});
