# Backlog

Deferred problems: what is known, what was tried, what the options are. Current
work is in [STATUS.md](STATUS.md), the stages in [PLAN.md](PLAN.md).

## A silent password prompt in a terminal is not recognised

**What happens.** The service wakes only on the kernel event "something was
printed to a terminal", and then looks at the terminal's mode at once, 50 ms
later, and once more about 250 ms later. If a program turns echo off without
printing anything, and more than a quarter of a second after the last output on
that terminal, there is no event, the service does not wake, and the layout
stays as it was.

**Example.** `sleep 3; read -s x`.

**What is not affected.** `read -s -p "Password: " x`, `echo "Enter password";
read -s x`, `sudo`, `ssh`, `getpass` — all of them print a prompt.

**How to reproduce.** It needs a quiet system: no terminal printing anything.
With a non-Latin layout active:

```
sleep 3; read -s x; echo "typed: $x"
```

After three seconds type a few letters and press Enter. Non-Latin text in the
reply means the prompt was not recognised.

**Why it does not always reproduce.** Any output through `/dev/tty` makes the
service check every terminal. On 2026-09-30 four attempts in a row still
switched the layout after 0.1–0.7 s: something was writing through `/dev/tty`
every couple of seconds at the time (what, was not established). It has not been
tried on a quiet system.

**Where it came from.** Constant polling every 200 ms caught this prompt. Polling
was dropped for the sake of resource use (see the 2026-09-30 journal in
STATUS.md).

**Known option.** An infrequent safety pass over all terminals, say every 2 s:
about 0.5 wakeups per second (~0.007% of a core) and a delay of up to 2 s for a
silent prompt. Deferred: the case is contrived, and the quiet state would no
longer be free.

**What to look for.** A way to learn about a terminal mode change without
polling. The kernel sends no inotify event for `tcsetattr`; the master side of
a pseudo-terminal is notified in packet mode (`TIOCPKT`), but it belongs to the
terminal emulator, not to us.

## Publish the package to the AUR

**Why it is not there.** An AUR account could not be created on 2026-09-30
(registration on aur.archlinux.org did not go through), so for now the package
is distributed through GitHub: a release with a prebuilt package, and the recipe
in `packaging/arch`.

**What is ready.** `packaging/arch` holds a `PKGBUILD`, its `.SRCINFO` and the
install script; the package builds from the release tag of the GitHub
repository. The names `password-layout` and `password-layout-git` were free in
the AUR on that date.

**What is left.** With an AUR account and an SSH key registered there:

```
git clone ssh://aur@aur.archlinux.org/password-layout.git aur
cp packaging/arch/{PKGBUILD,.SRCINFO,password-layout.install} aur/
cd aur && git add -A && git commit -m "Initial import" && git push
```

After that `omarchy pkg aur add password-layout` (or any AUR helper) installs
it. The AUR maintainer line and commits carry an address too; the recipe uses
the GitHub no-reply one.

## Releasing a new version

Not automated. By hand: set `pkgver` in `packaging/arch/PKGBUILD`, regenerate
`.SRCINFO` (`makepkg --printsrcinfo > .SRCINFO`), commit, tag `vX.Y.Z` on that
commit and push both; then build with `makepkg`, attach the package to a GitHub
release, and update the URL in the README. Worth a script or a CI job once there
is a second release.
