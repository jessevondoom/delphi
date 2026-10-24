import React from 'react';
import { render } from 'ink';
import { PassThrough } from 'node:stream';
import { readFileSync } from 'node:fs';
import { App } from './app.tsx';
import { Rpc } from './rpc.ts';
import { Term } from './terminal.ts';
import { initial, replay } from './fold.ts';
import { Observer } from './observer.ts';
import { envTheme, type Info } from './theme.ts';

function readInfo(): Info {
  const a = process.argv.indexOf('--info');
  if (a >= 0 && process.argv[a + 1]) return JSON.parse(process.argv[a + 1]!);
  const f = process.argv.indexOf('--info-file');
  if (f >= 0 && process.argv[f + 1]) return JSON.parse(readFileSync(process.argv[f + 1]!, 'utf8'));
  console.error('delphi-ui is started by `delphi chat`; it needs --info <json>');
  process.exit(2);
}

const info = readInfo();
let cmd: Rpc, ev: Rpc, mission: string, history = initial(Date.now());
let obs = new Observer(Object.keys(info.roles));
let cap0 = 0, pending0 = 0;
try {
  cmd = await Rpc.connect(info.console_socket);
  await cmd.attachAsConsole();
  mission = (await cmd.call('mission.open', { mission_id: info.mission })).mission_id as string;
  ev = await Rpc.connect(info.console_socket);
  await ev.attachAsConsole();
  await ev.call('events.subscribe', {});
  const snap = await cmd.call('observe.snapshot', { mission_id: mission }).catch(() => ({ events: [] }));
  history = replay(initial(Date.now()), snap.events ?? []);
  if (Array.isArray(snap.enabled_agents)) obs = new Observer(snap.enabled_agents);
  for (const e of snap.events ?? []) obs.apply(e);
  obs.setConnectors(snap.connectors);
  cap0 = Number(snap.budget?.cap_usd) || 0;
  pending0 = Number(snap.pending_proposals) || 0;
  const live = await cmd.call('session.snapshot', { mission_id: mission }).catch(() => ({ running_tasks: [] }));
  for (const t of live.running_tasks ?? []) history.running[String(t.task_id)] = { since: Date.now() - Number(t.seconds ?? 0) * 1000 };
} catch (e: any) {
  console.error('delphi-ui: cannot attach to the daemon: ' + (e?.message ?? JSON.stringify(e)));
  process.exit(1);
}

const term = new Term();
// Light or dark: DELPHI_THEME if set, else what the terminal answers to the background query, else COLORFGBG, else dark.
const env = envTheme();
let dark = env?.dark ?? true;
term.once('ev', () => {}); // keep the emitter alive until the app subscribes
term.start();
if (!env?.forced) {
  await new Promise<void>((done) => {
    const t = setTimeout(done, 250);
    term.on('ev', (e) => { if (e.t === 'bg') { dark = e.dark; clearTimeout(t); done(); } });
  });
}
const app = render(<App cmd={cmd} ev={ev} term={term} info={info} mission={mission} dark={dark} themeForced={Boolean(env?.forced)} history={history} obs={obs} cap0={cap0} pending0={pending0} />, {
  stdin: new PassThrough() as any, exitOnCtrlC: false, patchConsole: false,
  interactive: Boolean(process.stdout.isTTY), // a CI environment variable must not switch Ink to its print-once mode
});
try {
  await app.waitUntilExit();
} catch (e) { // a render error: leave the alternate screen first so the message stays readable
  term.stop();
  console.error('delphi-ui: the console stopped on an error:', e);
  cmd.close(); ev.close();
  process.exit(1);
}
term.stop();
cmd.close(); ev.close();
process.exit(0);
