# Development

This directory contains reproducible contributor workflows: environment setup,
build and test procedures, quality gates, release processes, debugging guidance,
and operational runbooks for CC Switch Agent OS.

Development documents describe how to work on the approved system. They must not
silently introduce product scope or architectural decisions.

## Required operating guidance

- [`codex-collaboration-protocol.md`](./codex-collaboration-protocol.md) defines
  the collaboration lifecycle shared by humans and Codex agents.
- [`agent-operation-guidelines.md`](./agent-operation-guidelines.md) defines the
  mandatory repository alignment, discovery, execution, validation, and reporting
  protocol for Codex and other agents.
- [`local-repository-usage.md`](./local-repository-usage.md) defines how local
  repository state is used without treating it as remote truth.
- [`agent-evidence-protocol.md`](./agent-evidence-protocol.md) defines the evidence
  required in Agent completion reports.
- [`command-publication-protocol.md`](./command-publication-protocol.md) defines
  when a task becomes available and how published commands are triggered.
- [`change-management.md`](./change-management.md) classifies documentation,
  architecture, and product-code changes and their review requirements.
