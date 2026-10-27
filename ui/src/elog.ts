// The console's half of the central error log (`specs/039`): the same file, the same one-line JSON records and the same
// redaction rules as the Rust side (`crates/delphi-core/src/elog.rs`), with no dependency. Logging is best-effort: any
// failure drops the record silently, and nothing here prints, throws or changes an exit code.
import { closeSync, mkdirSync, openSync, readFileSync, renameSync, rmSync, statSync, unlinkSync, writeSync, constants } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const FILE = 'delphi.log.jsonl';
export type Level = 'off' | 'error' | 'warn' | 'debug';
const RANK: Record<Level, number> = { off: 0, error: 1, warn: 2, debug: 3 };
const MAX_LINE = 16 * 1024, MAX_MESSAGE = 1500, MAX_CTX = 400, MAX_BACKTRACE = 4000, PRE_CUT = 16 * 1024, MIN_MAX_BYTES = 1024;
/** The only context keys a record keeps; the same list as the Rust side. */
export const CONTEXT_KEYS = ['mission', 'task', 'span', 'command', 'args_shape', 'exit_code', 'harness', 'provider', 'status', 'duration_ms', 'tool', 'method', 'kind',
  'model', 'role', 'attempt', 'stderr_tail', 'error_type', 'reason', 'from', 'to', 'retry_after_s', 'bytes', 'count', 'store', 'phase', 'view'];

export type Settings = { level: Level; maxBytes: number; files: number; dir: string; source: string };

export function parseLevel(s: string | undefined): Level | null {
  switch ((s ?? '').trim().toLowerCase()) {
    case 'off': case 'none': case '0': return 'off';
    case 'error': return 'error';
    case 'warn': case 'warning': return 'warn';
    case 'debug': case 'trace': return 'debug';
    default: return null;
  }
}

/** `[logging]` of `<home>/operator/config.toml`, then `DELPHI_LOG`: the operator's scope, never a file an agent can change. */
export function settingsFor(home: string, env: Record<string, string | undefined> = process.env): Settings {
  const s: Settings = { level: 'error', maxBytes: 10 * 1024 * 1024, files: 3, dir: join(home, 'logs'), source: 'default' };
  try {
    const text = readFileSync(join(home, 'operator', 'config.toml'), 'utf8');
    const m = /^\s*\[logging\]\s*$([\s\S]*?)(?=^\s*\[|(?![\s\S]))/m.exec(text);
    if (m) {
      const body = m[1]!;
      const lv = parseLevel(/^\s*level\s*=\s*["']([^"']+)["']/m.exec(body)?.[1]);
      if (lv) { s.level = lv; s.source = join(home, 'operator', 'config.toml'); }
      const mb = Number(/^\s*max_mb\s*=\s*(\d+)/m.exec(body)?.[1]);
      if (mb > 0) s.maxBytes = Math.max(MIN_MAX_BYTES, mb * 1024 * 1024);
      const files = Number(/^\s*files\s*=\s*(\d+)/m.exec(body)?.[1]);
      if (files > 0) s.files = Math.min(10, files);
    }
  } catch { /* no config */ }
  const lv = parseLevel(env.DELPHI_LOG);
  if (lv) { s.level = lv; s.source = 'DELPHI_LOG'; }
  const max = Number(env.DELPHI_LOG_MAX_BYTES);
  if (Number.isFinite(max) && max > 0) s.maxBytes = Math.max(MIN_MAX_BYTES, max);
  return s;
}

// ── sanitizing: the same pipeline as the Rust side ───────────────────────────────────────────────

const SECRET_PATTERNS: RegExp[] = [
  /sk-ant-[A-Za-z0-9_-]{20,}/g, /sk-[A-Za-z0-9]{32,}/g, /AIza[0-9A-Za-z_-]{35}/g, /gh[pousr]_[A-Za-z0-9]{20,}/g, /github_pat_[A-Za-z0-9_]{20,}/g,
  /A[KS]IA[0-9A-Z]{16}/g, /xox[baprs]-[A-Za-z0-9-]{10,}/g, /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g,
];
const KV_SECRET = /\b([A-Z][A-Z0-9_]*(?:API_KEY|SECRET_KEY|SECRET|TOKEN|PASSWORD|PASSWD))(\s*=\s*)([^\s'"]+)/g;
const MASK = '[REDACTED]';

const isBad = (c: number): boolean =>
  c < 0x20 || (c >= 0x7f && c <= 0x9f) || (c >= 0x200b && c <= 0x200f) || (c >= 0x202a && c <= 0x202e) || c === 0x2028 || c === 0x2029 ||
  (c >= 0x2060 && c <= 0x2069) || c === 0x061c || c === 0xfeff || (c >= 0xfff9 && c <= 0xfffb);

/** Control, bidi and zero-width characters out (a newline becomes the two characters `\n`); at most `max` characters. */
export function cleanChars(s: string, max: number): string {
  let out = '';
  let n = 0;
  for (const ch of s) {
    if (n++ >= max) break;
    const c = ch.codePointAt(0)!;
    if (ch === '\n') out += '\\n';
    else if (ch === '\r') out += '\\r';
    else if (ch === '\t') out += ' ';
    else if (isBad(c)) out += '?';
    else out += ch;
  }
  return out;
}

const truncate = (s: string, max: number): string => { const a = Array.from(s); return a.length <= max ? s : a.slice(0, max).join('') + '…'; };

/** Clean, redact, shorten. `home` becomes `$DELPHI_HOME`, the user's home `~`. */
export function sanitize(input: string, max: number, home?: string): string {
  let s = cleanChars(input, PRE_CUT);
  for (const p of SECRET_PATTERNS) s = s.replace(p, MASK);
  s = s.replace(KV_SECRET, `$1$2${MASK}`);
  s = s.replace(/([A-Za-z][A-Za-z0-9+.-]*:\/\/)[^/\s@]+@/g, `$1${MASK}@`);
  s = s.replace(/([?&](?:token|access_token|key|api_key|apikey|password|passwd|secret|sig|signature|code)=)[^&\s]+/gi, `$1${MASK}`);
  s = s.replace(/\b(bearer|basic|token)\s+[A-Za-z0-9._~+/=-]{8,}/gi, `$1 ${MASK}`);
  s = s.replace(/eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.?[A-Za-z0-9_-]*/g, MASK);
  s = s.replace(/did:[a-z0-9]+:[A-Za-z0-9._%:-]+/g, '[DID]');
  s = s.replace(/[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g, '[EMAIL]');
  s = s.replace(/\b[0-9a-fA-F]{32,}\b/g, '[HEX]');
  s = s.replace(/[A-Za-z0-9+_-]{40,}={0,2}/g, (m) => (/[0-9]/.test(m) && /[A-Za-z]/.test(m) ? '[BLOB]' : m));
  if (home && home.length > 1) s = s.split(home).join('$DELPHI_HOME');
  const user = process.env.HOME;
  if (user && user.length > 1) s = s.split(user).join('~');
  return truncate(s, max);
}

// ── writing ──────────────────────────────────────────────────────────────────────────────────────

export type Ctx = Record<string, string | number | boolean | null | undefined>;
type Logger = { settings: Settings; home: string; component: string; version: string; drops: number };
let logger: Logger | null = null;
const seen = new WeakSet<object>();
let lastCrash: { key: string; at: number } | null = null;

/** Start logging in this process. `home` is the Delphi home (`DELPHI_HOME`, else `~/.delphi`). */
export function initElog(opts: { home?: string; component?: string; version: string; env?: Record<string, string | undefined> }): void {
  const home = opts.home ?? process.env.DELPHI_HOME ?? join(homedir(), '.delphi');
  logger = { settings: settingsFor(home, opts.env ?? process.env), home, component: opts.component ?? 'ui', version: opts.version, drops: 0 };
}

export function stopElog(): void { logger = null; lastCrash = null; }
export const enabled = (level: Level): boolean => logger !== null && level !== 'off' && RANK[level] <= RANK[logger.settings.level];

/** One record, as a line without the newline. Pure apart from the clock and the pid. */
export function render(l: Logger, level: Exclude<Level, 'off'>, slug: string, message: string, ctx: Ctx = {}, backtrace?: string): string {
  const rec: Record<string, unknown> = {
    ts: new Date().toISOString(), level, component: l.component, pid: process.pid, version: l.version, os: `${process.platform === 'darwin' ? 'macos' : process.platform}-${process.arch === 'x64' ? 'x86_64' : process.arch === 'arm64' ? 'aarch64' : process.arch}`,
    slug: sanitize(slug, 80, l.home), message: sanitize(message, MAX_MESSAGE, l.home),
  };
  const c: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(ctx)) {
    if (!CONTEXT_KEYS.includes(k) || v === null || v === undefined) continue;
    c[k] = typeof v === 'string' ? sanitize(v, MAX_CTX, l.home) : v;
  }
  if (Object.keys(c).length) rec.context = c;
  if (l.settings.level === 'debug' && backtrace) rec.backtrace = sanitize(backtrace, MAX_BACKTRACE, l.home);
  if (l.drops > 0) rec.dropped = l.drops;
  let line = JSON.stringify(rec);
  if (line.length > MAX_LINE) { delete rec.backtrace; delete rec.context; rec.message = sanitize(message, 300, l.home); line = JSON.stringify(rec); }
  return line;
}

function rotate(path: string, s: Settings): void {
  let size = 0;
  try { size = statSync(path).size; } catch { return; }
  if (size < s.maxBytes) return;
  const lock = path + '.lock';
  let fd = -1;
  try { fd = openSync(lock, 'wx', 0o600); } catch {
    // A rotation that died holding the lock must not block every later one.
    try { if (Date.now() - statSync(lock).mtimeMs > 10_000) rmSync(lock, { force: true }); } catch { /* gone */ }
    return;
  }
  try {
    try { if (statSync(path).size < s.maxBytes) return; } catch { return; }
    const rotated = s.files - 1;
    if (rotated <= 0) { unlinkSync(path); return; }
    rmSync(`${path}.${rotated}`, { force: true });
    for (let n = rotated - 1; n >= 1; n--) { try { renameSync(`${path}.${n}`, `${path}.${n + 1}`); } catch { /* missing */ } }
    renameSync(path, `${path}.1`);
  } catch { /* best effort */ } finally {
    try { closeSync(fd); } catch { /* */ }
    rmSync(lock, { force: true });
  }
}

/** Write one record if the level allows. Never throws, never prints. */
export function log(level: Exclude<Level, 'off'>, slug: string, message: string, ctx: Ctx = {}, backtrace?: string): void {
  const l = logger;
  if (!l || !enabled(level)) return;
  try {
    mkdirSync(l.settings.dir, { recursive: true, mode: 0o700 });
    const path = join(l.settings.dir, FILE);
    const line = render(l, level, slug, message, ctx, backtrace) + '\n';
    rotate(path, l.settings);
    const fd = openSync(path, constants.O_WRONLY | constants.O_APPEND | constants.O_CREAT | constants.O_NOFOLLOW, 0o600);
    try { writeSync(fd, line); } finally { closeSync(fd); }
    l.drops = 0;
  } catch { l.drops += 1; }
}

export const error = (slug: string, message: string, ctx?: Ctx) => log('error', slug, message, ctx);
export const warn = (slug: string, message: string, ctx?: Ctx) => log('warn', slug, message, ctx);
export const debug = (slug: string, message: string, ctx?: Ctx) => log('debug', slug, message, ctx);

const describe = (e: unknown): { message: string; stack?: string } =>
  e instanceof Error ? { message: `${e.name}: ${e.message}`, stack: e.stack } : { message: typeof e === 'string' ? e : (() => { try { return JSON.stringify(e) ?? String(e); } catch { return String(e); } })() };

/** An error the console died of or survived: logged once per error object, however many handlers see it. */
export function logCrash(slug: 'uncaught_exception' | 'unhandled_rejection' | 'render_error' | 'console_stopped', e: unknown, ctx: Ctx = {}): void {
  try {
    if (typeof e === 'object' && e !== null) { if (seen.has(e)) return; seen.add(e); }
    const d = describe(e);
    // React renders a failing tree again before it gives up, with a fresh error object each time: the same text within two seconds is the same failure.
    const key = `${slug}\u0000${d.message}`;
    const now = Date.now();
    if (lastCrash && lastCrash.key === key && now - lastCrash.at < 2000) return;
    lastCrash = { key, at: now };
    log('error', slug, d.message, ctx, d.stack);
  } catch { /* never */ }
}

/** Log what Node is about to kill the process for, without changing what Node does: the monitor event does not replace the default handling. */
export function watchProcess(): void {
  process.on('uncaughtExceptionMonitor', (e, origin) => logCrash(origin === 'unhandledRejection' ? 'unhandled_rejection' : 'uncaught_exception', e));
}

/** The records on disk, oldest first (the tests and nothing else read them back here). */
export function readRecords(home: string): Record<string, any>[] {
  try {
    return readFileSync(join(home, 'logs', FILE), 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
  } catch { return []; }
}
