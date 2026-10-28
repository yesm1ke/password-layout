//! Terminal activity through inotify, ported from `src/activity.cpp`.

use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};

/// One record of an inotify queue: the watch it belongs to, the events and
/// the name without NUL padding (empty for directory and device watches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InotifyEvent {
    pub wd: i32,
    pub mask: u32,
    pub name: Vec<u8>,
}

/// Parses the queue as the kernel writes it: records of a 16-byte header
/// (`wd`, `mask`, `cookie`, `len`, native endian) followed by `len` name
/// bytes. Stops at a record that does not fit in the buffer.
pub fn parse_inotify_events(buffer: &[u8]) -> Vec<InotifyEvent> {
    let mut events = Vec::new();
    let mut at = 0usize;
    while at + 16 <= buffer.len() {
        let wd = i32::from_ne_bytes(buffer[at..at + 4].try_into().unwrap());
        let mask = u32::from_ne_bytes(buffer[at + 4..at + 8].try_into().unwrap());
        let _cookie = u32::from_ne_bytes(buffer[at + 8..at + 12].try_into().unwrap());
        let len = u32::from_ne_bytes(buffer[at + 12..at + 16].try_into().unwrap()) as usize;
        if at + 16 + len > buffer.len() {
            break;
        }
        let name_start = at + 16;
        let name = buffer[name_start..name_start + len]
            .iter()
            .take_while(|&&byte| byte != 0)
            .copied()
            .collect();
        events.push(InotifyEvent { wd, mask, name });
        at = name_start + len;
    }
    events
}

// Sleeps until a terminal does something, so the watcher need not poll, and
// remembers which terminals it was.
//
// A password prompt cannot appear silently: the program prints it. The kernel
// reports output on a pseudo-terminal, and new pseudo-terminals, through
// inotify, which is enough to know when and where a look at the terminal mode
// is due.
//
// Prompts are usually written through /dev/tty rather than the terminal's own
// device (sudo, ssh and getpass all do), and that output is reported against
// /dev/tty with no hint of which terminal it reached. It is watched too, and
// counts as output on every terminal.
pub struct TtyActivity {
    fd: Option<File>,
    directory_watch: i32,
    controlling_tty_watch: i32,
    watched: HashMap<i32, String>, // inotify watch -> path
    active: BTreeSet<String>,
}

impl TtyActivity {
    /// C++ `TtyActivity()`: watches /dev/pts, /dev/tty and every terminal
    /// that exists now. Without inotify nothing is watched and `wait` only
    /// sleeps.
    pub fn new() -> TtyActivity {
        // SAFETY: inotify_init1 takes flags and returns a new descriptor or -1.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return TtyActivity {
                fd: None,
                directory_watch: -1,
                controlling_tty_watch: -1,
                watched: HashMap::new(),
                active: BTreeSet::new(),
            };
        }
        // SAFETY: the descriptor is new, and we own it from here on.
        let mut this = TtyActivity {
            fd: Some(unsafe { File::from_raw_fd(fd) }),
            directory_watch: -1,
            controlling_tty_watch: -1,
            watched: HashMap::new(),
            active: BTreeSet::new(),
        };
        this.directory_watch = this.add_watch("/dev/pts", libc::IN_CREATE);
        this.controlling_tty_watch = this.add_watch("/dev/tty", libc::IN_MODIFY);
        if let Ok(directory) = fs::read_dir("/dev/pts") {
            for entry in directory.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    this.watch(&name);
                }
            }
        }
        this
    }

    /// False when inotify is unavailable; `wait` then only sleeps.
    pub fn available(&self) -> bool {
        self.fd.is_some()
    }

    /// Waits up to `timeout_ms` (forever when negative) and returns whether
    /// any terminal produced output or appeared. Returns early on a signal,
    /// and when `also_fd` becomes readable (the caller reads that one itself).
    pub fn wait(&mut self, timeout_ms: i32, also_fd: Option<RawFd>) -> bool {
        // A negative descriptor is ignored by poll(): with no inotify the call
        // degrades to a plain sleep and the caller falls back to polling.
        let fd = self.fd.as_ref().map_or(-1, |file| file.as_raw_fd());
        let mut descriptors = [
            libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: also_fd.unwrap_or(-1),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: two live pollfd, count matches, flags valid.
        let n = unsafe { libc::poll(descriptors.as_mut_ptr(), 2, timeout_ms) };
        if n <= 0 || (descriptors[0].revents & libc::POLLIN) == 0 {
            // n < 0 includes EINTR from a signal: return early so the loop
            // sees the stop flag, do not retry.
            return false;
        }
        self.drain()
    }

    /// Same answer for whatever has happened since the last call, without
    /// waiting.
    pub fn drain(&mut self) -> bool {
        let mut active = false;
        let mut buffer = [0u8; 4096];
        loop {
            let count = {
                let Some(mut file) = self.fd.as_ref() else {
                    return false;
                };
                // Ok(0) or any Err (WouldBlock = empty) stops the drain.
                match file.read(&mut buffer) {
                    Ok(count) if count > 0 => count,
                    _ => break,
                }
            };
            active = true;
            for event in parse_inotify_events(&buffer[..count]) {
                if (event.mask & libc::IN_Q_OVERFLOW) != 0 || event.wd == self.controlling_tty_watch
                {
                    // Events were lost, or the output went through /dev/tty:
                    // any terminal may have printed. Checked first: the
                    // overflow event has wd == -1.
                    for path in self.watched.values() {
                        self.active.insert(path.clone());
                    }
                } else if event.wd == self.directory_watch && (event.mask & libc::IN_CREATE) != 0 {
                    if let Ok(name) = std::str::from_utf8(&event.name) {
                        self.watch(name);
                    }
                } else if let Some(path) = self.watched.get(&event.wd) {
                    if (event.mask & libc::IN_IGNORED) != 0 {
                        // The terminal is gone and the kernel dropped its watch.
                        self.active.remove(path);
                        self.watched.remove(&event.wd);
                    } else {
                        self.active.insert(path.clone());
                    }
                }
            }
            if count < buffer.len() / 2 {
                break; // a short read means the queue is empty
            }
        }
        active
    }

    /// Paths of the terminals that printed or appeared since the last call.
    pub fn take_active(&mut self) -> BTreeSet<String> {
        std::mem::take(&mut self.active)
    }

    // inotify_add_watch for one path; errors are not reported.
    fn add_watch(&self, path: &str, mask: u32) -> i32 {
        let Some(fd) = self.fd.as_ref() else {
            return -1;
        };
        let path = match CString::new(path) {
            Ok(path) => path,
            Err(_) => return -1,
        };
        // SAFETY: path is NUL-terminated and outlives the call; the fd is open.
        unsafe { libc::inotify_add_watch(fd.as_raw_fd(), path.as_ptr(), mask) }
    }

    // C++ `TtyActivity::watch`: a new terminal counts as active.
    fn watch(&mut self, name: &str) {
        if name
            .bytes()
            .next()
            .is_none_or(|byte| !byte.is_ascii_digit())
        {
            return;
        }
        // Fails for other users' terminals, which cannot be inspected anyway.
        let path = format!("/dev/pts/{name}");
        let descriptor = self.add_watch(&path, libc::IN_MODIFY);
        if descriptor >= 0 {
            self.watched.insert(descriptor, path.clone());
            self.active.insert(path);
        }
    }
}

impl Default for TtyActivity {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(wd: i32, mask: u32, cookie: u32, name: &[u8]) -> Vec<u8> {
        let mut record = Vec::new();
        record.extend_from_slice(&wd.to_ne_bytes());
        record.extend_from_slice(&mask.to_ne_bytes());
        record.extend_from_slice(&cookie.to_ne_bytes());
        record.extend_from_slice(&(name.len() as u32).to_ne_bytes());
        record.extend_from_slice(name);
        record
    }

    #[test]
    fn two_records_are_parsed() {
        let mut buffer = record(3, libc::IN_MODIFY, 0, b"");
        buffer.extend_from_slice(&record(5, libc::IN_CREATE, 7, b"12"));
        let events = parse_inotify_events(&buffer);
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0],
            InotifyEvent {
                wd: 3,
                mask: libc::IN_MODIFY,
                name: vec![]
            }
        );
        assert_eq!(
            events[1],
            InotifyEvent {
                wd: 5,
                mask: libc::IN_CREATE,
                name: b"12".to_vec()
            }
        );
    }

    #[test]
    fn names_are_cut_at_the_first_nul() {
        // The kernel pads names to four bytes.
        let buffer = record(4, libc::IN_CREATE, 0, b"7\x00\x00\x00");
        let events = parse_inotify_events(&buffer);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InotifyEvent {
                wd: 4,
                mask: libc::IN_CREATE,
                name: b"7".to_vec()
            }
        );
    }

    #[test]
    fn a_cut_off_record_stops_the_parse() {
        let mut buffer = record(2, libc::IN_MODIFY, 0, b"");
        buffer.extend_from_slice(&record(9, libc::IN_DELETE, 0, b"33")[..10]);
        let events = parse_inotify_events(&buffer);
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0],
            InotifyEvent {
                wd: 2,
                mask: libc::IN_MODIFY,
                name: vec![]
            }
        );
    }
}
