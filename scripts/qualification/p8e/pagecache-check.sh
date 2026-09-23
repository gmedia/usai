#!/usr/bin/env bash
# Does a bind-mounted binary escape the box's memory bill?
#
# A floor cell is a claim about how small a machine an application fits on, and
# it is only true if the kernel charged the box for everything the box uses.
# A read-only bind mount does not guarantee that: page cache is charged to the
# cgroup that first faults a page in, so a binary the *host* has already run is
# free for the container. This is the two-line experiment that says whether
# that is happening here, on this machine, with this binary.
#
#   pagecache-check.sh <binary> [args...]        # e.g. pagecache-check.sh "$(command -v node)" -e 'setInterval(()=>{},1e3)'
#
# It starts the same process twice in a 512 MiB box — once with the binary
# bind-mounted from the host, once with it copied into an image — and prints,
# for each, the resident set the sampler would see and what the cgroup was
# charged. If the bind-mounted run is charged materially less than its RSS,
# every floor measured that way is optimistic and must be re-measured.
set -euo pipefail
bin="${1:?usage: pagecache-check.sh <binary> [args...]}"; shift || true
bin="$(readlink -f "$bin")"
base="${BASE_IMAGE:-ubuntu:26.04}"
name=p8e-pagecache
ctx="$(mktemp -d)"; trap 'rm -rf "$ctx"; docker rm -f "$name" >/dev/null 2>&1 || true' EXIT

charged() { # container name -> "rss_kib charged_kib"
  local pid rel rss cur
  pid=$(docker inspect -f '{{.State.Pid}}' "$name")
  # Touch the pages the way a request would: let it settle first.
  sleep 3
  rss=$(awk '/^VmRSS/ { print $2 }' "/proc/$pid/status")
  rel=$(awk -F: '$1 == "0" { print $3 }' "/proc/$pid/cgroup")
  cur=$(( $(cat "/sys/fs/cgroup$rel/memory.current") / 1024 ))
  echo "$rss $cur"
}

echo "binary: $bin ($(stat -c %s "$bin") bytes)"
# Warm the host's page cache for it, which is what a real campaign run does
# long before the cell starts (the harness itself runs the same binary).
cat "$bin" > /dev/null

docker rm -f "$name" >/dev/null 2>&1 || true
docker run -d --name "$name" --memory 512m --memory-swap 512m \
  -v "$bin:/subject:ro" --entrypoint /subject "$base" "$@" >/dev/null
read -r rss_mount cur_mount <<< "$(charged)"
docker rm -f "$name" >/dev/null 2>&1

cp "$bin" "$ctx/subject"; chmod a+rx "$ctx/subject"
printf 'FROM %s\nCOPY subject /subject\n' "$base" > "$ctx/Dockerfile"
docker build -q -t p8e-pagecache:local "$ctx" >/dev/null
docker run -d --name "$name" --memory 512m --memory-swap 512m \
  --entrypoint /subject p8e-pagecache:local "$@" >/dev/null
read -r rss_img cur_img <<< "$(charged)"
docker rm -f "$name" >/dev/null 2>&1

printf '%-14s %10s %10s\n' "" "RSS KiB" "charged KiB"
printf '%-14s %10s %10s\n' "bind mount" "$rss_mount" "$cur_mount"
printf '%-14s %10s %10s\n' "image layer" "$rss_img" "$cur_img"
gap=$(( rss_mount - cur_mount ))
echo
echo "bind-mounted run: the cgroup was charged $gap KiB less than the process holds."
[ "$gap" -gt 4096 ] && echo "VERDICT: a bind-mounted binary is not on the box's bill - floors measured that way are optimistic." || echo "VERDICT: no material gap on this machine."
