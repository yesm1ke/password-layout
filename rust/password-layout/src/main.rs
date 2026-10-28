// The `password-layout` binary: the rest of C++ `main` and its signal
// handling (src/main.cpp). The dispatch itself lives in `cli::run`.

use std::sync::atomic::{AtomicBool, Ordering};

static STOP: AtomicBool = AtomicBool::new(false);

// An atomic store is async-signal-safe; nothing else in the handler.
extern "C" fn request_stop(_: libc::c_int) {
    STOP.store(true, Ordering::Relaxed);
}

fn install_stop_handlers() {
    // SAFETY: all-zero is a valid sigaction: empty mask, no flags (no
    // SA_RESTART, so the signal ends the poll in wait(-1) with EINTR).
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = request_stop as *const () as libc::sighandler_t;
    for sig in [libc::SIGTERM, libc::SIGINT] {
        // Errors are ignored, as in C++.
        // SAFETY: valid action; the old action is not wanted.
        let _ = unsafe { libc::sigaction(sig, &action, std::ptr::null_mut()) };
    }
}

fn main() {
    // args_os, not env::args(), which panics on non-UTF-8 (a lossy holder is
    // simply invalid).
    let args: Vec<String> = std::env::args_os()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if args.get(1).map(String::as_str) == Some("watch-tty") {
        install_stop_handlers();
    }
    std::process::exit(password_layout::cli::run(&args, &STOP));
}
