//! tmux: running commands against it, parsing their output and asking which
//! pane is visible, ported from `src/tmux.cpp`.

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::io::{Error, ErrorKind, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::tty::{Device, Processes, current_uid, descends_from};
use password_layout_core::compositor::Pid;

/// What `tmux list-panes -a` prints per pane.
pub const TMUX_PANE_FORMAT: &str = "#{pane_tty} #{session_id} #{window_active} #{pane_active}";
/// What `tmux list-clients` prints per client.
pub const TMUX_CLIENT_FORMAT: &str = "#{client_pid} #{session_id} #{client_tty}";
/// The default time a command may take; the watcher must never hang on a
/// stuck server.
pub const COMMAND_TIMEOUT_MS: u64 = 1000;

/// One line of `tmux list-panes -a`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPane {
    /// "/dev/pts/N".
    pub terminal: String,
    /// "$N".
    pub session: String,
    /// The active pane of its session's active window.
    pub active: bool,
}

/// One line of `tmux list-clients`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxClient {
    pub pid: Pid,
    pub session: String,
    pub terminal: String,
}

/// Parses the output of `tmux list-panes -a`. The input is ONE stream of
/// whitespace-separated words, not lines: four words at a time, terminal,
/// session, window_active and pane_active. Stops at the first group that is
/// incomplete or whose numbers do not parse.
pub fn parse_tmux_panes(output: &str) -> Vec<TmuxPane> {
    let mut panes = Vec::new();
    let mut words = output.split_ascii_whitespace();
    while let (Some(terminal), Some(session), Some(window_active), Some(pane_active)) =
        (words.next(), words.next(), words.next(), words.next())
    {
        let (Some(window), Some(pane)) = (
            window_active.parse::<i32>().ok(),
            pane_active.parse::<i32>().ok(),
        ) else {
            break;
        };
        panes.push(TmuxPane {
            terminal: terminal.to_owned(),
            session: session.to_owned(),
            active: window == 1 && pane == 1,
        });
    }
    panes
}

/// Parses the output of `tmux list-clients`: three words at a time, pid,
/// session and terminal, and stops like [`parse_tmux_panes`].
pub fn parse_tmux_clients(output: &str) -> Vec<TmuxClient> {
    let mut clients = Vec::new();
    let mut words = output.split_ascii_whitespace();
    while let (Some(pid), Some(session), Some(terminal)) =
        (words.next(), words.next(), words.next())
    {
        let Some(pid) = pid.parse::<i32>().ok() else {
            break;
        };
        clients.push(TmuxClient {
            pid,
            session: session.to_owned(),
            terminal: terminal.to_owned(),
        });
    }
    clients
}

/// Runs a command and returns what it printed, or nothing if it failed or
/// took longer than `timeout_ms`. The output never reaches a terminal:
/// stderr goes to /dev/null, stdin does not.
pub fn run_command(argv: &[&str], timeout_ms: u64) -> Option<String> {
    if argv.is_empty() {
        return None;
    }
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take().unwrap();

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut bytes = Vec::new();
    let mut timed_out = false;
    loop {
        let left = deadline
            .saturating_duration_since(Instant::now())
            .as_millis();
        if left == 0 {
            timed_out = true;
            break;
        }
        let mut p = libc::pollfd {
            fd: stdout.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one live pollfd on the open stdout descriptor.
        let n = unsafe { libc::poll(&mut p, 1, left.min(i32::MAX as u128) as i32) };
        if n < 0 && Error::last_os_error().kind() == ErrorKind::Interrupted {
            continue;
        }
        if n <= 0 {
            timed_out = n == 0;
            break;
        }
        let mut buf = [0u8; 4096];
        match stdout.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(k) => bytes.extend_from_slice(&buf[..k]),
        }
    }
    drop(stdout);
    if timed_out {
        let _ = child.kill();
    }
    let Ok(status) = child.wait() else {
        return None;
    };
    if timed_out || !status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// What `Tmux::ask` reports about one terminal (C++ `Tmux::Answer`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxAnswer {
    /// A pane of this terminal is visible in a window of the given pid.
    pub visible_in_window: bool,
    /// The terminals the clients are running on; their output redraws panes.
    pub client_terminals: BTreeSet<String>,
}

/// Remembers which socket belongs to which tmux server (C++ `Tmux`).
#[derive(Debug, Default)]
pub struct Tmux {
    sockets: HashMap<Pid, String>,
}

impl Tmux {
    pub fn new() -> Tmux {
        Tmux::default()
    }

    /// Which session is shown in a pane of `tty`, and whether a client that
    /// shows it descends from `window_pid` (C++ `Tmux::ask`).
    pub fn ask(
        &mut self,
        tty: Device,
        window_pid: Pid,
        processes: &Processes,
    ) -> Option<TmuxAnswer> {
        let server = server_of(tty, processes)?;
        let socket = self.socket_of(server)?;
        let client = client_for(server);
        let panes = run_command(
            &[
                &client,
                "-S",
                &socket,
                "list-panes",
                "-a",
                "-F",
                TMUX_PANE_FORMAT,
            ],
            COMMAND_TIMEOUT_MS,
        );
        let clients = run_command(
            &[
                &client,
                "-S",
                &socket,
                "list-clients",
                "-F",
                TMUX_CLIENT_FORMAT,
            ],
            COMMAND_TIMEOUT_MS,
        );
        let (Some(panes), Some(clients)) = (panes, clients) else {
            self.sockets.remove(&server); // the server went away or restarted
            return None;
        };

        let mut answer = TmuxAnswer::default();
        let terminal = terminal_path(tty);
        let mut visible_sessions = BTreeSet::new();
        for pane in parse_tmux_panes(&panes) {
            if pane.terminal == terminal && pane.active {
                visible_sessions.insert(pane.session);
            }
        }
        for client in parse_tmux_clients(&clients) {
            answer.client_terminals.insert(client.terminal);
            if visible_sessions.contains(&client.session)
                && descends_from(client.pid, window_pid, processes)
            {
                answer.visible_in_window = true;
            }
        }
        Some(answer)
    }

    /// The socket a server listens on, remembered once found (C++
    /// `Tmux::socketOf`).
    fn socket_of(&mut self, server: Pid) -> Option<String> {
        if let Some(known) = self.sockets.get(&server) {
            return Some(known.clone());
        }
        let client = client_for(server);
        for socket in sockets_in_directory() {
            let output = run_command(
                &[&client, "-S", &socket, "display-message", "-p", "#{pid}"],
                COMMAND_TIMEOUT_MS,
            );
            if output.as_deref().and_then(|s| s.trim().parse::<i32>().ok()) == Some(server) {
                self.sockets.insert(server, socket.clone());
                return Some(socket);
            }
        }
        None
    }
}

/// "/dev/pts/N" of a terminal device (C++ `terminalPath`).
fn terminal_path(tty: Device) -> String {
    format!("/dev/pts/{}", libc::minor(tty))
}

/// The first line of `/proc/<pid>/comm`; empty when it cannot be read.
fn process_name(pid: Pid) -> String {
    let Ok(bytes) = std::fs::read(format!("/proc/{pid}/comm")) else {
        return String::new();
    };
    let end = bytes
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// The tmux server behind a pane: the parent of a process on its terminal
/// (C++ `serverOf`).
fn server_of(tty: Device, processes: &Processes) -> Option<Pid> {
    processes
        .iter()
        .find(|(_, p)| p.tty == tty && p.parent > 1 && process_name(p.parent).starts_with("tmux"))
        .map(|(_, p)| p.parent)
}

/// The tmux sockets this user owns: the entries of
/// `$TMUX_TMPDIR` (or /tmp) `tmux-<uid>` that are sockets (C++
/// `socketsInDirectory`).
fn sockets_in_directory() -> Vec<String> {
    let base = std::env::var_os("TMUX_TMPDIR")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
    let directory = base.join(format!("tmux-{}", current_uid()));
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| std::fs::metadata(path).is_ok_and(|m| m.file_type().is_socket()))
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

// The client is preferably the server's own executable: after a tmux upgrade
// it still speaks the protocol of the server that is running, which a newer
// client from PATH refuses to do. Inside a user namespace, which the unit's
// sandboxing creates, the kernel denies access to another process's exe link;
// then tmux from PATH is the client, as before.
fn client_for(server: Pid) -> String {
    let exe = format!("/proc/{server}/exe");
    let c = CString::new(exe.clone()).unwrap();
    // SAFETY: c is a valid, NUL-terminated C string.
    let ok = unsafe { libc::access(c.as_ptr(), libc::X_OK) } == 0;
    if ok { exe } else { "tmux".to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The parsers read a word stream, not lines.

    #[test]
    fn panes_split_across_lines_still_parse() {
        let panes = parse_tmux_panes("/dev/pts/1 $0 1 1 /dev/pts/2\n$0 0 1\n\t/dev/pts/3 $1 1 1\n");
        assert_eq!(
            panes,
            vec![
                TmuxPane {
                    terminal: "/dev/pts/1".into(),
                    session: "$0".into(),
                    active: true,
                },
                TmuxPane {
                    terminal: "/dev/pts/2".into(),
                    session: "$0".into(),
                    active: false,
                },
                TmuxPane {
                    terminal: "/dev/pts/3".into(),
                    session: "$1".into(),
                    active: true,
                },
            ]
        );
    }

    #[test]
    fn a_bad_number_stops_parsing() {
        // A number that does not parse ends the output; so does an incomplete group.
        let panes = parse_tmux_panes("/dev/pts/1 $0 1 1\n/dev/pts/2 $0 x 0\n/dev/pts/3 $0 1 0\n");
        assert_eq!(
            panes,
            vec![TmuxPane {
                terminal: "/dev/pts/1".into(),
                session: "$0".into(),
                active: true,
            }]
        );
        assert!(parse_tmux_panes("/dev/pts/1 $0 1\n").is_empty());
        let clients =
            parse_tmux_clients("4242 $0 /dev/pts/1\nnot-a-pid $0 /dev/pts/2\n4243 $0 /dev/pts/1\n");
        assert_eq!(
            clients,
            vec![TmuxClient {
                pid: 4242,
                session: "$0".into(),
                terminal: "/dev/pts/1".into(),
            }]
        );
        assert!(parse_tmux_clients("4242 $0\n").is_empty());
    }
}
