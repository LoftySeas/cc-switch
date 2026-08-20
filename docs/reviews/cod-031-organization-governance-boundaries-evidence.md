# COD-031 Organization Governance Boundaries Evidence

- **Status:** Implemented
- **Task:** [COD-031 Organization Governance Boundaries](../tasks/COD-031-organization-governance-boundaries.md)
- **Milestone:** Phase 3 M13 Enterprise Governance
- **Architecture:** [Agent OS Enterprise Governance Architecture v1](../architecture/agent-os-enterprise-governance-v1.md)
- **Review date:** 2026-08-20
- **Source baseline:** `LoftySeas/cc-switch` `main@9f6ddadea17a607af4eddb065e358b746e82c765`

## Scope traceability

| Requirement | Evidence |
| --- | --- |
| Independent Organization identity | `OrganizationId` and `Organization` are new governance-scope identities. No Organization field was added to Agent, Team, Membership, Role, Capability, Permission, Runtime, Provider, Model, Workflow, Execution, or Memory. |
| Revisioned lifecycle | Organization follows `Draft -> Active -> Suspended/Archived` with legal reactivation from Suspended, exact expected revisions, `revision + 1`, trusted timestamps, and terminal Archived state. |
| Explicit Team ownership | `OrganizationTeamBinding` reuses the existing `TeamId`, has Draft/Active/Ended lifecycle, validity, provenance, exact revisions, and preserves the existing Team, Membership, and Relationship aggregates. |
| One Active owning Organization per Team | In-memory updates check the complete binding set while holding one write lock. SQLite uses a partial unique index on Active `team_id`; repository and service operations fail closed on a competing owner. |
| Explicit exact-policy scope | `OrganizationPolicyBinding` targets an exact COD-030 Policy Record or Policy Scope Binding and freezes the exact policy ID, version, layer, record ID, and optional scope-binding ID. It never evaluates a Permission request or creates a Decision or Grant. |
| Organization is non-authoritative | Organization, Team, Membership, Role, and Capability references remain context/evidence only. The service does not import an evaluator, Permission Grant, Authorization Decision, Role, or Capability service. |
| Cross-organization failure closure | All binding mutations require an explicit Organization ID. Team and policy ownership conflicts, mismatched boundary scopes, stale revisions, inactive records, and mismatched Membership evidence produce typed denial or rejection paths. No federation or inferred shared ownership exists. |
| Immutable boundary evidence | `OrganizationBoundaryEvidence` records exact Organization/binding revisions, optional Team/Membership/Agent/Workflow/Execution/resource references, a typed accepted/denied outcome, Trusted Clock time, provenance, and the exact audit event reference. |
| Derived Agent scope is evidence only | Optional Agent scope requires an existing effective Team Membership and records its exact revision. The Agent identity and Membership record remain unchanged. |
| Scoped management views | `management_view(organization_id, limit)` requires one explicit Organization scope and queries Team bindings, policy bindings, and evidence by that exact ID. No unscoped cross-organization list view is exposed. |
| Archived read-only and no deletion | Archived Organizations reject lifecycle and binding mutation. Organizations with Active bindings cannot be archived. SQLite forbids physical deletion of Organization and binding history. |
| Audited lifecycle and denials | Creation, lifecycle changes, Team/policy binding lifecycle, accepted/denied resolution, stale operations, and cross-organization denial use the COD-029 append-only audit sink. Audit metadata contains only allowlisted lifecycle, revision, count, and reason-code values. |
| Validated construction, write, and load | IDs, aggregates, bindings, requests, and evidence validate on construction and DTO deserialization. Both repositories validate writes. SQLite reloads validate canonical JSON and every indexed identity/lifecycle/revision/time column. |

## Data and persistence changes

- Schema version: `25 -> 26`.
- Added `agent_os_organizations`.
- Added `agent_os_organization_team_bindings`.
- Added `agent_os_organization_policy_bindings`.
- Added `agent_os_organization_boundary_evidence`.
- Migration is additive for fresh databases and upgrades from v25.
- SQLite guards enforce immutable identities, bounded canonical JSON, lifecycle-only `revision + 1`, Archived read-only behavior, no physical deletion, one Active Team owner, one Active owner per exact policy target, exact Published policy activation, append-only boundary evidence, and secret-like field rejection.

## Compatibility and unchanged boundaries

- Existing Agent identity has no Organization field.
- Existing Team, Team Membership, and Team Relationship types and persistence are unchanged.
- Existing Permission Policy definitions, selection, Authorization Decisions, and Permission Grants are unchanged.
- Existing Role and Capability semantics are unchanged.
- Runtime, Provider, Model, Execution, Memory, Workflow, Collaboration, and Controlled Execution Environment behavior is unchanged.
- Existing CC Switch Provider switching and configuration behavior is unchanged.
- No Runtime invocation, Provider or Model API call, tool execution, network execution, filesystem execution, Workflow scheduler, autonomous loop, automatic Model selection/fallback, prompt/token/cost routing, implicit authorization, federation, or destructive migration was added.

## Test coverage

- Organization identity validation, lifecycle transitions, stale revisions, and terminal archive.
- Exact Draft/Active/Ended binding revisions and timestamp order.
- One Active owning Organization per Team.
- Archived Organization read-only behavior and Active-binding archive refusal.
- Exact Published policy target persistence and non-authoritative binding behavior.
- Cross-organization Team and policy ownership rejection.
- Missing, inactive, stale, and cross-scope boundary denials.
- Scoped management queries do not return another Organization's evidence.
- Derived Agent scope uses effective Membership evidence without changing Membership or Agent identity.
- Every accepted lifecycle and tested rejection path emits immutable audit evidence.
- SQLite v25-to-v26 migration, update/delete guards, canonical reload validation, policy foreign-key activation, and append-only evidence.
- Existing Team, Permission policy operations, audit, and governance admission regressions.

## Validation

| Check | Result |
| --- | --- |
| COD-031 Domain, Repository, Service, migration, audit, scope, and compatibility tests | Passed: 18 tests; 0 failed. |
| Existing Team organization tests | Passed: 3 tests; 0 failed. |
| Existing Permission policy operations tests | Passed: 22 tests; 0 failed. |
| Existing governance audit tests | Passed: 6 tests; 0 failed. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | Passed. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Passed with no warnings. |
| `cargo test --manifest-path src-tauri/Cargo.toml --all-targets --quiet` | Passed: 2,809 tests; 5 ignored; 0 failed. |
| Architecture forbidden-boundary scan | Passed; production Organization modules contain no invocation, Provider/Model routing, Workflow scheduler, Permission Grant, Authorization Decision, Role, Capability, filesystem, network, or command execution dependency. |
| `git diff --check` | Passed. |

The milestone-level full Rust, TypeScript, frontend, build, documentation-link, migration, and architecture acceptance results are recorded separately in the M13 evidence after COD-031 is committed and remotely verified.
