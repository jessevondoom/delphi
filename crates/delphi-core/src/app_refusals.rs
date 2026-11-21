//! What a refusal on the app socket costs, and how it is written down (`specs/120` §3 item 4, as amended by `specs/129` §10.1).
//!
//! There is no lockout. The daemon always sends the challenge to a connection within the limits on unproven connections, and a
//! refusal costs the caller: the refusal's answer waits, and the wait doubles with each failure, to [`DELAY_CAP`]. The failures are
//! counted three ways, because a process running as the operator can start new processes and so rotate its process id:
//! * per peer (the connection's process id where the platform gives it, its user id otherwise): accidents and noise;
//! * per key name claimed in the hello: a flood aimed at one key, from any number of processes;
//! * for the socket as a whole, past [`GLOBAL_FREE`] failures in the window.
//!
//! The wait falls on the refusal only. A valid signature is answered at once, whatever others have failed, so a flood can slow
//! the flooder and never the key it names. Both structures here take the time as an argument, so their tests need no clock.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The longest a refusal's answer waits.
pub const DELAY_CAP: Duration = Duration::from_secs(5);
/// The wait after a peer's (or a name's) first failure; it doubles with each further one.
pub const DELAY_BASE: Duration = Duration::from_millis(250);
/// Failures on the whole socket, within the window, that cost nothing beyond their own peer's and name's.
pub const GLOBAL_FREE: u32 = 20;
/// `app.refused` records written in full within one window; each one past these adds to a count written when the window ends.
pub const LOGGED_PER_WINDOW: usize = 20;
/// Peers and names remembered at once. Past this, records older than the window are dropped first, and a new one is not kept
/// (the socket-wide count still applies to it).
const MAX_TRACKED: usize = 1024;

/// Who knocked: the connection's process id, or, where the platform gives none, its user id.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PeerId {
    Pid(i32),
    Uid(u32),
}

impl PeerId {
    pub fn of(pid: Option<i32>, uid: u32) -> PeerId {
        pid.map(PeerId::Pid).unwrap_or(PeerId::Uid(uid))
    }
}

/// Failures in a row, each within the window of the one before.
#[derive(Clone, Copy, Debug)]
struct Streak {
    failures: u32,
    last: Instant,
}

impl Streak {
    /// How many failures came before this one (a streak whose last failure is older than the window starts over), then count it.
    fn bump(slot: &mut Option<Streak>, now: Instant, window: Duration) -> u32 {
        let before = match slot {
            Some(s) if now.saturating_duration_since(s.last) <= window => s.failures,
            _ => 0,
        };
        *slot = Some(Streak { failures: before.saturating_add(1), last: now });
        before
    }
}

/// The wait for a failure that has `before` failures ahead of it in its streak, of which `free` cost nothing.
fn wait(before: u32, free: u32, base: Duration, cap: Duration) -> Duration {
    if before < free {
        return Duration::ZERO;
    }
    let doublings = (before - free).min(16);
    base.saturating_mul(1u32 << doublings).min(cap)
}

/// The cost kept per peer, per claimed name and for the socket.
#[derive(Default)]
pub struct RefusalCost {
    peers: HashMap<PeerId, Streak>,
    names: HashMap<String, Streak>,
    global: Option<Streak>,
}

impl RefusalCost {
    /// Count one refusal and return how long its answer waits: the longest of the peer's, the name's and the socket's. `name` is
    /// counted only when it has a key name's shape; the caller checks that.
    pub fn charge(&mut self, peer: PeerId, name: Option<&str>, now: Instant, window: Duration, base: Duration, cap: Duration) -> Duration {
        let by_peer = Self::bump_in(&mut self.peers, peer, now, window);
        let by_name = name.map(|n| Self::bump_in(&mut self.names, n.to_string(), now, window)).unwrap_or(0);
        let by_socket = Streak::bump(&mut self.global, now, window);
        wait(by_peer, 1, base, cap).max(wait(by_name, 1, base, cap)).max(wait(by_socket, GLOBAL_FREE, base, cap))
    }

    fn bump_in<K: std::hash::Hash + Eq>(map: &mut HashMap<K, Streak>, key: K, now: Instant, window: Duration) -> u32 {
        if map.len() >= MAX_TRACKED && !map.contains_key(&key) {
            map.retain(|_, s| now.saturating_duration_since(s.last) <= window);
            if map.len() >= MAX_TRACKED {
                return 0;
            }
        }
        let mut slot = map.get(&key).copied();
        let before = Streak::bump(&mut slot, now, window);
        if let Some(s) = slot {
            map.insert(key, s);
        }
        before
    }
}

/// What to do with one `app.refused` record.
#[derive(Debug, PartialEq, Eq)]
pub enum LogNow {
    /// Write it in full.
    Write,
    /// Within its window it is past [`LOGGED_PER_WINDOW`]: it adds to the count.
    Counted,
}

/// The `app.refused` log's aggregation: the first [`LOGGED_PER_WINDOW`] in a window are written, each further one adds to a count,
/// and the count is written once, when its window ends.
#[derive(Default)]
pub struct RefusalLog {
    start: Option<Instant>,
    written: usize,
    counted: u64,
}

impl RefusalLog {
    /// Note one refusal. Returns what to do with it, and the count of a window that ended before it, to be written first.
    pub fn note(&mut self, now: Instant, window: Duration) -> (LogNow, Option<u64>) {
        let ended = self.flush(now, window);
        self.start.get_or_insert(now);
        if self.written < LOGGED_PER_WINDOW {
            self.written += 1;
            (LogNow::Write, ended)
        } else {
            self.counted += 1;
            (LogNow::Counted, ended)
        }
    }

    /// If the current window has ended, close it: the count of refusals that were not written (when there were any).
    pub fn flush(&mut self, now: Instant, window: Duration) -> Option<u64> {
        let start = self.start?;
        if now.saturating_duration_since(start) < window {
            return None;
        }
        let counted = self.counted;
        *self = RefusalLog::default();
        (counted > 0).then_some(counted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: Duration = Duration::from_secs(60);
    const B: Duration = DELAY_BASE;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn a_peers_first_failure_costs_nothing_and_each_further_one_doubles_to_the_cap() {
        let (mut c, t) = (RefusalCost::default(), Instant::now());
        let p = PeerId::Pid(41);
        let waits: Vec<Duration> = (0..8).map(|i| c.charge(p, None, t + ms(i), W, B, DELAY_CAP)).collect();
        assert_eq!(waits, [ms(0), ms(250), ms(500), ms(1000), ms(2000), ms(4000), ms(5000), ms(5000)]);
        // Another process of the same user starts with nothing against it.
        assert_eq!(c.charge(PeerId::Pid(42), None, t + ms(9), W, B, DELAY_CAP), ms(0));
        // A streak whose last failure is older than the window starts over.
        assert_eq!(c.charge(p, None, t + W + ms(10), W, B, DELAY_CAP), ms(0));
        assert_eq!(c.charge(p, None, t + W + ms(11), W, B, DELAY_CAP), ms(250));
    }

    #[test]
    fn rotating_the_process_id_does_not_escape_the_cost_kept_per_claimed_name() {
        let (mut c, t) = (RefusalCost::default(), Instant::now());
        let waits: Vec<Duration> = (0..5).map(|i| c.charge(PeerId::Pid(100 + i as i32), Some("app-0a1b2c"), t + ms(i), W, B, DELAY_CAP)).collect();
        assert_eq!(waits, [ms(0), ms(250), ms(500), ms(1000), ms(2000)], "each new process inherits the name's streak");
        // Another name, from yet another process, is not charged for that one.
        assert_eq!(c.charge(PeerId::Pid(200), Some("app-ffffff"), t + ms(6), W, B, DELAY_CAP), ms(0));
    }

    #[test]
    fn rotating_both_the_process_and_the_name_meets_the_cost_kept_for_the_socket() {
        let (mut c, t) = (RefusalCost::default(), Instant::now());
        let mut waits = Vec::new();
        for i in 0..(GLOBAL_FREE + 4) {
            let name = format!("app-{:06x}", i);
            waits.push(c.charge(PeerId::Pid(1000 + i as i32), Some(&name), t + ms(i as u64), W, B, DELAY_CAP));
        }
        assert!(waits[..GLOBAL_FREE as usize].iter().all(|w| w.is_zero()), "the first {GLOBAL_FREE} cost nothing: {waits:?}");
        assert_eq!(&waits[GLOBAL_FREE as usize..], [ms(250), ms(500), ms(1000), ms(2000)]);
    }

    #[test]
    fn where_the_platform_gives_no_process_id_the_user_id_is_the_peer() {
        assert_eq!(PeerId::of(Some(7), 501), PeerId::Pid(7));
        assert_eq!(PeerId::of(None, 501), PeerId::Uid(501));
    }

    #[test]
    fn the_peers_and_names_remembered_are_bounded() {
        let (mut c, t) = (RefusalCost::default(), Instant::now());
        for i in 0..(MAX_TRACKED as i32 + 50) {
            c.charge(PeerId::Pid(i), None, t, W, B, DELAY_CAP);
        }
        assert_eq!(c.peers.len(), MAX_TRACKED);
        // Once the old ones are past the window, they make room.
        c.charge(PeerId::Pid(-1), None, t + W + ms(1), W, B, DELAY_CAP);
        assert_eq!(c.peers.len(), 1);
    }

    #[test]
    fn the_log_writes_twenty_in_a_window_then_counts_and_writes_the_count_once_when_the_window_ends() {
        let (mut l, t) = (RefusalLog::default(), Instant::now());
        let got: Vec<LogNow> = (0..25).map(|i| l.note(t + ms(i), W).0).collect();
        assert!(got[..LOGGED_PER_WINDOW].iter().all(|g| *g == LogNow::Write));
        assert!(got[LOGGED_PER_WINDOW..].iter().all(|g| *g == LogNow::Counted));
        assert_eq!(l.flush(t + ms(30), W), None, "the window has not ended");
        assert_eq!(l.flush(t + W, W), Some(5));
        assert_eq!(l.flush(t + W + W, W), None, "written once");
        // A refusal after a quiet spell opens a new window, written in full; a window with nothing counted writes no count.
        assert_eq!(l.note(t + W * 3, W), (LogNow::Write, None));
        assert_eq!(l.flush(t + W * 5, W), None);
        // A refusal that arrives after a window ended with a count carries that count, to be written first.
        for i in 0..21 {
            l.note(t + W * 6 + ms(i), W);
        }
        assert_eq!(l.note(t + W * 8, W), (LogNow::Write, Some(1)));
    }
}
