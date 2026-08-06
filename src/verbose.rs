//! Opt-in diagnostics for the local side, enabled by `-v` / `--verbose`.
//!
//! Verbosity is process-global because the flag is: it is set once from the
//! parsed CLI and never varies per call. Threading a `bool` through `status`,
//! `sync`, `exec`, `run_command`, and `RemoteClient::connect` would touch every
//! signature on the path without any of them deciding anything with it.
//!
//! Diagnostics go to **stderr**. Stdout carries the remote command's own output
//! during `exec`, so anything printed there would corrupt a build log the user
//! piped somewhere.

use std::sync::atomic::{AtomicBool, Ordering};

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Print a diagnostic line to stderr when `--verbose` is on. The `[devsync]`
/// prefix keeps these distinguishable from the remote command's own stderr,
/// which is interleaved with them.
#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {
        if $crate::verbose::enabled() {
            eprintln!("[devsync] {}", format_args!($($arg)*));
        }
    };
}
