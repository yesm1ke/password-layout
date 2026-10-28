//! A fake Hyprland request socket for tests and the parity driver (there is
//! no C++ counterpart). It answers statically: `set_layout` does not change
//! what `j/devices` says. Tests that need a changing compositor use
//! [`crate::compositor::FakeCompositor`].

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::hyprland::socket_path;

/// The static answers of a [`FakeHyprland`].
#[derive(Debug, Clone, Default)]
pub struct FakeReplies {
    pub devices: String,
    pub active_window: String,
}

/// A Hyprland request socket with the behaviour Hyprland's has from the
/// client's point of view: one request per connection, one answer.
pub struct FakeHyprland {
    path: PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl FakeHyprland {
    /// Bind the request socket of `runtime_dir`/`signature` and serve it in a
    /// thread named `fake-hyprland`.
    pub fn start(
        runtime_dir: &Path,
        signature: &str,
        replies: FakeReplies,
    ) -> io::Result<FakeHyprland> {
        let path = socket_path(runtime_dir, signature, ".socket.sock");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _ = std::fs::remove_file(&path); // a stale socket file
        let listener = UnixListener::bind(&path)?;
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_requests = Arc::clone(&requests);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("fake-hyprland".into())
            .spawn(move || {
                loop {
                    if thread_stop.load(Ordering::Relaxed) {
                        return;
                    }
                    let stream = match listener.incoming().next() {
                        Some(Ok(stream)) => stream,
                        _ => continue,
                    };
                    serve(stream, &replies, &thread_requests);
                }
            })?;
        Ok(FakeHyprland {
            path,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    /// The requests received so far, in arrival order.
    pub fn requests(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

fn serve(stream: UnixStream, replies: &FakeReplies, requests: &Arc<Mutex<Vec<String>>>) {
    let mut stream = stream;
    if stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .is_err()
    {
        return;
    }
    let mut buffer = [0u8; 8192];
    // One request per connection, read once.
    let count = match stream.read(&mut buffer) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let request = String::from_utf8_lossy(&buffer[..count]).into_owned();
    let reply = if request == "j/devices" {
        &replies.devices
    } else if request == "j/activewindow" {
        &replies.active_window
    } else if request.starts_with("/switchxkblayout ") {
        "ok"
    } else {
        "unknown request"
    };
    requests
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(request);
    let _ = stream.write_all(reply.as_bytes());
}

impl Drop for FakeHyprland {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // One connection wakes accept, which then sees the stop flag. If the
        // socket is gone (its directory removed first), accept never wakes:
        // leave the thread blocked rather than hang the test in join.
        let woken = UnixStream::connect(&self.path).is_ok();
        if let Some(thread) = self.thread.take()
            && woken
        {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}
