//! The C interface for the shim (shim/passwordlayout.cpp). The only `unsafe`
//! in this crate; every block is one operation with a `// SAFETY:` comment.
//!
//! Contract (also in include/password_layout_addon.h): a worker pointer
//! comes from [`password_layout_addon_start`], is not used after
//! [`password_layout_addon_stop`], and every function also accepts a null
//! pointer. No panics cross the boundary: `Worker::new` returns spawn errors
//! and handles poisoned locks; `want` does the same, and an `extern "C"`
//! panic would abort the process.

use std::io::{self, Write};

use crate::worker::Worker;

/// Create the worker. Returns NULL if the worker thread cannot start; the
/// reason is written to stderr.
#[unsafe(no_mangle)]
pub extern "C" fn password_layout_addon_start() -> *mut Worker {
    match Worker::new(crate::apply) {
        Ok(worker) => Box::into_raw(Box::new(worker)),
        Err(error) => {
            // Not eprintln!: this runs inside fcitx5.
            let _ = writeln!(io::stderr(), "passwordlayout: {error}");
            std::ptr::null_mut()
        }
    }
}

/// Record the latest wish. Null is allowed.
///
/// # Safety
///
/// `worker` is null or a pointer from [`password_layout_addon_start`] that
/// has not been passed to [`password_layout_addon_stop`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn password_layout_addon_want(worker: *mut Worker, latin: bool) {
    // SAFETY: the shim only passes the pointer from
    // password_layout_addon_start and never after password_layout_addon_stop;
    // a null pointer is handled by as_ref.
    if let Some(worker) = unsafe { worker.as_ref() } {
        worker.want(latin);
    }
}

/// Stop the worker (the thread is joined) and take back its memory. Null is
/// allowed.
///
/// # Safety
///
/// `worker` is null or a pointer from [`password_layout_addon_start`] that
/// has not been passed here before; it must not be used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn password_layout_addon_stop(worker: *mut Worker) {
    if worker.is_null() {
        return;
    }
    // SAFETY: the shim only passes the pointer from
    // password_layout_addon_start and never after a stop, so it is exactly
    // one valid, still-owned Worker allocation.
    drop(unsafe { Box::from_raw(worker) });
}
