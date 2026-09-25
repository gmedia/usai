# ADR-0024: outbound I/O is HTTP, and a sidecar is the sanctioned shape for the rest

**Status:** accepted (v0)
**Date:** 2026-09-25
**Decides:** `docs/OPEN-QUESTIONS.md` → Q21, raised by a downstream deployment
that has to drive GoBGP (a gRPC API) for RTBH and FlowSpec announcements.

## Context

`httpClient` is the only way out of a world, and nothing said so. A gap
nobody has written down is discovered by hitting it, which is the worst way
to learn a boundary.

The ask was concrete and reasonable: a gRPC client resource with the same
ownership and cancellation semantics as `httpClient`. Two things make that
larger than it looks.

**gRPC needs proto definitions, and they have to come from somewhere.**
Either the build compiles `.proto` files — a new toolchain step and a new
input to the artifact, in the contract an adopter's deploy depends on — or
the runtime uses server reflection at call time, which is a new failure mode
on the outbound path, or the application hand-writes descriptors. None of
those is a small addition, and all of them land on the surface that is most
expensive to get wrong.

**Streaming RPCs are connection-bound work, from the outbound side.** This
runtime has an answer for connection-bound work that arrives — `socket` and
`stream`, with a lifetime family of their own — and no answer for it
leaving. A unary call fits the existing model exactly; a streaming one does
not. Shipping only the half that fits would publish an API with a hole in
it, and ADR-0013 states the budget this violates: *we will not add an API
without a lifetime rule.*

Against that, the workaround is not a workaround. The deployment that raised
this planned a small sidecar exposing a narrow HTTP API over GoBGP —
announce, withdraw, list, against an allow-list of prefixes — and noted that
this *is also a reasonable safety boundary*. It is: an allow-list in front
of something that can blackhole network prefixes is worth having whether or
not the runtime could speak gRPC.

## Decision

**Outbound I/O is HTTP.** `httpClient` is the whole of it, and
`SUPPORTED.md` and the guide say so rather than leaving it to be discovered.
Other protocols are reached through a process that exposes them over HTTP —
a sidecar — which the documentation now names as the supported shape rather
than treating as a gap.

## Consequences

- An adopter learns the boundary from the documents instead of from a
  failed integration, and gets the pattern in the same sentence.
- A sidecar is a real cost: another process to deploy, and one more hop on
  the path. Where that path is a mitigation — the case that raised this —
  the allow-list it makes room for is worth the hop.
- **What would reopen it**: an answer to outbound connection-bound work
  (which would make streaming RPCs expressible rather than omitted), or
  enough adopters hitting the same wall that the proto-toolchain cost is
  worth paying. This ADR is the record to argue against, not a wall.
- Nothing about `httpClient` changes. It gained private-CA and client
  certificate support in this same release (USAI-001), which is what most
  "we cannot reach that service" cases actually turn out to be.
