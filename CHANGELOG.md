# Changelog

What changed for someone who installs the package. The day-to-day record is in
[STATUS.md](STATUS.md).

## 0.6.0 — unreleased

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
