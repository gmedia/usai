#!/usr/bin/env bash
# P6 — two replicas behind one proxy, one PostgreSQL (the topology
# SUPPORTED.md states): scripts/qualification/p5/compose.replicas.yaml, its own
# compose project so it runs beside the P5/P6 deployment, confined to CPUSET.
#
#   scripts/qualification/p6/replicas.sh up            # postgres, migrate once, app + app2, caddy; a tenant token
#   scripts/qualification/p6/replicas.sh all           # every scenario below, evidence under out/replicas.*
#   scripts/qualification/p6/replicas.sh <scenario>    # http | queue | migrate | cron | rolling | kill
#   scripts/qualification/p6/replicas.sh down
#
# Pass rules are stated before each scenario runs and checked by the script;
# a scenario prints PASS or FAIL with the numbers, never just one word.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
p5="$here/../p5"
cd "$p5"
export COMPOSE_PROJECT_NAME=usai-p6r COMPOSE_FILE=compose.replicas.yaml
export CPUSET="${CPUSET:-12-15}"
BASE="http://127.0.0.1:8081"
APP1="http://127.0.0.1:3001"; APP2="http://127.0.0.1:3002"
CONTROL1="http://127.0.0.1:3901"
AUTH=(-H "authorization: Bearer p5-control-token")
OUT="$here/out"; mkdir -p "$OUT"
TOKEN_FILE="$OUT/replicas.token"
TOKEN="$(cat "$TOKEN_FILE" 2>/dev/null || true)"
PG=(docker compose exec -T postgres psql -U app -d invoicing -tAc)

log() { echo "[$(date -u +%H:%M:%S)] $*"; }
verdict() { # name ok "evidence"
  if [ "$2" = 1 ]; then echo "PASS $1 — $3"; else echo "FAIL $1 — $3"; FAILED=$((FAILED + 1)); fi
}
FAILED=0
status_of() { curl -s -m 3 "$1/_usai/status" || echo '{"unreachable":true}'; }
jget() { node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>{const o=JSON.parse(d);const v=process.argv[1].split(".").reduce((a,k)=>a?.[k],o);console.log(v===undefined?"null":typeof v==="object"?JSON.stringify(v):v)})' "$1"; }
worlds_created() { status_of "$1" | jget gauges.worldsCreated; }
queue_done() { status_of "$1" | node -e 'let d="";process.stdin.on("data",c=>d+=c).on("end",()=>{const o=JSON.parse(d);const r=(o.revisions||[]).find(r=>r.state==="active")||{};console.log(r.queue?r.queue.done:0)})'; }
schedules_cron() { status_of "$1" | jget scheduler.cron; }
ready() { curl -s -m 2 -o /dev/null -w "%{http_code}" "$1/_usai/ready" || echo 000; }
wait_ready() { local d=$((SECONDS + ${2:-120})); while [ $SECONDS -lt $d ]; do [ "$(ready "$1")" = 200 ] && return 0; sleep 0.25; done; return 1; }
summarize() {
  node -e '
    const fs=require("fs"); const lines=fs.readFileSync(process.argv[1],"utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
    const sum=(k)=>lines.reduce((a,l)=>a+(l[k]||0),0);
    const bad=lines.filter(l=>l.s5xx+l.errors>0).length;
    const p99s=lines.map(l=>l.p99||0).sort((a,b)=>a-b);
    console.log(JSON.stringify({seconds:lines.length, ok:sum("ok"), s4xx:sum("s4xx"), s5xx:sum("s5xx"), s503:sum("s503"), errors:sum("errors"), badSeconds:bad, p99median:p99s[Math.floor(p99s.length/2)], p99max:p99s[p99s.length-1]}));
  ' "$1"
}
load() { node "$p5/loadgen.mjs" "$BASE" "$TOKEN" "${CLIENTS:-8}" "$1" "$2" > /dev/null & LOAD_PID=$!; }
gateway() { docker network inspect "${COMPOSE_PROJECT_NAME}_default" -f '{{range .IPAM.Config}}{{.Gateway}}{{end}}'; }

up() {
  log "image: ${APP_IMAGE:-usai-p5-app:latest} $(docker image inspect "${APP_IMAGE:-usai-p5-app:latest}" -f "{{.Id}}" | cut -c8-19)"
  docker compose up -d --wait postgres
  docker compose run --rm --no-deps -T app db migrate --artifact /app/.usai/build
  docker compose up -d
  wait_ready "$APP1" || { echo "app not ready"; docker compose logs app | tail -20; exit 1; }
  wait_ready "$APP2" || { echo "app2 not ready"; docker compose logs app2 | tail -20; exit 1; }
  local body
  body=$(curl -s -X POST "$BASE/signup" -H 'content-type: application/json' -d '{"tenant":"replicas","email":"replicas@example.test","password":"replicas-test-password-1"}')
  echo "$body" | grep -q token || body=$(curl -s -X POST "$BASE/login" -H 'content-type: application/json' -d '{"tenant":"replicas","email":"replicas@example.test","password":"replicas-test-password-1"}')
  echo "$body" | sed 's/.*"token":"\([^"]*\)".*/\1/' > "$TOKEN_FILE"; TOKEN=$(cat "$TOKEN_FILE")
  log "up: $BASE → app ($APP1, schedules cron: $(schedules_cron "$APP1")) + app2 ($APP2, schedules cron: $(schedules_cron "$APP2"))"
}
down() { docker compose down -v --remove-orphans; }

# 1. HTTP: both replicas serve, nothing fails.
http() {
  log "== http: 8 clients, 60 s, round robin"
  local out="$OUT/replicas.http.jsonl"; rm -f "$out"
  local w1 w2; w1=$(worlds_created "$APP1"); w2=$(worlds_created "$APP2")
  load 60 "$out"; wait "$LOAD_PID" 2>/dev/null || true
  local s; s=$(summarize "$out"); echo "load: $s"
  local d1=$(( $(worlds_created "$APP1") - w1 )) d2=$(( $(worlds_created "$APP2") - w2 ))
  local ok=1; echo "$s" | grep -q '"s5xx":0,"s503":0,"errors":0' || ok=0; [ "$d1" -gt 0 ] && [ "$d2" -gt 0 ] || ok=0
  verdict http "$ok" "worlds created app $d1 / app2 $d2; $s"
}

# 2. Queue: every message consumed once, by either replica.
queue() {
  local n="${1:-1000}"
  log "== queue: $n invoices issued → $n webhook deliveries, consumed by both replicas"
  local sink; sink=$(gateway)
  node -e 'let n=0;require("http").createServer((q,r)=>{n++;r.writeHead(200);r.end("ok");}).listen(18998,"0.0.0.0");process.on("SIGTERM",()=>{console.log(n);process.exit(0)})' > "$OUT/replicas.sink.count" &
  local spid=$!; sleep 1
  curl -s -X PUT "$BASE/tenant/webhook" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d "{\"url\":\"http://$sink:18998/hook\",\"secret\":\"replicas-secret-1234\"}" > /dev/null
  local q1 q2; q1=$(queue_done "$APP1"); q2=$(queue_done "$APP2")
  local before; before=$("${PG[@]}" "select count(*) from webhook_deliveries where status = 200")
  node -e '
    const [base, token, n] = process.argv.slice(1); const headers={authorization:`Bearer ${token}`,"content-type":"application/json"};
    let next=0, failed=0;
    async function worker(){ while(next<Number(n)){ next++; try {
      const r=await fetch(`${base}/invoices`,{method:"POST",headers,body:JSON.stringify({customer:`q-${next}`,currency:"USD",dueDate:"2030-01-01",items:[{description:"x",quantity:1,unitCents:100}]})});
      const {id}=await r.json(); const i=await fetch(`${base}/invoices/${id}/issue`,{method:"POST",headers}); if(i.status>=300) failed++; } catch { failed++; } } }
    Promise.all(Array.from({length:16},worker)).then(()=>console.log(JSON.stringify({issued:Number(n)-failed,failed})));
  ' "$BASE" "$TOKEN" "$n"
  log "waiting for the topic to drain"
  for _ in $(seq 1 120); do
    local pend; pend=$("${PG[@]}" "select count(*) from usai_queue where topic = 'webhook.deliver' and state in ('ready','processing')")
    [ "${pend:-1}" = 0 ] && break; sleep 1
  done
  sleep 1
  local rows; rows=$("${PG[@]}" "select count(*) from webhook_deliveries where status = 200")
  local dead; dead=$("${PG[@]}" "select count(*) from usai_queue where topic = 'webhook.deliver' and state = 'dead'")
  local stuck; stuck=$("${PG[@]}" "select count(*) from usai_queue where topic = 'webhook.deliver' and state in ('ready','processing')")
  local d1=$(( $(queue_done "$APP1") - q1 )) d2=$(( $(queue_done "$APP2") - q2 ))
  kill -TERM $spid; wait $spid 2>/dev/null; local hits; hits=$(cat "$OUT/replicas.sink.count")
  local delivered=$(( rows - before ))
  local ok=1; [ "$delivered" = "$n" ] && [ "$hits" = "$n" ] && [ "$dead" = 0 ] && [ "$stuck" = 0 ] && [ "$d1" -gt 0 ] && [ "$d2" -gt 0 ] || ok=0
  verdict queue "$ok" "deliveries $delivered (sink saw $hits), done by app $d1 / app2 $d2, dead $dead, still pending $stuck"
  curl -s -X PUT "$BASE/tenant/webhook" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"url":"http://127.0.0.1:9/","secret":"replicas-secret-1234"}' > /dev/null
}

# 3. Migrations: two `migrate` jobs at once on a fresh database apply each file once.
migrate() {
  log "== migrate: two concurrent jobs on a fresh database"
  "${PG[@]}" "drop database if exists invoicing_fresh" >/dev/null; "${PG[@]}" "create database invoicing_fresh" >/dev/null
  local url="postgres://app:app@postgres:5432/invoicing_fresh"
  docker compose run --rm --no-deps -T -e DATABASE_URL="$url" app db migrate --artifact /app/.usai/build > "$OUT/replicas.migrate1.log" 2>&1 & local m1=$!
  docker compose run --rm --no-deps -T -e DATABASE_URL="$url" app db migrate --artifact /app/.usai/build > "$OUT/replicas.migrate2.log" 2>&1 & local m2=$!
  wait $m1; local e1=$?; wait $m2; local e2=$?
  local applied; applied=$(docker compose exec -T postgres psql -U app -d invoicing_fresh -tAc "select count(*) from usai_migrations")
  local files; files=$(docker compose run --rm --no-deps -T -e DATABASE_URL="$url" app db status --artifact /app/.usai/build 2>/dev/null | grep -c "applied" || true)
  local ok=1; [ "$e1" = 0 ] && [ "$e2" = 0 ] && [ "$applied" = 3 ] || ok=0
  verdict migrate "$ok" "exit codes $e1/$e2, usai_migrations rows $applied (3 files), status lines saying applied: $files"
  "${PG[@]}" "drop database if exists invoicing_fresh" >/dev/null
}

# 4. Cron: exactly one replica schedules; each says so.
cron() {
  log "== cron: app schedules, app2 runs with USAI_NO_CRON=1"
  local c1 c2; c1=$(schedules_cron "$APP1"); c2=$(schedules_cron "$APP2")
  local m1 m2; m1=$(curl -s "$APP1/_usai/metrics" | grep '^usai_scheduler{kind="cron"}' | awk '{print $2}'); m2=$(curl -s "$APP2/_usai/metrics" | grep '^usai_scheduler{kind="cron"}' | awk '{print $2}')
  local via; via=$(for _ in 1 2 3 4; do status_of "$BASE" | jget scheduler.cron; done | sort | uniq -c | tr '\n' ' ')
  local ok=1; [ "$c1" = true ] && [ "$c2" = false ] || ok=0
  verdict cron "$ok" "status.scheduler.cron app=$c1 app2=$c2; usai_scheduler{cron} $m1/$m2; through the proxy the answer alternates: $via (status behind a load balancer is one replica's — read replicas directly)"
  echo "note: the invoicing schedule is daily (15 0 * * *); that every scheduling instance fires is the scheduler's definition (tests/workloads cron_scheduler), not observed live here"
}

# 5. Rolling restart: no 502, sockets on the restarted replica closed 1012 only.
rolling() {
  log "== rolling: 8 clients 70 s; restart app2 at t=10, app at t=35; 20 connections held"
  local out="$OUT/replicas.rolling.jsonl"; rm -f "$out"
  load 70 "$out"; local lp=$LOAD_PID
  node "$here/connchurn.mjs" "$BASE" "$TOKEN" hold 20 65 > "$OUT/replicas.rolling.conn.json" & local hp=$!
  sleep 10
  local t=$SECONDS; docker compose restart -t 15 app2 >/dev/null 2>&1; wait_ready "$APP2" 60; local g2=$((SECONDS - t))
  log "app2 restarted; ready again after $g2 s"
  sleep $((25 - g2 > 0 ? 25 - g2 : 1))
  t=$SECONDS; docker compose restart -t 15 app >/dev/null 2>&1; wait_ready "$APP1" 60; local g1=$((SECONDS - t))
  log "app restarted; ready again after $g1 s"
  wait $lp 2>/dev/null || true; wait $hp 2>/dev/null || true
  local s; s=$(summarize "$out"); echo "load: $s"
  local conn; conn=$(cat "$OUT/replicas.rolling.conn.json"); echo "held: $conn"
  local ok=1; echo "$s" | grep -q '"s5xx":0' || ok=0; echo "$s" | grep -q '"errors":0' || ok=0
  echo "$conn" | grep -q '"serverCloses":20' || ok=0; echo "$conn" | grep -qv '1006' || ok=0
  verdict rolling "$ok" "restart → ready: app2 ${g2}s, app ${g1}s; $s; closes $(echo "$conn" | grep -o '"byReason":{[^}]*}')"
}

# 6. One replica killed -9 under load: bounded 502s (in-flight only), the queue drains anyway.
kill_one() {
  local slow="${KILL_SINK_MS:-2000}"
  log "== kill: 8 clients 60 s; app2 killed -9 at t=10 while 300 webhook deliveries are in flight (the endpoint takes $slow ms, so some are mid-delivery on the killed replica)"
  local sink; sink=$(gateway)
  node -e 'let n=0;const slow=Number(process.argv[1]);require("http").createServer((q,r)=>{n++;setTimeout(()=>{r.writeHead(200);r.end("ok")},slow)}).listen(18997,"0.0.0.0");process.on("SIGTERM",()=>{console.log(n);process.exit(0)})' "$slow" > "$OUT/replicas.kill.sink" &
  local spid=$!; sleep 1
  curl -s -X PUT "$BASE/tenant/webhook" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d "{\"url\":\"http://$sink:18997/hook\",\"secret\":\"replicas-secret-1234\"}" > /dev/null
  local before; before=$("${PG[@]}" "select count(*) from webhook_deliveries where status = 200")
  local out="$OUT/replicas.kill.jsonl"; rm -f "$out"
  load 60 "$out"; local lp=$LOAD_PID
  node -e '
    const [base, token, n] = process.argv.slice(1); const headers={authorization:`Bearer ${token}`,"content-type":"application/json"};
    let next=0, failed=0;
    async function worker(){ while(next<Number(n)){ next++; try {
      const r=await fetch(`${base}/invoices`,{method:"POST",headers,body:JSON.stringify({customer:`k-${next}`,currency:"USD",dueDate:"2030-01-01",items:[{description:"x",quantity:1,unitCents:100}]})});
      const {id}=await r.json(); const i=await fetch(`${base}/invoices/${id}/issue`,{method:"POST",headers}); if(i.status>=300) failed++; } catch { failed++; } } }
    Promise.all(Array.from({length:8},worker)).then(()=>console.log(JSON.stringify({issued:Number(n)-failed,failed})));
  ' "$BASE" "$TOKEN" 300 > "$OUT/replicas.kill.issue.json" & local ip=$!
  sleep 10
  local processing_at_kill; processing_at_kill=$("${PG[@]}" "select count(*) from usai_queue where topic = 'webhook.deliver' and state = 'processing'")
  # Kill the process, not the container: `docker kill` counts as a manual
  # stop and the restart policy would not bring it back (the way a real
  # crash or OOM kill does).
  local victim; victim=$(docker inspect -f '{{.State.Pid}}' usai-p6r-app2-1)
  kill -9 "$victim" 2>/dev/null || sudo -n kill -9 "$victim"
  local t=$SECONDS; wait_ready "$APP2" 90 || log "app2 did NOT come back within 90 s"; local back=$((SECONDS - t))
  log "app2 killed with ~$processing_at_kill messages processing; back after $back s (restart: unless-stopped)"
  wait $ip 2>/dev/null; cat "$OUT/replicas.kill.issue.json"
  wait $lp 2>/dev/null || true
  local s; s=$(summarize "$out"); echo "load: $s"
  log "waiting up to 150 s for the topic to drain"
  for _ in $(seq 1 150); do
    local pend; pend=$("${PG[@]}" "select count(*) from usai_queue where topic = 'webhook.deliver' and state in ('ready','processing')")
    [ "${pend:-1}" = 0 ] && break; sleep 1
  done
  local stuck; stuck=$("${PG[@]}" "select state, count(*) from usai_queue where topic = 'webhook.deliver' and state <> 'done' group by state" | tr '\n' ' ')
  local delivered=$(( $("${PG[@]}" "select count(*) from webhook_deliveries where status = 200") - before ))
  kill -TERM $spid; wait $spid 2>/dev/null; local hits; hits=$(cat "$OUT/replicas.kill.sink")
  local bad; bad=$(echo "$s" | grep -o '"s5xx":[0-9]*' | cut -d: -f2)
  local ok=1; [ "${bad:-1}" -le 16 ] || ok=0; [ -z "$stuck" ] || ok=0
  verdict kill "$ok" "5xx during the kill: $bad (in-flight requests of the killed replica; bound 16 = 2 × clients); processing on the killed replica at the kill: $processing_at_kill; deliveries $delivered of 300 (sink $hits); not done afterwards: ${stuck:-none}"
  curl -s -X PUT "$BASE/tenant/webhook" -H "authorization: Bearer $TOKEN" -H 'content-type: application/json' -d '{"url":"http://127.0.0.1:9/","secret":"replicas-secret-1234"}' > /dev/null
}

all() {
  http; queue 1000; migrate; cron; rolling; kill_one
  echo; echo "== $FAILED scenario(s) failed"
  for a in "$APP1" "$APP2"; do status_of "$a" > "$OUT/replicas.status.$(basename "$a" | tr -d :).json"; done
  docker compose logs --no-color app app2 2>/dev/null | grep -E "WARN|ERROR" | sed 's/^[^|]*| //' | sort | uniq -c | sort -rn | head -20 > "$OUT/replicas.warnings.txt"
  echo "warnings/errors in the two replicas' logs: $(wc -l < "$OUT/replicas.warnings.txt") distinct lines (out/replicas.warnings.txt)"
}

case "${1:-}" in
  up) up ;;
  down) down ;;
  all) all ;;
  http) http ;;
  queue) queue "${2:-1000}" ;;
  migrate) migrate ;;
  cron) cron ;;
  rolling) rolling ;;
  kill) kill_one ;;
  *) sed -n 2,12p "$0"; exit 2 ;;
esac
