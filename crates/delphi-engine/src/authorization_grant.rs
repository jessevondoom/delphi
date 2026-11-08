//! Operator-authorized tests, slice S2c (`specs/092` TA-2, TA-3, TA-5): the daemon-side grant, canonical matching, the use ledger,
//! mid-run revocation and the per-use re-probe. **Wired but dry: nothing here approves anything.**
//!
//! * **The grant** ([`Grant`]) is built once, at daemon boot, from a payload the launcher writes into an INHERITED PIPE ([`take_from_fd`]):
//!   no environment variable, no file, no socket path. It lives in memory only, is never serialized and its `Debug` prints nothing
//!   of it. A process the daemon starts (an `exec.run` child, a harness seat) gets no descriptor and a cleared environment.
//! * **Matching** ([`Grant::evaluate`]) looks at one card kind only (`run_command`); the argument vector must equal a manifest entry
//!   element by element; the interpreter, every pinned file, the store, the audit chain, the revocation state and the sandbox probe
//!   are all checked again immediately before the answer is returned.
//! * **The use ledger** ([`take_use`]) is `operator/uses/<id>`: append-only, hash-chained, fsynced before anything else happens, one
//!   writer at a time under the log's own lock. The count is the number of `use` entries. It is the one new write the daemon makes
//!   under `operator/`. The exposure is the audit log's: a process running as the operator's user can edit the file (a rewrite is
//!   detected by the chain; a deleted ledger reads as unused, which `authorize_launch` cannot tell from a fresh record, so the
//!   ledger's presence is also written into the mission log by every `authorization.would_use`).
//! * **No approval.** The answer is [`Outcome::WouldUse`], which the engine logs as `authorization.would_use`; the card is still
//!   raised and the run still denies it. S2d turns an outcome into an approval, in a separate, separately reviewed slice.

use crate::authorization as a;
use delphi_core::event::EventEnvelope;
use delphi_sandbox::authorized::AuthorizedProbe;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read as _;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The one card kind a manifest entry can match (`run_command` with an exact argument vector).
pub const MATCHED_TOOL: &str = "run_command";
/// Observation events, written to the mission log. `would_use` means the ledger entry was written and the action WOULD have been
/// approved by an enabled build; in S2c the card is denied all the same.
pub const WOULD_USE: &str = "authorization.would_use";
pub const REFUSED_USE: &str = "authorization.refused";
pub const NO_MATCH: &str = "authorization.no_match";
/// The ledger entry kind.
pub const USE_ENTRY: &str = "authorization.use";
/// The largest boot payload (a pipe holds at least 16 KiB without a reader, even on macOS, so the launcher never blocks writing it).
pub const MAX_BOOT_BYTES: usize = 15 * 1024;

// ------------------------------------------------------------------------------------------------ the use ledger

fn id_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// `$DELPHI_HOME/operator/uses/<id>`.
pub fn ledger_dir(home: &Path, id: &str) -> PathBuf {
    home.join("operator/uses").join(id)
}

/// How many uses the ledger records, its chain verified (a broken, unchained or unreadable ledger is an error: a count taken from a
/// ledger that does not verify would be a guess). A record that was never used has no ledger and counts 0.
pub fn ledger_count(home: &Path, id: &str) -> Result<u32, String> {
    if !id_ok(id) {
        return Err("an authorization id of 1 to 64 letters, digits, `-`, `_` or `.` is required".into());
    }
    let dir = ledger_dir(home, id);
    if !dir.join("events").is_dir() {
        return Ok(0);
    }
    let rep = delphi_core::log::verify_chain(&dir).map_err(|e| format!("the use ledger of {id} cannot be read: {e}"))?;
    if let Some(b) = &rep.broken {
        return Err(format!("the use ledger of {id} fails verification: {b}"));
    }
    if rep.unchained > 0 {
        return Err(format!("the use ledger of {id} holds an event with no chain fields"));
    }
    let events = delphi_core::log::replay(&dir).map_err(|e| format!("the use ledger of {id} cannot be read: {e}"))?;
    Ok(events.iter().filter(|e| e.kind == USE_ENTRY).count() as u32)
}

/// One consumed use: its 1-based index in the ledger and the ledger event's sequence number.
#[derive(Debug, Clone, PartialEq)]
pub struct Used {
    pub index: u32,
    pub seq: u64,
}

fn open_ledger(home: &Path, id: &str) -> Result<delphi_core::log::EventLog, String> {
    // The same approach as `authorization::open_audit`: the lock is a flock on an open file description, so a lock this process has
    // just released can read as held for a few milliseconds while another thread's fork has not yet exec'd. A short bounded retry
    // rides that out (about 250 ms); a real second writer still gets the "busy" error and the use is refused.
    let dir = ledger_dir(home, id);
    let mut tries = 0;
    loop {
        match delphi_core::log::EventLog::open(&dir, Default::default()) {
            Err(delphi_core::log::LogError::Locked(_)) if tries < 25 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(delphi_core::log::LogError::Locked(_)) => return Err(format!("the use ledger of {id} is busy (another use is being taken)")),
            Err(e) => return Err(format!("could not open the use ledger of {id}: {e}")),
            Ok(l) => return Ok(l),
        }
    }
}

/// The atomic decrement. Under the ledger's writer lock: read the count, require `count < max_uses`, append the `use` entry (the log
/// fsyncs before it returns). Returns only once the entry is on disk, so a crash after this call leaves the use consumed. Two callers
/// on one record with `max_uses = 1`: exactly one gets `Ok`.
pub fn take_use(home: &Path, id: &str, record_hash: &str, run: &str, max_uses: u32) -> Result<Used, String> {
    if !id_ok(id) {
        return Err("an authorization id of 1 to 64 letters, digits, `-`, `_` or `.` is required".into());
    }
    let mut log = open_ledger(home, id)?;
    let count = ledger_count(home, id)?;
    if count >= max_uses {
        return Err(format!("authorization {id} has no uses left ({count} of {max_uses} used)"));
    }
    let index = count + 1;
    let seq = log.append(EventEnvelope::new("operator-uses", USE_ENTRY, json!({"id": id, "record_hash": record_hash, "run": run, "index": index}))).map_err(|e| format!("could not write the use ledger of {id}: {e}"))?;
    Ok(Used { index, seq })
}

// ------------------------------------------------------------------------------------------------ the boot payload

/// What the launcher tells the daemon, once, over the inherited pipe. Never written to a file, never logged.
#[derive(Serialize, Deserialize)]
struct Boot {
    record: a::Record,
    record_hash: String,
    run: String,
    operator_home: String,
    launch_seq: u64,
    /// The launcher's pid: the daemon accepts the grant only if its own parent is that process (a non-environment check that the
    /// pipe came from the process that spawned it).
    launcher_pid: u32,
}

/// The bytes the launcher writes into the pipe. Refuses a payload the pipe could not hold without a reader.
pub fn encode_boot(record: &a::Record, run: &str, operator_home: &Path, launch_seq: u64) -> Result<Vec<u8>, String> {
    let b = Boot { record_hash: a::record_hash(record), record: record.clone(), run: run.to_string(), operator_home: operator_home.to_string_lossy().into_owned(), launch_seq, launcher_pid: std::process::id() };
    let bytes = serde_json::to_vec(&b).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BOOT_BYTES {
        return Err(format!("the grant is {} bytes, over the {} the boot pipe takes", bytes.len(), MAX_BOOT_BYTES));
    }
    Ok(bytes)
}

/// The launcher's whole step: the strict launch check ([`a::authorize_launch_with_head`], the use ledger included) and the payload for
/// the daemon's pipe. Read-only. Any failure refuses the run.
pub fn launch_payload(home: &Path, id: &str, run: &str, now_unix: i64, policy: &a::Policy) -> Result<Vec<u8>, String> {
    let (record, seq) = a::authorize_launch_with_head(home, id, run, now_unix, policy)?;
    encode_boot(&record, run, home, seq)
}

/// Read the grant pipe to its end and close it. The descriptor is closed whatever happens, so no child can ever inherit it.
pub fn take_from_fd(fd: i32) -> Result<Vec<u8>, String> {
    // SAFETY: the launcher passed this descriptor on purpose and nothing else in this process owns it; the File closes it on drop.
    let mut f = unsafe { fs::File::from_raw_fd(fd) };
    let mut out = Vec::new();
    f.by_ref().take(MAX_BOOT_BYTES as u64 + 1).read_to_end(&mut out).map_err(|e| format!("the grant pipe cannot be read: {e}"))?;
    if out.len() > MAX_BOOT_BYTES {
        return Err("the grant pipe holds more than it may".into());
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------------ the grant

/// What the engine would run, in the form it is matched: the tool (card kind), the argument vector, the working directory and the
/// environment. S2d's `run_authorized` takes its cwd and environment from the matched record entry, so they equal it by construction;
/// matching checks them anyway.
#[derive(Debug, Clone)]
pub struct Candidate<'a> {
    pub tool: &'a str,
    pub argv: &'a [String],
    pub cwd: &'a str,
    pub env: &'a BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Not a card this grant can ever match (a kind other than `run_command`, or no manifest entry equals it). Nothing is checked or consumed.
    NotMatched(String),
    /// It matched an entry (or the grant's own state is wrong) and something failed: nothing was consumed.
    Refused { entry: Option<usize>, reason: String },
    /// The ledger entry was written. In S2c the card is still denied.
    WouldUse { entry: usize, index: u32, uses_left: u32, seq: u64 },
}

type ProbeFn = Arc<dyn Fn() -> AuthorizedProbe + Send + Sync>;

pub struct Grant {
    record: a::Record,
    record_hash: String,
    run: String,
    operator_home: PathBuf,
    launch_seq: u64,
    policy: a::Policy,
    probe: ProbeFn,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The record, its hash and the paths are not printed: a Debug of the engine must never leak the grant.
        f.write_str("Grant(<held by the daemon>)")
    }
}

impl Grant {
    /// From the boot payload. `parent_pid` is this daemon's parent; the payload names the launcher and they must agree. The store is
    /// then read strictly and checked as at a use ([`Grant::reverify`]), so a grant that is already wrong never takes hold.
    pub fn from_boot(bytes: &[u8], parent_pid: u32, now_unix: i64, policy: a::Policy) -> Result<Grant, String> {
        let b: Boot = serde_json::from_slice(bytes).map_err(|e| format!("the grant does not parse: {e}"))?;
        if b.launcher_pid != parent_pid {
            return Err("the grant names another launcher than this daemon's parent; refused".into());
        }
        if a::record_hash(&b.record) != b.record_hash {
            return Err("the grant's record does not match its hash".into());
        }
        let g = Grant { record: b.record, record_hash: b.record_hash, run: b.run, operator_home: PathBuf::from(b.operator_home), launch_seq: b.launch_seq, policy, probe: Arc::new(delphi_sandbox::authorized::probe_authorized) };
        g.reverify(now_unix)?;
        Ok(g)
    }

    /// [`Grant::from_boot`] with the system runner policy (`/bin/bash` on a pinned script): what the daemon uses.
    pub fn from_boot_system(bytes: &[u8], parent_pid: u32, now_unix: i64) -> Result<Grant, String> {
        Grant::from_boot(bytes, parent_pid, now_unix, a::Policy::system())
    }

    /// A grant built directly, for tests (the probe is replaceable with [`Grant::with_probe`]).
    #[cfg(test)]
    pub(crate) fn for_test(record: a::Record, run: &str, operator_home: &Path, launch_seq: u64, policy: a::Policy) -> Grant {
        let record_hash = a::record_hash(&record);
        Grant { record, record_hash, run: run.into(), operator_home: operator_home.to_path_buf(), launch_seq, policy, probe: Arc::new(delphi_sandbox::authorized::probe_authorized) }
    }

    #[cfg(test)]
    pub(crate) fn with_probe(mut self, p: impl Fn() -> AuthorizedProbe + Send + Sync + 'static) -> Grant {
        self.probe = Arc::new(p);
        self
    }

    pub fn id(&self) -> &str {
        &self.record.id
    }

    /// Read the store and the audit chain again, strictly, and require: the record present, its hash equal to the one at launch, not
    /// revoked, unexpired, bound to this run, home and machine, the same manifest hash, and the store at least as far on as at launch.
    /// An older store-and-pin pair fails the last test.
    pub fn reverify(&self, now_unix: i64) -> Result<(), String> {
        a::read_audit(&self.operator_home)?;
        let store = a::load(&self.operator_home)?;
        let r = store.records.iter().find(|r| r.id == self.record.id).ok_or_else(|| format!("authorization {} is no longer in the store", self.record.id))?;
        if r.revoked.is_some() {
            return Err(format!("authorization {} was revoked", r.id));
        }
        if a::record_hash(r) != self.record_hash {
            return Err(format!("authorization {} changed since the run started", r.id));
        }
        if r.manifest_hash != self.record.manifest_hash {
            return Err(format!("authorization {}: the manifest changed", r.id));
        }
        let head = store.audit_head.as_ref().map(|h| h.seq).unwrap_or(0);
        if head < self.launch_seq {
            return Err(format!("the authorization store is older than when this run started (audit head {head}, was {}): an older store and pin were put back", self.launch_seq));
        }
        a::check_launch(r, now_unix, &self.run, &self.operator_home, &a::this_machine(), &self.policy)
    }

    /// The entry a candidate equals, or `None`. Pure: nothing is read. Only `run_command`; the argument vector is compared element by
    /// element (never joined); `argv[0]` is the pinned interpreter path and `argv[1]` a pinned file; the working directory and the
    /// environment equal the record's.
    pub fn match_entry(&self, c: &Candidate) -> Option<usize> {
        if c.tool != MATCHED_TOOL {
            return None;
        }
        self.record.manifest.iter().position(|e| {
            e.argv.len() == c.argv.len()
                && e.argv.iter().zip(c.argv.iter()).all(|(x, y)| x == y)
                && c.argv.first().is_some_and(|b| *b == e.binary.path)
                && c.argv.get(1).is_some_and(|s| e.files.iter().any(|f| f.path == *s))
                && c.cwd == e.cwd
                && *c.env == e.env
        })
    }

    /// Everything checked again, in this order, immediately before an answer: match → store/audit/revocation → deny-list and runner
    /// rules → interpreter and every file re-resolved and re-hashed → sandbox re-probe (the first use included) → the ledger decrement.
    /// Any failure refuses with a named reason and consumes nothing. It never runs a command and never calls `exec.run`.
    pub fn evaluate(&self, c: &Candidate, now_unix: i64) -> Outcome {
        if c.tool != MATCHED_TOOL {
            return Outcome::NotMatched(format!("a `{}` card is never matched by an authorization", a::clean_text(c.tool, 40)));
        }
        let Some(i) = self.match_entry(c) else { return Outcome::NotMatched("no manifest entry equals this command".into()) };
        let refuse = |why: String| Outcome::Refused { entry: Some(i), reason: why };
        if let Err(why) = self.reverify(now_unix) {
            return refuse(why);
        }
        let e = &self.record.manifest[i];
        let entry = a::Entry { tool: e.tool.clone(), argv: e.argv.clone(), files: e.files.iter().map(|f| f.path.clone()).collect(), runner_provenance: e.runner_provenance.clone() };
        if let Some(why) = a::deny_reason(&entry).or_else(|| a::isolation_problem(&entry, &self.policy)) {
            return refuse(format!("the deny-list or the runner rules refuse it at use: {why}"));
        }
        match a::pin_interpreter(&e.binary.path, &self.policy) {
            Ok(p) if p.blake3 == e.binary.blake3 => {}
            Ok(_) => return refuse(format!("the interpreter `{}` is not the file that was pinned (its hash changed)", a::clean_text(&e.binary.path, 60))),
            Err(why) => return refuse(format!("the interpreter cannot be pinned again: {why}")),
        }
        for f in &e.files {
            match a::pin_file(&f.path, false) {
                Ok(p) if p.blake3 == f.blake3 => {}
                Ok(_) => return refuse(format!("`{}` is not the file that was pinned (its hash changed)", a::clean_text(&f.path, 60))),
                Err(why) => return refuse(format!("a pinned file cannot be pinned again: {why}")),
            }
        }
        let probe = (self.probe)();
        if !probe.is_ok() {
            return refuse(format!("the authorized sandbox failed its probe, so nothing runs: {}", probe.summary()));
        }
        match take_use(&self.operator_home, &self.record.id, &self.record_hash, &self.run, self.record.max_uses) {
            Ok(u) => Outcome::WouldUse { entry: i, index: u.index, uses_left: self.record.max_uses.saturating_sub(u.index), seq: u.seq },
            Err(why) => refuse(why),
        }
    }

    /// The card as the engine sees it, matched with the working directory and environment an authorized command would get.
    pub fn evaluate_card(&self, tool: &str, argv: &[String], now_unix: i64) -> Outcome {
        let env = a::scrubbed_env();
        self.evaluate(&Candidate { tool, argv, cwd: a::CWD, env: &env }, now_unix)
    }

    /// The operator's words for the event.
    pub fn purpose(&self) -> &str {
        &self.record.purpose
    }

    pub fn record_hash(&self) -> &str {
        &self.record_hash
    }
}

// ------------------------------------------------------------------------------------------------ tools in a granted run (TA-5)

/// Why a tool may not be offered or called in a run that holds a grant, or `None`. Composition (TA-5): a Red entry never runs beside a
/// tool that can write a payload for it to execute. `is_connector` says whether the name is an external (connector) tool.
pub fn tool_stripped(name: &str, is_connector: bool) -> Option<&'static str> {
    if is_connector {
        return Some("a connector tool may write outside the sandbox");
    }
    let bare = name.rsplit('.').next().unwrap_or(name);
    match bare {
        "write_file" | "edit_file" | "harness_edit" | "apply_patch" | "multi_edit" | "create_file" | "delete_file" | "move_file" => Some("it writes files"),
        n if n.starts_with("write_") || n.starts_with("edit_") || n.starts_with("propose_") => Some("it writes files or proposes a write"),
        _ => None,
    }
}

/// Whether a harness seat's own tools can be proven off for a granted run (per CLI), or why not. A seat that fails is not started.
/// `tooled` is a seat with a worktree and the CLI's standard tools (a Builder); `native_empty` is no built-in tool left in the launch
/// profile; `app_server` is the Copilot ACP mode used by a Builder.
///
/// * **claude**: `--tools ""` removes every built-in tool (asserted by the argument tests in `harness.rs`), the account's connectors
///   are left out (`--strict-mcp-config`), and its permission prompts go to Delphi. Provable only for a tool-less, non-Builder seat
///   with no native tool in the profile.
/// * **copilot**: `--available-tools delphi-no-tools` (a tool-less span) leaves it nothing; the ACP Builder mode has file and shell
///   tools judged through the approval bridge, so it is refused.
/// * **agy**: its own tools cannot be switched off, only denied by a `PreToolUse` hook that Delphi cannot prove complete. Refused.
/// * **codex**: only `features.shell_tool=false` and a read-only sandbox are set; its other built-in tools are not proven off. Refused.
pub fn seat_native_tools_off(cli: &str, tooled: bool, native_empty: bool, app_server: bool) -> Result<(), String> {
    match cli {
        "claude" if !tooled && native_empty => Ok(()),
        "claude" => Err("the claude seat would keep built-in tools (a Builder keeps its file and shell tools, or the profile lists native tools)".into()),
        "copilot" if !tooled && !app_server => Ok(()),
        "copilot" => Err("the copilot seat would run in ACP Builder mode, with file and shell tools".into()),
        "agy" => Err("agy's own tools cannot be switched off, only denied by a hook that is not proven complete".into()),
        "codex" => Err("codex's built-in tools are not proven off (only its shell is disabled and its sandbox set read-only)".into()),
        other => Err(format!("the `{other}` harness has no proof that its native tools are off")),
    }
}

#[cfg(test)]
mod tests;
