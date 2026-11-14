// The bridge inside the app (spec 111 §3.2): the Rust side's commands and the notifications it forwards.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Bridge, Listener } from './bridge.ts';
import type { Conn, Envelope, Info, OState } from './types.ts';

export function appBridge(): Bridge {
  return {
    kind: 'app',
    info: () => invoke<Info>('info'),
    state: () => invoke<OState>('state'),
    snapshot: async () => (await invoke<{ events?: Envelope[] }>('snapshot')).events ?? [],
    subscribe(l: Listener) {
      const offs = [
        listen<Envelope[]>('delphi://events', (m) => m.payload.forEach((e) => l.event(e))),
        listen<Conn>('delphi://conn', (m) => l.conn(m.payload)),
        listen('delphi://resync', () => l.resync()),
      ];
      // The bridge may have attached before the window listened: ask where it stands.
      void invoke<Conn>('conn').then((c) => l.conn(c)).catch(() => {});
      return () => offs.forEach((p) => void p.then((off) => off()));
    },
  };
}
