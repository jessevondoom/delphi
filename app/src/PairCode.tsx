// The code screen (spec 120 §13.1 item 2): an app with no key asks the daemon to be approved, shows the code it was given,
// and waits. It cannot approve, widen or skip: the operator types this code at a terminal, in `delphi app approve`. Plain
// text, nothing that moves: it needs no reduce-motion rule because there is no motion.
import type { Conn } from './types.ts';

/** A code as the daemon makes it: three and three characters of an alphabet with no 0, O, 1, I or L. Anything else is not shown. */
const CODE = /^[A-HJKMNP-Z2-9]{3}-[A-HJKMNP-Z2-9]{3}$/;

/** The code to show for this connection: only a window that is attached and holds no key, and only a well-formed code. */
export const pairCodeOf = (conn: Conn): string | null => (conn.connected && !conn.paired && CODE.test(conn.pair_code ?? '') ? (conn.pair_code as string) : null);

/** The words under the code. One place, so the window and its test say the same thing. */
export const PAIR_WAITING = 'Waiting for you to approve this at your terminal';

export function PairCode({ code }: { code: string }) {
  return (
    <section className="pair-code" role="status" aria-label="Pairing code">
      <div className="caps">This app has no key yet</div>
      <div className="pair-code-value mono" aria-label={`Code ${code.replace('-', ' dash ').split('').join(' ')}`}>{code}</div>
      <p>
        {PAIR_WAITING}: run <span className="mono">delphi app approve</span> and type this code. Until then this window only reads.
      </p>
    </section>
  );
}
