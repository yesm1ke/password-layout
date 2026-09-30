# password-layout

Switches the keyboard layout to Latin while you type a password, and puts your
layout back afterwards. For [Hyprland](https://hyprland.org/).

If you type in a non-Latin layout, you know the routine: `sudo` asks for a
password, you type it blind, it is rejected, and only then do you notice the
layout was wrong.

## The macOS behaviour this copies

macOS solves this at the system level. When focus enters a secure text field
(a password box), the system turns on *Secure Event Input* and limits the
keyboard to ASCII-capable input sources: if you were typing in Russian, Greek
or Japanese, the input source flips to a Latin one on its own. When focus leaves
the field, the input source you had before comes back. Terminals take part too:
Terminal.app has *Secure Keyboard Entry*, and Ghostty on macOS detects a password
prompt in the shell and enables secure input for as long as it is up.

Linux has no equivalent. `password-layout` reproduces the visible half of that
mechanism — Latin while the password is being typed, the previous layout
afterwards — for a Hyprland session.

## What works today

- **Terminals**: password prompts of `sudo`, `ssh`, `read -s`, and anything else
  that reads a password the way `getpass` does. Any terminal emulator (tested in
  Ghostty and foot); there is no list of supported terminals.
- **Browsers**: password fields in Zen (Firefox-based) and Chromium, through an
  addon for the fcitx5 input method. Other applications work if they mark their
  password fields for the input method, as these browsers do; Qt applications
  are not surveyed yet.

The scope is deliberately narrow: one person at a real keyboard in a Hyprland
session. Virtual consoles, the boot and disk-unlock screens, remote machines and
other compositors are out of scope.

## Requirements

- Hyprland, with the Latin layout **first** in `kb_layout` (for example
  `us,ru`). Layout index 0 is what gets selected for passwords.
- `g++` with C++20 support and `make`. There are no library dependencies.
- For password fields in graphical applications: fcitx5 running as the input
  method (Omarchy starts it by default), and its development files at build
  time. Without them only the terminal part is built.
- systemd, for the user service.

## Install

On Arch-based systems (Arch, Omarchy, …) install the package from the latest
[release](https://github.com/yesm1ke/password-layout/releases) and enable the
service for your user:

```
curl -LO https://github.com/yesm1ke/password-layout/releases/download/v0.2.0/password-layout-0.2.0-1-x86_64.pkg.tar.zst
sudo pacman -U password-layout-0.2.0-1-x86_64.pkg.tar.zst
systemctl --user enable --now password-layout-tty.service
systemctl --user restart omarchy-fcitx5.service    # on Omarchy; elsewhere: fcitx5 -rd
```

The last line restarts fcitx5 so it loads the addon for password fields in
graphical applications.

Download first: the package is not signed, and pacman insists on a signature
when it is handed a URL, but not for a local file.

The attached package is built for x86_64. To build it yourself, on any
architecture, use the recipe in `packaging/arch`:

```
git clone https://github.com/yesm1ke/password-layout
cd password-layout/packaging/arch
makepkg -si
```

The package is not in the AUR yet; see [BACKLOG.md](BACKLOG.md).

From a checkout, for the current user only (no root needed):

```
make install-user     # build, copy to ~/.local/bin, enable the user service
make uninstall-user
```

System-wide, which is what a package does:

```
make
sudo make PREFIX=/usr install
systemctl --user enable --now password-layout-tty.service
```

`make install` only puts files in place (it honours `DESTDIR` and `PREFIX`);
the service is a per-user one, so each user enables it themselves. It starts
with the graphical session.

Other targets: `make test`, and `make bench` for the measurements in
[BENCHMARKS.md](BENCHMARKS.md).

## How it works

A terminal does not tell anyone that it is showing a password prompt. What gives
a prompt away is the mode of its pseudo-terminal: **echo is off while line input
is still on**. Shells and full-screen programs turn both off; ordinary line input
keeps both on. macOS Ghostty uses the same heuristic for its secure input.

The service does not poll. It sleeps until the kernel reports, through inotify,
that some terminal printed something, and then checks the mode of only the
terminals that printed. A prompt nearly always prints its text, so this is
enough; the one exception is described in [BACKLOG.md](BACKLOG.md).

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

The terminal service and the addon hold Latin under separate names (`tty` and
`im`). The layout that was active before the first of them asked for Latin is
restored after the last one lets go; it is kept in
`$XDG_RUNTIME_DIR/password-layout/state`.

Source layout: `src/fcitx/` is the fcitx5 addon, `src/tty.*` recognises a prompt and finds the window that owns
the terminal, `src/activity.*` waits for terminal output, `src/compositor.*`
talks to Hyprland, `src/tmux.*` asks tmux about panes and clients,
`src/state.*` tracks who holds Latin and what to restore,
`src/main.cpp` has the subcommands and the service loop.

## Limitations

- **Terminal multiplexers other than tmux** (zellij, screen): a prompt inside
  them is not recognised, because their server is not a child of the terminal
  window. tmux is handled: a prompt in the visible pane of a session shown in
  the focused window counts, one in a background pane or window does not.
- **Prompts that draw their own asterisks** (`systemd-ask-password`, `sudo` with
  `pwfeedback`) turn line input off as well and look like any full-screen
  program.
- **`sudo` on a remote machine inside `ssh`**: the local terminal is in raw mode
  at that point. The password prompt of `ssh` itself is recognised.
- **Silent prompts**: echo turned off with no output at all around that moment.
  See [BACKLOG.md](BACKLOG.md).
- **Prompts of programs run under `sudo`**: current `sudo` runs the command in
  a pseudo-terminal of its own, owned by root, which an unprivileged service
  cannot inspect. The password prompt of `sudo` itself is recognised; a prompt
  shown by what it runs (`sudo ssh …`, a pacman hook that calls `ssh`, a root
  shell from `sudo -i`) is not. See [BACKLOG.md](BACKLOG.md).
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
- [BACKLOG.md](BACKLOG.md) — deferred problems.
- [BENCHMARKS.md](BENCHMARKS.md) — resource use and how it was measured.

## License

[MIT](LICENSE).
