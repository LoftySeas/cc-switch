# COD-021 Context Memory Foundation

## Status
Completed

Implementation is delivered by the existing COD-021 milestone baseline. This
expanded specification is compatible with, and does not replace, the original
[`COD-021 Context Memory`](COD-021-context-memory.md) task record.

## Milestone
Agent OS Phase 2 / Milestone 9 — Context and Memory

## Goal

Establish Context and Memory domain boundaries for Agent OS.

This milestone introduces memory as a governed platform capability without changing Agent identity, execution history, or knowledge systems.

## Architecture Principles

Mandatory boundaries:

- Memory != Identity
- Memory != Execution History
- Memory != Knowledge Base
- Memory != Prompt Template
- Context != Agent State
- Context != Runtime State

## Scope

Implement:

- Context domain model
- Memory reference model
- Memory lifecycle states
- Memory storage abstraction boundary
- Context repository/service interfaces
- Memory reference validation
- Domain tests

## Design Direction

Expected model:

Agent
 |
 Context
 |
 Memory Reference
 |
 Memory Storage Boundary

Memory references represent governed access to information. They do not automatically become part of Agent identity.

## Allowed

- Define Context identity
- Define Memory identity
- Define lifecycle and ownership rules
- Create storage adapter interfaces
- Create retrieval abstraction contracts

## Forbidden

Do not implement:

- Automatic learning
- Vector database integration
- RAG pipeline
- Prompt injection strategy
- Uncontrolled conversation storage
- Model-specific memory behavior
- Runtime-specific memory coupling

## Compatibility Requirements

Existing:

- Agent Domain
- Runtime Adapter
- Execution Platform
- Workflow
- Provider/Model boundaries

must remain unchanged.

## Acceptance Criteria

- Domain boundaries are explicit
- Memory lifecycle is testable
- Repository/service separation exists
- No architecture boundary violations
- Existing full test suite remains passing

## Delivery

After implementation:

- commit to main
- push GitHub
- verify remote commit
- provide implementation and validation report

## Implementation Traceability

| Scope | Delivered boundary |
| --- | --- |
| Context domain model | `ContextPackage`, `ContextPackageId`, `ContextPackageLifecycle`, and `ContextPolicy` |
| Memory reference model | `ContextPackage` stores typed `MemoryEntryId` references; Memory content is not copied into Agent identity or Execution history |
| Memory lifecycle | Revisioned Active, Archived, Expired, and Revoked transitions with no reactivation |
| Memory storage abstraction | `ContextMemoryRepository` with a SQLite adapter and guarded, append-preserving records |
| Context repository/service interfaces | `ContextMemoryRepository` and generic `ContextMemoryService<R>` boundaries |
| Memory reference validation | Agent scope, lifecycle, expiry, source policy, sensitivity, trust, count, and duplicate-reference checks |
| Domain tests | Identity separation, Execution provenance, lifecycle, retention, secret isolation, policy, repository, and service tests |

Detailed implementation and validation evidence is recorded in
[`Milestone 9 Context and Memory Evidence`](../reviews/milestone-009-context-memory-evidence.md).

The delivered boundary has no RAG, embedding, vector database, automatic
learning, Runtime execution, Provider, or Model integration.
