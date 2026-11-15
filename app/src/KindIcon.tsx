// The line pictures of the design canvas (Work output): one per kind of thing, drawn in the text colour around them.
import type { Icon } from './output.ts';

const PATHS: Record<Icon | 'grid' | 'list', string> = {
  doc: 'M7 3h7l4 4v14H7zM14 3v4h4M10 12h5M10 16h5',
  deck: 'M3 5h18v11H3zM12 16v4M8 20h8',
  image: 'M4 5h16v14H4zM4 16l4-4 4 4 3-3 5 5M9 9h.01',
  video: 'M4 6h12v12H4zM16 10l4-2v8l-4-2',
  app: 'M3 5h18v14H3zM3 9h18M6 7h.01M9 7h.01',
  repo: 'M7 7v10M7 4a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zM7 17a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zM17 4a1.5 1.5 0 1 0 0 3 1.5 1.5 0 0 0 0-3zM17 7c0 5-10 3-10 8',
  research: 'M10 4a6 6 0 1 0 0 12 6 6 0 0 0 0-12zM14.5 14.5L20 20',
  council: 'M4 5h9v7H8l-4 3zM15 9h5v10l-3-3h-5v-2',
  grid: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z',
  list: 'M4 6h16M4 12h16M4 18h16',
};

/** Decoration only: the words beside it say what the thing is, so a screen reader is not told twice. */
export function KindIcon({ icon, size = 18 }: { icon: keyof typeof PATHS; size?: number }) {
  return (
    <svg className="kind-icon" aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={size > 24 ? 1.4 : 1.6} strokeLinecap="round" strokeLinejoin="round">
      <path d={PATHS[icon]} />
    </svg>
  );
}
