#!/usr/bin/env bash
# The benchmark suite (docs/measurements/BENCHMARKS.md): several workload
# classes, comparators doing the same work, four scoreboards. No single
# benchmark is the benchmark.
#
#   suite.sh prepare                 # build the Usai artifact and the Rust control; reset + migrate the database
#   suite.sh invoice [servers]       # c=1, pinned; per-class p50/p99/CPU per request; Usai's per-phase invoice
#   suite.sh sweep   [servers]       # c ∈ CONCS, all classes; oha for the reads, load.mjs for the writes
#   suite.sh leak    [servers]       # correctness probes: cross-request state, cancellation, RSS drift
#   suite.sh all                     # prepare, invoice, sweep, leak
#
# Environment: USAI (binary; default target/release/usai), DATABASE_URL
# (required; the suite drops and recreates the public schema), SERVERS
# (default "usai node rust bun deno"; "php" is docker, as shipped, c=1 only;
# "php-tuned" is docker with opcache + JIT + pm=static, c ≤ PHP_CHILDREN;
# "node-cluster" is Node with one worker per cpu of PIN_SERVER; "laravel-fpm"
# is Laravel 12 on a tuned FPM, docker, composer at image build, c ≤ PHP_CHILDREN),
# CONCS (default "1 2 4 8 16 32 64"), DUR (seconds per cell, default 10),
# PIN_SERVER / PIN_CLIENT (cpu lists for taskset; unset = no pinning),
# POOL_MAX (default 64), OUT (default out/<timestamp>).
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
USAI="${USAI:-$repo/target/release/usai}"
SERVERS="${SERVERS:-usai node rust bun deno}"
CONCS="${CONCS:-1 2 4 8 16 32 64}"
DUR="${DUR:-10}"
POOL_MAX="${POOL_MAX:-64}"
PHP_CHILDREN="${PHP_CHILDREN:-32}"
OUT="${OUT:-$here/out/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT"
: "${DATABASE_URL:?DATABASE_URL is required (a throwaway database: the suite resets its public schema)}"
export DATABASE_URL POOL_MAX
# Servers start in a background subshell; exec makes $! the server itself.
pin_server() { if [ -n "${PIN_SERVER:-}" ]; then exec taskset -c "$PIN_SERVER" "$@"; else exec "$@"; fi; }
pin_client() { if [ -n "${PIN_CLIENT:-}" ]; then taskset -c "$PIN_CLIENT" "$@"; else "$@"; fi; }
log() { echo "[$(date -u +%H:%M:%S)] $*"; }

# ---- database ---------------------------------------------------------------
reset_db() {
  (cd "$here" && node -e '
    const { Client } = require("pg");
    const c = new Client({ connectionString: process.env.DATABASE_URL });
    c.connect().then(() => c.query("drop schema public cascade; create schema public;")).then(() => c.end()).catch((e) => { console.error(e.message); process.exit(1); });
  ') && "$USAI" --root "$here/app" db migrate >/dev/null
}

# Writes leave state behind (paid orders → 409s, inserted users): every
# server measures class D and E from the same seeded state.
reset_writes() {
  (cd "$here" && node -e '
    const { Client } = require("pg");
    const c = new Client({ connectionString: process.env.DATABASE_URL });
    c.connect().then(() => c.query("truncate payments; update orders set paid = false where paid; delete from users where id > 10000;")).then(() => c.end()).catch((e) => { console.error(e.message); process.exit(1); });
  ')
}

prepare() {
  log "artifact"; "$USAI" --root "$here/app" build --no-typecheck >/dev/null || return 1
  log "rust control"; (cd "$here/baselines/rust-axum" && cargo build -q --release) || return 1
  log "database"; reset_db || return 1
  log "prepared"
}

# ---- servers ----------------------------------------------------------------
port_of() { case "$1" in usai) echo 3460;; node) echo 3461;; bun) echo 3462;; deno) echo 3463;; rust) echo 3464;; node-cluster) echo 3465;; php) echo 3005;; php-tuned) echo 3006;; laravel-fpm) echo 3007;; esac; }
is_php() { [ "$1" = php ] || [ "$1" = php-tuned ] || [ "$1" = laravel-fpm ]; }
# The compose directory of a docker comparator.
compose_dir() { if [ "$1" = laravel-fpm ]; then echo "$here/baselines/laravel"; else echo "$here/baselines/php"; fi; }
# Installed, or one of the servers that need no binary on PATH.
available() { command -v "${1%%-*}" >/dev/null 2>&1 || [ "$1" = usai ] || [ "$1" = rust ] || is_php "$1"; }
# Workers for the Node cluster: one per cpu the server is pinned to.
cluster_size() { if [ -n "${PIN_SERVER:-}" ]; then taskset -c "$PIN_SERVER" nproc; else nproc; fi; }
version_of() {
  case "$1" in
    usai) "$USAI" --version | awk '{print $2}';;
    node) node --version;;
    node-cluster) echo "$(node --version) × $(cluster_size)";;
    bun) bun --version;;
    deno) deno --version | head -1 | awk '{print $2}';;
    rust) rustc --version | awk '{print $2}';;
    php) echo "8.4-fpm as shipped";;
    php-tuned) echo "8.4-fpm opcache+jit pm=static $PHP_CHILDREN";;
    laravel-fpm) echo "laravel 12 on 8.4-fpm opcache+jit pm=static $PHP_CHILDREN, config/route cached";;
  esac
}
SERVER_PID=""
start_server() {
  local name="$1" port; port=$(port_of "$name")
  local logf="$OUT/$name.server.log"
  if curl -s -m 1 -o /dev/null "http://127.0.0.1:$port/health"; then log "port $port is already in use (a stale $name?); refusing to measure someone else's process"; return 1; fi
  case "$name" in
    usai) pin_server env USAI_MAX_WORLDS=256 ${USAI_PROFILE:+USAI_PROFILE=$USAI_PROFILE} "$USAI" --root "$here/app" run --artifact "$here/app/.usai/build" --port "$port" --status > "$logf" 2>&1 & SERVER_PID=$!;;
    node) pin_server env PORT=$port node "$here/baselines/node-fastify/server.mjs" > "$logf" 2>&1 & SERVER_PID=$!;;
    node-cluster) pin_server env PORT=$port CLUSTER=$(cluster_size) node "$here/baselines/node-fastify/server.mjs" > "$logf" 2>&1 & SERVER_PID=$!;;
    bun)  pin_server env PORT=$port bun run "$here/baselines/bun-hono/server.ts" > "$logf" 2>&1 & SERVER_PID=$!;;
    deno) pin_server env PORT=$port deno run -A --quiet "$here/baselines/deno-hono/server.ts" > "$logf" 2>&1 & SERVER_PID=$!;;
    rust) pin_server env PORT=$port "$here/baselines/rust-axum/target/release/bench-rust-axum" > "$logf" 2>&1 & SERVER_PID=$!;;
    php)  (cd "$here/baselines/php" && PHP_PROFILE=shipped PHP_PORT=3005 PHP_CPUSET="${PIN_SERVER:-}" DATABASE_URL="${DATABASE_URL//127.0.0.1/host.docker.internal}" docker compose up -d --build > "$logf" 2>&1); SERVER_PID="";;
    php-tuned) (cd "$here/baselines/php" && PHP_PROFILE=tuned PHP_CHILDREN="$PHP_CHILDREN" PHP_PORT=3006 PHP_CPUSET="${PIN_SERVER:-}" DATABASE_URL="${DATABASE_URL//127.0.0.1/host.docker.internal}" docker compose up -d --build > "$logf" 2>&1); SERVER_PID="";;
    laravel-fpm) (cd "$here/baselines/laravel" && PHP_CHILDREN="$PHP_CHILDREN" PHP_CPUSET="${PIN_SERVER:-}" DATABASE_URL="${DATABASE_URL//127.0.0.1/host.docker.internal}" docker compose up -d --build > "$logf" 2>&1); SERVER_PID="";;
  esac
  for i in $(seq 1 200); do curl -s -m 1 -o /dev/null "http://127.0.0.1:$port/health" && return 0; sleep 0.1; done
  log "$name did not start (see $logf)"; return 1
}
stop_server() {
  local name="$1"
  if is_php "$name"; then (cd "$(compose_dir "$name")" && docker compose down >/dev/null 2>&1); return; fi
  [ -n "$SERVER_PID" ] && { kill "$SERVER_PID" 2>/dev/null; wait "$SERVER_PID" 2>/dev/null; }
  SERVER_PID=""
}
# CPU seconds of the server so far (utime+stime of the process tree, or the
# php containers' cgroup).
cpu_seconds() {
  local name="$1"
  if is_php "$name"; then
    local total=0
    for c in $(docker compose -f "$(compose_dir "$name")/compose.yaml" ps -q 2>/dev/null); do
      local usec; usec=$(docker exec "$c" cat /sys/fs/cgroup/cpu.stat 2>/dev/null | awk '/usage_usec/ {print $2}'); total=$((total + ${usec:-0}))
    done
    echo "scale=3; $total / 1000000" | bc
  else
    local ticks=0
    for p in $SERVER_PID $(pgrep -P "$SERVER_PID" 2>/dev/null); do
      local t; t=$(awk '{print $14 + $15}' "/proc/$p/stat" 2>/dev/null); ticks=$((ticks + ${t:-0}))
    done
    echo "scale=3; $ticks / $(getconf CLK_TCK)" | bc
  fi
}
# RSS of the process tree (a Node cluster is a primary plus its workers).
rss_mib() {
  if is_php "$1"; then docker stats --no-stream --format "{{.MemUsage}}" 2>/dev/null | awk -F'[ /]' '{s += $1} END {printf "%d", s}'; else ps -o rss= -p "$SERVER_PID" $(pgrep -P "$SERVER_PID" 2>/dev/null) 2>/dev/null | awk '{s += $1} END {printf "%d", s/1024}'; fi
}

# ---- one cell ---------------------------------------------------------------
# cell <server> <class> <c>: writes $OUT/<server>.<class>.c<c>.json
QUOTE='{"customer":"Ayu","email":"ayu@example.com","currency":"IDR","country":"ID","requestedDate":"2026-10-01","reference":"6f1a2b3c-4d5e-4f60-8a71-92b3c4d5e6f7","tags":["a","b"],"items":[{"sku":"A1","quantity":2,"unitCents":1500},{"sku":"B2","quantity":1,"unitCents":990},{"sku":"C3","quantity":5,"unitCents":120}]}'
cell() {
  local name="$1" cls="$2" c="$3" port; port=$(port_of "$name")
  local base="http://127.0.0.1:$port" cpu0 cpu1 json
  case "$cls" in D|E) reset_writes;; esac
  cpu0=$(cpu_seconds "$name")
  if [ "$cls" = D ] || [ "$c" -le 4 ] || ! command -v oha >/dev/null; then
    json=$(cd "$here" && pin_client node "$here/load.mjs" "$base" "$cls" "$c" "$DUR")
  else
    local args=(-z "${DUR}s" -c "$c" --no-tui --output-format json)
    case "$cls" in
      A) args+=("$base/hello/world");;
      B) args+=(-m POST -H "content-type: application/json" -d "$QUOTE" "$base/orders/quote");;
      C) args+=("$base/users/42");;
      E) args+=(-m POST --rand-regex-url "$base/orders/[1-9][0-9]{0,4}/pay");;
      F) args+=(-H "x-api-key: key-5" "$base/me");;
    esac
    json=$(pin_client oha "${args[@]}" 2>/dev/null | node -e '
      const o = JSON.parse(require("fs").readFileSync(0, "utf8")); const codes = o.statusCodeDistribution || {};
      const sum = (f) => Object.entries(codes).filter(([k]) => f(Number(k))).reduce((a, [, v]) => a + v, 0);
      console.log(JSON.stringify({ class: process.argv[1], clients: Number(process.argv[2]), seconds: o.summary.total, requests: o.summary.successRate ? Math.round(o.summary.requestsPerSec * o.summary.total) : 0, rps: +o.summary.requestsPerSec.toFixed(0), p50: +o.metrics.latency_ms.p50.toFixed(3), p95: +o.metrics.latency_ms.p95.toFixed(3), p99: +o.metrics.latency_ms.p99.toFixed(3), ok: sum((s) => s < 300), s4xx: sum((s) => s >= 400 && s < 500), s5xx: sum((s) => s >= 500), errors: Object.values(o.errorDistribution || {}).reduce((a, b) => a + b, 0), errorKinds: o.errorDistribution || {} }));
    ' "$cls" "$c")
  fi
  cpu1=$(cpu_seconds "$name")
  local rss; rss=$(rss_mib "$name")
  echo "$json" | node -e '
    const o = JSON.parse(require("fs").readFileSync(0, "utf8"));
    const cpu = Number(process.argv[3]) - Number(process.argv[2]);
    o.server = process.argv[1]; o.cpuSeconds = +cpu.toFixed(3); o.cpuMsPerRequest = o.requests ? +((cpu * 1000) / o.requests).toFixed(4) : null; o.rssMiB = Number(process.argv[4]);
    console.log(JSON.stringify(o));
  ' "$name" "$cpu0" "$cpu1" "$rss" | tee "$OUT/$name.$cls.c$c.json" | node -e '
    const o = JSON.parse(require("fs").readFileSync(0, "utf8"));
    console.log(`  ${o.class} c=${o.clients}: ${o.rps} req/s  p50 ${o.p50} ms  p99 ${o.p99} ms  cpu/req ${o.cpuMsPerRequest} ms  rss ${o.rssMiB} MiB  ok ${o.ok} 4xx ${o.s4xx} 5xx ${o.s5xx} err ${o.errors}${o.invoice ? "\n    invoice " + Object.entries(o.invoice).map(([k, v]) => `${k}=${v}`).join(" ") : ""}`);
  '
}

conformant() {
  local name="$1"; local port; port=$(port_of "$name")
  if (cd "$here" && node "$here/conformance.mjs" "http://127.0.0.1:$port") > "$OUT/$name.conformance.txt" 2>&1; then return 0; fi
  log "$name deviates from the contract (see $OUT/$name.conformance.txt): not measured"; grep '✗' "$OUT/$name.conformance.txt"; return 1
}

invoice() {
  for name in ${1:-$SERVERS}; do
    available "$name" || { log "$name: not installed, skipped"; continue; }
    log "== $name $(version_of "$name") — invoice (c=1${PIN_SERVER:+, server on cpu $PIN_SERVER}${PIN_CLIENT:+, client on cpu $PIN_CLIENT})"
    USAI_PROFILE=$([ "$name" = usai ] && echo 1 || echo "") start_server "$name" || continue
    conformant "$name" || { stop_server "$name"; continue; }
    for cls in A B C D E F; do cell "$name" "$cls" 1; done
    stop_server "$name"
  done
}

sweep() {
  for name in ${1:-$SERVERS}; do
    available "$name" || { log "$name: not installed, skipped"; continue; }
    log "== $name $(version_of "$name") — sweep"
    start_server "$name" || continue
    conformant "$name" || { stop_server "$name"; continue; }
    local concs="$CONCS"
    # As shipped, FPM has five children: c=1 only. Tuned, up to its pool size.
    if [ "$name" = php ]; then concs="1"; elif is_php "$name"; then concs=$(for c in $CONCS; do [ "$c" -le "$PHP_CHILDREN" ] && echo "$c"; done | tr '\n' ' '); fi
    for cls in A B C D E F; do for c in $concs; do cell "$name" "$cls" "$c"; done; done
    stop_server "$name"
  done
}

leak() {
  for name in ${1:-$SERVERS}; do
    available "$name" || continue
    log "== $name — correctness probes"
    start_server "$name" || continue
    local port; port=$(port_of "$name"); local base="http://127.0.0.1:$port"
    # 1. cross-request state: ten requests to the counter
    local counts; counts=$(for i in $(seq 1 10); do curl -s "$base/counter" | node -pe 'JSON.parse(require("fs").readFileSync(0)).count'; done | tr '\n' ' ')
    echo "  counter over 10 requests: $counts"
    # 2. cancellation: 20 clients open /slow?ms=5000 and abort after 100 ms; then health and (usai) live worlds
    node -e '
      const base = process.argv[1]; const ctls = [];
      for (let i = 0; i < 20; i++) { const c = new AbortController(); ctls.push(c); fetch(`${base}/slow?ms=5000`, { signal: c.signal }).catch(() => {}); }
      setTimeout(() => { for (const c of ctls) c.abort(); }, 100);
      setTimeout(async () => { const r = await fetch(`${base}/health`); console.log(`  after 20 aborted slow requests: health ${r.status}`); process.exit(0); }, 600);
    ' "$base"
    if [ "$name" = usai ]; then
      sleep 1; echo "  live worlds now: $(curl -s "$base/_usai/status" 2>/dev/null | grep -o '"liveWorlds":[0-9]*' || echo "(status not exposed; start with --status to see)")"
    fi
    # 3. RSS drift: 10 000 requests of class C
    local r0; r0=$(rss_mib "$name")
    DUR=10 cell "$name" C 8 >/dev/null
    local r1; r1=$(rss_mib "$name")
    echo "  rss before/after 10 s of class C at c=8: $r0 → $r1 MiB"
    stop_server "$name"
  done
}

report() {
  node "$here/report.mjs" "$OUT" > "$OUT/report.md" && log "report: $OUT/report.md"
}

case "${1:-}" in
  prepare) prepare ;;
  invoice) invoice "${2:-}"; report ;;
  sweep) sweep "${2:-}"; report ;;
  leak) leak "${2:-}" ;;
  all) prepare && invoice && sweep && leak; report ;;
  *) sed -n 2,20p "$0"; exit 2 ;;
esac
