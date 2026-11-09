//! Operator-authorized tests, slice S2d1 (`specs/092` TA-3): `run_authorized`, the forced-sandbox runner.
//!
//! **Reachable from one place only:** `Engine::granted_run_command` (`authorization_use.rs`), the daemon check that
//! `GOVERNANCE.md` §5.12 and `context/invariant-enforcement.md` map the approval to. A card is approved under an operator authorization
//! only there; `delphi run`'s `decide()` never approves a Red card (it cannot: it has no grant). The scan in `authorization/tests.rs`
//! fails on any other caller. Spec 092 S2d2b connected it, with the operator's ratification.
//!
//! What it does, given a command that matches an entry of the grant's record:
//!
//! 1. takes the **record's** working directory and environment and ignores whatever the call carried (the card has neither);
//! 2. re-checks everything the dry path checks (`Grant::verify_entry_at_use`: store pin, audit chain, launch hash, revocation, expiry,
//!    run/home/machine, the deny-list, the interpreter and every pinned file re-hashed) plus the fresh home (re-walked), the snapshot
//!    (re-hashed) and the ledger anchor;
//! 3. copies the pinned script into a private 0500 directory made at use time inside the grant's sandbox area, and re-hashes THAT copy;
//! 4. probes the sandbox (no fallback), **then** takes one use from the REAL ledger (fsynced before anything else; hash-chained; under
//!    the ledger's writer lock; the ledger must be exactly where this run last saw it), **then** logs `authorization.used`, **then**
//!    re-hashes the copy and the interpreter and probes again immediately before exec;
//! 5. runs the command ONLY through the S2b sandbox (`authorized_spec`: no network, no-exec scratch, `file-map-executable` denied, the
//!    one literal interpreter, the scrubbed environment, the snapshot as the working directory, the real limits and a real scratch quota
//!    with kill-over-quota, the whole session killed on timeout). Never `exec.run`; never unsandboxed; if there is no sandbox it refuses.
//!
//! **Residual windows.** (a) Between the last re-hash of the private copy and bash opening it there is a gap measured in milliseconds;
//! the copy is owned by the operator's user (0400 in a 0500 directory) and the sandboxed command can read it and not write it, so only
//! a process of the same user OUTSIDE the sandbox could swap it; that attacker can do far more already. Executing by descriptor does
//! not close it for a script (bash opens the path it is given). (b) The interpreter, `/bin/bash`, is on the sealed system volume and is
//! re-hashed here, but it is exec'd by path: a swap in that gap needs a write to the sealed volume. (c) A use taken and then lost to a
//! failed second probe, a failed log write or a refused exec stays consumed (fail-safe, never refunded). (e) The script is run from a
//! private copy in its own directory, so a script that finds its siblings by a path relative to `$0` finds none; the other pinned files
//! are re-hashed at their original paths and are readable only where the snapshot is. This is a limit of S2d1 and is not widened. (d) Arguments after the
//! script are the record's own fixed text; a script that names per-run paths (a snapshot or scratch directory chosen at launch) can
//! only be authorized for paths the operator knew in advance (an open question for S2d2).

use crate::authorization as a;
use crate::authorization_grant::{self as g, Candidate, Grant, MATCHED_TOOL};
use crate::authorization_tokens::{self as tok, Places};
use delphi_sandbox::authorized::{authorized_run_options, authorized_spec, FreshHome, Minted, Snapshot};
use delphi_sandbox::{run_bounded, Sandbox};
use serde_json::{json, Value};
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Logged to the mission log BEFORE the command runs (the use is already on disk).
pub const USED: &str = "authorization.used";
/// Logged after the command ran: how it ended.
pub const RAN: &str = "authorization.ran";

/// The reason `run_authorized` gives when no manifest entry equals the command: the caller logs it as `authorization.no_match`.
pub const NO_ENTRY: &str = "no manifest entry equals this command";

/// The places an authorized command runs in: made by the launcher (or a test), never by the model. `minted` is what the daemon made
/// AFTER the record (S2d2a): the writable no-exec scratch directory with its fixtures, the canary and the decoy home. The symbolic
/// tokens of the record resolve to these and to the snapshot, and to nothing the card or the record names.
pub struct Area<'a> {
    pub snapshot: &'a Snapshot,
    pub home: &'a FreshHome,
    pub minted: &'a Minted,
}

/// Why a command did not run. `use_taken` is true when the ledger had already been written (a failure after the decrement consumes
/// the use, on purpose).
#[derive(Debug)]
pub struct Refusal {
    pub reason: String,
    pub use_taken: bool,
}

fn no(reason: impl Into<String>) -> Refusal {
    Refusal { reason: reason.into(), use_taken: false }
}

fn after_use(reason: impl Into<String>) -> Refusal {
    Refusal { reason: reason.into(), use_taken: true }
}

/// What the command did. Output is capped by the sandbox runner (`RUN_MAX_OUTPUT_BYTES` each).
#[derive(Debug)]
pub struct AuthorizedRun {
    pub entry: usize,
    pub index: u32,
    pub uses_left: u32,
    pub tree_hash: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub timed_out: bool,
    pub output_capped: bool,
    pub over_quota: bool,
    pub stdout: String,
    pub stderr: String,
    /// The snapshot, re-hashed after the command, was exactly what it was before.
    pub snapshot_unchanged: bool,
}

/// A private directory made at use time, removed (and made writable first) when dropped.
struct PrivateDir(PathBuf);

impl Drop for PrivateDir {
    fn drop(&mut self) {
        let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        let _ = fs::remove_dir_all(&self.0);
    }
}

use crate::authorization_tokens::read_nofollow;

/// Check the places the command will run in: the throwaway home re-walked, the snapshot re-hashed, the scratch a real private directory of this user.
fn check_area(area: &Area) -> Result<(), String> {
    area.home.recheck().map_err(|e| format!("the throwaway home no longer passes its walk: {e}"))?;
    area.snapshot.verify().map_err(|e| format!("the snapshot: {e}"))?;
    let scratch: &Path = &area.minted.scratch;
    let m = fs::symlink_metadata(scratch).map_err(|e| format!("the scratch directory: {e}"))?;
    // SAFETY: geteuid has no preconditions.
    let me = unsafe { libc::geteuid() };
    if m.is_symlink() || !m.is_dir() || m.uid() != me {
        return Err("the scratch directory is a symlink, not a directory, or not this user's".into());
    }
    let canon = fs::canonicalize(scratch).map_err(|e| e.to_string())?;
    if canon != scratch {
        return Err(format!("the scratch directory is not canonical (it resolves to {})", canon.display()));
    }
    if canon.starts_with(area.snapshot.path()) {
        return Err("the scratch directory is inside the snapshot".into());
    }
    // The canary and the decoy home are the daemon's, beside the snapshot, never inside it (and never inside scratch).
    for (what, p) in [("canary", &area.minted.canary), ("decoy home", &area.minted.operator_home)] {
        let m = fs::symlink_metadata(p).map_err(|e| format!("the {what}: {e}"))?;
        if m.is_symlink() || m.uid() != me || p.starts_with(area.snapshot.path()) || p.starts_with(scratch) {
            return Err(format!("the {what} is a symlink, not this user's, or inside the snapshot or scratch"));
        }
    }
    Ok(())
}

impl Grant {
    /// Run the command `argv` under this grant, in the sandbox, once. See the module comment for the order and the guarantees.
    /// `emit` writes an event to the mission log; an error from it after the use was taken ends the call (the use stays consumed).
    pub fn run_authorized(&self, argv: &[String], area: &Area, now_unix: i64, emit: &mut dyn FnMut(&str, Value) -> Result<(), String>) -> Result<AuthorizedRun, Refusal> {
        // 1. The record's working directory and environment, never the call's: the match is made with the record's own constants,
        // and on the SYMBOLIC vector: the card must equal the record element by element, token text included. Only then are the
        // tokens replaced, by paths the daemon minted for this run (a card never supplies a value).
        let env = a::scrubbed_env();
        let Some(i) = self.match_entry(&Candidate { tool: MATCHED_TOOL, argv, cwd: a::CWD, env: &env }) else { return Err(no(NO_ENTRY)) };
        let e = &self.record.manifest[i];
        if e.cwd != a::CWD || e.env != env || e.scratch_mode != a::SCRATCH_MODE {
            return Err(no("the record's working directory, environment or scratch mode is not the fixed one"));
        }
        if self.record.token_scheme > tok::TOKEN_SCHEME {
            return Err(no("the record uses a token scheme this build does not read"));
        }
        if self.snapshot_dir.as_deref().is_some_and(|d| d != area.snapshot.path()) {
            return Err(no("the snapshot of this call is not the one the launcher gave the daemon"));
        }
        let script_symbolic = argv.get(1).cloned().ok_or_else(|| no("the command has no script"))?;
        let pinned = e.files.iter().find(|f| f.path == script_symbolic).ok_or_else(|| no("the script is not pinned"))?;
        let resolved = if self.record.token_scheme >= 1 {
            tok::resolve_argv(argv, &Places { snapshot: area.snapshot, minted: area.minted }).map_err(no)?
        } else {
            argv.to_vec()
        };
        let script = resolved[1].clone();

        // 2. Everything the dry path checks, then the places, the budget and the anchor.
        self.verify_entry_at_use(i, now_unix, Some(area.snapshot.path())).map_err(no)?;
        // Defence in depth, on purpose: `verify_entry_at_use` re-hashes the pinned files at their path; this checks the tree pin and the
        // pinned `@snapshot/<rel>` files against the snapshot object the run holds. Neither is removed because the other exists.
        a::check_snapshot_pins(&self.record, area.snapshot.path(), area.snapshot.tree_hash()).map_err(no)?;
        check_area(area).map_err(no)?;
        let expect = self.anchor.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let seen = g::ledger_state(&self.operator_home, &self.record.id).map_err(no)?;
        if seen != expect {
            return Err(no("the use ledger is not where this run last saw it (cut, deleted, replaced or written by another process); no use is taken"));
        }
        // Defence in depth, on purpose: `take_use_anchored` makes the same test under the ledger's lock. This one refuses before the script is
        // copied and the sandbox probed, so a used-up record costs nothing.
        if seen.count >= self.record.max_uses {
            return Err(no(format!("authorization {} has no uses left ({} of {} used)", self.record.id, seen.count, self.record.max_uses)));
        }

        // 3. The pinned script, copied into a private 0500 directory made now, and that copy hashed.
        let private = g_private_dir(area).map_err(no)?;
        let name = Path::new(&script).file_name().and_then(|n| n.to_str()).ok_or_else(|| no("the script has no file name"))?.to_string();
        let copy = private.0.join(&name);
        let bytes = read_nofollow(Path::new(&script)).map_err(no)?;
        if delphi_core::lock::hash(&bytes) != pinned.blake3 {
            return Err(no(format!("`{}` is not the file that was pinned (its hash changed)", a::clean_text(&script, 60))));
        }
        {
            let mut f = fs::OpenOptions::new().write(true).create_new(true).mode(0o400).open(&copy).map_err(|e| no(e.to_string()))?;
            f.write_all(&bytes).map_err(|e| no(e.to_string()))?;
            f.sync_all().map_err(|e| no(e.to_string()))?;
        }
        fs::set_permissions(&copy, fs::Permissions::from_mode(0o400)).map_err(|e| no(e.to_string()))?;
        fs::set_permissions(&private.0, fs::Permissions::from_mode(0o500)).map_err(|e| no(e.to_string()))?;
        let rehash_copy = || -> Result<(), String> {
            let b = read_nofollow(&copy)?;
            if delphi_core::lock::hash(&b) != pinned.blake3 {
                return Err("the private copy of the script no longer matches its pinned hash".into());
            }
            let m = fs::metadata(&copy).map_err(|e| e.to_string())?;
            if m.permissions().mode() & 0o7777 != 0o400 {
                return Err("the private copy of the script changed mode".into());
            }
            Ok(())
        };
        // Intentionally redundant with the two re-hashes after the use is taken (the copy is hashed again immediately before exec): this one
        // refuses before a use is spent and before the sandbox is probed.
        rehash_copy().map_err(no)?;

        // 4. Probe (no fallback), then the decrement, then the log.
        self.probe_now().map_err(no)?;
        let (used, after) = g::take_use_anchored(&self.operator_home, &self.record.id, &self.record_hash, &self.run, self.record.max_uses, &expect).map_err(no)?;
        *self.anchor.lock().unwrap_or_else(|p| p.into_inner()) = after;
        let uses_left = self.record.max_uses.saturating_sub(used.index);
        let mut run_argv = resolved.clone();
        run_argv[1] = copy.to_string_lossy().into_owned();
        emit(
            USED,
            json!({
                "authorization_id": self.record.id, "record_hash": self.record_hash, "entry": i, "index": used.index, "uses_left": uses_left,
                "ledger_seq": used.seq, "purpose": self.record.purpose, "tree_hash": area.snapshot.tree_hash(), "argv": argv, "resolved_argv": resolved, "token_scheme": self.record.token_scheme,
                "run_argv_script": copy.to_string_lossy(), "cwd_env": "the record's; the call's own are ignored", "sandbox": "forced", "approved": true,
            }),
        )
        .map_err(|e| after_use(format!("the use was taken but could not be logged ({e}); the command was not run")))?;

        // 5. Immediately before exec: everything that can be hashed, hashed again, and the sandbox probed again.
        rehash_copy().map_err(after_use)?;
        match a::pin_interpreter(&e.binary.path, &self.policy) {
            Ok(p) if p.blake3 == e.binary.blake3 => {}
            _ => return Err(after_use("the interpreter is not the file that was pinned (its hash changed)")),
        }
        check_area(area).map_err(after_use)?;
        self.probe_now().map_err(after_use)?;
        let sb = Sandbox::detect().map_err(|e| after_use(format!("no sandbox: {e}")))?;
        let mut spec = authorized_spec(area.snapshot.path(), &area.minted.scratch, &e.binary.path);
        spec.read_only.push(private.0.clone());
        #[cfg(test)]
        if let Some(w) = &self.weaken {
            w(&mut spec);
        }
        let cmd_args: Vec<std::ffi::OsString> = run_argv[1..].iter().map(std::ffi::OsString::from).collect();
        let mut cmd = sb.command(&spec, &e.binary.path, cmd_args).map_err(|e| after_use(format!("the authorized profile cannot be built: {e}")))?;
        cmd.current_dir(area.snapshot.path());
        let out = run_bounded(cmd, &authorized_run_options(&area.minted.scratch)).map_err(|e| after_use(format!("the command could not be started: {e}")))?;

        // 6. After: the snapshot re-hashed, and how it ended logged.
        let snapshot_unchanged = area.snapshot.verify().is_ok();
        let result = AuthorizedRun {
            entry: i,
            index: used.index,
            uses_left,
            tree_hash: area.snapshot.tree_hash().to_string(),
            exit_code: out.status.and_then(|s| s.code()),
            success: out.success(),
            timed_out: out.timed_out,
            output_capped: out.output_capped,
            over_quota: !out.over_quota.is_empty(),
            stdout: out.stdout,
            stderr: out.stderr,
            snapshot_unchanged,
        };
        let _ = emit(RAN, json!({"authorization_id": self.record.id, "entry": i, "index": used.index, "exit_code": result.exit_code, "success": result.success, "timed_out": result.timed_out, "output_capped": result.output_capped, "over_quota": result.over_quota, "snapshot_unchanged": snapshot_unchanged}));
        Ok(result)
    }
}

/// What the daemon holds for the authorized commands of one granted run: non-owning views of the snapshot and the throwaway home the
/// launcher made, and the places the daemon mints itself after the record (`@scratch`, `@canary`, `@operator_home`). One command runs at
/// a time (the scratch directory is shared), so a second call waits for the first, and a one-use record still runs once.
pub struct RunPlaces {
    snapshot: delphi_sandbox::authorized::Snapshot,
    home: delphi_sandbox::authorized::FreshHome,
    minted: delphi_sandbox::authorized::Minted,
    one_at_a_time: std::sync::Mutex<()>,
}

impl std::fmt::Debug for RunPlaces {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunPlaces(<held by the daemon>)")
    }
}

impl RunPlaces {
    /// Attach to the launcher's snapshot (its tree hash must be the one reported at launch) and to the daemon's own home, and mint the
    /// scratch directory, the canary and the decoy home beside the snapshot. Any failure refuses (the daemon then cannot run a command).
    pub fn attach(snapshot_dir: &Path, expect_tree: &str, home_dir: &Path) -> Result<RunPlaces, String> {
        let snapshot = Snapshot::attach(snapshot_dir, expect_tree)?;
        let home = delphi_sandbox::authorized::FreshHome::attach_live(home_dir)?;
        let minted = Minted::mint(&snapshot)?;
        Ok(RunPlaces { snapshot, home, minted, one_at_a_time: Default::default() })
    }

    pub fn area(&self) -> Area<'_> {
        Area { snapshot: &self.snapshot, home: &self.home, minted: &self.minted }
    }

    /// Run `argv` under `grant` in these places, once, waiting for any command already running.
    pub fn run(&self, grant: &Grant, argv: &[String], now_unix: i64, emit: &mut dyn FnMut(&str, Value) -> Result<(), String>) -> Result<AuthorizedRun, Refusal> {
        let _one = self.one_at_a_time.lock().unwrap_or_else(|p| p.into_inner());
        grant.run_authorized(argv, &self.area(), now_unix, emit)
    }
}

fn g_private_dir(area: &Area) -> Result<PrivateDir, String> {
    delphi_sandbox::authorized::mkdtemp(area.snapshot.root(), "use-").map(PrivateDir).map_err(|e| format!("the private directory: {e}"))
}

#[cfg(test)]
mod tests;
