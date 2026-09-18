#!/usr/bin/env bash
# Container smoke test: the images used the way the scaffold tells a user to.
#
#   scripts/container-smoke.sh <runtime-image> <dev-image>
#
# scaffold (create-usai from this checkout) → app image from the template
# Dockerfile (dev stage builds, runtime stage serves) → run read-only as
# non-root → GET /hello/world is 200 → no toolchain in the runtime image →
# SIGTERM drains and exits 0 → `docker compose up` path serves too.
set -euo pipefail
RUNTIME_IMAGE="${1:?runtime image}"
DEV_IMAGE="${2:?dev image}"
cd "$(dirname "$0")/.."

work="$(mktemp -d)"
trap 'docker rm -f usai-smoke >/dev/null 2>&1 || true; (cd "$work/my-app" 2>/dev/null && docker compose down -v >/dev/null 2>&1) || true; rm -rf "$work"' EXIT

echo "== scaffold"
(cd packages/create-usai && pnpm run build >/dev/null)
node packages/create-usai/dist/cli.js "$work/my-app" >/dev/null
cd "$work/my-app"
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
