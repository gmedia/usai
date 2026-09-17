# Usai Documentation

> **A program should live only as long as its work requires.**

## Start here

| Read | When |
|---|---|
| [`STATUS.md`](STATUS.md) | every session — current milestone and next steps |
| [`GUIDE.md`](GUIDE.md) | to build an application on the runtime as it is today |
| [`../GOAL.md`](../GOAL.md) | before proposing structure, API, or scope |
| [`LIFECYCLE-CONTRACTS.md`](LIFECYCLE-CONTRACTS.md) | before writing or reviewing runtime code |
| [`GLOSSARY.md`](GLOSSARY.md) | when a term is unclear, or before reading research evidence |
| [`RESEARCH-REFERENCE.md`](RESEARCH-REFERENCE.md) | when you need the evidence behind a contract |
| [`OPEN-QUESTIONS.md`](OPEN-QUESTIONS.md) | before deciding anything about schema, response API, auth, artifacts, reload, runtime-local state, revisions, multi-core, or security |
| [`GUEST-ABI.md`](GUEST-ABI.md) | when touching the bridge, the SDK's `invoke`, or any host operation |
| [`THREAT-MODEL.md`](THREAT-MODEL.md) | before deploying, or when touching input handling, limits, or secrets |
| [`adr/`](adr/README.md) | to record or look up a strategic decision |

Working agreement for agents and contributors: [`../AGENTS.md`](../AGENTS.md).

## Layout

```text
docs/
  README.md                 this index
  STATUS.md                 living: current milestone
  GUIDE.md                  developer guide (v0 preview)
  LIFECYCLE-CONTRACTS.md    binding invariants C1–C13
  GLOSSARY.md               vocabulary (production + research-era)
  RESEARCH-REFERENCE.md     map to HasanH47/usai evidence
  OPEN-QUESTIONS.md         living: undecided design questions
  GUEST-ABI.md              host <-> guest contract
  THREAT-MODEL.md           what v0 promises and does not
  adr/                      architecture decision records
```

Developer-facing documentation (guides, API reference) will be added under `docs/` as milestones D3+ make it real. Do not write tutorials for features that do not exist.
