import React from 'react';
import { render } from 'ink';
import { PassThrough } from 'node:stream';
import { readFileSync } from 'node:fs';
import { App } from './app.tsx';
import { Rpc } from './rpc.ts';
import { Term } from './terminal.ts';
import { initial } from './fold.ts';
import { loadHistory } from './attach.ts';
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
const observeAt = process.argv.indexOf('--observe');
if (VERSION !== 'dev' && VERSION !== info.version) {
  // Exit code 3 tells `delphi chat` to use the inline console instead.
  console.error(`delphi-ui ${VERSION} does not match delphi ${info.version}; run \`delphi self-update\` (using the inline console for now)`);
  process.exit(3);
}
if (observeAt >= 0) {
  const { Observer } = await import('./observer.tsx');
  const { parseView } = await import('./observe-screen.ts');
  const arg = process.argv[observeAt + 1];
  const view = arg && !arg.startsWith('--') ? parseView(arg) : null;
  let cmd: Rpc, ev: Rpc;
  try {
    cmd = await Rpc.connect(info.narrator_socket);
    await cmd.attachAsNarrator();
    ev = await Rpc.connect(info.narrator_socket);
    await ev.attachAsNarrator();
    await ev.call('events.subscribe', {});
    cmd.keepAlive(async () => { await cmd.attachAsNarrator(); });
    ev.keepAlive(async () => { await ev.attachAsNarrator(); await ev.call('events.subscribe', {}); });
  } catch (e: any) {
    console.error('delphi observe: cannot attach to the daemon: ' + (e?.message ?? JSON.stringify(e)));
    process.exit(1);
  }
  const term = new Term({ mouse: !process.argv.includes('--no-mouse') && !process.env.DELPHI_NO_MOUSE });
  let dark = true;
  term.start();
  await new Promise<void>((done) => {
    const t = setTimeout(done, 250);
    term.on('ev', (e) => { if (e.t === 'bg') { dark = e.dark; clearTimeout(t); done(); } });
  });
  const app = render(<Observer cmd={cmd} ev={ev} term={term} info={info} dark={dark} view={view} />, { stdin: new PassThrough() as any, exitOnCtrlC: false, patchConsole: false, interactive: Boolean(process.stdout.isTTY) });
  term.on('resume', () => { app.clear(); app.rerender(<Observer cmd={cmd} ev={ev} term={term} info={info} dark={dark} view={view} />); });
  await app.waitUntilExit();
  term.stop();
  cmd.close(); ev.close();
  process.exit(0);
}

let cmd: Rpc, ev: Rpc, mission: string, history = initial(Date.now());
try {
  cmd = await Rpc.connect(info.console_socket);
  await cmd.attachAsConsole();
  mission = (await cmd.call('mission.open', { mission_id: info.mission })).mission_id as string;
  ev = await Rpc.connect(info.console_socket);
  await ev.attachAsConsole();
  await ev.call('events.subscribe', {});
  cmd.keepAlive(async () => { await cmd.attachAsConsole(); await cmd.call('mission.open', { mission_id: mission }); });
  ev.keepAlive(async () => { await ev.attachAsConsole(); await ev.call('events.subscribe', {}); });
  history = await loadHistory(cmd, mission);
} catch (e: any) {
  console.error('delphi-ui: cannot attach to the daemon: ' + (e?.message ?? JSON.stringify(e)));
  process.exit(1);
}

const term = new Term({ mouse: !process.argv.includes('--no-mouse') && !process.env.DELPHI_NO_MOUSE });
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
term.on('resume', () => { app.clear(); app.rerender(<App cmd={cmd} ev={ev} term={term} info={info} mission={mission} dark={dark} history={history} />); });
await app.waitUntilExit();
term.stop();
cmd.close(); ev.close();
process.exit(0);
