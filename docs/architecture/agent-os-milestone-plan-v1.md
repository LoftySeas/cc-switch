# Agent OS Milestone Execution Plan v1

## Purpose

This document defines the large-scale implementation roadmap for Agent OS. It is the execution plan for Codex implementation. Architecture principles remain defined by the architecture and ADR documents.

The goal is to avoid fragmented development. Codex should implement complete milestones instead of isolated micro tasks.

## Engineering Principles

- Agent is an independent identity object.
- Runtime is an execution boundary, not an Agent property.
- Provider is an infrastructure boundary, not a Runtime identity.
- Model is a capability resource, not an Agent identity.
- Capability describes ability.
- Permission describes authorization.
- Role describes assignment and organization semantics.
- Workflow describes orchestration.

No later milestone may violate these boundaries.

---

# Milestone 1: Agent Identity Foundation

Status: Completed

Includes:

- Agent Domain Foundation
- Agent Registry Management

Completed capabilities:

- Stable Agent identity
- Lifecycle management
- Revision control
- Registry management
- Retired immutability

---

# Milestone 2: Runtime Architecture

Status: Completed

Includes:

- Runtime Adapter Foundation
- Runtime Binding Management

Goals:

- Define Runtime identity
- Define Adapter contracts
- Bind Agent to Runtime through independent objects
- Prepare execution boundaries

Must not include:

- Real Runtime execution
- Provider integration
- Model selection

Completed capabilities:

- Independent Runtime, Runtime Adapter, Runtime Execution and Runtime Binding identities
- Runtime descriptor, capability metadata and read-only availability probing
- Extensible Runtime Adapter and registry boundaries without concrete adapters
- Independent Agent-to-Runtime binding lifecycle and revision control
- Binding lookup, relationship validation and immutable identity enforcement
- Execution Context validation foundation without productive execution

Completion evidence:

- [Milestone 2 Runtime Architecture evidence](../reviews/milestone-002-runtime-architecture-evidence.md)

---

# Milestone 3: Provider and Model Architecture

Status: Completed

Goals:

- Introduce Provider abstraction
- Introduce Model catalog
- Define capability discovery
- Separate model availability from Agent identity

Must not include:

- Permission decisions
- Workflow execution

Completed capabilities:

- Agent OS Provider identity, descriptor, capability metadata and adapter contract
- Provider adapter registry and read-only Provider catalog service
- Non-secret compatibility boundary around existing CC Switch Provider records
- Independent Model identity, descriptor, metadata and capability declarations
- Model registry with explicit Provider–Model availability relationships
- Provider and Model lookup services without routing or execution

Completion evidence:

- [Milestone 3 Provider and Model Architecture evidence](../reviews/milestone-003-provider-model-architecture-evidence.md)

---

# Milestone 4: Execution Platform

Status: Completed

Goals:

- Execution pipeline
- Execution lifecycle
- Runtime invocation boundary
- Execution history
- Observability

Requirements:

- Preserve immutable execution evidence
- Preserve Agent, Runtime, Provider, Model references

Completed capabilities:

- Immutable execution request with resolved Agent, Runtime, Provider and Model references
- Opaque Capability snapshot and Permission grant evidence references without policy enforcement
- Revisioned, append-only execution records, transitions and terminal results
- Runtime-neutral pipeline and mandatory admission-gate boundaries
- Separate execution-capable Runtime Adapter registry and invocation contract
- Normalized success, admission rejection, Runtime unavailability, context rejection and invocation failure handling
- Execution observability through ordered lifecycle transitions, result summaries and artifact references

Completion evidence:

- [Milestone 4 Execution Platform evidence](../reviews/milestone-004-execution-platform-evidence.md)

---

# Milestone 5: Governance System

Status: Completed

Goals:

- Capability registry
- Permission engine
- Role assignment
- Policy evaluation

Rules:

Capability != Permission
Role != Permission

Completed capabilities:

- Versioned Capability definitions, requirements, evidence and discovery registry
- Execution-scoped effective Capability snapshots with freshness, confidence and constraint validation
- Explicit fallback semantics for optional Capabilities and fail-closed required semantics
- Versioned Role definitions and scoped, lifecycle-managed Role Assignments
- Independent Permission Policy, Agent ceiling, bounded request and approval evidence models
- Deny-by-default layered policy evaluation with explicit deny precedence
- Immutable Authorization Decisions, Permission Grants and complete request audit records
- Read-only governed Execution admission that verifies mutually consistent Capability, Role, Decision and Grant evidence

Completion evidence:

- [Milestone 5 Governance System evidence](../reviews/milestone-005-governance-system-evidence.md)

---

# Milestone 6: Agent Platform

Status: Completed

Goals:

- Workflow orchestration
- Multi-agent collaboration
- Task coordination
- Advanced automation

Completed capabilities:

- Stable Team, Team Membership, and directed Team Relationship identities and
  lifecycles without implied Role, Capability, Permission, or control flow
- Versioned Workflow definitions with validated acyclic dependencies and explicit
  responsibility, governance, and acceptance references
- Revisioned Workflow Run, Step, and execution-bound Task state
- Governed participation requiring active Agent and Membership, scoped Role
  Assignment, effective Capability evidence, allowed Authorization Decision,
  bounded Permission Grant, and immutable Execution request
- Task state synchronization from normalized Execution evidence, with terminal
  results required before successful dependency release
- Permission-bound immutable communication records and explicit Handoff proposal,
  acceptance, rejection, and cancellation evidence
- Atomic Workflow Task/Run coordination updates
- Explicit separation between Handoff evidence and Workflow state transitions

Completion evidence:

- [Milestone 6 Agent Platform evidence](../reviews/milestone-006-agent-platform-evidence.md)

---

# Phase 2 Productization Milestones

## Milestone 7: Runtime Activation

Status: Completed

Runtime lifecycle activation, Provider integration boundary and policy-driven
Model routing were delivered without collapsing Runtime, Provider or Model
identities.

Completion evidence:

- [Milestone 7 Runtime Activation evidence](../reviews/milestone-007-runtime-activation-evidence.md)

## Milestone 8: Execution Platform Productization

Status: Completed

Durable execution history, queue/retry controls, audit records and bounded
dispatch were delivered through Runtime-neutral execution boundaries.

Completion evidence:

- [Milestone 8 Execution Platform evidence](../reviews/milestone-008-execution-platform-evidence.md)

## Milestone 9: Context and Memory

Status: Completed

Time-bounded Memory, Knowledge references and sealed Execution Context packages
were delivered without making Memory part of Agent identity.

Completion evidence:

- [Milestone 9 Context and Memory evidence](../reviews/milestone-009-context-memory-evidence.md)

## Milestone 10: Product Layer

Status: Completed

Agent management, governed Workflow operations and immutable Execution
visibility were exposed through backend services and Tauri commands. Product UI
does not access repositories or own Domain transition rules.

Completion evidence:

- [Milestone 10 Product Layer evidence](../reviews/milestone-010-product-layer-evidence.md)

---

# Codex Execution Rules

For each milestone:

1. Read this roadmap and related architecture documents.
2. Implement the complete milestone scope.
3. Do not redesign architecture.
4. Add tests.
5. Commit to main.
6. Push to GitHub.
7. Verify remote state.
8. Report completed milestone, tests, commit hash, and remaining roadmap.
