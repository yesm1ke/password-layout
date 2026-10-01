# password-layout

[![Release](https://img.shields.io/github/v/release/yesm1ke/password-layout)](https://github.com/yesm1ke/password-layout/releases/latest)
[![CI](https://github.com/yesm1ke/password-layout/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/yesm1ke/password-layout/actions/workflows/ci.yml)
[![For Hyprland, fcitx5, Arch](https://img.shields.io/badge/for-Hyprland%20%C2%B7%20fcitx5%20%C2%B7%20Arch-blue)](#requirements)
[![License: MIT](https://img.shields.io/github/license/yesm1ke/password-layout)](LICENSE)

Makes typing a password painless when you write in more than one language:
while you type a password the keyboard layout is Latin. If the current layout is
not Latin, the one you used last is switched to, and your layout comes back when
the password is done. For [Hyprland](https://hyprland.org/) with fcitx5;
developed and tested on [Omarchy](https://omarchy.org/), where both come out of
the box.

If you type in a non-Latin layout, you know the routine: `sudo` asks for a
password, you type it blind, it is rejected, and only then do you notice the
layout was wrong.

## The rule

The logic is the one macOS users are used to: in a password field only Latin
layouts are allowed, and the switch there and back happens by itself.

- if the active layout is already Latin, nothing changes — German stays German;
- otherwise the Latin layout that was last in use is switched to, or the first
  Latin one if none has been used yet;
- when the password is done, the layout that was active before comes back.

A layout counts as Latin unless its code is in the list of non-Latin layouts
Omarchy itself uses (`ru`, `ua`, `gr`, `il`, `ara`, …); a `latin` variant, as in
`rs(latin)`, counts as Latin. To know which Latin layout was last in use, the
service follows layout switches through Hyprland's event socket.

The same applies in terminals: a `sudo` or `ssh` password prompt counts as a
password field.

What it does not do: only keyboard layouts are switched. Input methods in fcitx5
(Pinyin, Mozc and the like) are left alone, and other programs are not kept from
reading the keyboard.

## What works today

- **Terminals**: password prompts of `sudo`, `ssh`, `read -s`, and anything else
  that reads a password the way `getpass` does. Any terminal emulator (tested in
  Ghostty and foot); there is no list of supported terminals.
- **Browsers**: password fields in Zen (Firefox-based) and Chromium, through an
  addon for the fcitx5 input method. Other applications work if they mark their
  password fields for the input method, as these browsers do. Qt applications
  do too, with one bug in fcitx5-qt ([#7](https://github.com/yesm1ke/password-layout/issues/7)); Electron password managers are
  not checked yet ([#8](https://github.com/yesm1ke/password-layout/issues/8)).

The scope is deliberately narrow: one person at a real keyboard in a Hyprland
session. Virtual consoles, the boot and disk-unlock screens, remote machines and
other compositors are out of scope.

## Requirements

- Hyprland, with a Latin layout somewhere in `kb_layout` (`us,ru`, `ru,us` and
  `us,de,ru` all work). With no Latin layout at all nothing is switched.
- `g++` with C++20 support and `make`. There are no library dependencies.
- For password fields in graphical applications: fcitx5 running as the input
  method (Omarchy starts it by default), and its development files at build
  time. Without them only the terminal part is built.
- systemd, for the user service.

On Omarchy all of this is already in place. On another Hyprland setup the
terminal part needs nothing more; for graphical applications fcitx5 has to be
installed and running as the input method. Only Omarchy has been tested so far.

## Install

### From a release (Arch-based systems)

```
curl -LO https://github.com/yesm1ke/password-layout/releases/latest/download/password-layout-x86_64.pkg.tar.zst
sudo pacman -U password-layout-x86_64.pkg.tar.zst
```

Then turn it on (below). To update, run the same two commands again. Give
pacman the downloaded file, not the URL: pacman caches downloads by file name
and would reinstall the old one.

Checking the package first is optional. Releases are built by this
repository's GitHub Actions from a tag; either check confirms that, and both
confirm the same thing:

```
gh attestation verify password-layout-x86_64.pkg.tar.zst -R yesm1ke/password-layout
```

```
curl -LO https://github.com/yesm1ke/password-layout/releases/latest/download/password-layout-x86_64.pkg.tar.zst.sig
gpg --keyserver hkps://keyserver.ubuntu.com --recv-keys 7C5242797972ED2923B944029AC95674831AE330
gpg --verify password-layout-x86_64.pkg.tar.zst.sig
```

The key (also in `packaging/arch/password-layout.asc`) signs releases and
nothing else. Do not add it to pacman's keyring: pacman would then trust it
for any package.

The fcitx5 addon in a release is built against the fcitx5 Arch had at that
time. If fcitx5 changes in a way that keeps the addon from loading,
`password-layout doctor` says so; a newer release or a build from a checkout
fixes it. The package is not in the AUR yet ([#11](https://github.com/yesm1ke/password-layout/issues/11)).

### From a checkout

For the current user only (no root needed):

```
git clone https://github.com/yesm1ke/password-layout
cd password-layout
make install-user     # build, copy to ~/.local/bin, enable the user service
make uninstall-user
```

System-wide, which is what a package does:

```
make
sudo make PREFIX=/usr install
```

`make install` only puts files in place (it honours `DESTDIR` and `PREFIX`).
To build the Arch package yourself, from the latest release tag:
`cd packaging/arch && makepkg -si`. Other targets: `make test`, and
`make bench` for the measurements in [BENCHMARKS.md](BENCHMARKS.md).

### Turning it on and off

Installing enables nothing, as usual on Arch. Each user turns the service on
and restarts fcitx5 so it loads the addon (`make install-user` does the first
part itself):

```
systemctl --user enable --now password-layout-tty
password-layout doctor     # says what is still missing
```

To turn it off: `systemctl --user disable --now password-layout-tty`. The
fcitx5 addon is unloaded when fcitx5 restarts after the package is removed.

## From your own scripts

For a password prompt the project does not recognise by itself, a script can
hold Latin for as long as it needs:

```
password-layout enter my-prompt              # Latin now, unless a Latin layout is active
trap 'password-layout leave my-prompt' EXIT  # always give it back
systemd-ask-password "Disk password:"        # anything that asks for a secret
```

- `enter <name>` switches at once, wherever focus is: unlike the terminal
  service, it does not check which window the prompt is in.
- The layout from before comes back after the last holder leaves, the
  service's own (`tty`, `im`) included. A name is lowercase letters, digits
  and `-`, up to 32 characters; `password-layout status` lists who holds Latin.
- A holder that is never given back keeps Latin until `leave` or the end of
  the session, so always pair `enter` with `leave`, as the `trap` above does.

Useful for the cases in [#9](https://github.com/yesm1ke/password-layout/issues/9)
(prompts that draw asterisks), a lock-screen hook while
[#7](https://github.com/yesm1ke/password-layout/issues/7) is open, or just to
check that switching works.

## Troubleshooting

Start with `password-layout doctor`. It checks everything the project depends
on and says what is wrong and what to do:

```
ok    Hyprland: layouts us,ru; Latin: us
ok    terminal service: running
ok    fcitx5: running, addon loaded
warn  fcitx5-qt 5.1.15-1: Qt windows that open with a password field already focused (KeePassXC, the polkit prompt, possibly the lock screen) are not recognised until focus moves
      -> fixed upstream in fcitx5-qt 5.1.16; update once your distribution ships it
note  sudo 1.9.17p2 runs commands in a terminal of its own: sudo's password prompt is recognised, prompts of the programs it runs are not
```

It exits with 1 when something is broken. For more detail:

- `password-layout status` — current layout, who holds Latin (`tty` for
  terminals, `im` for graphical applications), which terminals are at a prompt
  and whether one of them is in the focused window.
- Every actual switch is logged, one line each way:
  `journalctl --user -u password-layout-tty` for terminals, the fcitx5 log for
  graphical applications (`journalctl --user -u omarchy-fcitx5` on Omarchy).
- The service does not start (`systemctl --user status password-layout-tty`
  shows it restarting): part of its sandboxing needs unprivileged user
  namespaces, which some kernels (`linux-hardened`) turn off. Turn that part
  off with an empty drop-in of the same name, then restart the service:
  `mkdir -p ~/.config/systemd/user/password-layout-tty.service.d && touch ~/.config/systemd/user/password-layout-tty.service.d/sandbox-namespaces.conf`.
- What fcitx5 knows about the focused field: `tools/fcitx-watch` from a
  checkout. For the addon's own debug output:
  `busctl --user call org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1 SetLogRule s passwordlayout=5`.

## How it works

A terminal does not tell anyone that it is showing a password prompt. What gives
a prompt away is the mode of its pseudo-terminal: **echo is off while line input
is still on**. Shells and full-screen programs turn both off; ordinary line input
keeps both on. macOS Ghostty uses the same heuristic for its secure input.

While terminals are quiet the service sleeps: it wakes when the kernel
reports, through inotify, that some terminal printed something, and checks
the mode of only the terminals that printed. While a terminal keeps printing,
or a prompt is up, it looks every 200 ms instead (focus can move and a prompt
can end without any output); see [BENCHMARKS.md](BENCHMARKS.md) for what that
costs. A prompt nearly always prints its text, so waking on output is enough;
the one exception is [#6](https://github.com/yesm1ke/password-layout/issues/6).

The layout is switched only if the prompting terminal belongs to the focused
window, which is decided by walking the process tree from the terminal up to the
window's process. Inside tmux that walk ends at the tmux server, so tmux itself is
asked which pane is active and which of its clients — ordinary processes inside
a terminal window — shows that session. When the prompt ends, or focus moves elsewhere, the previous
layout is restored.

Graphical applications are a different story: they do tell the input method
what kind of field has focus, and mark password fields — the same fact macOS
acts on. The fcitx5 addon (`libpasswordlayout.so`) follows focus and that mark.
It never talks to the compositor on fcitx5's own thread, which every key press
goes through; a worker thread applies only the latest wish, so hopping across
fields costs at most one switch and a slow compositor cannot delay typing.

`password-layout` is one binary with subcommands:

| Subcommand | Purpose |
|---|---|
| `watch-tty` | the service: watches terminals for password prompts |
| `enter <who>` / `leave <who>` | for other sources: "Latin is needed" / "no longer needed" |
| `status` | current layout, who is holding Latin, which terminals are at a prompt |
| `doctor` | checks the setup and explains what is wrong |

The terminal service and the addon hold Latin under separate names (`tty` and
`im`). The layout that was active before the first of them asked for Latin is
restored after the last one lets go; it is kept in
`$XDG_RUNTIME_DIR/password-layout/state`.

Source layout: `src/fcitx/` is the fcitx5 addon, `src/tty.*` recognises a prompt and finds the window that owns
the terminal, `src/activity.*` waits for terminal output, `src/compositor.*`
talks to Hyprland, `src/tmux.*` asks tmux about panes and clients,
`src/state.*` tracks who holds Latin and what to restore, `src/doctor.*` checks the setup,
`src/main.cpp` has the subcommands and the service loop.

## Limitations

- **Ghostty tabs and windows**: Ghostty, as Omarchy starts it, runs all its
  windows and tabs in one process, so a prompt left waiting in a background
  Ghostty tab or window counts as being in the focused one and holds Latin
  while any Ghostty window has focus. Separate terminal processes (foot, for
  instance) are not affected. See [#43](https://github.com/yesm1ke/password-layout/issues/43).
- **Terminal multiplexers other than tmux** (zellij, screen): a prompt inside
  them is not recognised, because their server is not a child of the terminal
  window. tmux is handled: a prompt in the visible pane of a session shown in
  the focused window counts, one in a background pane or window does not.
- **Prompts that draw their own asterisks** (`systemd-ask-password`, `sudo` with
  `pwfeedback`) turn line input off as well and look like any full-screen
  program. See [#9](https://github.com/yesm1ke/password-layout/issues/9).
- **`sudo` on a remote machine inside `ssh`**: the local terminal is in raw mode
  at that point. The password prompt of `ssh` itself is recognised.
- **Silent prompts**: echo turned off with no output at all around that moment.
  See [#6](https://github.com/yesm1ke/password-layout/issues/6).
- **Prompts of programs run under `sudo`**: current `sudo` runs the command in
  a pseudo-terminal of its own, owned by root, which an unprivileged service
  cannot inspect. The password prompt of `sudo` itself is recognised; a prompt
  shown by what it runs (`sudo ssh …`, a pacman hook that calls `ssh`, a root
  shell from `sudo -i`) is not. See [#10](https://github.com/yesm1ke/password-layout/issues/10).
- Programs that keep a terminal in "echo off, line input on" for their own
  reasons, such as an Emacs shell buffer, look like a prompt.

## Resource use

The service is meant to run all the time, so it was measured and tuned: about
0.5 MB of its own memory, no wakeups while terminals are quiet, and roughly
0.05% of one core while a terminal prints continuously. Figures and method are
in [BENCHMARKS.md](BENCHMARKS.md).

## Project documents

- [PLAN.md](PLAN.md) — stages and the decisions behind them.
- [STATUS.md](STATUS.md) — what is done, what was verified, what is next.
- [Issues](https://github.com/yesm1ke/password-layout/issues) — known limitations and
  deferred work, one issue each (label `limitation`).
- [BENCHMARKS.md](BENCHMARKS.md) — resource use and how it was measured.
- [CHANGELOG.md](CHANGELOG.md) — what changed in each release.
- [SECURITY.md](SECURITY.md) — what it can see, and how to report a problem.
- [CONTRIBUTING.md](CONTRIBUTING.md) — reporting problems, building, testing.
- [AGENTS.md](AGENTS.md) — a working guide to the repository for coding agents
  and new contributors.

## License

[MIT](LICENSE).
