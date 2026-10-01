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
type. `tools/check-addon-events` checks this on every release, and the release
fails if the addon starts listening to anything else.

**The terminal service** (`password-layout watch-tty`) reads, for terminals that
just printed something:

- the terminal mode (`tcgetattr`): whether echo and line input are on. Not the
  text on the screen, not what is typed;
- the process table in `/proc` (parent, terminal) to find which window a
  terminal belongs to;
- the list of panes and clients from tmux, when a prompt is inside tmux.

It never writes to a terminal, opens no network connections and runs as you,
with no extra privileges.

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

Releases are built from the tag by GitHub Actions, not on a personal machine.
Each package is GPG-signed and has a GitHub build attestation that ties it to
the workflow run and the commit; `SHA256SUMS` is signed as well. How to check
both is in the README, under "Install".

## Reporting a problem

Use GitHub's private reporting: the "Report a vulnerability" button on the
repository's Security tab. Please do not open a public issue for a security
problem.
