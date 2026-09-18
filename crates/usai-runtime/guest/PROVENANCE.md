# Guest core provenance

`quickjs-async.wasm` is the QuickJS-ng + WASI + split-phase operation bridge
core the Wasm execution substrate runs (ADR-0016). It is built by
`build.sh` from the sources the research lineage pinned and sealed
(EXP-011C Stage B core, reused byte for byte by EXP-011D, EXP-012A-R1 and
EXP-012B), with one change: `-O3` instead of the research's `-Oz`.

```text
production core (OPT=-O3)   sha256 c4e58003609cc13ebc23b7c987999d6a5d366be5b84afe7f6f240aeb2a9dcf11   1264841 bytes
research core   (OPT=-Oz)   sha256 6b33cb45e2806fbd1c7add768b613985d4c841d0650f27fe659210449ac53b60    756910 bytes
```

`OPT=-Oz build.sh` reproduces the research core byte for byte (verified
2026-09-18); the recipe is therefore the same recipe.

## How it was built

| Input | Identity |
|---|---|
| quickjs-wasi | https://github.com/vercel-labs/quickjs-wasi.git @ `54c4d2dd4be2445409aeab603ecfc3bb209c7310` |
| quickjs-ng | https://github.com/quickjs-ng/quickjs.git @ `65641a0c1e85cc266d7613d6673a22ec834bb941` |
| WASI SDK | 32 (`wasi-sdk-32.0-x86_64-linux.tar.gz`) |
| patch | `exp011a-entropy-quickjs-ng.patch` — sha256 `3a0d33a5…` (Math.random reseed export) |
| patch | `exp011a-entropy-quickjs-wasi.patch` — sha256 `9f23dd51…` |
| patch | `exp011c-quickjs-wasi-async-bridge.patch` — sha256 `4985bba1…` (the `__usai_test_op` / `usai_op_start` / `qjs_usai_*` bridge, 271 lines) |

The patches are vendored in `patches/`; `build.sh` fetches the pinned
sources and the WASI SDK (no root needed), applies them, and builds. The
research repository (private; `artifacts/exp011c/patched-base-build.json`)
holds the original record. A checksum test refuses a modified core.

## ABI the runtime relies on

Imports (module `env`): `usai_op_start(op_id: u32, kind: u32, payload_ptr, payload_len) -> i32`
plus a small WASI preview1 subset (`clock_time_get`, `random_get`, `fd_*`) and
inert `host_*` hooks.

Exports used: `memory`, `wasm_malloc`, `wasm_free`, `qjs_init`, `qjs_eval`,
`qjs_is_exception`, `qjs_get_exception`, `qjs_get_string`, `qjs_free_cstring`,
`qjs_free_value`, `qjs_is_job_pending`, `qjs_execute_pending_job`,
`qjs_reseed_math_random`, `qjs_usai_install_async_bridge`,
`qjs_usai_pending_op_count`, `qjs_usai_op_complete`, `_initialize`.

JS side: `globalThis.__usai_test_op(kind, payload) -> Promise` (installed by
`qjs_usai_install_async_bridge`). The guest bridge (`src/engine/guest-bridge.js`)
detects it and routes every operation through it; operation ids are
allocated by the core (sequential from 1) and mirrored by the bridge.
