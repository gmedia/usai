# Environment variables

> **These documents describe `main`, not the last release.** A `0.0.x` line
> moves quickly and this repository is the working copy: when a page and the
> binary you are running disagree, the binary is right, and `CHANGELOG.md`
> says which version changed what. Anything added since the last tag is
> marked *(since X.Y.Z)* where it matters.

Everything the runtime, the CLI and the npm launcher read from the
environment, in one place. Three groups: **operating** an instance (what a
deployment sets), **tuning knobs** for measurements and profiling (not
application configuration — the defaults are the measured choice), and
**developer tooling** (the launcher, the test harness).

The application's own variables (`DATABASE_URL`, whatever `env("NAME")`
declares) are not here: they are the application's, listed by `usai
inspect` under *Environment* and by `/_usai/docs`, and validated at
activation (`missing required environment` refuses the revision, never a
request). `usai run` reads them from the process environment only; `.env`
files are for `usai dev` (`GUIDE.md` §13).

Every operating variable has a flag of the same meaning (`--max-worlds` ↔
`USAI_MAX_WORLDS`); the flag wins when both are set. Prefer the variable
for secrets — a flag shows in `ps`. Boolean variables are on for `1`,
`true`, `yes`, `on` (any case) and off for anything else.

## Operating an instance

| Variable | Flag | Default | Meaning |
|---|---|---|---|
| `USAI_MAX_WORLDS` | `--max-worlds` | `256` | Execution worlds this instance holds at once — the admission bound. Past it a request is `503 capacity_exhausted` before a world exists. Each slot reserves address space up front and keeps ≈ 4 MiB resident once touched (`runbooks/sizing.md`). |
| `USAI_DRAIN_TIMEOUT` | `--drain-timeout` | `30` (s) | How long a stop or a revision replacement waits for in-flight work before cancelling it (`runbooks/runtime-restart.md`). The cancelling is the **stop** path's (SIGTERM, `POST /stop`): work still running at the bound is cancelled and its clients get `499`. `POST /revisions/{id}/drain` uses the same bound to decide when to answer `409` and does **not** cancel — that work runs to completion and its clients get their answers (measured: a 20 s request returned 200 nineteen seconds after a 3 s bound had already answered). The orchestrator's grace period must exceed grace + timeout. |
| `USAI_DRAIN_GRACE` | `--drain-grace` | `2` (s) | On SIGTERM, keep serving this long while `/_usai/ready` answers 503 and responses carry `Connection: close`, so a balancer stops routing here before the listener closes. At or above the balancer's health-check interval; `0` closes the listener at once. |
| `USAI_STATUS_ADDR` | `--status-addr` | unset (`--status` puts the surfaces on the app listener) | A separate listener for `/_usai/status`, `/_usai/metrics`, `/_usai/live`, `/_usai/ready`, `/_usai/docs`, `/_usai/openapi.json` — a private address (`127.0.0.1:9090`, a pod-local IP). What production wants. `usai probe live|ready` reads it too, so one value configures the server and its probe (`--timeout` bounds the probe itself, 3 s by default). In a pod it must be `0.0.0.0:9090` (or the pod IP): the kubelet probes the pod's address, not its loopback. |
| `USAI_STATUS_TOKEN` | `--status-token` | unset | `Authorization: Bearer <token>` required on `/_usai/status`, `/_usai/metrics` and `/_usai/openapi.json` on both listeners; `/_usai/live` and `/_usai/ready` stay open for probes; the `/_usai/docs` shell asks for it in the browser. Read at start. |
| `USAI_ACTIVATION_RETRY` *(since 0.0.10)* | — | `60` | Seconds to keep retrying when a **bound resource** cannot be opened at activation. Activation still fails rather than the first request, and the process still exits with the same error when the budget ends — the budget only changes *when*. It is the default because exiting at once is `CrashLoopBackOff` on an orchestrator, with a backoff that outlives the outage which caused it: a pod restarted for an unrelated reason during a database blip stayed down long after the database was healthy. The process retries every 2 s and says so, so a startup probe with enough `failureThreshold` covers the blip. A missing or malformed *variable* is never retried — no waiting fixes a configuration error. `0` restores the immediate exit (ADR-0023). |
| `USAI_READY_REQUIRES_RESOURCES` | — | `0` | Whether a bound resource failing its probe makes `/_usai/ready` answer 503. **Off by default since 0.0.10** (ADR-0022): a proxy removes an unready upstream, so coupling them turns a *shared* database outage — the common case — into a total one, taking out every route on every replica including those that never touch the database. Uncoupled, readiness reports the failing resource in the body and the routes that need it answer 503 on their own. Set `1` when each replica owns its database, where taking itself out of rotation lets the others carry the traffic. Draining fails readiness either way. See `docs/runbooks/postgres-down.md`. |
| `USAI_SURFACES_OFF` | `--surfaces-off` | unset | Comma-separated surfaces to switch off on both listeners (a 404): `status`, `metrics`, `docs` (reference + OpenAPI), `live`, `ready`. `docs,metrics` when neither is consumed. |
| `USAI_CONTROL_TOKEN` | — | unset | Bearer token for the control surface (`--control <addr>`, `CONTROL-API.md`). Required when the control address is not loopback; the runtime refuses to bind otherwise. Read at start. |
| `USAI_REQUIRE_SIGNATURE` | `--require-signature` | unset | Ed25519 public keys (hex, comma-separated, or a file with one per line); an artifact not signed by one of them is refused at install (`GUIDE.md` §14, "Signed artifacts"). |
| `USAI_SIGNING_KEY` | `usai build --sign` | unset | The private key `usai build` signs the artifact with (`usai keygen`). Build side only. |
| `USAI_NO_CRON` | `--no-cron` | off | Do not run the cron scheduler here. Every instance that schedules ticks every non-`exclusive` schedule: with several replicas, set it on all but one (`SUPPORTED.md`, topology). |
| `USAI_NO_QUEUE` | `--no-queue` | off | Do not run queue consumers here. Consumers on several replicas share the work safely; this dedicates replicas, it is not needed for correctness. |
| `USAI_NO_SERVICES` | `--no-services` | off | Do not run `service()` declarations here. Every instance that runs them has its own copy of each. |
| `USAI_MAX_BODY_BYTES` | — | `1048576` (1 MiB) | Request body bound for the **process**, enforced before any world exists (`413` above it, and the message names which bound refused it). It is the ceiling: a route may declare `maxBodyBytes` to accept **less**, never more, so the import route that needs 8 MiB is the reason to raise this and the other routes stay small by declaring their own. A proxy in front usually bounds it too. |
| `USAI_SOCKET_IDLE_TIMEOUT` | — | `300` (s) | A WebSocket with no frame in **either direction** for this long is closed with `1008`, and its world with it — a server push resets the timer too, so a busy connection never idles out and a quiet one disconnects every viewer. A browser's `WebSocket` cannot send a ping frame from JavaScript, so the heartbeat has to be a server tick or an ordinary application message on a timer (GUIDE §9). It is **not** what reaps a vanished client: the connection ending aborts that world directly. |
| `USAI_SERVER_TIMING` | `--server-timing` | off | Put `Server-Timing: total;dur=…, world;dur=…, cpu;dur=…` on every response the application produced: the request end to end, how long the world was alive, and how much of that the guest spent computing rather than waiting. Timing only — no stacks, no error text — so it may stay on in production; a request rejected before a world exists carries no header (`runbooks/slow-route.md`). |
| `USAI_DIAGNOSTICS` | `--diagnostics` | off | Expose to clients what `usai dev` exposes: error details and stacks in 500 bodies, `x-usai-lifecycle` and `x-usai-server-ms` headers. Tests and trusted networks only. |
| `USAI_ENGINE` | `--engine` | `wasm` | Execution substrate: `wasm` (Wasmtime + the precompiled core, the default and the qualified one) or `quickjs` (the native reference engine, ADR-0015; for differential tests). |
| `RUST_LOG` | `-v` | `usai=info,usai_runtime=info,app=info` for `run`/`dev`; `warn` for one-shot commands (`-v` raises them to info); the compilers (`wasmtime`, `cranelift_*`) at `warn` unless named | A `tracing` filter (`usai_runtime=debug`). `--log-format json` chooses the format; this chooses the level. `app` is the application's own `console`/`ctx.log` lines — and a `RUST_LOG` that names **no** level for `app` keeps them at INFO whatever else it says, so `RUST_LOG=warn` quiets the runtime without deleting the application's own log (it used to delete it, which also blinded `app.logs()` and `waitForLog` in the test harness). An explicit `app=` directive wins, including `app=off`. **`RUST_LOG='warn,usai_runtime::observability=debug'` is the one to reach for during an incident**: it gives the `world trace` line — one per unit of work, with `duration_ms`, `cpu_us`, `request_id` and the workload — on top of the runtime's ordinary WARN and ERROR. **Write the `warn,` — without it the filter *replaces* the default rather than adding to it**, and you lose every `application error`, `task failed` and `message dead-lettered` line at the moment you turned it on to read them. Plain `debug` is seven lines per request (~3.8 MB/s at four clients), which is not something to turn on in production (`runbooks/slow-route.md`). |
| `USAI_EXIT_WITH_PARENT` | `--exit-with-parent` | off | Exit when the process that started this one does (Linux `PR_SET_PDEATHSIG`). For a supervisor that cannot clean up after itself — a test runner killed by a CI cancel, an editor task — which otherwise leaves a runtime serving and holding its database pool. `usai/test` sets it for the runtime it spawns. |
| `USAI_TEST_LOGS` | — | `problems` | What the **test harness** prints from the runtime it spawned: `problems` (the application's own lines plus WARN and above), `all`, or `none`. Every line is kept for `app.logs()` either way. |

## Tuning knobs (measurements and profiling)

Not application configuration; the defaults are what the measurements in
`docs/measurements/` chose. Set them for a study, not in a deployment,
unless a runbook says so.

| Variable | Default | Meaning |
|---|---|---|
| `USAI_WASM_KEEP_RESIDENT` | `8m` | Memory a world slot keeps resident after use (`8 MiB`, `2m`, `0`; k/m/g suffixes). Measured: 2 MiB is identical to 8; `0` returns the memory after every burst at half the throughput on one vCPU (P8E C2). The one knob a memory-constrained, bursty box might set (`runbooks/memory-pressure.md`). |
| `USAI_WASM_PAGEMAP_SCAN` | on | `0` disables the kernel `PAGEMAP_SCAN` fast path for resetting slot memory (Linux ≥ 6.7); for measuring the fallback, or a kernel where the ioctl misbehaves. |
| `USAI_WASM_EPOCH` | on | `0` disables epoch interruption — the mechanism behind the CPU slice and deadlines. **Never in production**: a runaway handler could not be stopped. Profiling only. |
| `USAI_WASM_OPT` | `speed` | Cranelift optimisation level for the core and the images: `speed`, `size`, `none`. |
| `USAI_WASM_PERFMAP` | off | `1` writes `/tmp/perf-<pid>.map` so `perf report` names the core's functions inside JIT code (Linux). |
| `USAI_WASM_CORE` | built in | Path to an alternative core (`core.wasm`) to run instead of the embedded one — how the research core and a rebuilt `guest/build.sh` output are compared. The embedded core's SHA-256 is pinned in `engine/wasm.rs`. |
| `USAI_COMPILE_CACHE` | on | `0` disables Wasmtime's on-disk compilation cache (`~/.cache/wasmtime`): for a read-only container serving a precompiled artifact there is nothing to cache, and the runtime's own `cache/image.cwasm` in the artifact already covers the start. |
| `USAI_PRECOMPILED` | on | `0` ignores the artifact's `cache/image.cwasm` and compiles the application at start — to measure the difference, or when the image was built for another CPU (`image rejected` in the log does the same automatically). **It also changes when your module-scope code runs.** With the image, module scope was evaluated once at *build* and every world resumes that snapshot, so a module-level `Date.now()` or `Math.random()` is a constant, identical on every replica. Without it, the module is evaluated once per *process*, so those values differ per instance and per restart. The runtime logs a WARN when the variable is set, because two supported configurations with two semantics for the same source is the kind of difference that reproduces in staging and not in production. Do not compute anything at module scope and neither behaviour can surprise you (`docs/GUIDE.md` → Multi-tenancy, last bullet). |
| `USAI_PROFILE` | off | `1` turns on the per-phase time ledger inside every world (`profile` in the world result, the `profile_*` tests). Allocates per request: measurements only (C13). |

## Developer tooling

| Variable | Where | Meaning |
|---|---|---|
| `USAI_BIN` | `usai/test`, the npm launcher | Path to the `usai` binary to run. The harness (`testApp`) uses it, then `usai` on PATH; `usai test` sets it to the binary that runs it. In this repository: `USAI_BIN=target/debug/usai`. |
| `USAI_CACHE_DIR` | the npm launcher (`npx usai`) | Where the launcher downloads and keeps release binaries (default `$XDG_CACHE_HOME/usai` or `~/.cache/usai`). |
| `USAI_RELEASE_BASE` | the npm launcher | Base URL for release assets (a mirror) instead of the GitHub release of the SDK's version. |
| `USAI_PARENT_PID` | set by the launcher | The pid of the process that started `usai`; when it is gone the runtime exits, so a killed wrapper (`pnpm usai dev`, an IDE task) never leaves a server on the port. Not for hand use. |
| `USAI_PROBE` | set by the launcher | Marks the launcher's own version probe so it does not recurse. Not for hand use. |

The test harness sets `USAI_CONTROL_TOKEN`, `USAI_DRAIN_GRACE=0` and
`--port 0 --control 127.0.0.1:0 --announce` on the process it spawns, so a
test run stops instantly and never collides on a port.

## Not variables

Things people look for here that live elsewhere: the listen address and
port (`--host`, `--port`; a container binds `0.0.0.0`), the artifact path
(`--artifact`), the project root (`--root`), the log format
(`--log-format json`, a global flag) and PostgreSQL's `pool.max`
(`postgres("db", { pool: { max } })` in the declaration; `usai inspect`
prints it).
