# Guest ABI

> The contract between the host runtime and the JavaScript inside one execution world.

Defined by `crates/usai-runtime/src/engine/guest-bridge.js` (the bridge) and consumed by the `usai` SDK. Application code never touches it directly. Version: bridge and SDK ship together; there is no cross-version compatibility promise before alpha.

## World start

1. The host creates a fresh engine instance (fresh globals, fresh heap).
2. The host installs three natives:
   - `__usai_host_start(kind: string, payload: string): number` — starts a host-owned operation; returns its ledger id (> 0) or `0` when refused (unknown kind, world no longer accepting ops).
   - `__usai_host_cancel(opId: number): void` — guest lost interest (e.g. `clearTimeout`). The operation's owner still reaches a terminal state; its completion becomes undeliverable.
   - `__usai_host_log(level: string, message: string): void`
3. The host evaluates the bridge as a strict global script. It defines `globalThis.__usai` (frozen) and host-owned `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval`/`queueMicrotask`/`console`.
4. The host loads the application module (ES module, from definition-lifetime bytecode) and evaluates it to its baseline. The module's default export becomes `globalThis.__usai_app`. The module must have registered `globalThis.__usai_sdk = { invoke(app, index, inputJson) }` during evaluation.

Application module top level runs once per world. It must be side-effect free beyond declarations (ADR-0009); the build phase evaluates the same module to extract the manifest.

## Operations

```js
__usai.op(kind, payload) -> Promise<string>       // resolves with the owner's JSON payload, rejects with an Error carrying `.usai = { code, status }`
__usai.startOp(kind, payload) -> { id, promise }  // same, exposing the id (timers use it)
__usai.cancelOp(id)
__usai.pendingCount() -> number
__usai.pendingKinds() -> string[]
__usai.onCancel(fn)                               // fn(reason) once the host cancels the world
__usai.isCancelled() -> boolean
```

Built-in kinds:

| kind | payload | resolves with |
|---|---|---|
| `timer` | milliseconds as decimal string | `null` |
| `resource` | `{"name","method","args"}` JSON | the manager's JSON result |

Other kinds are registered by runtime subsystems (tasks, cron, …) and documented with them.

Every operation payload crossing the boundary is a bounded, copied string. No handles, closures, or host pointers cross.

## Host → guest

Since ADR-0018 the host enters the guest by *calling* bridge functions
with one string argument and reading one string back — never by evaluating
a script per request (through 0.0.5 it evaluated
`__usai.seed(…);__usai.invoke(…)`). On the Wasm core the argument is staged
in a core-owned buffer (`qjs_usai_inbuf`), the call is
`qjs_usai_enter(name, name_len, arg, arg_len) -> (len << 32) | ptr` and the
result text is read straight from linear memory (valid until the next
result; bit 63 marks an exception's text); `qjs_usai_settle()` runs the job
queue to quiescence and returns `__usai.state()` in the same call. On the
native reference engine these are direct function calls plus the runtime's
own job loop. The functions:

```js
__usai.entry("<seed hex>␟<0|1 profiling>␟<index>␟<inputJson>")  // seeds the world and starts workload `index`; the outcome is captured, never thrown
__usai.state() -> string            // JSON: { outcome: null | {ok:true,value,profile?} | {ok:false,error:{name,message,stack?,usai?},profile?},
                                    //         pending: { count, kinds } }
__usai.complete(id, ok, payload)    // delivers one completion; returns whether a pending op accepted it
__usai.cancel(reason)               // rejects all pending ops with code "cancelled"; fires onCancel listeners
__usai.stop(reason)                 // graceful stop: the signal fires, timers resolve, other ops complete
```

For HTTP the input JSON's `request` carries `validated`: the slot names
(`params`, `query`, `headers`, `body`) the host checked against their JSON
Schema before the world existed. The SDK skips its own parse for a listed
slot only when the manifest proved the schema final (`contracts.boundaryFinal`),
and even then re-runs the library's own checks (`docs/GUIDE.md` §4).

`invoke(index, inputJson)` and `outcome()` remain as the functions `entry`
and `state` compose. The host calls these only when the guest is idle (never
re-entrantly). After every call it runs the microtask queue to quiescence.
The SDK stamps the ABI it speaks into the manifest (`builtWith.abi`) — the
SDK↔bridge surface (`__usai_sdk.*`, the `__usai` natives), which ADR-0018
did not change; a runtime whose bridge speaks another refuses the artifact
at install. How the host enters the bridge (`qjs_usai_call` vs `qjs_eval`)
is the runtime's own business and needs no artifact change.

## Terminal state and detached work

The world's work is terminal when `state().outcome` is non-null — every
call into the guest ends with a settle whose state the world keeps, so the
driver's read costs nothing until the next call. For
finite workloads the same read's `pending.count` decides: anything still
pending is **detached work** (contract C3): it is reported as a lifecycle
violation, cancelled, and the world ends anyway.

## Errors

An error thrown by application code with a `usai: { code, status, details? }` property is an application error contract; the runtime maps it to a stable transport response. Any other error is an unexpected failure: sanitized at the boundary, fully logged.


## Substrates

The same ABI is served by two engines. **Wasm (default, ADR-0016):** the core installs `__usai_test_op(kind, payload) -> Promise`; the bridge routes every operation through it as `kind\0payload`, mirrors the core's sequential op ids so timers can be cancelled, and the host answers the core's `usai_op_start` import. Control operations `__cancel\0<id>` and `__log\0<level>\0<message>` are refused by the host so the core drops their promises. Cancel/stop are driven by the host completing outstanding core ops (status 2 / status 0). **Native QuickJS (ADR-0015):** the host installs `__usai_host_start/cancel/log` and the bridge keeps its own pending map.

**Native codecs.** The Wasm core installs `globalThis.__usai_native` at `qjs_init` — `utf8dec(Uint8Array) -> string`, `utf8enc(string) -> Uint8Array`, `b64dec(string) -> Uint8Array`, `b64enc(Uint8Array) -> string`, in C (`patches/usai-native-codecs.patch`). The bridge's `TextEncoder`/`TextDecoder` and the SDK's `bytes` helpers use them when present and run a block-built JavaScript path otherwise (the reference engine, Node); `atob`/`btoa` are QuickJS-ng intrinsics the bridge never shadows. Measured: a 3 MB body decoded plus a 1 MB base64 round trip went from a CPU-slice fault (>5 s) to 0.2 s. Not part of `GUEST_ABI` (additive; a core without them still runs every bundle).
