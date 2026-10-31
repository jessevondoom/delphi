// The console's half of the central error log (spec 039): same file, same line format, same redaction as the Rust side.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PassThrough } from 'node:stream';
import React from 'react';
import { FILE, initElog, log, error, warn, debug, logCrash, readRecords, sanitize, settingsFor, stopElog, CONTEXT_KEYS } from '../src/elog.ts';
import { ErrorBoundary } from '../src/boundary.tsx';

const SECRET = 'sk-ant-api03-GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG';
const DID = 'did:key:z6MkhaXgBZDvotDkL5257faiztiGiC2QtKLGpbnnEGta2doK';
const fixture = fileURLToPath(new URL('./fixtures/crash-log.ts', import.meta.url));
const home = () => mkdtempSync(join(tmpdir(), 'delphi-elog-'));
const raw = (h: string) => readFileSync(join(h, 'logs', FILE), 'utf8');
const nasty = `key ${SECRET} did ${DID} url https://user:pw@example.com/x?token=abc123 mail a.b@corp.example \x1b[31mred\x1b[0m\nSECOND LINE \u202ertl Bearer abcdefghijklmnop1234 ${'A1b2'.repeat(20)}`;
const clean = (t: string) => {
  for (const bad of [SECRET, 'GGGGGGGG', DID, 'user:pw', 'token=abc123', 'a.b@corp.example', 'abcdefghijklmnop1234', 'A1b2A1b2A1b2A1b2', '\x1b', '\u202e']) assert.ok(!t.includes(bad), `leaked ${JSON.stringify(bad)} in ${t}`);
};

test('a record has the fields the Rust side writes, one line each, and nothing secret', () => {
  const h = home();
  initElog({ home: h, version: '9.9.9', env: {} });
  error('uncaught_exception', nasty, { mission: 'm1', view: 'console', body: 'the prompt text', prompt: 'p' } as any);
  const text = raw(h);
  clean(text);
  assert.equal(text.trimEnd().split('\n').length, 1);
  const r = JSON.parse(text);
  for (const k of ['ts', 'level', 'component', 'pid', 'version', 'os', 'slug', 'message']) assert.ok(r[k] !== undefined, k);
  assert.deepEqual([r.component, r.level, r.version, r.context.mission], ['ui', 'error', '9.9.9', 'm1']);
  assert.equal(r.context.body, undefined, 'only the known context keys survive');
  assert.ok(!text.includes('the prompt text'));
  assert.equal(statSync(join(h, 'logs', FILE)).mode & 0o777, 0o600);
  assert.equal(statSync(join(h, 'logs')).mode & 0o777, 0o700);
  assert.ok(CONTEXT_KEYS.includes('stderr_tail'));
  stopElog();
});

test('the home path is shortened and long fields are cut', () => {
  const h = home();
  initElog({ home: h, version: 't', env: {} });
  error('x', `failed at ${h}/run/x.sock ${'z'.repeat(5000)}`);
  const text = raw(h);
  assert.ok(!text.includes(h) && text.includes('$DELPHI_HOME') && text.length < 2500, text.length.toString());
  stopElog();
});

test('levels: error by default, warn and debug opt in, off writes nothing and creates nothing', () => {
  const h = home();
  initElog({ home: h, version: 't', env: {} });
  error('e', 'e'); warn('w', 'w'); debug('d', 'd');
  assert.equal(readRecords(h).length, 1);
  initElog({ home: h, version: 't', env: { DELPHI_LOG: 'debug' } });
  warn('w', 'w'); debug('d', 'd');
  assert.equal(readRecords(h).length, 3);
  const o = home();
  initElog({ home: o, version: 't', env: { DELPHI_LOG: 'off' } });
  error('e', 'e'); logCrash('uncaught_exception', new Error('x'));
  assert.ok(!existsSync(join(o, 'logs')));
  stopElog();
});

test('settings come from the operator config and DELPHI_LOG wins', () => {
  const h = home();
  assert.equal(settingsFor(h, {}).level, 'error');
  mkdirSync(join(h, 'operator'));
  writeFileSync(join(h, 'operator', 'config.toml'), '[other]\nlevel = "debug"\n[logging]\nlevel = "warn"\nmax_mb = 2\nfiles = 5\n');
  const s = settingsFor(h, {});
  assert.deepEqual([s.level, s.maxBytes, s.files], ['warn', 2 * 1024 * 1024, 5]);
  assert.equal(settingsFor(h, { DELPHI_LOG: 'debug', DELPHI_LOG_MAX_BYTES: '4096' }).level, 'debug');
  assert.equal(settingsFor(h, { DELPHI_LOG: 'off' }).level, 'off');
  assert.equal(settingsFor(h, { DELPHI_LOG: 'garbage' }).level, 'warn');
});

test('the size cap rotates and keeps the newest files only', () => {
  const h = home();
  initElog({ home: h, version: 't', env: { DELPHI_LOG_MAX_BYTES: '2048' } });
  for (let i = 0; i < 200; i++) error('fill', `record number ${i} ${'x'.repeat(100)}`);
  const names = readdirSync(join(h, 'logs')).filter((n) => !n.endsWith('.lock')).sort();
  assert.deepEqual(names, [FILE, `${FILE}.1`, `${FILE}.2`]);
  const total = names.reduce((n, f) => n + statSync(join(h, 'logs', f)).size, 0);
  assert.ok(total <= 3 * 2048 + 1024, `total ${total}`);
  assert.match(readRecords(h).at(-1)!.message, /record number 199/);
  stopElog();
});

test('an unwritable log location drops the record: nothing throws and nothing is printed', () => {
  const h = home();
  writeFileSync(join(h, 'logs'), 'a file where the directory should be');
  initElog({ home: h, version: 't', env: {} });
  const written: string[] = [];
  const out = process.stdout.write.bind(process.stdout), err = process.stderr.write.bind(process.stderr);
  (process.stdout as any).write = (s: any) => { written.push(String(s)); return true; };
  (process.stderr as any).write = (s: any) => { written.push(String(s)); return true; };
  try {
    assert.doesNotThrow(() => { error('e', 'e'); logCrash('render_error', new Error('x')); });
  } finally { (process.stdout as any).write = out; (process.stderr as any).write = err; }
  assert.deepEqual(written, [], 'logging is silent');
  stopElog();
  const h2 = home();
  mkdirSync(join(h2, 'logs')); chmodSync(join(h2, 'logs'), 0o500);
  initElog({ home: h2, version: 't', env: {} });
  assert.doesNotThrow(() => error('e', 'e'));
  chmodSync(join(h2, 'logs'), 0o700);
  stopElog();
});

test('a symlinked log file is refused, not followed', () => {
  const h = home();
  mkdirSync(join(h, 'logs'));
  writeFileSync(join(h, 'elsewhere'), 'keep');
  symlinkSync(join(h, 'elsewhere'), join(h, 'logs', FILE));
  initElog({ home: h, version: 't', env: {} });
  error('e', 'e');
  assert.equal(readFileSync(join(h, 'elsewhere'), 'utf8'), 'keep');
  stopElog();
});

test('sanitize: the canary string comes out clean, whatever the field', () => {
  clean(sanitize(nasty, 400));
  assert.equal(sanitize('a\u0000b\nc', 50), 'a?b\\nc');
});

test("the Term's fatal handlers log the error once, then give the terminal back and print it as before", () => {
  for (const [mode, slug] of [['term-throw', 'uncaught_exception'], ['term-reject', 'unhandled_rejection']] as const) {
    const h = home();
    const r = spawnSync(process.execPath, ['--import', 'tsx', fixture, mode], { env: { ...process.env, DELPHI_HOME: h, DELPHI_LOG: 'error' }, encoding: 'utf8', timeout: 20000 });
    assert.equal(r.status, 1, `${mode}: ${r.stderr}`);
    assert.match(r.stderr, /boom|rejected/, 'the error is still printed once the screen is given back');
    const recs = readRecords(h);
    assert.equal(recs.length, 1, `${mode}: ${existsSync(join(h, 'logs')) ? raw(h) : 'no log'}`);
    assert.equal(recs[0]!.slug, slug);
    assert.equal(recs[0]!.component, 'ui');
    clean(raw(h));
  }
});

test('without the Term, an uncaught error is logged and Node still dies as it always did', () => {
  const h = home();
  const r = spawnSync(process.execPath, ['--import', 'tsx', fixture, 'plain-throw'], { env: { ...process.env, DELPHI_HOME: h, DELPHI_LOG: 'error' }, encoding: 'utf8', timeout: 20000 });
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /Error: boom/, 'the default report is untouched');
  assert.equal(readRecords(h).length, 1);
  assert.equal(readRecords(h)[0]!.slug, 'uncaught_exception');
  clean(raw(h));
  const off = home();
  const r2 = spawnSync(process.execPath, ['--import', 'tsx', fixture, 'plain-throw'], { env: { ...process.env, DELPHI_HOME: off, DELPHI_LOG: 'off' }, encoding: 'utf8', timeout: 20000 });
  assert.equal(r2.status, r.status);
  assert.ok(!existsSync(join(off, 'logs')));
});

test('a React error boundary logs the render error once and rethrows', () => {
  const h = home();
  initElog({ home: h, version: 't', env: {} });
  const err = new Error(`render failed ${SECRET}`);
  assert.throws(() => {
    ErrorBoundary.getDerivedStateFromError(err);
    const boundary = new ErrorBoundary({});
    boundary.state = { error: err };
    boundary.render();
  }, /render failed/);
  const recs = readRecords(h);
  assert.equal(recs.length, 1);
  assert.equal(recs[0]!.slug, 'render_error');
  clean(raw(h));
  stopElog();
});

test('the TypeScript writer and the Rust reader agree on the format', () => {
  const h = home();
  initElog({ home: h, version: '1.2.3', env: {} });
  log('error', 'x', 'm', { mission: 'm1', exit_code: 2, duration_ms: 5 });
  const r = readRecords(h)[0]!;
  assert.match(r.ts, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/);
  assert.equal(typeof r.pid, 'number');
  assert.match(r.os, /^(macos|linux)-(aarch64|x86_64)$/);
  assert.deepEqual(r.context, { mission: 'm1', exit_code: 2, duration_ms: 5 });
  stopElog();
});
