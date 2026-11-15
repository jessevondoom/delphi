// The page `dev/diagram-check.mjs` opens in a real browser: every case of `cases.ts` drawn by the bundled renderer, the
// way the window draws it. The page has no Content Security Policy (the app's would block inline script and outside
// loads on its own), so what is shown here is what the cleaning alone does.
import { drawDiagram } from '../../src/diagram.ts';
import { loadMermaid } from '../../src/mermaid-load.ts';
import { CASES } from './cases.ts';

type Result = { name: string; ok: boolean; why?: string; svg?: string; text?: string; elements?: string[]; attributes?: [string, string][]; shown?: string };
const w = window as unknown as { __pwn: string[]; pwn: (x?: unknown) => void; __results?: Result[]; __done?: boolean; __error?: string };
w.__pwn = [];
// Any name a hostile diagram calls lands here; the check fails if one is ever called.
w.pwn = (x?: unknown) => { w.__pwn.push(String(x ?? 'pwn')); };
const settle = () => new Promise((r) => setTimeout(r, 300));

async function main() {
  const m = await loadMermaid();
  const out: Result[] = [];
  for (const c of CASES) {
    const host = document.createElement('div');
    document.body.appendChild(host);
    const r = await drawDiagram(m, c.text, { dark: false, still: c.still ?? false, host });
    if (!r.ok) { out.push({ name: c.name, ok: false, why: r.why }); host.remove(); continue; }
    const svg = decodeURIComponent(r.src.slice(r.src.indexOf(',') + 1));
    // As the window shows it: a picture. A click on it does nothing.
    const img = document.createElement('img');
    img.src = r.src;
    host.appendChild(img);
    await new Promise((done) => { img.onload = done; img.onerror = done; });
    img.click();
    // And, harder than the window ever does, the cleaned drawing put live into the page and every element of it clicked:
    // if the cleaning had left a handler, a script or a link, this is where it would run or navigate.
    const live = document.createElement('div');
    live.innerHTML = svg;
    host.appendChild(live);
    for (const el of Array.from(live.querySelectorAll('*'))) {
      el.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      el.dispatchEvent(new FocusEvent('focus'));
    }
    await settle();
    // What the drawing is made of, read from an inert parse of it: every element and every attribute.
    const doc = new DOMParser().parseFromString(svg, 'image/svg+xml');
    const all = Array.from(doc.querySelectorAll('*'));
    const elements = [...new Set(all.map((e) => e.localName.toLowerCase()))];
    const attributes = all.flatMap((e) => Array.from(e.attributes).map((a) => [a.name, a.value] as [string, string]));
    out.push({ name: c.name, ok: true, svg, text: doc.documentElement.textContent ?? '', elements, attributes, shown: img.naturalWidth > 0 ? 'picture' : 'broken' });
    host.remove();
  }
  await settle();
  w.__results = out;
  w.__done = true;
}

main().catch((e: unknown) => { w.__error = e instanceof Error ? `${e.message}\n${e.stack}` : String(e); w.__done = true; });
