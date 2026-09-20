// Bytes at the boundary. A world speaks JSON to the host, so binary data
// travels as base64: `bytea` columns arrive that way, a `Uint8Array`
// parameter leaves that way, raw request bodies and responses too. These
// two functions are the conversion, once, instead of a loop in every
// handler.

/** Base64 ↔ `Uint8Array`.
 *
 * @category HTTP
 */
// The runtime's core has native codecs (`__usai_native`, C); in Node (the
// test harness, the build) and any other host the JavaScript path runs.
type Native = { b64enc(data: Uint8Array): string; b64dec(text: string): Uint8Array };
const native = (globalThis as { __usai_native?: Native }).__usai_native;

export const bytes = {
  toBase64(data: Uint8Array): string {
    if (native) return native.b64enc(data);
    const parts: string[] = [];
    for (let i = 0; i < data.length; i += 8192)
      parts.push(String.fromCharCode.apply(null, Array.from(data.subarray(i, i + 8192))));
    return btoa(parts.join(""));
  },
  fromBase64(text: string): Uint8Array {
    if (native) return native.b64dec(text);
    const bin = atob(text);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  },
};
