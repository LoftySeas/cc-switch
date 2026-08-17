# Agent OS Development Roadmap v1

## Purpose

This document defines the complete Agent OS implementation sequence. Codex should implement milestones according to this roadmap instead of creating independent architecture plans.

Architecture source of truth:

- agent-os-architecture-v1.md
- agent-domain-model.md
- ADR-002-agent-domain-composition.md
- ADR-003-agent-os-architecture-boundaries.md

## Core Principles

- Agent is a stable identity object.
- Runtime, Provider, Model, Permission and Workflow are independent domains.
- Domains are connected through explicit bindings.
- No domain may absorb another domain's responsibility.

---

# Phase 1: Identity Foundation

## COD-005 Agent Domain Foundation

Status: Completed

Provides:

- Agent identity
- Lifecycle
- Revision control
- Persistence invariants

## COD-006 Agent Registry Management

Status: Completed

Provides:

- Agent management UI
- Search
- Lifecycle operations
- Conflict recovery

---

# Phase 2: Runtime Architecture

## COD-007 Runtime Adapter Foundation

Status: Completed

Provides:

- Runtime domain
- Runtime descriptor
- Adapter contract
- Execution context foundation

## COD-008 Runtime Binding Management

Goal:

Connect Agent identity with Runtime identity without execution.

Scope:

- AgentRuntimeBinding
- Binding lifecycle
- Binding registry
- Validation

Forbidden:

- Real runtime execution
- Provider integration

---

# Phase 3: Provider Architecture

## COD-009 Provider Boundary Foundation

Goal:

Create provider abstraction independent from Runtime.

Scope:

- Provider domain
- Provider descriptor
- Provider adapter contract
- Provider registry

Forbidden:

- Model execution
- API calls

## COD-010 Model Capability Layer

Goal:

Introduce model identity and capability description.

Scope:

- Model descriptor
- Model capability metadata
- Model registry

Forbidden:

- Routing logic
- Prompt execution

---

# Phase 4: Execution System

## COD-011 Execution Pipeline Foundation

Goal:

Create controlled execution lifecycle.

Scope:

- Execution request
- Execution record
- Execution state machine
- Audit trail

Forbidden:

- Autonomous workflow

## COD-012 Agent Runtime Execution

Goal:

Connect Agent, Runtime and Model through explicit execution pipeline.

---

# Phase 5: Governance

## COD-013 Permission Engine

Goal:

Implement authorization layer.

Scope:

- Permission policy
- Capability authorization
- Decision records

## COD-014 Role Assignment System

Goal:

Introduce organizational role management.

Important:

Role does not automatically grant permission.

---

# Phase 6: Advanced Agent Platform

## COD-015 Workflow Engine

Goal:

Multi-step Agent orchestration.

## COD-016 Multi-Agent Collaboration

Goal:

Agent teams and communication boundaries.

---

# Implementation Rule

Each COD milestone must:

1. Read this roadmap and related task specification.
2. Preserve existing architecture boundaries.
3. Add tests.
4. Commit to main.
5. Push and verify remote state.

Do not create alternative architecture documents unless changing architecture requires an ADR.
