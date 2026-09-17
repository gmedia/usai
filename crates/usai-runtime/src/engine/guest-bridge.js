// Usai guest bridge — the host <-> guest ABI inside one execution world.
//
// Evaluated as a global script before the application module. The host
// installs three natives first: __usai_host_start(kind, payload) -> opId,
// __usai_host_cancel(opId), __usai_host_log(level, message). Everything the
// application can do to the outside world goes through `__usai.op`, so every
// asynchronous thing is a host-owned operation with an id in the ledger.
//
// The SDK (npm package `usai`) builds `ctx` on top of this object and exposes
// `globalThis.__usai_sdk = { invoke(app, index, inputJson) }`.
"use strict";
(function () {
  const pending = new Map();
  const cancelListeners = [];
  let cancelled = null;
  let outcome = null;

  function bridgeError(code, message) {
    const error = new Error(message);
    error.name = "UsaiBridgeError";
    error.usai = { code, status: 500 };
    return error;
  }

  function startOp(kind, payload) {
    let id = -1;
    const promise = new Promise((resolve, reject) => {
      if (cancelled !== null) {
        reject(bridgeError("cancelled", "work was cancelled: " + cancelled));
        return;
      }
      id = __usai_host_start(String(kind), payload === undefined ? "" : String(payload));
      if (typeof id !== "number" || id <= 0) {
        id = -1;
        reject(bridgeError("op_refused", "host refused operation " + kind));
        return;
      }
      pending.set(id, { resolve, reject, kind: String(kind) });
    });
    return { id, promise };
  }

  function describeError(error) {
    if (error && typeof error === "object") {
      return {
        name: String(error.name || "Error"),
        message: String(error.message || ""),
        stack: typeof error.stack === "string" ? error.stack : undefined,
        usai: error.usai && typeof error.usai === "object" ? error.usai : undefined,
      };
    }
    return { name: "Error", message: String(error) };
  }

  const bridge = {
    op(kind, payload) {
      return startOp(kind, payload).promise;
    },
    startOp,
    cancelOp(id) {
      if (pending.delete(id)) __usai_host_cancel(id);
    },
    // Called by the host. Returns true only when the completion reached a
    // live pending operation; anything else is the host's fault to account.
    complete(id, ok, payload) {
      const entry = pending.get(id);
      if (!entry) return false;
      pending.delete(id);
      if (ok) entry.resolve(payload);
      else {
        let error;
        try {
          const parsed = JSON.parse(payload);
          error = new Error(parsed.message || "operation failed");
          error.name = parsed.name || "UsaiOperationError";
          error.usai = parsed.usai || { code: parsed.code || "operation_failed", status: parsed.status || 500 };
        } catch (_) {
          error = bridgeError("operation_failed", String(payload));
        }
        entry.reject(error);
      }
      return true;
    },
    pendingCount() {
      return pending.size;
    },
    pendingKinds() {
      return Array.from(pending.values(), (p) => p.kind);
    },
    onCancel(fn) {
      if (cancelled !== null) fn(cancelled);
      else cancelListeners.push(fn);
    },
    isCancelled() {
      return cancelled !== null;
    },
    // Called by the host. Rejects every pending operation so the handler can
    // unwind; the physical operations keep their own owners on the host.
    cancel(reason) {
      if (cancelled !== null) return;
      cancelled = String(reason);
      for (const fn of cancelListeners) {
        try { fn(cancelled); } catch (_) {}
      }
      const entries = Array.from(pending.values());
      pending.clear();
      for (const entry of entries) {
        entry.reject(bridgeError("cancelled", "work was cancelled: " + cancelled));
      }
    },
    invoke(index, inputJson) {
      outcome = null;
      const sdk = globalThis.__usai_sdk;
      const app = globalThis.__usai_app;
      let promise;
      try {
        if (!sdk || typeof sdk.invoke !== "function") {
          throw bridgeError("no_sdk", "the application bundle did not register __usai_sdk");
        }
        promise = Promise.resolve(sdk.invoke(app, index, inputJson));
      } catch (error) {
        promise = Promise.reject(error);
      }
      promise.then(
        (value) => { outcome = { ok: true, value: value === undefined ? null : value }; },
        (error) => { outcome = { ok: false, error: describeError(error) }; },
      );
    },
    outcome() {
      return outcome === null ? null : JSON.stringify(outcome);
    },
  };
  Object.freeze(bridge);
  Object.defineProperty(globalThis, "__usai", { value: bridge, writable: false, configurable: false });

  // Timers are host-owned operations, so a timer that is still live when a
  // finite world reaches its terminal state is visible to the host.
  const intervals = new Map();
  globalThis.setTimeout = function (fn, ms, ...args) {
    const { id, promise } = startOp("timer", String(Math.max(0, Number(ms) | 0)));
    promise.then(() => { if (typeof fn === "function") fn(...args); }, () => {});
    return id;
  };
  globalThis.clearTimeout = function (id) {
    bridge.cancelOp(id);
  };
  globalThis.setInterval = function (fn, ms, ...args) {
    const handle = { current: -1 };
    const arm = () => {
      const { id, promise } = startOp("timer", String(Math.max(0, Number(ms) | 0)));
      handle.current = id;
      promise.then(() => {
        if (!intervals.has(handle)) return;
        try { if (typeof fn === "function") fn(...args); } finally { if (intervals.has(handle)) arm(); }
      }, () => {});
    };
    intervals.set(handle, true);
    arm();
    return handle;
  };
  globalThis.clearInterval = function (handle) {
    if (handle && intervals.delete(handle)) bridge.cancelOp(handle.current);
  };
  globalThis.queueMicrotask = function (fn) {
    Promise.resolve().then(fn);
  };

  function fmt(args) {
    return args.map((a) => (typeof a === "string" ? a : safeStringify(a))).join(" ");
  }
  function safeStringify(value) {
    try { return JSON.stringify(value); } catch (_) { return String(value); }
  }
  globalThis.console = {
    log: (...a) => __usai_host_log("info", fmt(a)),
    info: (...a) => __usai_host_log("info", fmt(a)),
    debug: (...a) => __usai_host_log("debug", fmt(a)),
    warn: (...a) => __usai_host_log("warn", fmt(a)),
    error: (...a) => __usai_host_log("error", fmt(a)),
  };
})();
