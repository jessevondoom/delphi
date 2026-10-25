import React from 'react';
import { render } from 'ink';
import { PassThrough } from 'node:stream';
import { readFileSync } from 'node:fs';
import { App } from './app.tsx';
import { Rpc } from './rpc.ts';
import { Term } from './terminal.ts';
import { initial } from './fold.ts';
import { loadHistory } from './attach.ts';
import { capabilities, envTheme, useCapabilities, type Info, type Rgb } from './theme.ts';

declare const BUILD_VERSION: string | undefined; // set by the release build (bun --define); absent when run from source
const VERSION = typeof BUILD_VERSION === 'string' ? BUILD_VERSION : 'dev';
if (process.argv.includes('--version')) { console.log(`delphi-ui ${VERSION}`); process.exit(0); }
useCapabilities(capabilities()); // NO_COLOR and TERM=dumb, read once, for the console and the narrator alike

/**
 * Light or dark: `DELPHI_THEME` if set; else what the terminal answers to the background query; else `COLORFGBG`;
 * else dark. Call after `term.start()`, which sends the query.
 */
async function detectDark(term: Term): Promise<{ dark: boolean; forced: boolean; bg: Rgb | null }> {
  const env = envTheme();
  let dark = env?.dark ?? true;
  let bg: Rgb | null = null;
  // Even a forced theme waits for the answer: the colour itself is what the message tint is computed from.
  await new Promise<void>((done) => {
    const t = setTimeout(done, 250);
    term.on('ev', (e) => { if (e.t === 'bg') { if (!env?.forced) dark = e.dark; bg = e.rgb; clearTimeout(t); done(); } });
  });
  return { dark, forced: Boolean(env?.forced), bg };
}

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
  console.error(`delphi-ui ${VERSION} does not match delphi ${info.version}; run \`delphi update\` (using the inline console for now)`);
  process.exit(3);
}
if (observeAt >= 0) {
  const { Observer } = await import('./observer.tsx');
  const { parseView } = await import('./observe-screen.ts');
  const arg = process.argv[observeAt + 1];
  const view = arg && !arg.startsWith('--') ? parseView(arg) : null;
  if (process.argv.includes('--once')) {
    // One frame as text, no terminal needed: the size is the terminal's when there is one, else 80x24.
    const { observeFrame, emptyState } = await import('./observe-screen.ts');
    try {
      const c = await Rpc.connect(info.narrator_socket);
      await c.attachAsNarrator();
      const st = { ...emptyState(), ...(await c.call('observe.state', {})) };
      const shown = view ?? (st.default_view as typeof view) ?? 'epic';
      const b = shown === 'forge' ? st.builds[st.builds.length - 1] : undefined;
      const diff = b ? await c.call('observe.diff', { task_id: b.task_id }).catch(() => null) : null;
      c.close();
      const lines = observeFrame({ info, pal: { dark: true }, st, view: shown ?? 'epic', pick: Number.MAX_SAFE_INTEGER, scroll: 0, selNode: 0, search: { editing: false, query: '', hits: [], note: '' }, diff, live: {}, connected: true, still: true, now: 0 }, process.stdout.columns || 80, process.stdout.rows || 24);
      console.log(lines.map((l) => l.replace(/\x1b\[[0-9;]*m/g, '').trimEnd()).join('\n'));
      process.exit(0);
    } catch (e: any) {
      console.error('delphi observe: cannot read the daemon: ' + (e?.message ?? JSON.stringify(e)));
      process.exit(1);
    }
  }
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
  term.start();
  const { dark, forced } = await detectDark(term);
  const app = render(<Observer cmd={cmd} ev={ev} term={term} info={info} dark={dark} themeForced={forced} view={view} />, { stdin: new PassThrough() as any, exitOnCtrlC: false, patchConsole: false, interactive: Boolean(process.stdout.isTTY) });
  term.on('resume', () => { app.clear(); app.rerender(<Observer cmd={cmd} ev={ev} term={term} info={info} dark={dark} themeForced={forced} view={view} />); });
  try {
    await app.waitUntilExit();
  } catch (e) { // a render error: leave the alternate screen first so the message stays readable
    term.stop();
    console.error('delphi observe: the Observer stopped on an error:', e);
    cmd.close(); ev.close();
    process.exit(1);
  }
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
term.once('ev', () => {}); // keep the emitter alive until the app subscribes
term.start();
const { dark, forced, bg } = await detectDark(term);
const app = render(<App cmd={cmd} ev={ev} term={term} info={info} mission={mission} dark={dark} bg={bg} themeForced={forced} history={history} />, {
  stdin: new PassThrough() as any, exitOnCtrlC: false, patchConsole: false,
  interactive: Boolean(process.stdout.isTTY), // a CI environment variable must not switch Ink to its print-once mode
});
term.on('resume', () => { app.clear(); app.rerender(<App cmd={cmd} ev={ev} term={term} info={info} mission={mission} dark={dark} bg={bg} themeForced={forced} history={history} />); });
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
