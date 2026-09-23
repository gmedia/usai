# Usai Documentation

> **A program should live only as long as its work requires.**

## Start here

| Read | When |
|---|---|
| [`STATUS.md`](STATUS.md) | every session — current milestone and next steps |
| [`GUIDE.md`](GUIDE.md) | to build an application on the runtime as it is today |
| [`sdk/`](sdk/README.md) | the SDK reference: every export of `@sakaladev/usai`, `/test` and `/config` with its signature, semantics and an example (generated from the source; `make docs`) |
| [`../GOAL.md`](../GOAL.md) | before proposing structure, API, or scope |
| [`LIFECYCLE-CONTRACTS.md`](LIFECYCLE-CONTRACTS.md) | before writing or reviewing runtime code |
| [`GLOSSARY.md`](GLOSSARY.md) | when a term is unclear, or before reading research evidence |
| [`RESEARCH-REFERENCE.md`](RESEARCH-REFERENCE.md) | when you need the evidence behind a contract |
| [`OPEN-QUESTIONS.md`](OPEN-QUESTIONS.md) | before deciding anything about schema, response API, auth, artifacts, reload, runtime-local state, revisions, multi-core, or security |
| [`GUEST-ABI.md`](GUEST-ABI.md) | when touching the bridge, the SDK's `invoke`, or any host operation |
| [`THREAT-MODEL.md`](THREAT-MODEL.md) | before deploying, or when touching input handling, limits, or secrets |
| [`ENVIRONMENT.md`](ENVIRONMENT.md) | every `USAI_*` variable the runtime, CLI and launcher read — operating, tuning, tooling |
| [`CONTROL-API.md`](CONTROL-API.md) | the orchestrator's surface (`--control`): install, activate, drain, remove, invoke, stop |
| [`runbooks/`](runbooks/README.md) | when something is wrong in production: one page per incident class, plus metrics, sizing, systemd and how a release is cut |
| [`ROADMAP.md`](ROADMAP.md) | the phases to production-ready and what each one must prove — the phase decides what work is allowed |
| [`ACCEPTANCE-AUDIT.md`](ACCEPTANCE-AUDIT.md) | to see which `GOAL.md` §53 acceptance items have automated evidence and which are gaps |
| [`measurements/`](measurements/) | the substrate's measured economics (research VM), before any performance claim |
| [`adr/`](adr/README.md) | to record or look up a strategic decision |

Working agreement for agents and contributors: [`../AGENTS.md`](../AGENTS.md).

## Layout

```text
docs/
  README.md                 this index
  STATUS.md                 living: current milestone
  runbooks/                 incident pages from the P5 campaign
  ROADMAP.md                phases to production-ready (qualification, not capability)
  ACCEPTANCE-AUDIT.md       living: GOAL.md §53 items vs evidence
  measurements/             dated measurement reports (engineering evidence)
  GUIDE.md                  developer guide
  sdk/                      generated SDK reference (TypeDoc → markdown; never edit by hand)
  LIFECYCLE-CONTRACTS.md    binding invariants C1–C18
  GLOSSARY.md               vocabulary (production + research-era)
  RESEARCH-REFERENCE.md     map to the (private) research evidence
  OPEN-QUESTIONS.md         living: undecided design questions
  GUEST-ABI.md              host <-> guest contract
  THREAT-MODEL.md           what v0 promises and does not
  ENVIRONMENT.md            every USAI_* variable
  CONTROL-API.md            the control surface reference
  adr/                      architecture decision records
```

Developer-facing documentation is split three ways: the guide (`GUIDE.md`, how to build), the SDK reference (`sdk/`, what each export takes and promises, generated), and the application reference (`/_usai/docs` on a running application, what *this* application is and what happens when work enters it, generated from the definition the runtime executes). Do not write tutorials for features that do not exist.
