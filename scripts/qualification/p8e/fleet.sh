#!/usr/bin/env bash
# P8E — the efficiency envelope (docs/measurements/BENCHMARKS.md, scoreboard 2).
#
#   fleet.sh prepare                                  # artifacts, database, base image, node deps
#   fleet.sh floor <hello|template|node|php> <mem_mib> <cpus> <max_worlds>   # one instance in a cgroup box
#                                                     (node: Node + Fastify, max_worlds ignored; php: the tuned
#                                                     PHP-FPM + nginx compose with the cell's limits, max_worlds = children)
#   fleet.sh density <usai|node> <n> [minutes]        # n bare processes, mostly idle, one bursting
#   fleet.sh report <run_dir>                         # tables from the samples
#
# Environment: DATABASE_URL (a throwaway database), USAI (binary), OUT (run
# directory), PIN (cpu list the fleet is confined to, e.g. 12-15), BASE_IMAGE
# (a distro image whose glibc matches the host's binary; default ubuntu:26.04),
# DROP_CACHES=1 (drop the kernel's whole page cache before every cell, which
# needs passwordless sudo and a host with nothing else being measured on it),
# NODE (node binary for the comparator), USAI_WASM_KEEP_RESIDENT (forwarded to
# the boxed instance when set, the cell name gets a "-kr<bytes>" suffix: the C2
# residency lever). Ports 3800–3999.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
: "${DATABASE_URL:?DATABASE_URL is required (a throwaway database)}"
USAI="${USAI:-$repo/target/release/usai}"
PHASES=""
OUT="${OUT:-$here/out/$(date -u +%Y%m%dT%H%M%SZ)}"
PIN="${PIN:-}"
BASE_IMAGE="${BASE_IMAGE:-ubuntu:26.04}"
NODE="${NODE:-node}"
mkdir -p "$OUT"
log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$OUT/log.txt"; }
pin() { if [ -n "$PIN" ]; then taskset -c "$PIN" "$@"; else "$@"; fi; }
# For backgrounded servers: exec, so $! is the server itself, not a subshell.
pin_bg() { if [ -n "$PIN" ]; then exec taskset -c "$PIN" "$@"; else exec "$@"; fi; }

TEMPLATE="$here/template"
HELLO="$repo/examples/hello"
NODE_APP="$repo/scripts/qualification/bench/baselines/node-fastify/server.mjs"

prepare() {
  log "artifacts"
  "$USAI" --root "$TEMPLATE" build --no-typecheck >/dev/null
  "$USAI" --root "$HELLO" build --no-typecheck >/dev/null
  log "node deps"
  if [ ! -d "$repo/scripts/qualification/bench/node_modules/fastify" ]; then
    (cd "$repo/scripts/qualification/bench" && npm install --no-audit --no-fund >/dev/null)
  fi
  log "database"
  # The campaign's database is the campaign's to create: assuming it exists
  # made a fresh PostgreSQL container fail every cell with "not ready", and
  # the reason was four directories deep in a server log.
  (cd "$repo/scripts/qualification/bench" && "$NODE" -e '
    const { Client } = require("pg");
    const url = new URL(process.env.DATABASE_URL);
    const name = decodeURIComponent(url.pathname.slice(1));
    const admin = new URL(url); admin.pathname = "/postgres";
    (async () => {
      const a = new Client({ connectionString: admin.href });
      await a.connect();
      const { rows } = await a.query("select 1 from pg_database where datname = $1", [name]);
      if (rows.length === 0) { await a.query(`create database "${name}"`); console.log("created database " + name); }
      await a.end();
      const c = new Client({ connectionString: process.env.DATABASE_URL });
      await c.connect();
      await c.query("drop schema public cascade; create schema public;");
      await c.end();
    })().catch((e) => { console.error(e.message); process.exit(1); });
  ')
  "$USAI" --root "$TEMPLATE" db migrate >/dev/null
  if docker info >/dev/null 2>&1; then log "base image $BASE_IMAGE"; docker pull -q "$BASE_IMAGE" >/dev/null; fi
  log "prepared"
}

# Why every cell starts by dropping the page cache for what it will run:
#
# The kernel charges a file's pages to the cgroup that *first* faults them in.
# The harness runs the same binaries on the host (build, migrate, the load
# driver is node itself), so by the time a cell started its executable was
# already resident and already billed to the host: the box ran on memory
# nobody asked it to pay for. That is how the 2026-09-23 node cells reported
# 88 MiB of resident memory inside a 48 MiB box and were never OOM-killed.
#
# Copying the files into an image does not fix it — `docker build` warms the
# cache just as thoroughly, and the layer file cannot be evicted without root
# (measured: bind mount 41.9 MB resident / 8.7 MB charged, image layer 42.0 /
# 8.9). What fixes it is `posix_fadvise(POSIX_FADV_DONTNEED)` on the files the
# box is about to map, which needs no privileges: the container then faults
# them itself and is charged for them, which is what a machine that has never
# run this application looks like. `pagecache-check.sh` is the experiment.
# Sets COLD=true when the cache was actually dropped. A cell that could not
# drop it is still run and still reported, but it says so: a floor measured on
# a warm cache is the optimistic one this whole mechanism exists to avoid.
COLD=false
evict_for_cell() {
  COLD=false
  # `$dir` is the cell's directory: bash locals are visible to what they call.
  if python3 "$here/evict.py" "$@" > "$dir/evicted.txt" 2>&1; then
    COLD=true
  else
    log "  could not drop the page cache (python3?): this cell is measured warm"
  fi
  # The box also maps the base image's libc and friends, and those pages live
  # in a layer an unprivileged process cannot evict — and with the containerd
  # image store there is no layer path to aim at anyway (`GraphDriver` is
  # null). Three outcomes, and the cell records which one it got:
  #
  #   full     DROP_CACHES=1 on a host that is ours alone: the kernel's whole
  #            page cache goes, so the box is charged for every page it maps.
  #            Never on a host with a measurement running beside us — it is a
  #            host-wide I/O event, and the 72 h soak's only bad seconds were
  #            caused by exactly that.
  #   true     the bind-mounted files were evicted and the image's layers too.
  #   partial  the application's files were evicted and the shared libraries
  #            were not. The charge is then a lower bound, and a floor from
  #            such a cell is read from the resident set, not from the absence
  #            of an OOM kill.
  if [ "${DROP_CACHES:-0}" = 1 ] && sudo -n true 2>/dev/null; then
    sync
    if echo 3 | sudo -n tee /proc/sys/vm/drop_caches >/dev/null 2>&1; then
      COLD=full
      return
    fi
  fi
  local layers
  layers=$(docker image inspect "$BASE_IMAGE" -f '{{.GraphDriver.Data.LowerDir}}:{{.GraphDriver.Data.UpperDir}}' 2>/dev/null | tr ':' '\n' | grep -v '^$' || true)
  if [ -n "$layers" ] && sudo -n true 2>/dev/null; then
    # The word split is the point (one path per layer), and the redirect is
    # the caller's own file, not root's.
    # shellcheck disable=SC2086,SC2024
    sudo -n python3 "$here/evict.py" $layers >> "$dir/evicted.txt" 2>&1 || true
  elif [ "$COLD" = true ]; then
    COLD=partial
  fi
}

# The cgroup a container's init process lives in, so the run can read what the
# kernel charged it rather than what the sampler could see from outside.
cgroup_dir() { # pid
  local rel; rel=$(awk -F: '$1 == "0" { print $3 }' "/proc/$1/cgroup" 2>/dev/null || true)
  [ -n "$rel" ] && [ -d "/sys/fs/cgroup$rel" ] && echo "/sys/fs/cgroup$rel"
}
cgroup_field() { # file key ; a single-value file is read with key ""
  local f="$1" key="$2"
  [ -r "$f" ] || { echo 0; return; }
  if [ -z "$key" ]; then head -1 "$f"; else awk -v k="$key" '$1 == k { print $2 }' "$f" | head -1; fi
}

# ---- one instance in a cgroup box -------------------------------------------
# The binary and the artifact are bind-mounted into a plain distro container:
# --memory with --memory-swap equal makes a breach a real OOM kill, --cpus a
# real CPU ceiling. The status listener stays reachable on the host network.
# A cell that cannot even start is a verdict, not a crash: `set -e` used to
# abandon the whole `floor` run at the first failed `docker run`, leaving a
# directory with nothing but an empty container.id and no reason anywhere.
cell_failed() { # dir cell reason
  local reason; reason=$(printf '%s' "$3" | tr -d '\\"' | tr '\n' ' ')
  log "  $reason"
  echo "{\"cell\":\"$2\",\"pass\":false,\"reason\":\"$reason\"}" > "$1/result.json"
}

floor() {
  local app="$1" mem="$2" cpus="$3" worlds="$4"
  local root port status
  case "$app" in hello|template|node|php) ;; *) echo "floor: hello|template|node|php"; exit 2;; esac
  port=3800; status=3801
  local name="p8e-floor" cell="$app-${mem}m-${cpus}c-${worlds}w${USAI_WASM_KEEP_RESIDENT:+-kr$USAI_WASM_KEEP_RESIDENT}"
  local dir="$OUT/floor-$cell"; mkdir -p "$dir"
  PHASES="$dir/phases.jsonl"
  log "== floor $cell"
  docker rm -f "$name" >/dev/null 2>&1 || true
  # The readiness URL and the sampler label differ per comparator: Usai has a
  # status listener; Node answers /health on the app port; PHP is a compose
  # project (fpm master + children + nginx) whose limits come from the cell.
  local ready_url="http://127.0.0.1:$status/_usai/ready" label="usai"
  case "$app" in
    hello|template)
      # Runs as the invoking user so the sampler may read /proc/<pid>/smaps_rollup
      # and fd (root-owned processes hide them from it); no compile cache, the
      # artifact is precompiled and the filesystem is read-only.
      root=$([ "$app" = hello ] && echo "$HELLO" || echo "$TEMPLATE")
      evict_for_cell "$USAI" "$root/.usai"
      docker run -d --name "$name" --network host --user "$(id -u):$(id -g)" \
        --cpus "$cpus" --memory "${mem}m" --memory-swap "${mem}m" ${PIN:+--cpuset-cpus "$PIN"} \
        -v "$USAI:/usai:ro" -v "$root:/app:ro" -w /app --read-only --tmpfs /tmp \
        -e HOME=/tmp -e USAI_COMPILE_CACHE=0 -e DATABASE_URL="$DATABASE_URL" -e USAI_MAX_WORLDS="$worlds" \
        ${USAI_WASM_KEEP_RESIDENT:+-e USAI_WASM_KEEP_RESIDENT="$USAI_WASM_KEEP_RESIDENT"} \
        "$BASE_IMAGE" /usai run --artifact /app/.usai/build --port "$port" --status-addr "127.0.0.1:$status" --drain-timeout 5 \
        > "$dir/container.id" 2> "$dir/docker.err" \
        || { cell_failed "$dir" "$cell" "docker run failed: $(head -c 300 "$dir/docker.err")"; return 0; };;
    node)
      # The host's Node (bind-mounted with the bench's node_modules) in the same
      # box: Fastify + pg pool 4, the template's routes. All three things this
      # needs are checked here, because each one failed silently before: a
      # non-interactive shell has no nvm on PATH, so `command -v node` came back
      # empty and /node was mounted from a directory that has no bin/node.
      # The host's Node (bind-mounted with the bench's node_modules) in the
      # same box. All three things this needs are checked here, because each
      # one failed silently before: a non-interactive shell has no nvm on
      # PATH, so `command -v node` came back empty and /node was mounted from
      # a directory that has no bin/node.
      local node_bin node_dir
      node_bin="$(command -v "$NODE" 2>/dev/null || true)"
      if [ -z "$node_bin" ]; then
        cell_failed "$dir" "$cell" "NODE=$NODE is not on PATH (a non-interactive shell has no nvm) - set NODE to an absolute path"
        return 0
      fi
      # The *real* path: `node` on PATH is usually a symlink into a versioned
      # directory (nvm, ~/.local/opt/...), and a bind mount of the symlink's
      # parent carries the link, not its target.
      node_bin="$(readlink -f "$node_bin")"
      node_dir="$(cd "$(dirname "$node_bin")/.." && pwd)"
      if [ ! -x "$node_dir/bin/node" ] || [ "$(readlink -f "$node_dir/bin/node")" != "$node_bin" ]; then
        cell_failed "$dir" "$cell" "$node_bin is not <prefix>/bin/node, so /node/bin/node would not exist in the box"
        return 0
      fi
      if [ ! -d "$repo/scripts/qualification/bench/node_modules/fastify" ]; then
        cell_failed "$dir" "$cell" "the bench's node dependencies are missing - run fleet.sh prepare"
        return 0
      fi
      ready_url="http://127.0.0.1:$port/health"; label="node+"
      evict_for_cell "$node_bin" "$repo/scripts/qualification/bench/node_modules" "$repo/scripts/qualification/bench/baselines/node-fastify"
      docker run -d --name "$name" --network host --user "$(id -u):$(id -g)" \
        --cpus "$cpus" --memory "${mem}m" --memory-swap "${mem}m" ${PIN:+--cpuset-cpus "$PIN"} \
        -v "$node_dir:/node:ro" -v "$repo/scripts/qualification/bench:/bench:ro" -w /bench --read-only --tmpfs /tmp \
        -e HOME=/tmp -e DATABASE_URL="$DATABASE_URL" -e PORT="$port" -e POOL_MAX=4 \
        "$BASE_IMAGE" /node/bin/node baselines/node-fastify/server.mjs \
        > "$dir/container.id" 2> "$dir/docker.err" \
        || { cell_failed "$dir" "$cell" "docker run failed: $(head -c 300 "$dir/docker.err")"; return 0; };;
    php)
      # The tuned PHP-FPM compose (opcache + JIT, pm=static <worlds> children)
      # with the cell's memory and cpu limits on the php service; nginx unlimited.
      ready_url="http://127.0.0.1:3006/health"; label="php+"
      (cd "$repo/scripts/qualification/bench/baselines/php" && PHP_PROFILE=tuned PHP_CHILDREN="$worlds" PHP_PORT=3006 \
        PHP_MEM_LIMIT="${mem}m" PHP_CPUS="$cpus" PHP_CPUSET="${PIN:-}" DATABASE_URL="${DATABASE_URL//127.0.0.1/host.docker.internal}" \
        docker compose up -d --build > "$dir/compose.log" 2>&1)
      name=$(cd "$repo/scripts/qualification/bench/baselines/php" && docker compose ps -q php)
      port=3006
      # The compose file's mem_limit/cpus did not reach the container in the
      # run that produced the 2026-09-23 floors, so apply them to the container
      # itself; the limits check below is what proves either way.
      docker update --memory "${mem}m" --memory-swap "${mem}m" --cpus "$cpus" \
        ${PIN:+--cpuset-cpus "$PIN"} "$name" >/dev/null 2>&1 || true;;
  esac
  local started; started=$(date +%s.%N)
  local ready=""
  for _ in $(seq 1 120); do
    if curl -sf -m 5 "$ready_url" >/dev/null 2>&1; then ready=$(date +%s.%N); break; fi
    if [ "$(docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null)" != "true" ]; then break; fi
    sleep 0.5
  done
  local pid; pid=$(docker inspect -f '{{.State.Pid}}' "$name" 2>/dev/null || echo 0)
  if [ -z "$ready" ] || [ "$pid" = 0 ]; then
    log "  did not become ready (OOMKilled=$(docker inspect -f '{{.State.OOMKilled}}' "$name" 2>/dev/null))"
    docker logs "$name" > "$dir/server.log" 2>&1 || true
    echo "{\"cell\":\"$cell\",\"pass\":false,\"reason\":\"not ready\"}" > "$dir/result.json"
    floor_teardown "$app" "$name"
    return 0
  fi
  echo "ready_seconds $(echo "$ready - $started" | bc)" > "$dir/timing.txt"
  # A box that did not take the cell's limits prices the wrong machine, and it
  # does it silently: ask the daemon what it applied, not what we asked for.
  # (The php compose path is where this was found — its mem_limit/cpus reached
  # the file but not the container.)
  local applied_mem applied_cpu want_mem want_cpu
  applied_mem=$(docker inspect -f '{{.HostConfig.Memory}}' "$name" 2>/dev/null || echo 0)
  applied_cpu=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$name" 2>/dev/null || echo 0)
  want_mem=$((mem * 1024 * 1024))
  want_cpu=$(awk -v c="$cpus" 'BEGIN { printf "%.0f", c * 1000000000 }')
  if [ "$applied_mem" != "$want_mem" ] || [ "$applied_cpu" != "$want_cpu" ]; then
    docker logs "$name" > "$dir/server.log" 2>&1 || true
    cell_failed "$dir" "$cell" "the box did not take the cell's limits (memory $applied_mem want $want_mem, nanocpus $applied_cpu want $want_cpu)"
    floor_teardown "$app" "$name"
    return 0
  fi
  local sampled="$label:$pid"
  if [ "$app" = php ]; then
    local nginx; nginx=$(docker inspect -f '{{.State.Pid}}' "$(cd "$repo/scripts/qualification/bench/baselines/php" && docker compose ps -q nginx)" 2>/dev/null || echo 0)
    sampled="$sampled nginx+:$nginx"
  fi
  # shellcheck disable=SC2086
  "$here/sample.sh" "$dir/samples.jsonl" $sampled & local sampler=$!
  local base="http://127.0.0.1:$port"
  # Phases: idle → load → idle. The template gets the production pattern.
  if [ "$app" = hello ]; then
    phase idle 60
    phase load "$base" A 1 60
    phase load "$base" A 4 60
    phase idle 60
  else
    phase idle 120
    phase trickle "$base" 180
    phase load "$base" C 16 60
    phase idle 300
  fi
  # What the kernel charged this box, which is the number a floor is about:
  # the peak it ever held, how often it hit the ceiling and had to reclaim
  # (memory.events `max`), and whether anything in it was killed. A cell that
  # never OOMs but sits against its ceiling is passing on reclaim, and the
  # report should be able to say so.
  local cg peak_bytes end_bytes max_events oom_kills
  cg=$(cgroup_dir "$pid" || true)
  peak_bytes=$(cgroup_field "${cg:-/nonexistent}/memory.peak" "")
  end_bytes=$(cgroup_field "${cg:-/nonexistent}/memory.current" "")
  max_events=$(cgroup_field "${cg:-/nonexistent}/memory.events" max)
  oom_kills=$(cgroup_field "${cg:-/nonexistent}/memory.events" oom_kill)
  # Verdict inputs: OOM, errors, readiness at the end.
  local oom; oom=$(docker inspect -f '{{.State.OOMKilled}}' "$name" 2>/dev/null || echo unknown)
  local running; running=$(docker inspect -f '{{.State.Running}}' "$name" 2>/dev/null || echo false)
  local ready_end=false; curl -sf -m 5 "$ready_url" >/dev/null 2>&1 && ready_end=true
  if [ "$label" = usai ]; then
    curl -s -m 5 "http://127.0.0.1:$status/_usai/status" > "$dir/status.json" 2>/dev/null || true
    curl -s -m 5 "http://127.0.0.1:$status/_usai/metrics" > "$dir/metrics.txt" 2>/dev/null || true
  fi
  kill "$sampler" 2>/dev/null || true; wait "$sampler" 2>/dev/null || true
  docker logs "$name" > "$dir/server.log" 2>&1 || true
  floor_teardown "$app" "$name"
  echo "{\"cell\":\"$cell\",\"app\":\"$app\",\"memMib\":$mem,\"cpus\":$cpus,\"worlds\":$worlds,\"appliedMemoryBytes\":$applied_mem,\"appliedNanoCpus\":$applied_cpu,\"cgroupPeakBytes\":${peak_bytes:-0},\"cgroupEndBytes\":${end_bytes:-0},\"cgroupCeilingHits\":${max_events:-0},\"cgroupOomKills\":${oom_kills:-0},\"coldCache\":\"$COLD\",\"oomKilled\":$oom,\"runningAtEnd\":$running,\"readyAtEnd\":$ready_end}" > "$dir/result.json"
  log "  done: oom=$oom running=$running ready=$ready_end peak=$(( ${peak_bytes:-0} / 1048576 ))MiB ceiling_hits=${max_events:-0}"
}

floor_teardown() {
  if [ "$1" = php ]; then (cd "$repo/scripts/qualification/bench/baselines/php" && docker compose down >/dev/null 2>&1); return; fi
  docker stop -t 10 "$2" >/dev/null 2>&1 || true
  docker rm -f "$2" >/dev/null 2>&1 || true
}

# A phase writes a marker line into the run's phase log so the report can cut
# the samples: idle N seconds; load <base> <class> <clients> <seconds>;
# trickle <base> <seconds> (1 req/s across hello and a DB read).
phase() {
  local kind="$1"; shift
  local t0; t0=$(date -u +%s.%N)
  case "$kind" in
    idle) sleep "$1";;
    load) (cd "$repo/scripts/qualification/bench" && pin "$NODE" load.mjs "$1" "$2" "$3" "$4" > "$(dirname "$PHASES")/load-$2-c$3-$(date -u +%H%M%S).json" 2>&1) || true;;
    trickle) local end=$(( $(date +%s) + $2 )); local i=0
             while [ "$(date +%s)" -lt "$end" ]; do
               i=$((i + 1))
               if [ $((i % 2)) = 0 ]; then curl -s -m 5 -o /dev/null "$1/hello/x"; else curl -s -m 5 -o /dev/null "$1/users/42"; fi
               sleep 1
             done;;
  esac
  echo "{\"phase\":\"$kind\",\"args\":\"$*\",\"from\":$t0,\"to\":$(date -u +%s.%N)}" >> "$PHASES"
}

# ---- density: n bare processes ------------------------------------------------
# 45 of 50 idle, 4 at 1 req/s, 1 bursting 100 req/s for 10 s twice — the
# shared-host pattern; the same driver runs the Node comparator.
density() {
  local kind="$1" n="$2" minutes="${3:-10}"
  local dir="$OUT/density-$kind-$n"; mkdir -p "$dir"
  PHASES="$dir/phases.jsonl"
  log "== density $kind n=$n ${minutes} min"
  local pids=() labels=()
  local i
  for i in $(seq 1 "$n"); do
    local port=$((3810 + i)) status=$((3900 + i))
    if ss -ltn 2>/dev/null | grep -qE ":$port\b|:$status\b"; then
      log "  port $port or $status is in use (a stale instance?); refusing to measure someone else's process"; exit 1
    fi
    case "$kind" in
      usai) pin_bg env USAI_MAX_WORLDS=16 "$USAI" --root "$TEMPLATE" run --artifact "$TEMPLATE/.usai/build" --port "$port" --status-addr "127.0.0.1:$status" --drain-timeout 5 > "$dir/app-$i.log" 2>&1 & ;;
      node) pin_bg env PORT="$port" POOL_MAX=4 "$NODE" "$NODE_APP" > "$dir/app-$i.log" 2>&1 & ;;
      *) echo "density: usai|node"; exit 2;;
    esac
    pids+=($!); labels+=("app$i:$!")
  done
  # Readiness: every instance answers /health.
  local up=0
  for _ in $(seq 1 240); do
    up=0
    for i in $(seq 1 "$n"); do curl -sf -m 5 "http://127.0.0.1:$((3810 + i))/health" >/dev/null 2>&1 && up=$((up + 1)); done
    [ "$up" = "$n" ] && break
    sleep 0.5
  done
  log "  $up/$n up"
  bash ${SAMPLE_TRACE:+-x} "$here/sample.sh" "$dir/samples.jsonl" "${labels[@]}" 2> "$dir/sampler.err" & local sampler=$!
  local total=$((minutes * 60))
  echo "{\"phase\":\"settle\",\"from\":$(date -u +%s.%N),\"to\":$(($(date -u +%s) + 60))}" >> "$PHASES"
  sleep 60
  # Trickle on up to 4 apps, bursts on app 1 at the 1/3 and 2/3 marks; the rest idle.
  local end=$(( $(date +%s) + total - 60 ))
  local burst_at1=$(( $(date +%s) + (total - 60) / 3 )) burst_at2=$(( $(date +%s) + 2 * (total - 60) / 3 ))
  local done1=0 done2=0 tick=0
  while [ "$(date +%s)" -lt "$end" ]; do
    tick=$((tick + 1))
    local curls=()
    for i in $(seq 2 $(( n < 5 ? n : 5 ))); do
      if [ $((tick % 2)) = 0 ]; then curl -s -m 5 -o /dev/null "http://127.0.0.1:$((3810 + i))/hello/x" & else curl -s -m 5 -o /dev/null "http://127.0.0.1:$((3810 + i))/users/42" & fi
      curls+=($!)
    done
    for c in "${curls[@]}"; do wait "$c" 2>/dev/null || true; done
    local now; now=$(date +%s)
    if [ "$done1" = 0 ] && [ "$now" -ge "$burst_at1" ]; then done1=1; burst "$dir" 1 "http://127.0.0.1:3811"; fi
    if [ "$done2" = 0 ] && [ "$now" -ge "$burst_at2" ]; then done2=1; burst "$dir" 2 "http://127.0.0.1:3811"; fi
    sleep 1
  done
  # Cold first response: an app that saw no traffic during the whole run.
  if [ "$n" -gt 5 ]; then
    local cold=$((3810 + n))
    local t; t=$(curl -s -m 5 -o /dev/null -w '%{time_total}' "http://127.0.0.1:$cold/users/42")
    echo "{\"coldFirstResponseSeconds\":$t}" > "$dir/cold.json"
  fi
  if [ "$kind" = usai ]; then
    for i in $(seq 1 "$n"); do curl -s -m 5 "http://127.0.0.1:$((3900 + i))/_usai/status" > "$dir/status-$i.json" 2>/dev/null || true; done
  fi
  kill "$sampler" 2>/dev/null || true; wait "$sampler" 2>/dev/null || true
  for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done
  for p in "${pids[@]}"; do wait "$p" 2>/dev/null || true; done
  echo "{\"kind\":\"$kind\",\"n\":$n,\"minutes\":$minutes,\"up\":$up}" > "$dir/result.json"
  log "  done"
}

burst() { # dir index base — 100 req/s for 10 s on hello (oha rate-limited), recorded
  local dir="$1" idx="$2" base="$3"
  local t0; t0=$(date -u +%s.%N)
  if command -v oha >/dev/null; then
    pin oha -q 100 -z 10s -c 8 --no-tui --output-format json "$base/hello/burst" > "$dir/burst-$idx.json" 2>/dev/null || true
  else
    (cd "$repo/scripts/qualification/bench" && pin "$NODE" load.mjs "$base" A 8 10 > "$dir/burst-$idx.json" 2>&1) || true
  fi
  echo "{\"phase\":\"burst\",\"args\":\"$idx\",\"from\":$t0,\"to\":$(date -u +%s.%N)}" >> "$PHASES"
}

report() { "$NODE" "$here/report.mjs" "$1"; }

case "${1:-}" in
  prepare) prepare;;
  floor) shift; floor "$@";;
  density) shift; density "$@";;
  report) shift; report "$@";;
  *) sed -n 2,14p "$0"; exit 2;;
esac
