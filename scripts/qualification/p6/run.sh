#!/usr/bin/env bash
# P6 reliability qualification, on the P5 deployment (scripts/qualification/p5).
#
#   scripts/qualification/p6/run.sh churn <n>        # n revision replacements under load, no lost request
#   scripts/qualification/p6/run.sh db-flap <n>      # n PostgreSQL kill/restart cycles under load; pool recovers each time
#   scripts/qualification/p6/run.sh restart-loop <n> # n SIGTERM → drain → restart cycles under load
#   scripts/qualification/p6/run.sh overload <s>     # 128 clients: refusals are 503 capacity, never 5xx, latency bounded
#   scripts/qualification/p6/run.sh idle-burst       # 60 s idle then a burst: first-second latency and errors
#   scripts/qualification/p6/run.sh dead-letter      # a webhook endpoint that always fails: 5 attempts then dead
#   scripts/qualification/p6/run.sh soak <seconds>   # steady load; status sampled every 60 s (memory plateau, ownership)
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
rss_mb() { docker stats --no-stream --format "{{.MemUsage}}" usai-p5-app-1 | sed 's/MiB.*//; s/ //g'; }
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

soak() {
  local s="${1:-3600}"; local out="$here/out/soak.load.jsonl"; local samples="$here/out/soak.samples.jsonl"; rm -f "$out" "$samples"
  echo "soak start $(date -u +%FT%TZ) for $s s"
  load "$s" "$out"; local pid=$LOAD_PID
  local t0=$SECONDS
  while kill -0 "$pid" 2>/dev/null; do
    sleep 60
    echo "{\"t\":$((SECONDS - t0)),\"rssMiB\":\"$(rss_mb)\",\"status\":$(status)}" >> "$samples"
  done
  echo "soak end $(date -u +%FT%TZ); load: $(summarize "$out")"
  node -e '
    const fs=require("fs"); const s=fs.readFileSync(process.argv[1],"utf8").trim().split("\n").map(JSON.parse);
    const rss=s.map(x=>parseFloat(x.rssMiB)); const g=s.map(x=>x.status.gauges||{});
    console.log(JSON.stringify({samples:s.length, rssFirst:rss[0], rssMax:Math.max(...rss), rssLast:rss[rss.length-1], liveWorldsMax:Math.max(...g.map(x=>x.liveWorlds||0)), liveOpsLast:g[g.length-1].liveOps, quarantined:(s[s.length-1].status.resources||[]).map(r=>r.quarantined)}));
  ' "$samples"
}

case "${1:-}" in
  churn) churn "${2:-200}" ;;
  db-flap) db_flap "${2:-10}" ;;
  restart-loop) restart_loop "${2:-10}" ;;
  overload) overload "${2:-60}" ;;
  idle-burst) idle_burst ;;
  dead-letter) dead_letter ;;
  soak) soak "${2:-3600}" ;;
  *) echo "usage: $0 churn|db-flap|restart-loop|overload|idle-burst|dead-letter|soak"; exit 2 ;;
esac
