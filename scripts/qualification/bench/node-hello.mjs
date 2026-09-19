// The comparison baseline: the same JSON hello on Node's http module, one
// process, no framework. `node node-hello.mjs <port>`
import { createServer } from "node:http";
const port = Number(process.argv[2] ?? 3000);
createServer((req, res) => {
  const m = /^\/hello\/([^/?]+)/.exec(req.url ?? "");
  if (!m) {
    res.writeHead(404, { "content-type": "application/json" });
    res.end('{"error":{"code":"route_not_found"}}');
    return;
  }
  const name = decodeURIComponent(m[1]);
  if (name.length > 40) {
    res.writeHead(400, { "content-type": "application/json" });
    res.end('{"error":{"code":"validation_failed"}}');
    return;
  }
  const body = JSON.stringify({ hello: name });
  res.writeHead(200, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(body),
  });
  res.end(body);
}).listen(port, "127.0.0.1");
