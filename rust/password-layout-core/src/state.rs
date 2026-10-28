//! Holders and the layout to restore, in `$XDG_RUNTIME_DIR/password-layout`.
//! Ported from src/state.cpp.
//!
//! The state file is byte-compatible with the C++ one: first line the layout
//! to restore (`-1` when Latin was not switched to), then one holder per
//! line, every line ending in `\n`. Example: `1\ntty\nim\n`.

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::compositor::Compositor;

/// Holder names are lines of the state file: `a-z`, `0-9`, `-`, at most 32
/// bytes, non-empty.
pub fn is_valid_holder(holder: &str) -> bool {
    !holder.is_empty()
        && holder.len() <= 32
        && holder
            .bytes()
            .all(|c: u8| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

fn require_valid_holder(holder: &str) -> io::Result<()> {
    if is_valid_holder(holder) {
        Ok(())
    } else {
        Err(io::Error::new(
            ErrorKind::InvalidInput,
            "holder must be lowercase letters, digits and '-', at most 32 characters",
        ))
    }
}

/// The whole line as a layout number, or nothing: a half-written or foreign
/// line must never read as layout 0.
///
/// `std::from_chars` semantics: optional `-`, then digits only. `+1`, ` 1`,
/// `1 `, `1x` and the empty line are all malformed; so is anything that does
/// not fit in an `i32`.
fn parse_layout(line: &[u8], minimum: i32) -> Option<i32> {
    let (negative, digits) = if line.first() == Some(&b'-') {
        (true, &line[1..])
    } else {
        (false, line)
    };
    if digits.is_empty() || !digits.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // from_chars rejects what would not fit in the type, signed or not.
    let limit = if negative {
        2_147_483_648i64 // i32::MIN
    } else {
        2_147_483_647i64 // i32::MAX
    };
    let mut value: i64 = 0;
    for &c in digits {
        value = value.checked_mul(10)?.checked_add((c - b'0') as i64)?;
        if value > limit {
            return None;
        }
    }
    let value = (if negative { -value } else { value }) as i32;
    (value >= minimum).then_some(value)
}

/// Like `std::getline` split on `\n` only: a final `\n` ends the last line,
/// it does not start an empty one. `\r` is an ordinary character.
fn lines(text: &[u8]) -> Vec<&[u8]> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&[u8]> = text.split(|&c| c == b'\n').collect();
    if text.last() == Some(&b'\n') {
        lines.pop();
    }
    lines
}

/// Readers see either the old file or the new one, never a truncated one.
fn write_atomically(path: &Path, text: &[u8]) -> io::Result<()> {
    // C++ does `path + ".tmp"`: concatenate, not Path::push (which would
    // make a directory).
    let mut temporary: std::ffi::OsString = path.as_os_str().to_owned();
    temporary.push(".tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&temporary)?;
        file.write_all(text)?;
        file.flush()?;
    }
    std::fs::rename(&temporary, path)
}

/// Held for the lifetime of the operation; serializes enter() and leave()
/// across processes.
struct Lock {
    /// Held so the lock stays taken until the operation ends.
    #[allow(dead_code)]
    file: File,
}

impl Lock {
    fn acquire(directory: &Path) -> io::Result<Lock> {
        match std::fs::create_dir(directory) {
            Ok(()) => {
                // mkdir(0700) in the C++; create_dir honours the umask, so
                // tighten it.
                std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700))?;
            }
            // AlreadyExists is fine: the service owns the directory.
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let path = directory.join("lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false) // the lock file lives between calls
            .mode(0o600)
            .open(&path)?;
        file.lock()?; // exclusive flock
        Ok(Lock { file })
    }
}

struct Contents {
    previous: i32,
    holders: Vec<String>,
}

pub struct State {
    directory: PathBuf,
    path: PathBuf,
    last_latin_path: PathBuf,
}

impl State {
    pub fn new(directory: impl Into<PathBuf>) -> State {
        let directory = directory.into();
        State {
            path: directory.join("state"),
            last_latin_path: directory.join("last-latin"),
            directory,
        }
    }

    pub fn from_env() -> io::Result<State> {
        let runtime = std::env::var("XDG_RUNTIME_DIR");
        if runtime.as_deref().map(str::is_empty).unwrap_or(true) {
            return Err(io::Error::other("XDG_RUNTIME_DIR is not set"));
        }
        Ok(State::new(format!("{}/password-layout", runtime.unwrap())))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn last_latin(&self) -> Option<i32> {
        let Ok(text) = std::fs::read(&self.last_latin_path) else {
            return None;
        };
        lines(&text).first().and_then(|line| parse_layout(line, 0))
    }

    /// First line is the layout to restore, every following line one holder.
    fn load(&self) -> Contents {
        let Ok(text) = std::fs::read(&self.path) else {
            return Contents {
                previous: -1,
                holders: Vec::new(),
            };
        };
        let mut line_iter = lines(&text).into_iter();
        // Anything malformed counts as no state at all: switching nothing is
        // safer than restoring a layout read from garbage.
        let Some(first) = line_iter.next() else {
            return Contents {
                previous: -1,
                holders: Vec::new(),
            };
        };
        let Some(previous) = parse_layout(first, -1) else {
            return Contents {
                previous: -1,
                holders: Vec::new(),
            };
        };
        let mut holders = Vec::new();
        for line in line_iter {
            if !is_valid_holder(std::str::from_utf8(line).unwrap_or("")) {
                return Contents {
                    previous: -1,
                    holders: Vec::new(),
                };
            }
            holders.push(String::from_utf8_lossy(line).into_owned());
        }
        Contents { previous, holders }
    }

    fn save(&self, contents: &Contents) -> io::Result<()> {
        let mut text = String::new();
        text.push_str(&contents.previous.to_string());
        text.push('\n');
        for holder in &contents.holders {
            text.push_str(holder);
            text.push('\n');
        }
        write_atomically(&self.path, text.as_bytes())
    }

    pub fn enter(&self, holder: &str, compositor: &mut dyn Compositor) -> io::Result<()> {
        require_valid_holder(holder)?;
        let _lock = Lock::acquire(&self.directory)?;
        let mut contents = self.load();
        if contents.holders.is_empty() {
            contents.previous = -1;
            let current = compositor.layout_index()?;
            let latin = compositor.latin_layouts()?;
            let is_latin =
                |index: i32| index >= 0 && index < latin.len() as i32 && latin[index as usize];
            if !latin.is_empty() && !is_latin(current) {
                let mut target: Option<i32> = None;
                if let Some(last) = self.last_latin().filter(|last| is_latin(*last)) {
                    target = Some(last);
                } else {
                    for (index, is_latin) in latin.iter().enumerate() {
                        if *is_latin {
                            target = Some(index as i32);
                            break;
                        }
                    }
                }
                if let Some(target) = target {
                    compositor.set_layout(target)?;
                    contents.previous = current;
                    eprintln!(
                        "password-layout: Latin for {holder} (layout {target}, was {current})"
                    );
                }
            }
        }
        if !contents.holders.iter().any(|h| h == holder) {
            contents.holders.push(holder.to_owned());
        }
        self.save(&contents)
    }

    pub fn leave(&self, holder: &str, compositor: &mut dyn Compositor) -> io::Result<()> {
        require_valid_holder(holder)?;
        let _lock = Lock::acquire(&self.directory)?;
        let contents = self.load();
        let Some(position) = contents.holders.iter().position(|h| h == holder) else {
            return Ok(());
        };
        let mut holders = contents.holders.clone();
        holders.remove(position);
        if !holders.is_empty() {
            return self.save(&Contents {
                previous: contents.previous,
                holders,
            });
        }
        let _ = std::fs::remove_file(&self.path);
        if contents.previous >= 0 {
            compositor.set_layout(contents.previous)?;
            eprintln!(
                "password-layout: layout {} restored ({holder} left)",
                contents.previous
            );
        }
        Ok(())
    }

    pub fn note_layout(&self, compositor: &mut dyn Compositor) -> io::Result<()> {
        let current = compositor.layout_index()?;
        let latin = compositor.latin_layouts()?;
        if current < 0 || current >= latin.len() as i32 || !latin[current as usize] {
            return Ok(());
        }
        let _lock = Lock::acquire(&self.directory)?;
        if self.last_latin() != Some(current) {
            write_atomically(&self.last_latin_path, format!("{current}\n").as_bytes())?;
        }
        Ok(())
    }

    pub fn holders(&self) -> io::Result<Vec<String>> {
        let _lock = Lock::acquire(&self.directory)?;
        Ok(self.load().holders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_split_on_newlines_only() {
        assert_eq!(lines(b"1\ntty\n"), vec![&b"1"[..], &b"tty"[..]]);
        assert_eq!(lines(b"1\ntty"), vec![&b"1"[..], &b"tty"[..]]);
        assert_eq!(lines(b"1\ntty\n\n"), vec![&b"1"[..], &b"tty"[..], &b""[..]]);
        assert_eq!(lines(b""), Vec::<&[u8]>::new());
        // One empty line, like a successful getline of "\n".
        assert_eq!(lines(b"\n"), vec![&b""[..]]);
        assert_eq!(lines(b"\ntty\n"), vec![&b""[..], &b"tty"[..]]);
        // '\r' stays an ordinary character.
        assert_eq!(lines(b"1\r\ntty\r\n"), vec![&b"1\r"[..], &b"tty\r"[..]]);
    }

    #[test]
    fn a_layout_number_is_from_chars() {
        assert_eq!(parse_layout(b"0", -1), Some(0));
        assert_eq!(parse_layout(b"-1", -1), Some(-1));
        assert_eq!(parse_layout(b"-0", -1), Some(0));
        assert_eq!(parse_layout(b"2147483647", 0), Some(2147483647));
        assert_eq!(parse_layout(b"-2147483648", i32::MIN), Some(i32::MIN));
        // A plus sign, spaces and stray characters are not from_chars.
        assert_eq!(parse_layout(b"+1", -1), None);
        assert_eq!(parse_layout(b" 1", -1), None);
        assert_eq!(parse_layout(b"1 ", -1), None);
        assert_eq!(parse_layout(b"1x", -1), None);
        assert_eq!(parse_layout(b"-", -1), None);
        assert_eq!(parse_layout(b"", -1), None);
        assert_eq!(parse_layout(b"1\r", -1), None);
        // Overflow does not read as a small number.
        assert_eq!(parse_layout(b"2147483648", -1), None);
        assert_eq!(parse_layout(b"-2147483649", -1), None);
        assert_eq!(parse_layout(b"99999999999999999999", -1), None);
        // The minimum applies.
        assert_eq!(parse_layout(b"-2", -1), None);
        assert_eq!(parse_layout(b"-1", 0), None);
        assert_eq!(parse_layout(b"0", 0), Some(0));
    }

    fn read_contents(state: &State, text: impl AsRef<[u8]>) -> (i32, Vec<String>) {
        std::fs::write(&state.path, text.as_ref()).unwrap();
        let contents = state.load();
        (contents.previous, contents.holders)
    }

    #[test]
    fn a_state_file_reads_the_cpp_way() {
        let directory = std::env::temp_dir().join(format!(
            "password-layout-parse-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let state = State::new(&directory);

        // The C++ format.
        assert_eq!(
            read_contents(&state, "1\ntty\nim\n"),
            (1, vec!["tty".to_string(), "im".to_string()])
        );
        assert_eq!(
            read_contents(&state, "-1\ntty\n"),
            (-1, vec!["tty".to_string()])
        );
        assert_eq!(read_contents(&state, "1\n"), (1, Vec::new()));
        // No final newline: the last line still counts.
        assert_eq!(
            read_contents(&state, "1\ntty"),
            (1, vec!["tty".to_string()])
        );
        // '\r' is an ordinary character: not a valid holder, not a number.
        assert_eq!(read_contents(&state, "1\r\ntty\n"), (-1, Vec::new()));
        assert_eq!(read_contents(&state, "1\ntty\r\n"), (-1, Vec::new()));
        // '+1' and spaces are not from_chars.
        assert_eq!(read_contents(&state, "+1\ntty\n"), (-1, Vec::new()));
        assert_eq!(read_contents(&state, " 1\ntty\n"), (-1, Vec::new()));
        // An empty line in the middle is not a valid holder.
        assert_eq!(read_contents(&state, "1\n\ntty\n"), (-1, Vec::new()));
        // Overflow in the first line.
        assert_eq!(read_contents(&state, "2147483648\ntty\n"), (-1, Vec::new()));
        // Non-UTF-8 is simply invalid.
        assert_eq!(read_contents(&state, b"1\ntt\xffy\n"), (-1, Vec::new()));

        std::fs::remove_dir_all(&directory).unwrap();
    }
}
