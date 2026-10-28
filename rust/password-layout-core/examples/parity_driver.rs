//! The Rust side of the parity check (stage 1c). Same command line and same
//! output as rust/parity/parity_driver.cpp:
//!
//! ```text
//! parity_driver classify
//!     stdin: lines "LAYOUTS<TAB>VARIANTS" (VARIANTS may be empty)
//!     stdout: per line the result of latin_layouts as 0/1 characters, e.g. "10"
//! parity_driver run DIR INDEX LATIN OP...
//!     DIR exists; INDEX is the starting layout; LATIN like "10"
//!     (empty string = no layouts)
//!     OP is one of: enter:NAME  leave:NAME  note  set:N (user switches by
//!     hand)  latin:BITS (config changed)
//!     after every OP print one line: "OP ok index=I switches=A,B" or
//!     "OP error" (switches = all set_layout calls so far, comma-separated,
//!     empty when none)
//! parity_driver json KEY...
//!     stdin: read in whole, as bytes; for each KEY one line "int KEY" and
//!     one line " V" per value of json_integers, one line "str KEY" and one
//!     line " [S]" per value of json_strings (raw bytes); then the line
//!     "keyboard [LAYOUTS] [VARIANTS]" (keyboard_layouts of the whole input)
//!     and "most-common N", N = most_common(json_integers(input, first
//!     KEY), -1)
//! parity_driver hyprland RUNTIME SIGNATURE OP...
//!     RUNTIME is a runtime directory; SIGNATURE a Hyprland instance
//!     signature; together they name the request socket to talk to (as in
//!     `serve`). OP is one of: index  latin  focused  list  set:N
//!     after every OP print one line: "OP VALUE" (latin as 0/1 characters,
//!     set:N "ok") or "OP error"
//! parity_driver serve RUNTIME SIGNATURE DEVICES_FILE WINDOW_FILE  (Rust
//!     only; there is no C++ fake)
//!     serve a fake Hyprland at RUNTIME/SIGNATURE, answering j/devices with
//!     DEVICES_FILE and j/activewindow with WINDOW_FILE; read stdin until
//!     EOF (rust/parity/run.sh holds a fifo open until the client is done),
//!     then print the requests received, one per line
//! ```

use password_layout_core::compositor::{Compositor, FakeCompositor};
use password_layout_core::fake_hyprland::{FakeHyprland, FakeReplies};
use password_layout_core::hyprland::{Hyprland, keyboard_layouts, most_common};
use password_layout_core::json::{json_integers, json_strings};
use password_layout_core::latin::latin_layouts;
use password_layout_core::state::State;

use std::io::{BufRead, Read, Write};
use std::path::Path;

fn bits(text: &str) -> Vec<bool> {
    text.bytes().map(|c| c == b'1').collect()
}

fn classify() -> Result<(), std::io::Error> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        let (layouts, variants) = match line.find('\t') {
            Some(tab) => (&line[..tab], &line[tab + 1..]),
            None => (line.as_str(), ""),
        };
        let mut row = String::new();
        for is_latin in latin_layouts(layouts, variants) {
            row.push(if is_latin { '1' } else { '0' });
        }
        row.push('\n');
        stdout.write_all(row.as_bytes())?;
    }
    Ok(())
}

fn run(
    directory: &str,
    start: i32,
    latin: &str,
    operations: &[String],
) -> Result<(), std::io::Error> {
    let mut compositor = FakeCompositor::new(start, bits(latin));
    let state = State::new(directory);
    let mut stdout = std::io::stdout();
    for operation in operations {
        let ok = match operation.strip_prefix("enter:") {
            Some(holder) => state.enter(holder, &mut compositor).is_ok(),
            None => match operation.strip_prefix("leave:") {
                Some(holder) => state.leave(holder, &mut compositor).is_ok(),
                None => match operation.strip_prefix("set:") {
                    Some(value) => match value.parse::<i32>() {
                        Ok(index) => {
                            compositor.index = index;
                            true
                        }
                        Err(_) => false,
                    },
                    None => match operation.strip_prefix("latin:") {
                        Some(value) => {
                            compositor.latin = bits(value);
                            true
                        }
                        None => operation == "note" && state.note_layout(&mut compositor).is_ok(),
                    },
                },
            },
        };
        if !ok {
            writeln!(stdout, "{operation} error")?;
            continue;
        }
        let switches: Vec<String> = compositor
            .switches
            .iter()
            .map(ToString::to_string)
            .collect();
        let switches = switches.join(",");
        writeln!(
            stdout,
            "{operation} ok index={} switches={switches}",
            compositor.index
        )?;
    }
    Ok(())
}

fn json(keys: &[String]) -> Result<(), std::io::Error> {
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    let input = String::from_utf8_lossy(&input);
    let mut stdout = std::io::stdout();
    for key in keys {
        writeln!(stdout, "int {key}")?;
        for value in json_integers(&input, key) {
            writeln!(stdout, " {value}")?;
        }
        writeln!(stdout, "str {key}")?;
        for value in json_strings(&input, key) {
            stdout.write_all(value.as_bytes())?;
            stdout.write_all(b"\n")?;
        }
    }
    let (layouts, variants) = keyboard_layouts(&input);
    writeln!(stdout, "keyboard {layouts} {variants}")?;
    let most = most_common(&json_integers(&input, &keys[0]), -1);
    writeln!(stdout, "most-common {most}")?;
    Ok(())
}

/// One Hyprland client, a fresh request per OP; a failed request prints
/// "OP error" and the next OP goes on, like the C++ driver's catch.
fn hyprland(runtime: &str, signature: &str, operations: &[String]) {
    let mut hyprland = Hyprland::new(Path::new(runtime), signature);
    for operation in operations {
        let value = if operation == "index" {
            hyprland.layout_index().ok().map(|index| index.to_string())
        } else if operation == "latin" {
            hyprland.latin_layouts().ok().map(|bits| {
                bits.iter()
                    .map(|is_latin| if *is_latin { '1' } else { '0' })
                    .collect::<String>()
            })
        } else if operation == "focused" {
            hyprland.focused_pid().ok().map(|pid| pid.to_string())
        } else if operation == "list" {
            hyprland.layout_list().ok()
        } else {
            operation
                .strip_prefix("set:")
                .and_then(|number| number.parse::<i32>().ok())
                .and_then(|index| hyprland.set_layout(index).ok())
                .map(|()| "ok".to_string())
        };
        match value {
            Some(value) => {
                let _ = writeln!(std::io::stdout(), "{operation} {value}");
            }
            None => {
                let _ = writeln!(std::io::stdout(), "{operation} error");
            }
        }
    }
}

/// A fake Hyprland for the parity check (Rust only: C++ has no fake). The
/// script holds a fifo open on stdin until the client is done, so the fake
/// can print its requests afterwards.
fn serve(
    runtime: &str,
    signature: &str,
    devices_file: &str,
    window_file: &str,
) -> Result<(), std::io::Error> {
    let devices = std::fs::read_to_string(devices_file)?;
    let window = std::fs::read_to_string(window_file)?;
    let fake = FakeHyprland::start(
        Path::new(runtime),
        signature,
        FakeReplies {
            devices,
            active_window: window,
        },
    )?;
    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin)?;
    let requests = fake.requests();
    // Drop before anything removes the runtime directory (a deadlock found in review).
    drop(fake);
    for request in &requests {
        println!("{request}");
    }
    Ok(())
}

fn main() -> Result<(), std::io::Error> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("classify") => classify(),
        Some("run") if args.len() >= 5 => {
            let start: i32 = args[3].parse().unwrap_or(0);
            run(&args[2], start, &args[4], &args[5..])
        }
        Some("json") if args.len() >= 3 => json(&args[2..]),
        Some("hyprland") if args.len() >= 4 => {
            hyprland(&args[2], &args[3], &args[4..]);
            Ok(())
        }
        Some("serve") if args.len() == 6 => serve(&args[2], &args[3], &args[4], &args[5]),
        _ => {
            eprintln!(
                "usage: parity_driver classify | run DIR INDEX LATIN OP... | json KEY... | hyprland RUNTIME SIGNATURE OP... | serve RUNTIME SIGNATURE DEVICES_FILE WINDOW_FILE"
            );
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "bad command line",
            ))
        }
    }
}
