// Bytes at the boundary. A world speaks JSON to the host, so binary data
// travels as base64: `bytea` columns arrive that way, a `Uint8Array`
// parameter leaves that way, raw request bodies and responses too. These
// two functions are the conversion, once, instead of a loop in every
// handler.

/** Base64 ↔ `Uint8Array`.
 *
 * @category HTTP
 */
export const bytes = {
  toBase64(data: Uint8Array): string {
    let bin = "";
    for (let i = 0; i < data.length; i++) bin += String.fromCharCode(data[i]!);
    return btoa(bin);
  },
  fromBase64(text: string): Uint8Array {
    const bin = atob(text);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  },
};
