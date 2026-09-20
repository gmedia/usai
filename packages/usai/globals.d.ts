// Ambient declarations for exactly what a Usai execution world provides —
// nothing more. Reference it from tsconfig (`"types": ["@sakaladev/usai/globals"]`)
// instead of the `DOM` lib or `@types/node`, which would advertise APIs
// (fetch, fs, process, …) that do not exist inside a world. Outbound HTTP
// is the `httpClient` resource; `crypto` below is the subset a world has.

declare function setTimeout(
  fn: (...args: unknown[]) => void,
  ms?: number,
  ...args: unknown[]
): number;
declare function clearTimeout(id: number | undefined): void;
declare function setInterval(
  fn: (...args: unknown[]) => void,
  ms?: number,
  ...args: unknown[]
): unknown;
declare function clearInterval(handle: unknown): void;
declare function queueMicrotask(fn: () => void): void;
declare function structuredClone<T>(value: T): T;
declare function atob(data: string): string;
declare function btoa(data: string): string;

/** Present only to explain itself: rejects with `fetch_not_available`. Use an `httpClient` resource. */
declare function fetch(input: unknown, init?: unknown): Promise<never>;

type UsaiDigest = "SHA-256" | "SHA-384" | "SHA-512";
/** The key usages a world's HMAC keys take — the names the DOM lib uses, so
 * code written against `lib.dom` (`usage: KeyUsage[]`) typechecks here. */
type KeyUsage = "sign" | "verify";
type HmacImportParams = { name: "HMAC"; hash: UsaiDigest | { name: UsaiDigest } };
interface UsaiCryptoKey {
  readonly type: "secret";
  readonly algorithm: { name: "HMAC"; hash: { name: UsaiDigest } };
  readonly extractable: false;
  readonly usages: ReadonlyArray<KeyUsage>;
}
type CryptoKey = UsaiCryptoKey;
/** The WebCrypto subset a world provides: SHA-2 digests, HMAC, random bytes
 * and UUIDs (host entropy per world). Password hashing is `password` in the SDK. */
declare const crypto: {
  getRandomValues<T extends ArrayBufferView>(array: T): T;
  randomUUID(): `${string}-${string}-${string}-${string}-${string}`;
  readonly subtle: {
    digest(
      algorithm: UsaiDigest | { name: UsaiDigest },
      data: ArrayBuffer | ArrayBufferView,
    ): Promise<ArrayBuffer>;
    importKey(
      format: "raw",
      keyData: ArrayBuffer | ArrayBufferView,
      algorithm: HmacImportParams,
      extractable: boolean,
      usages: ReadonlyArray<KeyUsage>,
    ): Promise<UsaiCryptoKey>;
    sign(
      algorithm: "HMAC" | { name: "HMAC" },
      key: UsaiCryptoKey,
      data: ArrayBuffer | ArrayBufferView,
    ): Promise<ArrayBuffer>;
    verify(
      algorithm: "HMAC" | { name: "HMAC" },
      key: UsaiCryptoKey,
      signature: ArrayBuffer | ArrayBufferView,
      data: ArrayBuffer | ArrayBufferView,
    ): Promise<boolean>;
  };
};

declare class TextEncoder {
  readonly encoding: "utf-8";
  encode(input?: string): Uint8Array;
}
declare class TextDecoder {
  constructor(label?: string);
  readonly encoding: string;
  decode(input?: ArrayBuffer | ArrayBufferView): string;
}

declare class URLSearchParams {
  constructor(
    init?: string | Record<string, string> | Iterable<[string, string]> | URLSearchParams,
  );
  readonly size: number;
  append(name: string, value: string): void;
  delete(name: string, value?: string): void;
  get(name: string): string | null;
  getAll(name: string): string[];
  has(name: string, value?: string): boolean;
  set(name: string, value: string): void;
  sort(): void;
  forEach(
    callback: (value: string, name: string, params: URLSearchParams) => void,
    thisArg?: unknown,
  ): void;
  keys(): IterableIterator<string>;
  values(): IterableIterator<string>;
  entries(): IterableIterator<[string, string]>;
  [Symbol.iterator](): IterableIterator<[string, string]>;
  toString(): string;
}

declare class URL {
  constructor(url: string | URL, base?: string | URL);
  static canParse(url: string | URL, base?: string | URL): boolean;
  static parse(url: string | URL, base?: string | URL): URL | null;
  href: string;
  readonly origin: string;
  protocol: string;
  username: string;
  password: string;
  host: string;
  hostname: string;
  port: string;
  pathname: string;
  search: string;
  readonly searchParams: URLSearchParams;
  hash: string;
  toString(): string;
  toJSON(): string;
}

declare namespace console {
  function log(...data: unknown[]): void;
  function info(...data: unknown[]): void;
  function debug(...data: unknown[]): void;
  function warn(...data: unknown[]): void;
  function error(...data: unknown[]): void;
}
