# COD-003 Agent Runtime Boundary Design

## Role

Staff Architect

## Depends On

- COD-001
- COD-001.5
- COD-001.6
- COD-001.7
- COD-002

## Input Documents

Read before execution:

- docs/agent-os-blueprint.md
- docs/development/agent-operation-guidelines.md
- docs/architecture/current-state.md

## Objective

Design the Agent Runtime boundary for CC Switch Agent OS.

This task defines the architecture contract between CC Switch and external agent runtimes.

Do not implement product code.

## Analyze

Design how CC Switch should abstract:

- Claude Code
- Codex CLI
- Gemini CLI
- OpenCode
- Local model agents

## Output

Create:

`docs/architecture/agent-runtime-boundary.md`

The document should define:

### AgentRuntime abstraction

Include:

- lifecycle
- capabilities
- execution model
- input/output contract
- error handling
- permissions

### Agent Profile Model

Define the difference between:

- runtime
- provider
- model
- agent identity
- capabilities
- cost level

### Role Separation

Explain why:

Agent != Role != Model

Example:

A Codex runtime can act as Architect or Reviewer.

A Claude runtime can act as Developer or Researcher.

### Extension Strategy

Explain how future runtimes can be added without modifying core orchestration.

## Constraints

- Preserve current CC Switch functionality.
- Avoid premature implementation details.
- Focus on architecture contracts.
- Do not modify Rust or React code.

## Validation

Before reporting completion:

- verify source documents used
- report changed files
- report git delivery status according to Agent Operation Guidelines
