//! The C interface end to end, against the fake Hyprland socket. ONE test
//! only: it sets environment variables for the whole process.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use password_layout_addon::ffi::{
    password_layout_addon_start, password_layout_addon_stop, password_layout_addon_want,
};
use password_layout_addon::worker::Worker;
use password_layout_core::fake_hyprland::{FakeHyprland, FakeReplies};
use password_layout_core::state::State;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A unique runtime directory and signature; the paths stay under Unix's
/// 108-byte socket limit.
fn runtime() -> (std::path::PathBuf, String) {
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    (
        std::env::temp_dir().join(format!("pl-addon-ffi-{pid}-{now}-{number}")),
        format!("s{number}"),
    )
}

#[test]
fn the_c_interface_switches_and_gives_back() {
    let (dir, signature) = runtime();

    // SAFETY: this is the only test in this binary; no other threads exist
    // yet, so setting process environment variables is safe.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", &dir);
        std::env::set_var("HYPRLAND_INSTANCE_SIGNATURE", &signature);
    }

    // Two devices, layouts "ru,us", both on index 0: ru is not Latin, us is.
    let replies = FakeReplies {
        devices: r#"{"keyboards": [
    {"name": "kb", "layout": "ru,us", "variant": ",", "active_layout_index": 0},
    {"name": "acpi", "layout": "us", "variant": "", "active_layout_index": 0}]}"#
            .to_string(),
        ..Default::default()
    };
    let hyprland = FakeHyprland::start(&dir, &signature, replies).unwrap();

    let worker: *mut Worker = password_layout_addon_start();
    assert!(!worker.is_null());
    // SAFETY: worker comes from start and is not stopped yet (our pointer).
    unsafe { password_layout_addon_want(worker, true) };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !hyprland
        .requests()
        .contains(&"/switchxkblayout all 1".to_string())
    {
        assert!(
            std::time::Instant::now() < deadline,
            "no switch to the Latin layout in 3 s"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    // SAFETY: worker is ours, stopped exactly once here.
    unsafe { password_layout_addon_stop(worker) };

    let requests = hyprland.requests();
    assert_eq!(*requests.last().unwrap(), "/switchxkblayout all 0");
    let state = State::new(dir.join("password-layout"));
    assert!(state.holders().unwrap().is_empty());

    // Null pointers are allowed everywhere.
    // SAFETY: null is a valid argument to both functions.
    unsafe {
        password_layout_addon_want(std::ptr::null_mut(), true);
        password_layout_addon_stop(std::ptr::null_mut());
    }

    // Stop the fake before removing its socket: its Drop connects to the
    // socket to wake the accept loop.
    drop(hyprland);
    let _ = std::fs::remove_dir_all(&dir);
}
