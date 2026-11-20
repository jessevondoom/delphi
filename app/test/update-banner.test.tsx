import assert from 'node:assert/strict';
import { test } from 'node:test';
import { renderToStaticMarkup } from 'react-dom/server';
import { releaseVersion, updateBannerWords, type Daemon } from '../src/daemons.ts';
import { canUpdate, requestStates, stateWords, updateRequest } from '../src/requests.ts';
import { isCode } from '../src/PairCode.tsx';
import { UpdateBanner } from '../src/UpdateBanner.tsx';
import type { Conn } from '../src/types.ts';

const daemon = (more: Partial<Daemon> = {}): Daemon => ({ hash: 'aaaaaaaaaaaa', state: 'running', answers: true, workspace_name: 'default', workspace: '/w/default', home: '/h', version: '0.6.5', pid: 1, started: 't', summary: null, ...more });
const send = async () => ({});

test('a daemon that names a newer release shows it with delphi update to copy', () => {
  const w = updateBannerWords(daemon({ update_available: '0.6.6' }), false, undefined, false);
  assert.equal(w?.text, 'Update to v0.6.6 available');
  assert.equal(w?.command, 'delphi update');
  assert.equal(w?.canApply, false, 'not attached: nothing to press');
  const html = renderToStaticMarkup(<UpdateBanner d={daemon({ update_available: '0.6.6' })} attached={false} paired={false} />);
  assert.match(html, /Update to v0\.6\.6 available/);
  assert.match(html, /delphi update/);
  assert.doesNotMatch(html, /Update now/);
});

test('Update now is offered only to an attached window whose key holds update', () => {
  const d = daemon({ update_available: '0.6.6' });
  const has = (attached: boolean, grants: string[] | undefined, paired: boolean) => renderToStaticMarkup(<UpdateBanner d={d} attached={attached} grants={grants} paired={paired} send={send} />).includes('Update now');
  assert.equal(has(true, ['read', 'update'], true), true);
  assert.equal(has(true, ['read', 'request'], true), false, 'a key without update only copies');
  assert.equal(has(true, ['read', 'update'], false), false, 'an unpaired window holds no grants');
  assert.equal(has(false, ['read', 'update'], true), false, 'another daemon than the attached one');
  // Without a send function there is nothing to press either.
  assert.doesNotMatch(renderToStaticMarkup(<UpdateBanner d={d} attached grants={['update']} paired />), /Update now/);
  const conn = (grants: string[]): Conn => ({ connected: true, detail: '', paired: 'app-0a1b2c', grants });
  assert.equal(canUpdate(conn(['read', 'update'])), true);
  assert.equal(canUpdate(conn(['read', 'request'])), false);
  assert.equal(canUpdate({ connected: true, detail: '' }), false);
});

test('the update request carries no parameter at all', () => {
  assert.deepEqual(updateRequest(), { what: 'update.apply', arg: {} });
});

test('a release is a number, never text, and nothing is shown for a daemon that names none', () => {
  assert.equal(releaseVersion('0.6.6'), '0.6.6');
  for (const bad of ['', 'v0.6.6', '0.6', '0.6.6-rc1', '<b>0.6.6</b>', '0.6.6\n', 7, null, undefined]) assert.equal(releaseVersion(bad), null, JSON.stringify(bad));
  assert.equal(updateBannerWords(daemon(), true, ['update'], true), null);
  assert.equal(updateBannerWords(daemon({ update_available: null }), true, ['update'], true), null);
  assert.equal(updateBannerWords(daemon({ update_available: '<script>' }), true, ['update'], true), null);
  assert.equal(renderToStaticMarkup(<UpdateBanner d={daemon({ update_available: 'x' })} attached paired grants={['update']} send={send} />), '');
});

test('after an update the banner says restart when idle with a command to copy, and never a button that restarts', () => {
  const w = updateBannerWords(daemon({ restart_pending: true, update_available: '0.6.7' }), true, ['update'], true);
  assert.equal(w?.restart?.command, 'delphi daemon restart --workspace default');
  assert.equal(w?.canApply, false, 'no second update while a restart is due');
  const html = renderToStaticMarkup(<UpdateBanner d={daemon({ restart_pending: true })} attached paired grants={['update']} send={send} />);
  assert.match(html, /restart when idle/);
  assert.match(html, /delphi daemon restart --workspace default/);
  assert.doesNotMatch(html, /Update now/);
  // A workspace name that is not a plain one is never put into a command.
  assert.equal(updateBannerWords(daemon({ restart_pending: true, workspace_name: '-rf; x' }), true, [], true), null);
});

test('the code the daemon gave for a waiting update is shown only when it is in the shape the daemon makes', async () => {
  const d = daemon({ update_available: '0.6.6' });
  for (const [given, shown] of [['K7Q-4M2', true], ['k7q-4m2', false], ['K70-4M2', false], ['<b>x</b>', false], ['', false]] as const) {
    assert.equal(isCode(given), shown, given);
  }
  // The banner keeps what the daemon answered in its own state and prints it; renderToStaticMarkup cannot press the button, so the shape check is the test.
  assert.match(renderToStaticMarkup(<UpdateBanner d={d} attached paired grants={['update']} send={async () => ({ state: 'waiting', code: 'K7Q-4M2' })} />), /Update now/);
});

test('a request that waits for approval is said to wait at the terminal, and cannot be approved here', () => {
  const states = requestStates([{ type: 'app.request', payload: { request: 'aq-1', method: 'app.update_apply', ids: {}, state: 'waiting' } } as never]);
  assert.equal(states.get('aq-1')?.state, 'waiting');
  assert.match(stateWords({ state: 'waiting' }), /delphi app approve/);
  assert.match(stateWords({ state: 'waiting' }), /type the code shown here/);
  const html = renderToStaticMarkup(<UpdateBanner d={daemon({ update_available: '0.6.6' })} attached paired grants={['update']} send={send} events={[{ type: 'app.request', payload: { request: 'aq-1', method: 'app.update_apply', ids: {}, state: 'waiting' } } as never]} />);
  assert.doesNotMatch(html, /Update now/, 'one request at a time');
  assert.match(html, /Waiting for you to approve this at your terminal/);
});
