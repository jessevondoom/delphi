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
  subscribe(l: Listener): () => void;
}

/** Role ids as names, the built-in avatar table: what `delphi daemon where` sends, without a daemon. */
export function fixtureInfo(): Info {
  const roles: Info['roles'] = {};
  for (const [id, r] of Object.entries(BUILT_IN.roles)) roles[id] = { name: id, chassis: r.chassis, face: r.badge.idle?.cells ?? '•_•' };
  return { workspace_name: 'default', version: 'fixture', roles, avatar: BUILT_IN, projects: [{ name: 'default', root: '~/.delphi/workspaces/default', home: true }] };
}

export async function fixtureBridge(): Promise<Bridge> {
  const sim = (await import('./fixture/sim.json', { with: { type: 'json' } })).default as unknown as { state: OState; events: Envelope[] };
  return {
    kind: 'fixture',
    info: async () => fixtureInfo(),
    state: async () => sim.state,
    snapshot: async () => sim.events,
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
