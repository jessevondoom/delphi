import assert from 'node:assert/strict';
import { test } from 'node:test';
import { DEFAULTS, fromHash, load, parse, save } from '../src/prefs.ts';

test('stored preferences are read back, and anything damaged or unknown falls back to its default', () => {
  assert.deepEqual(parse(null), DEFAULTS);
  assert.deepEqual(parse('not json'), DEFAULTS);
  assert.deepEqual(parse('[1,2]').theme, 'system');
  assert.deepEqual(parse(JSON.stringify({ theme: 'light', menu: false, folded: { projects: true, x: 'yes' }, screen: 'council' })), { theme: 'light', menu: false, folded: { projects: true }, screen: 'council' });
  assert.equal(parse(JSON.stringify({ theme: 'neon', screen: 'nowhere' })).theme, 'system');
  assert.equal(parse(JSON.stringify({ screen: 'nowhere' })).screen, 'activity');
});

test('with no storage at all, loading gives the defaults and saving does nothing', () => {
  assert.deepEqual(load(), DEFAULTS);
  save(DEFAULTS);
});

test('a screen named in the address opens first, and any other word is ignored', () => {
  assert.equal(fromHash(DEFAULTS, '#council').screen, 'council');
  assert.equal(fromHash(DEFAULTS, '#nowhere').screen, 'activity');
  assert.equal(fromHash({ ...DEFAULTS, screen: 'output' }, '').screen, 'output');
});
