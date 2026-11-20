import { useState } from 'react';
import { updateBannerWords, type Daemon } from './daemons.ts';
import { ended, refusalWords, requestStates, stateWords, updateRequest, type Asked, type Request } from './requests.ts';
import { isCode } from './PairCode.tsx';
import { CopyButton } from './screens/Start.tsx';
import type { Envelope } from './types.ts';

/**
 * The update banner (spec 120 §13.2 item 12), on the start screen and in the title row. It is made from what the attached
 * daemon wrote in its presence file: a release number and whether a restart is due. The copy button puts `delphi update`
 * on the clipboard and starts nothing. "Update now" appears only for the daemon this window is attached to, with a key that
 * holds `update`; it sends `app.update_apply`, which carries no parameter, and the daemon then waits for the operator at a
 * terminal (or, under `--auto`, goes on). After an update there is "restart when idle" and a command to copy: never a
 * button that restarts.
 */
export function UpdateBanner({ d, attached, grants, paired, send, events }: { d: Daemon; attached: boolean; grants?: string[]; paired: boolean; send?: (r: Request) => Promise<Asked>; events?: Envelope[] }) {
  const [said, setSaid] = useState('');
  const [code, setCode] = useState('');
  const w = updateBannerWords(d, attached, grants, paired);
  if (!w) return null;
  const mine = events ? [...requestStates(events).values()].filter((s) => s.method === 'app.update_apply').pop() : undefined;
  const live = !!mine && (mine.state === 'waiting' || mine.state === 'running');
  return (
    <div className="update-banner" role="status" data-restart={w.restart ? 'true' : undefined}>
      {w.text && <b>{w.text}</b>}
      {w.text && <CopyButton command={w.command} />}
      {w.canApply && send && !live && (
        <button type="button" className="btn small" onClick={() => { setSaid(''); setCode(''); send(updateRequest()).then((a) => { if (isCode(a.code)) setCode(a.code); }, (e: unknown) => setSaid(refusalWords(e))); }}>Update now</button>
      )}
      {live && mine && <span className="muted">{stateWords(mine)}</span>}
      {/* The code only this window shows: typed at the terminal, it is what approves the update. */}
      {code && mine?.state !== 'running' && !(mine && ended(mine.state)) && <span>Code <b className="mono" aria-label={`Code ${code.replace('-', ' dash ').split('').join(' ')}`}>{code}</b></span>}
      {said && <span role="alert">{said}</span>}
      {w.restart && <><b>{w.restart.text}</b><CopyButton command={w.restart.command} /></>}
    </div>
  );
}
