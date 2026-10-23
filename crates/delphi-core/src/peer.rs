//! Who is on the other end of the socket (`specs/005` §2.1 item 2).
//!
//! A same-UID check can't tell the operator from their own scripts, so tokens are only issued to a
//! peer that is attached to an interactive terminal. [`controlling_tty`] reads that from the OS
//! (`proc_pidinfo` on macOS, `/proc/<pid>/stat` on Linux) using the peer's pid.

/// The peer's controlling terminal device number, if it has one.
#[cfg(target_os = "macos")]
pub fn controlling_tty(pid: i32) -> Option<u64> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    if n != size {
        return None;
    }
    // NODEV is (dev_t)-1; a process without a controlling terminal reports it (or 0).
    match info.e_tdev {
        0 | u32::MAX => None,
        dev => Some(dev as u64),
    }
}

#[cfg(target_os = "linux")]
pub fn controlling_tty(pid: i32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    // Fields after the parenthesised command name: state ppid pgrp session tty_nr ...
    let after = stat.rsplit_once(')')?.1;
    let tty: u64 = after.split_whitespace().nth(4)?.parse().ok()?;
    if tty == 0 {
        None
    } else {
        Some(tty)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn controlling_tty(_pid: i32) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonexistent_pid_has_no_tty() {
        assert_eq!(controlling_tty(i32::MAX - 7), None);
    }
}
