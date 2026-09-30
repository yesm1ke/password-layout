# Status

Updated with every change. The stages are described in [PLAN.md](PLAN.md).

| Stage | State |
|---|---|
| 1. Terminals | works and is installed (C++, driven by kernel events); awaiting a check by hand |
| 2. Survey of graphical applications | not started |
| 3. fcitx5 addon | not started |
| 4. Browser | depends on stage 2 |
| 5. Finishing | not started |

## Next steps

1. By hand, with a non-Latin layout active: type `sudo -k true` in Ghostty and
   connect somewhere over `ssh` with a password; confirm the layout becomes Latin
   and comes back. This was verified programmatically, not by typing.
2. tmux: a prompt inside tmux is not recognised (confirmed). There is a way: tmux
   reports which pane is on which terminal and which client is attached to which
   session. To be done if tmux matters.
3. Decide about prompts that draw asterisks (see stage 1 in the plan).
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
  recipe in `packaging/arch`. The package was built and its contents
  inspected; it has not been installed with pacman on the development machine,
  which still runs the `make install-user` copy.
- The AUR is pending an account; see [BACKLOG.md](BACKLOG.md).

State at the end of the day: stage 1 is done, installed and running as
`password-layout-tty.service`; 14 tests pass. Open: the check by typing, tmux,
prompts with asterisks, and the backlog: the silent prompt, the AUR, and a
release procedure. The licence is MIT.
