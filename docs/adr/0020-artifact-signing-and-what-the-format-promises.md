# ADR-0020: Artifact signing, and what the artifact's format promises

**Status:** accepted (v0)
**Date:** 2026-09-23
**Closes:** ADR-0005's open follow-up ("artifacts fetched from elsewhere need
artifact-level signing before their cache may be trusted"; "revisit: exact
byte format, signing, and compatibility policy at D3 and D14; record as a
follow-up ADR"). The code shipped in 0.0.5; this is the decision it was
missing.

## Context

ADR-0005 made the artifact a directory with an identity: `manifest.json`
(what the application declares, including `codeSha256`), `app.js` (the
bundle), the migrations it carries, and — since its own addendum —
`cache/image.cwasm`, the engine's precompiled form, which is **native code**.
The identity of the artifact is the manifest's; the cache is a host artefact
that any deployment may delete.

That left one thing unresolved and it was the important one: the digests in
the artifact protect against *mismatch and corruption*, not against a party
who can write the directory. An orchestrator that fetches an artifact from a
registry, a CI job that hands one to a deploy script, a rollback that reaches
for yesterday's directory — none of them could tell a built artifact from a
modified one, and `cache/image.cwasm` in particular is native code the
process loads into itself.

## Decision

**One signature covers every file the runtime will read, and verification
happens before anything listens.**

- `usai keygen` produces an Ed25519 key pair; the private key is a hex seed
  in a file, the public key is printed (it is not a file — it belongs in the
  deployment's configuration, not next to the artifact).
- `usai build --sign <key>` writes `signature.json`: the SHA-256 of every
  file the runtime reads — `manifest.json`, `app.js`, the migrations, the
  source map, **and `cache/image.cwasm`** — and an Ed25519 signature over
  that canonical, sorted list.
- `usai run --require-signature <public key>` (or `USAI_REQUIRE_SIGNATURE`,
  repeatable for a rotation window) refuses, **before the listener binds**:
  an artifact with no signature, one signed by a key it does not trust, one
  whose listed file has changed, and one that carries a file the signature
  does not cover. The control surface's `POST /revisions` verifies the same
  way, so an orchestrator cannot install what the process would have refused
  at start.
- Signing is **opt-in**: a runtime started without `--require-signature`
  verifies nothing, because a development loop and a local `usai dev` have no
  key and should not need one. The security posture is the operator's, and
  `docs/THREAT-MODEL.md` says which deployments need it.

**What the artifact's byte format promises, and what it does not.**

- `manifest.json` is **the compatibility surface** and carries its own
  version. An artifact built by version N runs on N and N+1
  (`SUPPORTED.md`); the CI matrix tests both directions on every push, now
  including the SDK axis.
- Everything else in the directory is **not** a stable format: `app.js` is
  the bundler's output, the source map is esbuild's, and `cache/image.cwasm`
  is the engine's serialized module, valid only for the exact engine
  fingerprint that wrote it (it is ignored and recompiled otherwise).
- Therefore **no third party may produce an artifact** except through
  `usai build`. The format is a build output, not an interchange format, and
  this ADR explicitly declines to make it one.

## Consequences

- The supply chain of a deployment is: the operator's key signs what the
  operator's build produced; the runtime accepts nothing else. A compromised
  key is the one case a restart does not fix — the procedure is
  `docs/runbooks/key-and-token-compromise.md`.
- Our own releases are a different chain and are covered separately: the
  binaries and images carry Sigstore build provenance, and npm publishes with
  `--provenance` (0.0.8).
- The claims are verified rather than asserted:
  `scripts/qualification/threat/run.sh` checks that a correctly signed
  artifact runs, a tampered one is refused, and one signed by another key is
  refused — and the release runbook runs it.
- **Open, deliberately:** signature *expiry* and revocation (there is no
  revocation list; rotation is a configuration change), and any notion of a
  signed artifact carrying its own provenance (who built it, from which
  commit). Both need a second party to be useful — a registry, or an
  orchestrator that keeps records — and neither should be invented before
  that party exists.
- **Revisit** when an artifact is routinely fetched from a registry rather
  than built next to the runtime: that is the deployment shape where expiry
  and provenance start paying for themselves.
