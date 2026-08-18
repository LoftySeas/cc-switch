# Agent OS Roadmap v3

## Purpose

Phase 1 and Phase 2 established Agent OS core domains and product observation capabilities.
Phase 3 focuses on operationalization: making the platform manageable, observable, and capable of controlled real-world operation.

## Principles

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Memory != Identity
- Execution History != Memory
- Product Layer does not bypass Domain rules

## Phase 3 Goals

### Operational Management

Provide management capabilities for existing domains without redefining them.

### Runtime Activation

Introduce controlled concrete runtime integrations through existing adapter boundaries.

### Governance Operations

Expose policy, audit, and lifecycle management.

## Planned Milestones

### Milestone 11: Operational Management

Status: Completed. See [Milestone 11 evidence](../reviews/milestone-011-operational-management-evidence.md).

- Workflow management views
- Team management views
- Agent lifecycle operations
- Execution observability improvements

### Milestone 12: Runtime Activation

Status: Completed. COD-025 through COD-028 delivered activated Runtime and Provider Adapter boundaries, explicit Model Resolution, and a preparation-only Controlled Execution Environment. See [COD-028 evidence](../reviews/cod-028-controlled-execution-environment-evidence.md).

- Runtime adapter implementations
- Provider adapter implementations
- Model resolution boundary (explicit validation foundation completed in COD-027)
- Controlled execution environments (preparation foundation completed in COD-028)

### Milestone 13: Enterprise Governance

- Audit operations
- Policy management
- Organization boundaries

### Milestone 14: Agent Product Platform

- Agent templates
- Marketplace foundation
- Collaboration experiences

## Non-Goals

Phase 3 does not allow:

- Direct model calls from UI
- Runtime bypassing adapters
- Memory as identity storage
- Permission bypass
- Hidden autonomous execution
