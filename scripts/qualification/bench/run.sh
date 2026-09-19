#!/usr/bin/env bash
# Comparative benchmark: the same JSON endpoint (GET /hello/:name with a
# 400 for a name over 40 chars) on Usai (release binary, examples/hello
# artifact), Node 24 http, Bun and Deno — one process each, same machine,
# same load tool (oha), same duration and concurrency. Reported as it comes
# out; a hello endpoint measures per-request overhead, nothing else.
#
#   scripts/qualification/bench/run.sh <usai-binary> <hello-artifact-dir> [seconds] [concurrency...]
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
USAI="${1:?usai binary}"; ARTIFACT="${2:?hello artifact}"; DUR="${3:-10}"; shift 3 || true
CONCS=("${@:-1 16 64}")
[ ${#CONCS[@]} -gt 0 ] || CONCS=(1 16 64)
port="${BENCH_PORT:-19800}"
start() { "$@" >/dev/null 2>&1 & echo $!; }
wait_up() { for i in $(seq 1 100); do curl -s -o /dev/null "http://127.0.0.1:$1/hello/x" && return 0; sleep 0.1; done; return 1; }
bench() {
  local name="$1" pid="$2"
  wait_up "$port" || { echo "$name did not start"; kill "$pid" 2>/dev/null; return; }
  for c in ${CONCS[*]}; do
    local out; out=$(oha -z "${DUR}s" -c "$c" --no-tui --output-format json "http://127.0.0.1:$port/hello/world" 2>/dev/null)
    local rps p50 p99; rps=$(echo "$out" | node -pe 'JSON.parse(require("fs").readFileSync(0)).summary.requestsPerSec.toFixed(0)'); p50=$(echo "$out" | node -pe 'JSON.parse(require("fs").readFileSync(0)).metrics.latency_ms.p50.toFixed(2)'); p99=$(echo "$out" | node -pe 'JSON.parse(require("fs").readFileSync(0)).metrics.latency_ms.p99.toFixed(2)')
    local rss; rss=$(ps -o rss= -p "$pid" | awk '{printf "%d", $1/1024}')
    echo "| $name | $c | $rps | $p50 | $p99 | $rss |"
  done
  kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  port=$((port + 1))
}
echo "| server | concurrency | req/s | p50 ms | p99 ms | RSS MiB |"
echo "|---|---|---|---|---|---|"
bench "usai $($USAI --version | awk '{print $2}')" "$(start "$USAI" --root / run --artifact "$ARTIFACT" --port $port)"
bench "node $(node --version)" "$(start node "$here/node-hello.mjs" $port)"
command -v bun >/dev/null && bench "bun $(bun --version)" "$(start bun run "$here/bun-hello.ts" $port)"
command -v deno >/dev/null && bench "deno $(deno --version | head -1 | awk '{print $2}')" "$(start deno run --allow-net --quiet "$here/deno-hello.ts" $port)"
