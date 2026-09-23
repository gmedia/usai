// The WebSocket idle timeout, from outside: `docs/THREAT-MODEL.md` says a
// silent client holding a world is closed with 1008 after
// `socket_idle_timeout`. The campaign runs the fixture with the timeout set
// to a few seconds and this opens a connection, says nothing, and reports
// what the server did.
//
//   node socket-idle.mjs <url> <seconds-to-wait>
// prints one JSON object: { opened, closeCode, closeReason, afterSeconds }
import { createConnection } from "node:net";
import { createHash, randomBytes } from "node:crypto";

const url = new URL(process.argv[2] ?? "ws://127.0.0.1:3600/socket");
const waitSeconds = Number(process.argv[3] ?? 20);
const key = randomBytes(16).toString("base64");
const expect = createHash("sha1")
  .update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`)
  .digest("base64");

const started = Date.now();
const socket = createConnection({ host: url.hostname, port: Number(url.port || 80) }, () => {
  socket.write(
    `GET ${url.pathname} HTTP/1.1\r\nHost: ${url.host}\r\nUpgrade: websocket\r\n` +
      `Connection: Upgrade\r\nSec-WebSocket-Key: ${key}\r\nSec-WebSocket-Version: 13\r\n\r\n`,
  );
});

let opened = false;
let buffer = Buffer.alloc(0);
let result = { opened: false, closeCode: null, closeReason: null, afterSeconds: null };

const done = (extra) => {
  result = {
    ...result,
    ...extra,
    afterSeconds: Number(((Date.now() - started) / 1000).toFixed(1)),
  };
  console.log(JSON.stringify(result));
  socket.destroy();
  process.exit(0);
};

socket.on("data", (chunk) => {
  buffer = Buffer.concat([buffer, chunk]);
  if (!opened) {
    const end = buffer.indexOf("\r\n\r\n");
    if (end === -1) return;
    const head = buffer.subarray(0, end).toString();
    if (!head.includes(" 101 ") || !head.includes(expect)) {
      done({ opened: false, closeReason: head.split("\r\n")[0] });
      return;
    }
    opened = true;
    result.opened = true;
    buffer = buffer.subarray(end + 4);
  }
  // Frames: only the close frame matters here.
  while (buffer.length >= 2) {
    const opcode = buffer[0] & 0x0f;
    let length = buffer[1] & 0x7f;
    let offset = 2;
    if (length === 126) {
      if (buffer.length < 4) return;
      length = buffer.readUInt16BE(2);
      offset = 4;
    }
    if (buffer.length < offset + length) return;
    const payload = buffer.subarray(offset, offset + length);
    buffer = buffer.subarray(offset + length);
    if (opcode === 0x8) {
      done({
        closeCode: length >= 2 ? payload.readUInt16BE(0) : null,
        closeReason: length > 2 ? payload.subarray(2).toString() : "",
      });
      return;
    }
  }
});

socket.on("close", () => done({ closeReason: result.closeReason ?? "tcp closed" }));
socket.on("error", (e) => done({ closeReason: `error: ${e.message}` }));
setTimeout(() => done({ closeReason: "still open" }), waitSeconds * 1000);
