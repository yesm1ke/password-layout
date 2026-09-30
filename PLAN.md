# Plan

The current state of each stage is in [STATUS.md](STATUS.md).

## Goal and scope

While a password is being typed the layout becomes Latin by itself, and the
previous layout returns afterwards — the behaviour macOS has in secure text
fields and, through Secure Keyboard Entry, in terminals.

In scope: application windows in a Hyprland session — terminals (Ghostty, foot
and the like) and the browser.

Out of scope: virtual consoles, the boot and disk-unlock screens, remote
machines, other compositors.

## Overall design

Sources notice a password prompt and call `password-layout enter <who>`, then
`leave <who>` once it is gone. The shared part remembers the layout before the
first `enter` and restores it after the last `leave`. Layouts stay under
Hyprland's control (`us,ru`); switching goes through `switchxkblayout all`.

The language is C++20 with no third-party libraries: the service runs all the
time, so memory and CPU matter, and the fcitx5 addon of stage 3 has to be C++
anyway and can link the same code.

There are two sources because the signals differ:

- **Terminals** announce a password field to nobody. A prompt shows only in the
  mode of the pseudo-terminal.
- **Graphical applications** tell the input method "this is a password field".
  This is the same information macOS acts on when it enables secure input. On
  the development machine an input method is already running: fcitx5, which
  Omarchy ships for Compose sequences.

## Stage 1. Terminals — first priority

The `password-layout watch-tty` service:

- a password prompt is "echo off, line input on" — how `sudo`, `ssh`, `read -s`
  and `getpass` read;
- while terminals are quiet the service sleeps and costs nothing: the kernel
  reports terminal output and new terminals through inotify;
- only terminals that printed are checked (and once more afterwards), plus those
  already at a prompt. There is no pass over every terminal on each tick;
- `sudo`, `ssh` and `getpass` write their prompt through `/dev/tty` rather than
  the terminal's own device. The kernel reports such output without saying which
  terminal it reached, so `/dev/tty` is watched as well and its event means
  "check them all";
- on waking it looks at once and again 50 ms later: some programs turn echo off
  first, others print the prompt first;
- while terminals keep printing or a prompt is up, it looks every 200 ms rather
  than on every write. Polling during a prompt is needed because focus can move,
  and the prompt can end, without any output;
- it switches only if the terminal belongs to the focused window: a process on
  that terminal must descend from the window's process. There is no list of
  "which programs are terminals"; any emulator works;
- Hyprland is asked only while a prompt is up, and the process table is re-read
  only when the focused window or the set of prompts changes.

Left in this stage:

1. A check by hand with real `sudo` and `ssh` in Ghostty.
2. tmux and similar: their server does not descend from the terminal window, so
   a prompt inside tmux is not recognised. tmux itself can report which pane is
   on which terminal and which client is attached to which session.
3. Decide about prompts that draw asterisks (`systemd-ask-password`, `sudo` with
   `pwfeedback`): line input is off as well, which makes them indistinguishable
   from an ordinary full-screen program.

## Stage 2. Survey of graphical applications

Without changing anything, find out which applications set the password flag.
fcitx5 lists its input contexts with their capability flags:

```
busctl --user call org.fcitx.Fcitx5 /controller org.fcitx.Fcitx.Controller1 DebugInfo
```

For the focused context (`focus:1`) the password flag is bit `0x8` of `cap:`.
To check: a password field on a web page in the browser, and Qt applications
such as KeePassXC. The main question was Firefox-based browsers: they might
disable the input method on a password field instead of setting the flag.
Answered: Zen and Chromium both set it (see STATUS.md), so stage 4 is not
needed for either.

## Stage 3. fcitx5 addon

A C++ module, `src/fcitx/passwordlayout.cpp`, loaded by fcitx5. It watches
input-context focus-in, focus-out and capability changes, and wants Latin
exactly while the focused field carries the Password capability. It links
`src/state.*` and `src/compositor.*` directly and holds Latin as `im`.

fcitx5 handles every key press on a single thread, so the addon does no I/O
there: a worker thread talks to Hyprland and applies only the latest wish.
When fcitx5 exits, the worker gives the layout back.

Installing: the packaged install puts the library in fcitx5's own addon
directory; `make install-user` puts it under `~/.local/lib/password-layout` and
names it by absolute path in the addon description, which fcitx5 accepts, so no
change to fcitx5's service is needed. fcitx5 loads the addon when it starts.

Testing without clicking: `tools/fake-field` acts as an application over
fcitx5's D-Bus interface and focuses a plain or a password field.

## Stage 4. Browser, if stage 3 is not enough

Only if the survey shows the browser does not set the flag. Options: a setting
of the browser itself, or an extension that reports focus on
`input[type=password]`. Decided after the survey.

## Stage 5. Finishing

- One-command setup on a new machine.
- Rebuilding the addon after an fcitx5 update.
- The lock screen and the polkit prompt are not application windows; low
  priority, and stage 3 probably covers them anyway.

## Known risks

- A silent prompt (echo turned off with no output at all) is not recognised —
  see [BACKLOG.md](BACKLOG.md).
- The terminal heuristic misfires for programs that keep a terminal in "echo
  off, line input on" themselves — an Emacs shell buffer, for instance.
- `sudo` on a remote machine inside `ssh` is not recognised: the local terminal
  is in raw mode then. Out of scope, but it looks like the same case to a user.
