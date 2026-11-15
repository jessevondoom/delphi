// The browser check of the Mermaid renderer (spec 118 §5 item 4): build `test/browser/diagram.html` with the window's own
// code and the bundled renderer, open it in headless Chrome, draw every case of `test/browser/cases.ts`, and check that
// no hostile diagram ran script, navigated, opened a window or fetched anything, that every drawing left is clean, and
// that the plain ones are drawn. The page has no Content Security Policy, so the cleaning is tested on its own.
// Run from app/: `npm run check-diagrams` (`node --import tsx dev/diagram-check.mjs`). Needs Chrome or Chromium (`CHROME=<path>` to name one); fails without.
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import http from 'node:http';
import { tmpdir } from 'node:os';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';

const app = fileURLToPath(new URL('..', import.meta.url));
const problems = [];
const say = (s) => console.log(`diagram-check: ${s}`);

const chrome = [process.env.CHROME, '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Chromium.app/Contents/MacOS/Chromium', '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser'].find((p) => p && existsSync(p));
if (!chrome) { say('no Chrome or Chromium found (set CHROME); this check cannot run'); process.exit(1); }

const { CASES: cases } = await import(join(app, 'test/browser/cases.ts'));

const out = mkdtempSync(join(tmpdir(), 'ddiag-out-'));
const profile = mkdtempSync(join(tmpdir(), 'ddiag-profile-'));
await build({ configFile: false, root: join(app, 'test/browser'), base: './', logLevel: 'warn', build: { outDir: out, emptyOutDir: true, target: 'es2022', rollupOptions: { input: join(app, 'test/browser/diagram.html') } } });

const served = [];
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const server = http.createServer((req, res) => {
  const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
  served.push(path);
  const file = normalize(join(out, path));
  if (!file.startsWith(out) || !existsSync(file) || path.endsWith('/')) { res.writeHead(404).end(); return; }
  res.writeHead(200, { 'content-type': types[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const origin = `http://127.0.0.1:${server.address().port}`;
const page = `${origin}/diagram.html`;

const browser = spawn(chrome, ['--headless=new', '--no-first-run', '--no-default-browser-check', '--disable-gpu', '--disable-extensions', `--user-data-dir=${profile}`, '--remote-debugging-port=0', 'about:blank'], { stdio: 'ignore' });
let portFile = null;
for (let i = 0; i < 300 && !portFile; i++) {
  const f = join(profile, 'DevToolsActivePort');
  if (existsSync(f) && readFileSync(f, 'utf8').includes('\n')) portFile = readFileSync(f, 'utf8').split('\n');
  else await new Promise((r) => setTimeout(r, 100));
}
if (!portFile) { say('Chrome did not open a debugging port'); browser.kill(); process.exit(1); }

const ws = new WebSocket(`ws://127.0.0.1:${portFile[0]}${portFile[1]}`);
await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
let id = 0;
const waiting = new Map();
const requests = [];
const navigations = [];
const targets = [];
ws.onmessage = (m) => {
  const msg = JSON.parse(m.data);
  if (msg.id && waiting.has(msg.id)) { const [ok, fail] = waiting.get(msg.id); waiting.delete(msg.id); if (msg.error) fail(new Error(JSON.stringify(msg.error))); else ok(msg.result); return; }
  if (msg.method === 'Network.requestWillBeSent') requests.push(msg.params.request.url);
  if (msg.method === 'Page.frameNavigated') navigations.push(msg.params.frame.url);
  if (msg.method === 'Page.windowOpen') targets.push(`window.open ${msg.params.url}`);
  if (msg.method === 'Target.targetCreated' && msg.params.targetInfo.type === 'page') targets.push(msg.params.targetInfo.targetId);
};
const send = (method, params = {}, sessionId) => new Promise((ok, fail) => { const n = ++id; waiting.set(n, [ok, fail]); ws.send(JSON.stringify({ id: n, method, params, sessionId })); });

let result = null;
try {
  await send('Target.setDiscoverTargets', { discover: true });
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  targets.length = 0;
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  await send('Network.enable', {}, sessionId);
  await send('Page.enable', {}, sessionId);
  await send('Page.navigate', { url: page }, sessionId);
  const evaluate = async (expression) => (await send('Runtime.evaluate', { expression, returnByValue: true }, sessionId)).result.value;
  for (let i = 0; i < 1200 && !(await evaluate('window.__done === true')); i++) await new Promise((r) => setTimeout(r, 100));
  result = JSON.parse(await evaluate('JSON.stringify({ results: window.__results ?? null, pwn: window.__pwn ?? null, error: window.__error ?? null, href: location.href })'));
} finally {
  ws.close();
  const gone = new Promise((r) => (browser.exitCode !== null ? r() : browser.once('exit', r)));
  browser.kill();
  await gone;
  server.close();
}

if (!result || !result.results) problems.push(`the page did not finish: ${result?.error ?? 'no answer'}`);
else {
  if (result.pwn.length) problems.push(`script ran: ${result.pwn.join(', ')}`);
  if (result.href !== page) problems.push(`the page navigated to ${result.href}`);
  for (const n of navigations) if (n !== page && n !== 'about:blank') problems.push(`a frame navigated to ${n}`);
  for (const t of targets) problems.push(`a window was opened: ${t}`);
  for (const u of requests) if (!u.startsWith(`${origin}/`) && !u.startsWith('data:')) problems.push(`fetched from outside: ${u.slice(0, 120)}`);
  for (const p of served) if (p !== '/diagram.html' && p !== '/favicon.ico' && !p.startsWith('/assets/')) problems.push(`asked of the page's server: ${p}`);
  const byName = new Map(result.results.map((r) => [r.name, r]));
  for (const c of cases) {
    const r = byName.get(c.name);
    if (!r) { problems.push(`${c.name}: no result`); continue; }
    say(`${r.ok ? 'drawn' : 'text '}  ${c.name}${r.ok ? '' : ` (${r.why})`}`);
    if (c.expect === 'drawn' && !r.ok) problems.push(`${c.name}: not drawn: ${r.why}`);
    if (c.expect === 'text' && r.ok) problems.push(`${c.name}: drawn, but its text should have been shown`);
    if (!r.ok) continue;
    if (r.shown !== 'picture') problems.push(`${c.name}: the picture did not load`);
    // The drawing's elements and attributes, as the browser parsed them: words in its text are only text.
    const BANNED = ['script', 'a', 'img', 'iframe', 'image', 'use', 'object', 'embed', 'animate', 'set', 'animatemotion', 'animatetransform', 'audio', 'video', 'form', 'input', 'button', 'link', 'meta', 'base', 'details'];
    for (const e of r.elements) if (BANNED.includes(e)) problems.push(`${c.name}: the drawing holds a <${e}>`);
    for (const [name, value] of r.attributes) {
      const n = name.toLowerCase();
      if (n.startsWith('on')) problems.push(`${c.name}: the drawing holds an event attribute ${name}`);
      if (/(^|:)(href|src|action|formaction|data)$/.test(n) && !value.startsWith('#')) problems.push(`${c.name}: the drawing holds an address ${name}=${value.slice(0, 60)}`);
      if (/javascript:|url\(\s*(?!['"]?#)|image-set|@import/i.test(value)) problems.push(`${c.name}: an attribute of the drawing loads or runs something: ${name}=${value.slice(0, 60)}`);
    }
    // Its style sheets: nothing that loads.
    const sheets = [...r.svg.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)].map((m) => m[1]).join('\n');
    if (/url\(\s*(?!['"]?#)|image-set|@import|@font-face|\\/i.test(sheets)) problems.push(`${c.name}: a style sheet of the drawing loads something or holds an escape`);
    for (const w of c.words ?? []) if (!r.text.includes(w)) problems.push(`${c.name}: the drawing does not say ${w}`);
    const still = sheets.includes('animation:none!important');
    if (still !== !!c.still) problems.push(`${c.name}: ${c.still ? 'moves under reduce motion' : 'was held still without being asked'}`);
  }
}

rmSync(out, { recursive: true, force: true });
rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
if (problems.length) { console.error(problems.map((p) => `diagram-check: FAIL ${p}`).join('\n')); process.exit(1); }
say(`ok: ${cases.length} diagrams (${result.results.filter((r) => r.ok).length} drawn, the rest shown as text); nothing ran, navigated, opened a window or was fetched`);
