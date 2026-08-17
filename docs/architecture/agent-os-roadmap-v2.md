# Agent OS Roadmap v2

## Purpose
Transition Agent OS from core architecture foundation into productization and runtime activation.

## Phase 2 Goals
- Activate real runtime capability while preserving domain boundaries.
- Integrate existing CC Switch capabilities through adapters.
- Build user-facing Agent platform features.

## Milestones

### Milestone 7 Runtime Activation

Status: Completed

Runtime execution boundary, concrete runtime adapters, provider connectivity.

Provides:

- Revisioned Runtime instance lifecycle and health observations
- Runtime-neutral lifecycle adapter registry and controlled command-host adapter
- Real process activation through fixed executable/argument configuration without
  shell interpolation
- Execution-scoped, non-secret Provider compatibility bindings over the existing
  CC Switch Provider source of truth
- Model capability matching, Provider readiness checks, availability freshness,
  allow-list and deterministic preference routing
- Explicit activation plan composition across independent Runtime, Provider,
  Model and Execution identities

Completion evidence:

- [Milestone 7 Runtime Activation evidence](../reviews/milestone-007-runtime-activation-evidence.md)

### Milestone 8 Execution Platform

Status: Completed

Execution queue, persistence, history, retry and audit.

Provides:

- Durable, revisioned execution history independent of Runtime and Workflow
- Priority queue with explicit lease and terminal state transitions
- Retry decisions based only on declared retry safety and bounded policy
- A new immutable Execution identity for every retry attempt
- Append-only audit evidence for all platform orchestration decisions
- Productive dispatch exclusively through the existing governed Runtime Adapter
  pipeline

Completion evidence:

- [Milestone 8 Execution Platform evidence](../reviews/milestone-008-execution-platform-evidence.md)

### Milestone 9 Context and Memory

Status: Completed

Context lifecycle, memory domain and knowledge references.

Provides:

- One bounded Context Package per immutable Execution attempt
- Separate Memory and Knowledge identities associated by governed references
- Mandatory expiration and explicit archive, expire and revoke lifecycles
- Secret isolation through opaque references instead of durable secret text
- Policy-controlled source selection, sensitivity, trust, counts and lifetime
- Sealed context references that integrate with the existing Runtime-neutral
  ExecutionContext

Completion evidence:

- [Milestone 9 Context and Memory evidence](../reviews/milestone-009-context-memory-evidence.md)

### Milestone 10 Product Layer
UI, management APIs and operational workflows.

## Immutable Boundaries
- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Capability != Permission
- Role != Permission
- Execution != Workflow
- Memory != Identity
