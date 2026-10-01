# Contributing

Bug reports, checks on systems other than the developer's, and fixes are
welcome. The scope is deliberately narrow (README, "What works today"): one
person at a real keyboard in a Hyprland session.

## Reporting a problem

Open an issue with the bug form; it asks for the output of
`password-layout doctor`, which covers most of what is needed. A security
problem goes through the private report on the Security tab instead
([SECURITY.md](SECURITY.md)).

Known limitations each have an issue with the label `limitation`; adding a
case you hit, or a check on your system, there is useful too.

## Building and testing

A map of the code, the live checks and the rules that are easy to break is in
[AGENTS.md](AGENTS.md).

```
make            # the service, and the fcitx5 addon when its headers are installed
make test       # the tests; a few use real pseudo-terminals and tmux
make install-user
```

C++20, no libraries beyond the standard one and fcitx5. Before a pull request,
run the tests the way CI does:

```
CXXFLAGS="-O2 -Werror" make test
make clean && CXXFLAGS="-O1 -g -fsanitize=address,undefined" LDFLAGS="-fsanitize=address,undefined" make test
tools/check-addon-events
```

## Pull requests

- One topic per pull request, with a test when the change is in `src/`.
- The fcitx5 addon reacts only to focus and capability events and never to
  key presses; `tools/check-addon-events` enforces this and CI refuses a
  release otherwise.
- Say what you checked by hand, and on which system: most of what this
  project does only shows on a live desktop.
- Documentation is in English. User-visible changes get a line in the
  `unreleased` section of [CHANGELOG.md](CHANGELOG.md).

## Releases

Done by the maintainer with `tools/release X.Y.Z`, run twice: it opens a
pull request that sets the version, and after the merge it tags the commit.
GitHub Actions builds, signs and publishes; see the comments in
`tools/release` and `.github/workflows/release.yml`.

Pull requests are merged with "Rebase and merge"; CI runs on each of them.

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
