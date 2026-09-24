#!/usr/bin/env bash
# A PostgreSQL for the suites that need one, without Docker.
#
# `make check` passes on a box with no database because every suite that
# needs one *skips*: the Rust `postgres` test, the example projects' tests,
# the queue and cron campaigns. That is the right behaviour for a
# contributor who is not touching those paths, and a trap for one who is —
# a change to the test harness passed `make check` locally and failed CI on
# `examples/invoicing`, which is exactly the test it should have broken.
#
#   eval "$(scripts/dev-postgres.sh start)"   # exports USAI_TEST_DATABASE_URL
#   make check
#   scripts/dev-postgres.sh stop
#
# It reuses the portable server the Rust tests cache in
# ~/.cache/usai/postgresql/<version>, and downloads it once if it is not
# there (~12 MB, the same asset those tests use).
set -euo pipefail

VERSION=18.6.0
ASSET="https://github.com/theseus-rs/postgresql-binaries/releases/download/${VERSION}/postgresql-${VERSION}-x86_64-unknown-linux-gnu.tar.gz"
CACHE="${HOME}/.cache/usai/postgresql/${VERSION}"
STATE="${TMPDIR:-/tmp}/usai-dev-postgres"
# The Unix socket path has a 107-byte limit and a scratch directory under a
# session temp path is longer than that, so the socket lives on its own.
SOCK="/tmp/usai-devpg"
PORT="${USAI_DEV_PG_PORT:-55444}"

fetch() {
  [ -x "${CACHE}/bin/initdb" ] && return 0
  echo "downloading portable PostgreSQL ${VERSION} to ${CACHE}" >&2
  mkdir -p "${CACHE}"
  curl -fsSL "${ASSET}" | tar -xz -C "${CACHE}" --strip-components=1
}

case "${1:-start}" in
  start)
    fetch
    if [ -f "${STATE}/postmaster.pid" ] && "${CACHE}/bin/pg_ctl" -D "${STATE}" status >/dev/null 2>&1; then
      :
    else
      rm -rf "${STATE}" "${SOCK}"
      mkdir -p "${STATE}" "${SOCK}"
      "${CACHE}/bin/initdb" -D "${STATE}" -U postgres --auth=trust >/dev/null
      "${CACHE}/bin/pg_ctl" -D "${STATE}" \
        -o "-p ${PORT} -k ${SOCK} -h 127.0.0.1" -l "${STATE}/log.txt" start >/dev/null
      "${CACHE}/bin/createdb" -h 127.0.0.1 -p "${PORT}" -U postgres usai_dev
    fi
    URL="postgres://postgres@127.0.0.1:${PORT}/usai_dev"
    echo "export USAI_TEST_DATABASE_URL=${URL}"
    echo "export DATABASE_URL=${URL}"
    ;;
  stop)
    if [ -d "${STATE}" ]; then
      "${CACHE}/bin/pg_ctl" -D "${STATE}" stop -m fast >/dev/null 2>&1 || true
      rm -rf "${STATE}" "${SOCK}"
    fi
    echo "stopped" >&2
    ;;
  url)
    echo "postgres://postgres@127.0.0.1:${PORT}/usai_dev"
    ;;
  *)
    echo "usage: $0 [start|stop|url]" >&2
    exit 2
    ;;
esac
