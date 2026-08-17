# Milestone 5 Governance System Evidence

- **Status:** Completed
- **Milestone:** Agent OS Milestone 5 — Governance System
- **Tasks:** COD-013 Capability Governance; COD-014 Permission and Role Assignment
- **Reviewed by:** Codex, acting as Staff Engineer
- **Review date:** 2026-08-17
- **Remote baseline:** `LoftySeas/cc-switch` `main` at `923c8c66a442cbd24ecbfb0c46b2f51b70251834`

## Purpose

This record traces Milestone 5 requirements to implementation and validation
evidence. It applies the existing Agent OS governance boundaries and does not
create a replacement Domain or advance Workflow and multi-Agent capabilities.

## Scope traceability

| Requirement | Implementation evidence |
| --- | --- |
| Capability registry | `CapabilityRegistry` and `InMemoryCapabilityRegistry` store immutable versioned definitions and independently sourced evidence. |
| Capability metadata | `CapabilityDefinition` records semantic version, display metadata and constraint vocabulary without authority semantics. |
| Capability discovery | Evidence can be discovered by Capability identity across explicit Runtime, Provider, Model, Tool and Configuration subject references. |
| Capability validation | `CapabilityGovernanceService` resolves required and optional requirements against version, support state, subject, freshness, confidence and exact constraints. |
| Effective Capability evidence | `CapabilitySnapshot` is immutable, execution-scoped and auditable; unknown required semantics, missing evidence and unusable evidence fail closed. |
| Role definition | `RoleDefinition` records versioned responsibility, Capability requirements and recommended Permission Request references, never a Grant. |
| Role Assignment | `RoleAssignment` references distinct Agent, Team Membership and Role identities plus immutable Role version, bounded scope, validity, provenance, narrowing Capability requirements and Permission policy references. |
| Role lifecycle and repository | Assignments use revisioned Draft, Active, Suspended and Ended transitions. Repository writes reject missing Role definitions, invalid initial state, identity mutation and skipped transitions. |
| Permission Policy and ceiling | `PermissionPolicy` records versioned rules by governance layer. `PermissionCeiling` independently bounds the maximum authority for one Agent. |
| Permission Request and approval | `PermissionRequest` records execution, Agent, Role Assignment, scope, Capability snapshot, ceiling version, policy set, bounded claims, explicit approvals and validity. Silence is never approval. |
| Authorization boundary | `PermissionGovernanceService` requires an effective scoped Role Assignment, matching execution Capability snapshot, enforcement Capability, Agent ceiling and repository-level policy. Every applicable policy must allow; any deny wins. |
| Audit Decisions and Grants | Every evaluation atomically stores the complete immutable Request and Decision. Only an allowed Decision produces a bounded, expiring `PermissionGrant`; denied and approval-required decisions produce no Grant. |
| Execution governance integration | `GovernedExecutionAdmissionGate` verifies Capability snapshot, scoped Role Assignment, allowed Decision and Grant identity/validity before Runtime invocation. Evidence cannot be reused for another execution. |

## Boundary and compatibility verification

- Capability records evidenced ability only. They contain no allow, deny, Grant,
  approval or authorization operation.
- Role records responsibility and contextual eligibility only. Recommended
  Permission Request references and constraint policies cannot produce authority.
- Permission evaluation requires Capability only as evidence that enforcement is
  possible; enforcement Capability never grants the requested action.
- Permission is deny-by-default. Missing repository policy, missing rule,
  constraint mismatch, unsatisfied enforcement Capability, expired request, or
  any explicit deny prevents Grant creation.
- Team Membership remains an explicit opaque reference in this milestone. No
  Team, Team Relationship, Workflow, task scheduler or collaboration engine was
  implemented early.
- The execution gate is read-only. It does not mutate policy, reassign Roles,
  broaden Grants, choose Runtime/Provider/Model, or perform execution.
- No concrete Runtime, model service, Provider API, tool execution, Tauri IPC,
  frontend UI, database migration or existing Provider/configuration change was
  introduced.
- Existing Provider, proxy, configuration, session, usage, Agent registry,
  Runtime binding and execution history behavior remains compatible.

## Validation evidence

| Check | Result |
| --- | --- |
| New Governance foundation tests | 17 tests cover Capability semantics and resolution, Role/Assignment lifecycle, deny-by-default policy, explicit approval, immutable audit, bounded Grant and end-to-end admission evidence. |
| `cargo fmt --check` | Passed. |
| `cargo test --all-targets --quiet` | Passed: 2,680 tests; 5 ignored; 0 failed. |
| `cargo clippy --all-targets -- -D warnings` | Passed with warnings denied. |
| `pnpm format:check` | Passed. |
| `pnpm typecheck` | Passed. |
| `pnpm exec vitest run --reporter=dot --maxWorkers=4 --minWorkers=1` | Passed: 124 files and 881 tests. Existing intentional stderr and React test warnings remain non-failing. |
| `pnpm build:renderer` | Passed. Existing Browserslist, mixed-import and chunk-size warnings remain non-failing. |
| Documentation-link validation | Passed for the Milestone plan and roadmap evidence links. |
| Forbidden-boundary source scan | Passed: no process launch, network client, Tauri command, concrete Runtime, Provider/Model integration, Workflow or Team Domain implementation was introduced. |

## Conclusion

COD-013 and COD-014 establish independent Capability, Role and Permission
governance with explainable, immutable evidence. Milestone 6 can consume these
contracts for Workflow and multi-Agent orchestration without moving authority
into Role labels, Capability declarations, Runtime adapters or model output.
