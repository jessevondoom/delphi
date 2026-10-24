import test from 'node:test';
import assert from 'node:assert/strict';
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Rpc } from '../src/rpc.ts';

const sock = join(mkdtempSync(join(tmpdir(), 'dl-rpc-')), 'f.sock');
const daemon = (): ChildProcess => spawn('node', [join(dirname(fileURLToPath(import.meta.url)), '../dev/fake-daemon.mjs'), sock], { stdio: 'ignore' });
const until = async (cond: () => boolean, ms = 8000) => { const end = Date.now() + ms; while (!cond()) { if (Date.now() > end) throw new Error('timed out'); await new Promise((r) => setTimeout(r, 25)); } };
const once = (e: Rpc, name: string) => new Promise<void>((ok) => e.once(name, () => ok()));

test('a lost daemon fails calls at once, and the client reattaches by itself when it returns', async () => {
  let d = daemon();
  await until(() => { try { return existsSync(sock); } catch { return false; } }).catch(() => {});
  await new Promise((r) => setTimeout(r, 400));
  const rpc = await Rpc.connect(sock);
  let attached = 0;
  const attach = async () => { await rpc.attachAsConsole(); attached++; };
  await attach();
  rpc.keepAlive(attach);
  assert.equal((await rpc.call('mission.open', {})).mission_id, 'demo-mission');

  const closed = once(rpc, 'closed');
  d.kill('SIGKILL');
  await closed;
  await assert.rejects(rpc.call('mission.open', {}), (e: any) => /not connected|closed/.test(e.message), 'no call hangs while the daemon is away');

  const back = once(rpc, 'reconnected');
  d = daemon();
  await back;
  assert.equal(attached, 2, 'attached again after the daemon came back');
  assert.equal((await rpc.call('mission.open', {})).mission_id, 'demo-mission');
  rpc.close();
  d.kill('SIGKILL');
});

test('a call in flight when the connection drops is rejected, not left waiting', async () => {
  const d = daemon();
  await new Promise((r) => setTimeout(r, 500));
  const rpc = await Rpc.connect(sock);
  const slow = rpc.call('never.answers', {}).then(() => 'answered', (e) => e.message);
  // The fake daemon answers everything, so kill it before the reply can be read.
  d.kill('SIGKILL');
  const r = await Promise.race([slow, new Promise((ok) => setTimeout(() => ok('still waiting'), 5000))]);
  assert.notEqual(r, 'still waiting');
  rpc.close();
});
