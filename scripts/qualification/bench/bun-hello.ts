// Same endpoint on Bun.serve. `bun run bun-hello.ts <port>`
const port = Number(Bun.argv[2] ?? 3000);
Bun.serve({
  port, hostname: "127.0.0.1",
  fetch(req) {
    const m = /^\/hello\/([^/?]+)/.exec(new URL(req.url).pathname);
    if (!m) return Response.json({ error: { code: "route_not_found" } }, { status: 404 });
    const name = decodeURIComponent(m[1]);
    if (name.length > 40) return Response.json({ error: { code: "validation_failed" } }, { status: 400 });
    return Response.json({ hello: name });
  },
});
