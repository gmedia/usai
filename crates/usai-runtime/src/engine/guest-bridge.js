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
  let stopping = null;
  let outcome = null;
  // On the Wasm substrate the core installs `__usai_test_op` (C): it owns the
  // pending promises and allocates ids sequentially from 1; the bridge mirrors
  // the counter so timers can be cancelled by id. On the native substrate the
  // host installs `__usai_host_start` instead.
  const nativeOp = typeof globalThis.__usai_test_op === "function" ? globalThis.__usai_test_op : null;
  let nextNativeId = 1;
  let nextHoldId = 0;
  let entropySeed = null;
  let entropyCounter = 0;
  let profiling = false;
  let entryMs = 0;
  const clock = () => (typeof performance !== "undefined" ? performance.now() : Date.now());

  function bridgeError(code, message) {
    const error = new Error(message);
    error.name = "UsaiBridgeError";
    error.usai = { code, status: 500 };
    return error;
  }

  function errorFromPayload(payload) {
    try {
      const parsed = JSON.parse(payload);
      const error = new Error(parsed.message || "operation failed");
      error.name = parsed.name || "UsaiOperationError";
      error.usai = parsed.usai || { code: parsed.code || "operation_failed", status: parsed.status || 500 };
      return error;
    } catch (_) {
      return bridgeError("operation_failed", String(payload));
    }
  }

  function startOp(kind, payload) {
    const text = payload === undefined ? "" : String(payload);
    if (nativeOp) {
      if (cancelled !== null) {
        return { id: -1, promise: Promise.reject(bridgeError("cancelled", "work was cancelled: " + cancelled)) };
      }
      const id = nextNativeId++;
      pending.set(id, { kind: String(kind), native: true });
      const promise = nativeOp(0, String(kind) + "\u0000" + text).then(
        (value) => { pending.delete(id); return value; },
        (error) => {
          pending.delete(id);
          if (error && error.code === "EXP011C_CANCELLED") throw bridgeError("cancelled", String(error.message));
          if (error && error.code === "EXP011C_START_REJECTED") throw bridgeError("op_refused", "host refused operation " + kind);
          if (error && error.code === "EXP011C_HOST_ERROR") throw errorFromPayload(error.message);
          throw error;
        },
      );
      return { id, promise };
    }
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
      if (!pending.has(id)) return;
      if (nativeOp) {
        // A control operation the host answers synchronously (refused, so
        // the core drops its promise); the pending entry clears on rejection.
        nextNativeId++;
        nativeOp(0, "__cancel\u0000" + id).catch(() => {});
        return;
      }
      pending.delete(id);
      __usai_host_cancel(id);
    },
    // Called by the host. Returns true only when the completion reached a
    // live pending operation; anything else is the host's fault to account.
    complete(id, ok, payload) {
      const entry = pending.get(id);
      if (!entry) return false;
      pending.delete(id);
      if (ok) entry.resolve(payload);
      else entry.reject(errorFromPayload(payload));
      return true;
    },
    // A synthetic pending entry for work the guest holds open across several
    // operations (an open database transaction): it counts as live
    // asynchronous work until released, so a finite world that ends without
    // closing it is diagnosed like a live timer would be.
    hold(kind) {
      const id = -(++nextHoldId);
      pending.set(id, { kind: String(kind), hold: true });
      return { release() { pending.delete(id); } };
    },
    // Per-world entropy from the host (32 bytes, hex), installed with the
    // invocation; `crypto` draws from it. Never reused across worlds: the
    // image is a snapshot, so the seed must arrive after the world exists.
    seed(hex, profile) {
      entropySeed = hex;
      entropyCounter = 0;
      profiling = profile === true;
    },
    // The SDK's phase ledger (dispatch, validation, handler, response) for
    // the current invocation, in ms; empty unless the world was seeded with
    // profiling on. Read by the host with the outcome.
    profiling() {
      return profiling;
    },
    pendingCount() {
      return pending.size;
    },
    pendingKinds() {
      return Array.from(pending.values(), (p) => p.kind);
    },
    onCancel(fn) {
      if (cancelled !== null) fn(cancelled);
      else if (stopping !== null) fn(stopping);
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
      // Native core: the host rejects outstanding operations itself.
      if (nativeOp) return;
      const entries = Array.from(pending.values());
      pending.clear();
      for (const entry of entries) {
        if (typeof entry.reject === "function") entry.reject(bridgeError("cancelled", "work was cancelled: " + cancelled));
      }
    },
    // Graceful stop (persistent workloads): the signal fires so loops can
    // exit, pending timers resolve now so `await ctx.sleep()` returns, and
    // other operations keep their owners until they complete.
    stop(reason) {
      if (cancelled !== null) return;
      const listeners = cancelListeners.slice();
      cancelListeners.length = 0;
      stopping = String(reason);
      for (const fn of listeners) {
        try { fn(stopping); } catch (_) {}
      }
      // Native core: the host resolves outstanding timers itself.
      if (nativeOp) return;
      for (const [id, entry] of Array.from(pending.entries())) {
        if (entry.kind === "timer") {
          pending.delete(id);
          __usai_host_cancel(id);
          entry.resolve("");
        }
      }
    },
    isStopping() {
      return stopping !== null;
    },
    invoke(index, inputJson) {
      outcome = null;
      const sdk = globalThis.__usai_sdk;
      const ns = globalThis.__usai_app_ns;
      const app = globalThis.__usai_app !== undefined ? globalThis.__usai_app : ns && ns.default;
      let promise;
      try {
        if (!sdk || typeof sdk.invoke !== "function") {
          throw bridgeError("no_sdk", "the application bundle did not register __usai_sdk");
        }
        promise = Promise.resolve(sdk.invoke(app, index, inputJson));
      } catch (error) {
        promise = Promise.reject(error);
      }
      // The bridge's own mark joins the SDK's ledger: `bridge.entry` is the
      // synchronous part of the call (dispatch until the first await).
      const ledger = () => {
        if (!profiling) return [];
        const out = sdk && typeof sdk.takeProfile === "function" ? sdk.takeProfile() : [];
        out.push(["bridge.entry", entryMs]);
        return out;
      };
      promise.then(
        (value) => { outcome = { ok: true, value: value === undefined ? null : value, profile: ledger() }; },
        (error) => { outcome = { ok: false, error: describeError(error), profile: ledger() }; },
      );
    },
    outcome() {
      return outcome === null ? null : JSON.stringify(outcome);
    },
    // ---- direct-call ABI (ADR-0018): one string in, one string out ----
    // `entry`: seed ␟ profiling ␟ index ␟ input JSON — seeds the world and
    // starts the handler in one guest call instead of an evaluated snippet.
    entry(payload) {
      const first = payload.indexOf("\u001f");
      const second = payload.indexOf("\u001f", first + 1);
      const third = payload.indexOf("\u001f", second + 1);
      bridge.seed(payload.slice(0, first), payload.slice(first + 1, second) === "1");
      const t = profiling ? clock() : 0;
      bridge.invoke(Number(payload.slice(second + 1, third)), payload.slice(third + 1));
      if (profiling) entryMs = clock() - t;
      return "";
    },
    // `state`: the outcome (null until the handler settles) and the
    // pending-work count in one read, for the driver's loop.
    state() {
      return JSON.stringify({ outcome, pending: { count: pending.size, kinds: Array.from(pending.values(), (p) => p.kind) } });
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

  // Web-standard encoding primitives (ADR-0013). QuickJS-ng ships `atob`/
  // `btoa` natively (never shadow them); UTF-8 goes through the core's native
  // codecs (`__usai_native`, C in the core) when the core has them, and
  // through JavaScript otherwise — built in blocks, since QuickJS has no
  // ropes and `out += ch` per character is quadratic.
  const native = globalThis.__usai_native;
  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const BLOCK = 8192;
  const fromUnits = (units, parts) => {
    if (units.length) { parts.push(String.fromCharCode.apply(null, units)); units.length = 0; }
  };
  if (typeof globalThis.btoa !== "function") {
    globalThis.btoa = function (data) {
      const s = String(data);
      const parts = [], units = [];
      for (let i = 0; i < s.length; i += 3) {
        const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
        if (a > 255 || b > 255 || c > 255) throw new Error("btoa: character out of Latin1 range");
        const n = (a << 16) | ((b || 0) << 8) | (c || 0);
        units.push(B64.charCodeAt((n >> 18) & 63), B64.charCodeAt((n >> 12) & 63), i + 1 < s.length ? B64.charCodeAt((n >> 6) & 63) : 61, i + 2 < s.length ? B64.charCodeAt(n & 63) : 61);
        if (units.length >= BLOCK) fromUnits(units, parts);
      }
      fromUnits(units, parts);
      return parts.join("");
    };
  }
  if (typeof globalThis.atob !== "function") {
    const B64V = new Int16Array(128).fill(-1);
    for (let i = 0; i < 64; i++) B64V[B64.charCodeAt(i)] = i;
    globalThis.atob = function (data) {
      const s = String(data).replace(/[\s=]+$/g, "").replace(/\s+/g, "");
      const parts = [], units = [];
      let bits = 0, acc = 0;
      for (let i = 0; i < s.length; i++) {
        const code = s.charCodeAt(i);
        const v = code < 128 ? B64V[code] : -1;
        if (v < 0) throw new Error("atob: invalid character");
        acc = ((acc << 6) | v) & 0xffffff; bits += 6;
        if (bits >= 8) { bits -= 8; units.push((acc >> bits) & 255); if (units.length >= BLOCK) fromUnits(units, parts); }
      }
      fromUnits(units, parts);
      return parts.join("");
    };
  }
  globalThis.TextEncoder = class TextEncoder {
    encode(input) {
      const s = input === undefined ? "" : String(input);
      if (native) return native.utf8enc(s);
      const bytes = [];
      for (let i = 0; i < s.length; i++) {
        let c = s.codePointAt(i);
        if (c > 0xffff) i++;
        if (c < 0x80) bytes.push(c);
        else if (c < 0x800) bytes.push(0xc0 | (c >> 6), 0x80 | (c & 63));
        else if (c < 0x10000) bytes.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
        else bytes.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      }
      return Uint8Array.from(bytes);
    }
  };
  globalThis.TextDecoder = class TextDecoder {
    constructor(label) { this.encoding = (label || "utf-8").toLowerCase(); }
    decode(input) {
      if (input === undefined) return "";
      const b = input instanceof Uint8Array ? input : new Uint8Array(input.buffer || input);
      if (native) return native.utf8dec(b);
      const parts = [], units = [];
      for (let i = 0; i < b.length;) {
        const x = b[i];
        let cp, n;
        if (x < 0x80) { cp = x; n = 1; }
        else if ((x & 0xe0) === 0xc0) { cp = x & 31; n = 2; }
        else if ((x & 0xf0) === 0xe0) { cp = x & 15; n = 3; }
        else { cp = x & 7; n = 4; }
        for (let j = 1; j < n; j++) cp = (cp << 6) | (b[i + j] & 63);
        if (cp > 0xffff) { cp -= 0x10000; units.push(0xd800 + (cp >> 10), 0xdc00 + (cp & 0x3ff)); }
        else units.push(cp);
        if (units.length >= BLOCK) fromUnits(units, parts);
        i += n;
      }
      fromUnits(units, parts);
      return parts.join("");
    }
  };

  // ---- crypto: a WebCrypto subset with a clear lifetime story ------------
  // Randomness comes from host entropy installed per world (`__usai.seed`),
  // expanded with SHA-256 in counter mode; digests and HMAC are pure. This is
  // the whole surface: SHA-256/384/512 digests, HMAC sign/verify, random
  // bytes and UUIDs. Password hashing is a host operation (`password` in the
  // SDK) because it is deliberately expensive.
  const K256 = [0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2];
  function sha256(bytes) {
    const h = new Int32Array([0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19]);
    const len = bytes.length, bitLen = len * 8;
    const padded = new Uint8Array(((len + 9 + 63) >> 6) << 6);
    padded.set(bytes); padded[len] = 0x80;
    const dv = new DataView(padded.buffer);
    dv.setUint32(padded.length - 4, bitLen >>> 0); dv.setUint32(padded.length - 8, Math.floor(bitLen / 0x100000000));
    const w = new Int32Array(64);
    for (let off = 0; off < padded.length; off += 64) {
      for (let i = 0; i < 16; i++) w[i] = dv.getInt32(off + i * 4);
      for (let i = 16; i < 64; i++) {
        const a = w[i - 15], b = w[i - 2];
        const s0 = ((a >>> 7) | (a << 25)) ^ ((a >>> 18) | (a << 14)) ^ (a >>> 3);
        const s1 = ((b >>> 17) | (b << 15)) ^ ((b >>> 19) | (b << 13)) ^ (b >>> 10);
        w[i] = (w[i - 16] + s0 + w[i - 7] + s1) | 0;
      }
      let [a, b, c, d, e, f, g, hh] = h;
      for (let i = 0; i < 64; i++) {
        const S1 = ((e >>> 6) | (e << 26)) ^ ((e >>> 11) | (e << 21)) ^ ((e >>> 25) | (e << 7));
        const ch = (e & f) ^ (~e & g);
        const t1 = (hh + S1 + ch + K256[i] + w[i]) | 0;
        const S0 = ((a >>> 2) | (a << 30)) ^ ((a >>> 13) | (a << 19)) ^ ((a >>> 22) | (a << 10));
        const maj = (a & b) ^ (a & c) ^ (b & c);
        const t2 = (S0 + maj) | 0;
        hh = g; g = f; f = e; e = (d + t1) | 0; d = c; c = b; b = a; a = (t1 + t2) | 0;
      }
      h[0] = (h[0] + a) | 0; h[1] = (h[1] + b) | 0; h[2] = (h[2] + c) | 0; h[3] = (h[3] + d) | 0;
      h[4] = (h[4] + e) | 0; h[5] = (h[5] + f) | 0; h[6] = (h[6] + g) | 0; h[7] = (h[7] + hh) | 0;
    }
    const out = new Uint8Array(32);
    const odv = new DataView(out.buffer);
    for (let i = 0; i < 8; i++) odv.setInt32(i * 4, h[i]);
    return out;
  }
  // SHA-512 core shared by SHA-384 (truncated, different IV); 64-bit words as pairs.
  const K512 = ["428a2f98d728ae22","7137449123ef65cd","b5c0fbcfec4d3b2f","e9b5dba58189dbbc","3956c25bf348b538","59f111f1b605d019","923f82a4af194f9b","ab1c5ed5da6d8118","d807aa98a3030242","12835b0145706fbe","243185be4ee4b28c","550c7dc3d5ffb4e2","72be5d74f27b896f","80deb1fe3b1696b1","9bdc06a725c71235","c19bf174cf692694","e49b69c19ef14ad2","efbe4786384f25e3","0fc19dc68b8cd5b5","240ca1cc77ac9c65","2de92c6f592b0275","4a7484aa6ea6e483","5cb0a9dcbd41fbd4","76f988da831153b5","983e5152ee66dfab","a831c66d2db43210","b00327c898fb213f","bf597fc7beef0ee4","c6e00bf33da88fc2","d5a79147930aa725","06ca6351e003826f","142929670a0e6e70","27b70a8546d22ffc","2e1b21385c26c926","4d2c6dfc5ac42aed","53380d139d95b3df","650a73548baf63de","766a0abb3c77b2a8","81c2c92e47edaee6","92722c851482353b","a2bfe8a14cf10364","a81a664bbc423001","c24b8b70d0f89791","c76c51a30654be30","d192e819d6ef5218","d69906245565a910","f40e35855771202a","106aa07032bbd1b8","19a4c116b8d2d0c8","1e376c085141ab53","2748774cdf8eeb99","34b0bcb5e19b48a8","391c0cb3c5c95a63","4ed8aa4ae3418acb","5b9cca4f7763e373","682e6ff3d6b2b8a3","748f82ee5defb2fc","78a5636f43172f60","84c87814a1f0ab72","8cc702081a6439ec","90befffa23631e28","a4506cebde82bde9","bef9a3f7b2c67915","c67178f2e372532b","ca273eceea26619c","d186b8c721c0c207","eada7dd6cde0eb1e","f57d4f7fee6ed178","06f067aa72176fba","0a637dc5a2c898a6","113f9804bef90dae","1b710b35131c471b","28db77f523047d84","32caab7b40c72493","3c9ebe0a15c9bebc","431d67c49c100d4c","4cc5d4becb3e42b6","597f299cfc657e2a","5fcb6fab3ad6faec","6c44198c4a475817"];
  const KH = new Int32Array(80), KL = new Int32Array(80);
  for (let i = 0; i < 80; i++) { KH[i] = parseInt(K512[i].slice(0, 8), 16) | 0; KL[i] = parseInt(K512[i].slice(8), 16) | 0; }
  function sha512core(bytes, iv, outWords) {
    const H = new Int32Array(iv);
    const len = bytes.length;
    const padded = new Uint8Array(((len + 17 + 127) >> 7) << 7);
    padded.set(bytes); padded[len] = 0x80;
    const dv = new DataView(padded.buffer);
    const bitLen = len * 8;
    dv.setUint32(padded.length - 4, bitLen >>> 0); dv.setUint32(padded.length - 8, Math.floor(bitLen / 0x100000000));
    const WH = new Int32Array(80), WL = new Int32Array(80);
    for (let off = 0; off < padded.length; off += 128) {
      for (let i = 0; i < 16; i++) { WH[i] = dv.getInt32(off + i * 8); WL[i] = dv.getInt32(off + i * 8 + 4); }
      for (let i = 16; i < 80; i++) {
        let xh = WH[i - 15], xl = WL[i - 15];
        const s0h = ((xh >>> 1) | (xl << 31)) ^ ((xh >>> 8) | (xl << 24)) ^ (xh >>> 7);
        const s0l = ((xl >>> 1) | (xh << 31)) ^ ((xl >>> 8) | (xh << 24)) ^ ((xl >>> 7) | (xh << 25));
        xh = WH[i - 2]; xl = WL[i - 2];
        const s1h = ((xh >>> 19) | (xl << 13)) ^ ((xl >>> 29) | (xh << 3)) ^ (xh >>> 6);
        const s1l = ((xl >>> 19) | (xh << 13)) ^ ((xh >>> 29) | (xl << 3)) ^ ((xl >>> 6) | (xh << 26));
        let lo = (WL[i - 16] >>> 0) + (s0l >>> 0) + (WL[i - 7] >>> 0) + (s1l >>> 0);
        WH[i] = (WH[i - 16] + s0h + WH[i - 7] + s1h + Math.floor(lo / 0x100000000)) | 0;
        WL[i] = lo | 0;
      }
      let ah = H[0], al = H[1], bh = H[2], bl = H[3], ch = H[4], cl = H[5], dh = H[6], dl = H[7];
      let eh = H[8], el = H[9], fh = H[10], fl = H[11], gh = H[12], gl = H[13], hh = H[14], hl = H[15];
      for (let i = 0; i < 80; i++) {
        const S1h = ((eh >>> 14) | (el << 18)) ^ ((eh >>> 18) | (el << 14)) ^ ((el >>> 9) | (eh << 23));
        const S1l = ((el >>> 14) | (eh << 18)) ^ ((el >>> 18) | (eh << 14)) ^ ((eh >>> 9) | (el << 23));
        const chh = (eh & fh) ^ (~eh & gh), chl = (el & fl) ^ (~el & gl);
        let lo = (hl >>> 0) + (S1l >>> 0) + (chl >>> 0) + (KL[i] >>> 0) + (WL[i] >>> 0);
        const t1h = (hh + S1h + chh + KH[i] + WH[i] + Math.floor(lo / 0x100000000)) | 0, t1l = lo | 0;
        const S0h = ((ah >>> 28) | (al << 4)) ^ ((al >>> 2) | (ah << 30)) ^ ((al >>> 7) | (ah << 25));
        const S0l = ((al >>> 28) | (ah << 4)) ^ ((ah >>> 2) | (al << 30)) ^ ((ah >>> 7) | (al << 25));
        const majh = (ah & bh) ^ (ah & ch) ^ (bh & ch), majl = (al & bl) ^ (al & cl) ^ (bl & cl);
        lo = (S0l >>> 0) + (majl >>> 0);
        const t2h = (S0h + majh + Math.floor(lo / 0x100000000)) | 0, t2l = lo | 0;
        hh = gh; hl = gl; gh = fh; gl = fl; fh = eh; fl = el;
        lo = (dl >>> 0) + (t1l >>> 0); eh = (dh + t1h + Math.floor(lo / 0x100000000)) | 0; el = lo | 0;
        dh = ch; dl = cl; ch = bh; cl = bl; bh = ah; bl = al;
        lo = (t1l >>> 0) + (t2l >>> 0); ah = (t1h + t2h + Math.floor(lo / 0x100000000)) | 0; al = lo | 0;
      }
      const add = (i, xh, xl) => { const lo = (H[i + 1] >>> 0) + (xl >>> 0); H[i] = (H[i] + xh + Math.floor(lo / 0x100000000)) | 0; H[i + 1] = lo | 0; };
      add(0, ah, al); add(2, bh, bl); add(4, ch, cl); add(6, dh, dl); add(8, eh, el); add(10, fh, fl); add(12, gh, gl); add(14, hh, hl);
    }
    const out = new Uint8Array(outWords * 4);
    const odv = new DataView(out.buffer);
    for (let i = 0; i < outWords; i++) odv.setInt32(i * 4, H[i]);
    return out;
  }
  const IV512 = [0x6a09e667,0xf3bcc908,0xbb67ae85,0x84caa73b,0x3c6ef372,0xfe94f82b,0xa54ff53a,0x5f1d36f1,0x510e527f,0xade682d1,0x9b05688c,0x2b3e6c1f,0x1f83d9ab,0xfb41bd6b,0x5be0cd19,0x137e2179];
  const IV384 = [0xcbbb9d5d,0xc1059ed8,0x629a292a,0x367cd507,0x9159015a,0x3070dd17,0x152fecd8,0xf70e5939,0x67332667,0xffc00b31,0x8eb44a87,0x68581511,0xdb0c2e0d,0x64f98fa7,0x47b5481d,0xbefa4fa4];
  const DIGESTS = {
    "SHA-256": { fn: sha256, block: 64 },
    "SHA-384": { fn: (b) => sha512core(b, IV384, 12), block: 128 },
    "SHA-512": { fn: (b) => sha512core(b, IV512, 16), block: 128 },
  };
  function toBytes(data) {
    if (data instanceof Uint8Array) return data;
    if (data instanceof ArrayBuffer) return new Uint8Array(data);
    if (ArrayBuffer.isView(data)) return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    throw new TypeError("expected an ArrayBuffer or ArrayBufferView");
  }
  function digestName(algorithm) {
    const name = typeof algorithm === "string" ? algorithm : algorithm && algorithm.name;
    const key = String(name || "").toUpperCase();
    if (!DIGESTS[key]) throw bridgeError("unsupported_algorithm", "crypto.subtle: unsupported digest " + JSON.stringify(name) + " (SHA-256, SHA-384, SHA-512)");
    return key;
  }
  function hmac(hashName, key, data) {
    const { fn, block } = DIGESTS[hashName];
    let k = key.length > block ? fn(key) : key;
    const ipad = new Uint8Array(block), opad = new Uint8Array(block);
    for (let i = 0; i < block; i++) { const b = i < k.length ? k[i] : 0; ipad[i] = b ^ 0x36; opad[i] = b ^ 0x5c; }
    const inner = new Uint8Array(block + data.length); inner.set(ipad); inner.set(data, block);
    const ih = fn(inner);
    const outer = new Uint8Array(block + ih.length); outer.set(opad); outer.set(ih, block);
    return fn(outer);
  }
  function randomBytes(n) {
    if (entropySeed === null) throw bridgeError("no_entropy", "crypto: this world received no entropy from the host");
    const out = new Uint8Array(n);
    const seed = new Uint8Array(entropySeed.length / 2);
    for (let i = 0; i < seed.length; i++) seed[i] = parseInt(entropySeed.substr(i * 2, 2), 16);
    const input = new Uint8Array(seed.length + 8);
    input.set(seed);
    let filled = 0;
    while (filled < n) {
      const c = ++entropyCounter;
      input[seed.length] = c & 255; input[seed.length + 1] = (c >>> 8) & 255; input[seed.length + 2] = (c >>> 16) & 255; input[seed.length + 3] = (c >>> 24) & 255;
      const block = sha256(input);
      const take = Math.min(32, n - filled);
      out.set(block.subarray(0, take), filled);
      filled += take;
    }
    return out;
  }
  const HmacKey = class CryptoKey {
    #raw; #hash;
    constructor(raw, hash) { this.#raw = raw; this.#hash = hash; }
    get type() { return "secret"; }
    get algorithm() { return { name: "HMAC", hash: { name: this.#hash } }; }
    get extractable() { return false; }
    get usages() { return ["sign", "verify"]; }
    _sign(data) { return hmac(this.#hash, this.#raw, data); }
  };
  const subtle = {
    async digest(algorithm, data) {
      return DIGESTS[digestName(algorithm)].fn(toBytes(data)).buffer;
    },
    async importKey(format, keyData, algorithm, _extractable, _usages) {
      if (format !== "raw") throw bridgeError("unsupported_algorithm", "crypto.subtle.importKey: only raw keys are supported");
      const name = algorithm && String(algorithm.name || "").toUpperCase();
      if (name !== "HMAC") throw bridgeError("unsupported_algorithm", "crypto.subtle.importKey: only HMAC keys are supported");
      return new HmacKey(toBytes(keyData).slice(), digestName(algorithm.hash || "SHA-256"));
    },
    async sign(algorithm, key, data) {
      if (!(key instanceof HmacKey)) throw bridgeError("unsupported_algorithm", "crypto.subtle.sign: only HMAC is supported");
      return key._sign(toBytes(data)).buffer;
    },
    async verify(algorithm, key, signature, data) {
      if (!(key instanceof HmacKey)) throw bridgeError("unsupported_algorithm", "crypto.subtle.verify: only HMAC is supported");
      const a = key._sign(toBytes(data)), b = toBytes(signature);
      if (a.length !== b.length) return false;
      let diff = 0;
      for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
      return diff === 0;
    },
  };
  const cryptoObject = {
    getRandomValues(array) {
      if (!ArrayBuffer.isView(array) || array instanceof Float32Array || array instanceof Float64Array || array instanceof DataView) throw new TypeError("getRandomValues: expected an integer typed array");
      if (array.byteLength > 65536) throw bridgeError("quota_exceeded", "getRandomValues: at most 65536 bytes per call");
      new Uint8Array(array.buffer, array.byteOffset, array.byteLength).set(randomBytes(array.byteLength));
      return array;
    },
    randomUUID() {
      const b = randomBytes(16);
      b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80;
      const h = Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
      return h.slice(0, 8) + "-" + h.slice(8, 12) + "-" + h.slice(12, 16) + "-" + h.slice(16, 20) + "-" + h.slice(20);
    },
    subtle,
  };
  Object.defineProperty(globalThis, "crypto", { value: Object.freeze(cryptoObject), writable: false, configurable: false });

  // `fetch` is not a global here: outbound HTTP is an external operation that
  // needs an owner and a declared destination. The name exists only to say
  // so, instead of `ReferenceError: fetch is not defined`.
  globalThis.fetch = function () {
    return Promise.reject(bridgeError(
      "fetch_not_available",
      "fetch() is not available inside a world: outbound HTTP is a declared resource. Declare `const api = httpClient(\"api\", { baseUrl: \"https://…\" })`, add it to the workload's `resources: [api]`, and call `ctx.resources.api.fetch(path, init)`.",
    ));
  };

  // ---- Web platform globals a backend handler reasonably expects ----------
  // Kept small and dependency-free; this file is evaluated into the image.
  // `URL`/`URLSearchParams` cover the WHATWG behaviour validators and
  // handlers use (parse, components, query manipulation, serialization);
  // `structuredClone` deep-copies plain data, Dates, Maps, Sets, arrays and
  // typed arrays. `crypto` and the `fetch` refusal are defined above.
  const SPECIAL_PORTS = { "http:": "80", "https:": "443", "ws:": "80", "wss:": "443", "ftp:": "21", "file:": "" };
  const encodeQuery = (s) => encodeURIComponent(s).replace(/%20/g, "+").replace(/[!'()~]/g, (c) => "%" + c.charCodeAt(0).toString(16).toUpperCase());
  const decodeQuery = (s) => { try { return decodeURIComponent(s.replace(/\+/g, " ")); } catch (_) { return s; } };

  class URLSearchParams {
    #list = [];
    #owner = null;
    constructor(init = "") {
      if (typeof init === "string") {
        for (const part of init.replace(/^\?/, "").split("&")) {
          if (!part) continue;
          const i = part.indexOf("=");
          this.#list.push(i < 0 ? [decodeQuery(part), ""] : [decodeQuery(part.slice(0, i)), decodeQuery(part.slice(i + 1))]);
        }
      } else if (init instanceof URLSearchParams) {
        this.#list = init.#list.map((e) => e.slice());
      } else if (init && typeof init === "object") {
        const entries = typeof init[Symbol.iterator] === "function" ? Array.from(init) : Object.entries(init);
        for (const [k, v] of entries) this.#list.push([String(k), String(v)]);
      }
    }
    static _attach(params, owner) { params.#owner = owner; return params; }
    #changed() { if (this.#owner) this.#owner._setSearchFromParams(this.toString()); }
    append(k, v) { this.#list.push([String(k), String(v)]); this.#changed(); }
    delete(k, v) { this.#list = this.#list.filter(([a, b]) => !(a === String(k) && (v === undefined || b === String(v)))); this.#changed(); }
    get(k) { const e = this.#list.find(([a]) => a === String(k)); return e ? e[1] : null; }
    getAll(k) { return this.#list.filter(([a]) => a === String(k)).map(([, b]) => b); }
    has(k, v) { return this.#list.some(([a, b]) => a === String(k) && (v === undefined || b === String(v))); }
    set(k, v) {
      k = String(k); v = String(v);
      const i = this.#list.findIndex(([a]) => a === k);
      if (i < 0) this.#list.push([k, v]);
      else { this.#list[i][1] = v; this.#list = this.#list.filter(([a], j) => a !== k || j === i); }
      this.#changed();
    }
    sort() { this.#list.sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)); this.#changed(); }
    forEach(fn, thisArg) { for (const [k, v] of this.#list) fn.call(thisArg, v, k, this); }
    keys() { return this.#list.map(([k]) => k)[Symbol.iterator](); }
    values() { return this.#list.map(([, v]) => v)[Symbol.iterator](); }
    entries() { return this.#list.map((e) => e.slice())[Symbol.iterator](); }
    [Symbol.iterator]() { return this.entries(); }
    get size() { return this.#list.length; }
    toString() { return this.#list.map(([k, v]) => encodeQuery(k) + "=" + encodeQuery(v)).join("&"); }
  }

  class URL {
    #p;
    static canParse(url, base) { try { new URL(url, base); return true; } catch (_) { return false; } }
    static parse(url, base) { try { return new URL(url, base); } catch (_) { return null; } }
    constructor(url, base) {
      url = String(url).trim();
      let m = /^([a-zA-Z][a-zA-Z0-9+.-]*):(.*)$/s.exec(url);
      if (!m) {
        if (base === undefined) throw new TypeError("Invalid URL: " + url);
        const b = base instanceof URL ? base : new URL(String(base));
        this.#p = b.#resolve(url);
        return;
      }
      this.#p = URL.#parseAbsolute(m[1].toLowerCase() + ":", m[2]);
    }
    static #parseAbsolute(protocol, rest) {
      const p = { protocol, username: "", password: "", hostname: "", port: "", pathname: "", search: "", hash: "" };
      const special = protocol in SPECIAL_PORTS;
      let h = rest.indexOf("#");
      if (h >= 0) { p.hash = "#" + rest.slice(h + 1); rest = rest.slice(0, h); }
      let q = rest.indexOf("?");
      if (q >= 0) { p.search = rest.length - q > 1 ? "?" + rest.slice(q + 1) : ""; rest = rest.slice(0, q); }
      if (special) rest = rest.replace(/\\/g, "/");
      if (rest.startsWith("//") || (special && protocol !== "file:")) {
        // `file:///path` has an empty host and the path keeps its slash.
        rest = protocol === "file:" ? rest.slice(2) : rest.replace(/^\/+/, "");
        const slash = rest.indexOf("/");
        let authority = slash < 0 ? rest : rest.slice(0, slash);
        p.pathname = slash < 0 ? "" : rest.slice(slash);
        const at = authority.lastIndexOf("@");
        if (at >= 0) {
          const cred = authority.slice(0, at);
          authority = authority.slice(at + 1);
          const colon = cred.indexOf(":");
          p.username = colon < 0 ? cred : cred.slice(0, colon);
          p.password = colon < 0 ? "" : cred.slice(colon + 1);
        }
        const portMatch = /^(\[[^\]]*\]|[^:]*)(?::(\d*))?$/.exec(authority);
        if (!portMatch) throw new TypeError("Invalid URL: bad host");
        p.hostname = portMatch[1].toLowerCase();
        p.port = portMatch[2] ?? "";
        if (p.port !== "" && (!/^\d+$/.test(p.port) || Number(p.port) > 65535)) throw new TypeError("Invalid URL: bad port");
        if (p.port === SPECIAL_PORTS[protocol]) p.port = "";
        if (special && protocol !== "file:") {
          if (p.hostname === "" || /[\s<>\^`{|}]/.test(p.hostname)) throw new TypeError("Invalid URL: missing host");
          if (p.pathname === "") p.pathname = "/";
        }
        if (special) p.pathname = URL.#normalizePath(p.pathname);
      } else {
        p.pathname = rest;
      }
      return p;
    }
    static #normalizePath(path) {
      const out = [];
      const segs = path.split("/");
      for (let i = 1; i < segs.length; i++) {
        const s = segs[i];
        if (s === "." || s === "%2e" || s === "%2E") { if (i === segs.length - 1) out.push(""); continue; }
        if (s === ".." || /^(\.|%2e){2}$/i.test(s)) { out.pop(); if (i === segs.length - 1) out.push(""); continue; }
        out.push(s);
      }
      return "/" + out.join("/");
    }
    #resolve(relative) {
      const b = this.#p;
      if (relative.startsWith("//")) return URL.#parseAbsolute(b.protocol, relative);
      const p = { ...b };
      let h = relative.indexOf("#");
      let hash = "";
      if (h >= 0) { hash = "#" + relative.slice(h + 1); relative = relative.slice(0, h); }
      let q = relative.indexOf("?");
      let search = null;
      if (q >= 0) { search = relative.length - q > 1 ? "?" + relative.slice(q + 1) : ""; relative = relative.slice(0, q); }
      if (relative === "") { p.search = search === null ? b.search : search; p.hash = hash; return p; }
      p.hash = hash; p.search = search ?? "";
      const special = b.protocol in SPECIAL_PORTS;
      if (special) relative = relative.replace(/\\/g, "/");
      if (relative.startsWith("/")) p.pathname = relative;
      else {
        const dir = b.pathname.slice(0, b.pathname.lastIndexOf("/") + 1) || "/";
        p.pathname = dir + relative;
      }
      if (special) p.pathname = URL.#normalizePath(p.pathname);
      return p;
    }
    _setSearchFromParams(qs) { this.#p.search = qs ? "?" + qs : ""; }
    get protocol() { return this.#p.protocol; }
    set protocol(v) { v = String(v).replace(/:$/, "").toLowerCase(); if (/^[a-z][a-z0-9+.-]*$/.test(v)) this.#p.protocol = v + ":"; }
    get username() { return this.#p.username; } set username(v) { this.#p.username = encodeURIComponent(String(v)); }
    get password() { return this.#p.password; } set password(v) { this.#p.password = encodeURIComponent(String(v)); }
    get hostname() { return this.#p.hostname; } set hostname(v) { this.#p.hostname = String(v).toLowerCase(); }
    get port() { return this.#p.port; } set port(v) { v = String(v); if (v === "" || /^\d{1,5}$/.test(v)) this.#p.port = v === SPECIAL_PORTS[this.#p.protocol] ? "" : v; }
    get host() { return this.#p.hostname + (this.#p.port ? ":" + this.#p.port : ""); }
    set host(v) { const m = /^([^:]*)(?::(\d*))?$/.exec(String(v)); if (m) { this.hostname = m[1]; this.port = m[2] ?? ""; } }
    get origin() { return this.#p.protocol in SPECIAL_PORTS && this.#p.protocol !== "file:" ? this.#p.protocol + "//" + this.host : "null"; }
    get pathname() { return this.#p.pathname; }
    set pathname(v) { v = String(v); if (this.#p.protocol in SPECIAL_PORTS) { if (!v.startsWith("/")) v = "/" + v; v = URL.#normalizePath(v); } this.#p.pathname = v; }
    get search() { return this.#p.search; }
    set search(v) { v = String(v); this.#p.search = v === "" || v === "?" ? "" : (v.startsWith("?") ? v : "?" + v); }
    get searchParams() { return URLSearchParams._attach(new URLSearchParams(this.#p.search), this); }
    get hash() { return this.#p.hash; }
    set hash(v) { v = String(v); this.#p.hash = v === "" || v === "#" ? "" : (v.startsWith("#") ? v : "#" + v); }
    get href() {
      const p = this.#p;
      const hasAuthority = p.protocol in SPECIAL_PORTS || p.hostname !== "";
      let s = p.protocol;
      if (hasAuthority) {
        s += "//";
        if (p.username || p.password) s += p.username + (p.password ? ":" + p.password : "") + "@";
        s += this.host;
      }
      return s + p.pathname + p.search + p.hash;
    }
    set href(v) { this.#p = new URL(v).#p; }
    toString() { return this.href; }
    toJSON() { return this.href; }
  }
  globalThis.URL = URL;
  globalThis.URLSearchParams = URLSearchParams;

  globalThis.structuredClone = function structuredClone(value) {
    const seen = new Map();
    const clone = (v) => {
      if (v === null || typeof v !== "object") {
        if (typeof v === "function" || typeof v === "symbol") throw new Error("structuredClone: " + typeof v + " cannot be cloned");
        return v;
      }
      if (seen.has(v)) return seen.get(v);
      let out;
      if (Array.isArray(v)) { out = []; seen.set(v, out); for (const x of v) out.push(clone(x)); return out; }
      if (v instanceof Date) return new Date(v.getTime());
      if (v instanceof RegExp) return new RegExp(v.source, v.flags);
      if (v instanceof Map) { out = new Map(); seen.set(v, out); for (const [k, x] of v) out.set(clone(k), clone(x)); return out; }
      if (v instanceof Set) { out = new Set(); seen.set(v, out); for (const x of v) out.add(clone(x)); return out; }
      if (v instanceof ArrayBuffer) return v.slice(0);
      if (ArrayBuffer.isView(v)) return new v.constructor(v);
      if (v instanceof Error) { out = new v.constructor(v.message); out.name = v.name; out.stack = v.stack; return out; }
      out = {}; seen.set(v, out);
      for (const k of Object.keys(v)) out[k] = clone(v[k]);
      return out;
    };
    return clone(value);
  };

  function fmt(args) {
    return args.map((a) => (typeof a === "string" ? a : safeStringify(a))).join(" ");
  }
  function safeStringify(value) {
    try { return JSON.stringify(value); } catch (_) { return String(value); }
  }
  const log = nativeOp
    ? (level, message) => { nextNativeId++; nativeOp(0, "__log\u0000" + level + "\u0000" + message).catch(() => {}); }
    : (level, message) => __usai_host_log(level, message);
  // `console.info("paid", { invoiceId, cents })`: a trailing plain object is
  // structured fields — it travels after the message (NUL-separated) and the
  // host emits it as its own `fields` attribute, so `--log-format json`
  // carries it as data rather than as text inside the message.
  const isFields = (v) => v !== null && typeof v === "object" && !Array.isArray(v) && !(v instanceof Error) && Object.getPrototypeOf(v) === Object.prototype;
  const line = (a) => {
    if (a.length >= 2 && isFields(a[a.length - 1])) {
      return fmt(a.slice(0, -1)) + "\u0000" + safeStringify(a[a.length - 1]);
    }
    return fmt(a);
  };
  // `console.time`/`timeEnd`: the first thing anyone reaches for when a
  // handler is slow, and its absence was a type error at build time rather
  // than a missing line at run time. The clock is the world's own monotonic
  // one; the labels live for the world, like everything else here.
  const timers = new Map();
  globalThis.console = {
    log: (...a) => log("info", line(a)),
    info: (...a) => log("info", line(a)),
    debug: (...a) => log("debug", line(a)),
    warn: (...a) => log("warn", line(a)),
    error: (...a) => log("error", line(a)),
    time: (label = "default") => {
      if (timers.has(label)) {
        log("warn", "Timer '" + label + "' already exists");
        return;
      }
      timers.set(label, performance.now());
    },
    timeLog: (label = "default", ...a) => {
      const started = timers.get(label);
      if (started === undefined) {
        log("warn", "Timer '" + label + "' does not exist");
        return;
      }
      const took = (performance.now() - started).toFixed(3) + "ms";
      log("info", line([label + ": " + took, ...a]));
    },
    timeEnd: (label = "default") => {
      const started = timers.get(label);
      if (started === undefined) {
        log("warn", "Timer '" + label + "' does not exist");
        return;
      }
      timers.delete(label);
      log("info", label + ": " + (performance.now() - started).toFixed(3) + "ms");
    },
  };
})();
