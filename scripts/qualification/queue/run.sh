#!/usr/bin/env bash
# Queue throughput campaign: the PostgreSQL-backed queue's messages per
# second, per consumer concurrency, on one and two instances; the producer
# route's cost; the claim-to-run latency.
#
#   run.sh prepare                       # build the app for each concurrency, migrate
#   run.sh cell <concurrency> <instances> <messages>   # one cell → out/<ts>/cell-*.json
#   run.sh all                           # 4/8/16 × 1/2 instances × 20 000 messages, then a report
#
# Environment: DATABASE_URL (a throwaway database), USAI (binary), PIN (cpu
# list), OUT (directory). Ports 4400–4409 (app), 4410–4419 (status).
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
: "${DATABASE_URL:?DATABASE_URL is required (a throwaway database)}"
USAI="${USAI:-$repo/target/release/usai}"
PIN="${PIN:-}"
OUT="${OUT:-$here/out/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT"
log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$OUT/log.txt"; }
pin_bg() { if [ -n "$PIN" ]; then exec taskset -c "$PIN" "$@"; else exec "$@"; fi; }
psql_q() { node -e '
  const { Client } = require("pg"); const c = new Client({ connectionString: process.env.DATABASE_URL });
  c.connect().then(() => c.query(process.argv[1])).then((r) => { console.log(JSON.stringify(r.rows)); return c.end(); }).catch((e) => { console.error(e.message); process.exit(1); });
' "$1"; }
export PATH="$repo/scripts/qualification/bench/node_modules/.bin:$PATH"
NODE_PATH="$repo/scripts/qualification/bench/node_modules"; export NODE_PATH

# One artifact per concurrency: the number is part of the definition.
prepare() {
  for c in 4 8 16; do
    local dir="$OUT/app-c$c"
    rm -rf "$dir"; mkdir -p "$dir"
    cp -r "$here/app/src" "$here/app/migrations" "$here/app/usai.config.ts" "$here/app/package.json" "$here/app/tsconfig.json" "$dir/"
    ln -s "$here/app/node_modules" "$dir/node_modules"
    sed -i "s/const concurrency = 8; \/\/ QUEUE_CONCURRENCY/const concurrency = $c; \/\/ QUEUE_CONCURRENCY/" "$dir/src/app.ts"
    "$USAI" --root "$dir" build --no-typecheck >/dev/null || { log "build failed for c=$c"; return 1; }
  done
  psql_q "drop schema public cascade; create schema public;" >/dev/null
  "$USAI" --root "$OUT/app-c8" db migrate >/dev/null
  log "prepared"
}

cell() {
  local c="$1" n="$2" messages="$3"
  local name="c$c-i$n-m$messages"
  local dir="$OUT/cell-$name"; mkdir -p "$dir"
  log "== cell $name"
  psql_q "truncate processed; do \$\$ begin if to_regclass('usai_queue') is not null then delete from usai_queue where topic = 'bench.work'; end if; end \$\$;" >/dev/null
  local pids=()
  for i in $(seq 1 "$n"); do
    pin_bg env USAI_MAX_WORLDS=64 "$USAI" --root "$OUT/app-c$c" run --artifact "$OUT/app-c$c/.usai/build" --port $((4400 + i)) --status-addr "127.0.0.1:$((4410 + i))" --drain-timeout 5 > "$dir/app-$i.log" 2>&1 &
    pids+=($!)
  done
  for i in $(seq 1 "$n"); do until curl -sf -m 2 "http://127.0.0.1:$((4410 + i))/_usai/ready" >/dev/null 2>&1; do sleep 0.2; done; done
  # Producer cost: 2 000 single-message publishes through the route, one client.
  local t0 t1
  t0=$(date +%s.%N)
  node -e '
    const base = process.argv[1]; const total = 2000; let done = 0;
    (async () => { for (let i = 0; i < total; i++) { const r = await fetch(`${base}/enqueue`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ n: 1000000 + i, batch: "route" }) }); if (r.status === 202) done++; } console.log(JSON.stringify({ done })); })();
  ' "http://127.0.0.1:4401" > "$dir/producer.json"
  t1=$(date +%s.%N)
  local producer_ms; producer_ms=$(echo "($t1 - $t0) * 1000 / 2000" | bc -l | cut -c1-6)
  # Let the consumers drain the 2 000, then the bulk publish.
  until [ "$(psql_q "select count(*)::int as n from usai_queue where topic = 'bench.work' and state <> 'done'" | tr -dc 0-9)" = 0 ]; do sleep 0.5; done
  psql_q "truncate processed; do \$\$ begin if to_regclass('usai_queue') is not null then delete from usai_queue where topic = 'bench.work'; end if; end \$\$;" >/dev/null
  # Bulk: one world publishes `messages` messages (each an owned INSERT); the
  # consumers start eating as soon as rows appear, so the drain clock starts
  # at the first publish.
  t0=$(date +%s.%N)
  "$USAI" --root "$OUT/app-c$c" app publish -- "$messages" "bulk" > "$dir/publish.json" 2>/dev/null
  local published_at; published_at=$(date +%s.%N)
  until [ "$(psql_q "select count(*)::int as n from usai_queue where topic = 'bench.work' and state <> 'done'" | tr -dc 0-9)" = 0 ]; do sleep 0.25; done
  t1=$(date +%s.%N)
  local processed; processed=$(psql_q "select count(*)::int as n from processed" | tr -dc 0-9)
  # Claim-to-run latency: per message, processed.at − usai_queue.created_at.
  local latency; latency=$(psql_q "select round(percentile_cont(0.5) within group (order by extract(epoch from p.at - q.created_at)) * 1000)::int as p50_ms, round(percentile_cont(0.99) within group (order by extract(epoch from p.at - q.created_at)) * 1000)::int as p99_ms, round(max(extract(epoch from p.at - q.created_at)) * 1000)::int as max_ms from processed p join usai_queue q on q.id = (select id from usai_queue where topic = 'bench.work' and (payload->>'n')::bigint = p.id and payload->>'batch' = 'bulk' limit 1)")
  local per_instance; per_instance=$(psql_q "select split_part(consumer, ':', 1) as revision, count(*)::int as n from processed group by 1 order by 1")
  for i in $(seq 1 "$n"); do curl -s -m 3 "http://127.0.0.1:$((4410 + i))/_usai/status" > "$dir/status-$i.json"; done
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null; done
  for p in "${pids[@]}"; do wait "$p" 2>/dev/null; done
  local total_s; total_s=$(echo "$t1 - $t0" | bc -l | cut -c1-6)
  local publish_s; publish_s=$(echo "$published_at - $t0" | bc -l | cut -c1-6)
  local rate; rate=$(echo "$messages / ($t1 - $t0)" | bc -l | cut -c1-8)
  echo "{\"cell\":\"$name\",\"concurrency\":$c,\"instances\":$n,\"messages\":$messages,\"processed\":$processed,\"seconds\":$total_s,\"publishSeconds\":$publish_s,\"messagesPerSecond\":$rate,\"producerRouteMsPerPublish\":$producer_ms,\"latency\":$latency,\"perInstance\":$per_instance}" > "$dir/result.json"
  log "  $messages messages in $total_s s ($rate msg/s; the publisher took $publish_s s of it), processed $processed, producer route $producer_ms ms/publish, latency $latency"
}

report() {
  echo "| concurrency | instances | messages | seconds | msg/s | publish s | claim→run p50 / p99 / max ms | producer route ms/publish | per instance |"
  echo "|---|---|---|---|---|---|---|---|---|"
  for f in "$OUT"/cell-*/result.json; do
    node -e '
      const r = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); const l = r.latency[0] || {};
      console.log(`| ${r.concurrency} | ${r.instances} | ${r.messages} | ${r.seconds} | ${Number(r.messagesPerSecond).toFixed(0)} | ${r.publishSeconds} | ${l.p50_ms} / ${l.p99_ms} / ${l.max_ms} | ${r.producerRouteMsPerPublish} | ${r.perInstance.map((p) => `${p.revision}: ${p.n}`).join(", ")} |`);
    ' "$f"
  done
}

all() {
  prepare || return 1
  for n in 1 2; do for c in 4 8 16; do cell "$c" "$n" "${MESSAGES:-20000}"; done; done
  report | tee "$OUT/report.md"
}

case "${1:-}" in
  prepare) prepare;;
  cell) shift; cell "$@";;
  all) all;;
  report) report;;
  *) sed -n 2,12p "$0"; exit 2;;
esac
