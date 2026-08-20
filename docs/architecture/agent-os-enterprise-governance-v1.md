# Agent OS Enterprise Governance Architecture v1

- **Status:** Approved for Phase 3 Milestone 13
- **Baseline:** Milestone 12 completed at `main@b94c3b38786c04212112ced6dafc0a6c39b3581e`
- **Milestone:** M13 Enterprise Governance

## Purpose

Milestone 13 adds an enterprise governance control plane around the existing Agent OS domains. It makes audit evidence durable and tamper-evident, makes Permission policy versions operationally manageable, and introduces an explicit organization boundary without redefining Agent, Team, Runtime, Provider, Model, Execution, Memory, Capability, Permission, Role, or Workflow.

M13 does not enable real Runtime, Provider, or Model invocation. Real execution remains governed by a separately approved task after all M13 exit gates pass.

## Architecture Position

```text
Product / Operations Layer
          |
Enterprise Governance Control Plane
          |
  +-------+---------+----------------+
  |                 |                |
Audit Evidence   Policy Operations   Organization Boundary
  |                 |                |
Existing Agent OS domains and immutable evidence
```

The governance control plane observes, versions, scopes, validates, and records. It does not become the owner of operational domain identities.

## Non-Negotiable Boundaries

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Execution != Runtime Session
- Memory != Identity
- Memory != Execution History
- Capability != Permission
- Role != Permission
- Team Membership != Permission
- Organization != Team
- Policy Definition != Authorization Decision
- Authorization Decision != Permission Grant
- Audit Evidence != mutable operational state

No Role Assignment, Team Membership, Organization binding, Capability evidence, UI action, or compatibility record grants authority by itself.

## 1. Audit Evidence Control Plane

### Audit event

An enterprise audit event must contain bounded, non-secret evidence:

- immutable Audit Event identity
- stream identity and monotonically increasing sequence
- event kind and normalized outcome
- actor/provenance reference
- subject type and subject reference
- correlation references such as Execution ID, Environment ID, Model Resolution ID, Authorization Decision ID, and Permission Grant ID
- trusted occurrence timestamp
- previous event digest and current deterministic digest
- sanitized structured details

Audit storage is append-only. Update and delete operations are forbidden at both repository and SQLite levels. Each stream must reject duplicate sequence numbers, duplicate event identities, and broken digest chains.

### Activation snapshots

A Controlled Execution Environment must freeze the exact operational state validated during preparation.

`RuntimeActivationSnapshot` must include:

- Runtime Instance ID
- Runtime Instance revision
- Runtime ID
- Runtime Adapter ID
- Runtime lifecycle
- health status and health observation time
- snapshot time

`ProviderActivationSnapshot` must include:

- Provider Instance ID
- Provider Instance revision
- Provider ID
- Provider Adapter ID
- Provider lifecycle
- probe or health observation time
- snapshot time

The environment must also retain the explicit Model Resolution ID, Execution ID, and isolation evidence. A future invocation boundary must compare current Runtime and Provider revisions with these snapshots and fail closed when they are stale. M13 implements the revalidation contract only; it does not invoke anything.

### Time and validation

A trusted clock abstraction supplies audit and preparation timestamps. Ordinary callers must not freely forge final governance timestamps.

The following order is mandatory:

```text
Execution accepted
  <= Model resolution requested
  <= Model resolved
  <= Environment requested
  <= Runtime / Provider snapshot observations
  <= Isolation prepared
  <= Environment prepared
  <= Audit event occurred
```

Persisted and deserialized governance objects must be validated. Persistence DTOs must not bypass domain constructors or validation.

## 2. Policy Operations

M13 reuses the existing deny-by-default Permission domain:

- `PermissionPolicy`
- `PermissionCeiling`
- `PermissionRequest`
- `AuthorizationDecision`
- `PermissionGrant`

It must not create a second Permission domain.

Policy operations add an operational envelope around immutable policy versions:

- Policy Record identity
- immutable policy ID and version reference
- lifecycle: Draft, Published, Retired
- scope/layer binding
- owner reference
- optimistic revision for the operational record
- publication, replacement, and retirement evidence
- explicit active-policy selection by scope

Publishing a new version never mutates an older version. Retiring a policy never rewrites historical Authorization Decisions or Permission Grants. Deny remains the default when no applicable published policy exists.

Role Assignment and Team Membership may narrow context or provide provenance, but neither creates Permission. Capability evidence remains a prerequisite signal, not authority.

## 3. Organization Governance Boundary

Organization is an enterprise scoping identity, not a replacement for Team.

The minimum organization model contains:

- Organization identity
- lifecycle: Draft, Active, Suspended, Archived
- owner reference
- optimistic revision
- created and updated timestamps
- Organization-to-Team binding
- Organization-to-policy binding
- provenance and audit references

Existing Team, Team Membership, and Team Relationship domains remain unchanged. Team Membership continues to describe collaboration context and never grants Permission.

A Team may have at most one active owning Organization binding. Cross-organization Team, policy, Workflow, or Execution references fail closed unless a later, explicit federation design authorizes them. Queries and operations must be organization-scoped and must not leak records across organization boundaries.

## 4. Repository and Service Boundaries

M13 requires replaceable boundaries:

- `GovernanceAuditRepository`
- `PolicyOperationsRepository`
- `OrganizationGovernanceRepository`
- audit query service and read models
- policy publication/retirement service
- organization lifecycle and binding service

SQLite adapters may be added through additive migrations. UI and Tauri commands may consume management views or application services only; they must not access SQLite or domain repositories directly.

## 5. Secret and Privacy Rules

Audit and governance records must never store:

- API keys or credential values
- raw Provider configuration secrets
- raw Memory content
- full prompts or model responses unless a separately approved retention policy explicitly permits them
- environment variables or filesystem content

Only opaque references, normalized outcomes, bounded diagnostics, and sanitized metadata are allowed.

## 6. M13 Delivery Sequence

1. **COD-029 Audit Evidence and Execution Readiness Hardening**
2. **COD-030 Permission Policy Operations**
3. **COD-031 Organization Governance Boundaries**

The tasks are cumulative and must be implemented in this order.

## 7. M13 Exit Gates

M13 is complete only when:

- activation snapshots and trusted-time ordering are enforced
- controlled-environment load and persistence paths validate domain invariants
- audit events are durable, append-only, ordered, and tamper-evident
- policy versions are immutable and operational lifecycles are explicit
- policy selection remains deny-by-default
- organization boundaries reject cross-organization leakage
- Team Membership, Role, and Capability remain non-authoritative
- all migrations are additive and backward compatible
- full Rust, TypeScript, frontend, formatting, lint, build, documentation, and architecture-boundary checks pass
- evidence documentation is committed
- `main` is pushed and remote verified

Until these gates pass, real Runtime, Provider, Model, tool, network, filesystem, or autonomous invocation remains prohibited.
