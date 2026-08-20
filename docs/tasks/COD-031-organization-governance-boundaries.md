# COD-031 Organization Governance Boundaries

- **Milestone:** M13 Enterprise Governance
- **Status:** Approved
- **Depends on:** COD-029 and COD-030
- **Architecture:** [Agent OS Enterprise Governance Architecture v1](../architecture/agent-os-enterprise-governance-v1.md)

## Goal

Introduce an explicit enterprise organization boundary that scopes Teams, policy bindings, management queries, and governance evidence without redefining Team or turning organizational membership into Permission.

Organization is a tenancy and ownership boundary. Team remains the collaboration aggregate. Agent identity remains independent from both.

## Existing Domains to Reuse

Reuse, do not duplicate:

- Agent identity and lifecycle
- `Team`, `TeamMembership`, and `TeamRelationship`
- Role Assignment
- Capability evidence
- Permission policy operations from COD-030
- governance audit streams from COD-029
- Workflow and Execution identities as opaque scoped references

## Required Organization Domain

### Organization

Introduce an `Organization` aggregate containing at minimum:

- Organization ID
- display name and purpose
- owner reference
- lifecycle: Draft, Active, Suspended, Archived
- optimistic revision
- created and updated timestamps
- provenance reference

Organization identity is immutable. Archived organizations are read-only and cannot receive new bindings.

### Organization-to-Team binding

Introduce an explicit binding containing:

- binding ID
- Organization ID
- Team ID
- lifecycle: Draft, Active, Ended
- validity interval
- provenance reference
- optimistic revision
- created and updated timestamps

A Team may have at most one active owning Organization binding. Ending a binding does not delete the Team or its historical Memberships and Relationships.

### Organization-to-policy binding

Bind exact COD-030 policy records or scope bindings to an Organization:

- binding ID
- Organization ID
- Permission Policy ID and version or Policy Scope Binding ID
- lifecycle and validity interval
- provenance reference
- optimistic revision

The binding establishes governance scope only. It does not grant Permission.

## Optional Agent Scope Reference

If organization-scoped Agent lookup is required, use an explicit organization scope record or derive it from an active Team binding plus active Team Membership. Do not add `organization_id` directly to Agent identity and do not create a second Team Membership domain.

Any derived Agent scope must be evidence with its own timestamp and provenance, not permanent authority.

## Boundary Resolution

Introduce a side-effect-free Organization Boundary Resolver that accepts explicit references and returns an immutable boundary evidence record containing:

- Organization ID
- Team ID where applicable
- Agent ID where applicable
- Workflow, Execution, policy, or resource references where applicable
- exact organization and binding revisions
- resolution timestamp from the trusted clock
- provenance and audit references

The resolver must fail closed for:

- missing or inactive Organization
- missing or inactive Team binding
- Team bound to another active Organization
- cross-organization policy binding
- inactive or expired Team Membership when membership evidence is required
- cross-organization Workflow, Execution, or management query references
- stale binding revisions

## Cross-Organization Rules

Until a separately approved federation architecture exists:

- cross-organization bindings are forbidden
- cross-organization policy selection is forbidden
- cross-organization Team management is forbidden
- cross-organization Workflow and Execution governance references are forbidden
- management queries require explicit Organization scope
- list operations must never return records from another Organization

The system must not infer federation from shared owners, labels, Provider configuration, Runtime instances, or Model identities.

## Repository and Persistence

Implement replaceable repositories and additive SQLite persistence for:

- Organizations
- Organization-to-Team bindings
- Organization-to-policy bindings
- immutable Organization Boundary evidence

Required guards:

- immutable Organization and binding identities
- revision increments exactly by one
- archived Organization mutation forbidden
- physical delete forbidden for Organization and historical bindings
- one active owning Organization per Team enforced transactionally
- boundary evidence append-only
- all loaded records validated

## Application Services

Implement services for:

- create Organization
- activate, suspend, and archive Organization
- create, activate, and end Team binding
- create, activate, and end policy binding
- resolve Organization boundary evidence
- list Organization-scoped management views

All mutable operations require expected revision and use the trusted clock.

## Audit Integration

Record COD-029 audit events for:

- Organization created or lifecycle changed
- Team binding created, activated, or ended
- policy binding created, activated, or ended
- boundary resolution accepted or rejected
- cross-organization access denied

Audit details must be bounded and sanitized.

## Product Boundary

A read-only Organization governance view may expose:

- Organization lifecycle and revision
- bound Teams
- policy bindings
- sanitized boundary evidence and audit summaries

UI and Tauri commands must consume application services/read models only. UI must not grant Permission, mutate Team Membership implicitly, or bypass expected revisions.

## Compatibility

- Do not modify Agent identity to include Organization.
- Do not duplicate or replace Team, Membership, or Relationship.
- Do not alter Role, Capability, or Permission semantics.
- Do not bind Runtime, Provider, or Model ownership to Organization in M13.
- Existing non-Agent-OS Provider switching and configuration behavior remains unchanged.
- Existing Workflow and Execution histories remain immutable.

## Forbidden

Do not implement:

- Team Membership as Permission
- Organization membership as Permission
- implicit Role Assignment from Organization or Team
- cross-organization federation
- billing, subscription, or marketplace tenancy
- Runtime, Provider, Model, tool, network, or filesystem invocation
- autonomous execution
- destructive migration or physical deletion

## Tests

Add tests covering at minimum:

1. Organization identity is immutable
2. lifecycle transitions and expected revisions are enforced
3. archived Organization is read-only
4. a Team cannot have two active owning Organizations
5. ending a binding preserves Team and Membership history
6. policy binding establishes scope but grants no Permission
7. cross-organization references fail closed
8. organization-scoped list queries do not leak records
9. stale Organization or binding revisions fail closed
10. derived Agent scope does not alter Agent identity
11. Team Membership, Role, and Capability remain non-authoritative
12. all lifecycle and denial operations emit audit events
13. persistence loading validates records
14. existing Team and governance tests remain green

## Milestone Completion Duties

COD-031 closes M13. In addition to implementation:

- update `agent-os-roadmap-v3.md` and `agent-os-milestone-plan-v3.md` to Completed
- add `docs/reviews/milestone-013-enterprise-governance-evidence.md`
- link COD-029, COD-030, and COD-031 evidence
- run all full test and architecture suites
- confirm no real invocation path was enabled

## Acceptance Criteria

COD-031 and M13 are complete when:

- Organization is an explicit independent scope identity
- Team ownership and policy bindings are explicit and revisioned
- cross-organization access fails closed
- organization-scoped queries do not leak data
- Membership, Role, Capability, and Organization bindings remain non-authoritative
- all Organization operations are audited
- migrations are additive and compatible
- full tests and architecture scans pass
- M13 evidence is committed
- `main` is pushed and remote verified
