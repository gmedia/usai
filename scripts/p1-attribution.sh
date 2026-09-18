#!/usr/bin/env bash
# P1 execution-path attribution: the workload matrix with the phase ledger
# for each substrate, plus hardware counters per request (perf stat, when
# available). Engineering evidence, not a benchmark claim.
#
#   scripts/p1-attribution.sh [oz-core.wasm]
#
# Env: USAI_MATRIX_N (ledger runs per row, default 300), P1_PERF_N (requests
# per perf row, default 20000), P1_PERF_ROWS (default "empty,zod,crud list").
set -euo pipefail
cd "$(dirname "$0")/.."

OZ_CORE="${1:-}"
N="${USAI_MATRIX_N:-300}"
PERF_N="${P1_PERF_N:-20000}"
PERF_ROWS="${P1_PERF_ROWS:-empty,zod,crud list}"

cargo build --release -p usai-runtime --test profile_matrix >/dev/null
BIN="$(ls -t target/release/deps/profile_matrix-* | grep -v '\.d$' | head -1)"
echo "test binary: $BIN"
echo "host: $(uname -srm) | $(nproc) cpus | $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | sed 's/^ //')"
echo "core -O3: $(sha256sum crates/usai-runtime/guest/quickjs-async.wasm | cut -c1-16)…"
[ -n "$OZ_CORE" ] && echo "core -Oz: $(sha256sum "$OZ_CORE" | cut -c1-16)… ($OZ_CORE)"

configs=("wasm-O3:USAI_ENGINE=wasm" "quickjs:USAI_ENGINE=quickjs")
[ -n "$OZ_CORE" ] && configs=("wasm-O3:USAI_ENGINE=wasm" "wasm-Oz:USAI_ENGINE=wasm USAI_WASM_CORE=$OZ_CORE" "quickjs:USAI_ENGINE=quickjs")

echo; echo "### phase ledger (ms per request, n=$N)"
for c in "${configs[@]}"; do
  name="${c%%:*}"; vars="${c#*:}"
  echo; echo "--- $name"
  env USAI_PROFILE=1 USAI_MATRIX_N="$N" $vars "$BIN" workload_matrix --ignored --nocapture --exact 2>/dev/null | sed -n '/^== /,/^(total/p'
done

if ! command -v perf >/dev/null; then echo; echo "perf not available; counters skipped"; exit 0; fi

EVENTS="cycles,instructions,page-faults,minor-faults,major-faults,context-switches,cpu-migrations"
echo; echo "### hardware counters per request (perf stat, delta of n=$PERF_N vs n=1, both after 20 warm-ups)"
printf "%-8s %-12s %14s %14s %10s %10s %10s %10s\n" config workload instructions cycles minflt majflt ctxsw ipc
IFS=',' read -ra ROWS <<< "$PERF_ROWS"
for c in "${configs[@]}"; do
  name="${c%%:*}"; vars="${c#*:}"
  for row in "${ROWS[@]}"; do
    row="${row# }"
    big="$(env USAI_PROFILE=1 USAI_MATRIX_N="$PERF_N" USAI_MATRIX_ROWS="$row" $vars perf stat -x, -e "$EVENTS" "$BIN" workload_matrix --ignored --exact 2>&1 >/dev/null | grep -E "^[0-9]")"
    small="$(env USAI_PROFILE=1 USAI_MATRIX_N=1 USAI_MATRIX_ROWS="$row" $vars perf stat -x, -e "$EVENTS" "$BIN" workload_matrix --ignored --exact 2>&1 >/dev/null | grep -E "^[0-9]")"
    get() { echo "$1" | awk -F, -v e="$2" '$3==e {print $1}'; }
    d=$((PERF_N - 1))
    ins=$(( ($(get "$big" instructions) - $(get "$small" instructions)) / d ))
    cyc=$(( ($(get "$big" cycles) - $(get "$small" cycles)) / d ))
    mnf=$(( ($(get "$big" minor-faults) - $(get "$small" minor-faults)) ))
    mjf=$(( ($(get "$big" major-faults) - $(get "$small" major-faults)) ))
    csw=$(( ($(get "$big" context-switches) - $(get "$small" context-switches)) ))
    ipc=$(awk -v i="$ins" -v c="$cyc" 'BEGIN { if (c > 0) printf "%.2f", i / c; else print "n/a" }')
    printf "%-8s %-12s %14d %14d %10.1f %10.3f %10.2f %10s\n" "$name" "$row" "$ins" "$cyc" "$(awk -v v=$mnf -v d=$d 'BEGIN{print v/d}')" "$(awk -v v=$mjf -v d=$d 'BEGIN{print v/d}')" "$(awk -v v=$csw -v d=$d 'BEGIN{print v/d}')" "$ipc"
  done
done
