# COD-004 Agent Domain Model Design

## Role

Staff Architect

## Depends On

- COD-002
- COD-003
- COD-003.1

## Input Documents

Read before execution:

- docs/agent-os-blueprint.md
- CONTEXT.md
- docs/architecture/agent-runtime-boundary.md
- docs/architecture/current-state.md
- docs/development/agent-operation-guidelines.md

## Objective

Design the Agent Domain Model for CC Switch Agent OS.

This task defines the core domain relationships required to evolve CC Switch from runtime/provider management into an Agent Organization Platform.

Do not implement product code.

## Mission

Define the conceptual model that connects:

- Agent
- Runtime
- Provider
- Model
- Role
- Capability
- Permission
- Team Membership

The design must preserve the architectural principle:

Agent != Role != Model

## Context

The Agent Runtime Boundary establishes the contract between orchestration and runtime-specific implementations.

The next architecture layer requires a stable domain model for agent identity, responsibility assignment, and future workflow execution.

## Constraints

- Do not define database schema.
- Do not select implementation language or framework.
- Do not modify Rust or React code.
- Keep runtime adapters independent from role definitions.
- Preserve existing CC Switch functionality.
- Use ADR-oriented architectural thinking.

## Success Criteria

Deliver architecture documentation defining:

- Agent identity model
- Runtime relationship model
- Provider and model separation
- Role assignment model
- Capability representation
- Permission boundary model
- Team membership relationship
- Extension strategy for future Agent OS features

The output should provide the foundation for:

- Team Graph
- Workflow Engine
- Context Management
- MVP implementation planning

## Validation

Before reporting completion:

- verify source documents used
- report changed files
- report git delivery status according to Agent Operation Guidelines
- provide evidence for important architectural decisions
