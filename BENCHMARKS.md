# Resource use

All figures were taken on 2026-09-30 on the development machine (Hyprland,
Ghostty, 6–9 terminals open). CPU is a share of one core. To repeat them:
`make bench`.

## Versions of the service

| | Python, polling | C++, polling | C++, event-driven (current) |
|---|---|---|---|
| Memory as accounted by systemd | 9.9 MB | 1.3 MB | 1.3–2.6 MB |
| Memory of the process itself (PSS) | not measured | 0.45 MB | 0.46 MB |
| CPU, terminals quiet | ~0.1% | 0.05–0.07% | ~0.006% |
| CPU, a terminal printing continuously | ~0.1% | 0.07% | 0.04–0.05% |
| Wakeups per second, terminals quiet | 5 | 5 | 0.7 |
| Wakeups per second, a terminal printing | 5 | 5 | 5 |
| Delay before switching, terminals were quiet | up to 200 ms | up to 200 ms | up to 50 ms |
| Delay before switching, a terminal was printing | up to 200 ms | up to 200 ms | up to 200 ms |

The current binary is 100 KB.

## A prompt pending in tmux

Measured 2026-09-30 over 30 s each, same machine, with a password prompt left
pending in a terminal outside the focused window:

| Where the prompt is | CPU in 30 s | Wakeups in 30 s |
|---|---|---|
| a plain terminal | 29.1 ms | 425 |
| a tmux pane, with a client attached | 29.7 ms | 403 |

Asking tmux costs nothing measurable while a prompt just sits there: tmux is
only asked again when the prompts, the focused window or a client's screen
change. Both rows are ~0.1% of a core, the price of polling while any prompt is
pending.

## Cost of one wakeup

Polling and the current service were measured at the same time, under the same
load (one terminal kept redrawing a progress indicator).

| Measurement | Terminals open | Polling | Current version |
|---|---|---|---|
| right after the optimisation | 7 | 136 us | 84 us |
| later the same day | 9 | 148 us | 102 us |
| `make bench` when this file was written | 8 | 143 us | 96 us |

At 5 wakeups per second that is 0.068–0.074% of a core for polling and
0.042–0.051% for the current version.

## What a polling tick is made of

One pass over the terminals right after a 200 ms sleep, 7 terminals, with the
overhead of the measurement itself subtracted:

| Part | Time |
|---|---|
| listing `/dev/pts` | ~47 us |
| opening every terminal | ~27 us |
| `stat`, reading the mode, closing | a few us |
| the wakeup itself (everything but the pass) | ~20 us |

The same pass in a hot loop with no sleep takes 9 us: the code is not what is
expensive, the first system calls after a sleep are, because they run cold.
Reading the whole process table takes about 1 ms, which is why it is only read
when the focused window or the set of pending prompts changes.

Both optimisations of the current version follow from this: do not wake while
terminals are quiet, and do not walk every terminal, only those that printed.

## Method

- **CPU time and wakeups of the service** — `bench/service.sh`: the difference
  in the first field of `/proc/<pid>/schedstat` (nanoseconds on a CPU) and in
  `voluntary_ctxt_switches` over a window of 20–40 seconds.
- **Polling, for comparison** — `bench/loop.cpp`: a "sleep 200 ms, scan every
  terminal" loop that times its own CPU use, 150 ticks.
- **Breakdown of a tick** — `bench/pass.cpp`: CPU time around each system call,
  100 passes. One reading of the clock costs about 3 us; the table above has it
  subtracted.
- **Memory** — `MemoryCurrent` of the systemd unit and `Pss` from
  `/proc/<pid>/smaps_rollup`.
- **Delay before switching** — time from the prompt text appearing on a real
  pseudo-terminal to the layout changing according to `password-layout status`.
  It includes starting the `status` command, so it overstates by a few
  milliseconds.

## Caveats

- **The Python version was measured coarsely**, with a counter that ticks every
  10 ms over 10 seconds, so "~0.1%" is an order of magnitude. It used noticeably
  more while a prompt was pending; that was not measured separately.
- **"Terminals quiet" for the current version** was taken on the build before
  only-terminals-that-printed checking, and some terminal did print during those
  40 seconds (29 wakeups, 2.2 ms). On a fully quiet system zero wakeups are
  expected; that has not been measured.
- **Memory as accounted by systemd** includes shared pages and varies between
  measurements (1.3–2.6 MB); the process's own memory did not change.
- **While a prompt is pending** the service looks every 200 ms. On the first C++
  build this cost 3% of a core (the process table was read on every tick), and
  about 0.1% by a coarse measurement once that was cached. It has not been
  measured separately for the current build.
- The cost of a wakeup depends on how many terminals printed between checks and
  on the state of the CPU after sleeping, so measurements taken at different
  times cannot be compared with each other — only the pairs within one row.
