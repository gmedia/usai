// Same endpoint on Deno.serve. `deno run --allow-net deno-hello.ts <port>`
const port = Number(Deno.args[0] ?? 3000);
Deno.serve({ port, hostname: "127.0.0.1" }, (req) => {
  const m = /^\/hello\/([^/?]+)/.exec(new URL(req.url).pathname);
  if (!m) return Response.json({ error: { code: "route_not_found" } }, { status: 404 });
  const name = decodeURIComponent(m[1]);
  if (name.length > 40) return Response.json({ error: { code: "validation_failed" } }, { status: 400 });
  return Response.json({ hello: name });
});
