//! Test helpers, ported from `tests/test.cpp` (`PtyChild`, `Deadline`,
//! `terminalOf`).
#![allow(dead_code)] // each test file uses only some of the helpers.

use std::ffi::CString;
use std::time::{Duration, Instant};

use password_layout::tty::read_processes;

/// Runs a command on a fresh pseudo-terminal and kills it when done
/// (C++ `PtyChild`). forkpty gives the child a controlling terminal, which
/// `/proc/<pid>/stat` `tty_nr` and `> /dev/tty` need.
pub struct PtyChild {
    pid: libc::pid_t,
    master: libc::c_int,
}

impl PtyChild {
    pub fn new(argv: &[&str]) -> PtyChild {
        // Build the argument list before forking: the child must run only
        // execvp/_exit, with no allocation and no panic (the test process
        // has other threads).
        let args: Vec<CString> = argv
            .iter()
            .map(|a| CString::new(*a).expect("test argument has no NUL"))
            .collect();
        let mut ptrs: Vec<*const libc::c_char> =
            args.iter().map(|a| a.as_c_str().as_ptr()).collect();
        ptrs.push(std::ptr::null());

        let mut master: libc::c_int = -1;
        // SAFETY: master is a live c_int, the rest are valid null pointers
        // as documented by forkpty.
        let pid = unsafe {
            libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert!(pid >= 0, "forkpty failed");
        if pid == 0 {
            // Child: nothing else may run here.
            // SAFETY: ptrs is NUL-terminated and outlives the exec.
            unsafe { libc::execvp(ptrs[0], ptrs.as_ptr()) };
            // SAFETY: _exit is always valid.
            unsafe { libc::_exit(127) };
        }
        // Let the child reach its prompt before a test looks.
        std::thread::sleep(Duration::from_millis(500));
        PtyChild { pid, master }
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }
}

impl Drop for PtyChild {
    fn drop(&mut self) {
        // SAFETY: kill on any pid is a valid call; the pid is our child.
        unsafe { libc::kill(self.pid, libc::SIGKILL) };
        // SAFETY: waitpid with a null status for our own child is valid.
        unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), 0) };
        // SAFETY: master is the fd forkpty gave us, closed exactly once.
        unsafe { libc::close(self.master) };
    }
}

/// The child's terminal path, e.g. `/dev/pts/3` (C++ `terminalOf`).
pub fn terminal_of(child: &PtyChild) -> String {
    let processes = read_processes();
    let process = processes
        .get(&child.pid())
        .unwrap_or_else(|| panic!("pid {} is not in /proc", child.pid()));
    format!("/dev/pts/{}", libc::minor(process.tty))
}

/// A private tmux server for tests (C++ `TmuxServer`). A socket name that is
/// not the C++ test's, so both suites can run at once.
pub struct TmuxServer;

impl TmuxServer {
    pub const NAME: &str = "password-layout-rust-test";

    /// Starts clean: a stale server from a crashed run is killed first.
    pub fn new() -> TmuxServer {
        Self::tmux(&["kill-server"]);
        TmuxServer
    }

    /// Runs tmux on the private socket (C++ `TmuxServer::tmux`).
    pub fn tmux(args: &[&str]) -> Option<String> {
        let mut argv: Vec<&str> = vec!["tmux", "-L", Self::NAME];
        argv.extend_from_slice(args);
        password_layout::tmux::run_command(&argv, 3000)
    }
}

impl Default for TmuxServer {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        Self::tmux(&["kill-server"]);
    }
}

/// Waits are bounded by time, not by a number of attempts: other terminals
/// on the machine print too, and each of their events ends a wait early.
pub struct Deadline {
    end: Instant,
}

impl Deadline {
    pub fn new(ms: u64) -> Deadline {
        Deadline {
            end: Instant::now() + Duration::from_millis(ms),
        }
    }

    pub fn passed(&self) -> bool {
        Instant::now() >= self.end
    }
}
