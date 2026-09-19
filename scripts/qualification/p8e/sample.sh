#!/usr/bin/env bash
# Samples what a set of processes hold, once a second, as JSON lines:
#
#   sample.sh <out.jsonl> <label:pid> [<label:pid> ...]
#
# Per process: RSS and PSS (KiB, from /proc/<pid>/status and smaps_rollup —
# PSS divides shared pages among their sharers and is the honest number when
# several processes map the same file), VmSize, minor/major faults, CPU
# ticks (utime+stime), threads, fds. Per sample: host MemAvailable and the
# kernel's context-switch and interrupt counters (/proc/stat), so a fleet's
# idle wakeups show up even when its CPU rounds to zero.
#
# Runs until killed (SIGTERM/SIGINT) or until every pid is gone.
set -u
out="$1"; shift
hz=$(getconf CLK_TCK)
procs=("$@")

read_stat() { # pid -> "minflt majflt utime+stime stime"
  local rest
  rest=$(sed 's/^[^)]*) //' "/proc/$1/stat" 2>/dev/null) || return 1
  # fields after the name: state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime stime
  # System time is reported on its own: memory-cgroup reclaim is charged to
  # the process as sys time, so a box at its memory limit shows up here.
  echo "$rest" | awk '{print $8, $10, $12+$13, $13}'
}

while :; do
  now=$(date -u +%s.%N)
  avail=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
  read -r ctxt intr < <(awk '/^ctxt/ {c=$2} /^intr/ {i=$2} END {print c, i}' /proc/stat)
  alive=0
  entries=""
  for p in "${procs[@]}"; do
    label="${p%%:*}"; pid="${p##*:}"
    [ -d "/proc/$pid" ] || continue
    alive=$((alive + 1))
    rss=$(awk '/^VmRSS/ {print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)
    vm=$(awk '/^VmSize/ {print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)
    thr=$(awk '/^Threads/ {print $2}' "/proc/$pid/status" 2>/dev/null || echo 0)
    pss=$(awk '/^Pss:/ {print $2}' "/proc/$pid/smaps_rollup" 2>/dev/null || echo 0)
    fds=$(ls "/proc/$pid/fd" 2>/dev/null | wc -l)
    read -r minflt majflt ticks sticks < <(read_stat "$pid" || echo "0 0 0 0")
    entries="$entries{\"label\":\"$label\",\"pid\":$pid,\"rssKib\":${rss:-0},\"pssKib\":${pss:-0},\"vmKib\":${vm:-0},\"threads\":${thr:-0},\"fds\":$fds,\"minflt\":$minflt,\"majflt\":$majflt,\"cpuTicks\":$ticks,\"sysTicks\":$sticks},"
  done
  entries="${entries%,}"
  echo "{\"t\":$now,\"hz\":$hz,\"memAvailableKib\":$avail,\"ctxt\":$ctxt,\"intr\":$intr,\"procs\":[$entries]}" >> "$out"
  [ "$alive" -gt 0 ] || exit 0
  sleep 1
done
