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

```js
__usai.invoke(index, inputJson)     // starts workload `index`; the outcome is captured, never thrown
__usai.complete(id, ok, payload)    // delivers one completion; returns whether a pending op accepted it
__usai.cancel(reason)               // rejects all pending ops with code "cancelled"; fires onCancel listeners
__usai.outcome() -> string | null   // JSON: {ok:true,value} | {ok:false,error:{name,message,stack?,usai?}}
```

The host calls these only when the guest is idle (never re-entrantly). After every call it runs the microtask queue to quiescence.

## Terminal state and detached work

The world's work is terminal when `outcome()` is non-null. For finite workloads the host then reads `pendingCount()`; anything still pending is **detached work** (contract C3): it is reported as a lifecycle violation, cancelled, and the world ends anyway.

## Errors

An error thrown by application code with a `usai: { code, status, details? }` property is an application error contract; the runtime maps it to a stable transport response. Any other error is an unexpected failure: sanitized at the boundary, fully logged.


## Substrates

The same ABI is served by two engines. **Wasm (default, ADR-0016):** the core installs `__usai_test_op(kind, payload) -> Promise`; the bridge routes every operation through it as `kind\0payload`, mirrors the core's sequential op ids so timers can be cancelled, and the host answers the core's `usai_op_start` import. Control operations `__cancel\0<id>` and `__log\0<level>\0<message>` are refused by the host so the core drops their promises. Cancel/stop are driven by the host completing outstanding core ops (status 2 / status 0). **Native QuickJS (ADR-0015):** the host installs `__usai_host_start/cancel/log` and the bridge keeps its own pending map.
