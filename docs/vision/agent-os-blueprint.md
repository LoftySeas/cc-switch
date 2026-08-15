# CC Switch Agent OS Vision

- **Status:** Draft
- **Owner:** Staff Architect
- **Created:** 2026-08-15
- **Last updated:** 2026-08-15
- **Approvers:** Product owner and architecture owner
- **Source:** [Original Agent OS planning blueprint](../agent-os-blueprint.md)
- **Related:** [Documentation governance](../repository-governance.md), task COD-002

## Document purpose

This document defines what CC Switch Agent OS is intended to become and why that
evolution matters. It is the canonical Vision-layer interpretation of the original
Agent OS planning blueprint.

The source blueprint remains intact for compatibility and historical context. Its
architecture, module, implementation, risk, MVP, and phase content is not repeated
here because those concerns belong to architecture, ADR, and roadmap documents.

## Product vision

CC Switch evolves from an AI runtime and provider manager into an **Agent
Organization Platform**, and ultimately an **AI Agent Operating System**.

The product enables people to build their own AI organization: a purposeful team
of agents and humans working through understandable responsibilities and workflows
toward a shared outcome. Model and runtime choice remain important, but they serve
the work rather than define it.

Agent OS does not mean replacing the user's operating system or granting agents
unrestricted autonomy. It means providing a coherent environment in which agent
teams can be composed, directed, reviewed, and economically governed while humans
retain control.

## Why CC Switch evolves beyond Agent Manager

Agent Manager capabilities answer which runtimes, providers, and models are
available and let users operate them consistently. Those capabilities remain a
valuable foundation and must not be discarded.

However, meaningful work rarely ends with selecting or launching one agent. It
spans responsibilities, handoffs, review, decisions, and multiple periods of
activity. Users need the intent and accountability of that work to survive changes
in sessions, agents, models, and providers.

CC Switch therefore evolves toward Agent OS so users can move from:

- choosing one agent to composing a team around an outcome;
- managing configurations to managing responsibilities;
- isolated conversations to durable, repeatable workflows;
- model-specific behavior to portable organizational intent;
- informal handoffs to visible review and approval; and
- unmanaged model spending to deliberate quality-and-cost trade-offs.

The evolution expands the product's responsibility without abandoning runtime
management. Runtime choice becomes one layer of a broader system for organizing
agent-assisted work.

## Core user value

### Build an AI organization

Users can express complex work as a team of understandable responsibilities rather
than depending on one general-purpose agent. The organization reflects the outcome
the user wants to achieve, not the branding of available models.

### Preserve continuity

Goals, responsibilities, decisions, and unresolved questions remain meaningful
across conversations and changes of model or provider. Users do not need to rebuild
the purpose of the work each time its execution context changes.

### Make collaboration governable

Users can understand who or what is responsible for a contribution, where review
is required, and which decisions remain human-owned. Delegation increases useful
capacity without hiding accountability.

### Reuse successful ways of working

Teams can repeat and improve workflows that produce trusted outcomes. Their
organizational learning remains useful even as individual models evolve.

### Retain runtime and provider choice

Existing runtime-management value remains part of Agent OS. Users can benefit from
different agents, models, and providers without allowing any single one to become
the permanent definition of a role or workflow.

### Balance quality, speed, and cost

Users can apply stronger or more expensive capability where it creates meaningful
value and use proportionate resources elsewhere. Economic control becomes part of
responsible workflow design.

## Agent Team concept

An **Agent Team** is a goal-oriented organization of roles contributing to a
shared outcome. Its identity comes from responsibility and collaboration, not from
a flat list of agents or models.

Roles may represent concerns such as architecture, implementation, review, quality
assurance, or research. Humans remain part of the team as intent owners, boundary
setters, approvers, and accountable decision-makers.

An Agent Team makes complex work legible. Users should understand why each role is
present, what value it contributes, and where its authority ends. Teams may vary
by outcome; the product does not assume that one fixed team structure fits every
kind of work.

## Role and Model separation

A **Role** expresses responsibility: why a contribution exists, what outcome it
owns, what judgment is expected, and what boundaries apply.

A **Model** is a capability resource. Its reasoning strength, speed, context,
availability, privacy posture, and cost may make it more or less suitable for a
particular role.

CC Switch Agent OS separates Role from Model because organizational meaning should
survive changes in model technology. This principle:

- keeps team composition centered on outcomes and accountability;
- allows users to change models without redefining the purpose of a role;
- avoids making one provider the permanent owner of a workflow;
- supports different quality, latency, privacy, and cost priorities across roles;
  and
- makes it possible to evaluate responsibility separately from the resource used
  to fulfill it.

Separation does not mean every model is equally suitable for every role. A role
may require a capability threshold. The principle is that model selection serves
the role; model identity does not define it.

## Workflow First principle

CC Switch Agent OS is **Workflow First**. Its primary product concern is purposeful
work moving from intent to an accepted outcome—not an isolated prompt, chat,
runtime invocation, or autonomous conversation.

A workflow gives context to agents, roles, models, and tools. It explains why work
exists, which responsibilities matter, where judgment and approval are needed, and
what completion means.

Workflow First matters because capable generation alone does not make work
dependable. Durable intent, visible handoffs, reviewability, and explicit acceptance
allow users to trust and improve agent-assisted work over time.

The principle is proportional, not bureaucratic. Simple work should remain simple.
More structure is justified when work has greater duration, value, risk, or need
for coordination.

## Cost optimization philosophy

Cost optimization means achieving the required outcome with an appropriate balance
of capability, latency, human attention, reliability, and monetary spend. It does
not mean always selecting the cheapest model or minimizing token use regardless of
quality.

The relevant cost includes:

- model and provider charges;
- time spent waiting for results;
- human review and correction effort;
- repeated context reconstruction;
- duplicated, failed, or unnecessary work; and
- risk created by unreliable or poorly governed outcomes.

Expensive reasoning capability creates the most value at responsibilities and
checkpoints where difficult judgment materially changes the outcome. Routine or
bounded work should not consume premium capability without a corresponding benefit.

Optimization succeeds only when the required quality, safety, privacy, reliability,
and human accountability are preserved. Users should be able to choose which of
quality, speed, privacy, predictability, or monetary cost matters most in their
context.

## Product principles

1. **Runtime first.** Preserve and build upon CC Switch's existing agent runtime
   and provider-management value.
2. **Outcomes before agents.** Begin with the result the user needs to create.
3. **Roles remain separate from models.** Responsibility is stable; capability
   resources may change.
4. **Workflows drive collaboration.** Explicit purposeful work is more dependable
   than uncontrolled agent conversation.
5. **Cost is visible and intentional.** Premium capability is reserved for work
   where it produces meaningful value.
6. **Humans remain in control.** Users define teams, boundaries, permissions,
   approval points, and final acceptance.
7. **Choice remains portable.** Teams and workflows should outlive changes in
   models, runtimes, and providers.

## Vision boundaries

CC Switch Agent OS is not envisioned as:

- a replacement for human ownership or final accountability;
- a promise of unrestricted or unsupervised agent autonomy;
- a model marketplace whose primary value is provider selection;
- a rigid process imposed on every task;
- a claim that every model can fulfill every role;
- a cost-cutting mechanism that compromises required quality, safety, privacy, or
  reliability; or
- an implementation specification for the current application.

## What success means

The Vision is being realized when users can:

- build an understandable AI organization around a meaningful outcome;
- preserve role and workflow intent across models, runtimes, and providers;
- coordinate agent contributions through repeatable, reviewable workflows;
- maintain explicit human authority over consequential decisions;
- reuse and improve successful team patterns without model lock-in; and
- make deliberate trade-offs among quality, speed, privacy, reliability, and cost.

These are product outcomes. Architecture and roadmap documents translate approved
parts of this Vision into system constraints and sequenced milestones.
