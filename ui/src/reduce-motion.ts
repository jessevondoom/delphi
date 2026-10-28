// The operator's "reduce motion" setting (an accessibility feature), as the console windows follow it. It lives in the
// workspace (`settings.get`) and changes by `/reduce-motion`, by `/settings set reduce_motion on`, or by asking; each of
// those ends in a `setting.changed` event, which is the one thing the windows listen for. `stillMotion` (motion.ts)
// is the single question every animation asks, with this as one of its inputs.
import { useEffect, useState } from 'react';
import type { Rpc } from './rpc.ts';

/** What a notification says about the setting: true or false when it is a `setting.changed` for `reduce_motion`, else null. */
export function reduceMotionFrom(n: any): boolean | null {
  if (n?.method !== 'event' || n.params?.type !== 'setting.changed') return null;
  const pl = n.params.payload ?? {};
  return pl.key === 'reduce_motion' ? pl.new === 'on' : null;
}

/** Whether the operator asked for reduced motion: read at start (and again after a reconnect), then kept by the events. */
export function useReduceMotion(cmd: Rpc, ev: Rpc): boolean {
  const [on, setOn] = useState(false);
  useEffect(() => {
    const read = () => { cmd.whenReady().then(() => cmd.call('settings.get', {})).then((v: any) => setOn(v?.reduce_motion === true)).catch(() => {}); };
    const onNote = (n: any) => { const v = reduceMotionFrom(n); if (v !== null) setOn(v); };
    read();
    ev.on('notification', onNote);
    ev.on('reconnected', read);
    return () => { ev.off('notification', onNote); ev.off('reconnected', read); };
  }, [cmd, ev]);
  return on;
}
