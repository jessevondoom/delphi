// The window's only way to the daemon (spec 111 §2.2): a handful of named reads and three kinds of notification. The window
// cannot name a daemon method; each function here is one of the bridge's commands. With no bridge present (a browser, a test)
// the same interface is served from a fixture, so every screen runs as a plain web page (§3.3).
import { BUILT_IN } from './avatar.ts';
import type { EncyRead } from './ency.ts';
import type { Asked, AskArg, AskWhat } from './requests.ts';
import type { Conn, Envelope, Info, OState } from './types.ts';

export type Listener = { event(e: Envelope): void; conn(c: Conn): void; resync(): void };
export interface Bridge {
  /** `fixture` when no daemon is behind it. */
  readonly kind: 'app' | 'fixture';
  info(): Promise<Info>;
  state(): Promise<OState>;
  /** The newest persisted events of the kinds the daemon folds, oldest first. */
  snapshot(): Promise<Envelope[]>;
  /** `session.snapshot`: the daemon's mode, budget and which agents are on. */
  session(): Promise<Record<string, unknown>>;
  /** One knowledge base read: `/v1/search`, `/v1/documents`, `/v1/doc`, `/v1/image`, `/v1/kinds` or `/v1/collections`, with its query. */
  kb(target: string): Promise<KbReply>;
  /**
   * One of the encyclopedia's eleven reads (spec 117 §3): the read's name and its few arguments. The app sends it only
   * when it proved a paired key (spec 120); otherwise, and for anything the daemon refuses, the promise is rejected with
   * the sentence to show. `quote` beside a `library.source` id names the cited passage, for a source shown only as that.
   */
  ency(read: EncyRead, arg?: Record<string, unknown>): Promise<unknown>;
  /**
   * One of the four requests of the daemon's `request` grant (spec 120 §4.2): ids and what the operator typed. The app
   * sends it only when its key holds the grant; otherwise, and for anything the daemon refuses, the promise is rejected
   * with the sentence to show. It is the window's only call that is not a read.
   */
  request(what: AskWhat, arg: AskArg): Promise<Asked>;
  /** The operator's own marks (spec 117 §5), as kept in the app's folder; never sent to the daemon. */
  marks(): Promise<unknown>;
  saveMarks(marks: unknown): Promise<void>;
  /** Write a page out as a Markdown file by `name` and say where it went. */
  exportPage(name: string, text: string): Promise<string>;
  /** The system's print dialog for what the window shows. */
  print(): Promise<void>;
  /** Show a workspace document in the Finder. Only the app has it; a browser has no Finder to show. */
  reveal?(path: string): Promise<void>;
  /** Open a deck, a page, a picture or a recording of the workspace or a registered project in the system's browser or viewer. */
  open?(path: string): Promise<void>;
  /** Open an `http` or `https` address in the system's browser. */
  openUrl?(url: string): Promise<void>;
  subscribe(l: Listener): () => void;
}
/** What the daemon's retrieval service answered: its status and its body. */
export type KbReply = { status: number; body: unknown };

/** Role ids as names, the built-in avatar table: what `delphi daemon where` sends, without a daemon. */
export function fixtureInfo(): Info {
  const roles: Info['roles'] = {};
  for (const [id, r] of Object.entries(BUILT_IN.roles)) roles[id] = { name: id, chassis: r.chassis, face: r.badge.idle?.cells ?? '•_•' };
  return { workspace_name: 'default', version: 'fixture', roles, avatar: BUILT_IN, projects: [{ name: 'default', root: '~/.delphi/workspaces/default', home: true }] };
}

type Fixture = { info?: Info; state: OState; events: Envelope[] };

/** The fixture's knowledge base: the same five reads, answered from a small file in the daemon's shapes. */
async function fixtureKb(target: string): Promise<KbReply> {
  const kb = (await import('./fixture/kb.json', { with: { type: 'json' } })).default as unknown as { search: unknown; documents: unknown; kinds: unknown; collections: unknown; docs: Record<string, unknown> };
  const [path, query = ''] = target.split('?');
  if (path === '/v1/search') return { status: 200, body: kb.search };
  if (path === '/v1/documents') return { status: 200, body: kb.documents };
  if (path === '/v1/kinds') return { status: 200, body: kb.kinds };
  if (path === '/v1/collections') return { status: 200, body: kb.collections };
  if (path === '/v1/image') return { status: 404, body: { error: 'the fixture holds no pictures' } };
  if (path === '/v1/doc') { const doc = kb.docs[new URLSearchParams(query).get('path') ?? '']; return doc ? { status: 200, body: doc } : { status: 404, body: { error: 'no such document' } }; }
  return { status: 404, body: { error: 'not a read the fixture knows' } };
}

/** `?data=./file.json` (same origin only): a dump from `cargo run --example probe -- --json`, to look at real data in a browser. */
async function fixtureData(): Promise<Fixture> {
  const named = typeof location !== 'undefined' ? new URLSearchParams(location.search).get('data') : null;
  if (named && /^\.?\/[\w./-]+\.json$/.test(named)) {
    const got = await fetch(named);
    if (got.ok) return (await got.json()) as Fixture;
  }
  return (await import('./fixture/sim.json', { with: { type: 'json' } })).default as unknown as Fixture;
}

/** The fixture's encyclopedia: the eleven reads, answered from a small file in the daemon's shapes (`fixture/ency.json`). */
async function fixtureEncy(read: EncyRead, arg: Record<string, unknown> = {}): Promise<unknown> {
  const all = (await import('./fixture/ency.json', { with: { type: 'json' } })).default as unknown as Record<string, unknown>;
  const one = (key: unknown, what: string): unknown => {
    const found = (all[read] as Record<string, unknown> | undefined)?.[String(key)];
    if (found === undefined) throw new Error(`the daemon refused (-32000): no ${what} \`${String(key)}\``);
    return found;
  };
  switch (read) {
    case 'pages.list': case 'topics.list': case 'memory.roles': return all[read];
    case 'pages.get': return one(arg.version ? `${String(arg.slug)}@${String(arg.version)}` : arg.slug, 'topic');
    case 'pages.versions': case 'pages.links': return one(arg.slug, 'topic');
    case 'topics.get': return one(arg.id, 'research topic');
    case 'library.source': return one(String(arg.id).replace(/^s-/, ''), 'source');
    case 'library.claim': return one(arg.id, 'claim');
    case 'memory.role': return one(arg.role, 'role');
    case 'changes.since': {
      // What the fixture holds that is dated after `time`, as the daemon filters it.
      const c = all['changes.since'] as { pages: { built: number }[]; research: { updated: string }[]; findings: { timestamp: string }[]; notes: { created: string }[] };
      const since = Number(arg.time) || 0;
      const after = (iso: string) => Date.parse(iso) / 1000 > since;
      return { since, pages: c.pages.filter((p) => p.built > since), research: c.research.filter((r) => after(r.updated)), findings: c.findings.filter((f) => after(f.timestamp)), notes: c.notes.filter((n) => n.created >= new Date(since * 1000).toISOString().slice(0, 10)) };
    }
    default: throw new Error('that is not an encyclopedia read this app makes');
  }
}

const MARKS_KEY = 'delphi.marks.v1';
/** With no app there is no folder to keep marks in: a browser keeps them in its own storage, a test in memory. */
function fixtureMarks(): Pick<Bridge, 'marks' | 'saveMarks'> {
  let held: unknown = {};
  return {
    marks: async () => { try { const raw = globalThis.localStorage?.getItem(MARKS_KEY); if (raw) held = JSON.parse(raw); } catch { /* what is in memory */ } return held; },
    saveMarks: async (m) => { held = m; try { globalThis.localStorage?.setItem(MARKS_KEY, JSON.stringify(m)); } catch { /* nowhere to keep it */ } },
  };
}

/** A browser's way to hand over a file: a download of the text. */
async function download(name: string, text: string): Promise<string> {
  if (typeof document === 'undefined') throw new Error('there is nowhere to export to');
  const url = URL.createObjectURL(new Blob([text], { type: 'text/markdown' }));
  const a = document.createElement('a');
  a.href = url;
  a.download = `${name}.md`;
  a.click();
  URL.revokeObjectURL(url);
  return `${name}.md, in your downloads`;
}

export async function fixtureBridge(): Promise<Bridge> {
  const sim = await fixtureData();
  return {
    kind: 'fixture',
    info: async () => (sim.info?.roles && Object.keys(sim.info.roles).length ? sim.info : fixtureInfo()),
    state: async () => sim.state,
    snapshot: async () => sim.events,
    session: async () => ({ mission_id: sim.state.mission_id, mode: 'default', budget: { cap_usd: 25, spent_usd: 3.4, reserved_usd: 0.5, available_usd: 21.1 }, solo_mode: false, running_tasks: [{}], pending_approvals: [{}], interactive_agents: ['orchestrator'], forbidden_agents: [] }),
    kb: fixtureKb,
    ency: fixtureEncy,
    // Sample data has no daemon behind it, so there is nothing to ask; the window offers the clipboard commands.
    request: async () => { throw new Error('sample data: there is no daemon to ask'); },
    ...fixtureMarks(),
    exportPage: download,
    print: async () => { globalThis.print?.(); },
    // A browser has no Finder; an address opens in a tab of its own.
    openUrl: async (url: string) => { if (/^https?:\/\//i.test(url)) globalThis.open?.(url, '_blank', 'noopener'); },
    subscribe(l) {
      l.conn({ connected: true, detail: 'sample data, no daemon' });
      return () => {};
    },
  };
}

type TauriWindow = { __TAURI_INTERNALS__?: unknown };

export async function openBridge(): Promise<Bridge> {
  if (typeof window !== 'undefined' && (window as TauriWindow).__TAURI_INTERNALS__) {
    const { appBridge } = await import('./bridge-app.ts');
    return appBridge();
  }
  return fixtureBridge();
}
