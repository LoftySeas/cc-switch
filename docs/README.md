# CC Switch Agent OS documentation

This directory is the durable engineering record for CC Switch Agent OS and the
home of the existing CC Switch product documentation. It separates intent,
architecture, decisions, delivery plans, executable agent instructions, and
review evidence so that each kind of information has one canonical location.

## Engineering documentation map

| Directory                                              | Purpose                                                                          | Typical contents                                                   |
| ------------------------------------------------------ | -------------------------------------------------------------------------------- | ------------------------------------------------------------------ |
| [`vision/`](./vision/)                                 | Defines why Agent OS exists and the product boundaries it must preserve.         | Vision, principles, target outcomes, non-goals                     |
| [`architecture/`](./architecture/)                     | Describes the current approved system design.                                    | Context, components, interfaces, data and trust boundaries         |
| [`architecture-decisions/`](./architecture-decisions/) | Records durable design choices and their consequences.                           | Numbered Architecture Decision Records (ADRs)                      |
| [`roadmap/`](./roadmap/)                               | Orders intended outcomes without making them architectural truth.                | Milestones, sequencing, dependencies, exit criteria                |
| [`commands/`](./commands/)                             | Stores reusable, reviewable task specifications for Codex and compatible agents. | Command contracts, required inputs, verification and handoff rules |
| [`reviews/`](./reviews/)                               | Captures time-bound evaluation evidence.                                         | Agent reviews, risk reviews, readiness reviews                     |
| [`development/`](./development/)                       | Explains how contributors build and validate the system.                         | Local setup, testing, release and operational workflows            |

The governance policy for these directories is defined in
[`repository-governance.md`](./repository-governance.md).

## Agent bootstrap

Every agent starts with the repository-level [`AGENTS.md`](../AGENTS.md) and
[`CONTEXT.md`](../CONTEXT.md), then reads the applicable documents in
[`development/`](./development/) and the published task in
[`commands/`](./commands/). A local checkout is an execution overlay; verify the
GitHub source of truth before drawing repository-wide conclusions.

## Existing product documentation

The existing documentation remains authoritative in its present locations:

- [`guides/`](./guides/) contains task-oriented integration guides.
- [`user-manual/`](./user-manual/) contains localized end-user documentation.
- [`release-notes/`](./release-notes/) contains versioned release history.
- Root-level `pi-*.md` files are existing Pi requirements, contracts, and review
  baselines. They are preserved in place until a separately reviewed migration
  assigns each document a canonical category and updates all inbound links.

## How to use these documents

### Humans

Start with `vision/`, then read `architecture/` and accepted ADRs before proposing
implementation work. Create or update a roadmap milestone for planned delivery,
and require review evidence before declaring a milestone complete. Treat commands
as task specifications, not as substitutes for design approval.

### Codex

Read this index, the applicable vision and architecture documents, all relevant
accepted ADRs, and the active milestone before changing code. Follow a command
document only after resolving its declared inputs. Record assumptions and
verification results in the requested review or milestone artifact. Do not edit
historical decisions merely to make current work appear compliant; propose a new
ADR that supersedes the old one.

### Other agents

Use the same precedence and lifecycle rules as Codex. Identify the agent and model
in generated review evidence, keep factual observations separate from proposals,
and never infer approval from the presence of a draft. Agents must not silently
change vision, accepted ADRs, or milestone scope.

## Document precedence

When documents conflict, use this order:

1. Accepted ADRs for the decision they explicitly govern.
2. Approved architecture documentation for the current system shape.
3. Vision and principles for product intent and boundaries.
4. Active roadmap milestones for delivery scope and sequencing.
5. Commands for execution details.
6. Reviews for point-in-time evidence and recommendations.

If a conflict remains, stop implementation and open an ADR or request human
resolution. The repository governance document defines status and ownership rules.

## Agent OS planning source and Vision

The original [`agent-os-blueprint.md`](./agent-os-blueprint.md) is preserved at its
existing path as the compatibility source for Agent OS planning. Its content spans
Vision, target architecture, modules, roadmap, risks, MVP, and long-term direction.

The canonical What/Why interpretation is
[`vision/agent-os-blueprint.md`](./vision/agent-os-blueprint.md). Architecture and
roadmap content will move into their governed directories through later,
content-preserving tasks. Until those migrations are complete, consumers should
read the original blueprint alongside the applicable layered documents.
