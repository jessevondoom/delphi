//! `delphi-core`: the per-workspace daemon. Normally started by `delphi` through
//! [`delphi_core::spawn::connect_or_spawn`], which passes the locked descriptor and a readiness
//! pipe; running it by hand is only for debugging.

use delphi_core::server::{Daemon, DaemonConfig};
use delphi_core::spawn::{ensure_run_dir, layout};
use delphi_engine::boot;
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
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("auth") {
        let workspace = PathBuf::from(arg(&args, "--workspace").unwrap_or_else(|| die("--workspace is required")));
        std::process::exit(delphi_engine::auth_cli::run(&workspace, &args[1..]));
    }
    if args.first().map(String::as_str) == Some("endpoints") {
        let workspace = PathBuf::from(arg(&args, "--workspace").unwrap_or_else(|| die("--workspace is required")));
        std::process::exit(delphi_engine::endpoints_cli::run(&workspace, &args[1..]));
    }
    if !args.iter().any(|a| a == "--daemon") {
        die("usage: delphi-core --daemon --workspace <workspace dir> --home <dir> --ready-fd N --lock-fd M");
    }
    let workspace = PathBuf::from(arg(&args, "--workspace").unwrap_or_else(|| die("--workspace is required")));
    let home = PathBuf::from(arg(&args, "--home").unwrap_or_else(|| die("--home is required")));
    let ready_fd: i32 = arg(&args, "--ready-fd").and_then(|s| s.parse().ok()).unwrap_or_else(|| die("--ready-fd is required"));
    // The inherited lock descriptor: held (never closed) for the daemon's lifetime. We do not re-lock by path.
    let lock_fd: i32 = arg(&args, "--lock-fd").and_then(|s| s.parse().ok()).unwrap_or_else(|| die("--lock-fd is required"));
    let _lock = unsafe { File::from_raw_fd(lock_fd) };
    let mut ready = unsafe { File::from_raw_fd(ready_fd) };

    let l = layout(&home, &workspace).unwrap_or_else(|e| die(&format!("workspace: {}", e)));
    ensure_run_dir(&l.run_dir).unwrap_or_else(|e| die(&format!("run dir: {}", e)));

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap_or_else(|e| die(&format!("runtime: {}", e)));
    rt.block_on(async move {
        let _ = std::fs::remove_file(&l.sock);
        // No window where the socket exists with looser permissions.
        let old = unsafe { libc::umask(0o177) };
        let listener = tokio::net::UnixListener::bind(&l.sock);
        unsafe { libc::umask(old) };
        let listener = listener.unwrap_or_else(|e| die(&format!("bind {}: {}", l.sock.display(), e)));
        std::fs::set_permissions(&l.sock, std::fs::Permissions::from_mode(0o600)).unwrap_or_else(|e| die(&format!("chmod socket: {}", e)));

        let canon = std::fs::canonicalize(&workspace).unwrap_or_else(|_| workspace.clone());
        let daemon = Daemon::new(DaemonConfig::new(home.clone(), canon.clone())).unwrap_or_else(|e| die(&format!("daemon: {}", e)));
        // The engine publishes through the daemon's hub, so attached clients see its events.
        let booted = boot::build(&home, &canon, daemon.hub.clone()).unwrap_or_else(|e| die(&e));
        daemon.set_extension(booted.service.extension());
        // Ready: the token travels over the pipe only.
        writeln!(ready, "{}", daemon.console_token()).unwrap_or_else(|e| die(&format!("ready pipe: {}", e)));
        drop(ready);
        eprintln!("delphi-core: ready on {} (mission {})", l.sock.display(), booted.default_mission);
        daemon.serve(listener).await;
        let _ = std::fs::remove_file(&l.sock);
    });
}
