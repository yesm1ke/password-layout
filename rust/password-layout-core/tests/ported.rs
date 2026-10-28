//! The C++ tests of stage 1, ported check by check from tests/test.cpp.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use password_layout_core::compositor::FakeCompositor;
use password_layout_core::hyprland;
use password_layout_core::json;
use password_layout_core::latin::latin_layouts;
use password_layout_core::pace::{Pace, next_pace};
use password_layout_core::state::{State, is_valid_holder};

static DIRECTORY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Like mkdtemp, with std only: a unique name, created empty.
fn temporary_directory() -> std::path::PathBuf {
    let pid = std::process::id();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    loop {
        let number = DIRECTORY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("password-layout-test-{pid}-{now}-{number}"));
        match std::fs::create_dir(&directory) {
            Ok(()) => return directory,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => panic!("mkdtemp: {error}"),
        }
    }
}

#[test]
fn state_switches_to_latin_and_back() {
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 0);
    assert_eq!(state.holders().unwrap(), vec!["tty".to_string()]);
    state.leave("tty", &mut compositor).unwrap();
    assert_eq!(compositor.switches, vec![0, 1]);
    assert!(state.holders().unwrap().is_empty());
}

#[test]
fn state_leaves_latin_alone() {
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(0, vec![true, false]);
    state.enter("tty", &mut compositor).unwrap();
    state.leave("tty", &mut compositor).unwrap();
    assert!(compositor.switches.is_empty());
}

#[test]
fn state_restores_only_after_last_holder() {
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    state.enter("tty", &mut compositor).unwrap();
    state.enter("im", &mut compositor).unwrap();
    state.leave("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 0);
    state.leave("im", &mut compositor).unwrap();
    assert_eq!(compositor.switches, vec![0, 1]);
}

#[test]
fn state_ignores_leave_without_enter() {
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    state.leave("tty", &mut compositor).unwrap();
    assert!(compositor.switches.is_empty());
}

#[test]
fn holder_names_are_one_safe_line() {
    assert!(is_valid_holder("tty"));
    assert!(is_valid_holder("im"));
    assert!(is_valid_holder("my-source-2"));
    assert!(!is_valid_holder(""));
    assert!(!is_valid_holder("tty\nim"));
    assert!(!is_valid_holder("Tty"));
    assert!(!is_valid_holder("../x"));
    assert!(!is_valid_holder(&"a".repeat(33)));

    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    let rejected = state
        .enter("tty\n7", &mut compositor)
        .is_err_and(|e| e.kind() == std::io::ErrorKind::InvalidInput);
    assert!(rejected);
    assert!(compositor.switches.is_empty());
    assert!(state.holders().unwrap().is_empty());
}

#[test]
fn stale_holder_is_given_back_on_restart() {
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    state.enter("im", &mut compositor).unwrap(); // a password field; then fcitx5 dies
    assert_eq!(compositor.index, 0);
    compositor.index = 1; // the user switches back by hand

    // While the dead source's "im" is held, a terminal prompt switches nothing.
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 1);
    state.leave("tty", &mut compositor).unwrap();

    // The restarted addon gives "im" back first; prompts work again.
    state.leave("im", &mut compositor).unwrap();
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 0);
    state.leave("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 1);
}

#[test]
fn broken_state_file_restores_nothing() {
    let directory = temporary_directory();
    let state = State::new(&directory);
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    // A first line that is not a number used to read as "restore layout 0".
    for broken in ["\ntty\n", "x\ntty\n", "1x\ntty\n", "1\ntty\nnot a holder\n"] {
        std::fs::write(directory.join("state"), broken).unwrap();
        compositor.switches.clear();
        state.leave("tty", &mut compositor).unwrap();
        assert!(compositor.switches.is_empty());
    }
    // No temporary file is left behind by a save.
    state.enter("tty", &mut compositor).unwrap();
    state.leave("tty", &mut compositor).unwrap();
    assert!(!directory.join("state.tmp").exists());
}

#[test]
fn state_file_is_byte_compatible_with_the_cpp() {
    // enter tty at layout 1 of us,ru -> `1\ntty\n`; already Latin -> `-1\ntty\n`.
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    state.enter("tty", &mut compositor).unwrap();
    state.enter("im", &mut compositor).unwrap();
    assert_eq!(
        std::fs::read(state.directory().join("state")).unwrap(),
        b"1\ntty\nim\n"
    );
    state.leave("tty", &mut compositor).unwrap();
    assert_eq!(
        std::fs::read(state.directory().join("state")).unwrap(),
        b"1\nim\n"
    );

    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(0, vec![true, false]);
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(
        std::fs::read(state.directory().join("state")).unwrap(),
        b"-1\ntty\n"
    );

    // note_layout writes the same format to last-latin.
    let mut compositor = FakeCompositor::new(2, vec![false, true, true]);
    state.note_layout(&mut compositor).unwrap();
    assert_eq!(
        std::fs::read(state.directory().join("last-latin")).unwrap(),
        b"2\n"
    );
}

#[test]
fn state_uses_the_latin_layout_wherever_it_is() {
    // kb_layout = ru,us: Russian active, Latin is the second layout.
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(0, vec![false, true]);
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 1);
    state.leave("tty", &mut compositor).unwrap();
    assert_eq!(compositor.switches, vec![1, 0]);
}

#[test]
fn state_does_nothing_without_a_latin_layout() {
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(0, vec![false, false]);
    state.enter("tty", &mut compositor).unwrap();
    state.leave("tty", &mut compositor).unwrap();
    assert!(compositor.switches.is_empty());
    assert!(state.holders().unwrap().is_empty());
}

#[test]
fn state_keeps_a_latin_layout_that_is_not_the_first() {
    // us,de,ru with German active: already Latin, so nothing to do - as macOS.
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, true, false]);
    state.enter("tty", &mut compositor).unwrap();
    state.leave("tty", &mut compositor).unwrap();
    assert!(compositor.switches.is_empty());
}

#[test]
fn state_prefers_the_last_latin_layout_used() {
    // us,de,ru: German was in use before switching to Russian.
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, true, false]);
    state.note_layout(&mut compositor).unwrap();
    assert_eq!(state.last_latin(), Some(1));
    compositor.index = 2;
    state.note_layout(&mut compositor).unwrap(); // Russian is not Latin, not remembered
    assert_eq!(state.last_latin(), Some(1));
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 1);
    state.leave("tty", &mut compositor).unwrap();
    assert_eq!(compositor.switches, vec![1, 2]);
}

#[test]
fn state_ignores_a_remembered_layout_that_is_gone() {
    // The config changed since: layout 1 is now Russian.
    let state = State::new(temporary_directory());
    let mut compositor = FakeCompositor::new(1, vec![true, true]);
    state.note_layout(&mut compositor).unwrap();
    compositor.latin = vec![true, false];
    state.enter("tty", &mut compositor).unwrap();
    assert_eq!(compositor.index, 0);
}

#[test]
fn latin_layouts_are_found_by_name() {
    assert_eq!(latin_layouts("us,ru", ","), vec![true, false]);
    assert_eq!(latin_layouts("ru,us", ","), vec![false, true]);
    assert_eq!(latin_layouts("ua,ru,de", ",,"), vec![false, false, true]);
    assert_eq!(latin_layouts("rs,rs", ",latin"), vec![false, true]);
    assert_eq!(latin_layouts("us", ""), vec![true]);
    assert_eq!(latin_layouts("", ""), vec![false]);
}

#[test]
fn layouts_are_latin_as_measured() {
    // tests/xkb-latin.tsv: every xkeyboard-config layout and how many letters
    // a-z its base level types (tools/xkb-latin.c). Latin means at least 20.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/xkb-latin.tsv");
    let table = std::fs::read_to_string(&path).expect("tests/xkb-latin.tsv is readable");
    let mut checked = 0;
    // split('\n') like std::getline, not lines(): '\r' stays an ordinary
    // character.
    for line in table.split('\n') {
        if line.is_empty() {
            continue;
        }
        let (name, count) = line.split_once('\t').expect("tab-separated row");
        let measured: i32 = count.parse().expect("row count is a number");
        let (layout, variant) = match name.find('(') {
            Some(open) => (&name[..open], &name[open + 1..name.len() - 1]),
            None => (name, ""),
        };
        let latin = latin_layouts(layout, variant);
        assert!(
            latin == [measured >= 20],
            "{name}: measured {}",
            if measured >= 20 { "Latin" } else { "not Latin" }
        );
        checked += 1;
    }
    assert!(checked > 500, "checked {checked} rows");
}

#[test]
fn json_integers_finds_every_value() {
    let devices = r#"{"keyboards": [
        {"name": "a", "active_layout_index": 1, "main": false},
        {"name": "b", "active_layout_index": 0, "main": true},
        {"name": "c", "active_layout_index": 1, "main": false}]})"#;
    assert_eq!(
        json::json_integers(devices, "active_layout_index"),
        vec![1, 0, 1]
    );
    assert_eq!(
        hyprland::most_common(&json::json_integers(devices, "active_layout_index"), 0),
        1
    );
    assert!(json::json_integers("{}", "pid").is_empty());
    assert_eq!(hyprland::most_common(&[], 7), 7);
}

#[test]
fn json_integers_ignores_keys_inside_strings() {
    // A window title is user-controlled text; its quotes arrive escaped.
    let window = r#"{"title": "fake \"pid\": 999 here", "pid": 3309, "xwayland": false}"#;
    assert_eq!(json::json_integers(window, "pid"), vec![3309]);
}

#[test]
fn json_strings_are_unescaped() {
    let devices = r#"{"keyboards": [
        {"name": "a \"layout\": no", "layout": "ru,us", "variant": ",", "active_layout_index": 0},
        {"name": "b", "layout": "us", "variant": "", "active_layout_index": 0}]})"#;
    assert_eq!(json::json_strings(devices, "layout"), vec!["ru,us", "us"]);
    assert_eq!(json::json_strings(devices, "variant"), vec![",", ""]);
    assert_eq!(
        json::json_strings(devices, "name"),
        vec![r#"a "layout": no"#, "b"]
    );
    assert!(json::json_strings(devices, "active_layout_index").is_empty()); // not a string
}

#[test]
fn keyboard_layouts_follow_the_majority() {
    // A virtual keyboard listed first with its own layout must not win.
    let devices = r#"{"keyboards": [
        {"name": "virtual", "layout": "us", "variant": "", "main": false},
        {"name": "real", "layout": "us,ru", "variant": ",", "main": false},
        {"name": "sleep-button", "layout": "us,ru", "variant": ",", "main": true}]})"#;
    assert_eq!(
        hyprland::keyboard_layouts(devices),
        ("us,ru".to_string(), ",".to_string())
    );
    assert!(hyprland::keyboard_layouts("{}").0.is_empty());
}

#[test]
fn silent_compositor_times_out() {
    use password_layout_core::compositor::Compositor;
    use password_layout_core::hyprland::{Hyprland, socket_path};
    use std::os::unix::net::UnixListener;

    // A socket that accepts nothing: connect queues, the read times out.
    // Short name: Unix socket paths must stay under 108 bytes.
    let pid = std::process::id();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let directory = std::env::temp_dir().join(format!("plt-{pid}-{now}"));
    let path = socket_path(&directory, "test", ".socket.sock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let _listener = UnixListener::bind(&path).unwrap();

    let start = std::time::Instant::now();
    let mut compositor = Hyprland::new(&directory, "test");
    let error = compositor.layout_index().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(3),
        "timed out in {}",
        start.elapsed().as_secs_f32()
    );
    std::fs::remove_file(path).ok();
}

#[test]
fn the_loop_keeps_its_pace() {
    // Woken from quiet by output: a second look before anything else.
    assert_eq!(next_pace(true, false, true, true), Pace::SecondLook);
    // A prompt found on the first look needs no second one, but polling.
    assert_eq!(next_pace(true, true, true, true), Pace::Poll);
    // While a prompt is up, or terminals keep printing: poll.
    assert_eq!(next_pace(false, true, false, true), Pace::Poll);
    assert_eq!(next_pace(false, false, true, true), Pace::Poll);
    // Without inotify there is nothing to wait on.
    assert_eq!(next_pace(false, false, false, false), Pace::Poll);
    // Quiet: sleep, costing nothing.
    assert_eq!(next_pace(false, false, false, true), Pace::Sleep);
}
