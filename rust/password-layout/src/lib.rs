//! Terminal side of password-layout: prompt detection, terminal activity,
//! tmux, the prompt watcher and the command line. Linux only.

pub mod activity;
pub mod cli;
pub mod doctor;
pub mod tmux;
pub mod tty;
pub mod watch;
