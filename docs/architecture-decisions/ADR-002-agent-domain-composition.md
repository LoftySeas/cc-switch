# ADR-002: Compose Agent Identity from Independent Domain Relationships

- **Status:** Proposed
- **Date:** 2026-08-16
- **Owners:** Staff Architect
- **Approvers:** Architecture owner and repository maintainer
- **Related:** [Agent OS blueprint](../agent-os-blueprint.md), [current-state architecture](../architecture/current-state.md), [Agent Runtime boundary](../architecture/agent-runtime-boundary.md), [Agent Domain Model](../architecture/agent-domain-model.md), [ADR-001](./ADR-001-agent-runtime-boundary.md)
- **Supersedes:** None
- **Superseded by:** None
- **Command:** COD-004 Agent Domain Model Design

## Context

CC Switch currently organizes mature functionality around Provider records,
managed client classifications, native configuration modules, proxy adapters, and
runtime-specific sessions. The current-state audit found no stable Agent identity,
Role assignment, Team Membership, Capability requirement, or Permission Grant.

ADR-001 proposes a capability-based Agent Runtime boundary and establishes that
Agent, Runtime, Provider, Model, and Role must remain separate. COD-004 must carry
that separation into the Agent Organization Layer so Team Graph, Workflow Engine,
and Context Management can refer to stable identities and auditable relationships.

A durable decision is required because collapsing these concepts into one Agent
record would shape schemas, UI, workflow routing, permission evaluation, history,
and runtime adapters. Correcting that coupling after executions and team history
exist would be costly and risky.

### Evidence

- The Agent OS blueprint requires any Agent to serve different Roles and requires
  expensive Models to be selected by value and cost policy rather than identity.
- The current-state audit warns that Provider, `AppType`, Agent, Role, Team, and
  execution lifecycle cannot be represented reliably by the existing provider
  model.
- ADR-001 defines runtime, Provider, Model, Agent identity, Role, Capability, and
  Permission as independent concepts at the execution boundary.
- Existing Providers contain credentials and runtime-shaped settings, so treating
  them as Agents would combine identity with mutable infrastructure binding and
  secret context.
- Team and workflow history must remain interpretable after a Model, Provider,
  Runtime, Role Assignment, or Permission policy changes.

The complete input and delivery evidence is recorded in the
[COD-004 evidence report](../reviews/cod-004-agent-domain-model-evidence.md).

## Decision drivers

- Preserve the invariant `Agent != Role != Model != Runtime != Provider`.
- Give each team participant a stable identity across changing assignments and
  execution bindings.
- Allow one Agent to hold multiple Roles and multiple Agents to share the same
  Runtime, Provider, or Model.
- Keep Capability evidence separate from Permission authority.
- Make Team Membership explicit without turning membership into a Role or grant.
- Preserve immutable historical execution context for audit and cost attribution.
- Keep runtime adapters independent from organizational and workflow policy.
- Preserve current CC Switch Provider, proxy, configuration, session, usage, and
  IPC behavior during additive migration.
- Avoid premature database, language, framework, transport, and UI decisions.

## Considered options

### Option A: Extend Provider or Runtime identity into Agent identity

This option reuses existing CC Switch records and application classifications.
It minimizes the number of new concepts, but an Agent would change identity when
its credential context or executable runtime changes. Multiple organizational
participants sharing one Provider or Runtime could not retain distinct policy,
membership, and history.

This option is rejected because it preserves the coupling identified by COD-002
and contradicts ADR-001.

### Option B: Use one mutable Agent Profile containing all relationships

This option stores Role, Runtime, Provider, Model, Capabilities, Permissions, and
Team fields directly in one profile.

It provides a simple initial editing surface, but makes transient assignments and
observations look like Agent identity. Capability evidence becomes stale profile
state, Permission can be implied by Role or membership, and historical execution
cannot be reproduced after the profile changes.

This option is rejected as the canonical domain model. An implementation may
present an aggregate view, but it must preserve the independent semantics and
identifiers decided here.

### Option C: Define Role-specific Agent subtypes

This option creates types such as Architect Agent, Developer Agent, and Reviewer
Agent, potentially with default Models and Permissions.

It makes common presets convenient but prevents the same Agent from changing
responsibility without identity mutation or duplication. It also encourages Role
names to become hidden Model-routing and authority rules.

This option is rejected. Presets may compose Role Assignments and policy, but
Role is never an Agent subtype.

### Option D: Use stable Agent identity with independent, contextual relationships

This option makes Agent the stable participant and represents Runtime eligibility,
Provider/Model policy, Role Assignment, Capability requirements/evidence,
Permission ceiling/request/grant, and Team Membership as explicit relationships.
One execution resolves those relationships into an immutable binding record.

It introduces more conceptual elements and requires a deterministic resolution
contract. In return, it preserves identity, enables Team Graph and workflow
selection, supports least privilege, and makes history auditable.

This is the selected option.

## Decision

CC Switch Agent OS will model Agent as a stable identity composed with independent
domain relationships rather than as a subtype or alias of Role, Model, Runtime, or
Provider.

The following rules are binding if this ADR is accepted:

1. **Agent has its own stable identity.** Agent IDs are not Provider, Model,
   Runtime, Role, Team Membership, native-session, or execution IDs. “Agent
   Profile” is the configurable aggregate view of an Agent, not a second identity.
2. **Runtime is an eligible execution binding.** One Agent may allow multiple
   Runtimes and one Runtime may serve multiple Agents. One execution attempt
   resolves one immutable Runtime binding through ADR-001.
3. **Provider and Model are separate.** Provider identifies an approved endpoint,
   account, or credential context. Model identifies inference behavior available
   through a Provider or local Runtime. Neither identifies the Agent.
4. **Role is assigned contextually.** Role is a versioned responsibility contract.
   Agents receive Roles through scoped Role Assignments; Agents do not inherit
   from Role types and Role names do not select Runtime or Model.
5. **Capability and Permission remain independent.** Capability requirements and
   evidence determine whether behavior is possible. Permission policy and Grants
   determine whether behavior is allowed. One never implies the other.
6. **Permission is deny-by-default.** Agent has a Permission ceiling. Each
   execution receives an explicit, bounded Grant no broader than the intersection
   of repository, human, Team, workflow, Agent, Role Assignment, workspace,
   Runtime enforcement, and approval constraints.
7. **Team Membership is an explicit association.** It links one Agent and one Team
   with its own lifecycle and provenance. Membership alone does not assign Role,
   establish hierarchy, satisfy Capability, or grant Permission.
8. **Role Assignment is scoped.** It links a Role to a Team Membership for a Team,
   workflow, step, task, or review scope and retains the Role definition version
   used by governed work.
9. **Team relationships are explicit directed edges.** Relationship kinds such as
   manager, reviewer, or consultant describe collaboration only. A workflow must
   explicitly interpret them; they are not implicit control flow or authority.
10. **Execution bindings are immutable.** Every execution records the Agent,
    Runtime, Provider, Model, Role Assignment, effective Capabilities, Permission
    Grant, and policy evidence actually used. Later configuration changes do not
    rewrite history.
11. **Resolution precedes execution.** Orchestration resolves membership,
    assignment, binding eligibility, Capability, budget, and Permission before an
    `AgentRuntime` adapter prepares productive work. Adapters cannot revisit
    organizational selection or broaden authority.
12. **Migration is additive.** Existing Provider, model catalog, native
    configuration, proxy, session, usage, profile, and IPC behavior remains
    authoritative for current functionality until separately approved migration.

This ADR decides conceptual identity, relationship, and authority boundaries. It
does not decide persistence, serialization, policy language, role catalogs,
workflow scheduling, context retention, Rust/React implementation, or user
experience.

## Consequences

### Positive

- Agent identity and Team history survive changes in Runtime, Provider, Model,
  Role, and policy.
- The same Agent can serve different Roles without duplication, and different
  Agents can share infrastructure without identity collision.
- Team Graph can be built from explicit Membership, Assignment, and Relationship
  edges rather than inferred names or configuration fields.
- Workflow selection can use Capability, quality, cost, policy, and availability
  constraints without hard-coding Runtime or Model by Role.
- Permission escalation and review evidence remain explicit and auditable.
- Immutable execution bindings support reproducibility, cost attribution, retry,
  handoff, and review.
- Existing CC Switch capabilities can be referenced and wrapped instead of
  replaced.

### Negative

- The project must define deterministic resolution across multiple policies and
  relationship scopes.
- More domain terms must be represented consistently in future schemas, APIs, UI,
  and documentation.
- Aggregate editing views must avoid hiding important identity and authority
  distinctions.
- Capability freshness and Provider/Model availability require evidence and
  revalidation rather than static profile flags.
- Team graph display alone cannot fully explain workflow eligibility or effective
  Permission.

### Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Domain model is implemented as excessive storage fragmentation | Preserve conceptual boundaries while allowing cohesive implementation aggregates and projections |
| Multiple policy layers produce inconsistent decisions | Define a single versioned, explainable resolution contract and conformance scenarios before implementation |
| Role presets become hidden Model or Permission rules | Require explicit requirements, routing constraints, requests, and Grants; reject identity-based inference |
| Capability evidence becomes stale | Carry source, timestamp, support state, constraints, and confidence; re-resolve before execution |
| Team Membership is mistaken for authority | Require scoped Role Assignment plus explicit Permission Grant for execution |
| Existing credentials are copied into Agent records | Reference current Provider identity and opaque secret references only |
| Mutable configuration rewrites historical meaning | Store immutable execution bindings and referenced definition versions |
| Runtime adapters absorb team logic | Enforce ADR-001 boundary and adapter conformance reviews |

## Validation

The architecture owner is responsible for approving the decision. Future
implementation owners must demonstrate conformance through:

- identity scenarios showing one Agent changing Role, Runtime, Provider, and Model
  without changing Agent ID;
- sharing scenarios showing multiple Agents using one Runtime/Model with different
  Permission ceilings and histories;
- Team lifecycle scenarios for invite, activation, suspension, ending, Role
  assignment, and directed relationships;
- resolution scenarios for missing Capability, stale evidence, unavailable Model,
  denied Permission, approval escalation, and eligible fallback;
- separation-of-duty scenarios that do not infer reviewer independence from Role
  names alone;
- immutable execution evidence containing exact bindings, definition versions,
  effective Capabilities, and Grant; and
- regression evidence proving current provider, proxy, configuration, session,
  usage, and IPC behavior remains unchanged.

An implementation is non-conforming if it uses Role, Model, Runtime, Provider,
Team Membership, or native session identity as Agent identity; grants authority
from Capability or Role alone; or lets a runtime adapter choose organizational
assignments.

## Follow-up work

- [ ] Approve or reject ADR-002 through human architecture review.
- [ ] Define a language-neutral domain contract and example instances without
      selecting persistence.
- [ ] Define deterministic eligibility, Capability, Permission, and cost
      resolution semantics.
- [ ] Define Team Graph and Role catalog governance.
- [ ] Design Workflow Engine state, selection, approval, retry, handoff, and
      separation-of-duty semantics.
- [ ] Design Context Package, memory, retention, and secret-handling boundaries.
- [ ] Produce an MVP milestone plan and compatibility test strategy before product
      implementation.

## Revisit triggers

Revisit this decision when:

- two or more supported workflow cases require Agent identity to change with a
  Role or Runtime binding;
- a supported runtime cannot retain immutable binding evidence through the
  AgentRuntime contract;
- organization or multi-tenant requirements introduce principals not expressible
  as ownership and policy layers around Agent and Team;
- Provider/model catalogs cannot express availability without merging their
  identities;
- Permission resolution cannot be made deterministic, explainable, and deny-by-
  default; or
- an accepted ADR supersedes the identity, relationship, or authority boundaries
  defined here.
