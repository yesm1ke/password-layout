# Security

password-layout sits next to your passwords: its fcitx5 addon runs inside the
input method that every key press goes through, and its service watches your
terminals. This is what each part can and cannot see, so you can judge it
without reading the code.

## What it sees

**The fcitx5 addon** (`src/fcitx/passwordlayout.cpp`) subscribes to exactly
three fcitx5 events: a field gained focus, lost focus, or changed its
capability flags. From them it learns one fact: whether the focused field is a
password field. It does not subscribe to key events, so it never sees what you
type. `tools/check-addon-events`, run by CI on every change, fails if the
addon's source names any other event type or a key event. It is a guard
against an accidental change, not a proof: read
`src/fcitx/passwordlayout.cpp` (about 160 lines) to check for yourself.

**The terminal service** (`password-layout watch-tty`) reads, for terminals that
just printed something:

- the terminal mode (`tcgetattr`): whether echo and line input are on. Not the
  text on the screen, not what is typed;
- the process table in `/proc` (parent, terminal) to find which window a
  terminal belongs to;
- the list of panes and clients from tmux, when a prompt is inside tmux.

It never writes to a terminal, opens no network connections and runs as you,
with no extra privileges. systemd sandboxes it on top of that: the file system
is read-only apart from its own runtime directory and there is no network at
all (these two need user namespaces, see "Troubleshooting" in the README), only
Unix sockets can be opened, and privileged system calls are refused
(`systemd/password-layout-tty.service.in`,
`systemd/sandbox-namespaces.conf`). Check it with
`systemd-analyze --user security password-layout-tty`.

**Both** talk to Hyprland over its socket only to read the keyboard layout and
the focused window, and to switch the layout.

## What it stores and logs

- `$XDG_RUNTIME_DIR/password-layout/` (private to you, gone at logout): the
  layout to restore, who currently holds Latin (`tty` or `im`) and the last
  Latin layout used, as layout numbers.
- The log has one line per switch, with layout numbers and `tty` or `im`.
  Nothing about the password, the window or the program asking for it.
- Only if you turn on the addon's debug log by hand (README, "Troubleshooting")
  does it also log the program name of each focused field and whether it is a
  password field.

## What it does not protect against

It switches the keyboard layout; it is not a secure-input feature. Other
programs running as you can still read the keyboard, as on any Wayland desktop
without extra isolation. A wrongly detected prompt only means a wrong layout,
never a leaked character.

## Verifying a release

There is no release at the moment; releases are being reworked
([#26](https://github.com/yesm1ke/password-layout/issues/26)). Build from a checkout and read the code you build.

## Reporting a problem

Use GitHub's private reporting: the "Report a vulnerability" button on the
repository's Security tab. Please do not open a public issue for a security
problem.
