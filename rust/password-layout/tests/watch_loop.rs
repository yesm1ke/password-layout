//! The watch-tty loop against a fake compositor, in its own process: the
//! test process's own pid plays the focused window, and prompts from other
//! test binaries must not count.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use password_layout::cli;
use password_layout_core::compositor::FakeCompositor;
use password_layout_core::state::State;

#[test]
fn a_focused_prompt_switches_to_latin_and_back() {
    // Layout 1 (the user's) is not Latin; layout 0 is.
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    compositor.focused = std::process::id() as i32;
    let compositor = Arc::new(Mutex::new(compositor));
    let stop = Arc::new(AtomicBool::new(false));
    let directory =
        std::env::temp_dir().join(format!("password-layout-watch-loop-{}", std::process::id()));
    let state = State::new(&directory);
    let state_check = State::new(&directory);

    // A prompt owned by the focused window (the test process) goes up.
    let child = common::PtyChild::new(&["bash", "-c", "read -s -p pw: x; sleep 30"]);

    let stop_watch = stop.clone();
    let compositor_watch = compositor.clone();
    let handle = std::thread::spawn(move || {
        cli::watch_tty(&mut *compositor_watch.lock().unwrap(), &state, &stop_watch)
    });

    // The prompt is seen: the holder `tty` is taken and the layout is Latin.
    let deadline = common::Deadline::new(5000);
    while !state_check
        .holders()
        .unwrap()
        .contains(&String::from("tty"))
    {
        assert!(!deadline.passed(), "the holder tty was not taken");
        std::thread::sleep(Duration::from_millis(20));
    }

    // The prompt ends: the holder is given back.
    drop(child);
    let deadline = common::Deadline::new(5000);
    while !state_check.holders().unwrap().is_empty() {
        assert!(!deadline.passed(), "the holder tty was not given back");
        std::thread::sleep(Duration::from_millis(20));
    }

    // Stop the loop; it may be in wait(-1), which a new terminal ends.
    stop.store(true, Ordering::Relaxed);
    let wake = common::PtyChild::new(&["cat"]);
    let result = handle.join().unwrap();
    drop(wake);
    assert!(result.is_ok());

    let compositor = compositor.lock().unwrap();
    assert_eq!(compositor.switches, vec![0, 1]);
}
