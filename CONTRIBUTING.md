# Contributing

Bug reports, checks on systems other than the developer's, and fixes are
welcome. The scope is deliberately narrow ([README](README.md#what-works-today)):
one person at a real keyboard in a Hyprland session.

## Reporting a problem

Open an issue with the bug form; it asks for the output of
`password-layout doctor`, which covers most of what is needed. A security
problem goes through the private report on the Security tab instead
([SECURITY.md](SECURITY.md)). Known limitations each have an issue with the
label `limitation`; a case you hit or a check on your system belongs there.

## Pull requests

How to build and test, a map of the code, live checks and the rules that are
easy to break: [AGENTS.md](AGENTS.md).

- One topic per pull request, with a test when the change is in `src/`; run
  the tests the way CI does (AGENTS.md, "Build and test").
- Say what you checked by hand, and on which system: most of what this
  project does only shows on a live desktop.
- Documentation is in English. User-visible changes get a line in the
  `unreleased` section of [CHANGELOG.md](CHANGELOG.md).

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
