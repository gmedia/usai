#!/usr/bin/env bash
# Container smoke test: the images used the way the scaffold tells a user to.
#
#   scripts/container-smoke.sh <runtime-image> <dev-image>
#
# scaffold (create-usai from this checkout) → app image from the template
# Dockerfile (dev stage builds, runtime stage serves) → run read-only as
# non-root → GET /hello/world is 200 → no toolchain in the runtime image →
# `usai probe` and `usai top` work from inside against the status listener →
# SIGTERM drains and exits 0 → `docker compose up` path serves too.
set -euo pipefail
RUNTIME_IMAGE="${1:?runtime image}"
DEV_IMAGE="${2:?dev image}"
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'docker rm -f usai-smoke usai-smoke-top >/dev/null 2>&1 || true; (cd "$work/my-app" 2>/dev/null && docker compose down -v >/dev/null 2>&1) || true; rm -rf "$work"' EXIT

echo "== scaffold"
(cd packages/create-usai && pnpm run build >/dev/null)
node packages/create-usai/dist/cli.js "$work/my-app" >/dev/null
# The SDK under test is this checkout, not whatever npm has (the tag that
# publishes it may not exist yet): pack it and depend on the tarball.
(cd packages/usai && pnpm run build >/dev/null && pnpm pack --out "$work/my-app/sakaladev-usai.tgz" >/dev/null)
cd "$work/my-app"
node -e '
const fs = require("fs"); const p = JSON.parse(fs.readFileSync("package.json", "utf8"));
p.dependencies["@sakaladev/usai"] = "file:./sakaladev-usai.tgz";
fs.writeFileSync("package.json", JSON.stringify(p, null, 2) + "\n");'
sed -i 's#^COPY --chown=node:node package.json #COPY --chown=node:node sakaladev-usai.tgz package.json #' Dockerfile
# Point the template at the images under test.
sed -i "s#^FROM sakaladev/usai:[^ ]*-dev AS build#FROM ${DEV_IMAGE} AS build#; s#^FROM sakaladev/usai:[^ ]*\$#FROM ${RUNTIME_IMAGE}#" Dockerfile
sed -i "s#image: sakaladev/usai:.*-dev#image: ${DEV_IMAGE}#" compose.yaml
grep -n "^FROM\|image:" Dockerfile compose.yaml

echo "== application image (two stages)"
docker build -t usai-smoke-app . >/dev/null

echo "== runtime image: non-root, no toolchain"
uid=$(docker run --rm --entrypoint id "$RUNTIME_IMAGE" -u)
[ "$uid" != "0" ] || { echo "runtime image runs as root"; exit 1; }
if docker run --rm --entrypoint sh "$RUNTIME_IMAGE" -c 'command -v node || command -v pnpm || command -v cc' >/dev/null 2>&1; then
  echo "runtime image carries a toolchain"; exit 1
fi

echo "== serve read-only"
docker run -d --name usai-smoke --read-only --tmpfs /tmp -p 3300:3000 usai-smoke-app >/dev/null
for i in $(seq 1 30); do
  code=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3300/hello/world || true)
  [ "$code" = "200" ] && break
  sleep 1
done
[ "$code" = "200" ] || { echo "expected 200, got $code"; docker logs usai-smoke; exit 1; }
body=$(curl -s http://127.0.0.1:3300/hello/world)
[ "$body" = '{"hello":"world"}' ] || { echo "unexpected body: $body"; exit 1; }

echo "== usai top inside the image (what the k8s runbook tells people to run)"
# The operator surfaces are on a second listener, the way production runs
# them, and the verb has to reach them from inside the container — a
# `kubectl exec … usai top` that cannot is a runbook that does not work.
# -m is deliberate: a pod always has a limit, and without one the runtime
# reports no cgroup fields at all (the charge on an unlimited slice is the
# slice's, which is not a number about this process).
docker run -d --name usai-smoke-top --read-only --tmpfs /tmp -m 512m usai-smoke-app \
  run --artifact /app/.usai/build --host 0.0.0.0 --port 3000 --status-addr 127.0.0.1:9090 >/dev/null
for i in $(seq 1 30); do
  docker exec usai-smoke-top usai probe ready --addr 127.0.0.1:9090 >/dev/null 2>&1 && break
  sleep 1
done
top=$(docker exec usai-smoke-top usai top --addr 127.0.0.1:9090 -n 1 -c 1 2>&1) || {
  echo "usai top failed inside the runtime image:"; echo "$top"; docker logs usai-smoke-top; exit 1; }
echo "$top" | grep -q "revision 1 active" || { echo "usai top printed no active revision:"; echo "$top"; exit 1; }
echo "$top" | grep -q "^  mem " || { echo "usai top printed no memory line:"; echo "$top"; exit 1; }
# In a container there is always a cgroup, so the charged/limit form is the
# one that must appear — that is the number an operator sizes against.
echo "$top" | grep -q "MiB charged" || { echo "usai top did not read the cgroup:"; echo "$top"; exit 1; }
docker rm -f usai-smoke-top >/dev/null

echo "== SIGTERM drains"
docker kill -s TERM usai-smoke >/dev/null
exit_code=$(docker wait usai-smoke)
[ "$exit_code" = "0" ] || { echo "exit code $exit_code after SIGTERM"; docker logs usai-smoke; exit 1; }
docker logs usai-smoke 2>&1 | grep -q "drained; ownership returned to baseline" || { echo "no drain line"; docker logs usai-smoke; exit 1; }
docker rm usai-smoke >/dev/null

echo "== docker compose up (dev path)"
sed -i 's/"3000:3000"/"3301:3000"/' compose.yaml
docker compose up -d >/dev/null
for i in $(seq 1 90); do
  code=$(curl -s -o /dev/null -w "%{http_code}" http://127.0.0.1:3301/hello/world || true)
  [ "$code" = "200" ] && break
  sleep 2
done
[ "$code" = "200" ] || { echo "compose path: expected 200, got $code"; docker compose logs; exit 1; }
docker compose down -v >/dev/null

echo "container smoke: ok"
