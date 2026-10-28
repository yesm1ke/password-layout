//! The process table and terminal devices, ported from `src/tty.cpp`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

use password_layout_core::compositor::Pid;

pub type Device = libc::dev_t;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Process {
    pub parent: Pid,
    pub tty: Device,
}

pub type Processes = HashMap<Pid, Process>;

/// C++ `isPasswordMode`: echo off and line input on, other bits do not matter.
pub fn is_password_mode(local_flags: libc::tcflag_t) -> bool {
    local_flags & libc::ECHO == 0 && local_flags & libc::ICANON != 0
}

/// C++ `ttyDevice`: decode the `tty_nr` field of `/proc/<pid>/stat`.
pub fn tty_device(tty_nr: u64) -> Device {
    let major = (tty_nr >> 8) & 0xFFF;
    let minor = (tty_nr & 0xFF) | ((tty_nr >> 12) & 0xFFF00);
    libc::makedev(major as u32, minor as u32)
}

/// The parsing part of C++ `readProcesses`: the parent and the tty device
/// from one `/proc/<pid>/stat` line.
pub fn parse_stat(stat: &[u8]) -> Option<Process> {
    // The command name sits in parentheses and may itself contain spaces,
    // ')' and non-UTF-8 bytes, so the fields are counted from the last ')'.
    let close = stat.iter().rposition(|b| *b == b')')?;
    let fields = std::str::from_utf8(&stat[close + 1..]).ok()?;
    let mut tokens = fields.split_ascii_whitespace();
    let _state = tokens.next()?;
    let parent = tokens.next()?.parse::<Pid>().ok()?;
    let _group = tokens.next()?.parse::<i32>().ok()?;
    let _session = tokens.next()?.parse::<i32>().ok()?;
    let tty_nr = tokens.next()?.parse::<u64>().ok()?;
    Some(Process {
        parent,
        tty: tty_device(tty_nr),
    })
}

/// C++ `readProcesses`: every process in `/proc` that has a readable `stat`.
///
/// `fs::read`, not `fs::read_to_string`: a non-UTF-8 command name must not
/// drop the process.
pub fn read_processes() -> Processes {
    let mut processes = Processes::new();
    let directory = match fs::read_dir("/proc") {
        Ok(directory) => directory,
        Err(_) => return processes,
    };
    for entry in directory.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(name) if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) => name,
            _ => continue,
        };
        let pid = match name.parse::<Pid>() {
            Ok(pid) => pid,
            Err(_) => continue,
        };
        let stat = match fs::read(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(_) => continue,
        };
        if let Some(process) = parse_stat(&stat) {
            processes.insert(pid, process);
        }
    }
    processes
}

/// C++ `descendsFrom`: is `pid` `ancestor` or a descendant of it?
pub fn descends_from(pid: Pid, ancestor: Pid, processes: &Processes) -> bool {
    if ancestor <= 0 {
        return false;
    }
    // Bounded, in case a parent chain read mid-change ever loops.
    let mut pid = pid;
    let mut steps = 0usize;
    while pid > 1 && steps <= processes.len() {
        if pid == ancestor {
            return true;
        }
        pid = processes.get(&pid).map_or(0, |p| p.parent);
        steps += 1;
    }
    false
}

/// C++ `windowOwnsTty`: some process in `processes` has one of `devices` and
/// descends from `window_pid`.
pub fn window_owns_tty(window_pid: Pid, devices: &BTreeSet<Device>, processes: &Processes) -> bool {
    processes.iter().any(|(pid, process)| {
        devices.contains(&process.tty) && descends_from(*pid, window_pid, processes)
    })
}

/// C++ `getuid()`: the id of the user that runs the service.
pub fn current_uid() -> u32 {
    // SAFETY: getuid cannot fail.
    unsafe { libc::getuid() }
}

// Whether the terminal at `path` is at a password prompt; returns its device
// number when it is.
fn at_password_prompt(path: &str) -> Option<Device> {
    // Opened read-only and closed on every look: a descriptor kept on the
    // slave would stop the terminal from seeing its shell exit. O_NOCTTY is
    // essential: the service is a session leader without a controlling
    // terminal, and opening a pty slave without it would make that pty its
    // controlling terminal. std adds O_CLOEXEC itself.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOCTTY | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    let mut settings: libc::termios = unsafe { std::mem::zeroed() };
    // SAFETY: termios is plain integers, so zeroed bytes are a valid value.
    // SAFETY: the fd is open for the duration of the call and settings is live.
    let rc = unsafe { libc::tcgetattr(file.as_raw_fd(), &mut settings) };
    if rc != 0 || !is_password_mode(settings.c_lflag) {
        // rc is non-zero (ENOTTY) for /dev/null and regular files.
        return None;
    }
    file.metadata().ok().map(|metadata| metadata.rdev())
}

/// C++ `passwordPtys`: the devices of our terminals at a password prompt.
pub fn password_ptys(uid: u32) -> BTreeSet<Device> {
    let mut found = BTreeSet::new();
    let directory = match fs::read_dir("/dev/pts") {
        Ok(directory) => directory,
        Err(_) => return found,
    };
    for entry in directory.flatten() {
        let name = match entry.file_name().into_string() {
            Ok(name) if !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit()) => name,
            _ => continue,
        };
        let path = format!("/dev/pts/{}", name);
        // fs::metadata follows links, like C++ stat.
        let owned = match fs::metadata(&path) {
            Ok(meta) => meta.uid() == uid,
            Err(_) => false,
        };
        if !owned {
            continue;
        }
        if let Some(device) = at_password_prompt(&path) {
            found.insert(device);
        }
    }
    found
}

/// C++ `passwordPtysAmong`: the password prompts among `paths`, by path.
pub fn password_ptys_among(paths: &BTreeSet<String>) -> BTreeMap<String, Device> {
    let mut found = BTreeMap::new();
    for path in paths {
        if let Some(device) = at_password_prompt(path) {
            found.insert(path.clone(), device);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_stat_reads_parent_and_tty() {
        let process = parse_stat(b"42 (bash) S 7 42 42 34819 0 -1 4194304").unwrap();
        assert_eq!(process.parent, 7);
        assert_eq!(process.tty, libc::makedev(136, 3));
    }

    #[test]
    fn parse_stat_counts_fields_from_the_last_bracket() {
        // The command name may contain spaces and ')' itself.
        let process = parse_stat(b"5 (we ) ird) S 3 5 5 34819 0 -1 4194304").unwrap();
        assert_eq!(process.parent, 3);
        assert_eq!(process.tty, libc::makedev(136, 3));
    }

    #[test]
    fn parse_stat_rejects_too_few_fields() {
        assert_eq!(parse_stat(b"6 (x) S 3"), None);
        assert_eq!(parse_stat(b"6 (x)"), None);
        assert_eq!(parse_stat(b""), None);
    }

    #[test]
    fn not_a_prompt() {
        // /dev/null and regular files give ENOTTY; missing paths do not open.
        assert_eq!(at_password_prompt("/dev/null"), None);
        assert_eq!(at_password_prompt("/etc/hostname"), None);
        assert_eq!(at_password_prompt("/nonexistent-tty"), None);
    }

    #[test]
    fn empty_path_set_has_no_prompts() {
        assert!(password_ptys_among(&BTreeSet::new()).is_empty());
    }
}
