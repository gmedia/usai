#!/usr/bin/env bash
# Is this machine's floor harness charging the box for what it runs?
#
# A floor cell claims an application fits on a machine of a given size, and
# that is only true if the kernel charged the box for everything it used.
# Page cache is charged to the cgroup that *first* faults a page in, so an
# executable the host has already read is free to the container — through a
# bind mount **and** through an image layer, since `docker build` warms the
# cache just as well. The fix is to drop that file's page cache before the
# cell starts (`evict.py`, `posix_fadvise(POSIX_FADV_DONTNEED)`, no root).
#
#   pagecache-check.sh <binary> [args...]
#   pagecache-check.sh "$(command -v node)" -e 'setInterval(()=>{},1000)'
#
# Runs the same process three ways in a 512 MiB box and prints, for each, the
# resident set a sampler would see and what the cgroup was charged:
#
#   bind mount, warm    the old harness: charged far less than it holds
#   image layer, warm   copying into an image does not help
#   bind mount, cold    what the harness does now: charge ≈ resident
set -euo pipefail
bin="${1:?usage: pagecache-check.sh <binary> [args...]}"; shift || true
bin="$(readlink -f "$bin")"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
base="${BASE_IMAGE:-ubuntu:26.04}"
name=p8e-pagecache
ctx="$(mktemp -d)"; trap 'rm -rf "$ctx"; docker rm -f "$name" >/dev/null 2>&1 || true' EXIT

charged() { # -> "rss_kib charged_kib"
  local pid rel rss cur
  pid=$(docker inspect -f '{{.State.Pid}}' "$name")
  sleep 3   # let it touch what it touches
  rss=$(awk '/^VmRSS/ { print $2 }' "/proc/$pid/status")
  rel=$(awk -F: '$1 == "0" { print $3 }' "/proc/$pid/cgroup")
  cur=$(( $(cat "/sys/fs/cgroup$rel/memory.current") / 1024 ))
  echo "$rss $cur"
}
box() { docker run -d --name "$name" --memory 512m --memory-swap 512m "$@" >/dev/null; }

echo "binary: $bin ($(stat -c %s "$bin") bytes)"

# 1. bind mount, warm — the harness before 2026-09-23, and what a campaign
#    always produces: it runs the same binary on the host beforehand.
cat "$bin" > /dev/null
docker rm -f "$name" >/dev/null 2>&1 || true
box -v "$bin:/subject:ro" --entrypoint /subject "$base" "$@"
read -r rss_warm cur_warm <<< "$(charged)"
docker rm -f "$name" >/dev/null 2>&1

# 2. image layer, warm — the obvious "fix", which is not one.
cp "$bin" "$ctx/subject"; chmod a+rx "$ctx/subject"
printf 'FROM %s\nCOPY subject /subject\n' "$base" > "$ctx/Dockerfile"
docker build -q -t p8e-pagecache:local "$ctx" >/dev/null
box --entrypoint /subject p8e-pagecache:local "$@"
read -r rss_img cur_img <<< "$(charged)"
docker rm -f "$name" >/dev/null 2>&1

# 3. bind mount, cold — the file's page cache dropped just before the box
#    starts, so the box faults its own pages and pays for them.
python3 "$here/evict.py" "$bin" >/dev/null
box -v "$bin:/subject:ro" --entrypoint /subject "$base" "$@"
read -r rss_cold cur_cold <<< "$(charged)"
docker rm -f "$name" >/dev/null 2>&1

printf '%-20s %10s %10s\n' "" "RSS KiB" "charged KiB"
printf '%-20s %10s %10s\n' "bind mount, warm" "$rss_warm" "$cur_warm"
printf '%-20s %10s %10s\n' "image layer, warm" "$rss_img" "$cur_img"
printf '%-20s %10s %10s\n' "bind mount, cold" "$rss_cold" "$cur_cold"
echo
gap=$(( rss_warm - cur_warm ))
cold_gap=$(( rss_cold - cur_cold ))
echo "warm: the box holds $gap KiB it was not charged for."
echo "cold: the gap is $cold_gap KiB."
if [ "$gap" -gt 4096 ] && [ "$cold_gap" -lt "$((gap / 2))" ]; then
  echo "VERDICT: a warm cache hides the cost, and dropping it before the cell restores the bill."
elif [ "$gap" -le 4096 ]; then
  echo "VERDICT: no material gap on this machine - nothing to correct."
else
  echo "VERDICT: dropping the cache did not restore the bill; do not publish a floor from this machine."
fi
