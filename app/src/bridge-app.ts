// The bridge inside the app (spec 111 §3.2): the Rust side's commands and the notifications it forwards.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Bridge, KbReply, Listener } from './bridge.ts';
import type { DaemonList } from './daemons.ts';
import type { Asked } from './requests.ts';
import type { Conn, Envelope, Info, OState } from './types.ts';

/** A command the bridge refused says why in a sentence; the window shows it. */
const said = (e: unknown): never => { throw new Error(typeof e === 'string' ? e : e instanceof Error ? e.message : String(e)); };

export function appBridge(): Bridge {
  return {
    kind: 'app',
    info: () => invoke<Info>('info'),
    state: () => invoke<OState>('state'),
    snapshot: async () => (await invoke<{ events?: Envelope[] }>('snapshot')).events ?? [],
    session: () => invoke<Record<string, unknown>>('session'),
    kb: (target: string) => invoke<KbReply>('kb', { target }),
    // The bridge names the daemon's method from `read` and builds its parameters from `arg` (`src-tauri/src/ency.rs`).
    ency: (read, arg) => invoke<unknown>('encyclopedia', { read, arg: arg ?? {} }).catch(said),
    // The bridge names the daemon's method from `what` and builds its parameters from `arg`'s known fields (`src-tauri/src/ask.rs`).
    request: (what, arg) => invoke<Asked>('request', { what, arg }).catch(said),
    marks: () => invoke<unknown>('marks_load').catch(said),
    saveMarks: (marks) => invoke<void>('marks_save', { marks }).catch(said),
    // The Rust side reads the page again and acts on the daemon's decision (spec 141 §2.4; `src-tauri/src/sink.rs`).
    exportPage: (name, text, page, confirmed) => invoke<string>('export_page', { name, text, slug: page.slug, version: page.version ?? null, confirmed }).catch(said),
    print: (page, confirmed) => invoke<void>('print_page', { slug: page.slug, version: page.version ?? null, confirmed }).catch(said),
    reveal: (path: string) => invoke<void>('reveal', { path }),
    open: (path: string) => invoke<void>('open_file', { path }),
    openUrl: (url: string) => invoke<void>('open_url', { url }),
    // The bridge reads the run folder and names the daemons; `attach` takes a hash from its last list and nothing else (`src-tauri/src/bridge.rs`).
    daemons: (summaries) => invoke<DaemonList>('daemons', { summaries }).catch(said),
    attach: (hash) => invoke<void>('attach', { hash }).catch(said),
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
