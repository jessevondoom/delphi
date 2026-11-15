import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { Agent, Plate } from '../src/Badge.tsx';
import { BUILT_IN, MARK_ROLES, activityOf, colorOf, faceOf, fillOf, motionOf, nameOf } from '../src/avatar.ts';
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
  assert.equal(colorOf(info, 'orchestrator', true), '#8f23f3');
  assert.equal(fillOf(info, 'orchestrator', true), '#ab59f6');
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

test('a chip is three face cells on a plate of the role\'s colour, the brighter fill on a light window', () => {
  const html = renderToStaticMarkup(<Plate info={info} role="orchestrator" light={false} />);
  assert.match(html, /class="bdg"/);
  assert.match(html, /color:#a855f7/);
  assert.equal((html.match(/<i>/g) ?? []).length, 3, html);
  assert.match(renderToStaticMarkup(<Plate info={info} role="orchestrator" light={true} />), /color:#ab59f6/);
  assert.match(renderToStaticMarkup(<Plate info={info} role="builder" light={false} activity="waiting_approval" />), /class="bdg wait"/);
  // A one-off agent wears its own face in its base role's colour.
  const one = renderToStaticMarkup(<Plate info={info} role="builder" light={false} face="^‿^" />);
  assert.ok(one.includes('>^<') && one.includes('>‿<') && one.includes('color:#f59e0b'), one);
});

test('an agent is always its chip, its name in bold in its colour, then its role in grey', () => {
  const named = { ...info, roles: { ...info.roles, architect: { ...info.roles.architect!, name: 'A Name' } } };
  const html = renderToStaticMarkup(<Agent info={named} role="architect" light={false} />);
  assert.match(html, /^<span class="agent-id"><span class="bdg"[^>]*>.*<\/span><span class="who" style="--c:#38bdf8">A Name<\/span><span class="role">\(architect\)<\/span><\/span>$/);
  // On a light window the name is the text colour and the chip the fill.
  const light = renderToStaticMarkup(<Agent info={named} role="architect" light={true} />);
  assert.ok(light.includes('--c:#076d9d') && light.includes('color:#088ac7'), light);
  // With no name of its own the role is written once; a one-off agent's label is its name.
  assert.ok(!renderToStaticMarkup(<Agent info={info} role="architect" light={false} />).includes('class="role"'));
  assert.ok(renderToStaticMarkup(<Agent info={info} role="architect" light={false} label="A one-off" />).includes('(architect)'));
  // Text from the daemon is text: markup in a name is escaped.
  assert.ok(renderToStaticMarkup(<Agent info={info} role="architect" light={false} label="<b>x</b>" />).includes('&lt;b&gt;'));
});

test('an older delphi that sends no fill gets the built-in light pair, never a mix of two sets', () => {
  const old = JSON.parse(JSON.stringify(info)) as typeof info;
  for (const r of Object.values(old.avatar!.roles)) { delete r.chassis_light_fill; r.chassis_light = '#111111'; }
  assert.equal(colorOf(old, 'orchestrator', true), '#8f23f3');
  assert.equal(fillOf(old, 'orchestrator', true), '#ab59f6');
  assert.equal(fillOf(old, 'orchestrator', false), '#a855f7');
  assert.equal(fillOf(info, 'someone-new', true), '#475569');
});
