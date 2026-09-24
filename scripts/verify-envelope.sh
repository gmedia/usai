#!/usr/bin/env bash
# `make verify-envelope`: re-measure the published memory envelope and assert
# it, so the most load-bearing numbers in `SUPPORTED.md` are a **test** rather
# than a dated measurement somebody has to trust.
#
# Round 21 (a claims audit) could check 44 statements and not this one: the
# whole cgroup-conditioned envelope — the 192 MiB supported floor, the 64 MiB
# technical floor — needs a memory limit the reviewer had no permission to
# set. An adopter is in the same position, and "trust the measurement file"
# is not an answer for the number a deployment is sized on.
#
# What it asserts, from `SUPPORTED.md` → Host envelope:
#
#   * the production shape (the scaffold template, `--max-worlds 48`, a
#     PostgreSQL pool of 4, status and metrics on) runs in a **192 MiB**
#     box: never OOM-killed, ready at the end, **zero** cgroup ceiling hits,
#     and a charged peak well under the limit (the published figure is
#     93 MiB; this asserts ≤ 128 MiB, which is the headroom claim that
#     matters — 128 MiB serves the shape with no reclaim at all);
#   * the technical floor — `hello` alone, `--max-worlds 16` — is alive and
#     serving in a **64 MiB** box.
#
# It needs Docker (the cgroup box), a throwaway PostgreSQL and a release
# build. Without them it says which one is missing and exits 0: a check that
# cannot run is not a failure, and pretending otherwise teaches people to
# ignore it.
#
#   DATABASE_URL=postgres://… make verify-envelope
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/.." && pwd)"
fleet="$repo/scripts/qualification/p8e/fleet.sh"

skip() { echo "verify-envelope: skipped — $1"; exit 0; }

command -v docker >/dev/null 2>&1 || skip "docker is not installed (the cgroup box needs it)"
docker info >/dev/null 2>&1 || skip "the docker daemon is not reachable"
[ -n "${DATABASE_URL:-}" ] || skip "DATABASE_URL is not set (a throwaway database; scripts/dev-postgres.sh start prints one)"
USAI="${USAI:-$repo/target/release/usai}"
[ -x "$USAI" ] || skip "no release binary at $USAI (cargo build --release -p usai-cli)"

OUT="${OUT:-$repo/target/verify-envelope/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT"
export DATABASE_URL USAI OUT
echo "verify-envelope: $OUT"
echo "  binary: $USAI ($(stat -c %s "$USAI") bytes)"

"$fleet" prepare || { echo "verify-envelope: prepare failed"; exit 1; }

# app  mem  cpus  worlds  max charged peak (MiB, 0 = only "it survived")
cells="template 192 1 48 128
hello 64 1 16 0"

failures=0
printf '\n%-28s %10s %8s %8s %7s %s\n' cell peakMiB hits oom ready verdict
while read -r app mem cpus worlds cap; do
  [ -n "$app" ] || continue
  "$fleet" floor "$app" "$mem" "$cpus" "$worlds" >/dev/null 2>&1
  result=$(ls -d "$OUT"/floor-"$app-${mem}m-${cpus}c-${worlds}w"*/result.json 2>/dev/null | head -1)
  if [ -z "$result" ]; then
    printf '%-28s %10s %8s %8s %7s %s\n' "$app/${mem}m" - - - - "NO RESULT"
    failures=$((failures + 1))
    continue
  fi
  read -r peak hits oom ready < <(python3 - "$result" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(
    round(d.get("cgroupPeakBytes", 0) / 1048576),
    d.get("cgroupCeilingHits", "?"),
    str(d.get("oomKilled", "?")).lower(),
    str(d.get("readyAtEnd", "?")).lower(),
)
PY
)
  bad=""
  [ "$oom" = false ] || bad="$bad oom-killed"
  [ "$ready" = true ] || bad="$bad not-ready"
  # The ceiling-hit assertion is the production claim: a box that never OOMs
  # but sits against its limit is passing on reclaim, and at the supported
  # floor it must not have to.
  if [ "$cap" != 0 ]; then
    [ "$hits" = 0 ] || bad="$bad ceiling-hits=$hits"
    [ "$peak" -le "$cap" ] 2>/dev/null || bad="$bad peak=${peak}MiB>${cap}MiB"
  fi
  if [ -n "$bad" ]; then
    failures=$((failures + 1))
    printf '%-28s %10s %8s %8s %7s %s\n' "$app/${mem}m" "$peak" "$hits" "$oom" "$ready" "FAIL:$bad"
  else
    printf '%-28s %10s %8s %8s %7s %s\n' "$app/${mem}m" "$peak" "$hits" "$oom" "$ready" ok
  fi
done <<EOF
$cells
EOF

echo
if [ "$failures" -gt 0 ]; then
  echo "verify-envelope: $failures cell(s) did not hold the published envelope (evidence in $OUT)"
  echo "  SUPPORTED.md → Host envelope is the claim; docs/measurements/2026-09-23-floor-accounting.md is the method."
  exit 1
fi
echo "verify-envelope: the published envelope holds (evidence in $OUT)"
