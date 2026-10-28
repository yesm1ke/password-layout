//! Stage 2a/2b tests ported from `tests/test.cpp`, check by check.

mod common;

use std::collections::{BTreeSet, HashMap};
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, Instant};

use password_layout::activity::TtyActivity;
use password_layout::doctor::{
    Mapped, addon_requires_core, exe_replaced, fcitx_keyboard_layouts, maps_library, sudo_version,
    version_less,
};
use password_layout::tmux::{
    COMMAND_TIMEOUT_MS, Tmux, parse_tmux_clients, parse_tmux_panes, run_command,
};
use password_layout::tty::{
    Device, Process, Processes, current_uid, is_password_mode, password_ptys, password_ptys_among,
    read_processes, tty_device, window_owns_tty,
};
use password_layout::watch::PromptWatcher;
use password_layout_core::compositor::{FakeCompositor, Pid};

use crate::common::{PtyChild, terminal_of};

#[test]
fn password_mode_needs_echo_off_and_line_input() {
    assert!(is_password_mode(libc::ICANON));
    assert!(!is_password_mode(libc::ECHO | libc::ICANON));
    assert!(!is_password_mode(0));
    assert!(!is_password_mode(libc::ECHO));
}

#[test]
fn tty_device_decodes_proc_field() {
    assert_eq!(tty_device(0x8803), libc::makedev(136, 3));
    // Minors past 255 spill into the high bits.
    assert_eq!(
        tty_device((1u64 << 20) | 0x8800 | 0x2C),
        libc::makedev(136, 300)
    );
}

#[test]
fn window_owns_tty_follows_parents() {
    let tty: Device = libc::makedev(136, 3);
    let other: Device = libc::makedev(136, 4);
    // terminal(100) -> shell(200) -> sudo(300); unrelated shell(400) elsewhere.
    let processes: Processes = [
        (100, Process { parent: 1, tty: 0 }),
        (200, Process { parent: 100, tty }),
        (300, Process { parent: 200, tty }),
        (
            400,
            Process {
                parent: 1,
                tty: other,
            },
        ),
    ]
    .into_iter()
    .collect::<HashMap<Pid, Process>>();
    assert!(window_owns_tty(100, &BTreeSet::from([tty]), &processes));
    assert!(!window_owns_tty(400, &BTreeSet::from([tty]), &processes));
    assert!(!window_owns_tty(0, &BTreeSet::from([tty]), &processes));
}

// Other terminals on the machine print during a test too, so these tests ask
// about the child's own terminal rather than counting what was reported.

#[test]
fn silent_read_is_a_prompt_owned_by_us() {
    let child = PtyChild::new(&["bash", "-c", "read -s -p pw: x"]);
    let devices = password_ptys(current_uid());
    let processes = read_processes();
    let process = processes
        .get(&child.pid())
        .unwrap_or_else(|| panic!("pid {} is not in /proc", child.pid()));
    assert!(devices.contains(&process.tty));
    assert!(window_owns_tty(
        std::process::id() as i32,
        &devices,
        &processes
    ));
}

#[test]
fn plain_input_is_not_a_prompt() {
    let child = PtyChild::new(&["cat"]);
    let processes = read_processes();
    assert!(processes.contains_key(&child.pid()));
    let process = processes
        .get(&child.pid())
        .unwrap_or_else(|| panic!("pid {} is not in /proc", child.pid()));
    assert!(!password_ptys(current_uid()).contains(&process.tty));
}

#[test]
fn terminal_output_names_the_terminal() {
    let mut activity = TtyActivity::new();
    assert!(activity.available());
    activity.take_active();
    // Started after the watcher exists: the new terminal is reported first, and
    // once that is forgotten, what is left to report is its output.
    let child = PtyChild::new(&["bash", "-c", "sleep 1; echo hello; sleep 5"]);
    let path = crate::common::terminal_of(&child);
    activity.drain();
    assert!(activity.take_active().contains(&path));

    let deadline = crate::common::Deadline::new(5000);
    let mut printed = false;
    while !printed && !deadline.passed() {
        activity.wait(200, None);
        printed = activity.take_active().contains(&path);
    }
    assert!(printed);
}

#[test]
fn output_through_dev_tty_makes_every_terminal_suspect() {
    let child = PtyChild::new(&["bash", "-c", "sleep 1; echo prompt > /dev/tty; sleep 5"]);
    let mut activity = TtyActivity::new();
    activity.take_active();
    let path = crate::common::terminal_of(&child);
    let deadline = crate::common::Deadline::new(5000);
    let mut reported = false;
    while !reported && !deadline.passed() {
        activity.wait(200, None);
        reported = activity.take_active().contains(&path);
    }
    assert!(reported);
}

#[test]
fn prompt_is_found_among_given_terminals() {
    let prompting = PtyChild::new(&["bash", "-c", "read -s -p pw: x"]);
    let plain = PtyChild::new(&["cat"]);
    let path = crate::common::terminal_of(&prompting);
    let prompts = password_ptys_among(&BTreeSet::from([
        path.clone(),
        crate::common::terminal_of(&plain),
    ]));
    assert_eq!(prompts.len(), 1);
    assert!(prompts.contains_key(&path));
    let processes = read_processes();
    let process = processes
        .get(&prompting.pid())
        .unwrap_or_else(|| panic!("pid {} is not in /proc", prompting.pid()));
    assert_eq!(prompts[&path], process.tty);
    assert!(password_ptys_among(&BTreeSet::new()).is_empty());
}

#[test]
fn tmux_output_is_parsed() {
    let panes = parse_tmux_panes("/dev/pts/7 $0 1 1\n/dev/pts/8 $0 1 0\n/dev/pts/9 $1 0 1\n");
    assert_eq!(panes.len(), 3);
    assert_eq!(panes[0].terminal, "/dev/pts/7");
    assert_eq!(panes[0].session, "$0");
    assert!(panes[0].active);
    assert!(!panes[1].active); // pane in the active window, not active itself
    assert!(!panes[2].active); // active pane of a window in the background

    let clients = parse_tmux_clients("4242 $0 /dev/pts/3\n");
    assert_eq!(clients.len(), 1);
    assert_eq!(clients[0].pid, 4242);
    assert_eq!(clients[0].session, "$0");
    assert_eq!(clients[0].terminal, "/dev/pts/3");
    assert!(parse_tmux_panes("").is_empty());
    assert!(parse_tmux_clients("").is_empty());
}

#[test]
fn tmux_pane_is_seen_only_while_visible() {
    if run_command(&["tmux", "-V"], COMMAND_TIMEOUT_MS).is_none() {
        println!("skipped: tmux is not installed");
        return;
    }
    let _server = crate::common::TmuxServer::new();
    // Window numbering depends on the user's base-index, so windows are named
    // by the ids tmux prints for them.
    let first = crate::common::TmuxServer::tmux(&[
        "new-session",
        "-d",
        "-P",
        "-F",
        "#{pane_tty}",
        "-s",
        "t",
        "-x",
        "80",
        "-y",
        "24",
        "bash -c 'read -s -p pw: x; sleep 30'",
    ]);
    let second = crate::common::TmuxServer::tmux(&[
        "new-window",
        "-d",
        "-P",
        "-F",
        "#{window_id}",
        "-t",
        "t:",
        "sleep 30",
    ]);
    let (Some(first), Some(second)) = (first, second) else {
        return;
    };
    // The client runs on a terminal this test process started, so the test
    // plays the part of the terminal window. tmux refuses to attach without a
    // terminal type, and CI containers have no TERM; env keeps the process id.
    let client = PtyChild::new(&[
        "env",
        "TERM=xterm",
        "tmux",
        "-L",
        crate::common::TmuxServer::NAME,
        "attach",
        "-t",
        "t",
    ]);

    let path = first.split_once('\n').map_or(first.as_str(), |p| p.0);
    let pane = std::fs::metadata(path)
        .expect("the pane terminal exists")
        .rdev();

    let mut tmux = Tmux::new();
    let processes = read_processes();
    let shown = tmux.ask(pane, std::process::id() as i32, &processes);
    let shown = shown.expect("the visible pane is seen");
    assert!(shown.visible_in_window);
    assert_eq!(shown.client_terminals.len(), 1);
    // And the pane is at the prompt.
    assert!(password_ptys_among(&BTreeSet::from([path.to_string()])).contains_key(path));

    // Another window, the one the client is not running in.
    let elsewhere = tmux
        .ask(pane, client.pid() + 100000, &read_processes())
        .expect("the pane is still a tmux pane");
    assert!(!elsewhere.visible_in_window);

    // Switch the session to its second window: the pane is out of sight.
    let window = second.split_once('\n').map_or(second.as_str(), |w| w.0);
    assert!(crate::common::TmuxServer::tmux(&["select-window", "-t", window]).is_some());
    let hidden = tmux.ask(pane, std::process::id() as i32, &read_processes());
    let hidden = hidden.expect("the pane is still a tmux pane");
    assert!(!hidden.visible_in_window);

    // A terminal that is not a tmux pane at all.
    let plain = PtyChild::new(&["cat"]);
    let processes = read_processes();
    let process = processes
        .get(&plain.pid())
        .unwrap_or_else(|| panic!("pid {} is not in /proc", plain.pid()));
    assert!(
        tmux.ask(process.tty, std::process::id() as i32, &processes)
            .is_none()
    );
}

#[test]
fn watcher_finds_the_prompt_in_the_focused_window() {
    // The test process plays the terminal window: the prompt's terminal
    // belongs to it, since its process descends from this one.
    let prompt = PtyChild::new(&["bash", "-c", "read -s -p pw: x; sleep 30"]);
    let path = terminal_of(&prompt);
    let found = password_ptys_among(&BTreeSet::from([path.clone()]));
    assert!(found.contains_key(&path));
    let devices = BTreeSet::from([found[&path]]);
    let mut compositor = FakeCompositor::new(1, vec![true, false]);
    let mut watcher = PromptWatcher::new();
    let us = std::process::id() as i32;

    compositor.focused = us;
    let seen = watcher
        .look(&mut compositor, devices.clone(), &BTreeSet::new())
        .unwrap();
    assert!(seen.pending && seen.in_focus);

    compositor.focused = 1; // another window takes focus
    let seen = watcher
        .look(&mut compositor, devices.clone(), &BTreeSet::new())
        .unwrap();
    assert!(seen.pending && !seen.in_focus);

    compositor.focused = us; // and gives it back
    let seen = watcher
        .look(&mut compositor, devices.clone(), &BTreeSet::new())
        .unwrap();
    assert!(seen.pending && seen.in_focus);

    let seen = watcher
        .look(&mut compositor, BTreeSet::new(), &BTreeSet::new())
        .unwrap(); // the prompt is gone
    assert!(!seen.pending && !seen.in_focus);
}

// Ported from `tests/test.cpp::doctorHelpersReadVersionsAndMaps`, check by check.
#[test]
fn doctor_helpers_read_versions_and_maps() {
    assert!(version_less("5.1.15-1", "5.1.16"));
    assert!(!version_less("5.1.16-1", "5.1.16"));
    assert!(!version_less("5.2", "5.1.16"));
    assert!(version_less("1.9.13p3", "1.9.14"));
    assert!(!version_less("1.9.17p2", "1.9.14"));
    assert_eq!(
        sudo_version("Sudo version 1.9.17p2\nSudoers policy plugin version 1.9.17p2\n"),
        Some("1.9.17p2".to_string())
    );
    assert_eq!(sudo_version("sudo: command not found"), None);
    let maps = "7f00-7f01 r-xp 00000000 00:1f 123  /usr/lib/fcitx5/libpasswordlayout.so\n"
        .to_string()
        + "7f02-7f03 r--p 00000000 00:1f 124  /usr/lib/libnotpasswordlayout.so.1\n";

    assert_eq!(maps_library(&maps, "libpasswordlayout.so"), Mapped::Yes);
    assert_eq!(maps_library(&maps, "libFcitx5Core.so"), Mapped::No);
    assert_eq!(
        maps_library(
            "7f00-7f01 r-xp 0 00:1f 123  /usr/lib/fcitx5/libpasswordlayout.so (deleted)\n",
            "libpasswordlayout.so"
        ),
        Mapped::Replaced
    );
    assert!(exe_replaced("/usr/bin/password-layout (deleted)"));
    let omarchy = "[Groups/0]\nName=Default\nDefault Layout=us\nDefaultIM=keyboard-us\n\n"
        .to_string()
        + "[Groups/0/Items/0]\nName=keyboard-us\n\n[GroupOrder]\n0=Default\n";
    assert_eq!(
        fcitx_keyboard_layouts(&omarchy),
        vec!["keyboard-us".to_string()]
    );
    let switching = format!(
        "{omarchy}[Groups/0/Items/1]\nName=keyboard-ru\n[Groups/1/Items/0]\nName=keyboard-us\n"
    ) + "[Groups/1/Items/1]\nName=pinyin\n";
    assert_eq!(
        fcitx_keyboard_layouts(&switching),
        vec!["keyboard-us".to_string(), "keyboard-ru".to_string()]
    );
    assert!(!exe_replaced("/usr/bin/password-layout"));
    assert_eq!(
        Mapped::No,
        maps_library(
            "7f02-7f03 r--p 0 00:1f 124  /x/libpasswordlayout.so.old\n",
            "libpasswordlayout.so"
        )
    );
    assert_eq!(
        addon_requires_core("[Addon]\nName=x\n\n[Addon/Dependencies]\n0=core:5.1.23\n"),
        Some("5.1.23".to_string())
    );
    assert_eq!(addon_requires_core("[Addon]\nName=x\n"), None);
    assert!(version_less("5.1.22", "5.1.23"));
}

#[test]
fn commands_are_run_and_timed_out() {
    // The C++ tests used the default timeout: COMMAND_TIMEOUT_MS.
    assert_eq!(
        run_command(&["echo", "hello"], COMMAND_TIMEOUT_MS),
        Some("hello\n".to_owned())
    );
    assert!(run_command(&["false"], COMMAND_TIMEOUT_MS).is_none());
    assert!(run_command(&["password-layout-no-such-command"], COMMAND_TIMEOUT_MS).is_none());
    let started = Instant::now();
    assert!(run_command(&["sleep", "5"], 200).is_none());
    assert!(started.elapsed() < Duration::from_secs(2));
}
