# COD-004 Agent Domain Model Design Evidence

- **Status:** Final
- **Date:** 2026-08-16
- **Reviewer:** Codex, acting as Staff Architect
- **Requested by:** Repository maintainer through COD-004
- **Scope:** COD-004 architecture artifacts relative to `LoftySeas/cc-switch` `main` at `2b98894f730ad043414aaf4b0968edcd83a5fba7`
- **Related:** [COD-004](../commands/COD-004-agent-domain-model-design.md), [Agent Domain Model](../architecture/agent-domain-model.md), [ADR-002](../architecture-decisions/ADR-002-agent-domain-composition.md)

## Evidence objective

Record the source, traceability, scope, and validation evidence for the COD-004
Agent Domain Model Design. This report demonstrates that the proposed design
addresses the command's success criteria and remains within its documentation-only
constraints. It does not approve the architecture; human disposition remains
required.

## Repository context alignment

| Field | Verified value |
| --- | --- |
| Repository root | `/Users/shengyou/Documents/CCSwitch-Agent-API/cc-switch-source` |
| Repository identity | `LoftySeas/cc-switch` |
| Canonical remote | `origin`, `https://github.com/LoftySeas/cc-switch.git` |
| Upstream reference | `upstream`, `https://github.com/farion1231/cc-switch.git`, push disabled |
| Verified baseline | `origin/main` at `2b98894f730ad043414aaf4b0968edcd83a5fba7` |
| Verification method | Authorized `git fetch origin main` followed by `git ls-remote --heads origin refs/heads/main` |
| Task source | `docs/commands/COD-004-agent-domain-model-design.md` at the verified baseline |
| Task branch | `agent/codex/cod-004-agent-domain-model-design` |
| Initial workspace | Clean before branch creation and document changes |
| Source of truth | Verified GitHub `origin/main`; task branch is the proposed delta; local workspace is the execution overlay |
| Architecture commit | `d12dbcbdb26963bc6ea0385759ec7d1a61d95b27`, `docs(agent-os): define agent domain model` |

## Inputs examined

The following tracked documents were read from the verified baseline before
design execution:

- `AGENTS.md`;
- `CONTEXT.md`;
- `docs/commands/COD-004-agent-domain-model-design.md`;
- `docs/agent-os-blueprint.md`;
- `docs/architecture/current-state.md`;
- `docs/architecture/agent-runtime-boundary.md`;
- `docs/architecture-decisions/ADR-001-agent-runtime-boundary.md`;
- `docs/architecture-decisions/0000-template.md`;
- `docs/repository-governance.md`;
- `docs/development/agent-operation-guidelines.md`;
- `docs/development/agent-evidence-protocol.md`; and
- architecture, ADR, and review directory indexes and templates.

The design reuses observed facts from COD-002 rather than making new runtime
implementation claims. In particular, COD-002 identifies Provider as the current
core entity, a closed `AppType`/`AppId` runtime classification, reusable provider
and proxy services, and the absence of first-class Agent OS organization entities.

## Decision traceability

| Source requirement or evidence | COD-004 design response |
| --- | --- |
| Blueprint: Role separated from Model | Role is a contextual assignment and cannot select Agent, Runtime, Provider, or Model by identity |
| Blueprint: workflow-first | Team graph relationships describe eligibility and collaboration; only an explicit workflow or bounded task creates execution intent |
| Blueprint: cost-aware and human-controlled | Cost constrains eligible bindings after Capability and Permission checks; human policy and approvals bound every execution Grant |
| COD-002: Provider is not Agent | Agent receives a dedicated stable identity and only references eligible Provider bindings |
| COD-002: preserve mature current behavior | Migration is additive; Provider, proxy, native configuration, sessions, usage, profiles, and IPC remain unchanged |
| ADR-001: capability-based runtime boundary | Agent Organization resolves one immutable Runtime binding and passes a bounded request to `AgentRuntime` |
| ADR-001: Capability is not Permission | Separate Capability Definition/Requirement/Evidence/Effective Set and Permission Policy/Ceiling/Request/Grant stages |
| ADR-001: Agent != Role != Model != Runtime | ADR-002 extends the invariant to Provider and makes all relationships explicit |

## Success-criteria coverage

| COD-004 success criterion | Evidence location | Result |
| --- | --- | --- |
| Agent identity model | Agent concept, lifecycle, Agent Profile alias rule, immutable history | Covered |
| Runtime relationship model | Runtime eligibility and one resolved Runtime per execution attempt | Covered |
| Provider and Model separation | Independent Provider, Model, Model Availability, and Model Binding Policy concepts | Covered |
| Role assignment model | Role definition plus scoped Role Assignment through Team Membership | Covered |
| Capability representation | Definition, Requirement, Evidence, and Effective Capability Set | Covered |
| Permission boundary model | Policy, ceiling, request, grant, intersection, and deny-by-default rules | Covered |
| Team Membership relationship | Team and Membership identity, lifecycle, Role Assignment, and directed Team Relationship | Covered |
| Extension strategy | Dedicated Team Graph, Workflow Engine, Context Management, catalog, and organizational-scale sections | Covered |
| Foundation for MVP planning | Compatibility posture, resolution flow, validation gates, and deferred MVP sequencing | Covered |

## Constraint evidence

- No Rust, TypeScript, React, database schema, migration, dependency, build
  configuration, or product behavior is modified by COD-004.
- The architecture documents are language-neutral and contain no concrete storage
  or IPC schema.
- Runtime adapters remain independent from Role, Team, workflow, cost-routing, and
  Permission-grant decisions.
- Existing information is preserved; the task adds proposed artifacts and updates
  indexes without deleting planning content.
- ADR-002 remains `Proposed`; generated architecture is not represented as human
  approved.

## Validation performed

| Check | Result | Evidence |
| --- | --- | --- |
| Required source documents present | Pass | Verified all documents named by COD-004 plus AGENTS, ADR-001, governance, and evidence protocol at baseline `2b98894f` |
| COD-004 criteria coverage | Pass | Traceability and success-criteria tables cover all eight requested design outcomes |
| Markdown relative links | Pass | Node-based repository-local check resolved every relative Markdown link across all seven changed Markdown files |
| Product-code exclusion | Pass | Changed paths are limited to `docs/architecture*` and `docs/reviews`; no `src/` or `src-tauri/` path changed |
| Whitespace and patch integrity | Pass | `git diff --check` and `git diff --cached --check` returned no errors |
| Final architecture commit scope | Pass | Commit `d12dbcbdb26963bc6ea0385759ec7d1a61d95b27` contains five architecture and index files, 913 insertions, and 11 index-line deletions |
| Remote delivery | Reported separately | Push and independent remote hash verification occur after this evidence artifact is committed, avoiding a circular self-hash |

## Findings

### Finding 1: Architecture approval is still required

- **Severity:** Informational
- **Evidence:** The Agent Domain Model and ADR-002 both declare `Proposed`; ADR-001
  is also `Proposed` at the COD-004 baseline.
- **Impact:** The artifacts guide architecture review but do not authorize product
  implementation.
- **Recommendation:** The repository maintainer should approve, request changes,
  or reject ADR-001 and ADR-002 before schema or feature implementation begins.
- **Owner:** Architecture owner and repository maintainer
- **Disposition:** Open

### Finding 2: Resolution semantics remain a follow-up decision

- **Severity:** Informational
- **Evidence:** The domain model defines inputs, precedence boundaries, and
  fail-closed invariants but deliberately defers policy-language and serialization
  choices.
- **Impact:** Workflow and MVP implementation cannot yet rely on a concrete
  eligibility or Permission evaluation contract.
- **Recommendation:** Define a language-neutral, explainable resolution contract
  as part of Workflow Engine design or a dedicated ADR.
- **Owner:** Staff Architect
- **Disposition:** Deferred

## Limitations

- This is documentation and static architecture analysis; no external AI runtime
  was launched and no user configuration was read or modified.
- No database, IPC, Rust, React, or policy-engine representation was evaluated
  because COD-004 explicitly excludes implementation design.
- Acceptance remains a human architecture decision; agent-generated evidence
  establishes traceability and delivery, not approval.

## Recommendation

The validated COD-004 artifacts are suitable for human architecture review.
Product implementation should wait for architecture disposition and a subsequent
Workflow Engine or MVP planning command.

## Human disposition

- **Decision:** Pending
- **Approver:** Architecture owner and repository maintainer
- **Date:** Pending
- **Notes:** Review ADR-001 and ADR-002 together because the domain model depends
  on the runtime identity and permission boundary.
