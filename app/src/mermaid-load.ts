// Loads the bundled Mermaid renderer (spec 118 §5 item 4) the first time a diagram is drawn, never before. The file is
// `@mermaid-js/tiny` at the version pinned and hashed in the lock file, copied unchanged into the app's own files by the
// build (`?url`), so it is loaded from disk under the window's `script-src 'self'`; nothing comes from the network.
import tinyUrl from '@mermaid-js/tiny/dist/mermaid.tiny.js?url';
import { mermaidConfig, type Mermaid } from './diagram.ts';

let loading: Promise<Mermaid> | null = null;

export function loadMermaid(): Promise<Mermaid> {
  loading ??= new Promise<Mermaid>((resolve, reject) => {
    const g = globalThis as { mermaid?: Mermaid };
    if (g.mermaid) { resolve(g.mermaid); return; }
    const s = document.createElement('script');
    s.src = tinyUrl;
    s.async = true;
    s.onload = () => {
      const m = g.mermaid;
      if (!m) { reject(new Error('the diagram renderer did not start')); return; }
      // Before anything else: it never looks for diagrams in the page by itself, and it is strict from the start.
      m.initialize(mermaidConfig(true));
      resolve(m);
    };
    s.onerror = () => reject(new Error('the diagram renderer could not be loaded'));
    document.head.appendChild(s);
  }).catch((e: unknown) => { loading = null; throw e; });
  return loading;
}
