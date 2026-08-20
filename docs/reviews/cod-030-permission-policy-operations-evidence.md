# COD-030 Permission Policy Operations Evidence

- **Status:** Implemented
- **Task:** [COD-030 Permission Policy Operations](../tasks/COD-030-permission-policy-operations.md)
- **Milestone:** Phase 3 M13 Enterprise Governance
- **Architecture:** [Agent OS Enterprise Governance Architecture v1](../architecture/agent-os-enterprise-governance-v1.md)
- **Reviewed by:** Codex, acting as Staff Architect and implementation engineer
- **Review date:** 2026-08-20
- **Source baseline:** `LoftySeas/cc-switch` `main@e24eaf6310348813c656fea3f4f00a993722650b`

## Scope traceability

| Requirement | Evidence |
| --- | --- |
| Reuse the existing Permission domain | `PermissionPolicyRecord` embeds the existing `PermissionPolicy`; scope bindings and selection evidence use the existing `PermissionPolicyId`, `PermissionPolicyVersionRef`, and `PermissionPolicyLayer`. No second policy definition, rule, Decision, or Grant model was introduced. |
| Immutable policy versions | A record owns one exact policy ID/version/layer. Repository updates compare the complete definition and identity, while SQLite guards reject definition or identity changes and prohibit physical deletion. Publishing N+1 inserts a distinct record and cannot rewrite N. |
| Explicit operational lifecycle | Policy records use `Draft -> Published -> Retired`; bindings use `Draft -> Active -> Ended`. Every transition requires the caller's expected revision, advances by exactly one, and receives its final timestamp from an injected `TrustedClock`. |
| Explicit scope binding | Each binding freezes one record and exact policy version together with layer, scope kind/reference, optional opaque boundary, validity interval, provenance, lifecycle, and revision. Role, Capability, Membership, Team, and Organization references do not produce authority. |
| Transactional active uniqueness | In-memory replacement holds one write lock. SQLite ends the old binding and activates the replacement inside one transaction. A partial unique index permits at most one Active binding for an exact layer and selector, including a normalized optional boundary. |
| Deterministic policy selection | Callers provide scope evidence without choosing policy layers. The service resolves effective bindings across all layers, verifies exact Published records, and orders exact version references through an explicit stable precedence function, then policy ID and version. |
| Deny by default and fail closed | No applicable policy produces immutable `NoPolicy` evidence. Conflicting candidates, a retired referenced policy, and scope/boundary inconsistency produce typed `AmbiguousPolicy`, `RetiredPolicy`, or `OutOfScope` evidence with no selected versions. |
| Selection is not authorization | Selection never invokes the evaluator and never creates or changes an `AuthorizationDecision` or `PermissionGrant`. It records only the exact policy-version set that a separately governed evaluator may consume. |
| Audit integration | Draft creation, publication, binding creation/activation/replacement/end, retirement, accepted selection, denied selection, and rejected operations use the COD-029 append-only governance audit sink. Metadata is restricted to allowlisted lifecycle, revision, layer, scope, selection-count, and reason-code values. |
| Bounded management boundary | Serialize-only record, inspection, binding, and selection views are returned through the application service. Every list request is bounded; the presentation boundary has no direct SQLite access. |
| Validated construction and loading | Domain constructors, aggregate `validate()` methods, validated DTO deserialization, repository writes, canonical JSON reloads, and indexed-column comparisons all reject malformed or non-canonical persisted state. |

## Audit and state-commit ordering

Lifecycle acceptance is appended to the immutable governance stream before the mutable operational repository write. An audit failure therefore leaves no operational mutation. If the subsequent atomic repository write fails, the service appends a stable operation-rejection event and returns failure; the requested operational transition remains unapplied. Audit records contain no policy rules, credentials, Provider configuration, Memory content, Prompt content, model output, environment variables, or file content.

## Compatibility and unchanged boundaries

- Existing `PermissionGovernanceService` evaluation semantics are unchanged.
- Existing Permission Requests, Ceilings, Authorization Decisions, and Permission Grants are unchanged.
- Historical Decisions and Grants continue to retain their original exact policy-version references and have no mutation or deletion API.
- Existing legacy policy owner-reference syntax remains valid inside the operational envelope.
- Role Assignment, Capability evidence, Team Membership, and opaque Organization boundary references remain context only and cannot grant permission.
- Agent, Team, Runtime, Provider, Model, Execution, Memory, Workflow, Collaboration, and Controlled Execution Environment behavior is unchanged.
- No Runtime invocation, Provider or Model API call, tool execution, network execution, filesystem execution, Workflow scheduling, autonomous loop, implicit Allow, or automatic model selection/fallback was added.

## Persistence migration

- Schema version: `24 -> 25`.
- Added `agent_os_permission_policy_records`.
- Added `agent_os_permission_policy_scope_bindings`.
- Added `agent_os_permission_policy_selection_evidence`.
- The migration is additive and idempotent for fresh-database creation and v24 upgrades.
- SQL guards enforce exact JSON/index-column agreement, legal lifecycle steps, `revision + 1`, immutable policy definitions and historical timestamps, Published-only activation, active-selector uniqueness, retirement refusal while active, append-only selection evidence, and no physical deletion of policy or binding history.
- SQLite adapters canonically reserialize all loaded aggregates and fail closed on unknown or discarded nested fields.

## Test coverage

- Domain IDs, aggregate construction, and persisted DTOs validate during deserialization.
- Draft, Published, Retired, Active, and Ended states enforce exact revisions and time order.
- Legacy owner references remain compatible.
- Published definitions and policy-version identities cannot change.
- Stale expected revisions leave current state unchanged and emit rejection audit evidence.
- Active binding uniqueness and explicit version replacement are transactional.
- Exact selection discovers all applicable layers without accepting caller-selected layers.
- No-policy, ambiguous, retired, and out-of-scope paths return immutable deny evidence.
- Audit failure prevents operational persistence.
- Historical Authorization Decision and Permission Grant values remain byte-for-byte equivalent across policy operations.
- SQLite recreation restores validated records; tampered indexed columns and non-canonical nested policy JSON fail on load.
- SQLite selection evidence rejects update and delete.
- v24-to-v25 migration verifies table creation and mutation guards.

## Validation

| Check | Result |
| --- | --- |
| COD-030 Domain, Repository, Service, Audit, and migration tests | Passed: 22 tests; 0 failed. |
| Existing Permission governance tests | Passed: 4 tests; 0 failed. |
| Existing governed admission tests | Passed: 2 tests; 0 failed. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | Passed. |
| `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` | Passed with no warnings. |
| `cargo test --manifest-path src-tauri/Cargo.toml --all-targets --quiet` | Passed: 2,791 tests; 5 ignored; 0 failed. |
| Database migration and direct guard checks | Passed. |
| Architecture forbidden-boundary scan | Passed; production policy-operations modules contain no authority-producing Role, Capability, Membership, Team, Organization, Runtime, Provider, Model, Execution, Memory, Workflow, Decision, or Grant dependency. |
| `git diff --check` | Passed. |
| Independent COD-030 release-blocker review | Approved; no P1 release blocker found. |
