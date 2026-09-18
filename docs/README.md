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
  GUIDE.md                  developer guide (v0 preview)
  LIFECYCLE-CONTRACTS.md    binding invariants C1–C13
  GLOSSARY.md               vocabulary (production + research-era)
  RESEARCH-REFERENCE.md     map to the (private) research evidence
  OPEN-QUESTIONS.md         living: undecided design questions
  GUEST-ABI.md              host <-> guest contract
  THREAT-MODEL.md           what v0 promises and does not
  adr/                      architecture decision records
```

Developer-facing documentation (guides, API reference) will be added under `docs/` as milestones D3+ make it real. Do not write tutorials for features that do not exist.
