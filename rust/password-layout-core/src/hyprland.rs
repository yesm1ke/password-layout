//! Hyprland's socket client and JSON-derived helpers, ported from
//! `src/compositor.cpp`. std only: `UnixStream` with read and write timeouts
//! (the C++ `SO_RCVTIMEO`/`SO_SNDTIMEO`); std sends with `MSG_NOSIGNAL`, so a
//! compositor that closes the socket first cannot kill the process with
//! SIGPIPE.

use std::collections::HashMap;
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::compositor::{Compositor, Pid};
use crate::json;

/// A hung compositor must not hang the service, nor fcitx5's exit, which
/// waits for the addon's worker. Per read and per write, as in C++.
pub const TIMEOUT: Duration = Duration::from_secs(1);
/// The message of a request that timed out.
pub const SILENT: &str = "Hyprland did not answer within a second";

/// What most keyboards have: the layout with the most votes (not the first
/// one listed, which may be a virtual or ACPI device, and not the one
/// Hyprland marks "main"), ported from `keyboardLayouts`. With no layouts at
/// all, two empty strings.
pub fn keyboard_layouts(devices: &str) -> (String, String) {
    let layouts = json::json_strings(devices, "layout");
    let variants = json::json_strings(devices, "variant");
    if layouts.is_empty() {
        return (String::new(), String::new());
    }
    let mut counts: HashMap<&str, i32> = HashMap::new();
    let mut best = 0;
    for (index, layout) in layouts.iter().enumerate() {
        *counts.entry(layout.as_str()).or_insert(0) += 1;
        // Read after the increment, as the C++ `++counts[a] > counts[b]` does
        // under GCC: [a/x, a/y, b/z] gives (a, x), [b/z, a/x, a/y] gives
        // (a, y).
        if counts[layout.as_str()] > counts[layouts[best].as_str()] {
            best = index;
        }
    }
    // Every keyboard entry carries both keys, so the lists line up.
    let variant = if variants.len() == layouts.len() {
        variants[best].clone()
    } else {
        String::new()
    };
    (layouts[best].clone(), variant)
}

/// The value that occurs most often, or `fallback` when nothing does; ties go
/// to the value that reached the count first. Ported from `mostCommon`.
pub fn most_common(values: &[i64], fallback: i32) -> i32 {
    let mut counts: HashMap<i64, i32> = HashMap::new();
    let mut best = fallback;
    let mut best_count = 0;
    for value in values {
        *counts.entry(*value).or_insert(0) += 1;
        if counts[value] > best_count {
            best_count = counts[value];
            best = *value as i32;
        }
    }
    best
}

/// `{runtime_dir}/hypr/{signature}/{name}`, Hyprland's socket convention.
pub fn socket_path(runtime_dir: &Path, signature: &str, name: &str) -> PathBuf {
    runtime_dir.join("hypr").join(signature).join(name)
}

fn require_env(name: &str) -> io::Result<std::ffi::OsString> {
    match std::env::var_os(name) {
        Some(value) if !value.is_empty() => Ok(value),
        _ => Err(io::Error::other(format!("{name} is not set"))),
    }
}

fn context(what: &str, e: io::Error) -> io::Error {
    // The C++ side throws std::system_error, whose text is "what: strerror";
    // Rust appends " (os error N)" to raw errors, and doctor prints this
    // message, so drop the suffix.
    let detail = e
        .to_string()
        .rsplit_once(" (os error ")
        .map_or_else(|| e.to_string(), |(head, _)| head.to_string());
    io::Error::new(e.kind(), format!("{what}: {detail}"))
}

#[cfg(test)]
mod context_tests {
    use super::context;

    #[test]
    fn drops_the_os_error_suffix_like_c_strerror_does() {
        let missing = std::io::Error::from_raw_os_error(2); // ENOENT
        assert_eq!(
            context("connect to Hyprland", missing).to_string(),
            "connect to Hyprland: No such file or directory"
        );
    }
}

/// Hyprland's event socket, to notice layout switches as they happen.
pub trait LayoutEvents {
    /// To wait on; `None` while not connected.
    fn fd(&self) -> Option<RawFd>;
    /// Reads what has arrived without waiting; whether the active layout
    /// changed.
    fn layout_changed(&mut self) -> bool;
}

#[derive(Debug, Default)]
pub struct NoLayoutEvents;

impl LayoutEvents for NoLayoutEvents {
    fn fd(&self) -> Option<RawFd> {
        None
    }

    fn layout_changed(&mut self) -> bool {
        false
    }
}

/// Talks to Hyprland over its request socket. Each request is a fresh
/// connection, closed before returning.
#[derive(Debug, Clone)]
pub struct Hyprland {
    socket: PathBuf,
}

impl Hyprland {
    /// The request socket of a Hyprland instance; nothing is checked until
    /// the first request.
    pub fn new(runtime_dir: &Path, signature: &str) -> Hyprland {
        Hyprland {
            socket: socket_path(runtime_dir, signature, ".socket.sock"),
        }
    }

    /// From `XDG_RUNTIME_DIR` and `HYPRLAND_INSTANCE_SIGNATURE`, the first
    /// checked first (the message order of C++). Does not connect.
    pub fn from_env() -> io::Result<Hyprland> {
        let runtime = require_env("XDG_RUNTIME_DIR")?;
        let signature = require_env("HYPRLAND_INSTANCE_SIGNATURE")?;
        Ok(Hyprland::new(
            Path::new(&runtime),
            &signature.to_string_lossy(),
        ))
    }

    pub fn request(&self, command: &str) -> io::Result<String> {
        let mut stream =
            UnixStream::connect(&self.socket).map_err(|e| context("connect to Hyprland", e))?;
        // A non-zero duration cannot fail, but keep the error path anyway.
        stream
            .set_read_timeout(Some(TIMEOUT))
            .map_err(|e| context("connect to Hyprland", e))?;
        stream
            .set_write_timeout(Some(TIMEOUT))
            .map_err(|e| context("connect to Hyprland", e))?;
        stream
            .write_all(command.as_bytes())
            .map_err(|e| context("write to Hyprland", e))?;
        // Hyprland reads one request per connection; do not shut down the
        // write side. The timeout is per read, as in C++ (a compositor that
        // trickles bytes can take longer; accepted).
        let mut reply = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => reply.extend_from_slice(&buffer[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::TimedOut => {
                    // The timeout (Linux gives EAGAIN): the compositor is silent.
                    return Err(io::Error::new(ErrorKind::TimedOut, SILENT));
                }
                // Any other error ends the request (a signal ends it in C++
                // too: the loop then sees the stop flag).
                Err(e) => return Err(context("read from Hyprland", e)),
            }
        }
        Ok(String::from_utf8_lossy(&reply).into_owned())
    }

    /// xkb's comma-separated layout list, e.g. "us,ru"; for messages.
    pub fn layout_list(&mut self) -> io::Result<String> {
        Ok(keyboard_layouts(&self.request("j/devices")?).0)
    }
}

impl Compositor for Hyprland {
    fn layout_index(&mut self) -> io::Result<i32> {
        // Keyboards are switched together, but a stray device can drift (an
        // ACPI button left on another layout), so go with what most of them
        // agree on.
        let reply = self.request("j/devices")?;
        Ok(most_common(
            &json::json_integers(&reply, "active_layout_index"),
            0,
        ))
    }

    fn latin_layouts(&mut self) -> io::Result<Vec<bool>> {
        let (layouts, variants) = keyboard_layouts(&self.request("j/devices")?);
        Ok(if layouts.is_empty() {
            vec![]
        } else {
            crate::latin::latin_layouts(&layouts, &variants)
        })
    }

    fn set_layout(&mut self, index: i32) -> io::Result<()> {
        // "all", never "current": with fcitx5 running, "current" resolves to
        // whichever device Hyprland saw last, which is often not a keyboard.
        // The reply is ignored; the exact bytes, e.g. "/switchxkblayout all 0".
        self.request(&format!("/switchxkblayout all {index}"))?;
        Ok(())
    }

    fn focused_pid(&mut self) -> io::Result<Pid> {
        // No window focused: Hyprland answers `{}`.
        let pids = json::json_integers(&self.request("j/activewindow")?, "pid");
        Ok(pids.first().copied().unwrap_or(0) as Pid)
    }
}

/// Hyprland's event socket, to notice layout switches as they happen.
#[derive(Debug)]
pub struct HyprlandEvents {
    socket: PathBuf,
    stream: Option<UnixStream>,
    partial: Vec<u8>,
}

impl HyprlandEvents {
    /// The event socket of a Hyprland instance; connects.
    pub fn new(runtime_dir: &Path, signature: &str) -> HyprlandEvents {
        let mut events = HyprlandEvents {
            socket: socket_path(runtime_dir, signature, ".socket2.sock"),
            stream: None,
            partial: Vec::new(),
        };
        events.connect();
        events
    }

    /// From the environment, the same two checks in the same order as
    /// [`Hyprland::from_env`].
    pub fn from_env() -> io::Result<HyprlandEvents> {
        let runtime = require_env("XDG_RUNTIME_DIR")?;
        let signature = require_env("HYPRLAND_INSTANCE_SIGNATURE")?;
        Ok(HyprlandEvents::new(
            Path::new(&runtime),
            &signature.to_string_lossy(),
        ))
    }

    fn connect(&mut self) {
        self.stream = match UnixStream::connect(&self.socket) {
            Ok(stream) => match stream.set_nonblocking(true) {
                Ok(()) => Some(stream),
                Err(_) => None,
            },
            // A failed connect is not an error; try again next time.
            Err(_) => None,
        };
        self.partial.clear();
    }
}

impl LayoutEvents for HyprlandEvents {
    fn fd(&self) -> Option<RawFd> {
        self.stream.as_ref().map(|s| s.as_raw_fd())
    }

    fn layout_changed(&mut self) -> bool {
        if self.stream.is_none() {
            self.connect();
            // Whatever happened while disconnected is unknown; assume a change.
            return self.stream.is_some();
        }
        let mut changed = false;
        let mut buffer = [0u8; 4096];
        loop {
            match self
                .stream
                .as_mut()
                .expect("checked above")
                .read(&mut buffer)
            {
                Ok(0) => {
                    // The compositor went away; try again next time.
                    self.stream = None;
                    break;
                }
                Ok(n) => {
                    self.partial.extend_from_slice(&buffer[..n]);
                }
                Err(e)
                    if e.kind() == ErrorKind::WouldBlock || e.kind() == ErrorKind::Interrupted =>
                {
                    break;
                }
                Err(_) => {
                    self.stream = None;
                    break;
                }
            }
        }
        // A line without "\n" yet stays in `partial` for the next call.
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            changed = changed || line.starts_with(b"activelayout>>");
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn most_common_prefers_the_first_to_reach_the_count() {
        assert_eq!(most_common(&[], 7), 7);
        assert_eq!(most_common(&[1, 0, 1], 0), 1);
        // A tie goes to the value that reached the count first.
        assert_eq!(most_common(&[1, 2], 0), 1);
        assert_eq!(most_common(&[5], 0), 5);
    }
}
