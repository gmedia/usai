#!/usr/bin/env bash
# Builds the guest core (quickjs-async.wasm) from pinned sources.
#
#   guest/build.sh              # -O3 (the production core)
#   OPT=-Oz guest/build.sh      # reproduces the research core byte for byte
#                               # (sha256 6b33cb45…), given the same toolchain
#
# Inputs are pinned in PROVENANCE.md. Everything is fetched into
# ${USAI_GUEST_BUILD:-target/guest-build} and the WASI SDK is cached in
# ~/.cache/usai/wasi-sdk-32. No root required.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
build="${USAI_GUEST_BUILD:-$repo/target/guest-build}"
sdk="${WASI_SDK:-$HOME/.cache/usai/wasi-sdk-32}"
opt="${OPT:--O3}"

QUICKJS_WASI_URL=https://github.com/vercel-labs/quickjs-wasi.git
QUICKJS_WASI_COMMIT=54c4d2dd4be2445409aeab603ecfc3bb209c7310
QUICKJS_NG_URL=https://github.com/quickjs-ng/quickjs.git
QUICKJS_NG_COMMIT=65641a0c1e85cc266d7613d6673a22ec834bb941
WASI_SDK_URL=https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-32/wasi-sdk-32.0-x86_64-linux.tar.gz

if [ ! -x "$sdk/bin/clang" ]; then
  echo "downloading WASI SDK 32 to $sdk"
  mkdir -p "$sdk"
  curl -sL --fail "$WASI_SDK_URL" | tar xz -C "$sdk" --strip-components=1
fi

checkout() {
  local dir="$1" url="$2" commit="$3"
  if [ ! -d "$dir/.git" ]; then
    mkdir -p "$(dirname "$dir")"
    git init -q "$dir"
    git -C "$dir" remote add origin "$url"
    git -C "$dir" fetch -q --depth=1 origin "$commit"
    git -C "$dir" checkout -q --detach FETCH_HEAD
  fi
  git -C "$dir" checkout -q -- .
  [ "$(git -C "$dir" rev-parse HEAD)" = "$commit" ] || { echo "$dir is not at $commit" >&2; exit 1; }
}

src="$build/quickjs-wasi"
ng="$src/quickjs-ng"
checkout "$src" "$QUICKJS_WASI_URL" "$QUICKJS_WASI_COMMIT"
checkout "$ng" "$QUICKJS_NG_URL" "$QUICKJS_NG_COMMIT"

for patch in exp011a-entropy-quickjs-ng.patch; do git -C "$ng" apply "$here/patches/$patch"; done
for patch in exp011a-entropy-quickjs-wasi.patch exp011c-quickjs-wasi-async-bridge.patch usai-direct-call.patch usai-native-codecs.patch; do git -C "$src" apply "$here/patches/$patch"; done

make -C "$src" WASI_SDK="$sdk" clean >/dev/null
make -C "$src" WASI_SDK="$sdk" OPT="$opt" quickjs.wasm

out="$here/quickjs-async.wasm"
cp "$src/quickjs.wasm" "$out"
echo "built $out ($(wc -c < "$out") bytes, OPT=$opt)"
sha256sum "$out"
