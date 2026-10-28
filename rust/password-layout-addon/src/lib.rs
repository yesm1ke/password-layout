//! The Rust side of the fcitx5 addon: the worker thread that applies the
//! latest "Latin wanted" wish through Hyprland and the shared state, behind a
//! C interface for the C++ shim (shim/passwordlayout.cpp). It never sees
//! fcitx5 events, only the wishes. See the wiki page "Rust rewrite".

pub mod ffi;
pub mod worker;

use std::io::{self, Write};

use password_layout_core::hyprland::Hyprland;
use password_layout_core::state::State;

/// The holder name the addon uses in the shared state (C++ `kHolder`).
pub const HOLDER: &str = "im";

/// Apply one wish: enter the shared state under the addon's holder when a
/// password field is focused, leave it when it is not. Runs on the worker
/// thread only.
///
/// Errors are written to stderr and dropped: this runs inside fcitx5, where
/// a failed switch must not kill the process.
pub fn apply(latin: bool) {
    let result = (|| {
        let mut compositor = Hyprland::from_env()?;
        let state = State::from_env()?;
        if latin {
            state.enter(HOLDER, &mut compositor)
        } else {
            state.leave(HOLDER, &mut compositor)
        }
    })();
    if let Err(error) = result {
        // Not eprintln!: it panics if stderr is closed, and this runs
        // inside fcitx5. Note the prefix "passwordlayout:" (the addon's,
        // no hyphen).
        let _ = writeln!(io::stderr(), "passwordlayout: {error}");
    }
}
