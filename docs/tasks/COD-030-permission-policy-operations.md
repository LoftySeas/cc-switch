# COD-030 Permission Policy Operations

- **Milestone:** M13 Enterprise Governance
- **Status:** Approved
- **Depends on:** COD-029 Audit Evidence and Execution Readiness Hardening
- **Architecture:** [Agent OS Enterprise Governance Architecture v1](../architecture/agent-os-enterprise-governance-v1.md)

## Goal

Operationalize the existing deny-by-default Permission domain without creating a second authorization model.

COD-030 manages immutable policy versions, publication state, active scope bindings, retirement, and audit evidence. It does not make Role, Capability, Team Membership, Organization binding, or UI state authoritative.

## Existing Domain to Reuse

Reuse the existing contracts:

- `PermissionPolicy`
- `PermissionCeiling`
- `PermissionRequest`
- `AuthorizationDecision`
- `PermissionGrant`
- `PermissionPolicyLayer`
- existing Capability snapshots and Role Assignment references

Do not redefine their meaning.

## Required Operational Domain

Introduce a policy operations envelope around immutable Permission policy versions.

### Policy record

A `PermissionPolicyRecord` or equivalent must contain:

- operational record ID
- Permission Policy ID
- immutable policy version
- policy layer
- owner reference
- lifecycle: Draft, Published, Retired
- optimistic revision
- created, updated, published, and retired timestamps as applicable
- provenance reference
- replacement policy-version reference when applicable

The embedded or referenced `PermissionPolicy` definition is immutable once the record is Published.

### Policy scope binding

Introduce an explicit binding that selects one published policy version for a bounded scope:

- binding ID
- policy ID and version
- policy layer
- scope kind and scope reference
- organization reference when COD-031 provides one; until then use an optional opaque boundary reference
- lifecycle: Draft, Active, Ended
- optimistic revision
- validity interval
- provenance reference

A scope may not have multiple simultaneously active bindings for the same policy layer and selector unless the existing authorization evaluator explicitly supports deterministic composition. Ambiguity must fail closed.

## Required Operations

Implement application services for:

- create draft policy record
- publish a policy version
- activate a scope binding
- replace an active policy version explicitly
- retire a policy version
- end a scope binding
- list and inspect policy records and bindings through read models

All operations must use expected revision where records are mutable.

## Policy Semantics

- Publishing version N+1 never mutates version N.
- Retiring a policy does not alter historical Authorization Decisions or Permission Grants.
- Existing Decisions and Grants continue to reference the exact policy versions evaluated at the time.
- No applicable active Published policy means deny by default.
- A `RequireApproval` rule never becomes Allow without a separate approved decision flow.
- A Role Assignment may contribute context and narrowing constraints only.
- Capability evidence may prove feasibility only.
- Team Membership and Organization bindings may establish scope only.
- None of these create authority.

## Policy Selection Boundary

Introduce a deterministic, side-effect-free policy selection service that accepts explicit scope evidence and returns:

- exact selected policy IDs and versions
- policy layers and precedence order
- selection timestamp from the trusted clock
- selection evidence ID
- typed no-policy, ambiguous-policy, retired-policy, or out-of-scope failures

The selection service must not issue an Authorization Decision or Permission Grant. It only resolves the exact policy set for an existing evaluator.

## Audit Integration

Use COD-029 governance audit streams to record:

- draft created
- policy published
- scope binding activated
- active version replaced
- binding ended
- policy retired
- selection accepted or rejected

Audit metadata must be sanitized and must not contain secrets, raw Memory, prompts, model output, or credential data.

## Repository and Persistence

Implement replaceable repositories and additive SQLite persistence for:

- policy operational records
- scope bindings
- immutable policy selection evidence

Required database guards:

- policy ID and version immutable after insertion
- Published policy definition immutable
- revision increments exactly by one on operational updates
- historical policy versions cannot be deleted
- selection evidence append-only
- active binding uniqueness enforced transactionally

## Management Boundary

A bounded management query or read-only UI may be added for:

- policy versions and lifecycle
- active scope bindings
- replacement lineage
- sanitized selection evidence

The product layer must not directly access repositories or SQLite, and no UI control may bypass application services or expected revisions.

## Compatibility

- Preserve existing authorization evaluation semantics.
- Preserve all existing Permission Request, Decision, and Grant structures.
- Preserve Role and Capability independence.
- Preserve Team Membership as non-authoritative.
- Do not modify Runtime, Provider, Model, Execution, Memory, Workflow, or Controlled Execution Environment behavior except to carry exact immutable policy-selection references where already supported.

## Forbidden

Do not implement:

- implicit permission from Role, Team, Membership, Organization, or Capability
- automatic policy generation
- autonomous approval
- Runtime or Model invocation
- Provider routing
- tool execution
- hidden default Allow
- mutation or deletion of historical Decisions or Grants
- organization tenancy rules beyond optional opaque scope references; that is COD-031

## Tests

Add tests covering at minimum:

1. published policy definitions are immutable
2. publishing a new version does not modify the old version
3. stale expected revisions are rejected
4. active scope binding uniqueness is transactional
5. ambiguous selection fails closed
6. absent policy selection returns deny-by-default evidence
7. retired policies cannot become newly active
8. historical Decisions and Grants remain unchanged
9. Role, Capability, Membership, and Organization references do not grant permission
10. audit events are emitted for every lifecycle operation
11. persisted records validate on load
12. legacy Permission tests remain green

## Acceptance Criteria

COD-030 is complete when:

- policy versions are immutable and operational lifecycles are explicit
- deterministic scope selection returns exact policy-version evidence
- deny-by-default is preserved
- historical Decisions and Grants remain immutable
- active policy ambiguity fails closed
- operations are audited through COD-029
- migrations are additive and compatible
- full tests and architecture scans pass
- evidence is recorded in `docs/reviews/cod-030-permission-policy-operations-evidence.md`
- `main` is pushed and remote verified
