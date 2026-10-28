//! doctor: the helpers and the pure findings, ported from `src/doctor.cpp`.
//! This file does no I/O: the checks that gather text come in as arguments.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use password_layout_core::compositor::Compositor;
use password_layout_core::hyprland::Hyprland;

use crate::tmux::{self, run_command};
use crate::tty;

/// The user unit the service runs as.
pub const UNIT: &str = "password-layout-tty.service";
/// The fcitx5 addon file.
pub const ADDON: &str = "libpasswordlayout.so";
/// Where the addon describes itself, in order.
pub const ADDON_CONFS: [&str; 2] = [
    "/usr/share/fcitx5/addon/passwordlayout.conf",
    "/usr/local/share/fcitx5/addon/passwordlayout.conf",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Ok,
    Info,
    Warn,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub what: String,
    pub hint: String,
}

impl Finding {
    pub fn new(level: Level, what: impl Into<String>, hint: impl Into<String>) -> Finding {
        Finding {
            level,
            what: what.into(),
            hint: hint.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mapped {
    No,
    Yes,
    Replaced,
}

pub fn trim(text: &str) -> &str {
    text.trim_end_matches(['\n', ' '])
}

// C `atoi`: leading whitespace, an optional sign, then the longest run of
// digits; no digits at all is zero.
pub fn atoi(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r') {
        i += 1;
    }
    let mut sign = 1i64;
    if i < bytes.len() && (bytes[i] == b'-' || bytes[i] == b'+') {
        if bytes[i] == b'-' {
            sign = -1;
        }
        i += 1;
    }
    let mut value = 0i64;
    let mut digits = false;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        digits = true;
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(bytes[i] - b'0'));
        i += 1;
    }
    if !digits {
        return 0;
    }
    (sign * value).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn version_numbers(text: &str) -> Vec<i64> {
    let mut parts = Vec::new();
    let mut value: i64 = -1;
    for c in text.chars() {
        if c.is_ascii_digit() {
            let start = if value < 0 {
                0
            } else {
                value.saturating_mul(10)
            };
            value = start.saturating_add(i64::from(c as u32 - b'0' as u32));
        } else if c == '.' && value >= 0 {
            parts.push(value);
            value = -1;
        } else {
            break;
        }
    }
    if value >= 0 {
        parts.push(value);
    }
    parts
}

pub fn version_less(a: &str, b: &str) -> bool {
    version_numbers(a) < version_numbers(b)
}

pub fn sudo_version(output: &str) -> Option<String> {
    let rest = output.strip_prefix("Sudo version ")?;
    Some(rest.split('\n').next().unwrap_or("").to_string())
}

pub fn addon_requires_core(conf: &str) -> Option<String> {
    for line in conf.split('\n') {
        if let Some(at) = line.find("=core:") {
            return Some(trim(&line[at + "=core:".len()..]).to_string());
        }
    }
    None
}

pub fn fcitx_keyboard_layouts(profile: &str) -> Vec<String> {
    let mut layouts = Vec::new();
    let mut in_item = false;
    for line in profile.split('\n') {
        if line.starts_with('[') {
            in_item = line.contains("/Items/");
        } else if in_item && line.starts_with("Name=keyboard-") {
            let name = trim(&line["Name=".len()..]).to_string();
            if !layouts.contains(&name) {
                layouts.push(name);
            }
        }
    }
    layouts
}

pub fn maps_library(maps: &str, file_name: &str) -> Mapped {
    let needle = format!("/{file_name}");
    let deleted = format!("{needle} (deleted)");
    let mut found = Mapped::No;
    for line in maps.split('\n') {
        if line.ends_with(&needle) {
            return Mapped::Yes;
        }
        if line.ends_with(&deleted) {
            found = Mapped::Replaced;
        }
    }
    found
}

pub fn exe_replaced(link: &str) -> bool {
    link.ends_with(" (deleted)")
}

pub fn systemd_properties(shown: &str) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    for line in shown.split('\n') {
        if let Some(at) = line.find('=') {
            properties.insert(line[..at].to_string(), line[at + 1..].to_string());
        }
    }
    properties
}

pub fn fcitx_profile_path(config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(config) = config_home.filter(|c| !c.is_empty()) {
        let mut path = config.to_os_string();
        path.push("/fcitx5/profile");
        return Some(PathBuf::from(path));
    }
    if let Some(home) = home {
        let mut path = home.to_os_string();
        path.push("/.config/fcitx5/profile");
        return Some(PathBuf::from(path));
    }
    None
}

pub fn hyprland_finding(result: io::Result<(Vec<bool>, String)>) -> Finding {
    let (latin, list) = match result {
        Err(error) => {
            return Finding::new(
                Level::Fail,
                format!("Hyprland is not reachable: {error}"),
                "run this inside your Hyprland session",
            );
        }
        Ok(found) => found,
    };
    if latin.is_empty() {
        return Finding::new(
            Level::Fail,
            "Hyprland: the keyboard layouts could not be read",
            "",
        );
    }
    let mut latin_names = String::new();
    for (index, is_latin) in latin.iter().enumerate() {
        let Some(name) = list.split(',').nth(index) else {
            break;
        };
        if *is_latin {
            latin_names += if latin_names.is_empty() { "" } else { ", " };
            latin_names += name;
        }
    }
    if latin_names.is_empty() {
        Finding::new(
            Level::Fail,
            format!("Hyprland: no Latin layout among \"{list}\", so there is nothing to switch to"),
            "add one, such as us, to kb_layout in Hyprland's input settings",
        )
    } else {
        Finding::new(
            Level::Ok,
            format!("Hyprland: layouts {list}; Latin: {latin_names}"),
            "",
        )
    }
}

pub fn service_finding(shown: Option<&str>, exe_link: Option<&str>, userns_clone: &str) -> Finding {
    let shown = match shown {
        None => {
            return Finding::new(
                Level::Fail,
                "terminal service: systemctl --user does not answer",
                "the service needs a systemd user session",
            );
        }
        Some(shown) => shown,
    };
    let p = systemd_properties(shown);
    if p.get("LoadState").is_none_or(|s| s != "loaded") {
        return Finding::new(
            Level::Fail,
            "terminal service: not installed",
            "install the package, or run make install-user from a checkout",
        );
    }
    let state = p.get("ActiveState").map(String::as_str).unwrap_or("");
    let restarts = atoi(p.get("NRestarts").map(String::as_str).unwrap_or(""));
    let enabled = p.get("UnitFileState").is_some_and(|s| s == "enabled");
    if state == "active" && restarts == 0 {
        if exe_link.is_some_and(exe_replaced) {
            return Finding::new(
                Level::Warn,
                "terminal service: running the binary of a package that has been updated since",
                "systemctl --user restart password-layout-tty",
            );
        }
        if !enabled {
            return Finding::new(
                Level::Warn,
                "terminal service: running, but not enabled: it will not start at the next login",
                "systemctl --user enable password-layout-tty",
            );
        }
        return Finding::new(Level::Ok, "terminal service: running", "");
    }
    if restarts > 0 {
        let hint = if trim(userns_clone) == "0" {
            "this kernel has unprivileged user namespaces turned off, which part of the sandbox \
needs; turn that part off and restart the service:\n\
mkdir -p ~/.config/systemd/user/password-layout-tty.service.d && touch \
~/.config/systemd/user/password-layout-tty.service.d/sandbox-namespaces.conf && \
systemctl --user restart password-layout-tty"
        } else {
            "see journalctl --user -u password-layout-tty"
        };
        return Finding::new(
            if state == "active" {
                Level::Warn
            } else {
                Level::Fail
            },
            format!("terminal service: {state}, restarted {restarts} times"),
            hint,
        );
    }
    Finding::new(
        Level::Fail,
        format!(
            "terminal service: {state}{}",
            if enabled { "" } else { ", not enabled" }
        ),
        "systemctl --user enable --now password-layout-tty",
    )
}

pub fn fcitx_finding(maps: &[String]) -> Option<Finding> {
    if maps.is_empty() {
        return Some(Finding::new(
            Level::Warn,
            "fcitx5 is not running: password fields in graphical applications are not covered, \
terminals are",
            "start fcitx5 as the input method (Omarchy does this by default)",
        ));
    }
    for map in maps {
        match maps_library(map, ADDON) {
            Mapped::Yes => {
                return Some(Finding::new(Level::Ok, "fcitx5: running, addon loaded", ""));
            }
            Mapped::Replaced => {
                return Some(Finding::new(
                    Level::Warn,
                    "fcitx5 runs the addon of a package that has been updated since",
                    "restart fcitx5 to load the new one: fcitx5 -rd (on Omarchy: systemctl \
--user restart omarchy-fcitx5)",
                ));
            }
            Mapped::No => {}
        }
    }
    None
}

pub fn fcitx_without_addon_finding(running: Option<&str>, confs: &[String]) -> Finding {
    if let Some(running) = running {
        for conf in confs {
            if let Some(required) =
                addon_requires_core(conf).filter(|r| version_less(trim(running), r))
            {
                return Finding::new(
                    Level::Warn,
                    format!(
                        "fcitx5 {} is running without the addon, which needs {required} or newer",
                        trim(running)
                    ),
                    "update the system (sudo pacman -Syu; on Omarchy: omarchy update), or \
install a package built for this fcitx5",
                );
            }
        }
    }
    Finding::new(
        Level::Warn,
        "fcitx5 is running without the addon",
        "restart fcitx5 so it loads it: fcitx5 -rd (on Omarchy: systemctl --user restart \
omarchy-fcitx5)",
    )
}

pub fn fcitx_layouts_finding(profile: &str) -> Option<Finding> {
    let layouts = fcitx_keyboard_layouts(profile);
    if layouts.len() < 2 {
        return None;
    }
    Some(Finding::new(
        Level::Warn,
        format!(
            "fcitx5 switches between keyboard layouts ({}); only layouts in Hyprland's kb_layout \
are switched for passwords",
            layouts.join(", ")
        ),
        "keep one keyboard layout in fcitx5 and list the others in kb_layout (Hyprland's input \
settings)",
    ))
}

pub fn fcitx_qt_finding(installed: Option<&str>) -> Option<Finding> {
    let installed = installed?;
    let fields: Vec<&str> = trim(installed).split(' ').collect();
    if fields.len() < 2 || !version_less(fields[1], "5.1.16") {
        return None;
    }
    Some(Finding::new(
        Level::Warn,
        format!(
            "fcitx5-qt {}: Qt windows that open with a password field already focused (KeePassXC, \
the polkit prompt, possibly the lock screen) are not recognised until focus moves",
            fields[1]
        ),
        "fixed in fcitx5-qt 5.1.16, which Arch and Omarchy ship: update the system (sudo pacman \
-Syu; on Omarchy: omarchy update)",
    ))
}

pub fn sudo_finding(output: Option<&str>) -> Option<Finding> {
    let version = output.and_then(sudo_version)?;
    if version_less(&version, "1.9.14") {
        return None;
    }
    Some(Finding::new(
        Level::Info,
        format!(
            "sudo {version} runs commands in a terminal of its own: sudo's password prompt is \
recognised, prompts of the programs it runs are not"
        ),
        "",
    ))
}

/// C++ `readFile`: the file's text, `""` on any error.
fn read_text(path: impl AsRef<Path>) -> String {
    fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// C++ `checkHyprland`: environment, `j/devices` for the Latin flags, then
/// again for the list; the first error becomes the finding.
pub fn check_hyprland() -> Finding {
    let result = (|| {
        let mut hyprland = Hyprland::from_env()?;
        let latin = hyprland.latin_layouts()?;
        let list = hyprland.layout_list()?;
        Ok((latin, list))
    })();
    hyprland_finding(result)
}

/// C++ `checkService`.
pub fn check_service() -> Finding {
    let shown = run_command(
        &[
            "systemctl",
            "--user",
            "show",
            UNIT,
            "-p",
            "LoadState",
            "-p",
            "ActiveState",
            "-p",
            "NRestarts",
            "-p",
            "UnitFileState",
            "-p",
            "MainPID",
        ],
        tmux::COMMAND_TIMEOUT_MS,
    );
    let properties = shown.as_deref().map(systemd_properties).unwrap_or_default();
    // `MainPID` missing or not a pid: the read fails and the link is `None`,
    // as when C++ reads `properties["MainPID"]` (empty string).
    let main_pid = properties.get("MainPID").cloned().unwrap_or_default();
    let exe_link = fs::read_link(format!("/proc/{main_pid}/exe"))
        .ok()
        .map(|link| link.to_string_lossy().into_owned());
    let userns_clone = read_text("/proc/sys/kernel/unprivileged_userns_clone");
    service_finding(shown.as_deref(), exe_link.as_deref(), &userns_clone)
}

/// C++ `fcitxProcesses`: fcitx5 processes of this user, in `/proc` order.
pub fn fcitx_processes() -> Vec<i32> {
    let mut pids = Vec::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return pids;
    };
    let uid = tty::current_uid();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let Some(&first) = name.as_bytes().first() else {
            continue;
        };
        if !(b'1'..=b'9').contains(&first) {
            continue;
        }
        let Ok(meta) = fs::metadata(entry.path()) else {
            continue;
        };
        if meta.uid() != uid {
            continue;
        }
        if trim(&read_text(entry.path().join("comm"))) == "fcitx5" {
            pids.push(atoi(name));
        }
    }
    pids
}

/// C++ `checkFcitx`; `fcitx5 --version` runs only when the addon is not
/// mapped.
pub fn check_fcitx() -> Finding {
    let maps: Vec<String> = fcitx_processes()
        .iter()
        .map(|pid| read_text(format!("/proc/{pid}/maps")))
        .collect();
    if let Some(finding) = fcitx_finding(&maps) {
        return finding;
    }
    let running = run_command(&["fcitx5", "--version"], tmux::COMMAND_TIMEOUT_MS);
    let confs: Vec<String> = ADDON_CONFS.iter().map(read_text).collect();
    fcitx_without_addon_finding(running.as_deref(), &confs)
}

/// C++ `checkFcitxLayouts`.
pub fn check_fcitx_layouts() -> Option<Finding> {
    let profile = fcitx_profile_path(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
    .map(read_text)
    .unwrap_or_default();
    fcitx_layouts_finding(&profile)
}

/// C++ `checkFcitxQt`.
pub fn check_fcitx_qt() -> Option<Finding> {
    fcitx_qt_finding(
        run_command(&["pacman", "-Q", "fcitx5-qt"], tmux::COMMAND_TIMEOUT_MS).as_deref(),
    )
}

/// C++ `checkSudo`.
pub fn check_sudo() -> Option<Finding> {
    sudo_finding(run_command(&["sudo", "-V"], tmux::COMMAND_TIMEOUT_MS).as_deref())
}

/// C++ `diagnose`.
pub fn diagnose() -> Vec<Finding> {
    let mut findings = vec![check_hyprland(), check_service(), check_fcitx()];
    findings.extend(
        [check_fcitx_layouts(), check_fcitx_qt(), check_sudo()]
            .into_iter()
            .flatten(),
    );
    findings
}

pub fn print_findings(findings: &[Finding], out: &mut dyn Write) -> i32 {
    let mut failed = false;
    for finding in findings {
        let label = match finding.level {
            Level::Ok => "ok  ",
            Level::Info => "note",
            Level::Warn => "warn",
            Level::Fail => {
                failed = true;
                "FAIL"
            }
        };
        let _ = writeln!(out, "{label}  {}", finding.what);
        for line in finding.hint.split('\n') {
            if !line.is_empty() {
                let _ = writeln!(out, "      -> {line}");
            }
        }
    }
    if failed { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_removes_only_trailing_newlines_and_spaces() {
        assert_eq!(trim("x \n \n  "), "x");
        assert_eq!(trim("x\n"), "x");
        assert_eq!(trim(" x"), " x");
        assert_eq!(trim("x\r"), "x\r");
        assert_eq!(trim("a b c"), "a b c");
    }

    #[test]
    fn atoi_reads_c_atoi() {
        assert_eq!(atoi("3x"), 3);
        assert_eq!(atoi(""), 0);
        assert_eq!(atoi(" -2"), -2);
        assert_eq!(atoi("  \t+7"), 7);
        assert_eq!(atoi("x"), 0);
        assert_eq!(atoi("-"), 0);
    }

    #[test]
    fn version_less_compares_number_lists() {
        assert!(version_less("", "1"));
        assert!(version_less("5.1", "5.1.0"));
        assert!(version_less("5.1.15-1", "5.1.16"));
        assert!(!version_less("5.1.16", "5.1.16"));
        assert!(!version_less("5.2", "5.1.16"));
    }

    #[test]
    fn sudo_version_reads_the_first_line() {
        assert_eq!(
            sudo_version("Sudo version 1.9.17p2\nSudoers policy plugin version 1.9.17p2\n"),
            Some("1.9.17p2".to_string())
        );
        assert_eq!(
            sudo_version("Sudo version 1.9.17p2"),
            Some("1.9.17p2".to_string())
        );
        assert_eq!(sudo_version("sudo: command not found"), None);
    }

    #[test]
    fn addon_requires_core_finds_the_first_line() {
        assert_eq!(
            addon_requires_core("[Addon]\nName=x\n\n[Addon/Dependencies]\n0=core:5.1.23\n"),
            Some("5.1.23".to_string())
        );
        assert_eq!(addon_requires_core("[Addon]\nName=x\n"), None);
    }

    #[test]
    fn fcitx_keyboard_layouts_keeps_first_appearance() {
        let omarchy = "[Groups/0]\nName=Default\nDefault Layout=us\nDefaultIM=keyboard-us\n\n\
                       [Groups/0/Items/0]\nName=keyboard-us\n\n[GroupOrder]\n0=Default\n";
        assert_eq!(
            fcitx_keyboard_layouts(omarchy),
            vec!["keyboard-us".to_string()]
        );
        let switching = format!(
            "{omarchy}[Groups/0/Items/1]\nName=keyboard-ru\n[Groups/1/Items/0]\nName=keyboard-us\n\
             [Groups/1/Items/1]\nName=pinyin\n"
        );
        assert_eq!(
            fcitx_keyboard_layouts(&switching),
            vec!["keyboard-us".to_string(), "keyboard-ru".to_string()]
        );
    }

    #[test]
    fn maps_library_tells_live_from_replaced() {
        let maps = "7f00-7f01 r-xp 00000000 00:1f 123  /usr/lib/fcitx5/libpasswordlayout.so\n\
                    7f02-7f03 r--p 00000000 00:1f 124  /usr/lib/libnotpasswordlayout.so.1\n";
        assert_eq!(maps_library(maps, "libpasswordlayout.so"), Mapped::Yes);
        assert_eq!(maps_library(maps, "libFcitx5Core.so"), Mapped::No);
        assert_eq!(
            maps_library(
                "7f00-7f01 r-xp 0 00:1f 123  /usr/lib/fcitx5/libpasswordlayout.so (deleted)\n",
                "libpasswordlayout.so"
            ),
            Mapped::Replaced
        );
        assert_eq!(
            maps_library(
                "7f02-7f03 r--p 0 00:1f 124  /x/libpasswordlayout.so.old\n",
                "libpasswordlayout.so"
            ),
            Mapped::No
        );
    }

    #[test]
    fn exe_replaced_marks_deleted_links() {
        assert!(exe_replaced("/usr/bin/password-layout (deleted)"));
        assert!(!exe_replaced("/usr/bin/password-layout"));
    }

    #[test]
    fn systemd_properties_keeps_the_last_value() {
        let properties =
            systemd_properties("LoadState=loaded\nActiveState=active\nActiveState=auto\n");
        assert_eq!(properties.get("LoadState"), Some(&"loaded".to_string()));
        assert_eq!(properties.get("ActiveState"), Some(&"auto".to_string()));
        assert!(systemd_properties("").is_empty());
    }

    #[test]
    fn fcitx_profile_path_prefers_the_config_home() {
        assert_eq!(
            fcitx_profile_path(Some(OsStr::new("/x")), Some(OsStr::new("/h"))),
            Some(PathBuf::from("/x/fcitx5/profile"))
        );
        assert_eq!(
            fcitx_profile_path(Some(OsStr::new("")), Some(OsStr::new("/h"))),
            Some(PathBuf::from("/h/.config/fcitx5/profile"))
        );
        assert_eq!(
            fcitx_profile_path(None, Some(OsStr::new(""))),
            Some(PathBuf::from("/.config/fcitx5/profile"))
        );
        assert_eq!(fcitx_profile_path(None, None), None);
    }

    fn active(enabled: bool) -> String {
        let unit_file_state = if enabled { "enabled" } else { "disabled" };
        format!(
            "LoadState=loaded\nActiveState=active\nNRestarts=0\nUnitFileState={unit_file_state}\n"
        )
    }

    #[test]
    fn service_finding_reports_when_systemctl_is_silent() {
        let finding = service_finding(None, None, "1");
        assert_eq!(finding.level, Level::Fail);
        assert_eq!(
            finding.what,
            "terminal service: systemctl --user does not answer"
        );
        assert_eq!(finding.hint, "the service needs a systemd user session");
    }

    #[test]
    fn service_finding_reports_a_missing_unit() {
        let finding = service_finding(Some("LoadState=not-found\n"), None, "1");
        assert_eq!(finding.level, Level::Fail);
        assert_eq!(finding.what, "terminal service: not installed");
        assert_eq!(
            finding.hint,
            "install the package, or run make install-user from a checkout"
        );
    }

    #[test]
    fn service_finding_reports_an_updated_binary() {
        let finding = service_finding(
            Some(active(true).as_str()),
            Some("/usr/bin/password-layout (deleted)"),
            "1",
        );
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(
            finding.what,
            "terminal service: running the binary of a package that has been updated since"
        );
        assert_eq!(finding.hint, "systemctl --user restart password-layout-tty");
    }

    #[test]
    fn service_finding_reports_a_running_service_that_is_not_enabled() {
        let finding = service_finding(
            Some(active(false).as_str()),
            Some("/usr/bin/password-layout"),
            "1",
        );
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(
            finding.what,
            "terminal service: running, but not enabled: it will not start at the next login"
        );
        assert_eq!(finding.hint, "systemctl --user enable password-layout-tty");
    }

    #[test]
    fn service_finding_accepts_a_running_service() {
        let finding = service_finding(
            Some(active(true).as_str()),
            Some("/usr/bin/password-layout"),
            "1",
        );
        assert_eq!(finding.level, Level::Ok);
        assert_eq!(finding.what, "terminal service: running");
        assert_eq!(finding.hint, "");
    }

    #[test]
    fn service_finding_reports_restarts() {
        let shown = "LoadState=loaded\nActiveState=active\nNRestarts=3\nUnitFileState=enabled\n";
        let finding = service_finding(Some(shown), None, "1");
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(finding.what, "terminal service: active, restarted 3 times");
        assert_eq!(finding.hint, "see journalctl --user -u password-layout-tty");

        let failed = service_finding(
            Some("LoadState=loaded\nActiveState=failed\nNRestarts=2\nUnitFileState=enabled\n"),
            None,
            "1",
        );
        assert_eq!(failed.level, Level::Fail);
        assert_eq!(failed.what, "terminal service: failed, restarted 2 times");

        // The kernel without user namespaces gets the long hint.
        let off = service_finding(Some(shown), None, "0\n");
        assert_eq!(
            off.hint,
            "this kernel has unprivileged user namespaces turned off, which part of the sandbox \
needs; turn that part off and restart the service:\nmkdir -p \
~/.config/systemd/user/password-layout-tty.service.d && touch \
~/.config/systemd/user/password-layout-tty.service.d/sandbox-namespaces.conf && systemctl \
--user restart password-layout-tty"
        );
    }

    #[test]
    fn service_finding_reports_a_stopped_service() {
        let shown = "LoadState=loaded\nActiveState=inactive\nNRestarts=0\nUnitFileState=disabled\n";
        let finding = service_finding(Some(shown), None, "1");
        assert_eq!(finding.level, Level::Fail);
        assert_eq!(finding.what, "terminal service: inactive, not enabled");
        assert_eq!(
            finding.hint,
            "systemctl --user enable --now password-layout-tty"
        );

        let shown = "LoadState=loaded\nActiveState=inactive\nNRestarts=0\nUnitFileState=enabled\n";
        let finding = service_finding(Some(shown), None, "1");
        assert_eq!(finding.what, "terminal service: inactive");
    }

    #[test]
    fn fcitx_finding_reads_the_maps_of_each_process() {
        let finding = fcitx_finding(&[]).expect("no fcitx5 at all is a finding");
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(
            finding.what,
            "fcitx5 is not running: password fields in graphical applications are not covered, \
terminals are"
        );

        let loaded = String::from(
            "7f00-7f01 r-xp 00000000 00:1f 123  /usr/lib/fcitx5/libpasswordlayout.so\n",
        );
        let finding = fcitx_finding(&[loaded]).expect("the addon is mapped");
        assert_eq!(finding.level, Level::Ok);
        assert_eq!(finding.what, "fcitx5: running, addon loaded");

        let replaced = String::from(
            "7f00-7f01 r-xp 00000000 00:1f 123  /usr/lib/fcitx5/libpasswordlayout.so (deleted)\n",
        );
        let finding = fcitx_finding(&[String::new(), replaced]).expect("the old addon is mapped");
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(
            finding.what,
            "fcitx5 runs the addon of a package that has been updated since"
        );

        assert!(fcitx_finding(&[String::new()]).is_none());
    }

    #[test]
    fn fcitx_without_addon_finding_checks_the_core_version() {
        let confs = [String::from(
            "[Addon]\nName=x\n\n[Addon/Dependencies]\n0=core:5.1.23\n",
        )];
        // The output of `fcitx5 --version` itself starts with "fcitx5".
        let finding = fcitx_without_addon_finding(Some("fcitx5 5.1.22\n"), &confs);
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(
            finding.what,
            "fcitx5 fcitx5 5.1.22 is running without the addon, which needs 5.1.23 or newer"
        );

        // version_less("fcitx5 …", …) is always true (parsing stops at the "f"),
        // exactly like in C++; the plain message needs a bare version.
        let finding = fcitx_without_addon_finding(Some("5.1.23\n"), &confs);
        assert_eq!(finding.what, "fcitx5 is running without the addon");
        let finding = fcitx_without_addon_finding(None, &confs);
        assert_eq!(finding.what, "fcitx5 is running without the addon");
    }

    #[test]
    fn fcitx_layouts_finding_wants_two_layouts() {
        let profile =
            "[Groups/0/Items/0]\nName=keyboard-us\n[Groups/0/Items/1]\nName=keyboard-ru\n";
        let finding = fcitx_layouts_finding(profile).expect("two layouts are switched");
        assert_eq!(finding.level, Level::Warn);
        assert_eq!(
            finding.what,
            "fcitx5 switches between keyboard layouts (keyboard-us, keyboard-ru); only layouts \
in Hyprland's kb_layout are switched for passwords"
        );
        assert!(fcitx_layouts_finding("[Groups/0/Items/0]\nName=keyboard-us\n").is_none());
    }

    #[test]
    fn fcitx_qt_finding_warns_only_about_old_versions() {
        let finding = fcitx_qt_finding(Some("fcitx5-qt 5.1.15-1 x86_64\n")).expect("too old");
        assert_eq!(finding.level, Level::Warn);
        assert!(finding.what.starts_with("fcitx5-qt 5.1.15-1: Qt windows"));
        assert!(fcitx_qt_finding(Some("fcitx5-qt 5.1.16-1\n")).is_none());
        assert!(fcitx_qt_finding(Some("fcitx5-qt\n")).is_none());
        assert!(fcitx_qt_finding(None).is_none());
    }

    #[test]
    fn sudo_finding_recommends_the_new_sudo() {
        let finding = sudo_finding(Some(
            "Sudo version 1.9.17p2\nplugin: sudoers version 1.9.17p2\n",
        ))
        .expect("the new sudo is noted");
        assert_eq!(finding.level, Level::Info);
        assert!(
            finding
                .what
                .starts_with("sudo 1.9.17p2 runs commands in a terminal")
        );
        assert!(sudo_finding(Some("Sudo version 1.9.13p3\n")).is_none());
        assert!(sudo_finding(Some("sudo: command not found\n")).is_none());
        assert!(sudo_finding(None).is_none());
    }

    #[test]
    fn hyprland_finding_reaches_four_outcomes() {
        let finding = hyprland_finding(Err(io::Error::other("no environment")));
        assert_eq!(finding.level, Level::Fail);
        assert_eq!(finding.what, "Hyprland is not reachable: no environment");

        let finding = hyprland_finding(Ok((vec![], String::new())));
        assert_eq!(
            finding.what,
            "Hyprland: the keyboard layouts could not be read"
        );

        let finding = hyprland_finding(Ok((vec![false], "ru,ua".to_string())));
        assert_eq!(finding.level, Level::Fail);
        assert_eq!(
            finding.what,
            "Hyprland: no Latin layout among \"ru,ua\", so there is nothing to switch to"
        );

        let finding = hyprland_finding(Ok((vec![true, false], "us,ru".to_string())));
        assert_eq!(finding.level, Level::Ok);
        assert_eq!(finding.what, "Hyprland: layouts us,ru; Latin: us");
    }

    #[test]
    fn print_findings_labels_and_indents() {
        let findings = [
            Finding::new(Level::Ok, "a is fine", ""),
            Finding::new(Level::Info, "b is worth a note", ""),
            Finding::new(Level::Warn, "c needs attention", "first step\nsecond step"),
        ];
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(print_findings(&findings, &mut out), 0);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            // The label "ok  " already ends in two spaces; printf adds two more.
            "ok    a is fine\nnote  b is worth a note\nwarn  c needs attention\n      -> first \
step\n      -> second step\n"
        );

        let findings = [
            Finding::new(Level::Fail, "d is broken", "fix it"),
            Finding::new(Level::Ok, "a is fine", ""),
        ];
        let mut out: Vec<u8> = Vec::new();
        assert_eq!(print_findings(&findings, &mut out), 1);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "FAIL  d is broken\n      -> fix it\nok    a is fine\n"
        );
    }
}
