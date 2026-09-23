#!/usr/bin/env bash
# Threat-model verification: every claim in `docs/THREAT-MODEL.md` that can be
# checked from outside the process, checked.
#
#   run.sh                 # build the fixture, start it, run every probe
#
# This is not an attack suite. Each probe asserts the outcome the document
# already promises; a deviation is a finding against the runtime or against the
# document, and either way it is a bug. Run it before a release.
#
# Environment: USAI (binary, default target/debug/usai), OUT (report dir),
# PORT (base port, default 3600).
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
USAI="${USAI:-$repo/target/debug/usai}"
APP="$here/app"
PORT="${PORT:-3600}"
STATUS_PORT=$((PORT + 1))
OUT="${OUT:-$here/out/$(date -u +%Y%m%dT%H%M%SZ)}"
mkdir -p "$OUT"
LOG="$OUT/server.log"
CANARY="canary-8f3a1d-do-not-log"
TOKEN="status-token-$RANDOM$RANDOM"

pass=0
fail=0
skip=0
say() { echo "$*" | tee -a "$OUT/report.txt"; }
ok() {
  pass=$((pass + 1))
  say "  PASS  $1"
}
bad() {
  fail=$((fail + 1))
  say "  FAIL  $1"
  [ -n "${2:-}" ] && say "        evidence: $2"
}
skipped() {
  skip=$((skip + 1))
  say "  SKIP  $1${2:+ ($2)}"
}
claim() { say ""; say "$1"; }

code() { curl -s -o /dev/null -w '%{http_code}' -m 20 "$@"; }
body() { curl -s -m 20 "$@"; }
heads() { curl -s -o /dev/null -D- -m 20 "$@"; }

cleanup() {
  [ -n "${SERVER_PID:-}" ] && kill "$SERVER_PID" 2>/dev/null
  wait "${SERVER_PID:-}" 2>/dev/null
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
say "Threat-model verification — $(date -u +%FT%TZ)"
say "binary: $USAI"
"$USAI" --root "$APP" build --no-typecheck >"$OUT/build.log" 2>&1 || {
  say "build failed; see $OUT/build.log"
  exit 1
}

THREAT_SECRET="$CANARY" "$USAI" --root "$APP" run \
  --artifact "$APP/.usai/build" --port "$PORT" \
  --status --status-addr "127.0.0.1:$STATUS_PORT" --status-token "$TOKEN" \
  >"$LOG" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 60); do
  [ "$(code "http://127.0.0.1:$STATUS_PORT/_usai/ready")" = "200" ] && break
  sleep 0.5
done
[ "$(code "http://127.0.0.1:$STATUS_PORT/_usai/ready")" = "200" ] || {
  say "the fixture did not become ready; see $LOG"
  exit 1
}
base="http://127.0.0.1:$PORT"
status_base="http://127.0.0.1:$STATUS_PORT"
worlds() {
  body -H "Authorization: Bearer $TOKEN" "$status_base/_usai/status" |
    node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);console.log(j.gauges?.worlds_created ?? j.worldsCreated ?? j.gauges?.worldsCreated ?? "?")})'
}

claim "Oversized request bodies → 413 before any world (USAI_MAX_BODY_BYTES)"
# The gauge must be known to move, or "it did not move" proves nothing.
code "$base/alive" >/dev/null
zero=$(worlds)
code "$base/alive" >/dev/null
one=$(worlds)
[ "$one" != "$zero" ] && [ "$one" != "?" ] && ok "the world counter moves on a served request ($zero → $one)" ||
  bad "the world counter moves on a served request" "$zero → $one"
before=$(worlds)
head -c 2000000 /dev/zero | tr '\0' 'x' >"$OUT/big.bin"
got=$(code -X POST --data-binary "@$OUT/big.bin" -H 'content-type: application/octet-stream' "$base/upload")
after=$(worlds)
[ "$got" = "413" ] && ok "a 2 MB body is 413" || bad "a 2 MB body is 413" "got $got"
[ "$before" = "$after" ] && ok "no world was created for it ($before)" || bad "no world was created for it" "$before → $after"

claim "Malformed / invalid input never reaches application code (C6)"
before=$(worlds)
got=$(code -X POST -H 'content-type: application/json' -d '{"name":"","url":"nope"}' "$base/contract")
after=$(worlds)
[ "$got" = "400" ] && ok "a contract violation is 400" || bad "a contract violation is 400" "got $got"
[ "$before" = "$after" ] && ok "no world was created for it ($before)" || bad "no world was created for it" "$before → $after"
b=$(body -X POST -H 'content-type: application/json' -d '{"name":"","url":"nope"}' "$base/contract")
echo "$b" | grep -q '"slot":"body"' && ok "the envelope names the slot" || bad "the envelope names the slot" "$b"

claim "Slow / hung handlers are bounded (deadline, CPU slice)"
t0=$(date +%s)
got=$(code "$base/slow")
t1=$(date +%s)
[ "$got" = "504" ] && ok "an awaiting handler past its deadline is 504" || bad "an awaiting handler past its deadline is 504" "got $got"
[ $((t1 - t0)) -lt 5 ] && ok "it answered in $((t1 - t0))s, not at the handler's own 30s" || bad "it answered promptly" "$((t1 - t0))s"
t0=$(date +%s)
got=$(code "$base/busy")
t1=$(date +%s)
[ "$got" = "504" ] && ok "synchronous work past its deadline is 504" || bad "synchronous work past its deadline is 504" "got $got"
[ $((t1 - t0)) -lt 8 ] && ok "the watchdog interrupted it in $((t1 - t0))s" || bad "the watchdog interrupted it" "$((t1 - t0))s"

claim "One world cannot exhaust the process's memory"
got=$(code "$base/hog")
[ "$got" = "500" ] && ok "a world past its memory bound is 500" || bad "a world past its memory bound is 500" "got $got"
b=$(body "$base/hog")
echo "$b" | grep -q "runtime_fault\|internal" && ok "the client is told nothing more than a fault" || bad "the fault is sanitized" "$b"
[ "$(code "$base/alive")" = "200" ] && ok "the runtime keeps serving afterwards" || bad "the runtime keeps serving afterwards"

claim "A response tells the client nothing about the server unless asked"
h=$(heads "$base/alive")
echo "$h" | grep -qi "^server-timing:" && bad "Server-Timing is on without --server-timing" "$h" ||
  ok "no Server-Timing (it is opt-in: --server-timing / USAI_SERVER_TIMING)"
echo "$h" | grep -qi "^x-usai-" && bad "an x-usai-* header is exposed by default" "$h" ||
  ok "no x-usai-* header (those are --diagnostics and USAI_PROFILE)"
echo "$h" | grep -qi "^x-request-id:" && ok "x-request-id comes back, which is the one thing it should say" ||
  bad "x-request-id comes back" "$h"

claim "Internal detail does not leak in error responses"
b=$(body "$base/boom")
echo "$b" | grep -q '"code":"internal"' && ok "an unexpected error is code internal" || bad "an unexpected error is code internal" "$b"
echo "$b" | grep -q "$CANARY" && bad "the response leaked the secret" "$b" || ok "the response carries no secret"
echo "$b" | grep -qi "stack\|at .*\.ts:" && bad "the response carries a stack" "$b" || ok "the response carries no stack"

claim "Logs carry what the application says, not what the client sent"
body -X POST -H 'content-type: application/json' -H "x-probe: $CANARY" \
  -H "cookie: probe=$CANARY" -d "{\"note\":\"$CANARY\"}" "$base/echo?q=$CANARY" >/dev/null
sleep 1
n=$(grep -c "$CANARY" "$LOG")
# The application's own env value is allowed to exist in the process, not in a line.
[ "$n" = "0" ] && ok "the canary appears in no log line" || bad "the canary appears in $n log line(s)" "$(grep -m1 "$CANARY" "$LOG" | cut -c1-200)"
grep -q "$TOKEN" "$LOG" && bad "the status token appears in the log" || ok "the status token appears in no log line"

claim "Secrets are not in the artifact: resources and env reference names"
grep -q "$CANARY" "$APP/.usai/build/manifest.json" && bad "the manifest carries the secret's value" || ok "the manifest carries no secret value"
grep -q "THREAT_SECRET" "$APP/.usai/build/manifest.json" && ok "the manifest names the variable" || bad "the manifest names the variable"

claim "Operator surfaces are protected and switchable"
[ "$(code "$status_base/_usai/status")" = "401" ] && ok "status without a token is 401" || bad "status without a token is 401"
[ "$(code -H "Authorization: Bearer $TOKEN" "$status_base/_usai/status")" = "200" ] && ok "status with the token is 200" || bad "status with the token is 200"
[ "$(code -H "Authorization: Bearer wrong-$TOKEN" "$status_base/_usai/status")" = "401" ] && ok "a wrong token is 401" || bad "a wrong token is 401"
[ "$(code "$status_base/_usai/live")" = "200" ] && ok "the liveness probe stays open" || bad "the liveness probe stays open"
[ "$(code "$status_base/_usai/ready")" = "200" ] && ok "the readiness probe stays open" || bad "the readiness probe stays open"
[ "$(code -H "Authorization: Bearer $TOKEN" "$status_base/_usai/metrics")" = "200" ] && ok "metrics answer with the token" || bad "metrics answer with the token"

claim "Leaked async work does not extend a request"
t0=$(date +%s)
got=$(code "$base/detach")
t1=$(date +%s)
[ "$got" = "200" ] && [ $((t1 - t0)) -lt 3 ] && ok "a live timer is cancelled and the response commits ($((t1 - t0))s)" ||
  bad "a live timer is cancelled and the response commits" "status $got in $((t1 - t0))s"

claim "A hand-off nobody declared is reported"
body "$base/hand-off" >/dev/null
sleep 1
grep -q "does not declare" "$LOG" && ok "the undeclared dispatch is logged" || bad "the undeclared dispatch is logged"

cleanup
unset SERVER_PID

claim "A silent WebSocket client does not hold a world forever"
# The documented control is `socket_idle_timeout` (300 s by default, far too
# long for a probe): the campaign runs a second instance with it set to 3 s.
THREAT_SECRET="$CANARY" USAI_SOCKET_IDLE_TIMEOUT=3 "$USAI" --root "$APP" run \
  --artifact "$APP/.usai/build" --port $((PORT + 10)) --status --status-addr "127.0.0.1:$((PORT + 11))" \
  >"$OUT/socket-idle.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 60); do
  [ "$(code "http://127.0.0.1:$((PORT + 11))/_usai/ready")" = "200" ] && break
  sleep 0.5
done
idle=$(node "$here/socket-idle.mjs" "ws://127.0.0.1:$((PORT + 10))/socket" 30 2>&1 | tail -1)
say "        $idle"
echo "$idle" | grep -q '"opened":true' && ok "the connection was accepted" || bad "the connection was accepted" "$idle"
echo "$idle" | grep -q '"closeCode":1008' && ok "an idle connection is closed with 1008" || bad "an idle connection is closed with 1008" "$idle"
cleanup
unset SERVER_PID

claim "A surface that is switched off is not served"
THREAT_SECRET="$CANARY" USAI_SURFACES_OFF=metrics,docs "$USAI" --root "$APP" run \
  --artifact "$APP/.usai/build" --port $((PORT + 2)) --status --status-addr "127.0.0.1:$((PORT + 3))" \
  >"$OUT/surfaces-off.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 60); do
  [ "$(code "http://127.0.0.1:$((PORT + 3))/_usai/ready")" = "200" ] && break
  sleep 0.5
done
[ "$(code "http://127.0.0.1:$((PORT + 3))/_usai/metrics")" = "404" ] && ok "a switched-off surface is 404" || bad "a switched-off surface is 404"
[ "$(code "http://127.0.0.1:$((PORT + 3))/_usai/status")" = "200" ] && ok "the surfaces left on still answer" || bad "the surfaces left on still answer"
cleanup
unset SERVER_PID

claim "The public listener does not admit that an operator surface exists"
# Production shape: surfaces on their own listener, a token set, nothing under
# /_usai/ on the application port. Every one of the six paths must answer the
# same way there — a 401 for two of them (which is what 0.0.8 did) tells an
# unauthenticated caller that this is a Usai runtime with a protected surface
# to come back for, and names the variable that guards it.
THREAT_SECRET="$CANARY" USAI_STATUS_TOKEN="$TOKEN" "$USAI" --root "$APP" run \
  --artifact "$APP/.usai/build" --port $((PORT + 12)) --status-addr "127.0.0.1:$((PORT + 13))" \
  >"$OUT/private-surfaces.log" 2>&1 &
SERVER_PID=$!
for _ in $(seq 1 60); do
  [ "$(code "http://127.0.0.1:$((PORT + 13))/_usai/live")" = "200" ] && break
  sleep 0.5
done
public="http://127.0.0.1:$((PORT + 12))"
all404=1
for path in /_usai/status /_usai/metrics /_usai/live /_usai/ready /_usai/docs /_usai/openapi.json; do
  got=$(code "$public$path")
  [ "$got" = "404" ] || { all404=0; say "    $path answered $got on the application listener"; }
done
[ "$all404" = 1 ] && ok "every /_usai/ path on the application listener is 404" ||
  bad "every /_usai/ path on the application listener is 404"
body "$public/_usai/status" | grep -qi "USAI_STATUS_TOKEN\|unauthorized" &&
  bad "the 404 body names no operator surface" ||
  ok "the 404 body names no operator surface"
cleanup
unset SERVER_PID

claim "The control surface refuses a public address without a token"
out=$(THREAT_SECRET="$CANARY" "$USAI" --root "$APP" run --artifact "$APP/.usai/build" \
  --port $((PORT + 4)) --control "0.0.0.0:$((PORT + 5))" 2>&1 | head -5)
echo "$out" | grep -qi "refus\|USAI_CONTROL_TOKEN" && ok "binding the control surface publicly without a token is refused" ||
  bad "binding the control surface publicly without a token is refused" "$(echo "$out" | tr '\n' ' ' | cut -c1-200)"

claim "A stale or tampered artifact is not served silently"
cp -r "$APP/.usai/build" "$OUT/tampered"
printf '\n// tampered\n' >>"$OUT/tampered/app.js"
out=$(THREAT_SECRET="$CANARY" "$USAI" --root "$APP" run --artifact "$OUT/tampered" --port $((PORT + 6)) 2>&1 | head -5)
echo "$out" | grep -qi "hash\|mismatch\|refus\|invalid" && ok "a modified app.js is refused" ||
  bad "a modified app.js is refused" "$(echo "$out" | tr '\n' ' ' | cut -c1-200)"

claim "A signature can be required, and a tampered signed artifact is refused"
if "$USAI" --root "$APP" keygen --out "$OUT/key" >"$OUT/keygen.log" 2>&1; then
  # keygen prints the public key; it writes no .pub file.
  pub=$(grep -oE '\b[0-9a-f]{64}\b' "$OUT/keygen.log" | head -1)
  if [ -z "$pub" ]; then
    bad "the public key can be read from keygen's output" "$(head -3 "$OUT/keygen.log" | tr '\n' ' ')"
  else
    "$USAI" --root "$APP" build --no-typecheck --sign "$OUT/key" >"$OUT/sign.log" 2>&1
    # a) the untouched signed artifact must run
    THREAT_SECRET="$CANARY" "$USAI" --root "$APP" run --artifact "$APP/.usai/build" \
      --port $((PORT + 7)) --require-signature "$pub" >"$OUT/signed-ok.log" 2>&1 &
    SERVER_PID=$!
    up=""
    for _ in $(seq 1 60); do
      [ "$(code "http://127.0.0.1:$((PORT + 7))/alive")" = "200" ] && up=yes && break
      sleep 0.5
    done
    [ -n "$up" ] && ok "a correctly signed artifact runs under --require-signature" ||
      bad "a correctly signed artifact runs under --require-signature" "$(tail -2 "$OUT/signed-ok.log" | tr '\n' ' ' | cut -c1-200)"
    cleanup
    unset SERVER_PID
    # b) one byte changed must be refused
    cp -r "$APP/.usai/build" "$OUT/signed-tampered"
    printf '\n// tampered\n' >>"$OUT/signed-tampered/app.js"
    out=$(THREAT_SECRET="$CANARY" "$USAI" --root "$APP" run --artifact "$OUT/signed-tampered" \
      --port $((PORT + 8)) --require-signature "$pub" 2>&1 | head -5)
    echo "$out" | grep -qi "refus\|signature" && ok "a tampered signed artifact is refused" ||
      bad "a tampered signed artifact is refused" "$(echo "$out" | tr '\n' ' ' | cut -c1-200)"
    # c) a different key must be refused
    "$USAI" --root "$APP" keygen --out "$OUT/other-key" >"$OUT/other-keygen.log" 2>&1
    other=$(grep -oE '\b[0-9a-f]{64}\b' "$OUT/other-keygen.log" | head -1)
    out=$(THREAT_SECRET="$CANARY" "$USAI" --root "$APP" run --artifact "$APP/.usai/build" \
      --port $((PORT + 9)) --require-signature "$other" 2>&1 | head -5)
    echo "$out" | grep -qi "refus\|signature\|signer" && ok "an artifact signed by another key is refused" ||
      bad "an artifact signed by another key is refused" "$(echo "$out" | tr '\n' ' ' | cut -c1-200)"
  fi
else
  skipped "signature probes" "no keygen command in this build"
fi

say ""
say "— $pass passed, $fail failed, $skip skipped. Report: $OUT/report.txt"
[ "$fail" = "0" ]
