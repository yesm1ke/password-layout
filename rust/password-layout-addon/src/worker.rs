//! Applies "Latin wanted / not wanted" on a thread of its own. fcitx5 handles
//! every key press on one thread, so the addon must never wait for the
//! compositor there; it only records the latest wish here, and this thread
//! applies it. Wishes that change faster than they can be applied collapse
//! into the last one, so hopping across fields costs at most one switch.
//!
//! Kept apart from the addon, with `apply` given in, so the tests can run it
//! without fcitx5 or a compositor. Ported from src/fcitx/worker.h.

use std::io;
use std::panic;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

struct Wish {
    wanted: bool,
    stopping: bool,
}

/// Applies `apply(latin)` on its own thread.
///
/// `apply` is called with `false` first, to give back what a previous
/// instance may have left held (a fcitx5 that died with a password field
/// focused), and with `false` at the end.
pub struct Worker {
    shared: Arc<(Mutex<Wish>, Condvar)>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// `apply` runs on the worker thread only. A failed thread start is
    /// returned, not panicked.
    pub fn new(apply: impl FnMut(bool) + Send + 'static) -> io::Result<Worker> {
        let shared = Arc::new((
            Mutex::new(Wish {
                wanted: false,
                stopping: false,
            }),
            Condvar::new(),
        ));
        let thread_shared = Arc::clone(&shared);
        let thread = thread::Builder::new()
            .name("passwordlayout".into())
            .spawn(move || run(&thread_shared, apply))?;
        Ok(Worker {
            shared,
            thread: Some(thread),
        })
    }

    /// Record the latest wish. Never waits for an apply (fcitx5's thread
    /// must not); a wish that is already recorded returns without a notify.
    pub fn want(&self, latin: bool) {
        let (mutex, condvar) = &*self.shared;
        let mut wish = mutex.lock().unwrap_or_else(PoisonError::into_inner);
        if wish.wanted == latin {
            return;
        }
        wish.wanted = latin;
        drop(wish);
        condvar.notify_one();
    }
}

/// The worker thread. A panic in one apply (for example `eprintln!` inside
/// `State` with stderr closed) must not end the thread; fcitx5 would never
/// switch again.
fn run(shared: &Arc<(Mutex<Wish>, Condvar)>, mut apply: impl FnMut(bool)) {
    let mut call = |latin: bool| {
        let _ = panic::catch_unwind(panic::AssertUnwindSafe(|| apply(latin)));
    };
    call(false);
    let mut applied = false;
    let (mutex, condvar) = &**shared;
    let mut guard = mutex.lock().unwrap_or_else(PoisonError::into_inner);
    loop {
        guard = condvar
            .wait_while(guard, |wish| !wish.stopping && wish.wanted == applied)
            .unwrap_or_else(PoisonError::into_inner);
        if guard.wanted != applied {
            let latin = guard.wanted;
            drop(guard); // apply outside the lock
            call(latin);
            guard = mutex.lock().unwrap_or_else(PoisonError::into_inner);
            applied = latin;
        }
        if guard.stopping && guard.wanted == applied {
            return;
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let (mutex, condvar) = &*self.shared;
        {
            let mut wish = mutex.lock().unwrap_or_else(PoisonError::into_inner);
            wish.stopping = true;
            wish.wanted = false; // give the layout back when fcitx5 exits
        }
        condvar.notify_one();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
