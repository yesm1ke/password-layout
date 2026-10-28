//! Prompt detection: is a password prompt up, and is it in the focused
//! window? Port of `src/watch.cpp`; the pacing (`nextPace`) stays in
//! `password_layout_core::pace`.

use std::collections::BTreeSet;
use std::io;

use password_layout_core::compositor::{Compositor, Pid};

use crate::tmux::Tmux;
use crate::tty::{Device, Processes, read_processes, window_owns_tty};

/// C++ `Prompt`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Prompt {
    pub pending: bool,
    pub in_focus: bool,
}

/// C++ `promptInWindow`.
pub fn prompt_in_window(
    window: Pid,
    devices: &BTreeSet<Device>,
    processes: &Processes,
    tmux: &mut Tmux,
    tmux_client_terminals: &mut BTreeSet<String>,
) -> bool {
    if window_owns_tty(window, devices, processes) {
        // The focused window owns the prompt's terminal: tmux is not asked.
        return true;
    }
    let mut visible = false;
    for &device in devices {
        if let Some(answer) = tmux.ask(device, window, processes) {
            tmux_client_terminals.extend(answer.client_terminals);
            visible |= answer.visible_in_window;
        }
    }
    visible
}

/// C++ `PromptWatcher`.
#[derive(Debug, Default)]
pub struct PromptWatcher {
    devices: BTreeSet<Device>,
    focused: Pid,
    owned: bool,
    tmux: Tmux,
    tmux_client_terminals: BTreeSet<String>,
}

impl PromptWatcher {
    pub fn new() -> PromptWatcher {
        PromptWatcher::default()
    }

    pub fn look(
        &mut self,
        compositor: &mut dyn Compositor,
        devices: BTreeSet<Device>,
        printed: &BTreeSet<String>,
    ) -> io::Result<Prompt> {
        // The compositor is only asked while a prompt is pending.
        if devices.is_empty() {
            self.devices.clear();
            self.tmux_client_terminals.clear();
            return Ok(Prompt::default());
        }
        let focused = compositor.focused_pid()?;
        // Walking the whole process table and asking tmux are the expensive
        // parts, so they are only redone when the prompts or the focused
        // window change, or when a tmux client redrew - which is what
        // switching panes or windows inside tmux looks like from outside.
        let tmux_redrew = printed
            .iter()
            .any(|path| self.tmux_client_terminals.contains(path));
        if devices != self.devices || focused != self.focused || tmux_redrew {
            self.tmux_client_terminals.clear();
            self.owned = prompt_in_window(
                focused,
                &devices,
                &read_processes(),
                &mut self.tmux,
                &mut self.tmux_client_terminals,
            );
            self.devices = devices;
            self.focused = focused;
        }
        Ok(Prompt {
            pending: true,
            in_focus: self.owned,
        })
    }
}
