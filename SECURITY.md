# Security policy

Usai is a developer preview (`0.0.x`); see `SUPPORTED.md` for the envelope
inside which reports are actionable and `docs/THREAT-MODEL.md` for what the
runtime does and does not defend against. Worlds are **semantic isolation**,
not a sandbox for hostile code (ADR-0008): a report that a malicious
application can affect the host is out of scope until that boundary is
declared.

## Reporting

Use GitHub's private vulnerability reporting on this repository
("Security" → "Report a vulnerability"). Include `usai --version`, the
platform, and the smallest reproduction. The maintainers acknowledge within
7 days and publish a fix and an advisory together; there is no bounty.

## In scope

- The HTTP boundary (`/`): request parsing, validation before a world exists,
  authentication declarations, body and header limits, error sanitization.
- The control surface (`--control`): authentication, revision install and
  activation, artifact loading.
- Artifact loading (`manifest.json`, `app.js`, `cache/image.cwasm`): a crafted
  artifact must be refused, never executed with elevated capability.
- PostgreSQL and outbound HTTP resources: credential handling, TLS verification,
  origin pinning.
- The published images and binaries: supply chain (dependencies are audited in
  CI; releases are built by the workflow in this repository).
