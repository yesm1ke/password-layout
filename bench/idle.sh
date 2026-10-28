#!/usr/bin/env bash
# The idle cost of the service, for one binary (stage 4e).
#
#   bench/idle.sh BINARY [SECONDS] [prompt]
#
# Runs `BINARY watch-tty` under env -i against the Rust fake Hyprland
# (parity_driver serve) in a fresh XDG_RUNTIME_DIR: one keyboard with the
# layouts us,ru and an active window owned by pid 1, which owns no terminal
# (the ancestry walk stops at pid 1), so a prompt is never in focus.
# With `prompt`, script(1) puts a pty into password mode in the background
# (owned by no focused window), so the service polls; without script(1) the
# prompt run is skipped with a message.
#
# After a 2 s settle, for SECONDS the service runs while its /proc counters
# are read once at each end: wakeups = voluntary + nonvoluntary context
# switches, summed over all its threads; CPU = schedstat field 1 (ns on CPU).
# Then its memory: Pss and Rss from smaps_rollup, VmHWM and Threads from
# status. Prints one line:
#
#   NAME [idle|prompt], S s: W wakeups, C us on CPU; Pss X kB, Rss Y kB,
#   VmHWM Z kB, T threads
#
# A trap on EXIT kills everything the script started and removes its temp
# directory.
set -euo pipefail

if [ "$#" -lt 1 ] || [ "$#" -gt 3 ]; then
  echo "usage: bench/idle.sh BINARY [SECONDS] [prompt]" >&2
  exit 2
fi
binary=$1
seconds=${2:-30}
mode=idle
if [ "$#" -eq 3 ]; then
  if [ "$3" != prompt ]; then
    echo "bench/idle.sh: unknown mode '$3' (want 'prompt')" >&2
    exit 2
  fi
  mode=prompt
fi
if [ ! -x "$binary" ]; then
  echo "bench/idle.sh: not an executable: $binary" >&2
  exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/rust/target}
driver=$target/release/examples/parity_driver
if [ ! -x "$driver" ]; then
  echo "bench/idle.sh: $driver is missing; 'make bench' builds it" >&2
  exit 1
fi

workdir=$(mktemp -d)
fifo=$workdir/fifo
mkfifo "$fifo"
printf '{"keyboards": [{"name": "kb", "layout": "us,ru", "variant": "", "main": true, "active_layout_index": 0}]}\n' \
  > "$workdir/devices.json"
printf '{"pid": 1}\n' > "$workdir/window.json"

serve_pid=0
service_pid=0
prompt_pid=0
cleanup() {
  trap - EXIT INT TERM
  for pid in "$prompt_pid" "$service_pid" "$serve_pid"; do
    if [ "$pid" -gt 0 ]; then
      kill "$pid" 2>/dev/null || true
    fi
  done
  for pid in "$prompt_pid" "$service_pid" "$serve_pid"; do
    if [ "$pid" -gt 0 ]; then
      wait "$pid" 2>/dev/null || true
    fi
  done
  exec 3>&- 2>/dev/null || true
  rm -rf "$workdir"
}
trap cleanup EXIT
trap 'exit 143' INT TERM

# The fake Hyprland; its stdin is a fifo held open (fd 3) so that it keeps
# serving until the script ends. A write-only open would block until the fake
# comes up, so open it read-write.
"$driver" serve "$workdir" bench "$workdir/devices.json" "$workdir/window.json" \
  < "$fifo" > "$workdir/serve.log" 2>&1 &
serve_pid=$!
exec 3<>"$fifo"
socket=$workdir/hypr/bench/.socket.sock
for _ in $(seq 1 50); do
  [ -S "$socket" ] && break
  kill -0 "$serve_pid" 2>/dev/null || break
  sleep 0.1
done
if [ ! -S "$socket" ]; then
  echo "bench/idle.sh: the fake Hyprland did not come up:" >&2
  cat "$workdir/serve.log" >&2
  exit 1
fi

env -i PATH=/usr/bin:/bin XDG_RUNTIME_DIR="$workdir" HYPRLAND_INSTANCE_SIGNATURE=bench \
  "$binary" watch-tty > "$workdir/service.log" 2>&1 &
service_pid=$!

if [ "$mode" = prompt ]; then
  if command -v script >/dev/null 2>&1; then
    script -qfc "bash -c 'read -s x'" /dev/null </dev/null >/dev/null 2>&1 &
    prompt_pid=$!
  else
    echo "bench/idle.sh: script(1) not found; skipping the prompt run" >&2
    exit 0
  fi
fi

# Settle: the service notes the layout and the prompt (if any) is found.
sleep 2
if ! kill -0 "$service_pid" 2>/dev/null; then
  echo "bench/idle.sh: the service died during the run:" >&2
  cat "$workdir/service.log" >&2
  exit 1
fi

# One line of counters: ns on CPU (schedstat field 1, per thread), then the
# voluntary + nonvoluntary context switches summed over all threads.
counters() {
  local pid=$1 cpu switches
  cpu=$(awk '{ s += $1 } END { print s + 0 }' /proc/"$pid"/task/*/schedstat)
  switches=$(awk -F: '/ctxt_switches/ { s += $2 } END { print s + 0 }' /proc/"$pid"/task/*/status)
  printf '%s %s\n' "$cpu" "$switches"
}

read -r cpu1 switches1 < <(counters "$service_pid")
sleep "$seconds"
if ! kill -0 "$service_pid" 2>/dev/null; then
  echo "bench/idle.sh: the service died during the run:" >&2
  cat "$workdir/service.log" >&2
  exit 1
fi
read -r cpu2 switches2 < <(counters "$service_pid")
read -r pss rss < <(awk '/^Pss:/ { p = $2 } /^Rss:/ { r = $2 } END { print p, r }' \
  /proc/"$service_pid"/smaps_rollup)
read -r hwm threads < <(awk '/^VmHWM:/ { h = $2 } /^Threads:/ { t = $2 } END { print h, t }' \
  /proc/"$service_pid"/status)

printf '%s %s, %s s: %s wakeups, %s us on CPU; Pss %s kB, Rss %s kB, VmHWM %s kB, %s threads\n' \
  "$(basename "$binary")" "$mode" "$seconds" \
  "$((switches2 - switches1))" "$(((cpu2 - cpu1) / 1000))" \
  "$pss" "$rss" "$hwm" "$threads"
