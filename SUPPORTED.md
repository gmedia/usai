# Supported envelope

What "supported" means here: inside this envelope the maintainers treat a
defect as a bug to fix, and the qualification campaigns (`docs/ROADMAP.md`
P5/P6) were run. Outside it Usai may well work, but nothing has been proven
and nothing is promised.

## Runtime

| | Supported | Notes |
|---|---|---|
| Linux x86_64 (glibc ≥ 2.36) | ✓ | Debian 12 / Ubuntu 22.04 and later; the Docker images are Debian bookworm |
| Linux aarch64 (glibc ≥ 2.36) | ✓ | same |
| macOS arm64 (14+) | development only | binaries are published; no production qualification |
| Windows | ✗ | use WSL2 or Docker |
| Kernel | ≥ 5.10 | the substrate uses `madvise`/`mprotect`-based memory reset; pagemap scanning is used when the kernel offers it |
| Memory | 512 MB minimum for the runtime; 1 GB recommended | the pooling allocator reserves per-world slots up front |

## Data and network

| | Supported |
|---|---|
| PostgreSQL | 15, 16, 17, 18 (TLS with verified certificates; `sslmode=disable|prefer|require`) |
| Outbound HTTP | http/1.1 and h2 to any http(s) origin the application declares |
| Inbound HTTP | HTTP/1.1 behind a reverse proxy that terminates TLS (Caddy, nginx, an ingress) |
| WebSocket / SSE | ✓ through the same proxy (idle timeout 300 s by default) |

## Developer toolchain

| | Supported |
|---|---|
| Node | 24 (build toolchain, tests, `pnpm usai`) |
| pnpm | 12; npm and yarn work for installing, `pnpm` is what the scaffold and docs use |
| TypeScript | 5.9+ |
| Schemas | Zod 4 (Standard Schema + Standard JSON Schema); any Standard Schema library validates *in* the world only |

## Versioning and upgrades

- `0.0.x` (now): contracts may change between versions; each release note
  says what. Runtime and SDK are released together and must be at the same
  version; the runtime refuses an artifact of another manifest format before
  serving, with a message that names both versions.
- Upgrade path: build with the new SDK, deploy the new runtime with the new
  artifact. Rollback: activate the previous artifact on the previous runtime.
  The matrix runtime × artifact (N, N−1) is tested in CI on every push
  against the last published release.
- Migrations are immutable once applied (checksum-verified); a rollback of
  application code does not roll back the database — write migrations to be
  compatible with the previous code (add columns, do not drop them in the same
  release).

## Not supported (and not planned before 1.0)

HTTP/2 or TLS termination in the runtime, Redis/Kafka substrates, Node
compatibility (`require`, `process`, `fs` in a world), a hostile multi-tenant
sandbox (worlds are semantic isolation, ADR-0008), an ORM, multi-region.

## Reporting

Security issues: `SECURITY.md`. Everything else: an issue with `usai --version`,
the platform, and the smallest application that shows the problem.
