#!/usr/bin/env bash
# Runtime-level before/after for ADR-0019 lever 2: burst, then idle, sampling
# what the process holds. Argument: USAI_IDLE_DECOMMIT_MS (0 = control).
set -euo pipefail
export PATH=$HOME/.nvm/versions/node/v24.20.0/bin:$PATH
ROOT=/home/hasanh47/usai-p9
BIN="$ROOT/target/release/usai"
MS="${1:-30000}"
PORT="${PORT:-3311}"
DUR="${DUR:-10}"
CONC="${CONC:-32}"
IDLE="${IDLE:-70}"
# The contract-heavy class (validate 12 fields, compute, encode) is what the
# 2026-09-23 sweep measured the half-gigabyte plateau on; a trivial handler
# barely dirties a slot, so it has almost no plateau to release.
APP="${APP:-$ROOT/scripts/qualification/bench/app}"
PATH_="${PATH_:-/orders/quote}"
BODY="${BODY:-$HOME/quote.json}"

DATABASE_URL="${DATABASE_URL:-postgres://bench:bench@127.0.0.1:54330/bench}" \
  USAI_IDLE_DECOMMIT_MS="$MS" USAI_IDLE_DECOMMIT_FLOOR="${FLOOR:-64m}" \
  USAI_WASM_KEEP_RESIDENT="${KEEP_RESIDENT:-8m}" \
  "$BIN" --root "$APP" run --host 127.0.0.1 --port "$PORT" \
  --status-addr "127.0.0.1:$((PORT+1))" --max-worlds 48 >"$2" 2>&1 &
pid=$!
trap 'kill $pid 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
  curl -sf -o /dev/null "http://127.0.0.1:$((PORT+1))/_usai/ready" && break
  sleep 1
done

sample() {
  local label=$1
  local s
  s=$(curl -sf "http://127.0.0.1:$((PORT+1))/_usai/status" || echo '{}')
  python3 - "$label" "$s" <<'PY'
import json, sys
label = sys.argv[1]
try:
    d = json.loads(sys.argv[2])
except Exception:
    print(f"{label}\tunreadable"); raise SystemExit
p = d.get("process") or {}
pm = d.get("poolMemory") or {}
mib = lambda kib: (kib or 0) / 1024
print(
    f"{label}\trss={mib(p.get('rssKib')):.1f}MiB\tpss={mib(p.get('pssKib')):.1f}MiB"
    f"\tplateau={(pm.get('residentUnusedBytes') or 0)/2**20:.1f}MiB"
    f"\treleased={(pm.get('releasedBytes') or 0)/2**20:.1f}MiB"
    f"\treleases={pm.get('releases')}\tworlds={d.get('gauges', {}).get('worldsCreated')}"
)
PY
}

echo "# USAI_IDLE_DECOMMIT_MS=$MS floor=${FLOOR:-64m} keep_resident=${KEEP_RESIDENT:-8m} c=$CONC dur=${DUR}s idle=${IDLE}s"
sample "before-burst"
# `usai bench` serves in-process, so it cannot drive this server. CONC
# keep-alive clients touch CONC slots, which is what the plateau is made of;
# a curl-per-request loop cannot (process spawn dominates and in-flight
# concurrency never reaches CONC).
node "$HOME/burst.mjs" "http://127.0.0.1:$PORT$PATH_" "$CONC" "$DUR" "$BODY"
sample "after-burst"
for i in $(seq 10 10 "$IDLE"); do
  sleep 10
  sample "idle+${i}s"
done
# The cost side: the first request after the quiet period.
node "$HOME/wake.mjs" "http://127.0.0.1:$PORT$PATH_" "$BODY"
sample "after-wake"
