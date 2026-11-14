// The window's only way to the daemon (spec 111 §2.2): a handful of named reads and three kinds of notification. The window
// cannot name a daemon method; each function here is one of the bridge's commands. With no bridge present (a browser, a test)
// the same interface is served from a fixture, so every screen runs as a plain web page (§3.3).
import { BUILT_IN } from './avatar.ts';
import type { Conn, Envelope, Info, OState } from './types.ts';

export type Listener = { event(e: Envelope): void; conn(c: Conn): void; resync(): void };
export interface Bridge {
  /** `fixture` when no daemon is behind it. */
  readonly kind: 'app' | 'fixture';
  info(): Promise<Info>;
  state(): Promise<OState>;
  /** The newest persisted events of the kinds the daemon folds, oldest first. */
  snapshot(): Promise<Envelope[]>;
  /** One knowledge base read: `/v1/search`, `/v1/documents`, `/v1/doc`, `/v1/kinds` or `/v1/collections`, with its query. */
  kb(target: string): Promise<KbReply>;
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

export async function fixtureBridge(): Promise<Bridge> {
  const sim = await fixtureData();
  return {
    kind: 'fixture',
    info: async () => (sim.info?.roles && Object.keys(sim.info.roles).length ? sim.info : fixtureInfo()),
    state: async () => sim.state,
    snapshot: async () => sim.events,
    kb: fixtureKb,
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
