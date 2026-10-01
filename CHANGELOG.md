# Changelog

What changed for someone who installs the package. Work in progress is in the
[issues](https://github.com/yesm1ke/password-layout/issues).

The releases below, up to 0.6.2, were withdrawn on 2026-10-01: the history was
rewritten and they no longer match it. Their entries stay as a record.

## 0.7.1 — unreleased

- Which layouts count as Latin is now measured, not guessed by layout code:
  85 of xkeyboard-config's 597 layouts and variants were wrong before. Urdu,
  Egyptian and Moroccan Arabic, Uzbek, Burmese, `us(rus)` and others no longer
  pass for Latin, so a password is no longer typed in them; Indian English and
  the Latin Kurdish variants no longer cause a needless switch.
- `password-layout enter`/`leave` are documented for holding Latin from your
  own scripts.
- `doctor` warns when keyboard layouts are switched by fcitx5 instead of
  Hyprland, a setup that is not handled.

## 0.7.0 — 2026-10-01

- Fixed: if fcitx5 died while a password field had focus, terminal prompts
  stopped switching the layout until the next login. fcitx5 now clears its
  leftover on start.
- Fixed: a restarting Hyprland could kill the terminal service; a hung one
  could hang it, and fcitx5's exit with it. Requests now time out after a
  second.
- Fixed: a half-written state file could restore the first layout instead of
  yours. The state is written atomically and read strictly.
- The layout list comes from what most keyboards have, not from whichever
  device Hyprland lists first.
- The package no longer enables the service for every user, nor starts or
  restarts anything in running sessions (other users' fcitx5 included). It
  prints what to run; `password-layout doctor` says what is missing. **When
  upgrading from 0.6 or earlier, enable the service once:**
  `systemctl --user enable password-layout-tty`.
- `doctor` notices a service or addon still running from a package that has
  been updated since, and a service that runs but is not enabled.

## 0.6.2 — 2026-10-01

- Fixed: the fcitx5 addon from the 0.6.1 package asked for fcitx5 5.1.23, the
  version it was built with, and fcitx5 5.1.22 skipped it without a word, so
  password fields in browsers no longer switched the layout. The addon now
  asks for the 5.1 series.
- `doctor` names this cause when it sees it: the running fcitx5 is older than
  the one the addon asks for.

## 0.6.1 — 2026-10-01

The first release built by CI. The tag v0.6.0 exists but was never published:
its CI run failed on a test that needed a terminal type the CI container does
not set.

- Releases are built, tested and published by GitHub Actions from the tag,
  instead of on the developer's machine.
- Each package is GPG-signed and carries a GitHub build attestation, so you can
  check that it was built from this repository; `SHA256SUMS` is signed too.
  See "Install" in the README.
- A fixed download link for the latest package:
  `releases/latest/download/password-layout-x86_64.pkg.tar.zst`.
- The PKGBUILD runs the test suite (`check()`).
- `SECURITY.md`: what the service and the addon can see, and how to report a
  problem.
- The terminal service runs sandboxed: read-only file system, no network, no
  privileged system calls (`systemd-analyze --user security` rates it 2.9
  instead of 9.4). The part that needs user namespaces is a separate drop-in;
  see README, "Troubleshooting", if your kernel has them turned off.
- `password-layout doctor`: one command that checks Hyprland and its Latin
  layout, the service, fcitx5 and the addon, and known problems of fcitx5-qt
  and sudo, and says what to do.
- `enter` and `leave` accept only holder names of lowercase letters, digits and
  `-`.

## 0.5.0 — 2026-09-30

- The macOS rule for choosing the layout: a Latin layout that is already active
  is left alone, otherwise the Latin layout used last is switched to.

## 0.4.0 — 2026-09-30

- No manual steps after installing: the service is enabled for every user, and
  the install script starts it and restarts fcitx5 in running sessions.
- The Latin layout is found by name instead of assumed to be the first.
- Every switch is logged.

## 0.3.0 — 2026-09-30

- Password prompts inside tmux are recognised in the visible pane.

## 0.2.0 — 2026-09-30

- The fcitx5 addon: password fields in Zen, Chromium and other applications
  that mark them for the input method.

## 0.1.0 — 2026-09-30

- First release: password prompts in terminals (`sudo`, `ssh`, `read -s`,
  `getpass`), as an Arch package.
