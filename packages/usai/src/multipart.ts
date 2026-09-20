// `multipart/form-data` for an `http.raw` route: the exact bytes of the
// request, split into fields and files. A parser, not a framework: the
// whole body is in memory already (the runtime bounded it,
// `USAI_MAX_BODY_BYTES`), so this walks it once. Large uploads belong in
// object storage behind a presigned URL; this is for the form with a file.

/** One file part of a multipart body.
 *
 * @category HTTP
 */
export interface MultipartFile {
  /** The form field's name. */
  name: string;
  filename: string;
  /** The part's `Content-Type`, `application/octet-stream` when absent. */
  contentType: string;
  data: Uint8Array;
}

/** A parsed `multipart/form-data` body.
 *
 * @category HTTP
 */
export interface MultipartBody {
  /** Text fields (a repeated name keeps the last value; use `all` for every value). */
  fields: Record<string, string>;
  files: MultipartFile[];
  /** Every field value in order, repeated names included. */
  all: Array<{ name: string; value: string }>;
}

const decoder = new TextDecoder();

function indexOf(haystack: Uint8Array, needle: Uint8Array, from: number): number {
  outer: for (let i = from; i <= haystack.length - needle.length; i++) {
    for (let j = 0; j < needle.length; j++) if (haystack[i + j] !== needle[j]) continue outer;
    return i;
  }
  return -1;
}

function parameter(header: string, name: string): string | undefined {
  // `name="file"; filename="a.txt"` — quoted or bare, case-insensitive name.
  const re = new RegExp(`(?:^|;)\\s*${name}\\s*=\\s*(?:"((?:[^"\\\\]|\\\\.)*)"|([^;]*))`, "i");
  const m = re.exec(header);
  if (!m) return undefined;
  return m[1] !== undefined ? m[1].replace(/\\(.)/g, "$1") : (m[2] ?? "").trim();
}

/** Parses a `multipart/form-data` body. `contentType` is the request's
 * `content-type` header (the boundary is read from it). Throws a
 * `TypeError` on a malformed body — answer 400 with it.
 *
 * @example
 * ```ts
 * export const upload = http.raw("/notes/:id/attachment", { method: "POST", auth, resources: [db] }, async (ctx) => {
 *   const form = multipart.parse(await ctx.request.bytes(), ctx.headers["content-type"]);
 *   const file = form.files[0];
 *   if (!file) throw errors.custom("validation_failed", 400, "one file expected");
 *   await ctx.resources.db.execute("insert into attachments (note_id, type, data) values ($1, $2, $3)", [ctx.params.id, file.contentType, file.data]);
 *   return http.rawResponse(201, JSON.stringify({ size: file.data.length }), { "content-type": "application/json" });
 * });
 * ```
 *
 * @category HTTP
 */
export function parseMultipart(body: Uint8Array, contentType: string | undefined): MultipartBody {
  if (!contentType || !/^multipart\/form-data\b/i.test(contentType))
    throw new TypeError(`not multipart/form-data: ${contentType ?? "no content-type"}`);
  const boundary = parameter(contentType, "boundary");
  if (!boundary) throw new TypeError("multipart/form-data without a boundary");
  const delimiter = new TextEncoder().encode(`--${boundary}`);
  const crlf = new Uint8Array([13, 10]);
  const out: MultipartBody = { fields: {}, files: [], all: [] };

  let at = indexOf(body, delimiter, 0);
  if (at < 0) throw new TypeError("multipart body without its first boundary");
  at += delimiter.length;
  for (;;) {
    // After a delimiter: `--` closes, CRLF opens a part.
    if (body[at] === 45 && body[at + 1] === 45) break;
    if (body[at] !== 13 || body[at + 1] !== 10)
      throw new TypeError("malformed multipart boundary line");
    at += 2;
    const headersEnd = indexOf(body, new Uint8Array([13, 10, 13, 10]), at);
    if (headersEnd < 0) throw new TypeError("multipart part without headers");
    const headers = decoder.decode(body.subarray(at, headersEnd)).split("\r\n");
    const disposition =
      headers
        .find((h) => /^content-disposition:/i.test(h))
        ?.slice(20)
        .trim() ?? "";
    const partType = headers
      .find((h) => /^content-type:/i.test(h))
      ?.slice(13)
      .trim();
    const name = parameter(disposition, "name");
    if (name === undefined) throw new TypeError("multipart part without a field name");
    const filename = parameter(disposition, "filename");
    const dataStart = headersEnd + 4;
    const next = indexOf(body, delimiter, dataStart);
    if (next < 2) throw new TypeError("multipart part without a closing boundary");
    // The CRLF before the delimiter belongs to the framing, not the data.
    const dataEnd = body[next - 2] === crlf[0] && body[next - 1] === crlf[1] ? next - 2 : next;
    const data = body.subarray(dataStart, dataEnd);
    if (filename !== undefined) {
      out.files.push({
        name,
        filename,
        contentType: partType ?? "application/octet-stream",
        data,
      });
    } else {
      const value = decoder.decode(data);
      out.fields[name] = value;
      out.all.push({ name, value });
    }
    at = next + delimiter.length;
  }
  return out;
}

/** The multipart helper as one object.
 *
 * @category HTTP
 */
export const multipart = { parse: parseMultipart };
