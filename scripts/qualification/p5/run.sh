#!/usr/bin/env bash
# P5 operational qualification on one host with Docker.
#
#   scripts/qualification/p5/run.sh build      # app image from the checkout (dev image builds the artifact)
#   scripts/qualification/p5/run.sh up         # compose up, migrate, seed a tenant, print a token
#   scripts/qualification/p5/run.sh scenario <name> [seconds]   # one deliberate failure under load
#   scripts/qualification/p5/run.sh all        # every scenario, evidence under out/
#   scripts/qualification/p5/run.sh down
#
# Evidence per scenario: loadgen JSON lines (per second: ok/4xx/5xx/503/errors,
# p50/p99), /_usai/status before and after, the app's log lines during the
# window, and the metrics that moved. The runbooks in docs/runbooks/ are
# written from these.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
cd "$here"
export COMPOSE_PROJECT_NAME=usai-p5
RUNTIME_IMAGE="${USAI_IMAGE:-sakaladev/usai:p5}"
DEV_IMAGE="${USAI_DEV_IMAGE:-sakaladev/usai:p5-dev}"
BASE="http://127.0.0.1:8080"
CONTROL="http://127.0.0.1:3900"
TOKEN_FILE="$here/out/token"
mkdir -p out

status() { curl -s -m 3 "$BASE/_usai/status" || echo '{"unreachable":true}'; }
metrics() { curl -s -m 3 "$BASE/_usai/metrics" || true; }
health() { curl -s -m 2 -o /dev/null -w "%{http_code}" "$BASE/invoices" || echo 000; }   # 401 = up
app_logs_since() { docker compose logs --no-color --since "$1" app 2>/dev/null | sed 's/^[^|]*| //'; }

build() {
  echo "== artifact via $DEV_IMAGE"
  docker run --rm --entrypoint sh -v "$repo:/src" -w /src -e HOME=/tmp -e USAI_CACHE_DIR=/tmp/usai-cache "$DEV_IMAGE" -c '
    set -e
    pnpm install --frozen-lockfile >/dev/null
    pnpm --filter @sakaladev/create-usai run build >/dev/null
    usai --root examples/invoicing build --no-typecheck'
  rm -rf build && cp -r "$repo/examples/invoicing/.usai/build" build
  echo "== app image"
  docker build -q --build-arg "USAI_IMAGE=$RUNTIME_IMAGE" -f app.Dockerfile -t usai-p5-app:latest .
}

up() {
  docker compose up -d --wait postgres
  docker compose run --rm --no-deps -T app db migrate --artifact /app/.usai/build
  docker compose up -d
  for i in $(seq 1 60); do [ "$(health)" = "401" ] && break; sleep 1; done
  [ "$(health)" = "401" ] || { echo "app did not come up"; docker compose logs app | tail -20; exit 1; }
  local body
  body=$(curl -s -X POST "$BASE/signup" -H 'content-type: application/json' -d '{"tenant":"load","email":"load@example.test","password":"load-test-password-1"}')
  if ! echo "$body" | grep -q token; then
    body=$(curl -s -X POST "$BASE/login" -H 'content-type: application/json' -d '{"tenant":"load","email":"load@example.test","password":"load-test-password-1"}')
  fi
  echo "$body" | sed 's/.*"token":"\([^"]*\)".*/\1/' > "$TOKEN_FILE"
  echo "up: $BASE (token in $TOKEN_FILE)"
}

down() { docker compose down -v --remove-orphans; }


wait_healthy() {
  local deadline=$((SECONDS + ${1:-120}))
  while [ $SECONDS -lt $deadline ]; do [ "$(health)" = "401" ] && return 0; sleep 0.5; done
  return 1
}

scenario() {
  local name="$1" dur="${2:-40}"
  rm -f "out/$name".*
  echo "== scenario $name ($dur s)"
  status > "out/$name.status.before.json"; metrics > "out/$name.metrics.before.txt"
  local start; start=$(date -u +%Y-%m-%dT%H:%M:%SZ)
  node "$here/loadgen.mjs" "$BASE" "$(cat "$TOKEN_FILE")" "${CLIENTS:-8}" "$dur" "out/$name.load.jsonl" > /dev/null &
  local pid=$!
  sleep 8
  local t0=$SECONDS
  case "$name" in
    baseline)            : ;;
    pg-kill)             docker compose kill -s SIGKILL postgres; sleep 10; docker compose start postgres ;;
    pg-restart)          docker compose restart postgres ;;
    network-partition)   docker network disconnect usai-p5_default usai-p5-postgres-1; sleep 10; docker network connect usai-p5_default usai-p5-postgres-1 ;;
    app-sigterm)         docker compose kill -s SIGTERM app; sleep 3; docker compose start app ;;
    app-sigkill)         docker compose kill -s SIGKILL app; sleep 1; docker compose start app ;;
    app-restart)         docker compose restart app ;;
    bad-deploy)          # install an artifact whose manifest is broken through the control surface
                         docker compose exec -T app sh -c 'mkdir -p /tmp/bad && cp -r /app/.usai/build/. /tmp/bad/ && sed -i "s/\"manifestVersion\": 1/\"manifestVersion\": 99/" /tmp/bad/manifest.json'
                         curl -s -X POST "$CONTROL/revisions" -H 'authorization: Bearer p5-control-token' -H 'content-type: application/json' -d '{"artifact":"/tmp/bad"}' > "out/$name.control.json"; echo >> "out/$name.control.json" ;;
    rollback)            # install the same artifact as a new revision, activate, then re-activate the previous one
                         curl -s -X POST "$CONTROL/revisions" -H 'authorization: Bearer p5-control-token' -H 'content-type: application/json' -d '{"artifact":"/app/.usai/build"}' > "out/$name.control.json"
                         local new; new=$(sed 's/.*"id":"\{0,1\}\([a-z0-9]*\)"\{0,1\}.*/\1/' "out/$name.control.json")
                         curl -s -X POST "$CONTROL/revisions/$new/activate" -H 'authorization: Bearer p5-control-token' >> "out/$name.control.json"; echo >> "out/$name.control.json"
                         sleep 5
                         curl -s -X POST "$CONTROL/revisions/rev1/activate" -H 'authorization: Bearer p5-control-token' >> "out/$name.control.json"; echo >> "out/$name.control.json" ;;
    revision-churn)      for i in $(seq 1 20); do
                           r=$(curl -s -X POST "$CONTROL/revisions" -H 'authorization: Bearer p5-control-token' -H 'content-type: application/json' -d '{"artifact":"/app/.usai/build"}' | sed 's/.*"id":"\{0,1\}\([a-z0-9]*\)"\{0,1\}.*/\1/')
                           curl -s -o /dev/null -X POST "$CONTROL/revisions/$r/activate" -H 'authorization: Bearer p5-control-token'
                           echo "$r" >> "out/$name.control.json"
                         done ;;
    traffic-spike)       CLIENTS=64 node "$here/loadgen.mjs" "$BASE" "$(cat "$TOKEN_FILE")" 64 15 "out/$name.spike.jsonl" > /dev/null ;;
    memory-pressure)     docker update --memory 96m --memory-swap 96m usai-p5-app-1; sleep 15; docker update --memory 512m --memory-swap 512m usai-p5-app-1 ;;
    disk-full)           docker compose exec -T app sh -c 'dd if=/dev/zero of=/tmp/fill bs=1M count=4096 2>/dev/null || true; df -h /tmp' > "out/$name.disk.txt"; sleep 10; docker compose exec -T app rm -f /tmp/fill ;;
    invalid-config)      docker compose stop app; docker compose run --rm --no-deps -T -e DATABASE_URL= app run --artifact /app/.usai/build --port 3000 > "out/$name.run.txt" 2>&1 || true; docker compose start app ;;
    *) echo "unknown scenario $name"; kill "$pid" 2>/dev/null; exit 2 ;;
  esac
  local action_s=$((SECONDS - t0))
  local recovered="no"; local rec_s="-"
  if wait_healthy 120; then recovered="yes"; rec_s=$((SECONDS - t0)); fi
  wait "$pid" 2>/dev/null || true
  status > "out/$name.status.after.json"; metrics > "out/$name.metrics.after.txt"
  app_logs_since "$start" > "out/$name.app.log"
  # Summary: totals over the window, worst second.
  node -e '
    const fs=require("fs"); const lines=fs.readFileSync(process.argv[1],"utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
    const sum=(k)=>lines.reduce((a,l)=>a+(l[k]||0),0);
    const worst=lines.reduce((w,l)=> (l.s5xx+l.errors+l.s503) > (w.s5xx+w.errors+w.s503) ? l : w, lines[0]);
    const badSeconds=lines.filter(l=>l.s5xx+l.errors+l.s503>0).length;
    console.log(JSON.stringify({seconds:lines.length, ok:sum("ok"), s4xx:sum("s4xx"), s5xx:sum("s5xx"), s503:sum("s503"), errors:sum("errors"), badSeconds, worst, p99max:Math.max(...lines.map(l=>l.p99||0))}));
  ' "out/$name.load.jsonl" > "out/$name.summary.json"
  echo "action ${action_s}s, healthy again: $recovered (${rec_s}s); $(cat "out/$name.summary.json")"
  grep -E "warn|error|WARN|ERROR|quarantin|drain|refus|shutting|activ" "out/$name.app.log" | head -15 || true
}

all() {
  for s in baseline pg-kill pg-restart network-partition app-sigterm app-sigkill bad-deploy rollback revision-churn traffic-spike memory-pressure disk-full invalid-config; do
    scenario "$s" 40
    sleep 5
  done
}

case "${1:-}" in
  build) build ;;
  up) up ;;
  down) down ;;
  scenario) shift; scenario "$@" ;;
  all) all ;;
  *) echo "usage: $0 build|up|down|scenario <name> [seconds]|all"; exit 2 ;;
esac
