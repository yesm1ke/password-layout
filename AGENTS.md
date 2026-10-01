# Working on password-layout

For coding agents and new contributors: what you need to start working without
reading the whole history. Humans start with [README.md](README.md); the rules
for pull requests are in [CONTRIBUTING.md](CONTRIBUTING.md).

## What it is

While a password is typed in a Hyprland session, the keyboard layout is Latin;
afterwards the previous layout comes back. Two sources notice a password:

- **terminals**: the user service `password-layout watch-tty` sees a pty in
  "echo off, line input on" mode, owned by the focused window (tmux panes
  included);
- **graphical applications**: the fcitx5 addon sees a focused field with the
  Password capability.

Both call into one shared state (`src/state.*`), which switches through
Hyprland and restores the old layout after the last holder leaves.

## Where things are

| Path | What |
|---|---|
| `src/main.cpp` | subcommands (`watch-tty`, `enter`, `leave`, `status`, `doctor`) and the service loop |
| `src/tty.*` | prompt detection by terminal mode; process tree; which window owns a pty |
| `src/activity.*` | inotify on `/dev/pts` and `/dev/tty`: wakes the service only when terminals print |
| `src/tmux.*` | asks tmux which pane is visible in which client |
| `src/compositor.*` | Hyprland socket: layouts, switching, focused window; minimal JSON reading; which layouts are Latin (measured tables, `tests/xkb-latin.tsv`, `tools/xkb-latin.c`) |
| `src/state.*` | holders (`tty`, `im`) and the layout to restore, in `$XDG_RUNTIME_DIR/password-layout/` |
| `src/doctor.*` | `password-layout doctor`: checks the setup, explains problems |
| `src/fcitx/` | the fcitx5 addon (`libpasswordlayout.so`) and its description template |
| `systemd/` | the user unit and its sandbox drop-in |
| `tests/test.cpp` | all tests, one binary, no framework |
| `bench/` | resource measurements (`make bench`; results in the [wiki](https://github.com/yesm1ke/password-layout/wiki/Resource-use)) |
| `tools/` | `release`, `check-version`, `release-notes`, `check-addon-events` (release/CI); `fake-field`, `fcitx-watch` (live checks); `setup-release-key` (maintainer only) |
| `packaging/arch/` | PKGBUILD (the only place the version is kept), install script, the release public key |
| `.github/workflows/` | `ci.yml` (every pull request and push to main), `build.yml` (tests, package, smoke test; shared), `release.yml` (`v*` tags: sign, attest, publish) |

Plans, tasks and status live on GitHub, not in the repository: the pinned
plan issue, milestones, and one issue per task (known limitations carry the
label `limitation`). Background is in the [wiki](https://github.com/yesm1ke/password-layout/wiki): how it works, design
decisions, measurements, the journal of the first two days. In the
repository: [CHANGELOG.md](CHANGELOG.md), [SECURITY.md](SECURITY.md).

## Build and test

```
make                     # build/password-layout; build/libpasswordlayout.so if fcitx5 headers exist
make test                # all tests; some use real ptys and a private tmux server
CXXFLAGS="-O2 -Werror" make test
make clean && CXXFLAGS="-O1 -g -fsanitize=address,undefined" LDFLAGS="-fsanitize=address,undefined" make test
tools/check-addon-events
```

Pass `CXXFLAGS`/`LDFLAGS` through the environment, not as `make` arguments:
the Makefile appends `-std=c++20 -Isrc` to them. Run the tests with
`env -u TERM` as well; CI has no `TERM`. Needs g++ with C++20, make, tmux, and
fcitx5 development files for the addon. C++20, no third-party libraries:
keep it that way (the service runs all the time; memory and wakeups matter).

## Checking on a live desktop

Unit tests do not cover the desktop; most changes need a live check in a
Hyprland session, with a non-Latin layout active.

- `password-layout doctor` and `password-layout status`.
- A terminal prompt: open a terminal running `read -s -p "Password: " x`
  (for example `foot -e bash -c '…'`), check that `status` shows the holder
  `tty` and the layout switched, close it.
- A password field: `tools/fake-field --password 2` focuses a fake field over
  fcitx5's D-Bus; the holder becomes `im`.
- What fcitx5 sees from a real application: `tools/fcitx-watch`.
- Logs: `journalctl --user -u password-layout-tty`, and the fcitx5 log
  (`journalctl --user -u omarchy-fcitx5` on Omarchy) for the addon.
- To try a build as the running service without installing it, add a
  temporary drop-in in `~/.config/systemd/user/password-layout-tty.service.d/`
  that overrides `ExecStart=`, and remove it afterwards.

## Rules that are easy to break

- **The addon never looks at key events.** It watches only focus and
  capability events. SECURITY.md promises this; `tools/check-addon-events`
  fails the release otherwise.
- **The service makes no network requests and never writes to a terminal.**
  The sandbox (`PrivateNetwork`, `RestrictAddressFamilies=AF_UNIX`) enforces the
  first.
- **Switch with `switchxkblayout all`**, never `current`: with fcitx5 running,
  `current` hits the wrong device.
- **Holder names** are `[a-z0-9-]{1,32}`; they are lines of the state file.
- **Sandbox options that need user namespaces** go in
  `systemd/sandbox-namespaces.conf`, not in the unit, so that kernels without
  them can mask just that file. Inside a user namespace another process's
  `/proc/<pid>/exe` is not accessible (see `clientFor` in `src/tmux.cpp`).
- **The addon asks fcitx5 for its series** (`core:5.1.0`), not the exact build
  version: fcitx5 silently skips an addon that asks for a newer core.
- **Issue forms**: GitHub rejects a form whose field labels contain words such
  as "password"; use them in descriptions only.

## Workflow

- Work on a branch and open a pull request; main accepts nothing else and
  requires CI (`build / test`, `build / package`, `build / smoke`, `emails`).
- Merge with "Rebase and merge" only (`gh pr merge --rebase`): merge
  commits are disabled, and once carried a personal e-mail address that took
  a history rewrite to remove. CI fails on any address other than the GitHub
  no-reply ones.
- Every change has an issue; record in the pull request what was done and how
  it was verified, including what was not. User-visible changes also get a
  line in CHANGELOG.md's `unreleased` section.
- Everything in the repository is in English.
- Stage files by explicit path, never `git add -A`: release packages
  (`*.pkg.tar.zst`) get downloaded into the checkout and must not be
  committed.
- The repository is public: no personal data (names, e-mail addresses, home
  paths, hostnames, IP addresses, tokens) in commits, code or issues.

## Releases

Only the maintainer releases, with two runs of `tools/release X.Y.Z` on main:
the first opens a pull request that sets the version and dates the CHANGELOG
section; after it is merged, the second tags that commit, but only if CI
passed on it. `release.yml` then builds from the tag, signs with the release
key (a secret of the `release` environment), attests and publishes. A failed
release tag is never reused; the next patch version is used instead. Tags
`v*` can be created only by repository admins.
