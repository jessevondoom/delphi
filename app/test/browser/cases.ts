// Diagrams for the browser check of the Mermaid renderer (spec 118 §5 item 4): plain ones that must be drawn, and hostile
// ones written the way a document could carry them. Every hostile one calls `pwn`, which records the call; the check
// passes only if no call is recorded, nothing navigates and nothing is fetched. `expect` is what the window must do with
// the text: draw it (`drawn`), or show the text because it fails (`text`), or either (`any`) as long as nothing runs.
export type Case = { name: string; text: string; expect: 'drawn' | 'text' | 'any'; still?: boolean; words?: string[] };

const OUTSIDE = 'http://127.0.0.1:9/'; // a port nothing listens on: a request to it is seen by the browser, not answered

export const CASES: Case[] = [
  { name: 'a flowchart', expect: 'drawn', words: ['Start', 'Done'], text: 'graph TD\n  A[Start] --> B{Is it?}\n  B -->|Yes| C[Done]\n  B -->|No| A' },
  { name: 'a sequence diagram', expect: 'drawn', words: ['Alice', 'Bob', 'Hello'], text: 'sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi' },
  { name: 'a class diagram', expect: 'drawn', words: ['Animal'], text: 'classDiagram\n  class Animal {\n    +String name\n    +eat()\n  }' },
  { name: 'a syntax error', expect: 'text', text: 'graph TD\n  A -->' },
  { name: 'click with a callback', expect: 'any', text: 'graph TD\n  A-->B\n  click A pwn "a tip"\n  click B call pwn("call")' },
  { name: 'click with a javascript: link', expect: 'any', text: 'graph TD\n  A-->B\n  click A "javascript:pwn(\'href\')"\n  click B href "javascript:pwn(\'href2\')" _self' },
  {
    name: 'HTML in labels', expect: 'any',
    text: 'graph TD\n  A["<img src=x onerror=pwn(\'img\')>"] --> B["<script>pwn(\'script\')</script>"]\n  C["<a href=\'javascript:pwn(1)\'>link</a>"] --> D["<iframe srcdoc=\'<script>parent.pwn(2)</script>\'></iframe>"]\n  E["<svg onload=pwn(\'svg\')>"] --> F["<details open ontoggle=pwn(\'toggle\')>"]',
  },
  {
    name: 'an init directive that lowers the security level', expect: 'any',
    text: '%%{init: {"securityLevel": "loose", "htmlLabels": true, "flowchart": {"htmlLabels": true}, "dompurifyConfig": {"ADD_TAGS": ["script", "iframe"], "ADD_ATTR": ["onerror", "onload"]}}}%%\ngraph TD\n  A["<img src=x onerror=pwn(\'init-img\')>"] --> B\n  click A pwn\n  click B "javascript:pwn(\'init-href\')"',
  },
  {
    name: 'front matter that lowers the security level', expect: 'any',
    text: '---\nconfig:\n  securityLevel: loose\n  htmlLabels: true\n  dompurifyConfig:\n    ADD_ATTR: [onerror]\n---\ngraph TD\n  A["<img src=x onerror=pwn(\'fm\')>"] --> B\n  click A pwn',
  },
  { name: 'a markdown label with HTML', expect: 'any', text: 'graph TD\n  A["`**bold** <img src=x onerror=pwn(\'md\')>`"] --> B' },
  { name: 'sequence links', expect: 'any', text: 'sequenceDiagram\n  participant A\n  link A: Evil @ javascript:pwn(\'link\')\n  links A: {"x": "javascript:pwn(\'links\')"}\n  A->>A: <img src=x onerror=pwn(\'seq\')>' },
  { name: 'a class link', expect: 'any', text: 'classDiagram\n  class A\n  link A "javascript:pwn(\'class\')"\n  callback A "pwn" "tip"' },
  { name: 'a style that loads from outside', expect: 'text', text: `graph TD\n  A-->B\n  style A fill:url(${OUTSIDE}fill)` },
  { name: 'a class style that loads from outside', expect: 'text', text: `graph TD\n  A:::c --> B\n  classDef c background:image-set("${OUTSIDE}set" 1x)` },
  { name: 'an escaped url in a style', expect: 'text', text: `graph TD\n  A-->B\n  style A fill:\\75rl(${OUTSIDE}escaped)` },
  { name: 'an escaped url in a theme variable', expect: 'text', text: `%%{init: {"themeVariables": {"primaryColor": "\\u0075rl(${OUTSIDE}theme)"}}}%%\ngraph TD\n  A-->B` },
  {
    name: 'a font loaded through a theme variable', expect: 'text',
    text: `%%{init: {"themeVariables": {"fontFamily": "evil; } @font-face { font-family: evil; src: url(${OUTSIDE}font); } svg { font-family: evil"}}}%%\ngraph TD\n  A[Words]-->B`,
  },
  { name: 'a theme style sheet from a directive', expect: 'drawn', text: '%%{init: {"themeCSS": ".node rect { fill: red; }"}}%%\ngraph TD\n  A-->B' },
  { name: 'an animated edge, under reduce motion', expect: 'drawn', still: true, text: 'graph TD\n  A e1@--> B\n  e1@{ animate: true }' },
];
