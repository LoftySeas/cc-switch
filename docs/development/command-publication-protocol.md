# Command Publication Protocol

## Purpose

Define how Agent tasks are published and executed.

## Lifecycle

Every Agent task follows:

```
Draft
  |
Published to GitHub
  |
Agent Trigger
  |
Execution
  |
Validation
  |
Review
```

## Rules

A task is not considered available until the command file exists in GitHub.

Agents must not execute tasks that only exist in chat messages.

## Command Location

Commands should be stored under:

```
docs/commands/
```

## Command Requirements

Each command should define:

- command id
- role
- dependencies
- input documents
- objective
- constraints
- expected output

## Trigger Model

Human provides a short trigger.

Agent reads the command file and related documents.

The command file is the task source of truth.
