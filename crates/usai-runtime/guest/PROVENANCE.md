# Guest core provenance

`quickjs-async.wasm` is the QuickJS-ng + WASI + split-phase operation bridge
core the Wasm execution substrate runs (ADR-0016). It is built by
`build.sh` from the sources the research lineage pinned and sealed
(EXP-011C Stage B core, reused byte for byte by EXP-011D, EXP-012A-R1 and
EXP-012B), with three changes: `-O3` instead of the research's `-Oz`, the
direct-call exports (ADR-0018) so the host enters the guest by calling
instead of evaluating, and native byte↔text codecs on
`globalThis.__usai_native` (UTF-8 and base64 over `Uint8Array`) so a request
body is not decoded one interpreted iteration per byte.

```text
production core (OPT=-O3)   sha256 91f178dff664c187c0c8aa9d83ca1ab2a67bad6c78c946aa1329031a02167502   1269244 bytes   (0.0.6+, with usai-direct-call.patch and usai-native-codecs.patch)
before codecs   (OPT=-O3)   sha256 d9e9d7b53077995b9fab145873a13ba4aced5073dee27a881bb0d42c1a9dfccd   1266779 bytes   (main between ADR-0018 and the codecs)
previous core   (OPT=-O3)   sha256 c4e58003609cc13ebc23b7c987999d6a5d366be5b84afe7f6f240aeb2a9dcf11   1264841 bytes   (0.0.1 – 0.0.5)
research core   (OPT=-Oz)   sha256 6b33cb45e2806fbd1c7add768b613985d4c841d0650f27fe659210449ac53b60    756910 bytes
```

`OPT=-Oz build.sh` without our two patches reproduces the research core
byte for byte (verified 2026-09-18); the recipe is therefore the same recipe
plus two patches of our own.

## How it was built

| Input | Identity |
|---|---|
| quickjs-wasi | https://github.com/vercel-labs/quickjs-wasi.git @ `54c4d2dd4be2445409aeab603ecfc3bb209c7310` |
| quickjs-ng | https://github.com/quickjs-ng/quickjs.git @ `65641a0c1e85cc266d7613d6673a22ec834bb941` |
| WASI SDK | 32 (`wasi-sdk-32.0-x86_64-linux.tar.gz`) |
| patch | `exp011a-entropy-quickjs-ng.patch` — sha256 `3a0d33a5…` (Math.random reseed export) |
| patch | `exp011a-entropy-quickjs-wasi.patch` — sha256 `9f23dd51…` |
| patch | `exp011c-quickjs-wasi-async-bridge.patch` — sha256 `4985bba1…` (the `__usai_test_op` / `usai_op_start` / `qjs_usai_*` bridge, 271 lines) |
| patch | `usai-direct-call.patch` — sha256 `686fc019…` (`qjs_usai_inbuf`, `qjs_usai_enter`, `qjs_usai_settle`: the host calls `globalThis.__usai[name](arg)` through core-owned buffers and drains the job queue in the core; ours, ADR-0018) |
| patch | `usai-native-codecs.patch` — sha256 `3fc5f245…` (`globalThis.__usai_native.utf8dec/utf8enc/b64dec/b64enc` installed at `qjs_init`: UTF-8 and base64 over `Uint8Array` in C; ours) |

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
`qjs_reseed_math_random`, `qjs_run_gc`, `qjs_usai_install_async_bridge`,
`qjs_usai_pending_op_count`, `qjs_usai_op_complete`, `qjs_usai_inbuf`,
`qjs_usai_enter`, `qjs_usai_settle`, `_initialize`. `qjs_eval` and the
value helpers are used when the image is built (module evaluation,
validator warm-up) and by tests; on the request path the host calls
`qjs_reseed_math_random`, `qjs_usai_inbuf`, `qjs_usai_enter`,
`qjs_usai_settle` and, for operations, `qjs_usai_op_complete`
(`docs/GUEST-ABI.md`, ADR-0018).

JS side: `globalThis.__usai_test_op(kind, payload) -> Promise` (installed by
`qjs_usai_install_async_bridge`). The guest bridge (`src/engine/guest-bridge.js`)
detects it and routes every operation through it; operation ids are
allocated by the core (sequential from 1) and mirrored by the bridge.
