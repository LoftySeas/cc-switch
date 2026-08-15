# CC Switch Agent OS - Agent Instructions

> This file is the repository bootstrap guide for all AI agents working on this project.

## Purpose

CC Switch is evolving from an AI runtime/provider manager into an Agent Organization Platform.

All agents must treat this repository as a long-term Agent OS project.

## Before Any Task

Every agent MUST complete Context Alignment before analysis or modification.

Required checks:

1. Confirm repository identity.
2. Confirm current branch.
3. Confirm remote state.
4. Confirm current commit.
5. Identify the Source of Truth documents.

Do not make conclusions based only on local files without checking repository state.

## Source of Truth Priority

Use the following priority:

1. User explicitly specified repository, branch, PR, or commit.
2. Verified remote GitHub branch.
3. Current task branch changes relative to the baseline.
4. Local workspace changes.
5. Agent memory or previous conversation context.

## Required Documents

Before architecture or implementation work, read:

- docs/agent-os-blueprint.md
- docs/repository-governance.md
- docs/development/agent-operation-guidelines.md

Before executing a specific task, read the related file in:

- docs/commands/

## Task Execution Protocol

Follow four phases:

### Phase 0 - Context Alignment

Verify repository, branch, commit, and task source.

### Phase 1 - Analysis

Understand current architecture and constraints.

### Phase 2 - Execution

Perform only the requested work.

Do not expand scope without approval.

### Phase 3 - Validation

Verify:

- changed files
- tests or checks
- git status
- delivery status

## Git Delivery Rules

Every task report MUST clearly state:

- changed files
- current branch
- commit hash if created
- push status
- remote verification status

Never use only "completed" as a delivery statement.

## Branch Rules

Do not directly develop on main.

Use appropriate branches:

- feature/<name>
- agent/<agent-name>/<task>
- review/<topic>

## Architecture Principles

1. Runtime and Role are separate concepts.
2. Agent capability must be extensible through adapters.
3. Workflow is preferred over uncontrolled autonomous conversations.
4. Expensive reasoning models should be used at high-value decision points.
5. Preserve existing CC Switch functionality during migration.

## Modification Rules

- Do not modify product code unless the task explicitly requests it.
- Prefer incremental changes.
- Create documentation and ADRs for important architectural decisions.
- Preserve existing information when reorganizing documents.

## Completion Standard

A task is considered complete only when:

1. Work is implemented or documented.
2. Validation is performed.
3. Delivery status is explicitly reported.
4. Evidence is provided for important claims.
