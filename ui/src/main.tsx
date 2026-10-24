import React from 'react';
import { render } from 'ink';
import { PassThrough } from 'node:stream';
import { readFileSync } from 'node:fs';
import { App } from './app.tsx';
import { Rpc } from './rpc.ts';
import { Term } from './terminal.ts';
import { fold, initial, replay } from './fold.ts';
import type { Info } from './theme.ts';

declare const BUILD_VERSION: string | undefined; // set by the release build (bun --define); absent when run from source
const VERSION = typeof BUILD_VERSION === 'string' ? BUILD_VERSION : 'dev';
if (process.argv.includes('--version')) { console.log(`delphi-ui ${VERSION}`); process.exit(0); }

function readInfo(): Info {
  const a = process.argv.indexOf('--info');
  if (a >= 0 && process.argv[a + 1]) return JSON.parse(process.argv[a + 1]!);
  const f = process.argv.indexOf('--info-file');
  if (f >= 0 && process.argv[f + 1]) return JSON.parse(readFileSync(process.argv[f + 1]!, 'utf8'));
  console.error('delphi-ui is started by `delphi chat`; it needs --info <json>');
  process.exit(2);
}

const info = readInfo();
if (VERSION !== 'dev' && VERSION !== info.version) {
  // Exit code 3 tells `delphi chat` to use the inline console instead.
  console.error(`delphi-ui ${VERSION} does not match delphi ${info.version}; run \`delphi self-update\` (using the inline console for now)`);
  process.exit(3);
}
let cmd: Rpc, ev: Rpc, mission: string, history = initial(Date.now());
try {
  cmd = await Rpc.connect(info.console_socket);
  await cmd.attachAsConsole();
  mission = (await cmd.call('mission.open', { mission_id: info.mission })).mission_id as string;
  ev = await Rpc.connect(info.console_socket);
  await ev.attachAsConsole();
  await ev.call('events.subscribe', {});
  const snap = await cmd.call('observe.snapshot', { mission_id: mission }).catch(() => ({ events: [] }));
  history = replay(initial(Date.now()), snap.events ?? []);
  const waiting = await cmd.call('approval.list', {}).catch(() => []);
  for (const a of waiting ?? []) history = fold(history, { method: 'event', params: { type: 'approval.requested', task_id: a.task_id, payload: a } }, Date.now());
  const live = await cmd.call('session.snapshot', { mission_id: mission }).catch(() => ({ running_tasks: [] }));
  for (const t of live.running_tasks ?? []) history.running[String(t.task_id)] = { since: Date.now() - Number(t.seconds ?? 0) * 1000 };
} catch (e: any) {
  console.error('delphi-ui: cannot attach to the daemon: ' + (e?.message ?? JSON.stringify(e)));
  process.exit(1);
}

const term = new Term();
let dark = true;
term.once('ev', () => {}); // keep the emitter alive until the app subscribes
term.start();
await new Promise<void>((done) => {
  const t = setTimeout(done, 250);
  term.on('ev', (e) => { if (e.t === 'bg') { dark = e.dark; clearTimeout(t); done(); } });
});
const app = render(<App cmd={cmd} ev={ev} term={term} info={info} mission={mission} dark={dark} history={history} />, {
  stdin: new PassThrough() as any, exitOnCtrlC: false, patchConsole: false,
  interactive: Boolean(process.stdout.isTTY), // a CI environment variable must not switch Ink to its print-once mode
});
await app.waitUntilExit();
term.stop();
cmd.close(); ev.close();
process.exit(0);
