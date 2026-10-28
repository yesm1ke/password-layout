//! The built binary: usage, exit codes, the Hyprland client end to end, and
//! SIGTERM ending `watch-tty` cleanly. Each case runs with a fresh short temp
//! dir under `/tmp` as XDG_RUNTIME_DIR and without
//! HYPRLAND_INSTANCE_SIGNATURE unless a case sets it.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use password_layout_core::fake_hyprland::{FakeHyprland, FakeReplies};

const BIN: &str = env!("CARGO_BIN_EXE_password-layout");
/// The instance signature the cases that set one use; it keeps the socket
/// paths short.
const SIG: &str = "sig";
/// One keyboard with a non-Latin layout first and a Latin one second.
const DEVICES: &str = r#"{"keyboards": [{"name": "kb", "layout": "ru,us", "variant": ",", "active_layout_index": 0}]}"#;

/// A fresh state directory for one case; short, so the socket paths under it
/// stay short.
fn runtime_dir(test: &str) -> std::path::PathBuf {
    fresh_dir("cli", test)
}

/// A fresh empty directory to stand in for `PATH` in the `doctor` cases.
fn path_dir(test: &str) -> std::path::PathBuf {
    fresh_dir("cli-path", test)
}

fn fresh_dir(prefix: &str, test: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new("/tmp").join(format!("{prefix}-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(test: &str, args: &[&str]) -> std::process::Output {
    let dir = runtime_dir(test);
    Command::new(BIN)
        .args(args)
        .env("XDG_RUNTIME_DIR", &dir)
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .output()
        .unwrap()
}

/// As [`run`], with the instance signature set.
fn run_with_sig(test: &str, args: &[&str]) -> std::process::Output {
    let dir = runtime_dir(test);
    Command::new(BIN)
        .args(args)
        .env("XDG_RUNTIME_DIR", &dir)
        .env("HYPRLAND_INSTANCE_SIGNATURE", SIG)
        .output()
        .unwrap()
}

/// `doctor` runs with the environment emptied and `PATH` pointing at an
/// empty (or stub-filled) temp dir, so that only the commands the case
/// provides can be run.
fn doctor(path: &std::path::Path, extra_env: &[(&str, &str)]) -> std::process::Output {
    let mut command = Command::new(BIN);
    command.arg("doctor").env_clear().env("PATH", path);
    for (name, value) in extra_env {
        command.env(name, value);
    }
    command.output().unwrap()
}

#[test]
fn doctor_without_environment_fails() {
    let path = path_dir("doctor-bare");
    let out = doctor(&path, &[]);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let expected = concat!(
        "FAIL  Hyprland is not reachable: XDG_RUNTIME_DIR is not set\n",
        "      -> run this inside your Hyprland session\n",
        "FAIL  terminal service: systemctl --user does not answer\n",
        "      -> the service needs a systemd user session\n"
    );
    assert!(stdout.starts_with(expected), "stdout: {stdout:?}");
}

#[test]
fn doctor_with_fake_hyprland_reads_layouts() {
    let dir = runtime_dir("doctor-fake");
    let _fake = FakeHyprland::start(
        &dir,
        SIG,
        FakeReplies {
            devices: DEVICES.to_string(),
            active_window: String::new(),
        },
    )
    .unwrap();
    let path = path_dir("doctor-fake");
    let out = doctor(
        &path,
        &[
            ("XDG_RUNTIME_DIR", dir.to_str().unwrap()),
            ("HYPRLAND_INSTANCE_SIGNATURE", SIG),
        ],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.lines().next(),
        Some("ok    Hyprland: layouts ru,us; Latin: us"),
        "stdout: {stdout:?}"
    );
}

#[test]
fn doctor_runs_stub_systemctl() {
    let path = path_dir("doctor-stub");
    let script = path.join("systemctl");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf 'LoadState=loaded\nActiveState=active\nNRestarts=0\n\
UnitFileState=enabled\nMainPID=0\n'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    let mut permissions = std::fs::metadata(&script).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&script, permissions).unwrap();
    let out = doctor(&path, &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    // The second finding (hint lines are indented):
    let lines: Vec<&str> = stdout
        .lines()
        .filter(|line| !line.starts_with("      ->"))
        .collect();
    assert_eq!(
        lines.get(1),
        Some(&"ok    terminal service: running"),
        "stdout: {stdout:?}"
    );
}

#[test]
fn no_arguments_show_the_usage() {
    let out = run("usage", &[]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("usage: password-layout enter <holder>"),
        "stderr: {stderr:?}"
    );
}

#[test]
fn without_a_signature_the_message_names_it() {
    let out = run("enter-no-sig", &["enter", "x"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        stderr,
        "password-layout: HYPRLAND_INSTANCE_SIGNATURE is not set\n"
    );
}

#[test]
fn without_the_socket_the_message_names_the_connect() {
    let out = run_with_sig("enter-no-socket", &["enter", "x"]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with("password-layout: connect to Hyprland: "),
        "stderr: {stderr:?}"
    );
}

#[test]
fn leave_of_an_absent_holder_succeeds() {
    let out = run_with_sig("leave", &["leave", "x"]);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn enter_and_leave_switch_layouts() {
    let dir = runtime_dir("switch");
    let fake = FakeHyprland::start(
        &dir,
        SIG,
        FakeReplies {
            devices: DEVICES.to_string(),
            active_window: String::new(),
        },
    )
    .unwrap();
    let out = Command::new(BIN)
        .args(["enter", "x"])
        .env("XDG_RUNTIME_DIR", &dir)
        .env("HYPRLAND_INSTANCE_SIGNATURE", SIG)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let requests = fake.requests();
    assert_eq!(*requests.last().unwrap(), "/switchxkblayout all 1");
    let out = Command::new(BIN)
        .args(["leave", "x"])
        .env("XDG_RUNTIME_DIR", &dir)
        .env("HYPRLAND_INSTANCE_SIGNATURE", SIG)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(*fake.requests().last().unwrap(), "/switchxkblayout all 0");
}

#[test]
fn status_shows_the_layout_index() {
    let dir = runtime_dir("status");
    let _fake = FakeHyprland::start(
        &dir,
        SIG,
        FakeReplies {
            devices: DEVICES.to_string(),
            active_window: "{}".to_string(),
        },
    )
    .unwrap();
    let out = Command::new(BIN)
        .arg("status")
        .env("XDG_RUNTIME_DIR", &dir)
        .env("HYPRLAND_INSTANCE_SIGNATURE", SIG)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().any(|line| line == "layout index:        0"),
        "stdout: {stdout:?}"
    );
}

#[test]
fn sigterm_ends_watch_tty_cleanly() {
    let dir = runtime_dir("watch-tty");
    let mut child = Command::new(BIN)
        .arg("watch-tty")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env("XDG_RUNTIME_DIR", &dir)
        .env("HYPRLAND_INSTANCE_SIGNATURE", SIG)
        .spawn()
        .unwrap();
    // Let it install its state and reach the loop.
    std::thread::sleep(Duration::from_millis(500));
    // std Child::kill sends SIGKILL, not wanted: the binary must handle
    // SIGTERM itself and exit 0.
    // SAFETY: child.id() is the pid of the spawned child.
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait().unwrap() {
            Some(status) => {
                assert_eq!(status.code(), Some(0));
                break;
            }
            None => {
                assert!(
                    deadline.elapsed() < Duration::from_secs(2),
                    "watch-tty did not exit within 2 s of SIGTERM"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}
