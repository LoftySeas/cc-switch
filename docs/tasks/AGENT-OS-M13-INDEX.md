# Agent OS Milestone 13 Task Index

- **Milestone:** M13 Enterprise Governance
- **Status:** Completed (2026-08-20)
- **Source architecture:** [Agent OS Enterprise Governance Architecture v1](../architecture/agent-os-enterprise-governance-v1.md)
- **Baseline:** `main@b94c3b38786c04212112ced6dafc0a6c39b3581e`
- **Evidence:** [Milestone 13 Enterprise Governance Evidence](../reviews/milestone-013-enterprise-governance-evidence.md)

## Objective

Complete the enterprise governance control plane around the existing Agent OS without enabling real Runtime, Provider, Model, tool, network, filesystem, Workflow scheduler, or autonomous invocation.

## Required Order

### 1. COD-029 Audit Evidence and Execution Readiness Hardening

[Task specification](COD-029-audit-evidence-and-execution-readiness.md)

Closes the known evidence gaps at the M12 boundary:

- freezes Runtime and Provider activation revisions and adapter identities
- enforces trusted-time ordering
- validates persisted/deserialized Controlled Execution Environments
- adds append-only, tamper-evident governance audit streams
- introduces stale-environment revalidation without invoking anything

### 2. COD-030 Permission Policy Operations

[Task specification](COD-030-permission-policy-operations.md)

Operationalizes the existing Permission domain:

- immutable policy versions
- Draft, Published, and Retired operational lifecycle
- explicit active-policy selection by scope and layer
- deny-by-default behavior
- audit evidence for policy publication and retirement

### 3. COD-031 Organization Governance Boundaries

[Task specification](COD-031-organization-governance-boundaries.md)

Introduces enterprise scoping without redefining Team:

- Organization identity and lifecycle
- Organization-to-Team and Organization-to-policy bindings
- cross-organization isolation
- organization-scoped management queries
- audit evidence for organization operations

## Shared Architecture Rules

All three tasks must preserve:

- Agent != Runtime
- Runtime != Provider
- Provider != Model
- Capability != Permission
- Role != Permission
- Team Membership != Permission
- Organization != Team
- Policy Definition != Authorization Decision
- Authorization Decision != Permission Grant
- Memory != Identity
- Memory != Execution History

## Implementation Rules

- Reuse existing domains and repositories; do not create duplicate Agent, Team, Capability, Role, Permission, Runtime, Provider, Model, Execution, Memory, Workflow, or Collaboration domains.
- All new persistence is additive and migration-safe.
- All mutable operational records use optimistic revisions.
- Historical policy versions, decisions, grants, audit events, and evidence snapshots are immutable.
- Domain validation must run on construction, persistence insertion, and persistence loading.
- Timestamps for final governance evidence come from an injected trusted clock.
- Audit details are bounded and sanitized; secrets and raw Memory content are forbidden.
- UI and Tauri commands consume application services/read models only.
- Real invocation remains prohibited through the entire milestone.

## Milestone Delivery

Codex should implement COD-029, COD-030, and COD-031 continuously in order. It may create separate commits for stable task boundaries, but each commit must keep `main` buildable and tested.

Before declaring M13 complete:

1. Update the roadmap and milestone plan to Completed.
2. Add `docs/reviews/milestone-013-enterprise-governance-evidence.md` with requirement-to-code traceability.
3. Run the full Rust, TypeScript, frontend, formatting, lint, production build, migration, documentation-link, and architecture-boundary suites.
4. Commit to `main`.
5. Push to GitHub.
6. Verify Local HEAD, `origin/main`, `git ls-remote`, and GitHub API all match.

## Completion Output

Return:

- task-by-task completion status
- architecture impact and preserved boundaries
- database schema changes and migrations
- changed files
- tests and builds executed
- commit hashes
- Remote Verified evidence
- any remaining blockers before a future real-execution milestone
