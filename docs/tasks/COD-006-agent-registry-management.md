# COD-006 Agent Registry Management

## Purpose

Implement Agent Registry management capabilities on top of the completed Agent Domain Foundation.

This task continues the Agent OS architecture defined in:

- `docs/architecture/agent-os-architecture-v1.md`
- `docs/architecture/agent-domain-model.md`
- `docs/architecture/ADR-003-agent-os-architecture-boundaries.md`
- `docs/architecture-decisions/ADR-002-agent-domain-composition.md`

## Goal

Provide stable Agent lifecycle management while preserving Agent OS boundaries.

## Scope

Implement:

- Agent list/query
- Agent creation
- Agent metadata update
- Agent retirement
- Revision conflict handling and user feedback
- Registry management UI
- API integration
- Tests

## Constraints

This task MUST NOT introduce:

- Model binding
- Provider binding
- Runtime execution
- Permission system
- Role system
- Workflow orchestration

Agent remains an independent identity and lifecycle object.

## Architecture Rules

- Agent identity is stable.
- Retired agents cannot be mutated.
- Physical deletion is forbidden.
- Optimistic concurrency through revision must be preserved.
- Domain boundaries must remain independent.

## Acceptance Criteria

- Agent Registry can display existing agents.
- Users can create agents.
- Users can update allowed metadata.
- Users can retire agents.
- Revision conflicts are handled explicitly.
- Existing features have no regression.
- Rust tests pass.
- Frontend tests pass.
- Formatting and static checks pass.

## Delivery

Commit to `main` after completion.

Provide:

- commit hash
- changed files
- test results
- implementation summary
