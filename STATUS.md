# Status

Updated with every change. The stages are described in [PLAN.md](PLAN.md).

| Stage | State |
|---|---|
| 1. Terminals | works and is installed (C++, driven by kernel events); awaiting a check by hand |
| 2. Survey of graphical applications | browsers done; Qt: blocked by an upstream fcitx5-qt bug (backlog) |
| 3. fcitx5 addon | works in Zen and Chromium (checked by hand); released in 0.2.0 |
| 4. Browser | not needed: Zen and Chromium mark password fields |
| 5. Finishing | setup without manual steps, Latin layout by name, logging (0.4.0) |

## Next steps

1. By hand, with a non-Latin layout active: connect somewhere over `ssh` with a
   password; confirm the layout becomes Latin and comes back. `sudo` was
   checked by typing on 2026-09-30.
2. tmux: a prompt inside tmux is not recognised (confirmed). There is a way: tmux
   reports which pane is on which terminal and which client is attached to which
   session. To be done if tmux matters.
3. Prompts that draw asterisks: deferred to the backlog.
4. Start the survey of stage 2.

Deferred problems are in [BACKLOG.md](BACKLOG.md), resource measurements in
[BENCHMARKS.md](BENCHMARKS.md).

## Journal

### 2026-09-30

Repository created, stage 1 done.

#### First prototype, in Python

- A shared part (`enter` / `leave` / `status`) and a terminal watcher
  (`watch-tty`) that polled every pseudo-terminal five times a second.
- Installed as the user service `password-layout-tty.service`.

Verified on the live system, with a Russian layout active:

| Situation | Layout |
|---|---|
| before the prompt | Russian |
| `read -s` waiting for input in the focused terminal | Latin |
| prompt finished | Russian |
| `less` running | Russian, untouched |
| a real `sudo -k true` in a new foot window | Latin, Russian again once the window closed |
| `read -s` in a new Ghostty window (a separate process) | Latin, then Russian |
| `read -s` inside a tmux session | **not recognised** |

The windows and prompts were opened programmatically; nobody typed a password on
the keyboard, hence step 1 above.

Terminal modes, the measurement the heuristic rests on:

| Program | Echo | Line input | Recognised |
|---|---|---|---|
| `sudo` | off | on | yes |
| `read -s` in bash | off | on | yes |
| Python `getpass` | off | on | yes |
| ordinary input (`cat`) | on | on | no, and should not be |
| bash waiting for a command, `less` | off | off | no, and should not be |
| `systemd-ask-password` | off | off | no |

Facts about the development machine that matter for the later stages:

- The default terminal is Ghostty 1.3.1. It has the same password-prompt
  detection, but only on macOS, where it drives secure input; on Linux it is not
  exposed.
- fcitx5 5.1.22 runs as `omarchy-fcitx5.service` with only `keyboard-us` in its
  profile; the `us,ru` layouts are held by Hyprland.
- `gcc`, `make` and the fcitx5 headers are available for building the addon;
  cmake is not installed.
- `switchxkblayout` only with `all`: because of fcitx5's virtual keyboard,
  `current` reaches the wrong device.

#### Rewritten in C++

The Python version used too much for something that runs all the time (10 MB of
memory); the polling logic stayed the same.

- Sources in `src/`, tests in `tests/test.cpp`.
- Hyprland's replies are taken as JSON and parsed by looking up integers by key
  (`pid`, `active_layout_index`); there is a test for a window title with quotes
  in it.
- The live check was repeated: a real `sudo -k true` in a new foot window gives
  Latin, and Russian again once the window closes.

The first C++ build spent 3% of a core while a prompt was pending, because it
re-read the whole process table on every tick. It is now re-read only when the
set of prompts or the focused window changes.

#### Optimisation: where the CPU time went

One polling tick cost 113 us of CPU at 5 ticks per second. The pass over the
terminals takes under 10 us in a hot loop, so the number of wakeups was the
thing to reduce.

The service no longer wakes on a timer. It waits for an inotify event about
output on any terminal (verified: the kernel reports both output and the
appearance of a new terminal).

Measured over 40 seconds with no work going on in terminals: 29 wakeups and
2.2 ms of CPU time, where polling would have taken 200 wakeups and about 22 ms.
Not zero, because some terminal did print during that time; each separate burst
of output costs three wakeups.

#### Optimisation of the "terminal is printing" case

The question was why this case seemed to cost ~0.07% after the move to inotify
instead of ~0.05%. Three variants of the loop run side by side under one load
showed that almost nothing had changed: reading the events adds about 8 us per
tick (5%), and the rest of the difference between the two earlier measurements
was different conditions. It also showed where a tick really goes: the earlier
conclusion "100 us is the price of a wakeup" was inaccurate, it is the price of
a cold pass — listing `/dev/pts` and opening every terminal right after a sleep.

Done:

- `/dev/pts` is no longer listed on every tick; the list of terminals is kept up
  to date from inotify events;
- only the terminals that printed are checked, not all of them.

This took a wakeup under load from 136 us to 84 us.

A flaw in the inotify approach itself was found and fixed along the way: `sudo`,
`ssh` and `getpass` write their prompt through `/dev/tty`, and the event arrives
for `/dev/tty`, not for the terminal. It had been masked by every terminal being
checked on any activity. With all terminals quiet and a prompt appearing more
than a quarter of a second after Enter (typical for `ssh`), it would have gone
unrecognised. `/dev/tty` is now watched separately.

Verified on real pseudo-terminals, with the prompt appearing 3 s after the last
output: `read -s -p`, Python `getpass`, `sudo -k true` — Latin comes on and goes
back.

A new limitation, the silent prompt, is recorded in [BACKLOG.md](BACKLOG.md).

#### Measurements collected in one place

The measurement tools moved to `bench/` with a `make bench` target; the summary
tables, method and caveats are in [BENCHMARKS.md](BENCHMARKS.md).

#### Prepared for publication

- Stray Python bytecode from the prototype removed from the tree.
- All documents translated to English; the README now describes the macOS
  behaviour this project reproduces.

#### Published

- The history was squashed into a single commit, authored with the GitHub
  no-reply address. What led here is recorded in this journal instead.
- The repository is public: https://github.com/yesm1ke/password-layout
- `make install` became packageable (`DESTDIR`, `PREFIX`, files only); the
  per-user install that enables the service is `make install-user`.
- Three tests that failed when another terminal on the machine printed at the
  wrong moment were fixed; the suite then passed 40 runs in a row.
- Release v0.1.0 with a prebuilt Arch package, built from the release tag by the
  recipe in `packaging/arch`. The package was built, its contents
  inspected, and it installs into a scratch root.
- The first install instruction was wrong: `pacman -U <url>` fails with a 404 on
  the `.sig` file, because pacman requires a signature for a package given as a
  URL and the release is unsigned. The instruction now downloads the file first.
  Installed that way on the development machine: `pacman -Q` shows 0.1.0-1 and
  the service runs from `/usr/bin`, replacing the `make install-user` copy.
- The AUR is pending an account; see [BACKLOG.md](BACKLOG.md).

#### Found in use: prompts under sudo

A pacman hook asked for an ssh key passphrase and the layout did not switch.
Cause: `sudo` runs its command in a root-owned pseudo-terminal that the service
cannot open. Recorded in [BACKLOG.md](BACKLOG.md) with the options; the README
limitation was rewritten to say this plainly, as it covers far more than the
`sudo -i` it used to mention.

#### Decision: prompts under sudo stay in the backlog

Weighed on 2026-09-30: a setgid-`tty` helper would add roughly 200–300 lines and
a privileged binary, cost 0.25–0.5% of a core while a command under `sudo`
prints (estimated, not measured), and help only with passwords typed inside
commands run under `sudo` — `sudo`'s own prompt already works, and the case that
triggered this was fixed at its source. Password fields in graphical
applications come first.

#### Stage 2 started

`tools/fcitx-watch` prints, on every change, the focused window and what fcitx5
knows about the focused field, with the password flag decoded. First finding
before any clicking: Chromium runs without `--enable-wayland-ime`, so it does
not talk to fcitx5 at all.

#### Survey: Zen marks password fields

`tools/fcitx-watch`, clicking through the test page
`data:text/html,<input placeholder=text> <input type=password placeholder=password>`
and https://github.com/login in Zen, without restarting the browser:

| Field in Zen | What fcitx5 sees (`cap`) | Flags |
|---|---|---|
| ordinary text field | `72` | none |
| password field | `100000007a` | **password** (bit 3), sensitive (bit 36) |
| page area with no field | no focused input field | — |

So Zen (Firefox-based, frontend `wayland_v2`, i.e. text-input on Wayland) does
not switch the input method off in password fields, it marks them — the
signal stage 3 needs. Stage 4 is not needed for Zen. The flag appears the
moment the field gets focus and is gone when focus leaves it.

#### Survey: Chromium marks them too, with no extra flags

The expectation was that Chromium needs `--enable-wayland-ime` to talk to an
input method at all. It does not: Chromium as installed, started with only
`--ozone-platform=wayland`, gave the same picture as Zen:

| Field in Chromium | `cap` | Flags |
|---|---|---|
| address bar | `1072` | url |
| ordinary text field | `72` / `80072` | none |
| password field | `100000007a` | **password**, sensitive |

(The earlier "no focused input field" in a Chromium web-app window simply had
no field clicked.)

Both browsers are covered by the fcitx5 route. Still to survey: Telegram (Qt),
the polkit prompt.

#### Stage 3: the fcitx5 addon

`src/fcitx/passwordlayout.cpp`, built by `make` when fcitx5's development files
are present, installed by both `make install` and `make install-user`.

- fcitx5 5.1.22 loads it ("Loaded addon passwordlayout"). An absolute path in
  `Library=` works, so the per-user install needs no drop-in for fcitx5's
  service, contrary to the plan.
- Checked with `tools/fake-field`, which acts as an application over fcitx5's
  D-Bus interface, with a Russian layout active:

| Situation | Layout |
|---|---|
| password field focused | Latin, held as `im` |
| focus leaves it | Russian |
| plain field focused | Russian, untouched |
| a focused plain field turns into a password field ("show password" in reverse) | Latin |
| and back to plain | Russian |
| password field, then quickly a plain one | Russian |
| password field destroyed while focused (application closes) | Russian |

Checked by hand afterwards: real password fields in Zen and Chromium switch to
Latin and back. Not yet checked: the packaged
install (the addon is not in the 0.1.0 package). On the development machine the
addon is loaded from a hand-made description pointing at `build/`.

#### Qt applications: an upstream bug

Reported by hand: KeePassXC opens with its password field focused and the
layout does not switch; moving focus between fields makes it work. Traced with
the addon's new debug log (off by default; `SetLogRule passwordlayout=5` over
fcitx5's D-Bus controller turns it on) and `dbus-monitor`: the Qt input-method
module sends only base capabilities for the first focus of a window, without the
password hint. The Omarchy polkit prompt shows the same. It is fcitx5-qt issue
#85, fixed upstream in 5.1.16; Arch has 5.1.15. Details in
[BACKLOG.md](BACKLOG.md).

Tried along the way without success: `QT_IM_MODULE=wayland` (the field then gets
no input context at all).

#### Release 0.2.0

- The fcitx5 addon is in the package: `/usr/lib/fcitx5/libpasswordlayout.so` and
  `/usr/share/fcitx5/addon/passwordlayout.conf`. The package now depends on
  fcitx5, and its install script says to restart fcitx5.
- A binary package that had slipped into the repository (downloaded into the
  checkout, then committed with everything else) was removed from every commit
  on `main`; built packages are now ignored anywhere in the tree.
- Installed on the development machine from the release: `pacman -Q` shows
  0.2.0-1, fcitx5 maps `/usr/lib/fcitx5/libpasswordlayout.so`, both services are
  active, and a password field still switches the layout. The hand-made addon
  description pointing at `build/` is gone.
- Known problem, noted in the release: Qt applications that open with a password
  field already focused (KeePassXC, the polkit prompt) until Arch ships
  fcitx5-qt 5.1.16.

#### tmux

A prompt in a tmux pane was recognised as a prompt but never as "in the focused
window": the pane's processes descend from the tmux server, not from the
terminal window. Now, when the walk up the process tree ends at a tmux server,
`src/tmux.*` finds that server's socket and asks it which pane is active and
which clients show which session; a client is a process inside a terminal
window, so the usual walk works from there. tmux is asked again only when a
client's terminal redraws.

Checked:

- Tests (17, 20 runs in a row without a failure), including one with a private
  tmux server, a client attached from the test, and a switch to another tmux
  window. The first version failed on this machine's `base-index 1`; windows
  are now named by their ids.
- Live, with the new build standing in for the service and a Russian layout: a
  prompt in tmux in a new Ghostty window gives Latin; switching tmux to another
  window gives Russian; switching back gives Latin; closing tmux gives Russian.
- Cost: see BENCHMARKS.md — no measurable difference while a prompt waits.

Released in 0.3.0 and checked by hand: `sudo -k true` in a tmux pane switches
the layout to Latin and back.

#### Release 0.3.0

tmux support; nothing else changed. Built from the tag and published like 0.2.0.
Installed on the development machine: `pacman -Q` shows 0.3.0-1 and the
restarted service runs the new binary.

#### Setup without manual steps, Latin layout by name

Asked for by the user as the next most valuable things for other people.

- The package ships `graphical-session.target.wants/password-layout-tty.service`,
  so the service is enabled for every user, the way gnupg ships its sockets.
  The install script starts it and restarts fcitx5 in the sessions of
  logged-in users (`systemctl --user --machine=<user>@`; fcitx5 through
  `omarchy-fcitx5.service` when it exists, otherwise through its D-Bus
  `Restart`), restarts both on upgrade, and stops/unloads on removal. Checked
  by running the upgrade hook as the user: the watcher and fcitx5 both came
  back with new process ids and the addon loaded. Checked as root through
  pacman with 0.4.0 (below); the D-Bus branch for systems without Omarchy's
  unit is not checked.
- The Latin layout is the first one not in Omarchy's list of non-Latin layout
  codes (a `latin` variant counts as Latin), read from Hyprland's `layout` and
  `variant` fields. No Latin layout means no switching. Checked live by setting
  `kb_layout` to `ru,us` through `hyprctl eval`: Latin was layout 1 and the
  switch went there and back.
- Every actual switch is logged, e.g. `password-layout: Latin for tty (layout
  1, was 0)` and `password-layout: layout 0 restored (tty left)`. The state
  file now records whether Latin was switched to at all (`-1` when not).
- README: requirements, a two-line install, a troubleshooting section.
- Backlog: Electron password managers, prompts with asterisks, and a note that
  the Omarchy lock screen is probably hit by the fcitx5-qt bug.
- Tests: 21.

#### Release 0.4.0

Setup without manual steps, Latin layout by name, switch logging. Built from the
tag and published like the earlier releases.

Installed on the development machine with `pacman -U` as root: the install
script restarted the watcher and fcitx5 in the running session (both came back
at the moment of the upgrade, the addon loaded), with nothing done by hand. A
simulated password field then logged `Latin for im (layout 0, was 1)` and
`layout 1 restored (im left)` in fcitx5's journal.

#### Which layout: the macOS rule

The user asked for the macOS logic: in a secure field only ASCII-capable input
sources are allowed; if the current one already is, nothing changes, otherwise
the last used ASCII-capable one is chosen, and the previous source returns
afterwards. Until now the first Latin layout was always chosen, which e.g. took
a German user from `de` to `us`.

- `State::enter`: no switch when the active layout is Latin; otherwise the last
  Latin layout in use (`$XDG_RUNTIME_DIR/password-layout/last-latin`), falling
  back to the first Latin one.
- The terminal watcher follows layout switches through Hyprland's event socket
  (`.socket2.sock`, `activelayout` events) and records the Latin ones; its idle
  wait also wakes on that socket, which only happens when the layout changes.
  The fcitx5 addon reads the same file, so it follows the rule too as long as
  the watcher runs.
- `password-layout status` shows the remembered layout.
- Checked live with `kb_layout = us,de,ru` and the new build standing in for
  the service: German active — no switch; German, then Russian — the password
  gets German; US, then Russian — it gets US; Russian comes back each time.
  Layouts restored to `us,ru` afterwards.
- Tests: 24.

#### The description around the rule

README, PLAN and the package description now start from the rule and the goal
of making typing a password painless: Latin while a password is typed, the last
used Latin layout if the current one is not Latin, the previous one back after.
macOS is mentioned only as the logic users know, without its API. The rule
moved from "How it works" to its own section, together with what the project
does not do (fcitx5 input methods are not touched, the keyboard is not guarded).

#### Release 0.5.0

The layout is chosen by the rule: no switch when the active layout is Latin,
otherwise the last used Latin one. Built from the tag and published like the
earlier releases.

Installed on the development machine with `pacman -U`: the install script
restarted the watcher and fcitx5 (the addon unloaded and loaded again), and
`status` shows the remembered Latin layout. The `sudo` for that install was a
real password typed by hand in the terminal with Russian active: the journal
has `Latin for tty (layout 0, was 1)` while it was typed and `layout 1
restored (tty left)` after — the first check by typing, for `sudo` (by 0.4.0,
which was still running). `ssh` has not been typed by hand yet.

State at the end of the day: stage 1 is done, installed and running as
`password-layout-tty.service`; 14 tests pass. Open: the check by typing, and the backlog: the silent prompt, prompts under
`sudo`, the AUR, and a release procedure. The licence is MIT.
