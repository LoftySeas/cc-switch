# CC Switch Agent OS Terminology

## Agent

An executable intelligent software entity capable of performing tasks through a runtime.

An Agent is not equal to a model.

## Runtime

The execution boundary that connects CC Switch with an Agent implementation.

Examples:

- Claude Code runtime
- Codex runtime
- Gemini CLI runtime
- OpenCode runtime

## Model

The underlying reasoning model used by a runtime.

A model is an implementation resource, not an organizational role.

## Provider

The service or infrastructure providing model access.

Examples:

- OpenAI
- Anthropic
- Google
- Local inference

## Role

The responsibility assigned to an Agent.

Examples:

- Architect
- Staff Engineer
- Developer
- Reviewer
- QA Engineer
- Researcher

Role and Runtime must remain independent.

## Team

A collection of Agents connected by responsibilities and collaboration relationships.

## Workflow

A defined sequence of tasks and handoffs between Agents.

## Context

The information package provided to an Agent for a specific task.

Context should be role-specific and not blindly copy full conversation history.
