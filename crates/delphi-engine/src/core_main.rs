//! The per-workspace daemon and its maintenance subcommands. Normally started by `delphi` itself
//! (`delphi __core --daemon …`, through [`delphi_core::spawn::connect_or_spawn`], which passes the locked
//! descriptor and a readiness pipe), so one executable is all an install needs.

use delphi_core::server::{Daemon, DaemonConfig};
use delphi_core::spawn::{ensure_run_dir, layout};
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::io::FromRawFd;
use std::path::PathBuf;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn die(msg: &str) -> ! {
    eprintln!("delphi-core: {}", msg);
    // The daemon's own startup failure, beside the per-daemon file the CLI shows (`specs/039`).
    delphi_core::elog::error("daemon", "startup_failed", msg, serde_json::json!({"phase": "startup"}));
    std::process::exit(1);
}

/// Run as the per-workspace daemon, or as one of its maintenance subcommands. `args` excludes the program name.
pub fn run(args: Vec<String>) -> ! {
    // Before any credential is read: no core dump, no memory read by another process of this user.
    delphi_sandbox::harden_self().unwrap_or_else(|e| die(&format!("process hardening: {}", e)));
    // The MCP relay a subscription CLI starts for the Delphi tool bridge (`bridge.rs`). It takes no workspace and reads no secret of ours.
    if args.first().map(String::as_str) == Some("__mcp-bridge") {
        std::process::exit(crate::bridge::run_relay(&args[1..]));
    }
    // The Antigravity CLI's `PreToolUse` gate hook (`bridge.rs`): it asks the span's server whether a tool call may run.
    if args.first().map(String::as_str) == Some("__agy-hook") {
        std::process::exit(crate::bridge::run_agy_hook());
    }
    if args.first().map(String::as_str) == Some("auth") {
        let workspace = PathBuf::from(arg(&args, "--workspace").unwrap_or_else(|| die("--workspace is required")));
        std::process::exit(crate::auth_cli::run(&workspace, &args[1..]));
    }
    if args.first().map(String::as_str) == Some("endpoints") {
        let workspace = PathBuf::from(arg(&args, "--workspace").unwrap_or_else(|| die("--workspace is required")));
        std::process::exit(crate::endpoints_cli::run(&workspace, &args[1..]));
    }
    if !args.iter().any(|a| a == "--daemon") {
        die("usage: delphi-core --daemon --workspace <workspace dir> --home <dir> --ready-fd N --lock-fd M");
    }
    let workspace = PathBuf::from(arg(&args, "--workspace").unwrap_or_else(|| die("--workspace is required")));
    let home = PathBuf::from(arg(&args, "--home").unwrap_or_else(|| die("--home is required")));
    delphi_core::elog::init(&home, "daemon", &std::env::var("DELPHI_RELEASE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()));
    delphi_core::elog::install_panic_hook("daemon");
    let ready_fd: i32 = arg(&args, "--ready-fd").and_then(|s| s.parse().ok()).unwrap_or_else(|| die("--ready-fd is required"));
    // The inherited lock descriptor: held (never closed) for the daemon's lifetime. We do not re-lock by path.
    let lock_fd: i32 = arg(&args, "--lock-fd").and_then(|s| s.parse().ok()).unwrap_or_else(|| die("--lock-fd is required"));
    let _lock = unsafe { File::from_raw_fd(lock_fd) };
    let mut ready = unsafe { File::from_raw_fd(ready_fd) };

    // Spec 092 S2c: the grant of an operator-authorized run arrives on an inherited pipe, is read once and the descriptor is closed.
    // No environment variable, no file, no socket path. A daemon started without `--grant-fd` holds no grant. A grant that does not
    // verify (not from this daemon's parent, an unreadable store, a revoked or expired record) stops the daemon before it is ready.
    // The parent is captured ONCE, here: the grant check and the watcher below use this one value. Reading it again later would let a
    // launcher that died in between leave the daemon with pid 1 as its parent, holding a grant and never exiting.
    // SAFETY: getppid has no preconditions.
    let boot_parent = unsafe { libc::getppid() } as u32;
    let boot_grant = arg(&args, "--grant-fd").map(|s| {
        let fd: i32 = s.parse().unwrap_or_else(|_| die("--grant-fd takes a descriptor number"));
        let bytes = crate::authorization_grant::take_from_fd(fd).unwrap_or_else(|e| die(&e));
        let now = delphi_core::time::parse_rfc3339(&delphi_core::time::now_rfc3339()).unwrap_or_else(|| die("the clock cannot be read, so a grant's expiry cannot be checked"));
        crate::authorization_grant::Grant::from_boot_system(&bytes, boot_parent, now).unwrap_or_else(|e| die(&format!("the operator authorization is refused: {e}")))
    });

    // A daemon that holds a grant lives and dies with the launcher that handed it over: if the launcher is gone (killed, crashed), the
    // grant goes with the daemon, and no later session can attach to a daemon that still holds it.
    if boot_grant.is_some() {
        std::thread::spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(250));
            // SAFETY: getppid has no preconditions.
            if crate::authorization_grant::launcher_gone(boot_parent, unsafe { libc::getppid() } as u32) {
                std::process::exit(0);
            }
        });
    }

    let l = layout(&home, &workspace).unwrap_or_else(|e| die(&format!("workspace: {}", e)));
    ensure_run_dir(&l.run_dir).unwrap_or_else(|e| die(&format!("run dir: {}", e)));

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap_or_else(|e| die(&format!("runtime: {}", e)));
    rt.block_on(async move {
        // Three sockets, each 0600 from the moment it exists: the console's, the read-only one the Narrator uses, and
        // the one a relay uses to carry approval cards to the operator.
        let bind_private = |path: &std::path::Path| {
            let _ = std::fs::remove_file(path);
            let old = unsafe { libc::umask(0o177) };
            let listener = tokio::net::UnixListener::bind(path);
            unsafe { libc::umask(old) };
            let listener = listener.unwrap_or_else(|e| die(&format!("bind {}: {}", path.display(), e)));
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap_or_else(|e| die(&format!("chmod socket: {}", e)));
            listener
        };
        let listener = bind_private(&l.sock);
        let narrator_listener = bind_private(&l.narrator_sock);
        let relay_listener = bind_private(&l.relay_sock);
        let host_listener = bind_private(&l.host_sock);

        let canon = std::fs::canonicalize(&workspace).unwrap_or_else(|_| workspace.clone());
        let mut cfg = DaemonConfig::new(home.clone(), canon.clone());
        if let Ok(v) = std::env::var("DELPHI_RELEASE_VERSION") {
            cfg.binary_version = v;
        }
        let daemon = Daemon::new(cfg).unwrap_or_else(|e| die(&format!("daemon: {}", e)));
        // The engine publishes through the daemon's hub, so attached clients see its events.
        let booted = crate::boot::build(&home, &canon, daemon.hub.clone()).unwrap_or_else(|e| die(&e));
        if let Some(g) = boot_grant {
            let _ = booted.engine.grant.set(std::sync::Arc::new(g));
        }
        daemon.set_extension(booted.service.extension());
        booted.service.start_timers();
        // Ready: the token travels over the pipe only.
        writeln!(ready, "{}", daemon.console_token()).unwrap_or_else(|e| die(&format!("ready pipe: {}", e)));
        drop(ready);
        eprintln!("delphi-core: ready on {} (mission {})", l.sock.display(), booted.default_mission);
        daemon.serve_everything(listener, Some(narrator_listener), Some(relay_listener), Some(host_listener)).await;
        let _ = std::fs::remove_file(&l.sock);
        let _ = std::fs::remove_file(&l.narrator_sock);
        let _ = std::fs::remove_file(&l.relay_sock);
        let _ = std::fs::remove_file(&l.host_sock);
    });
    std::process::exit(0);
}
