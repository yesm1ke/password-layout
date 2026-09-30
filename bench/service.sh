#!/bin/bash
# CPU time and wakeups of the installed service over a window (default 30 s).
# Usage: bench/service.sh [seconds]
seconds=${1:-30}
pid=$(systemctl --user show -p MainPID --value password-layout-tty.service)
read_counters() {
  echo "$(cut -d' ' -f1 "/proc/$pid/schedstat") $(awk '/^voluntary/{print $2}' "/proc/$pid/status")"
}
read -r cpu1 wake1 < <(read_counters)
sleep "$seconds"
read -r cpu2 wake2 < <(read_counters)
cpu=$(( (cpu2 - cpu1) / 1000 ))
wakeups=$(( wake2 - wake1 ))
echo "service, $seconds s: $wakeups wakeups, $cpu us on CPU" \
  "($(( wakeups ? cpu / wakeups : 0 )) us per wakeup)"
systemctl --user show password-layout-tty.service -p MemoryCurrent
grep -E '^Pss:' "/proc/$pid/smaps_rollup"
