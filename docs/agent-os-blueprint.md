# CC Switch Agent OS Blueprint

## Vision

CC Switch evolves from an AI runtime/provider manager into an Agent Organization Platform.

The goal is not only switching models, but allowing users to compose AI teams where any agent can act as Architect, Staff Engineer, Developer, Reviewer, QA, or Researcher.

Example:

```
User
 |
Agent Team
 |
+----------------+
|                |
Architect     Developer
GPT-5          Claude Code
 |
Reviewer
Codex
```

## Product Principles

1. Runtime first: preserve existing agent runtime management capabilities.
2. Role separated from model: any agent can perform any role.
3. Workflow driven: agents collaborate through explicit workflows instead of uncontrolled conversations.
4. Cost aware: expensive reasoning models are used at high-value checkpoints.
5. Human controlled: users define teams, permissions, and approval points.

## Target Architecture

```
CC Switch
 |
+-- Agent Runtime Layer
|      - Claude Code
|      - Codex CLI
|      - Gemini CLI
|      - OpenCode
|      - Local Models
|
+-- Agent Organization Layer
|      - Agent Registry
|      - Role System
|      - Team Graph
|      - Workflow Engine
|      - Context Manager
|      - Memory
|
+-- User Interface
       - Team Builder
       - Workflow Editor
       - Cost Dashboard
```

## Core Modules

### Agent Registry

Stores available agents.

Fields:

- name
- runtime type
- provider
- model
- capabilities
- cost level
- permissions

### Role System

Predefined roles:

- Architect
- Staff Engineer
- Developer
- Reviewer
- QA Engineer
- Researcher

Role and runtime are independent.

### Team Graph

Defines relationships between agents.

Example:

```yaml
team:
  architect: codex
  developer: claude-code
  reviewer: codex
```

Supported relationships:

- manager
- reviewer
- worker
- consultant

### Workflow Engine

Initial implementation should support sequential workflows.

Example:

```
Requirement
   |
Architect creates plan
   |
Developer implements
   |
Reviewer checks diff
   |
Developer fixes
   |
QA validates
```

Future:

- DAG workflows
- parallel agents
- approval gates

### Context Manager

Responsible for generating role-specific context packages.

Architect receives:

- requirements
- architecture documents
- constraints

Developer receives:

- task
- files
- implementation plan

Reviewer receives:

- git diff
- tests
- changed files

## Development Roadmap

## Phase 0 - Planning

Goal: establish architecture without changing existing behavior.

Tasks:

- Define Agent Runtime interface
- Define Agent Profile schema
- Define Role schema
- Define Team schema
- Define Workflow schema

Deliverables:

- architecture document
- JSON/YAML schema
- technical decisions

## Phase 1 - Agent Profiles

Goal: extend CC Switch from provider profiles to agent profiles.

Tasks:

- Add agent registry
- Add runtime adapters
- Add role metadata
- Add capability detection

Acceptance:

User can define multiple agents and launch them independently.

## Phase 2 - Team Management

Goal: allow users to create AI teams.

Tasks:

- Team configuration
- Role assignment
- Relationship graph
- Team import/export

Acceptance:

User can create Architect + Developer + Reviewer teams.

## Phase 3 - Workflow Execution

Goal: automate collaboration.

Tasks:

- Task object
- Message protocol
- Workflow runner
- Agent handoff

Acceptance:

A feature request can execute through multiple agents automatically.

## Phase 4 - Review and Cost Optimization

Goal: minimize expensive model usage.

Tasks:

- Checkpoint review
- Cost tracking
- Smart routing
- Model selection policies

Example:

```
Simple coding -> cheap model
Architecture -> premium model
Review -> premium model
```

## Phase 5 - Agent Marketplace

Future platform direction:

- share agent profiles
- share workflows
- share team templates

Examples:

- Startup CTO Team
- Full Stack Team
- Research Team
- Game Development Team

## Technical Risks

### Runtime Compatibility

Different agents have different CLI interfaces.

Solution:

Create a unified AgentRuntime interface.

### Context Transfer

Large context passing can become expensive.

Solution:

Use structured context packages instead of raw chat history.

### Permission Control

Agents require different access levels.

Example:

Architect: read only
Developer: write access
Reviewer: read diff only

## MVP Definition

The first valuable version should only implement:

1. Agent profiles
2. Roles
3. Team configuration
4. Sequential workflow
5. Agent handoff
6. Review checkpoint

Avoid initially:

- autonomous agent conversations
- complex DAG scheduling
- marketplace

## Long Term Vision

CC Switch becomes an AI Agent Operating System:

Users do not choose one AI model.

Users build their own AI organization.
