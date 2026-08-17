# Agent OS Future Task Index

This document defines the implementation task sequence after the current completed milestones.

## Milestone 3 Provider and Model Architecture

- COD-009 Provider Boundary Foundation
- COD-010 Model Architecture Foundation

## Milestone 4 Execution Platform

- COD-011 Execution Pipeline Foundation
- COD-012 Runtime Execution Orchestration

## Milestone 5 Governance

- COD-013 Capability Governance
- COD-014 Permission and Role Assignment

## Milestone 6 Agent Platform

Status: Completed

- COD-015 Workflow Engine Foundation
- COD-016 Multi-Agent Collaboration

Completion evidence:

- [Milestone 6 Agent Platform evidence](../reviews/milestone-006-agent-platform-evidence.md)

All implementations must preserve:

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Capability != Permission
- Role != Permission

Existing CC Switch features are compatibility boundaries and must not be directly replaced by Agent OS domains.
