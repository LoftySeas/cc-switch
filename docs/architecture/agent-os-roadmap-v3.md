# Agent OS Roadmap v3

## Purpose

Phase 1 and Phase 2 established Agent OS core domains and product observation capabilities.
Phase 3 focuses on operationalization: making the platform manageable, observable, governed, and ready for separately approved controlled real-world operation.

## Principles

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Memory != Identity
- Execution History != Memory
- Capability != Permission
- Role != Permission
- Team Membership != Permission
- Organization != Team
- Product Layer does not bypass Domain rules
- Governance evidence does not become mutable operational state

## Phase 3 Goals

### Operational Management

Provide management capabilities for existing domains without redefining them.

### Runtime Activation

Introduce controlled concrete runtime integrations through existing adapter boundaries, stopping at a non-executable preparation environment.

### Governance Operations

Provide durable audit evidence, immutable policy operations, deny-by-default selection, and explicit organization isolation before any future real invocation is considered.

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
- Model resolution boundary
- controlled preparation environment
- no real Runtime, Provider, Model, tool, network, or filesystem invocation

### Milestone 13: Enterprise Governance

Status: In progress. See [Enterprise Governance Architecture v1](agent-os-enterprise-governance-v1.md) and the [M13 task index](../tasks/AGENT-OS-M13-INDEX.md).

- COD-029 audit evidence and execution-readiness hardening
- COD-030 Permission policy operations
- COD-031 Organization governance boundaries
- trusted time, immutable activation snapshots, and stale-environment revalidation
- append-only, ordered, tamper-evident governance evidence
- deny-by-default policy version and scope management
- cross-organization isolation without redefining Team

Real invocation remains outside M13 and requires a separately approved task after all governance exit gates pass.

### Milestone 14: Agent Product Platform

Status: Planned; blocked on M13 completion.

- Agent templates
- Marketplace foundation
- Collaboration experiences

## Non-Goals

Phase 3 does not allow:

- direct model calls from UI
- Runtime bypassing adapters
- Provider or Model calls from preparation or governance services
- Memory as identity storage
- Permission bypass
- authority inferred from Role, Capability, Team Membership, or Organization binding
- cross-organization data leakage
- hidden autonomous execution
- tool, network, filesystem, or Workflow scheduling without a separately approved execution milestone
