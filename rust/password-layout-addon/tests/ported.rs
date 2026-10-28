//! The C++ Worker tests, ported check by check from tests/test.cpp.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use password_layout_addon::worker::Worker;

/// What a Worker applied, in order, and how long each apply takes.
struct Applied {
    calls: Arc<Mutex<Vec<bool>>>,
    delay: Duration,
}

/// The recorder as the `apply` callback of a Worker.
impl Applied {
    fn new(delay: Duration) -> Applied {
        Applied {
            calls: Arc::new(Mutex::new(Vec::new())),
            delay,
        }
    }

    fn into_apply(self) -> impl FnMut(bool) + Send {
        move |latin: bool| {
            thread::sleep(self.delay);
            self.calls
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(latin);
        }
    }
}

fn taken(calls: &Arc<Mutex<Vec<bool>>>) -> Vec<bool> {
    calls.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Bounded wait: the test must never hang, 3 s is enough for these applies.
fn wait_for(calls: &Arc<Mutex<Vec<bool>>>, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while taken(calls).len() < count && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn worker_gives_back_first_and_last() {
    let applied = Applied::new(Duration::ZERO);
    let calls = applied.calls.clone();
    {
        let worker = Worker::new(applied.into_apply()).unwrap();
        worker.want(true);
        wait_for(&calls, 2);
        assert_eq!(taken(&calls), vec![false, true]);
    }
    // Stopping gives the layout back.
    assert_eq!(taken(&calls), vec![false, true, false]);
}

#[test]
fn worker_applies_only_the_latest_wish() {
    let applied = Applied::new(Duration::from_millis(150)); // a slow compositor
    let calls = applied.calls.clone();
    {
        let worker = Worker::new(applied.into_apply()).unwrap();
        // Focus hops across fields while the first apply is still running.
        let started = Instant::now();
        for latin in [true, false, true, false, true] {
            worker.want(latin);
        }
        // fcitx5's thread is never held up by the compositor.
        assert!(started.elapsed() < Duration::from_millis(50));
        wait_for(&calls, 2);
        thread::sleep(Duration::from_millis(200));
        assert_eq!(taken(&calls), vec![false, true]);
    }
    assert_eq!(taken(&calls), vec![false, true, false]);
}
