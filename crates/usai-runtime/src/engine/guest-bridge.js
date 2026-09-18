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
        entry.reject(bridgeError("cancelled", "work was cancelled: " + cancelled));
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

  // Web-standard encoding primitives QuickJS does not ship (ADR-0013).
  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  globalThis.btoa = function (data) {
    const s = String(data);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i), b = s.charCodeAt(i + 1), c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("btoa: character out of Latin1 range");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[(n >> 18) & 63] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return out;
  };
  globalThis.atob = function (data) {
    const s = String(data).replace(/[\s=]+$/g, "").replace(/\s+/g, "");
    let out = "", bits = 0, acc = 0;
    for (let i = 0; i < s.length; i++) {
      const v = B64.indexOf(s[i]);
      if (v < 0) throw new Error("atob: invalid character");
      acc = (acc << 6) | v; bits += 6;
      if (bits >= 8) { bits -= 8; out += String.fromCharCode((acc >> bits) & 255); }
    }
    return out;
  };
  globalThis.TextEncoder = class TextEncoder {
    encode(input) {
      const s = input === undefined ? "" : String(input);
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
      let out = "";
      for (let i = 0; i < b.length;) {
        const x = b[i];
        let cp, n;
        if (x < 0x80) { cp = x; n = 1; }
        else if ((x & 0xe0) === 0xc0) { cp = x & 31; n = 2; }
        else if ((x & 0xf0) === 0xe0) { cp = x & 15; n = 3; }
        else { cp = x & 7; n = 4; }
        for (let j = 1; j < n; j++) cp = (cp << 6) | (b[i + j] & 63);
        out += String.fromCodePoint(cp);
        i += n;
      }
      return out;
    }
  };

  // ---- Web platform globals a backend handler reasonably expects ----------
  // Kept small and dependency-free; this file is evaluated into the image.
  // `URL`/`URLSearchParams` cover the WHATWG behaviour validators and
  // handlers use (parse, components, query manipulation, serialization);
  // `structuredClone` deep-copies plain data, Dates, Maps, Sets, arrays and
  // typed arrays. `fetch` and `crypto` are deliberately absent: outbound
  // HTTP is an external operation that needs an owner (a host operation,
  // not a global), and randomness for secrets must come from the host.
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
  globalThis.console = {
    log: (...a) => log("info", fmt(a)),
    info: (...a) => log("info", fmt(a)),
    debug: (...a) => log("debug", fmt(a)),
    warn: (...a) => log("warn", fmt(a)),
    error: (...a) => log("error", fmt(a)),
  };
})();
