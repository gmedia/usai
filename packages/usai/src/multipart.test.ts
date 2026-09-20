import assert from "node:assert/strict";
import { test } from "node:test";
import { encodeMultipart, parseMultipart } from "./multipart.ts";

const body = (boundary: string, parts: string[]) =>
  new TextEncoder().encode(
    `--${boundary}\r\n${parts.join(`\r\n--${boundary}\r\n`)}\r\n--${boundary}--\r\n`,
  );

test("parseMultipart: fields, files, repeated names, quoted filenames, binary data intact", () => {
  const b = "----usai-boundary-1";
  const raw = body(b, [
    'Content-Disposition: form-data; name="title"\r\n\r\nhello world',
    'Content-Disposition: form-data; name="tag"\r\n\r\na',
    'Content-Disposition: form-data; name="tag"\r\n\r\nb',
    'Content-Disposition: form-data; name="file"; filename="a \\"quoted\\".bin"\r\nContent-Type: application/octet-stream\r\n\r\n\u0000\u0001\r\n\u0002',
    "content-disposition: form-data; name=notes; filename=n.txt\r\n\r\nplain",
  ]);
  const form = parseMultipart(raw, `multipart/form-data; boundary=${b}`);
  assert.deepEqual(form.fields, { title: "hello world", tag: "b" });
  assert.deepEqual(form.all, [
    { name: "title", value: "hello world" },
    { name: "tag", value: "a" },
    { name: "tag", value: "b" },
  ]);
  assert.equal(form.files.length, 2);
  assert.equal(form.files[0]!.filename, 'a "quoted".bin');
  assert.equal(form.files[0]!.contentType, "application/octet-stream");
  assert.deepEqual(
    Array.from(form.files[0]!.data),
    [0, 1, 13, 10, 2],
    "CRLF inside the data is data",
  );
  assert.equal(form.files[1]!.name, "notes");
  assert.equal(form.files[1]!.contentType, "application/octet-stream");
  assert.equal(new TextDecoder().decode(form.files[1]!.data), "plain");
});

test("parseMultipart: what a browser sends (quoted boundary, empty file part)", () => {
  const b = "WebKitFormBoundary7MA4YWxkTrZu0gW";
  const raw = body(b, [
    'Content-Disposition: form-data; name="empty"; filename=""\r\nContent-Type: application/octet-stream\r\n\r\n',
  ]);
  const form = parseMultipart(raw, `multipart/form-data; boundary="${b}"`);
  assert.equal(form.files.length, 1);
  assert.equal(form.files[0]!.data.length, 0);
});

test("parseMultipart: malformed input is a TypeError, not a hang or a partial result", () => {
  assert.throws(() => parseMultipart(new Uint8Array(0), "application/json"), /not multipart/);
  assert.throws(() => parseMultipart(new Uint8Array(0), "multipart/form-data"), /boundary/);
  assert.throws(
    () => parseMultipart(new TextEncoder().encode("garbage"), "multipart/form-data; boundary=x"),
    /first boundary/,
  );
  assert.throws(
    () =>
      parseMultipart(
        new TextEncoder().encode("--x\r\nContent-Disposition: form-data\r\n\r\nv\r\n--x--"),
        "multipart/form-data; boundary=x",
      ),
    /field name/,
  );
  assert.throws(
    () =>
      parseMultipart(
        new TextEncoder().encode('--x\r\nContent-Disposition: form-data; name="a"\r\n\r\nv'),
        "multipart/form-data; boundary=x",
      ),
    /closing boundary/,
  );
});

test("encodeMultipart round-trips through parseMultipart", () => {
  const data = new Uint8Array([0, 255, 13, 10, 7]);
  const { body, contentType } = encodeMultipart([
    { name: "title", value: "hello" },
    { name: "file", filename: 'q"uote.bin', contentType: "application/x-test", data },
  ]);
  const form = parseMultipart(body, contentType);
  assert.deepEqual(form.fields, { title: "hello" });
  assert.equal(form.files[0]!.filename, "q_uote.bin");
  assert.equal(form.files[0]!.contentType, "application/x-test");
  assert.deepEqual(Array.from(form.files[0]!.data), [0, 255, 13, 10, 7]);
  assert.throws(() => encodeMultipart([{ name: "a;b", value: "x" }]), TypeError);
});
