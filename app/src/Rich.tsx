import { blocks, type Span } from './rich.ts';

function Spans({ spans }: { spans: Span[] }) {
  return <>{spans.map((s, i) => (s.kind === 'bold' ? <strong key={i}>{s.text}</strong> : s.kind === 'code' ? <code key={i}>{s.text}</code> : <span key={i}>{s.text}</span>))}</>;
}

/** An agent's reply, with the little Markdown the window draws. Every piece is a text node. */
export function Rich({ text }: { text: string }) {
  return (
    <div className="rich">
      {blocks(text).map((b, i) => {
        switch (b.kind) {
          case 'gap': return <div key={i} className="rich-gap" />;
          case 'code': return <pre key={i} className="code">{b.text}</pre>;
          case 'head': return <div key={i} className="rich-head"><Spans spans={b.spans} /></div>;
          case 'item': return <div key={i} className="rich-item" style={{ paddingLeft: b.depth * 18 }}><span className="rich-mark">{b.marker}</span><span><Spans spans={b.spans} /></span></div>;
          default: return <div key={i}><Spans spans={b.spans} /></div>;
        }
      })}
    </div>
  );
}
