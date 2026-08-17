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
Execution queue, persistence, history, retry and audit.

### Milestone 9 Context and Memory
Context lifecycle, memory domain and knowledge references.

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
