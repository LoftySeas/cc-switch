# Command: Imperative outcome

- **Status:** Draft
- **Owner:** Name or team
- **Created:** YYYY-MM-DD
- **Last updated:** YYYY-MM-DD
- **Compatible agents:** Codex / other named agents
- **Related:** Architecture, ADR, milestone, issue, or review links

## Purpose

State the single outcome this command is intended to produce.

## Preconditions

- Required repository state, tools, permissions, and approvals
- Documents that must be read before execution
- Inputs that must be supplied by a human

## Scope

### In scope

- Allowed files, systems, and changes

### Out of scope

- Explicit exclusions and actions requiring separate approval

## Instructions

1. Provide ordered, unambiguous execution steps.
2. Identify required checkpoints or stop conditions.
3. Require preservation of unrelated user changes.

## Constraints and safety

- State invariants, security boundaries, destructive-action restrictions, and
  handling requirements for credentials or user data.

## Verification

List exact checks and expected results. Include automated tests, static checks,
manual inspection, and documentation validation as applicable.

## Required output

Specify the concise handoff: changed files, evidence, risks, assumptions, and next
step. Require the agent to report incomplete checks rather than imply success.

## Escalation conditions

List conflicts, missing decisions, permission changes, destructive actions, or
scope expansions that require the agent to stop and request human direction.
