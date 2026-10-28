//! The command line: `watch-tty`, `status` and the dispatch from `main`.
//! Port of `src/main.cpp` (`watchTty`, `printStatus`, `usage`, `main`);
//! `cli::run` is the only place that builds the Hyprland client.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use password_layout_core::compositor::{Compositor, Pid};
use password_layout_core::hyprland::{Hyprland, HyprlandEvents, LayoutEvents, NoLayoutEvents};
use password_layout_core::pace::{Pace, next_pace};
use password_layout_core::state::State;

use crate::activity::TtyActivity;
use crate::doctor;
use crate::tmux::Tmux;
use crate::tty::{self, Device};
use crate::watch::PromptWatcher;

/// How often to look while a prompt is up or terminals keep printing. Focus
/// can move and a prompt can end without any output to announce it.
pub const POLL_MS: u64 = 200;
/// Delay before the second look after terminals wake the watcher from quiet.
pub const SETTLE_MS: u64 = 50;
/// The holder name terminal prompts take in the state file.
pub const TTY_HOLDER: &str = "tty";
/// The error message [`Unavailable`] answers every call with.
pub const NOT_AVAILABLE: &str = "Hyprland is not supported by the Rust build yet (stage 3)";

/// A compositor that always fails; for tests.
pub struct Unavailable;

impl Compositor for Unavailable {
    fn layout_index(&mut self) -> io::Result<i32> {
        Err(io::Error::other(NOT_AVAILABLE))
    }

    fn latin_layouts(&mut self) -> io::Result<Vec<bool>> {
        Err(io::Error::other(NOT_AVAILABLE))
    }

    fn set_layout(&mut self, _index: i32) -> io::Result<()> {
        Err(io::Error::other(NOT_AVAILABLE))
    }

    fn focused_pid(&mut self) -> io::Result<Pid> {
        Err(io::Error::other(NOT_AVAILABLE))
    }
}

/// C++ `watchTty` without a source of layout events: the stage-2 form,
/// kept for tests.
pub fn watch_tty(
    compositor: &mut dyn Compositor,
    state: &State,
    stop: &AtomicBool,
) -> io::Result<()> {
    watch_tty_with(compositor, &mut NoLayoutEvents, state, stop)
}

/// C++ `watchTty`: the service loop, following layout switches through
/// `events` (Hyprland's event socket) and waiting on its fd.
pub fn watch_tty_with(
    compositor: &mut dyn Compositor,
    events: &mut dyn LayoutEvents,
    state: &State,
    stop: &AtomicBool,
) -> io::Result<()> {
    // A previous run that died mid-prompt must not leave Latin held forever.
    state.leave(TTY_HOLDER, compositor)?;
    let mut activity = TtyActivity::new();
    let mut watcher = PromptWatcher::new();
    // Layout switches are followed as they happen, so that a password gets
    // the Latin layout that was last in use - macOS does the same.
    if let Err(e) = state.note_layout(compositor) {
        eprintln!("password-layout: {e}");
    }
    let mut held = false;
    // Terminals printed recently, so keep looking at a steady pace instead of
    // waking for every write.
    let mut busy = true;
    // Programs print the prompt and turn echo off in either order, so the look
    // taken right on waking is followed by a second one a moment later.
    let mut second_look_owed = false;
    // A terminal's mode only needs checking after it printed something, so
    // each look covers the terminals that printed lately plus those at a
    // prompt.
    let mut suspects = activity.take_active();
    let mut printed_before: BTreeSet<String> = BTreeSet::new();
    while !stop.load(Ordering::Relaxed) {
        // Layout switches are followed as they happen, so that a password
        // gets the Latin layout that was last in use - macOS does the same.
        // The stage-3 design fixes this nested form (the try/catch of C++).
        #[allow(clippy::collapsible_if)]
        if events.layout_changed() {
            if let Err(e) = state.note_layout(compositor) {
                eprintln!("password-layout: {e}");
            }
        }
        let mut pending = true;
        let at_prompt;
        match one_look(
            compositor,
            state,
            &mut watcher,
            &suspects,
            activity.available(),
            &mut held,
        ) {
            Ok((prompt_pending, prompt_paths)) => {
                pending = prompt_pending;
                at_prompt = prompt_paths;
            }
            Err(e) => {
                // The compositor restarting drops the socket for a moment;
                // keep polling until it answers again.
                eprintln!("password-layout: {e}");
                at_prompt = suspects.clone();
            }
        }

        let pace = next_pace(second_look_owed, pending, busy, activity.available());
        second_look_owed = false;
        match pace {
            Pace::SecondLook => {
                // Same terminals again.
                std::thread::sleep(Duration::from_millis(SETTLE_MS));
                continue;
            }
            Pace::Poll => {
                std::thread::sleep(Duration::from_millis(POLL_MS));
                busy = activity.drain() || pending;
            }
            Pace::Sleep => {
                // Also wake on Hyprland layout events; `wait` returns false
                // when only the event socket is readable, so no second look
                // is owed (the next round reads the events). As in C++.
                busy = activity.wait(-1, events.fd());
                second_look_owed = busy;
            }
        }
        // A terminal stays a suspect for one more look after it printed, in
        // case that look fell between the prompt and echo being turned off.
        let printed = activity.take_active();
        let mut next_suspects = at_prompt;
        next_suspects.extend(printed.iter().cloned());
        next_suspects.extend(printed_before.iter().cloned());
        printed_before = printed;
        suspects = next_suspects;
    }
    if held {
        state.leave(TTY_HOLDER, compositor)?;
    }
    Ok(())
}

/// The body of the `try` block in C++ `watchTty`: one look at the terminals,
/// and the layout switch when the prompt crosses into or out of the focused
/// window. Fills `at_prompt` (terminals at a prompt) and `pending` (a prompt
/// is up somewhere) and flips `held` on a change.
fn one_look(
    compositor: &mut dyn Compositor,
    state: &State,
    watcher: &mut PromptWatcher,
    suspects: &BTreeSet<String>,
    can_wait: bool,
    held: &mut bool,
) -> io::Result<(bool, BTreeSet<String>)> {
    let mut at_prompt: BTreeSet<String> = BTreeSet::new();
    let devices = if can_wait {
        let mut devices: BTreeSet<Device> = BTreeSet::new();
        for (path, device) in tty::password_ptys_among(suspects) {
            at_prompt.insert(path);
            devices.insert(device);
        }
        devices
    } else {
        tty::password_ptys(tty::current_uid())
    };
    let prompt = watcher.look(compositor, devices, suspects)?;
    if prompt.in_focus != *held {
        if prompt.in_focus {
            state.enter(TTY_HOLDER, compositor)?;
        } else {
            state.leave(TTY_HOLDER, compositor)?;
        }
        *held = prompt.in_focus;
    }
    Ok((prompt.pending, at_prompt))
}

/// C++ `printStatus`.
pub fn print_status(
    compositor: &mut dyn Compositor,
    state: &State,
    out: &mut dyn Write,
) -> io::Result<()> {
    let devices = tty::password_ptys(tty::current_uid());
    let focused = compositor.focused_pid()?;
    let holders = state.holders()?;
    let index = compositor.layout_index()?;
    let last = state.last_latin();
    let in_focus = prompt_in_focus(focused, &devices);
    // Everything above may fail; an error writes nothing.
    let holders = if holders.is_empty() {
        "none".to_string()
    } else {
        holders.join(", ")
    };
    let prompts: Vec<String> = devices
        .iter()
        .map(|device| format!("pts/{}", libc::minor(*device)))
        .collect();
    let prompts = if prompts.is_empty() {
        "none".to_string()
    } else {
        prompts.join(", ")
    };
    writeln!(out, "layout index:        {index}")?;
    writeln!(
        out,
        "last Latin layout:   {}",
        last.map_or("none seen".to_string(), |index| index.to_string())
    )?;
    writeln!(out, "holders:             {holders}")?;
    writeln!(out, "password prompts:    {prompts}")?;
    writeln!(out, "focused window pid:  {focused}")?;
    writeln!(
        out,
        "prompt in focus:     {}",
        if in_focus { "yes" } else { "no" }
    )?;
    Ok(())
}

/// Whether a prompt is in the focused window: C++ `printStatus`'s last line.
fn prompt_in_focus(focused: Pid, devices: &BTreeSet<Device>) -> bool {
    let processes = tty::read_processes();
    let mut tmux = Tmux::new();
    let mut tmux_client_terminals: BTreeSet<String> = BTreeSet::new();
    crate::watch::prompt_in_window(
        focused,
        devices,
        &processes,
        &mut tmux,
        &mut tmux_client_terminals,
    )
}

/// C++ `usage`: writes the usage text to stderr.
fn usage() -> i32 {
    // 5 lines, exactly the C++ usage text (continuation lines at column 8).
    eprintln!("usage: password-layout enter <holder>   hold Latin (from your own scripts)");
    eprintln!("       password-layout leave <holder>   give it back");
    eprintln!("       password-layout watch-tty        the terminal service");
    eprintln!("       password-layout status           layout, holders, prompts");
    eprintln!("       password-layout doctor           check the setup");
    2
}

/// C++ `main`. `args[0]` is the program name; returns the exit code.
pub fn run(args: &[String], stop: &AtomicBool) -> i32 {
    let command = args.get(1).map_or("", String::as_str);
    let takes_holder = command == "enter" || command == "leave";
    if args.len() != if takes_holder { 3 } else { 2 }
        || !(takes_holder || command == "watch-tty" || command == "status" || command == "doctor")
    {
        return usage();
    }
    // Before anything that needs Hyprland: finding out why it is missing is
    // part of the job.
    if command == "doctor" {
        return doctor::print_findings(&doctor::diagnose(), &mut io::stdout());
    }
    // C++ order: the compositor before the state, so with neither variable
    // set the message is about XDG_RUNTIME_DIR.
    let mut compositor = match Hyprland::from_env() {
        Ok(compositor) => compositor,
        Err(e) => {
            eprintln!("password-layout: {e}");
            return 1;
        }
    };
    let state = match State::from_env() {
        Ok(state) => state,
        Err(e) => {
            eprintln!("password-layout: {e}");
            return 1;
        }
    };
    let result = if command == "enter" {
        state.enter(&args[2], &mut compositor)
    } else if command == "leave" {
        state.leave(&args[2], &mut compositor)
    } else if command == "watch-tty" {
        let mut events = match HyprlandEvents::from_env() {
            Ok(events) => events,
            Err(e) => {
                eprintln!("password-layout: {e}");
                return 1;
            }
        };
        watch_tty_with(&mut compositor, &mut events, &state, stop)
    } else {
        print_status(&mut compositor, &state, &mut io::stdout())
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("password-layout: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn wrong_arguments_and_unknown_commands_get_usage() {
        let stop = AtomicBool::new(false);
        assert_eq!(run(&[], &stop), 2);
        assert_eq!(run(&args(&["password-layout"]), &stop), 2);
        assert_eq!(run(&args(&["password-layout", "enter"]), &stop), 2);
        assert_eq!(
            run(&args(&["password-layout", "enter", "x", "y"]), &stop),
            2
        );
        assert_eq!(
            run(&args(&["password-layout", "watch-tty", "extra"]), &stop),
            2
        );
        assert_eq!(run(&args(&["password-layout", "frobnicate"]), &stop), 2);
    }
}
