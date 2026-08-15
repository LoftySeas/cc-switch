# Architecture

This directory contains current-state and target-state architecture material for
CC Switch Agent OS: system context, components, contracts, data ownership,
security and trust boundaries, failure modes, and operational characteristics.

A document is authoritative only when it declares an approved status and records
the responsible approver. Existing documents without lifecycle metadata remain
reference material; their presence in this directory does not imply approval.

Use ADRs for individual durable choices. Keep architecture documents current when
accepted decisions or implemented behavior alter the system model.

## Architecture documents

- [`current-state.md`](./current-state.md) records the observed pre-Agent-OS
  architecture and migration seams.
- [`agent-runtime-boundary.md`](./agent-runtime-boundary.md) proposes the
  orchestration-facing runtime contract.
- [`agent-domain-model.md`](./agent-domain-model.md) proposes the Agent
  Organization identity, relationship, Capability, Permission, and Team model.
