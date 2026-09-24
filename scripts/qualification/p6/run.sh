#!/usr/bin/env bash
# P6 reliability qualification, on the P5 deployment (scripts/qualification/p5).
#
#   scripts/qualification/p6/run.sh churn <n>        # n revision replacements under load, no lost request
#   scripts/qualification/p6/run.sh db-flap <n>      # n PostgreSQL kill/restart cycles under load; pool recovers each time
#   scripts/qualification/p6/run.sh restart-loop <n> # n SIGTERM → drain → restart cycles under load
#   scripts/qualification/p6/run.sh overload <s>     # 128 clients: refusals are 503 capacity, never 5xx, latency bounded
#   scripts/qualification/p6/run.sh idle-burst       # 60 s idle then a burst: first-second latency and errors
#   scripts/qualification/p6/run.sh dead-letter      # a webhook endpoint that always fails: 5 attempts then dead
#   scripts/qualification/p6/run.sh soak <seconds> [growing|bounded]
#                                                    # steady load; status sampled every 60 s (memory plateau,
#                                                    # ownership). `bounded` prunes the data the load makes, so a
#                                                    # decay cannot be blamed on the dataset — the control run the
#                                                    # 72 h soak never had.
#   scripts/qualification/p6/run.sh conn-churn       # connection-bound worlds (SSE + WebSocket): 500 cycles, held through a
#                                                     revision replacement, an app restart and a proxy restart, abrupt client
#                                                     death, a client that never reads, an idle socket (USAI_SOCKET_IDLE_TIMEOUT)
#   scripts/qualification/p6/run.sh replicas <cmd>   # two replicas behind one proxy (p6/replicas.sh: up | all | http | queue |
#                                                     migrate | cron | rolling | kill | down), its own compose project
# Campaign steps tolerate failing curls (that is the point); only the setup is strict.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
p5="$here/../p5"
cd "$p5"
export COMPOSE_PROJECT_NAME=usai-p5
BASE="http://127.0.0.1:8080"
CONTROL="http://127.0.0.1:3900"
TOKEN=$(cat out/token)
AUTH=(-H "authorization: Bearer p5-control-token")
mkdir -p "$here/out"
status() { curl -s -m 3 "$BASE/_usai/status" || echo '{"unreachable":true}'; }
health() { curl -s -m 2 -o /dev/null -w "%{http_code}" "$BASE/invoices" || echo 000; }
wait_healthy() { local d=$((SECONDS + ${1:-120})); while [ $SECONDS -lt $d ]; do [ "$(health)" = "401" ] && return 0; sleep 0.5; done; return 1; }
# What `docker stats` reports is the **cgroup's charge**, which is the number
# that gets the container killed. It is not the process's RSS, and on this
# runtime the two differ by tens of MiB in both directions at once: RSS counts
# the pooled Wasm image once per slot it is mapped into (over), while the
# cgroup is not charged for text pages another cgroup faulted in first (under)
# — see docs/measurements/2026-09-24-bounded-soak.md. The samples carry
# `/_usai/status`, so the summary reports all three rather than one labelled
# "RSS" that is not one.
rss_mb() { docker stats --no-stream --format "{{.MemUsage}}" usai-p5-app-1 | sed 's/MiB.*//; s/ //g'; }
cpu_pct() { docker stats --no-stream --format "{{.CPUPerc}}" usai-p5-app-1 | tr -d '%'; }
fds() { docker exec usai-p5-app-1 sh -c 'ls /proc/1/fd | wc -l' 2>/dev/null || echo null; }
summarize() {
  node -e '
    const fs=require("fs"); const lines=fs.readFileSync(process.argv[1],"utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
    const sum=(k)=>lines.reduce((a,l)=>a+(l[k]||0),0);
    const bad=lines.filter(l=>l.s5xx+l.errors>0).length;
    const p99s=lines.map(l=>l.p99||0).sort((a,b)=>a-b);
    console.log(JSON.stringify({seconds:lines.length, ok:sum("ok"), s4xx:sum("s4xx"), s5xx:sum("s5xx"), s503:sum("s503"), errors:sum("errors"), badSeconds:bad, p99median:p99s[Math.floor(p99s.length/2)], p99max:p99s[p99s.length-1]}));
  ' "$1"
}
# One load generator at a time: a campaign that ends early must not leave
# its load running into the next one.
load() {
  if [ -f "$here/out/load.pid" ]; then kill "$(cat "$here/out/load.pid")" 2>/dev/null || true; fi
  node "$p5/loadgen.mjs" "$BASE" "$TOKEN" "${CLIENTS:-8}" "$1" "$2" > /dev/null &
  LOAD_PID=$!; echo $LOAD_PID > "$here/out/load.pid"
}

churn() {
  local n="${1:-200}"; local out="$here/out/churn.load.jsonl"; rm -f "$out"
  local dur=$((n * 2 + 20)); load "$dur" "$out"; local pid=$LOAD_PID
  sleep 5
  local t0=$SECONDS; local failed=0
  local installs_refused=0
  for i in $(seq 1 "$n"); do
    local body; body=$(curl -s -X POST "$CONTROL/revisions" "${AUTH[@]}" -H 'content-type: application/json' -d '{"artifact":"/app/.usai/build"}' || true)
    local r; r=$(echo "$body" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
    if [ -z "$r" ]; then installs_refused=$((installs_refused + 1)); echo "install $i refused: $body" | cut -c1-200; sleep 0.5; continue; fi
    local code; code=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$CONTROL/revisions/$r/activate" "${AUTH[@]}" || echo 000)
    [ "$code" = "200" ] || { failed=$((failed + 1)); echo "activate rev$r: $code"; }
    # Drained revisions leave the runtime by themselves (drain removes them);
    # the previous active one is draining now — wait for it so the bound of
    # held revisions is never the limiting factor.
    while curl -s "$CONTROL/revisions" "${AUTH[@]}" | grep -q '"state":"draining"'; do sleep 0.05; done
  done
  echo "$n replacements in $((SECONDS - t0)) s, $failed activation failures, $installs_refused installs refused"
  wait "$pid" 2>/dev/null || true
  echo "load: $(summarize "$out")"; status > "$here/out/churn.status.json"
  echo "rss now: $(rss_mb) MiB; images live: $(status | grep -o '"compiledImagesLive":[0-9]*')"
  echo "revisions now: $(status | grep -o '"state":"[a-z]*"' | sort | uniq -c | tr '\n' ' ')"
}

db_flap() {
  local n="${1:-10}"; local out="$here/out/db-flap.load.jsonl"; rm -f "$out"
  load $((n * 12 + 10)) "$out"; local pid=$LOAD_PID; sleep 5
  for i in $(seq 1 "$n"); do docker kill -s SIGKILL usai-p5-postgres-1 >/dev/null; sleep 2; docker start usai-p5-postgres-1 >/dev/null; sleep 8; done
  wait_healthy 60 || echo "NOT HEALTHY after flaps"
  wait "$pid" 2>/dev/null || true
  echo "load: $(summarize "$out")"
  status | grep -o '"quarantined":[0-9]*\|"poolSize":[0-9]*\|"available":[0-9]*' | tr '\n' ' '; echo
}

restart_loop() {
  local n="${1:-10}"; local out="$here/out/restart-loop.load.jsonl"; rm -f "$out"
  load $((n * 8 + 10)) "$out"; local pid=$LOAD_PID; sleep 5
  local total=0
  for i in $(seq 1 "$n"); do
    local t=$SECONDS
    docker kill -s SIGTERM usai-p5-app-1 >/dev/null
    while [ "$(docker inspect -f '{{.State.Running}}' usai-p5-app-1)" = "true" ]; do sleep 0.1; done
    docker compose start app >/dev/null 2>&1
    wait_healthy 60 || { echo "cycle $i: not healthy"; break; }
    total=$((total + SECONDS - t))
  done
  wait "$pid" 2>/dev/null || true
  echo "$n restart cycles, mean $((total / n)) s each; load: $(summarize "$out")"
  docker logs usai-p5-app-1 2>&1 | grep -c "ownership returned to baseline" | sed 's/^/drain lines: /'
}

overload() {
  local s="${1:-60}"; local out="$here/out/overload.load.jsonl"; rm -f "$out"
  CLIENTS=128 node "$p5/loadgen.mjs" "$BASE" "$TOKEN" 128 "$s" "$out" > /dev/null
  echo "load: $(summarize "$out")"
  curl -s "$BASE/_usai/metrics" | grep -E "usai_http_rejections_total|usai_worlds_live|usai_http_request_seconds_(sum|count)"
}

idle_burst() {
  echo "idle 60 s"; sleep 60
  local out="$here/out/idle-burst.load.jsonl"; rm -f "$out"
  CLIENTS=32 node "$p5/loadgen.mjs" "$BASE" "$TOKEN" 32 10 "$out" > /dev/null
  echo "first seconds:"; head -3 "$out"
}

dead_letter() {
  # A tenant whose webhook always answers 500: five attempts, then dead.
  local sink; sink=$(docker network inspect usai-p5_default -f '{{range .IPAM.Config}}{{.Gateway}}{{end}}')
  node -e 'require("http").createServer((q,r)=>{r.writeHead(500);r.end("no")}).listen(18999,"0.0.0.0")' &
  local spid=$!
  sleep 1
  curl -s -X PUT "$BASE/tenant/webhook" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d "{\"url\":\"http://$sink:18999/hook\",\"secret\":\"dead-letter-secret-1234\"}" > /dev/null
  local inv; inv=$(curl -s -X POST "$BASE/invoices" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"customer":"dead","currency":"USD","dueDate":"2030-01-01","items":[{"description":"x","quantity":1,"unitCents":100}]}' | sed 's/.*"id":"\([^"]*\)".*/\1/')
  curl -s -o /dev/null -X POST "$BASE/invoices/$inv/issue" -H "authorization: Bearer $TOKEN"
  echo "waiting for 5 attempts (500 ms exponential: ~15 s)"
  for i in $(seq 1 60); do
    dead=$(status | grep -o '"dead":[0-9]*' | head -1 | cut -d: -f2)
    [ "${dead:-0}" -ge 1 ] && break
    sleep 1
  done
  status | grep -o '"queue":{[^}]*}' | head -1
  docker compose exec -T postgres psql -U app -d invoicing -tAc "select attempt, status, error from webhook_deliveries where invoice_id = '$inv' order by attempt" 
  docker compose exec -T postgres psql -U app -d invoicing -tAc "select state, attempts, last_error from usai_queue where topic = 'webhook.deliver' order by id desc limit 1"
  kill $spid 2>/dev/null || true
  curl -s -X PUT "$BASE/tenant/webhook" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"url":"http://127.0.0.1:9/","secret":"dead-letter-secret-1234"}' > /dev/null
}

live_worlds() { status | grep -o '"liveWorlds":[0-9]*' | cut -d: -f2; }
wait_baseline() { local d=$((SECONDS + ${1:-30})); while [ $SECONDS -lt $d ]; do [ "$(live_worlds)" = "0" ] && { echo "live worlds back to 0 after $((SECONDS - d + ${1:-30})) s"; return 0; }; sleep 0.5; done; echo "live worlds still $(live_worlds) after ${1:-30} s"; return 1; }
conn() { node "$here/connchurn.mjs" "$BASE" "$TOKEN" "$@"; }

conn_churn() {
  local n="${1:-500}"
  echo "== cycles: $n × (SSE two events + WS hello/ask/answer), 16 at a time"
  conn cycles "$n" 16
  wait_baseline 15

  echo "== held through a revision replacement: 40 connections (USAI_MAX_WORLDS is 48 here), replacement at t=10 s"
  conn hold 40 40 > "$here/out/conn.replace.json" &
  local hp=$!; sleep 10
  local body; body=$(curl -s -X POST "$CONTROL/revisions" "${AUTH[@]}" -H 'content-type: application/json' -d '{"artifact":"/app/.usai/build"}')
  local r; r=$(echo "$body" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  local t0=$SECONDS
  curl -s -o /dev/null -X POST "$CONTROL/revisions/$r/activate" "${AUTH[@]}"
  while curl -s "$CONTROL/revisions" "${AUTH[@]}" | grep -q '"state":"draining"'; do sleep 0.2; done
  echo "replacement rev$r active; previous drained in $((SECONDS - t0)) s (connections are asked to stop, then closed at the drain bound)"
  wait $hp; cat "$here/out/conn.replace.json"
  wait_baseline 15

  echo "== held through an app restart (SIGTERM → drain → start): 40 connections (USAI_MAX_WORLDS is 48 here), restart at t=10 s"
  conn hold 40 40 > "$here/out/conn.restart.json" &
  hp=$!; sleep 10
  t0=$SECONDS
  docker kill -s SIGTERM usai-p5-app-1 >/dev/null
  while [ "$(docker inspect -f '{{.State.Running}}' usai-p5-app-1)" = "true" ]; do sleep 0.1; done
  echo "app exited after $((SECONDS - t0)) s"
  docker compose start app >/dev/null 2>&1
  wait_healthy 60 || echo "NOT HEALTHY after restart"
  echo "healthy again after $((SECONDS - t0)) s"
  wait $hp; cat "$here/out/conn.restart.json"
  wait_baseline 15

  echo "== held through a proxy restart: 40 connections (USAI_MAX_WORLDS is 48 here), caddy restarted at t=10 s"
  conn hold 40 40 > "$here/out/conn.proxy.json" &
  hp=$!; sleep 10
  t0=$SECONDS
  docker compose restart caddy >/dev/null 2>&1
  wait_healthy 60 || echo "NOT HEALTHY after proxy restart"
  echo "proxy back after $((SECONDS - t0)) s"
  wait $hp; cat "$here/out/conn.proxy.json"
  wait_baseline 15

  echo "== abrupt client death: 40 connections (USAI_MAX_WORLDS is 48 here), client killed -9 at t=5 s (no close frames, no aborts)"
  conn hold 40 60 > "$here/out/conn.abrupt.json" &
  hp=$!; sleep 5
  echo "live worlds before kill: $(live_worlds)"
  kill -9 $hp; wait $hp 2>/dev/null || true
  wait_baseline 30

  echo "== a client that never reads: 10 SSE connections unread for 30 s (rss before/after)"
  echo "rss before: $(rss_mb) MiB"
  for i in $(seq 1 10); do conn slow 30 > /dev/null & done; wait
  echo "rss after: $(rss_mb) MiB"
  wait_baseline 15

  echo "== idle socket: nothing sent; the server closes it (USAI_SOCKET_IDLE_TIMEOUT=${USAI_SOCKET_IDLE_TIMEOUT:-300} s in this deployment)"
  conn idle $(( ${USAI_SOCKET_IDLE_TIMEOUT:-300} + 5 ))
  wait_baseline 15
  status > "$here/out/conn.status.json"
  echo "gauges: $(status | grep -o '"gauges":{[^}]*}')"
}

# `soak <seconds> bounded` keeps the dataset from growing: every minute it
# deletes the invoices and queue rows the load just made, so the table stays
# the size it started at. The 72 h soak's throughput decayed 794 → 154 req/s
# with a flat p50 and a flat per-request CPU, and the explanation was the 7.4
# million invoices it had written — an explanation with no control run behind
# it. This is the control run: same load, same duration, a dataset that does
# not grow. If the decay is gone, the runtime was never the cause; if it is
# still there, it is ours.
soak() {
  local s="${1:-3600}"; local mode="${2:-growing}"
  local out="$here/out/soak.load.jsonl"; local samples="$here/out/soak.samples.jsonl"; rm -f "$out" "$samples"
  echo "soak start $(date -u +%FT%TZ) for $s s (dataset: $mode)"
  load "$s" "$out"; local pid=$LOAD_PID
  local t0=$SECONDS
  local pruned=0
  while kill -0 "$pid" 2>/dev/null; do
    sleep 60
    if [ "$mode" = bounded ]; then
      # Keep the newest 20 000 invoices and the queue's finished rows for one
      # minute; both are the application's data, deleted the way an operator
      # would (`usai queue prune` is the supported verb for the second).
      local gone
      gone=$(docker compose exec -T postgres psql -U app -d invoicing -tAc \
        "with victims as (select id from invoices order by created_at desc offset 20000)
         delete from invoices where id in (select id from victims) returning 1" 2>/dev/null | grep -c 1)
      docker compose exec -T postgres psql -U app -d invoicing -qc \
        "delete from usai_queue where state in ('done','dead') and coalesce(locked_at, available_at) < now() - interval '1 minute'" >/dev/null 2>&1
      pruned=$((pruned + ${gone:-0}))
    fi
    # One sample a minute: memory, CPU, open descriptors and the runtime's
    # own status (ownership gauges, pool/quarantine, queue depth, images,
    # latency histogram) — the drift a long soak is for.
    echo "{\"t\":$((SECONDS - t0)),\"rssMiB\":\"$(rss_mb)\",\"cpuPct\":\"$(cpu_pct)\",\"fds\":$(fds),\"status\":$(status)}" >> "$samples"
  done
  echo "soak end $(date -u +%FT%TZ); dataset: $mode${pruned:+, pruned $pruned invoices}; load: $(summarize "$out")"
  node -e '
    const fs=require("fs"); const s=fs.readFileSync(process.argv[1],"utf8").trim().split("\n").map(JSON.parse);
    const rss=s.map(x=>parseFloat(x.rssMiB)); const g=s.map(x=>x.status.gauges||{});
    const fds=s.map(x=>x.fds).filter(x=>typeof x==="number"); const cpu=s.map(x=>parseFloat(x.cpuPct)).filter(x=>!isNaN(x));
    const q=(a,p)=>{const b=[...a].sort((x,y)=>x-y); return b.length?b[Math.min(b.length-1,Math.floor(p*b.length))]:null;};
    const proc=s.map(x=>x.status.process||{}); const mib=(k)=>proc.map(x=>(x[k]||0)/1024);
    const procRss=mib("rssKib"); const procPss=mib("pssKib");
    console.log(JSON.stringify({samples:s.length, chargedFirst:rss[0], chargedMax:Math.max(...rss), chargedLast:rss[rss.length-1], procRssFirst:+procRss[0].toFixed(1), procRssLast:+procRss[procRss.length-1].toFixed(1), procPssFirst:+procPss[0].toFixed(1), procPssLast:+procPss[procPss.length-1].toFixed(1), fdsFirst:fds[0]??null, fdsMax:fds.length?Math.max(...fds):null, fdsLast:fds[fds.length-1]??null, cpuMedian:q(cpu,0.5), cpuP95:q(cpu,0.95), liveWorldsMax:Math.max(...g.map(x=>x.liveWorlds||0)), liveOpsLast:g[g.length-1].liveOps, worldsCreated:g[g.length-1].worldsCreated, detached:g[g.length-1].detachedWorkDetected, imagesLive:s[s.length-1].status.compiledImagesLive, quarantined:(s[s.length-1].status.resources||[]).map(r=>r.quarantined)}));
  ' "$samples"
}

case "${1:-}" in
  churn) churn "${2:-200}" ;;
  db-flap) db_flap "${2:-10}" ;;
  restart-loop) restart_loop "${2:-10}" ;;
  overload) overload "${2:-60}" ;;
  idle-burst) idle_burst ;;
  dead-letter) dead_letter ;;
  replicas) shift; exec "$here/replicas.sh" "$@" ;;
  soak) soak "${2:-3600}" "${3:-growing}" ;;
  conn-churn) conn_churn "${2:-500}" ;;
  *) echo "usage: $0 churn|db-flap|restart-loop|overload|idle-burst|dead-letter|soak|conn-churn|replicas"; exit 2 ;;
esac
