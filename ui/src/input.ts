// Our own terminal input parser. Ink's `useInput` cannot see the mouse wheel, treats a return that arrives with
// text as part of a paste, and has no bracketed paste. Reading stdin ourselves fixes all three.

export type Key =
  | 'enter' | 'tab' | 'shift-tab' | 'backspace' | 'delete' | 'esc'
  | 'up' | 'down' | 'left' | 'right' | 'home' | 'end' | 'pgup' | 'pgdn';

/** A left-button press, drag or release at a 0-based cell. */
export type Mouse = { t: 'mouse'; k: 'down' | 'drag' | 'up'; x: number; y: number };

export type Ev =
  | Mouse
  | { t: 'text'; s: string }
  | { t: 'paste'; s: string }
  | { t: 'key'; k: Key; alt: boolean; shift: boolean; ctrl: boolean }
  | { t: 'ctrl'; c: string } // ctrl+letter, lower case
  | { t: 'wheel'; dir: 'up' | 'down' }
  | { t: 'bg'; dark: boolean; rgb: [number, number, number] }; // reply to the background-colour query; rgb is 0-255

const CSI_KEYS: Record<string, Key> = { A: 'up', B: 'down', C: 'right', D: 'left', H: 'home', F: 'end' };
const TILDE: Record<string, Key> = { '1': 'home', '3': 'delete', '4': 'end', '5': 'pgup', '6': 'pgdn', '7': 'home', '8': 'end' };

export class InputParser {
  private pasting = false;
  private paste = '';
  private rest = '';

  feed(chunk: string): Ev[] {
    const out: Ev[] = [];
    let s = this.rest + chunk;
    this.rest = '';
    let text = '';
    const flush = () => { if (text) { out.push({ t: 'text', s: text }); text = ''; } };
    let i = 0;
    while (i < s.length) {
      if (this.pasting) {
        const end = s.indexOf('\x1b[201~', i);
        if (end < 0) { this.paste += s.slice(i); i = s.length; break; }
        this.paste += s.slice(i, end);
        out.push({ t: 'paste', s: this.paste.replace(/\r\n?/g, '\n') });
        this.pasting = false; this.paste = ''; i = end + 6;
        continue;
      }
      const c = s[i]!;
      if (c === '\x1b') {
        const m = this.escape(s, i);
        if (m === 'incomplete') { this.rest = s.slice(i); break; }
        flush();
        if (m.ev === 'paste-start') this.pasting = true;
        else if (m.ev) out.push(m.ev);
        i += m.len;
        continue;
      }
      if (c === '\r' || c === '\n') {
        // A bare Enter. Pasted text arrives in a bracketed paste, so a return here is always a keypress, even
        // when it shares a read with the text before it.
        flush();
        out.push(c === '\n' ? { t: 'key', k: 'enter', alt: true, shift: false, ctrl: false } : { t: 'key', k: 'enter', alt: false, shift: false, ctrl: false });
        i++;
        continue;
      }
      if (c === '\t') { flush(); out.push({ t: 'key', k: 'tab', alt: false, shift: false, ctrl: false }); i++; continue; }
      if (c === '\x7f' || c === '\b') { flush(); out.push({ t: 'key', k: 'backspace', alt: false, shift: false, ctrl: false }); i++; continue; }
      const code = c.charCodeAt(0);
      if (code < 32) { flush(); out.push({ t: 'ctrl', c: String.fromCharCode(code + 96) }); i++; continue; }
      const cp = s.codePointAt(i)!;
      const ch = String.fromCodePoint(cp);
      text += ch; i += ch.length;
    }
    flush();
    return out;
  }

  private escape(s: string, i: number): 'incomplete' | { len: number; ev: Ev | 'paste-start' | null } {
    if (i + 1 >= s.length) return { len: 1, ev: { t: 'key', k: 'esc', alt: false, shift: false, ctrl: false } };
    const n = s[i + 1]!;
    if (n === '[') {
      let j = i + 2;
      while (j < s.length && !/[@-~]/.test(s[j]!)) j++;
      if (j >= s.length) return 'incomplete';
      const body = s.slice(i + 2, j), fin = s[j]!, len = j + 1 - i;
      if (fin === '~' && body === '200') return { len, ev: 'paste-start' };
      if (fin === 'M' || fin === 'm') {
        const m = /^<(\d+);(\d+);(\d+)$/.exec(body);
        if (m) {
          const b = Number(m[1]), x = Number(m[2]) - 1, y = Number(m[3]) - 1;
          if (fin === 'M' && b === 64) return { len, ev: { t: 'wheel', dir: 'up' } };
          if (fin === 'M' && b === 65) return { len, ev: { t: 'wheel', dir: 'down' } };
          // The left button, with or without shift, alt or ctrl held (bits 4, 8, 16); other buttons are not used.
          const base = b & ~28;
          if (fin === 'M' && base === 0) return { len, ev: { t: 'mouse', k: 'down', x, y } };
          if (fin === 'M' && base === 32) return { len, ev: { t: 'mouse', k: 'drag', x, y } };
          if (fin === 'm' && (base & 3) === 0 && base < 32) return { len, ev: { t: 'mouse', k: 'up', x, y } };
          return { len, ev: null };
        }
      }
      const mods = /;(\d+)$/.exec(body);
      const mod = mods ? Number(mods[1]) - 1 : 0;
      const flags = { shift: (mod & 1) !== 0, alt: (mod & 2) !== 0, ctrl: (mod & 4) !== 0 };
      if (fin === 'Z') return { len, ev: { t: 'key', k: 'shift-tab', alt: false, shift: true, ctrl: false } };
      // modifyOtherKeys (`CSI 27 ; mod ; code ~`) and the kitty protocol (`CSI code ; mod u`): Shift+Enter and friends.
      const mok = fin === '~' ? /^27;(\d+);(\d+)$/.exec(body) : fin === 'u' ? /^(\d+);(\d+)$/.exec(body) : null;
      if (mok) {
        const [code, m2] = fin === '~' ? [Number(mok[2]), Number(mok[1]) - 1] : [Number(mok[1]), Number(mok[2]) - 1];
        const f = { shift: (m2 & 1) !== 0, alt: (m2 & 2) !== 0, ctrl: (m2 & 4) !== 0 };
        if (code === 13) return { len, ev: { t: 'key', k: 'enter', ...f } };
        if (code === 9) return { len, ev: { t: 'key', k: f.shift ? 'shift-tab' : 'tab', ...f } };
        if (code === 27) return { len, ev: { t: 'key', k: 'esc', ...f } };
        if (code === 127) return { len, ev: { t: 'key', k: 'backspace', ...f } };
        if (f.ctrl && code >= 97 && code <= 122) return { len, ev: { t: 'ctrl', c: String.fromCharCode(code) } };
        if (code >= 32 && code < 127 && !f.ctrl) return { len, ev: { t: 'text', s: String.fromCharCode(code) } };
        return { len, ev: null };
      }
      if (fin === '~') { const k = TILDE[body.split(';')[0]!]; return { len, ev: k ? { t: 'key', k, ...flags } : null }; }
      const k = CSI_KEYS[fin];
      return { len, ev: k ? { t: 'key', k, ...flags } : null };
    }
    if (n === 'O') { // application cursor keys
      if (i + 2 >= s.length) return 'incomplete';
      const k = CSI_KEYS[s[i + 2]!];
      return { len: 3, ev: k ? { t: 'key', k, alt: false, shift: false, ctrl: false } : null };
    }
    if (n === ']') { // OSC reply, e.g. the background colour: ESC ] 11 ; rgb:RRRR/GGGG/BBBB BEL
      const bel = s.indexOf('\x07', i), st = s.indexOf('\x1b\\', i);
      const end = bel >= 0 && (st < 0 || bel < st) ? bel : st;
      if (end < 0) return 'incomplete';
      const len = end + (end === bel ? 1 : 2) - i;
      const m = /rgb:([0-9a-f]+)\/([0-9a-f]+)\/([0-9a-f]+)/i.exec(s.slice(i, end));
      if (!m) return { len, ev: null };
      const v = (h: string) => parseInt(h, 16) / (16 ** h.length - 1);
      const rgb = [m[1]!, m[2]!, m[3]!].map((h) => Math.round(v(h) * 255)) as [number, number, number];
      return { len, ev: { t: 'bg', dark: 0.2126 * v(m[1]!) + 0.7152 * v(m[2]!) + 0.0722 * v(m[3]!) < 0.5, rgb } };
    }
    // Alt+key: ESC then a character (Alt+Enter is ESC CR).
    if (n === '\r' || n === '\n') return { len: 2, ev: { t: 'key', k: 'enter', alt: true, shift: false, ctrl: false } };
    if (n === '\x7f') return { len: 2, ev: { t: 'key', k: 'backspace', alt: true, shift: false, ctrl: false } };
    // Escape followed at once by a control key (Ctrl+C) is two keypresses, not an Alt combination.
    if (n < ' ') return { len: 1, ev: { t: 'key', k: 'esc', alt: false, shift: false, ctrl: false } };
    return { len: 2, ev: { t: 'text', s: n } }; // Alt+letter: treated as the letter
  }
}
