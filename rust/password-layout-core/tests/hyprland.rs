//! The Hyprland socket client against [`FakeHyprland`] and a couple of
//! hand-rolled servers, from the stage 3 design. Tests run in parallel, so
//! every test gets its own runtime directory and signature; the paths stay
//! under Unix's 108-byte socket limit.

use std::io::{Read, Write};
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use password_layout_core::compositor::Compositor;
use password_layout_core::fake_hyprland::{FakeHyprland, FakeReplies};
use password_layout_core::hyprland::{Hyprland, HyprlandEvents, LayoutEvents, socket_path};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn runtime() -> (std::path::PathBuf, String) {
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let number = COUNTER.fetch_add(1, Ordering::Relaxed);
    (
        std::env::temp_dir().join(format!("pl-hyprland-{pid}-{now}-{number}")),
        format!("s{number}"),
    )
}

// ru,us at index 0: the real keyboard plus a stray device.
const DEVICES: &str = r#"{"keyboards": [
    {"name": "kb", "layout": "ru,us", "variant": ",", "active_layout_index": 0},
    {"name": "acpi", "layout": "us", "variant": "", "active_layout_index": 0}]})"#;

fn fake(devices: String, active_window: String) -> (std::path::PathBuf, String, FakeHyprland) {
    let (directory, signature) = runtime();
    let server = FakeHyprland::start(
        &directory,
        &signature,
        FakeReplies {
            devices,
            active_window,
        },
    )
    .unwrap();
    (directory, signature, server)
}

#[test]
fn layout_index_is_the_majority() {
    let (dir, sig, server) = fake(DEVICES.into(), "{}".into());
    let mut compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.layout_index().unwrap(), 0);
    assert_eq!(server.requests(), vec!["j/devices".to_string()]);
}

#[test]
fn latin_layouts_are_measured_from_devices() {
    let (dir, sig, _server) = fake(DEVICES.into(), "{}".into());
    let mut compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.latin_layouts().unwrap(), vec![false, true]);
}

#[test]
fn focused_pid_comes_from_the_active_window() {
    let (dir, sig, _server) = fake(
        DEVICES.into(),
        r#"{"title": "x", "pid": 3309, "xwayland": false}"#.into(),
    );
    let mut compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.focused_pid().unwrap(), 3309);
}

#[test]
fn no_focused_window_gives_pid_zero() {
    let (dir, sig, _server) = fake(DEVICES.into(), "{}".into());
    let mut compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.focused_pid().unwrap(), 0);
}

#[test]
fn set_layout_switches_all_devices() {
    let (dir, sig, server) = fake(DEVICES.into(), "{}".into());
    let mut compositor = Hyprland::new(&dir, &sig);
    compositor.set_layout(2).unwrap();
    assert_eq!(
        server.requests(),
        vec!["/switchxkblayout all 2".to_string()]
    );
}

#[test]
fn layout_list_is_the_majority_layout() {
    let (dir, sig, _server) = fake(DEVICES.into(), "{}".into());
    let mut compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.layout_list().unwrap(), "ru,us");
}

#[test]
fn without_a_server_the_error_names_the_connect() {
    let (dir, _sig) = runtime();
    let compositor = Hyprland::new(&dir, "missing");
    let error = compositor.request("j/devices").unwrap_err();
    assert!(
        error.to_string().starts_with("connect to Hyprland: "),
        "{error}"
    );
}

#[test]
fn a_server_that_closes_without_answering_gives_an_empty_reply() {
    let (dir, sig) = runtime();
    let path = socket_path(&dir, &sig, ".socket.sock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&path).unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut stream = stream;
        let mut buffer = [0u8; 8192];
        let _ = stream.read(&mut buffer); // consume the request, answer nothing
        drop(stream);
    });
    let compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.request("j/devices").unwrap(), "");
    server.join().unwrap();
    std::fs::remove_file(path).ok();
}

#[test]
fn a_reply_larger_than_one_buffer_arrives_whole() {
    let big = format!("{{\"x\":\"{}\"}}", "a".repeat(20_000));
    let (dir, sig, _server) = fake(big.clone(), "{}".into());
    let compositor = Hyprland::new(&dir, &sig);
    assert_eq!(compositor.request("j/devices").unwrap(), big);
}

#[test]
fn events_report_layout_changes_and_reconnect() {
    let (dir, sig) = runtime();
    let path = socket_path(&dir, &sig, ".socket2.sock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&path).unwrap();
    let mut events = HyprlandEvents::new(&dir, &sig);
    assert!(events.fd().is_some());
    let mut stream = listener.accept().unwrap().0;

    // A line split across two writes is one change.
    stream.write_all(b"activelayout>>").unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(!events.layout_changed()); // the line is still partial
    stream.write_all(b"us,ru\n").unwrap();
    assert!(events.layout_changed());

    // Other events are ignored.
    stream
        .write_all(b"activewindow>>foo\nactivelayoutx>>\n")
        .unwrap();
    assert!(!events.layout_changed());

    // When the compositor goes away, the stream goes with it.
    drop(stream);
    assert!(!events.layout_changed());
    assert!(events.fd().is_none());

    // The next call reconnects and reports a change (the unknown past).
    assert!(events.layout_changed());
    assert!(events.fd().is_some());
    let _ = listener.accept(); // the reconnect
}
